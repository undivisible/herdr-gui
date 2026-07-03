mod ghostty;
mod herdr;

use crepuscularity_gpui as gpui;
use crepuscularity_gpui::prelude::*;
use crepuscularity_gpui::{
    actions, bounds, div, gpui_window_options, point, px, rgb, size, App, Application, Context,
    FocusHandle, IntoElement, KeyBinding, Keystroke, MouseButton, Render, SharedString, Window,
    WindowBounds,
};
use ghostty::{GhosttyRuntime, TerminalSession};
use herdr::{HerdrClient, HerdrState, Pane, Tab, Workspace};
use std::{sync::mpsc::Receiver, time::Duration};

actions!(
    herdr_gui,
    [
        ToggleHelp,
        Refresh,
        SplitRight,
        SplitDown,
        FocusRight,
        ResizeRight,
        ClosePane,
        PreviousTab,
        NextTab
    ]
);

struct HerdrGui {
    client: Option<HerdrClient>,
    ghostty_status: String,
    terminal: Option<TerminalSession>,
    terminal_target: Option<String>,
    terminal_text: String,
    state: HerdrState,
    status: String,
    show_help: bool,
    focus_handle: FocusHandle,
}

impl HerdrGui {
    fn new(cx: &mut Context<Self>) -> Self {
        let ghostty_status = match GhosttyRuntime::detect() {
            Ok(runtime) => format!("Ghostty VT · {}", runtime.root.display()),
            Err(err) => err,
        };
        let (client, state, status) = match HerdrClient::bootstrap() {
            Ok(client) => match client.state() {
                Ok(state) => (Some(client), state, "connected".to_string()),
                Err(err) => (Some(client), HerdrState::default(), err.to_string()),
            },
            Err(err) => (None, HerdrState::default(), err.to_string()),
        };
        let mut app = Self {
            client,
            ghostty_status,
            terminal: None,
            terminal_target: None,
            terminal_text: String::new(),
            state,
            status,
            show_help: false,
            focus_handle: cx.focus_handle(),
        };
        app.attach_focused_terminal(cx);
        app
    }

    fn toggle_help(&mut self, _: &ToggleHelp, _window: &mut Window, cx: &mut Context<Self>) {
        self.show_help = !self.show_help;
        cx.notify();
    }

    fn refresh(&mut self, _: &Refresh, _window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_state();
        self.attach_focused_terminal(cx);
        cx.notify();
    }

    fn split_right(&mut self, _: &SplitRight, _window: &mut Window, cx: &mut Context<Self>) {
        self.with_first_pane(|client, pane| client.split_right(&pane.pane_id));
        self.refresh_state();
        cx.notify();
    }

    fn split_down(&mut self, _: &SplitDown, _window: &mut Window, cx: &mut Context<Self>) {
        self.with_first_pane(|client, pane| client.split_down(&pane.pane_id));
        self.refresh_state();
        cx.notify();
    }

    fn focus_right(&mut self, _: &FocusRight, _window: &mut Window, cx: &mut Context<Self>) {
        self.with_client(HerdrClient::focus_right);
        self.refresh_state();
        self.attach_focused_terminal(cx);
        cx.notify();
    }

    fn resize_right(&mut self, _: &ResizeRight, _window: &mut Window, cx: &mut Context<Self>) {
        self.with_first_pane(|client, pane| client.resize_right(&pane.pane_id));
        self.refresh_state();
        cx.notify();
    }

    fn close_pane(&mut self, _: &ClosePane, _window: &mut Window, cx: &mut Context<Self>) {
        self.with_first_pane(|client, pane| client.close_pane(&pane.pane_id));
        self.refresh_state();
        cx.notify();
    }

    fn previous_tab(&mut self, _: &PreviousTab, _window: &mut Window, cx: &mut Context<Self>) {
        self.focus_tab_offset(-1, cx);
    }

    fn next_tab(&mut self, _: &NextTab, _window: &mut Window, cx: &mut Context<Self>) {
        self.focus_tab_offset(1, cx);
    }

    fn focus_workspace_id(&mut self, workspace_id: String, cx: &mut Context<Self>) {
        self.with_client(|client| client.focus_workspace(&workspace_id));
        self.refresh_state();
        self.attach_focused_terminal(cx);
        cx.notify();
    }

    fn focus_tab_id(&mut self, tab_id: String, cx: &mut Context<Self>) {
        self.with_client(|client| client.focus_tab(&tab_id));
        self.refresh_state();
        self.attach_focused_terminal(cx);
        cx.notify();
    }

    fn focus_tab_offset(&mut self, offset: isize, cx: &mut Context<Self>) {
        let tabs = self.visible_tabs();
        if tabs.is_empty() {
            return;
        }
        let active_id = self
            .state
            .focused_tab_id
            .as_deref()
            .or_else(|| self.active_tab().map(|tab| tab.tab_id.as_str()));
        let active_index = active_id
            .and_then(|id| tabs.iter().position(|tab| tab.tab_id == id))
            .unwrap_or(0);
        let next_index = (active_index as isize + offset).rem_euclid(tabs.len() as isize) as usize;
        self.focus_tab_id(tabs[next_index].tab_id.clone(), cx);
    }

    fn focus_pane_id(&mut self, pane_id: String, cx: &mut Context<Self>) {
        self.with_client(|client| client.focus_pane(&pane_id));
        self.refresh_state();
        self.attach_focused_terminal(cx);
        cx.notify();
    }

    fn refresh_state(&mut self) {
        if let Some(client) = &self.client {
            match client.state() {
                Ok(state) => {
                    self.state = state;
                    self.status = "connected".to_string();
                }
                Err(err) => self.status = err.to_string(),
            }
        }
    }

    fn attach_focused_terminal(&mut self, cx: &mut Context<Self>) {
        let target = self
            .focused_pane()
            .and_then(|pane| pane.terminal_id.clone())
            .or_else(|| self.focused_pane().map(|pane| pane.pane_id.clone()));
        if target.is_none() || target == self.terminal_target {
            return;
        }
        let target = match target {
            Some(target) => target,
            None => return,
        };
        match TerminalSession::attach(&target, 120, 40) {
            Ok(mut session) => {
                if let Some(receiver) = session.output.take() {
                    self.terminal = Some(session);
                    self.terminal_target = Some(target);
                    self.status = "connected".to_string();
                    poll_terminal(receiver, cx);
                }
            }
            Err(err) => {
                self.terminal = None;
                self.terminal_target = None;
                self.terminal_text = err.clone();
                self.status = err;
            }
        }
    }

    fn with_client(&mut self, f: impl FnOnce(&HerdrClient) -> Result<(), herdr::HerdrError>) {
        if let Some(client) = &self.client {
            if let Err(err) = f(client) {
                self.status = err.to_string();
            }
        }
    }

    fn with_first_pane(
        &mut self,
        f: impl FnOnce(&HerdrClient, &Pane) -> Result<(), herdr::HerdrError>,
    ) {
        let pane = self.focused_pane().cloned();
        if let (Some(client), Some(pane)) = (&self.client, pane) {
            if let Err(err) = f(client, &pane) {
                self.status = err.to_string();
            }
        }
    }

    fn focused_pane(&self) -> Option<&Pane> {
        if let Some(focused_id) = self.state.focused_pane_id.as_deref() {
            if let Some(pane) = self
                .state
                .panes
                .iter()
                .find(|pane| pane.pane_id == focused_id)
            {
                return Some(pane);
            }
        }
        self.state
            .panes
            .iter()
            .find(|pane| pane.focused)
            .or_else(|| self.state.panes.first())
    }

    fn handle_keystroke(&mut self, key: &Keystroke) {
        let Some(bytes) = terminal_bytes(key) else {
            return;
        };
        if let Some(terminal) = &self.terminal {
            if terminal.input.send(bytes).is_err() {
                self.status = "terminal input disconnected".to_string();
            }
        } else if let (Some(client), Some(pane)) = (&self.client, self.focused_pane().cloned()) {
            let result = if let Some(text) = key.key_char.as_deref() {
                client.send_text(&pane.pane_id, text)
            } else {
                client.send_key(&pane.pane_id, key_name(key))
            };
            if let Err(err) = result {
                self.status = err.to_string();
            }
        }
    }
}

impl Render for HerdrGui {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let panes = self.visible_panes();
        let pane_text = self.terminal_text.clone();

        div()
            .w_full()
            .h_full()
            .bg(rgb(0x1f242e))
            .text_color(rgb(0xd7dde8))
            .flex()
            .key_context("HerdrGui")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::toggle_help))
            .on_action(cx.listener(Self::refresh))
            .on_action(cx.listener(Self::split_right))
            .on_action(cx.listener(Self::split_down))
            .on_action(cx.listener(Self::focus_right))
            .on_action(cx.listener(Self::resize_right))
            .on_action(cx.listener(Self::close_pane))
            .on_action(cx.listener(Self::previous_tab))
            .on_action(cx.listener(Self::next_tab))
            .child(self.sidebar(cx))
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .bg(rgb(0x0f1117))
                    .overflow_hidden()
                    .child(self.pane_grid(panes, pane_text, cx))
                    .when(self.show_help, |el| el.child(help_overlay())),
            )
    }
}

impl HerdrGui {
    fn active_workspace(&self) -> Option<&Workspace> {
        if let Some(id) = self.state.focused_workspace_id.as_deref() {
            if let Some(workspace) = self
                .state
                .workspaces
                .iter()
                .find(|workspace| workspace.workspace_id == id)
            {
                return Some(workspace);
            }
        }
        self.state
            .workspaces
            .iter()
            .find(|workspace| workspace.focused)
            .or_else(|| self.state.workspaces.first())
    }

    fn active_workspace_id(&self) -> Option<&str> {
        self.state.focused_workspace_id.as_deref().or_else(|| {
            self.active_workspace()
                .map(|workspace| workspace.workspace_id.as_str())
        })
    }

    fn active_tab(&self) -> Option<&Tab> {
        if let Some(id) = self.state.focused_tab_id.as_deref() {
            if let Some(tab) = self.state.tabs.iter().find(|tab| tab.tab_id == id) {
                return Some(tab);
            }
        }
        self.state
            .tabs
            .iter()
            .find(|tab| tab.focused)
            .or_else(|| self.state.tabs.first())
    }

    fn visible_tabs(&self) -> Vec<Tab> {
        let Some(workspace_id) = self.active_workspace_id() else {
            return Vec::new();
        };
        let mut tabs = self
            .state
            .tabs
            .iter()
            .filter(|tab| {
                tab.workspace_id
                    .as_deref()
                    .is_none_or(|tab_workspace| tab_workspace == workspace_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        if tabs.is_empty() {
            tabs = self.state.tabs.clone();
        }
        tabs
    }

    fn visible_panes(&self) -> Vec<Pane> {
        let focused_tab_id = self
            .state
            .focused_tab_id
            .as_deref()
            .or_else(|| self.active_tab().map(|tab| tab.tab_id.as_str()));
        let focused_workspace_id = self.active_workspace_id();
        let mut panes = self
            .state
            .panes
            .iter()
            .filter(|pane| {
                focused_tab_id.is_none_or(|tab_id| {
                    pane.tab_id
                        .as_deref()
                        .is_none_or(|pane_tab| pane_tab == tab_id)
                }) && focused_workspace_id.is_none_or(|workspace_id| {
                    pane.workspace_id
                        .as_deref()
                        .is_none_or(|pane_workspace| pane_workspace == workspace_id)
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        if panes.is_empty() {
            panes = self.state.panes.clone();
        }
        panes
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = self.active_workspace();
        let tab = self.active_tab();
        let pane_count = self.visible_panes().len();

        div()
            .w(px(210.0))
            .h_full()
            .border_r_1()
            .border_color(rgb(0x343b48))
            .bg(rgb(0x1b2028))
            .p_2()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .h(px(24.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(section("window"))
                    .child(status_dot(&self.status)),
            )
            .child(status_band(&self.status))
            .child(status_band(&self.ghostty_status))
            .when_some(workspace, |el, workspace| {
                let id = workspace.workspace_id.clone();
                el.child(row(
                    workspace
                        .label
                        .as_deref()
                        .unwrap_or(&workspace.workspace_id)
                        .to_string(),
                    workspace
                        .agent_status
                        .as_deref()
                        .unwrap_or("unknown")
                        .to_string(),
                    true,
                    cx.listener(move |this, _, _, cx| this.focus_workspace_id(id.clone(), cx)),
                ))
            })
            .child(section("current tab"))
            .when_some(tab, |el, tab| {
                let id = tab.tab_id.clone();
                el.child(row(
                    tab.label.as_deref().unwrap_or(&tab.tab_id).to_string(),
                    tab.agent_status
                        .clone()
                        .unwrap_or_else(|| format!("{pane_count} pane{}", plural(pane_count))),
                    true,
                    cx.listener(move |this, _, _, cx| this.focus_tab_id(id.clone(), cx)),
                ))
            })
            .child(section("visible panes"))
            .children(self.visible_panes().into_iter().map(|pane| {
                let id = pane.pane_id.clone();
                row(
                    pane.agent
                        .as_deref()
                        .or(pane.label.as_deref())
                        .unwrap_or(&pane.pane_id)
                        .to_string(),
                    pane.agent_status
                        .as_deref()
                        .unwrap_or("unknown")
                        .to_string(),
                    pane.focused,
                    cx.listener(move |this, _, _, cx| this.focus_pane_id(id.clone(), cx)),
                )
            }))
    }

    fn pane_grid(
        &self,
        panes: Vec<Pane>,
        pane_text: String,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        if panes.is_empty() {
            return div()
                .flex()
                .flex_1()
                .h_full()
                .items_center()
                .justify_center()
                .child(empty_state(&self.status));
        }

        div()
            .flex()
            .flex_1()
            .h_full()
            .bg(rgb(0x0f1117))
            .children(panes.into_iter().map(|pane| {
                let pane_id = pane.pane_id.clone();
                div()
                    .flex_1()
                    .h_full()
                    .bg(rgb(0x0f1117))
                    .flex()
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| this.focus_pane_id(pane_id.clone(), cx)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .p_4()
                            .text_size(px(12.0))
                            .font_family("Menlo")
                            .line_height(px(18.0))
                            .text_color(rgb(0xc5ceda))
                            .child(SharedString::from(if pane.focused {
                                pane_text.clone()
                            } else {
                                String::new()
                            })),
                    )
            }))
    }
}

fn label(text: &str, color: u32) -> impl IntoElement {
    div()
        .text_size(px(14.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(color))
        .child(text.to_string())
}

fn small(text: &str) -> impl IntoElement {
    div()
        .text_size(px(11.0))
        .text_color(rgb(0x94a3b8))
        .child(text.to_string())
}

fn section(text: &str) -> impl IntoElement {
    div()
        .pt_1()
        .text_size(px(10.0))
        .text_color(rgb(0x7b8494))
        .child(text.to_string())
}

fn row(
    title: String,
    detail: String,
    focused: bool,
    on_click: impl Fn(&crepuscularity_gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .rounded_md()
        .bg(if focused {
            rgb(0x303847)
        } else {
            rgb(0x242b36)
        })
        .px_3()
        .py_2()
        .flex()
        .flex_col()
        .gap_0p5()
        .cursor_pointer()
        .hover(|style| style.bg(rgb(0x343c4b)))
        .on_mouse_down(MouseButton::Left, on_click)
        .child(label(&title, 0xe2e8f0))
        .child(small(&detail))
}

fn status_band(text: &str) -> impl IntoElement {
    div()
        .rounded_md()
        .bg(rgb(0x1d232d))
        .px_3()
        .py_2()
        .text_size(px(12.0))
        .text_color(if text == "connected" || text.starts_with("Ghostty VT") {
            rgb(0xa7f3d0)
        } else {
            rgb(0xfca5a5)
        })
        .child(text.to_string())
}

fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

fn status_dot(text: &str) -> impl IntoElement {
    div()
        .w(px(9.0))
        .h(px(9.0))
        .rounded_full()
        .bg(if text == "connected" {
            rgb(0x34d399)
        } else {
            rgb(0xf87171)
        })
}

fn empty_state(status: &str) -> impl IntoElement {
    div()
        .w(px(560.0))
        .rounded_lg()
        .bg(rgb(0x242b36))
        .border_1()
        .border_color(rgb(0x343b48))
        .p_5()
        .flex()
        .flex_col()
        .gap_3()
        .child(label("No Herdr panes visible", 0xf8fafc))
        .child(small(status))
        .child(
            div()
                .rounded_lg()
                .bg(rgb(0x090d14))
                .border_1()
                .border_color(rgb(0x202633))
                .p_3()
                .font_family("Menlo")
                .text_size(px(12.0))
                .text_color(rgb(0x94a3b8))
                .child("Open Herdr in a terminal, create a workspace/pane, then press Refresh."),
        )
}

fn kbd_hint(text: &str) -> impl IntoElement {
    div()
        .rounded_sm()
        .bg(rgb(0x303847))
        .border_1()
        .border_color(rgb(0x475266))
        .px_2()
        .py_1()
        .text_size(px(11.0))
        .text_color(rgb(0x94a3b8))
        .child(text.to_string())
}

fn key_row(key: &str, action: &str) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_6()
        .child(kbd_hint(key))
        .child(small(action))
}

fn help_overlay() -> impl IntoElement {
    div()
        .absolute()
        .top(px(20.0))
        .right(px(20.0))
        .w(px(300.0))
        .rounded_lg()
        .bg(rgb(0x101722))
        .border_1()
        .border_color(rgb(0x2c3544))
        .p_4()
        .flex()
        .flex_col()
        .gap_2()
        .child(label("Keys", 0xf8fafc))
        .child(key_row("F1", "toggle help"))
        .child(key_row("Cmd R", "refresh"))
        .child(key_row("Cmd ]", "split right"))
        .child(key_row("Cmd Shift ]", "split down"))
        .child(key_row("→", "focus right"))
        .child(key_row("Shift →", "resize right"))
        .child(key_row("Cmd ←", "previous tab"))
        .child(key_row("Cmd →", "next tab"))
        .child(key_row("Cmd W", "close pane"))
}

fn main() {
    std::env::set_var("OS_ACTIVITY_MODE", "disable");

    Application::new().run(|cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("f1", ToggleHelp, None),
            KeyBinding::new("cmd-r", Refresh, None),
            KeyBinding::new("cmd-]", SplitRight, None),
            KeyBinding::new("cmd-shift-]", SplitDown, None),
            KeyBinding::new("right", FocusRight, None),
            KeyBinding::new("shift-right", ResizeRight, None),
            KeyBinding::new("cmd-w", ClosePane, None),
            KeyBinding::new("cmd-left", PreviousTab, None),
            KeyBinding::new("cmd-right", NextTab, None),
        ]);

        let options = gpui_window_options(
            "dev.undivisible.herdr-gui",
            "",
            Some(WindowBounds::Windowed(bounds(
                point(px(80.0), px(80.0)),
                size(px(1280.0), px(820.0)),
            ))),
            Some(size(px(920.0), px(600.0))),
        );
        let window = cx.open_window(options, |_window, cx| cx.new(HerdrGui::new));
        if let Ok(window) = window {
            let view = window.update(cx, |_, _, cx| cx.entity());
            if let Ok(view) = view {
                cx.observe_keystrokes(move |event, _, cx| {
                    view.update(cx, |view, cx| {
                        view.handle_keystroke(&event.keystroke);
                        cx.notify();
                    });
                })
                .detach();
            }
        }
        cx.activate(true);
    });
}

fn poll_terminal(receiver: Receiver<String>, cx: &mut Context<HerdrGui>) {
    cx.spawn(async move |this, cx| loop {
        cx.background_executor()
            .timer(Duration::from_millis(33))
            .await;
        let mut latest = None;
        while let Ok(text) = receiver.try_recv() {
            latest = Some(text);
        }
        if let Some(text) = latest {
            if this
                .update(cx, |view, cx| {
                    view.terminal_text = text;
                    cx.notify();
                })
                .is_err()
            {
                break;
            }
        }
    })
    .detach();
}

fn terminal_bytes(key: &Keystroke) -> Option<Vec<u8>> {
    if key.modifiers.platform {
        return None;
    }
    if key.modifiers.control {
        let text = key.key_char.as_deref().or(Some(key.key.as_str()))?;
        let byte = text.as_bytes().first().copied()?;
        return Some(vec![byte & 0x1f]);
    }
    match key.key.as_str() {
        "enter" => Some(b"\r".to_vec()),
        "backspace" => Some(vec![0x7f]),
        "tab" => Some(b"\t".to_vec()),
        "escape" => Some(vec![0x1b]),
        "up" => Some(b"\x1b[A".to_vec()),
        "down" => Some(b"\x1b[B".to_vec()),
        "right" => Some(b"\x1b[C".to_vec()),
        "left" => Some(b"\x1b[D".to_vec()),
        _ => key.key_char.as_ref().map(|text| text.as_bytes().to_vec()),
    }
}

fn key_name(key: &Keystroke) -> &str {
    match key.key.as_str() {
        "enter" => "enter",
        "backspace" => "backspace",
        "tab" => "tab",
        "escape" => "escape",
        "up" => "up",
        "down" => "down",
        "right" => "right",
        "left" => "left",
        _ => key.key.as_str(),
    }
}
