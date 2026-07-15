use crate::acp::AcpSession;
use crepuscularity_gpui::prelude::*;
use crepuscularity_gpui::{div, px, rgb, AnyElement, FontWeight, IntoElement};

use crate::theme::UiTheme;

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
    pub visible: bool,
    pub messages: Vec<AgentChatMessage>,
    pub input_text: String,
    pub session: Option<AcpSession>,
    pub is_working: bool,
}

impl AgentChatState {
    pub fn new() -> Self {
        Self {
            visible: false,
            messages: Vec::new(),
            input_text: String::new(),
            session: None,
            is_working: false,
        }
    }
}

/// Render the agent chat panel — message list + composer.
pub fn render_agent_chat(
    state: &AgentChatState,
    theme: UiTheme,
    _cx: &mut Context<crate::HerdrGui>,
) -> AnyElement {
    if !state.visible {
        return div().into_any_element();
    }

    div()
        .h_full()
        .w_full()
        .flex()
        .flex_col()
        .bg(rgb(theme.panel))
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
                            el.child(
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
                                    ),
                            )
                        })
                        .when(state.messages.is_empty() && !state.is_working, |el| {
                            el.child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .py_8()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_size(px(13.0))
                                            .text_color(rgb(theme.muted))
                                            .child("No agent connected"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .text_color(rgb(theme.label))
                                            .child("Start an ACP agent to chat"),
                                    ),
                            )
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
                                    "Ask an agent…".to_string()
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
                                .child("Enter to send"),
                        )
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(rgb(theme.muted))
                                .child(format!("{} messages", state.messages.len())),
                        ),
                ),
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
