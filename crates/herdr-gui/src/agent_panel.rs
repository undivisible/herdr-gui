use crate::acp::AcpSession;
use crepuscularity_gpui::prelude::*;
use crepuscularity_gpui::{div, px, rgb, AnyElement, FontWeight, IntoElement};
use std::process::Command;

use crate::theme::UiTheme;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PaneView {
    Terminal,
    AgentChat,
}

#[derive(Debug, Clone)]
pub struct AgentChatMessage {
    pub role: AgentRole,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum AgentRole {
    User,
    Agent,
    ToolCall,
    Error,
}

pub struct AgentChatState {
    pub view: PaneView,
    pub visible: bool,
    pub messages: Vec<AgentChatMessage>,
    pub input_text: String,
    pub session: Option<AcpSession>,
    pub is_working: bool,
    pub selected_agent: AgentKind,
    /// Agents detected as installed on this machine.
    pub installed_agents: Vec<AgentKind>,
    /// Agent detected from herdr's running agents.
    pub herdr_agent: Option<AgentKind>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AgentKind {
    ClaudeCode,
    GeminiCli,
    CodexCli,
    OpenCode,
    Pi,
    Goose,
    Cursor,
    Kimi,
}

impl AgentKind {
    pub fn all() -> &'static [AgentKind] {
        &[
            AgentKind::ClaudeCode,
            AgentKind::GeminiCli,
            AgentKind::CodexCli,
            AgentKind::OpenCode,
            AgentKind::Pi,
            AgentKind::Goose,
            AgentKind::Cursor,
            AgentKind::Kimi,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "Claude Code",
            AgentKind::GeminiCli => "Gemini CLI",
            AgentKind::CodexCli => "Codex CLI",
            AgentKind::OpenCode => "OpenCode",
            AgentKind::Pi => "Pi",
            AgentKind::Goose => "Goose",
            AgentKind::Cursor => "Cursor",
            AgentKind::Kimi => "Kimi CLI",
        }
    }

    pub fn command(&self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "claude-code",
            AgentKind::GeminiCli => "gemini",
            AgentKind::CodexCli => "codex",
            AgentKind::OpenCode => "opencode",
            AgentKind::Pi => "pi-acp",
            AgentKind::Goose => "goose",
            AgentKind::Cursor => "cursor-agent",
            AgentKind::Kimi => "kimi",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "◈",
            AgentKind::GeminiCli => "✦",
            AgentKind::CodexCli => "◉",
            AgentKind::OpenCode => "◇",
            AgentKind::Pi => "π",
            AgentKind::Goose => "♦",
            AgentKind::Cursor => "▸",
            AgentKind::Kimi => "◆",
        }
    }

    /// Detect which ACP agents are installed on this machine.
    pub fn detect_installed() -> Vec<AgentKind> {
        Self::all()
            .iter()
            .filter(|kind| {
                Command::new("which")
                    .arg(kind.command())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false)
            })
            .copied()
            .collect()
    }

    /// Map a herdr agent name to an AgentKind.
    pub fn from_herdr_agent(name: &str) -> Option<AgentKind> {
        let lower = name.to_lowercase();
        if lower.contains("claude") {
            Some(AgentKind::ClaudeCode)
        } else if lower.contains("gemini") {
            Some(AgentKind::GeminiCli)
        } else if lower.contains("codex") {
            Some(AgentKind::CodexCli)
        } else if lower.contains("opencode") {
            Some(AgentKind::OpenCode)
        } else if lower.contains("pi") {
            Some(AgentKind::Pi)
        } else if lower.contains("goose") {
            Some(AgentKind::Goose)
        } else if lower.contains("cursor") {
            Some(AgentKind::Cursor)
        } else if lower.contains("kimi") {
            Some(AgentKind::Kimi)
        } else {
            None
        }
    }
}

impl AgentChatState {
    pub fn new() -> Self {
        let installed = AgentKind::detect_installed();
        Self {
            view: PaneView::Terminal,
            visible: false,
            messages: Vec::new(),
            input_text: String::new(),
            session: None,
            is_working: false,
            selected_agent: installed.first().copied().unwrap_or(AgentKind::ClaudeCode),
            installed_agents: installed,
            herdr_agent: None,
        }
    }

    /// Pick the best agent: herdr running agent > first installed > default.
    pub fn auto_select(&mut self, herdr_agents: &[crate::herdr::Agent]) {
        // Try to match a running herdr agent that is also installed on PATH
        for ha in herdr_agents {
            if let Some(name) = ha.agent.as_deref() {
                if let Some(kind) = AgentKind::from_herdr_agent(name) {
                    if self.installed_agents.contains(&kind) {
                        self.selected_agent = kind;
                        self.herdr_agent = Some(kind);
                        return;
                    }
                }
            }
        }
        // Fall back to first installed
        if let Some(first) = self.installed_agents.first() {
            self.selected_agent = *first;
        }
    }
}

/// Render the agent chat panel — Zed/arbor inspired.
pub fn render_agent_chat(
    state: &AgentChatState,
    theme: UiTheme,
    _cx: &mut Context<crate::HerdrGui>,
) -> AnyElement {
    div()
        .h_full()
        .w_full()
        .flex()
        .flex_col()
        .bg(rgb(theme.terminal))
        // Header bar
        .child(render_header(state, theme))
        // Message list
        .child(
            div()
                .id("agent-chat-messages")
                .flex_1()
                .w_full()
                .min_h_0()
                .overflow_y_scroll()
                .overflow_x_hidden()
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .px_4()
                        .py_3()
                        .children(
                            state
                                .messages
                                .iter()
                                .enumerate()
                                .map(|(i, msg)| render_message(msg, i, theme)),
                        )
                        .when(state.is_working, |el| el.child(render_thinking(theme)))
                        .when(state.messages.is_empty() && !state.is_working, |el| {
                            el.child(render_empty(state.selected_agent, state, theme))
                        }),
                ),
        )
        // Composer
        .child(render_composer(state, theme))
        .into_any_element()
}

fn render_header(state: &AgentChatState, theme: UiTheme) -> AnyElement {
    div()
        .w_full()
        .h(px(36.0))
        .flex()
        .items_center()
        .justify_between()
        .px_3()
        .bg(rgb(theme.panel))
        .border_b_1()
        .border_color(rgb(theme.border))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_size(px(13.0))
                        .text_color(rgb(theme.text))
                        .font_weight(FontWeight::BOLD)
                        .child(format!(
                            "{} {}",
                            state.selected_agent.icon(),
                            state.selected_agent.label()
                        )),
                )
                .when(state.is_working, |el| {
                    el.child(
                        div()
                            .w(px(6.0))
                            .h(px(6.0))
                            .rounded_full()
                            .bg(rgb(theme.active)),
                    )
                })
                .when(state.herdr_agent.is_some(), |el| {
                    el.child(
                        div()
                            .text_size(px(9.0))
                            .text_color(rgb(theme.muted))
                            .child("herdr"),
                    )
                }),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(rgb(theme.muted))
                        .child(format!("{} msgs", state.messages.len())),
                )
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(rgb(theme.label))
                        .child(format!("{} installed", state.installed_agents.len())),
                ),
        )
        .into_any_element()
}

fn render_message(msg: &AgentChatMessage, index: usize, theme: UiTheme) -> AnyElement {
    let is_user = msg.role == AgentRole::User;
    let is_error = msg.role == AgentRole::Error;
    let is_tool = msg.role == AgentRole::ToolCall;

    // User right, agent/tool left (Zed/arbor pattern)
    let container = div().w_full().flex().flex_col().gap_1();

    let aligned = if is_user {
        container.items_end()
    } else {
        container.items_start()
    };

    let bubble = div()
        .max_w(px(520.0))
        .text_size(px(12.0))
        .line_height(px(20.0))
        .px_3()
        .py_2()
        .rounded_lg();

    let styled_bubble = if is_user {
        bubble.bg(rgb(theme.active)).text_color(rgb(theme.bg))
    } else if is_error {
        bubble
            .border_1()
            .border_color(rgb(0xc94040))
            .bg(rgb(0x2a1010))
            .text_color(rgb(0xfca5a5))
    } else if is_tool {
        bubble
            .border_1()
            .border_color(rgb(theme.border))
            .bg(rgb(theme.hover))
            .text_color(rgb(theme.muted))
    } else {
        bubble.text_color(rgb(theme.text))
    };

    aligned
        .child(styled_bubble.child(div().child(msg.text.clone())))
        // Role label below bubble
        .child(
            div()
                .text_size(px(9.0))
                .text_color(rgb(theme.label))
                .px_1()
                .child(match &msg.role {
                    AgentRole::User => "you",
                    AgentRole::Agent => {
                        if index == 0 {
                            "agent"
                        } else {
                            ""
                        }
                    }
                    AgentRole::ToolCall => "tool",
                    AgentRole::Error => "error",
                }),
        )
        .into_any_element()
}

fn render_thinking(theme: UiTheme) -> AnyElement {
    div()
        .flex()
        .items_start()
        .gap_2()
        .px_1()
        .py_1()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .rounded_lg()
                .text_color(rgb(theme.muted))
                .text_size(px(12.0))
                .child("●  ●  ●"),
        )
        .into_any_element()
}

fn render_empty(agent: AgentKind, state: &AgentChatState, theme: UiTheme) -> AnyElement {
    let status_text = if state.installed_agents.is_empty() {
        "No ACP agents found on PATH".to_string()
    } else if state.herdr_agent.is_some() {
        format!("Connected via herdr — {}", agent.label())
    } else {
        format!("{} available — type to start", agent.label())
    };

    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .py_12()
        .gap_4()
        .child(
            div()
                .text_size(px(32.0))
                .text_color(rgb(theme.active))
                .child(agent.icon().to_string()),
        )
        .child(
            div()
                .text_size(px(15.0))
                .text_color(rgb(theme.text))
                .font_weight(FontWeight::BOLD)
                .child(agent.label().to_string()),
        )
        .child(
            div()
                .text_size(px(11.0))
                .text_color(rgb(theme.muted))
                .child(status_text),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .items_center()
                .mt_2()
                .children(state.installed_agents.iter().map(|kind| {
                    div()
                        .text_size(px(10.0))
                        .text_color(rgb(theme.label))
                        .child(format!("{} {}", kind.icon(), kind.label()))
                })),
        )
        .into_any_element()
}

fn render_composer(state: &AgentChatState, theme: UiTheme) -> AnyElement {
    let has_agent = !state.installed_agents.is_empty() || state.herdr_agent.is_some();
    let is_agent_view = state.view == PaneView::AgentChat;

    div()
        .w_full()
        .flex_none()
        .border_t_1()
        .border_color(rgb(theme.border))
        .bg(rgb(theme.panel))
        .px_3()
        .py_2()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .w_full()
                .rounded_lg()
                .border_1()
                .border_color(rgb(theme.border))
                .bg(rgb(theme.hover))
                .px_3()
                .py_2()
                .flex()
                .items_center()
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(rgb(if state.input_text.is_empty() {
                            theme.muted
                        } else {
                            theme.text
                        }))
                        .child(if !has_agent {
                            "No agent available — install one to chat".to_string()
                        } else if state.input_text.is_empty() {
                            if is_agent_view {
                                format!("Type a message to {}…", state.selected_agent.label())
                            } else {
                                format!("Ask {}…", state.selected_agent.label())
                            }
                        } else {
                            state.input_text.clone()
                        }),
                ),
        )
        .child(
            div()
                .flex()
                .justify_between()
                .items_center()
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(rgb(theme.label))
                        .child(if is_agent_view {
                            "Enter to send · Tab to switch agent · Esc to close"
                        } else {
                            "Click agent icon to start chatting"
                        }),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .children(state.installed_agents.iter().map(|kind| {
                            div()
                                .text_size(px(10.0))
                                .text_color(rgb(if *kind == state.selected_agent {
                                    theme.active
                                } else {
                                    theme.muted
                                }))
                                .child(kind.icon().to_string())
                        })),
                ),
        )
        .into_any_element()
}
