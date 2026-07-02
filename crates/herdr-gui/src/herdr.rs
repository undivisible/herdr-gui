use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    env,
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HerdrError {
    #[error("herdr is not installed and `wax install herdr` failed: {0}")]
    InstallFailed(String),
    #[error("herdr socket unavailable at {0}: {1}")]
    SocketUnavailable(String, String),
    #[error("herdr command failed: {0}")]
    CommandFailed(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("herdr api: {0}")]
    Api(String),
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Snapshot {
    pub workspaces: Vec<Workspace>,
    pub tabs: Vec<Tab>,
    pub panes: Vec<Pane>,
    pub pane_text: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Workspace {
    pub workspace_id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Tab {
    pub tab_id: String,
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Pane {
    pub pane_id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub agent_status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    result: Option<T>,
    error: Option<ApiError>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    message: Option<String>,
}

pub struct HerdrClient {
    socket_path: PathBuf,
}

impl HerdrClient {
    pub fn bootstrap() -> Result<Self, HerdrError> {
        ensure_herdr_installed()?;
        let socket_path = socket_path();
        if !socket_path.is_file()
            || (Self {
                socket_path: socket_path.clone(),
            })
            .ping()
            .is_err()
        {
            start_server()?;
            wait_for_socket(&socket_path)?;
        }
        let client = Self { socket_path };
        client.ping()?;
        Ok(client)
    }

    pub fn ping(&self) -> Result<(), HerdrError> {
        let _: Value = self.call("ping", json!({}))?;
        Ok(())
    }

    pub fn snapshot(&self) -> Result<Snapshot, HerdrError> {
        let workspaces: Vec<Workspace> = self.call("workspace.list", json!({}))?;
        let tabs: Vec<Tab> = self.call("tab.list", json!({}))?;
        let panes: Vec<Pane> = self.call("pane.list", json!({}))?;
        let pane_text = panes
            .first()
            .map(|pane| self.read_pane(&pane.pane_id))
            .transpose()?
            .unwrap_or_default();
        Ok(Snapshot {
            workspaces,
            tabs,
            panes,
            pane_text,
        })
    }

    pub fn split_right(&self, pane_id: &str) -> Result<(), HerdrError> {
        let _: Value = self.call(
            "pane.split",
            json!({ "pane_id": pane_id, "direction": "right" }),
        )?;
        Ok(())
    }

    pub fn split_down(&self, pane_id: &str) -> Result<(), HerdrError> {
        let _: Value = self.call(
            "pane.split",
            json!({ "pane_id": pane_id, "direction": "down" }),
        )?;
        Ok(())
    }

    pub fn close_pane(&self, pane_id: &str) -> Result<(), HerdrError> {
        let _: Value = self.call("pane.close", json!({ "pane_id": pane_id }))?;
        Ok(())
    }

    pub fn send_text(&self, pane_id: &str, text: &str) -> Result<(), HerdrError> {
        let _: Value = self.call(
            "pane.send_text",
            json!({ "pane_id": pane_id, "text": text }),
        )?;
        Ok(())
    }

    pub fn send_key(&self, pane_id: &str, key: &str) -> Result<(), HerdrError> {
        let _: Value = self.call("pane.send_keys", json!({ "pane_id": pane_id, "keys": key }))?;
        Ok(())
    }

    pub fn resize_right(&self, pane_id: &str) -> Result<(), HerdrError> {
        let _: Value = self.call(
            "pane.resize",
            json!({ "pane_id": pane_id, "direction": "right", "amount": 0.05 }),
        )?;
        Ok(())
    }

    pub fn focus_right(&self) -> Result<(), HerdrError> {
        let _: Value = self.call("pane.focus_direction", json!({ "direction": "right" }))?;
        Ok(())
    }

    fn read_pane(&self, pane_id: &str) -> Result<String, HerdrError> {
        let output = Command::new("herdr")
            .args([
                "pane", "read", pane_id, "--source", "recent", "--lines", "80",
            ])
            .output()?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(HerdrError::CommandFailed(
                String::from_utf8_lossy(&output.stderr).into_owned(),
            ))
        }
    }

    fn call<T: DeserializeOwned>(&self, method: &str, params: Value) -> Result<T, HerdrError> {
        let mut stream = UnixStream::connect(&self.socket_path).map_err(|err| {
            HerdrError::SocketUnavailable(self.socket_path.display().to_string(), err.to_string())
        })?;
        let request = json!({ "id": "herdr-gui", "method": method, "params": params });
        writeln!(stream, "{request}")?;
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line)?;
        let response: ApiResponse<T> = serde_json::from_str(&line)?;
        if let Some(error) = response.error {
            Err(HerdrError::Api(
                error.message.unwrap_or_else(|| "unknown error".to_string()),
            ))
        } else if let Some(result) = response.result {
            Ok(result)
        } else {
            Err(HerdrError::Api("missing result".to_string()))
        }
    }
}

fn ensure_herdr_installed() -> Result<(), HerdrError> {
    if command_exists("herdr") {
        return Ok(());
    }
    let output = Command::new("wax").args(["install", "herdr"]).output()?;
    if output.status.success() && command_exists("herdr") {
        Ok(())
    } else {
        Err(HerdrError::InstallFailed(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ))
    }
}

fn command_exists(command: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {command}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn start_server() -> Result<(), HerdrError> {
    Command::new("herdr")
        .arg("server")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

fn wait_for_socket(path: &Path) -> Result<(), HerdrError> {
    for _ in 0..30 {
        if path.is_file() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(HerdrError::SocketUnavailable(
        path.display().to_string(),
        "timed out waiting for herdr server".to_string(),
    ))
}

fn socket_path() -> PathBuf {
    if let Some(path) = env::var_os("HERDR_SOCKET_PATH") {
        return PathBuf::from(path);
    }
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    if let Some(session) = env::var_os("HERDR_SESSION") {
        return home
            .join(".config/herdr/sessions")
            .join(session)
            .join("herdr.sock");
    }
    home.join(".config/herdr/herdr.sock")
}

#[cfg(test)]
mod tests {
    use super::{command_exists, socket_path};

    #[test]
    fn socket_path_should_default_to_config_dir() {
        let path = socket_path();
        assert!(path.ends_with(".config/herdr/herdr.sock") || path.ends_with("herdr.sock"));
    }

    #[test]
    fn command_exists_should_find_shell() {
        assert!(command_exists("sh"));
    }
}
