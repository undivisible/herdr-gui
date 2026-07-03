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
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("herdr api: {0}")]
    Api(String),
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct HerdrState {
    #[serde(default)]
    pub focused_workspace_id: Option<String>,
    #[serde(default)]
    pub focused_tab_id: Option<String>,
    #[serde(default)]
    pub focused_pane_id: Option<String>,
    pub workspaces: Vec<Workspace>,
    pub tabs: Vec<Tab>,
    pub panes: Vec<Pane>,
    pub agents: Vec<Agent>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Workspace {
    pub workspace_id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub agent_status: Option<String>,
    #[serde(default)]
    pub active_tab_id: Option<String>,
    #[serde(default)]
    pub focused: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Tab {
    pub tab_id: String,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub terminal_title: Option<String>,
    #[serde(default)]
    pub agent_status: Option<String>,
    #[serde(default)]
    pub focused: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Pane {
    pub pane_id: String,
    #[serde(default)]
    pub terminal_id: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub tab_id: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub terminal_title: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub agent_status: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub focused: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Agent {
    pub terminal_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub display_agent: Option<String>,
    #[serde(default)]
    pub agent_status: Option<String>,
    #[serde(default)]
    pub custom_status: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub tab_id: Option<String>,
    #[serde(default)]
    pub pane_id: Option<String>,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub foreground_cwd: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WorkspaceList {
    workspaces: Vec<Workspace>,
}

#[derive(Debug, Deserialize)]
struct TabList {
    tabs: Vec<Tab>,
}

#[derive(Debug, Deserialize)]
struct PaneList {
    panes: Vec<Pane>,
}

#[derive(Debug, Deserialize)]
struct SessionSnapshotResult {
    snapshot: SessionSnapshot,
}

#[derive(Debug, Deserialize)]
struct SessionSnapshot {
    #[serde(default)]
    focused_workspace_id: Option<String>,
    #[serde(default)]
    focused_tab_id: Option<String>,
    #[serde(default)]
    focused_pane_id: Option<String>,
    workspaces: Vec<Workspace>,
    tabs: Vec<Tab>,
    panes: Vec<Pane>,
    #[serde(default)]
    agents: Vec<Agent>,
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
        if !socket_path.exists()
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

    pub fn state(&self) -> Result<HerdrState, HerdrError> {
        match self.snapshot_state() {
            Ok(state) => Ok(state),
            Err(_) => self.list_state(),
        }
    }

    fn snapshot_state(&self) -> Result<HerdrState, HerdrError> {
        let result: SessionSnapshotResult = self.call("session.snapshot", json!({}))?;
        Ok(result.snapshot.into())
    }

    fn list_state(&self) -> Result<HerdrState, HerdrError> {
        let workspaces: WorkspaceList = self.call("workspace.list", json!({}))?;
        let focused_workspace_id = workspaces
            .workspaces
            .iter()
            .find(|workspace| workspace.focused)
            .map(|workspace| workspace.workspace_id.clone())
            .or_else(|| {
                workspaces
                    .workspaces
                    .first()
                    .map(|workspace| workspace.workspace_id.clone())
            });
        let tabs: TabList = self.call(
            "tab.list",
            focused_workspace_id
                .as_ref()
                .map(|workspace_id| json!({ "workspace_id": workspace_id }))
                .unwrap_or_else(|| json!({})),
        )?;
        let focused_tab_id = tabs
            .tabs
            .iter()
            .find(|tab| tab.focused)
            .map(|tab| tab.tab_id.clone())
            .or_else(|| tabs.tabs.first().map(|tab| tab.tab_id.clone()));
        let panes: PaneList = self.call(
            "pane.list",
            focused_workspace_id
                .as_ref()
                .map(|workspace_id| json!({ "workspace_id": workspace_id }))
                .unwrap_or_else(|| json!({})),
        )?;
        let focused_pane_id = panes
            .panes
            .iter()
            .find(|pane| pane.focused)
            .map(|pane| pane.pane_id.clone())
            .or_else(|| panes.panes.first().map(|pane| pane.pane_id.clone()));
        Ok(HerdrState {
            focused_workspace_id,
            focused_tab_id,
            focused_pane_id,
            workspaces: workspaces.workspaces,
            tabs: tabs.tabs,
            panes: panes.panes,
            agents: Vec::new(),
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

    pub fn focus_workspace(&self, workspace_id: &str) -> Result<(), HerdrError> {
        let _: Value = self.call("workspace.focus", json!({ "workspace_id": workspace_id }))?;
        Ok(())
    }

    pub fn focus_tab(&self, tab_id: &str) -> Result<(), HerdrError> {
        let _: Value = self.call("tab.focus", json!({ "tab_id": tab_id }))?;
        Ok(())
    }

    pub fn create_tab(&self, workspace_id: Option<&str>) -> Result<(), HerdrError> {
        let _: Value = self.call(
            "tab.create",
            json!({ "workspace_id": workspace_id, "focus": true }),
        )?;
        Ok(())
    }

    pub fn close_tab(&self, tab_id: &str) -> Result<(), HerdrError> {
        let _: Value = self.call("tab.close", json!({ "tab_id": tab_id }))?;
        Ok(())
    }

    pub fn create_workspace(&self) -> Result<(), HerdrError> {
        let _: Value = self.call("workspace.create", json!({ "focus": true }))?;
        Ok(())
    }

    pub fn focus_pane(&self, pane_id: &str) -> Result<(), HerdrError> {
        let _: Value = self.call("pane.focus", json!({ "pane_id": pane_id }))?;
        Ok(())
    }

    pub fn send_key(&self, pane_id: &str, key: &str) -> Result<(), HerdrError> {
        let _: Value = self.call("pane.send_keys", json!({ "pane_id": pane_id, "keys": key }))?;
        Ok(())
    }

    pub fn send_text(&self, pane_id: &str, text: &str) -> Result<(), HerdrError> {
        let _: Value = self.call(
            "pane.send_text",
            json!({ "pane_id": pane_id, "text": text }),
        )?;
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

    pub fn reload_config(&self) -> Result<(), HerdrError> {
        let _: Value = self.call("server.reload_config", json!({}))?;
        Ok(())
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

impl From<SessionSnapshot> for HerdrState {
    fn from(snapshot: SessionSnapshot) -> Self {
        Self {
            focused_workspace_id: snapshot.focused_workspace_id,
            focused_tab_id: snapshot.focused_tab_id,
            focused_pane_id: snapshot.focused_pane_id,
            workspaces: snapshot.workspaces,
            tabs: snapshot.tabs,
            panes: snapshot.panes,
            agents: snapshot.agents,
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
        if path.exists() {
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
    use super::{
        command_exists, socket_path, PaneList, SessionSnapshotResult, TabList, WorkspaceList,
    };

    #[test]
    fn socket_path_should_default_to_config_dir() {
        let path = socket_path();
        assert!(path.ends_with(".config/herdr/herdr.sock") || path.ends_with("herdr.sock"));
    }

    #[test]
    fn command_exists_should_find_shell() {
        assert!(command_exists("sh"));
    }

    #[test]
    fn list_responses_should_parse_herdr_socket_shape() {
        let workspaces: WorkspaceList = parse_json(
            r#"{"type":"workspace_list","workspaces":[{"workspace_id":"w1","label":"repo"}]}"#,
        );
        let tabs: TabList = parse_json(r#"{"type":"tab_list","tabs":[{"tab_id":"w1:t1"}]}"#);
        let panes: PaneList = parse_json(
            r#"{"type":"pane_list","panes":[{"pane_id":"w1:p1","terminal_id":"term_1","agent_status":"working"}]}"#,
        );

        assert_eq!(workspaces.workspaces[0].workspace_id, "w1");
        assert_eq!(tabs.tabs[0].tab_id, "w1:t1");
        assert_eq!(panes.panes[0].agent_status.as_deref(), Some("working"));
        assert_eq!(panes.panes[0].terminal_id.as_deref(), Some("term_1"));
    }

    #[test]
    fn session_snapshot_should_preserve_focused_window_context() {
        let snapshot: SessionSnapshotResult = parse_json(
            r#"{"type":"session_snapshot","snapshot":{"version":"0.6.0","protocol":8,"focused_workspace_id":"1","focused_tab_id":"1:2","focused_pane_id":"1:p3","workspaces":[{"workspace_id":"1","label":"repo","focused":true,"active_tab_id":"1:2"}],"tabs":[{"tab_id":"1:1","workspace_id":"1","label":"build","focused":false},{"tab_id":"1:2","workspace_id":"1","label":"shell","focused":true}],"panes":[{"pane_id":"1:p3","terminal_id":"term_3","workspace_id":"1","tab_id":"1:2","focused":true,"agent_status":"idle"}],"layouts":[],"agents":[{"terminal_id":"term_3","agent":"pi","agent_status":"working","workspace_id":"1","tab_id":"1:2","pane_id":"1:p3","focused":true,"revision":1}]}}"#,
        );
        let state = super::HerdrState::from(snapshot.snapshot);

        assert_eq!(state.focused_workspace_id.as_deref(), Some("1"));
        assert_eq!(state.focused_tab_id.as_deref(), Some("1:2"));
        assert_eq!(state.focused_pane_id.as_deref(), Some("1:p3"));
        assert_eq!(state.tabs[1].workspace_id.as_deref(), Some("1"));
        assert_eq!(state.panes[0].tab_id.as_deref(), Some("1:2"));
        assert_eq!(state.agents.len(), 1);
        assert_eq!(state.agents[0].agent.as_deref(), Some("pi"));
        assert_eq!(state.agents[0].pane_id.as_deref(), Some("1:p3"));
    }

    fn parse_json<T: serde::de::DeserializeOwned>(json: &str) -> T {
        match serde_json::from_str(json) {
            Ok(value) => value,
            Err(err) => panic!("{err}"),
        }
    }
}
