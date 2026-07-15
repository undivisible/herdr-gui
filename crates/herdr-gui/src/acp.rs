#![allow(dead_code)]

use agent_client_protocol::schema::v1::{
    ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, SelectedPermissionOutcome,
    SessionNotification, SessionUpdate, TextContent,
};
use agent_client_protocol::{AcpAgent, Agent, Client, ConnectionTo};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::mpsc;
use std::thread;

#[derive(Debug)]
pub enum AgentEvent {
    Text(String),
    ToolCall { title: String },
    Done,
    Error(String),
}

pub struct AcpSession {
    event_rx: mpsc::Receiver<AgentEvent>,
    prompt_tx: mpsc::Sender<String>,
    _handle: thread::JoinHandle<()>,
}

impl AcpSession {
    pub fn spawn(command: &str, cwd: PathBuf) -> Result<Self, String> {
        let (event_tx, event_rx) = mpsc::channel();
        let (prompt_tx, prompt_rx) = mpsc::channel::<String>();
        let command = command.to_string();

        let handle = thread::spawn(move || {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = event_tx.send(AgentEvent::Error(format!("tokio: {e}")));
                    return;
                }
            };

            rt.block_on(async move {
                if let Err(e) = run_agent(&command, cwd, &event_tx, &prompt_rx).await {
                    let _ = event_tx.send(AgentEvent::Error(e.to_string()));
                }
                let _ = event_tx.send(AgentEvent::Done);
            });
        });

        Ok(Self {
            event_rx,
            prompt_tx,
            _handle: handle,
        })
    }

    pub fn try_recv(&self) -> Option<AgentEvent> {
        self.event_rx.try_recv().ok()
    }

    pub fn send_prompt(&self, text: &str) -> Result<(), String> {
        self.prompt_tx
            .send(text.to_string())
            .map_err(|e| format!("channel closed: {e}"))
    }
}

async fn run_agent(
    command: &str,
    cwd: PathBuf,
    event_tx: &mpsc::Sender<AgentEvent>,
    prompt_rx: &mpsc::Receiver<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let agent = AcpAgent::from_str(command)?;
    let tx = event_tx.clone();

    Client
        .builder()
        .on_receive_notification(
            async move |notification: SessionNotification, _cx| {
                match notification.update {
                    SessionUpdate::AgentMessageChunk(chunk) => {
                        if let ContentBlock::Text(TextContent { text, .. }) = chunk.content {
                            let _ = tx.send(AgentEvent::Text(text));
                        }
                    }
                    SessionUpdate::ToolCallUpdate(update) => {
                        let _ = tx.send(AgentEvent::ToolCall {
                            title: update.fields.title.unwrap_or_default(),
                        });
                    }
                    _ => {}
                }
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |_request: RequestPermissionRequest, responder, _connection| {
                responder.respond(RequestPermissionResponse::new(
                    RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                        "approve".to_string(),
                    )),
                ))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, |connection: ConnectionTo<Agent>| async move {
            // Initialize
            connection
                .send_request(InitializeRequest::new(
                    agent_client_protocol::schema::ProtocolVersion::V1,
                ))
                .block_task()
                .await?;

            // Create session
            let session_response = connection
                .send_request(NewSessionRequest::new(cwd))
                .block_task()
                .await?;

            let session_id = session_response.session_id.clone();

            // Wait for prompts from the GUI
            while let Ok(prompt_text) = prompt_rx.recv() {
                let prompt_req = PromptRequest::new(
                    session_id.clone(),
                    vec![ContentBlock::Text(TextContent::new(prompt_text))],
                );

                if let Err(e) = connection.send_request(prompt_req).block_task().await {
                    let _ = event_tx.send(AgentEvent::Error(format!("prompt failed: {e}")));
                }
            }

            Ok(())
        })
        .await?;

    Ok(())
}
