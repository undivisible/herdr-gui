#![allow(dead_code)]

use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, InitializeRequest, NewSessionRequest, PromptRequest,
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
    _handle: thread::JoinHandle<()>,
}

impl AcpSession {
    pub fn spawn(command: &str, cwd: PathBuf) -> Result<Self, String> {
        let (event_tx, event_rx) = mpsc::channel();
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
                if let Err(e) = run_agent(&command, cwd, &event_tx).await {
                    let _ = event_tx.send(AgentEvent::Error(e.to_string()));
                }
                let _ = event_tx.send(AgentEvent::Done);
            });
        });

        Ok(Self {
            event_rx,
            _handle: handle,
        })
    }

    pub fn try_recv(&self) -> Option<AgentEvent> {
        self.event_rx.try_recv().ok()
    }
}

async fn run_agent(
    command: &str,
    cwd: PathBuf,
    event_tx: &mpsc::Sender<AgentEvent>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let agent = AcpAgent::from_str(command)?;
    let tx = event_tx.clone();

    Client
        .builder()
        .on_receive_notification(
            async move |notification: SessionNotification, _cx| {
                match notification.update {
                    SessionUpdate::AgentMessageChunk(ContentChunk {
                        content: ContentBlock::Text(TextContent { text, .. }),
                        ..
                    }) => {
                        let _ = tx.send(AgentEvent::Text(text));
                    }
                    SessionUpdate::ToolCall(tool_call) => {
                        let _ = tx.send(AgentEvent::ToolCall {
                            title: tool_call.title,
                        });
                    }
                    _ => {}
                }
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |_request: agent_client_protocol::schema::v1::RequestPermissionRequest,
                        responder,
                        _connection| {
                responder.respond(
                    agent_client_protocol::schema::v1::RequestPermissionResponse::new(
                        agent_client_protocol::schema::v1::RequestPermissionOutcome::Selected(
                            agent_client_protocol::schema::v1::SelectedPermissionOutcome::new(
                                "approve".to_string(),
                            ),
                        ),
                    ),
                )
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, |connection: ConnectionTo<Agent>| async move {
            connection
                .send_request(InitializeRequest::new(
                    agent_client_protocol::schema::ProtocolVersion::V1,
                ))
                .block_task()
                .await?;

            let session_response = connection
                .send_request(NewSessionRequest::new(cwd))
                .block_task()
                .await?;

            let session_id = session_response.session_id.clone();
            let prompt_req = PromptRequest::new(
                session_id,
                vec![ContentBlock::Text(TextContent::new("help"))],
            );

            connection.send_request(prompt_req).block_task().await?;

            Ok(())
        })
        .await?;

    Ok(())
}
