use crate::acp::AcpSession;
use crepuscularity_gpui::prelude::*;
use crepuscularity_gpui::{div, px, rgb, AnyElement, FontWeight, IntoElement};

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
}

impl AgentChatState {
    pub fn new() -> Self {
        Self {
            view: PaneView::Terminal,
            visible: false,
            messages: Vec::new(),
            input_text: String::new(),
            session: None,
            is_working: false,
            selected_agent: AgentKind::ClaudeCode,
        }
    }
}

/// Render the agent chat panel — message list + composer.
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
        .child(
            div()
                .w_full()
                .h(px(32.0))
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
                        .child(div().text_size(px(12.0)).text_color(rgb(theme.text)).child(
                            format!(
                                "{} {}",
                                state.selected_agent.icon(),
                                state.selected_agent.label()
                            ),
                        ))
                        .when(state.is_working, |el| {
                            el.child(
                                div()
                                    .w(px(6.0))
                                    .h(px(6.0))
                                    .rounded_full()
                                    .bg(rgb(theme.active)),
                            )
                        }),
                )
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(rgb(theme.muted))
                        .child(format!("{} messages", state.messages.len())),
                ),
        )
        // Message list
        .child(
            div()
                .id("agent-chat-messages")
                .flex_1()
                .w_full()
                .min_h_0()
                .overflow_y_scroll()
                .overflow_x_hidden()
                .px_3()
                .py_2()
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .children(state.messages.iter().map(|msg| render_message(msg, theme)))
                        .when(state.is_working, |el| {
                            el.child(render_thinking_indicator(theme))
                        })
                        .when(state.messages.is_empty() && !state.is_working, |el| {
                            el.child(render_empty_state(state.selected_agent, theme))
                        }),
                ),
        )
        // Composer
        .child(
            div()
                .w_full()
                .border_t_1()
                .border_color(rgb(theme.border))
                .px_3()
                .py_2()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .w_full()
                        .h(px(28.0))
                        .rounded_md()
                        .bg(rgb(theme.hover))
                        .px_2()
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
                                .child(if state.input_text.is_empty() {
                                    format!("Ask {}…", state.selected_agent.label())
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
                                .child("Enter to send · Tab to switch agent"),
                        )
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(rgb(theme.muted))
                                .child("Esc to close"),
                        ),
                ),
        )
        .into_any_element()
}

fn render_thinking_indicator(theme: UiTheme) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .py_1()
        .child(
            div()
                .w(px(8.0))
                .h(px(8.0))
                .rounded_full()
                .bg(rgb(theme.active)),
        )
        .child(
            div()
                .text_size(px(11.0))
                .text_color(rgb(theme.muted))
                .child("thinking…"),
        )
        .into_any_element()
}

fn render_empty_state(agent: AgentKind, theme: UiTheme) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .py_8()
        .gap_3()
        .child(
            div()
                .text_size(px(24.0))
                .text_color(rgb(theme.active))
                .child(agent.icon().to_string()),
        )
        .child(
            div()
                .text_size(px(14.0))
                .text_color(rgb(theme.text))
                .child(agent.label().to_string()),
        )
        .child(
            div()
                .text_size(px(11.0))
                .text_color(rgb(theme.muted))
                .child(format!(
                    "Type a message to start chatting with {}",
                    agent.label()
                )),
        )
        .into_any_element()
}

fn render_message(msg: &AgentChatMessage, theme: UiTheme) -> AnyElement {
    let (bg_color, text_color, label, label_color) = match &msg.role {
        AgentRole::User => (theme.hover, theme.text, "You", theme.active),
        AgentRole::Agent => (theme.bg, theme.text, "Agent", theme.active),
        AgentRole::ToolCall => (theme.hover, theme.muted, "Tool", theme.label),
        AgentRole::Error => (theme.hover, 0xfca5a5, "Error", 0xfca5a5),
    };

    div()
        .w_full()
        .rounded_lg()
        .bg(rgb(bg_color))
        .px_3()
        .py_2()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_size(px(10.0))
                .text_color(rgb(label_color))
                .font_weight(FontWeight::BOLD)
                .child(label.to_string()),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(rgb(text_color))
                .child(msg.text.clone()),
        )
        .into_any_element()
}
