use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    env,
    io::{BufRead, BufReader, Read, Write},
    os::unix::{fs::MetadataExt, fs::OpenOptionsExt, net::UnixStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
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
    /// The pane reported focused by exactly one entry in the latest pane list.
    /// None means focus was absent or ambiguous; it must not authorize input.
    #[serde(default)]
    pub focused_pane_id: Option<String>,
    pub workspaces: Vec<Workspace>,
    pub tabs: Vec<Tab>,
    pub panes: Vec<Pane>,
    pub agents: Vec<Agent>,
    #[serde(skip)]
    pub agent_list_error: Option<String>,
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

#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize)]
pub struct PaneLayout {
    pub workspace_id: String,
    pub tab_id: String,
    pub zoomed: bool,
    pub area: PaneLayoutRect,
    pub focused_pane_id: String,
    pub panes: Vec<PaneLayoutPane>,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
pub struct PaneLayoutRect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize)]
pub struct PaneLayoutPane {
    pub pane_id: String,
    pub focused: bool,
    pub rect: PaneLayoutRect,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
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

impl Agent {
    #[allow(dead_code)]
    pub fn from_pane(pane: &Pane) -> Self {
        Self {
            terminal_id: pane.terminal_id.clone().unwrap_or_default(),
            agent: pane.agent.clone(),
            name: pane.agent.clone(),
            title: pane.title.clone(),
            display_agent: pane.agent.clone(),
            agent_status: pane.agent_status.clone(),
            workspace_id: pane.workspace_id.clone(),
            tab_id: pane.tab_id.clone(),
            pane_id: Some(pane.pane_id.clone()),
            focused: pane.focused,
            cwd: pane.cwd.clone(),
            ..Default::default()
        }
    }
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
struct PaneLayoutResponse {
    layout: PaneLayout,
}

#[derive(Debug, Deserialize)]
struct AgentList {
    agents: Vec<Agent>,
}

#[derive(Debug, Deserialize)]
struct PaneReadResponse {
    read: PaneRead,
}

#[derive(Debug, Deserialize)]
struct PaneRead {
    text: String,
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

#[derive(Clone)]
pub struct HerdrClient {
    socket_path: PathBuf,
}

impl HerdrClient {
    pub fn bootstrap() -> Result<Self, HerdrError> {
        if let Err(err) = clear_legacy_request_log(Path::new("/tmp/herdr-gui-lag.log")) {
            eprintln!("failed to clear legacy Herdr GUI request log: {err}");
        }
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
        let tabs: TabList = self.call("tab.list", json!({}))?;
        let focused_tab_id = tabs
            .tabs
            .iter()
            .find(|tab| tab.focused)
            .map(|tab| tab.tab_id.clone())
            .or_else(|| tabs.tabs.first().map(|tab| tab.tab_id.clone()));
        let panes: PaneList = self.call("pane.list", json!({}))?;
        let mut focused_panes = panes.panes.iter().filter(|pane| pane.focused);
        let focused_pane_id = match (focused_panes.next(), focused_panes.next()) {
            (Some(pane), None) => Some(pane.pane_id.clone()),
            _ => None,
        };
        let (agents, agent_list_error) = match self.call::<AgentList>("agent.list", json!({})) {
            Ok(list) => (list.agents, None),
            Err(_) => (Vec::new(), Some("Failed to load agents".to_string())),
        };
        Ok(HerdrState {
            focused_workspace_id,
            focused_tab_id,
            focused_pane_id,
            workspaces: workspaces.workspaces,
            tabs: tabs.tabs,
            panes: panes.panes,
            agents,
            agent_list_error,
        })
    }

    pub fn split_right(&self, pane_id: &str) -> Result<(), HerdrError> {
        let _: Value = self.call(
            "pane.split",
            json!({ "pane_id": pane_id, "direction": "right" }),
        )?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn pane_layout(&self, pane_id: &str) -> Result<PaneLayout, HerdrError> {
        let response: PaneLayoutResponse =
            self.call("pane.layout", json!({ "pane_id": pane_id }))?;
        Ok(response.layout)
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

    pub fn read_pane_ansi(&self, pane_id: &str) -> Result<String, HerdrError> {
        let response: PaneReadResponse = self.call(
            "pane.read",
            json!({
                "pane_id": pane_id,
                "source": "visible",
                "format": "ansi",
                "strip_ansi": false
            }),
        )?;
        Ok(response.read.text)
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

    pub fn close_workspace(&self, workspace_id: &str) -> Result<(), HerdrError> {
        let _: Value = self.call("workspace.close", json!({ "workspace_id": workspace_id }))?;
        Ok(())
    }

    pub fn focus_pane(&self, pane_id: &str) -> Result<(), HerdrError> {
        let _: Value = self.call("pane.focus", json!({ "pane_id": pane_id }))?;
        Ok(())
    }

    pub fn send_key(&self, pane_id: &str, key: &str) -> Result<(), HerdrError> {
        let _: Value = self.call(
            "pane.send_keys",
            json!({ "pane_id": pane_id, "keys": [key] }),
        )?;
        Ok(())
    }

    pub fn send_text(&self, pane_id: &str, text: &str) -> Result<(), HerdrError> {
        let _: Value = self.call(
            "pane.send_text",
            json!({ "pane_id": pane_id, "text": text }),
        )?;
        Ok(())
    }

    pub fn paste_text(&self, pane_id: &str, text: &str) -> Result<(), HerdrError> {
        let _: Value = self.call(
            "pane.send_input",
            json!({ "pane_id": pane_id, "text": text, "keys": [] }),
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
        let started = Instant::now();
        let mut stream = UnixStream::connect(&self.socket_path).map_err(|err| {
            HerdrError::SocketUnavailable(self.socket_path.display().to_string(), err.to_string())
        })?;
        stream.set_read_timeout(Some(Duration::from_millis(500)))?;
        stream.set_write_timeout(Some(Duration::from_millis(500)))?;
        let request = json!({ "id": "herdr-gui", "method": method, "params": params });
        writeln!(stream, "{request}")?;
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line)?;
        let response: ApiResponse<T> = serde_json::from_str(&line)?;
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        let bytes = line.len();
        lag_log(format_args!(
            "herdr.call {method} {ms:.2}ms resp_bytes={bytes}"
        ));
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

fn clear_legacy_request_log(path: &Path) -> std::io::Result<()> {
    let mut file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.uid() != unsafe { libc::geteuid() } || metadata.nlink() != 1
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "legacy log is not a singly linked file owned by the current user",
        ));
    }
    let mut contents = Vec::new();
    file.read_to_end(&mut contents)?;
    if contents.split(|byte| *byte == b'\n').any(|line| {
        line.windows(11).any(|part| part == b"herdr.call ")
            && line.windows(8).any(|part| part == b" params=")
    }) {
        file.set_len(0)?;
    }
    Ok(())
}

fn lag_log(args: std::fmt::Arguments<'_>) {
    use std::io::Write;
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let line = format!("[{ts:.3}] {args}");
    eprintln!("{line}");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/herdr-gui-lag.log")
    {
        let _ = writeln!(file, "{line}");
        let _ = file.flush();
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
    let client = HerdrClient {
        socket_path: path.to_path_buf(),
    };
    for _ in 0..30 {
        if client.ping().is_ok() {
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
        clear_legacy_request_log, command_exists, socket_path, wait_for_socket, AgentList,
        HerdrClient, HerdrError, PaneList, TabList, WorkspaceList,
    };
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    use std::time::Duration;

    #[test]
    fn pane_layout_reads_rectangles_zoom_and_reports_unsupported_method() {
        let dir = std::env::temp_dir().join(format!("herdr-layout-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap_or_else(|err| panic!("{err}"));
        let path = dir.join("socket");
        let listener = UnixListener::bind(&path).unwrap_or_else(|err| panic!("{err}"));
        let server = std::thread::spawn(move || {
            for reply in [
                r#"{"id":"herdr-gui","result":{"type":"pane_layout","layout":{"workspace_id":"w1","tab_id":"w1:t1","zoomed":false,"area":{"x":2,"y":3,"width":100,"height":40},"focused_pane_id":"p2","panes":[{"pane_id":"p1","focused":false,"rect":{"x":2,"y":3,"width":29,"height":40}},{"pane_id":"p2","focused":true,"rect":{"x":31,"y":3,"width":71,"height":40}}],"splits":[{"id":"s1","direction":"right","ratio":0.29,"rect":{"x":2,"y":3,"width":100,"height":40}}]}}}"#,
                r#"{"id":"herdr-gui","result":{"type":"pane_layout","layout":{"workspace_id":"w1","tab_id":"w1:t1","zoomed":true,"area":{"x":2,"y":3,"width":100,"height":40},"focused_pane_id":"p2","panes":[{"pane_id":"p1","focused":false,"rect":{"x":2,"y":3,"width":29,"height":40}},{"pane_id":"p2","focused":true,"rect":{"x":31,"y":3,"width":71,"height":40}}],"splits":[]}}}"#,
                r#"{"id":"herdr-gui","error":{"code":"unknown_method","message":"unknown method"}}"#,
            ] {
                let (mut stream, _) = listener.accept().unwrap_or_else(|err| panic!("{err}"));
                let mut line = String::new();
                BufReader::new(&stream)
                    .read_line(&mut line)
                    .unwrap_or_else(|err| panic!("{err}"));
                let request: serde_json::Value =
                    serde_json::from_str(&line).unwrap_or_else(|err| panic!("{err}"));
                assert_eq!(request["method"], "pane.layout");
                assert_eq!(request["params"], serde_json::json!({"pane_id":"p2"}));
                writeln!(stream, "{reply}").unwrap_or_else(|err| panic!("{err}"));
            }
        });
        let client = HerdrClient { socket_path: path };
        let layout = client
            .pane_layout("p2")
            .unwrap_or_else(|err| panic!("{err}"));
        assert!(!layout.zoomed);
        assert_eq!(layout.area.width, 100);
        assert_eq!(layout.panes.len(), 2);
        assert_eq!(layout.panes[0].rect.width, 29);
        assert_eq!(layout.panes[1].rect.x, 31);
        assert_eq!(layout.panes[1].rect.width, 71);
        assert_eq!(layout.focused_pane_id, "p2");
        let zoomed = client
            .pane_layout("p2")
            .unwrap_or_else(|err| panic!("{err}"));
        assert!(zoomed.zoomed);
        assert_eq!(zoomed.panes.len(), 2);
        assert_eq!(zoomed.panes[1].rect.width, 71);
        assert!(
            matches!(client.pane_layout("p2"), Err(HerdrError::Api(message)) if message == "unknown method")
        );
        server.join().unwrap_or_else(|err| panic!("{err:?}"));
        std::fs::remove_dir_all(dir).unwrap_or_else(|err| panic!("{err}"));
    }

    #[test]
    fn state_preserves_other_data_when_agent_list_fails() {
        for (suffix, response, failed) in [
            ("error", r#"{"error":{"message":"sensitive detail"}}"#, true),
            ("empty", r#"{"result":{"agents":[]}}"#, false),
            (
                "agents",
                r#"{"result":{"agents":[{"terminal_id":"term1"}]}}"#,
                false,
            ),
        ] {
            let dir =
                std::env::temp_dir().join(format!("herdr-state-{}-{suffix}", std::process::id()));
            std::fs::create_dir(&dir).unwrap_or_else(|err| panic!("{err}"));
            let path = dir.join("socket");
            let listener = UnixListener::bind(&path).unwrap_or_else(|err| panic!("{err}"));
            let server = std::thread::spawn(move || {
                for (method, reply) in [
                    (
                        "workspace.list",
                        r#"{"result":{"workspaces":[{"workspace_id":"w1"}]}}"#,
                    ),
                    ("tab.list", r#"{"result":{"tabs":[{"tab_id":"t1"}]}}"#),
                    ("pane.list", r#"{"result":{"panes":[{"pane_id":"p1"}]}}"#),
                    ("agent.list", response),
                ] {
                    let (mut stream, _) = listener.accept().unwrap_or_else(|err| panic!("{err}"));
                    let mut line = String::new();
                    BufReader::new(&stream)
                        .read_line(&mut line)
                        .unwrap_or_else(|err| panic!("{err}"));
                    let request: serde_json::Value =
                        serde_json::from_str(&line).unwrap_or_else(|err| panic!("{err}"));
                    assert_eq!(request["method"], method);
                    writeln!(stream, "{reply}").unwrap_or_else(|err| panic!("{err}"));
                }
            });
            let state = HerdrClient { socket_path: path }
                .state()
                .unwrap_or_else(|err| panic!("{err}"));
            server.join().unwrap_or_else(|err| panic!("{err:?}"));
            std::fs::remove_dir_all(dir).unwrap_or_else(|err| panic!("{err}"));
            assert_eq!(state.focused_workspace_id.as_deref(), Some("w1"));
            assert_eq!(state.focused_tab_id.as_deref(), Some("t1"));
            assert_eq!(state.focused_pane_id, None);
            assert_eq!(state.workspaces.len(), 1);
            assert_eq!(state.tabs.len(), 1);
            assert_eq!(state.panes.len(), 1);
            assert_eq!(
                state.agent_list_error.as_deref(),
                failed.then_some("Failed to load agents")
            );
            assert_eq!(state.agents.len(), usize::from(suffix == "agents"));
            let serialized = serde_json::to_value(&state).unwrap_or_else(|err| panic!("{err}"));
            assert!(serialized.get("agent_list_error").is_none());
        }
    }

    #[test]
    fn state_only_reports_unambiguous_server_focused_pane() {
        for (suffix, panes, expected) in [
            (
                "none",
                r#"[{"pane_id":"p1"},{"pane_id":"p2","focused":false}]"#,
                None,
            ),
            (
                "one",
                r#"[{"pane_id":"p1"},{"pane_id":"p2","focused":true}]"#,
                Some("p2"),
            ),
            (
                "multiple",
                r#"[{"pane_id":"p1","focused":true},{"pane_id":"p2","focused":true}]"#,
                None,
            ),
        ] {
            let dir =
                std::env::temp_dir().join(format!("herdr-focus-{}-{suffix}", std::process::id()));
            std::fs::create_dir(&dir).unwrap_or_else(|err| panic!("{err}"));
            let path = dir.join("socket");
            let listener = UnixListener::bind(&path).unwrap_or_else(|err| panic!("{err}"));
            let server = std::thread::spawn(move || {
                for (method, reply) in [
                    (
                        "workspace.list",
                        r#"{"result":{"workspaces":[]}}"#.to_string(),
                    ),
                    ("tab.list", r#"{"result":{"tabs":[]}}"#.to_string()),
                    ("pane.list", format!(r#"{{"result":{{"panes":{panes}}}}}"#)),
                    ("agent.list", r#"{"result":{"agents":[]}}"#.to_string()),
                ] {
                    let (mut stream, _) = listener.accept().unwrap_or_else(|err| panic!("{err}"));
                    let mut line = String::new();
                    BufReader::new(&stream)
                        .read_line(&mut line)
                        .unwrap_or_else(|err| panic!("{err}"));
                    let request: serde_json::Value =
                        serde_json::from_str(&line).unwrap_or_else(|err| panic!("{err}"));
                    assert_eq!(request["method"], method);
                    writeln!(stream, "{reply}").unwrap_or_else(|err| panic!("{err}"));
                }
            });
            let state = HerdrClient { socket_path: path }
                .state()
                .unwrap_or_else(|err| panic!("{err}"));
            server.join().unwrap_or_else(|err| panic!("{err:?}"));
            std::fs::remove_dir_all(dir).unwrap_or_else(|err| panic!("{err}"));
            assert_eq!(state.panes.len(), 2);
            assert_eq!(state.focused_pane_id.as_deref(), expected);
        }
    }

    #[test]
    fn wait_for_socket_retries_stale_socket_until_ping_succeeds() {
        let dir = std::env::temp_dir().join(format!("herdr-ready-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap_or_else(|err| panic!("{err}"));
        let path = dir.join("socket");
        drop(UnixListener::bind(&path).unwrap_or_else(|err| panic!("{err}")));
        let server_path = path.clone();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(250));
            std::fs::remove_file(&server_path).unwrap_or_else(|err| panic!("{err}"));
            let listener = UnixListener::bind(&server_path).unwrap_or_else(|err| panic!("{err}"));
            let (mut stream, _) = listener.accept().unwrap_or_else(|err| panic!("{err}"));
            let mut line = String::new();
            BufReader::new(&stream)
                .read_line(&mut line)
                .unwrap_or_else(|err| panic!("{err}"));
            let request: serde_json::Value =
                serde_json::from_str(&line).unwrap_or_else(|err| panic!("{err}"));
            assert_eq!(request["method"], "ping");
            writeln!(stream, "{{\"result\":{{}}}}").unwrap_or_else(|err| panic!("{err}"));
            done_tx.send(()).unwrap_or_else(|err| panic!("{err}"));
        });
        let result = wait_for_socket(&path);
        assert!(result.is_ok(), "{result:?}");
        done_rx
            .recv_timeout(Duration::from_secs(4))
            .unwrap_or_else(|err| panic!("{err}"));
        server.join().unwrap_or_else(|err| panic!("{err:?}"));
        std::fs::remove_dir_all(dir).unwrap_or_else(|err| panic!("{err}"));
    }

    #[test]
    fn wait_for_socket_times_out_on_stale_socket() {
        let dir = std::env::temp_dir().join(format!("herdr-stale-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap_or_else(|err| panic!("{err}"));
        let path = dir.join("socket");
        drop(UnixListener::bind(&path).unwrap_or_else(|err| panic!("{err}")));
        let result = wait_for_socket(&path);
        std::fs::remove_dir_all(dir).unwrap_or_else(|err| panic!("{err}"));
        assert!(
            matches!(result, Err(HerdrError::SocketUnavailable(_, message)) if message == "timed out waiting for herdr server")
        );
    }

    #[test]
    fn call_times_out_when_server_does_not_respond() {
        let dir = std::env::temp_dir().join(format!("herdr-timeout-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap_or_else(|err| panic!("{err}"));
        let path = dir.join("socket");
        let listener = UnixListener::bind(&path).unwrap_or_else(|err| panic!("{err}"));
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap_or_else(|err| panic!("{err}"));
            let mut line = String::new();
            BufReader::new(&stream)
                .read_line(&mut line)
                .unwrap_or_else(|err| panic!("{err}"));
            std::thread::sleep(Duration::from_secs(2));
        });
        let result = HerdrClient { socket_path: path }.ping();
        server.join().unwrap_or_else(|err| panic!("{err:?}"));
        std::fs::remove_dir_all(dir).unwrap_or_else(|err| panic!("{err}"));
        assert!(
            matches!(result, Err(HerdrError::Io(err)) if matches!(err.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut))
        );
    }

    #[test]
    fn paste_text_sends_input_and_reports_server_error() {
        let dir = std::env::temp_dir().join(format!("herdr-paste-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap_or_else(|err| panic!("{err}"));
        let path = dir.join("socket");
        let listener = UnixListener::bind(&path).unwrap_or_else(|err| panic!("{err}"));
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap_or_else(|err| panic!("{err}"));
            let mut line = String::new();
            BufReader::new(&stream)
                .read_line(&mut line)
                .unwrap_or_else(|err| panic!("{err}"));
            let request: serde_json::Value =
                serde_json::from_str(&line).unwrap_or_else(|err| panic!("{err}"));
            writeln!(stream, "{{\"error\":{{\"message\":\"unknown method\"}}}}")
                .unwrap_or_else(|err| panic!("{err}"));
            request
        });
        let result = HerdrClient { socket_path: path }.paste_text("pane-1", "hello\nworld");
        let request = server.join().unwrap_or_else(|err| panic!("{err:?}"));
        std::fs::remove_dir_all(dir).unwrap_or_else(|err| panic!("{err}"));
        assert_eq!(request["method"], "pane.send_input");
        assert_eq!(
            request["params"],
            serde_json::json!({"pane_id": "pane-1", "text": "hello\nworld", "keys": []})
        );
        assert!(matches!(result, Err(HerdrError::Api(message)) if message == "unknown method"));
    }

    #[test]
    fn legacy_log_cleanup_removes_request_parameters_without_following_symlinks() {
        let dir = std::env::temp_dir().join(format!("herdr-gui-log-test-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap_or_else(|err| panic!("{err}"));
        let log = dir.join("log");
        let link = dir.join("link");
        let sensitive =
            b"[1] herdr.call pane.send_text 1ms resp_bytes=1 params={\"text\":\"secret\"}\n";
        std::fs::write(&log, sensitive).unwrap_or_else(|err| panic!("{err}"));
        std::os::unix::fs::symlink(&log, &link).unwrap_or_else(|err| panic!("{err}"));
        assert!(clear_legacy_request_log(&link).is_err());
        assert_eq!(
            std::fs::read(&log).ok().as_deref(),
            Some(sensitive.as_slice())
        );
        let hard_link = dir.join("hard-link");
        std::fs::hard_link(&log, &hard_link).unwrap_or_else(|err| panic!("{err}"));
        assert!(clear_legacy_request_log(&log).is_err());
        assert_eq!(
            std::fs::read(&hard_link).ok().as_deref(),
            Some(sensitive.as_slice())
        );
        std::fs::remove_file(&hard_link).unwrap_or_else(|err| panic!("{err}"));
        clear_legacy_request_log(&log).unwrap_or_else(|err| panic!("{err}"));
        assert_eq!(std::fs::read(&log).ok().as_deref(), Some(&[][..]));
        let unrelated = b"unrelated log contents\n";
        std::fs::write(&log, unrelated).unwrap_or_else(|err| panic!("{err}"));
        clear_legacy_request_log(&log).unwrap_or_else(|err| panic!("{err}"));
        assert_eq!(
            std::fs::read(&log).ok().as_deref(),
            Some(unrelated.as_slice())
        );
        std::fs::remove_dir_all(&dir).unwrap_or_else(|err| panic!("{err}"));
    }

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
    fn agent_list_should_parse_cross_workspace_agents() {
        let agents: AgentList = parse_json(
            r#"{"type":"agent_list","agents":[{"terminal_id":"term_1","agent":"pi","agent_status":"idle","workspace_id":"w1","tab_id":"w1:t1","pane_id":"w1:p1"},{"terminal_id":"term_2","agent":"devin","agent_status":"working","workspace_id":"w2","tab_id":"w2:t1","pane_id":"w2:p1"}]}"#,
        );

        assert_eq!(agents.agents.len(), 2);
        assert_eq!(agents.agents[0].workspace_id.as_deref(), Some("w1"));
        assert_eq!(agents.agents[1].agent.as_deref(), Some("devin"));
    }

    fn parse_json<T: serde::de::DeserializeOwned>(json: &str) -> T {
        match serde_json::from_str(json) {
            Ok(value) => value,
            Err(err) => panic!("{err}"),
        }
    }
}
