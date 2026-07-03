mod ghostty;
mod herdr;

use crepuscularity_gpui as gpui;
use crepuscularity_gpui::prelude::*;
use crepuscularity_gpui::{
    actions, bounds, div, gpui_window_options, point, px, rgb, size, AnyElement, App, Application,
    Context, FocusHandle, IntoElement, KeyBinding, Keystroke, Menu, MenuItem, MouseButton, Render,
    ScrollWheelEvent, SystemMenuType, TitlebarOptions, Window, WindowBounds,
};
use ghostty::{TerminalFrame, TerminalLine, TerminalRun, TerminalSession};
use herdr::{HerdrClient, HerdrState, Pane, Tab, Workspace};
use std::{
    sync::mpsc::{Receiver, TryRecvError},
    time::Duration,
};

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
        NextTab,
        NewTab,
        CloseTab,
        NewWorkspace,
        PreviousWorkspace,
        NextWorkspace,
        ToggleSpaces,
        ToggleSidebar,
        ToggleAgents,
        ToggleSidebarLayout
    ]
);

#[derive(Clone, Copy, Eq, PartialEq)]
enum SidebarLayout {
    Warp,
    Arc,
}

struct HerdrGui {
    client: Option<HerdrClient>,
    terminal: Option<TerminalSession>,
    terminal_target: Option<String>,
    terminal_size: Option<TerminalSize>,
    terminal_token: u64,
    terminal_frame: TerminalFrame,
    state: HerdrState,
    status: String,
    show_help: bool,
    show_spaces: bool,
    sidebar_collapsed: bool,
    sidebar_hovered: bool,
    agents_collapsed: bool,
    sidebar_layout: SidebarLayout,
    scroll_x: f64,
    focus_handle: FocusHandle,
}

type TerminalSize = (u16, u16, u16, u16);

impl HerdrGui {
    fn new(cx: &mut Context<Self>) -> Self {
        let (client, state, status) = match HerdrClient::bootstrap() {
            Ok(client) => match client.state() {
                Ok(state) => (Some(client), state, "connected".to_string()),
                Err(err) => (Some(client), HerdrState::default(), err.to_string()),
            },
            Err(err) => (None, HerdrState::default(), err.to_string()),
        };
        Self {
            client,
            terminal: None,
            terminal_target: None,
            terminal_size: None,
            terminal_token: 0,
            terminal_frame: TerminalFrame::default(),
            state,
            status,
            show_help: false,
            show_spaces: false,
            sidebar_collapsed: false,
            sidebar_hovered: false,
            agents_collapsed: false,
            sidebar_layout: SidebarLayout::Arc,
            scroll_x: 0.0,
            focus_handle: cx.focus_handle(),
        }
    }

    fn toggle_help(&mut self, _: &ToggleHelp, _window: &mut Window, cx: &mut Context<Self>) {
        self.show_help = !self.show_help;
        cx.notify();
    }

    fn toggle_sidebar_layout(
        &mut self,
        _: &ToggleSidebarLayout,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sidebar_layout = match self.sidebar_layout {
            SidebarLayout::Warp => SidebarLayout::Arc,
            SidebarLayout::Arc => SidebarLayout::Warp,
        };
        cx.notify();
    }

    fn toggle_spaces(&mut self, _: &ToggleSpaces, _window: &mut Window, cx: &mut Context<Self>) {
        self.show_spaces = !self.show_spaces;
        cx.notify();
    }

    fn toggle_sidebar(&mut self, _: &ToggleSidebar, _window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_collapsed = !self.sidebar_collapsed;
        cx.notify();
    }

    fn toggle_agents(&mut self, _: &ToggleAgents, _window: &mut Window, cx: &mut Context<Self>) {
        self.agents_collapsed = !self.agents_collapsed;
        cx.notify();
    }

    fn refresh(&mut self, _: &Refresh, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_state();
        self.attach_focused_terminal(window, cx);
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

    fn focus_right(&mut self, _: &FocusRight, window: &mut Window, cx: &mut Context<Self>) {
        self.with_client(HerdrClient::focus_right);
        self.refresh_state();
        self.attach_focused_terminal(window, cx);
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

    fn previous_tab(&mut self, _: &PreviousTab, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_tab_offset(-1, window, cx);
    }

    fn next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_tab_offset(1, window, cx);
    }

    fn new_tab(&mut self, _: &NewTab, window: &mut Window, cx: &mut Context<Self>) {
        let workspace_id = self.active_workspace_id().map(str::to_string);
        self.with_client(|client| client.create_tab(workspace_id.as_deref()));
        self.refresh_state();
        self.attach_focused_terminal(window, cx);
        cx.notify();
    }

    fn close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        let tab_id = self.active_tab().map(|tab| tab.tab_id.clone());
        if let Some(tab_id) = tab_id {
            self.with_client(|client| client.close_tab(&tab_id));
            self.refresh_state();
            self.attach_focused_terminal(window, cx);
            cx.notify();
        }
    }

    fn new_workspace(&mut self, _: &NewWorkspace, window: &mut Window, cx: &mut Context<Self>) {
        self.with_client(HerdrClient::create_workspace);
        self.refresh_state();
        self.attach_focused_terminal(window, cx);
        cx.notify();
    }

    fn previous_workspace(
        &mut self,
        _: &PreviousWorkspace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_workspace_offset(-1, window, cx);
    }

    fn next_workspace(&mut self, _: &NextWorkspace, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_workspace_offset(1, window, cx);
    }

    fn focus_workspace_id(
        &mut self,
        workspace_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_spaces = false;
        self.with_client(|client| client.focus_workspace(&workspace_id));
        self.refresh_state();
        self.attach_focused_terminal(window, cx);
        cx.notify();
    }

    fn focus_tab_id(&mut self, tab_id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.with_client(|client| client.focus_tab(&tab_id));
        self.refresh_state();
        self.attach_focused_terminal(window, cx);
        cx.notify();
    }

    fn focus_tab_offset(&mut self, offset: isize, window: &mut Window, cx: &mut Context<Self>) {
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
        self.focus_tab_id(tabs[next_index].tab_id.clone(), window, cx);
    }

    fn focus_workspace_offset(
        &mut self,
        offset: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.workspaces.is_empty() {
            return;
        }
        let active_id = self.state.focused_workspace_id.as_deref().or_else(|| {
            self.active_workspace()
                .map(|workspace| workspace.workspace_id.as_str())
        });
        let active_index = active_id
            .and_then(|id| {
                self.state
                    .workspaces
                    .iter()
                    .position(|workspace| workspace.workspace_id == id)
            })
            .unwrap_or(0);
        let next_index = (active_index as isize + offset)
            .rem_euclid(self.state.workspaces.len() as isize) as usize;
        self.focus_workspace_id(
            self.state.workspaces[next_index].workspace_id.clone(),
            window,
            cx,
        );
    }

    fn focus_pane_id(&mut self, pane_id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.with_client(|client| client.focus_pane(&pane_id));
        self.refresh_state();
        self.attach_focused_terminal(window, cx);
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

    fn attach_focused_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let size = terminal_size(window);
        let target = self
            .focused_pane()
            .and_then(|pane| pane.terminal_id.clone())
            .or_else(|| self.focused_pane().map(|pane| pane.pane_id.clone()));
        if target.is_none() {
            return;
        }
        let target = match target {
            Some(target) => target,
            None => return,
        };
        if self.terminal_target.as_deref() == Some(target.as_str()) {
            if self.terminal_size != Some(size) {
                if let Some(terminal) = &self.terminal {
                    terminal.resize(size.0, size.1, size.2, size.3);
                }
                self.terminal_size = Some(size);
            }
            return;
        }
        match TerminalSession::attach(&target, size.0, size.1) {
            Ok(mut session) => {
                if let Some(receiver) = session.output.take() {
                    self.terminal_token = self.terminal_token.wrapping_add(1);
                    let token = self.terminal_token;
                    self.terminal = Some(session);
                    self.terminal_target = Some(target);
                    self.terminal_size = Some(size);
                    self.status = "connected".to_string();
                    poll_terminal(receiver, token, cx);
                }
            }
            Err(err) => {
                self.terminal = None;
                self.terminal_target = None;
                self.terminal_size = None;
                self.terminal_frame = TerminalFrame {
                    lines: vec![TerminalLine {
                        runs: vec![TerminalRun {
                            text: err.clone(),
                            fg: 0xfca5a5,
                            bg: None,
                        }],
                    }],
                };
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

    fn handle_workspace_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let delta = event.delta.pixel_delta(px(18.0));
        if delta.x.abs() <= delta.y.abs() {
            return;
        }
        self.scroll_x += delta.x.to_f64();
        if self.scroll_x.abs() < 80.0 {
            return;
        }
        let offset = if self.scroll_x < 0.0 { 1 } else { -1 };
        self.scroll_x = 0.0;
        self.focus_workspace_offset(offset, window, cx);
    }
}

impl Render for HerdrGui {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.attach_focused_terminal(window, cx);
        let panes = self.visible_panes();
        let pane_frame = self.terminal_frame.clone();

        div()
            .w_full()
            .h_full()
            .bg(rgb(0x0b0b0b))
            .text_color(rgb(0xf2f2f2))
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
            .on_action(cx.listener(Self::new_tab))
            .on_action(cx.listener(Self::close_tab))
            .on_action(cx.listener(Self::new_workspace))
            .on_action(cx.listener(Self::previous_workspace))
            .on_action(cx.listener(Self::next_workspace))
            .on_action(cx.listener(Self::toggle_spaces))
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(Self::toggle_agents))
            .on_action(cx.listener(Self::toggle_sidebar_layout))
            .on_scroll_wheel(cx.listener(Self::handle_workspace_scroll))
            .child(self.sidebar(cx))
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .bg(rgb(0x080808))
                    .overflow_hidden()
                    .child(self.pane_grid(panes, pane_frame, cx))
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

    fn agent_panes(&self) -> Vec<Pane> {
        self.state
            .panes
            .iter()
            .filter(|pane| pane.agent.is_some())
            .cloned()
            .collect()
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(if self.sidebar_collapsed && !self.sidebar_hovered {
                px(6.0)
            } else {
                px(210.0)
            })
            .h_full()
            .border_r_1()
            .border_color(rgb(0x181818))
            .bg(rgb(0x0b0b0b))
            .p_2()
            .flex()
            .flex_col()
            .gap_2()
            .overflow_hidden()
            .when(self.sidebar_collapsed && !self.sidebar_hovered, |el| el)
            .child(space_switcher(self.active_workspace(), cx))
            .when(self.show_spaces, |el| {
                el.child(
                    div().flex().flex_col().gap_2().children(
                        self.state.workspaces.iter().map(|workspace| {
                            workspace_row(workspace, self.active_workspace_id(), cx)
                        }),
                    ),
                )
            })
            .when(self.sidebar_layout == SidebarLayout::Warp, |el| {
                el.child(self.warp_sidebar(cx))
            })
            .when(self.sidebar_layout == SidebarLayout::Arc, |el| {
                el.child(self.arc_sidebar(cx))
            })
            .into_any_element()
    }

    fn warp_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_full()
            .flex()
            .flex_col()
            .gap_2()
            .child(section("sessions"))
            .children(
                self.state
                    .workspaces
                    .iter()
                    .map(|workspace| workspace_row(workspace, self.active_workspace_id(), cx)),
            )
            .child(div().flex_1())
            .child(agent_header(cx))
            .when(!self.agents_collapsed, |el| {
                el.children(
                    self.agent_panes()
                        .into_iter()
                        .map(|pane| agent_row(&pane, &self.state, cx)),
                )
            })
    }

    fn arc_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_full()
            .flex()
            .flex_col()
            .gap_2()
            .child(tab_header(cx))
            .children(self.visible_tabs().into_iter().map(|tab| {
                let id = tab.tab_id.clone();
                let close_id = tab.tab_id.clone();
                tab_row(
                    tab.label.as_deref().unwrap_or(&tab.tab_id).to_string(),
                    tab.agent_status.unwrap_or_else(|| "tab".to_string()),
                    tab.focused
                        || self
                            .state
                            .focused_tab_id
                            .as_deref()
                            .is_some_and(|focused| focused == tab.tab_id),
                    cx.listener(move |this, _, window, cx| {
                        this.focus_tab_id(id.clone(), window, cx)
                    }),
                    cx.listener(move |this, _, window, cx| {
                        this.with_client(|client| client.close_tab(&close_id));
                        this.refresh_state();
                        this.attach_focused_terminal(window, cx);
                        cx.notify();
                    }),
                )
            }))
            .child(div().flex_1())
            .child(agent_header(cx))
            .when(!self.agents_collapsed, |el| {
                el.children(
                    self.agent_panes()
                        .into_iter()
                        .map(|pane| agent_row(&pane, &self.state, cx)),
                )
            })
    }

    fn pane_grid(
        &self,
        panes: Vec<Pane>,
        pane_frame: TerminalFrame,
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

        let pane = panes
            .iter()
            .find(|pane| pane.focused)
            .or_else(|| panes.first())
            .cloned();
        let Some(pane) = pane else {
            return div().flex().flex_1().h_full().bg(rgb(0x080808));
        };
        let pane_id = pane.pane_id.clone();
        div()
            .flex()
            .flex_1()
            .h_full()
            .bg(rgb(0x080808))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    this.focus_pane_id(pane_id.clone(), window, cx)
                }),
            )
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .text_size(px(12.0))
                    .font_family("Menlo")
                    .line_height(px(18.0))
                    .text_color(rgb(0xc5ceda))
                    .child(terminal_frame(&pane_frame)),
            )
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
        .text_color(rgb(0x8a8a8a))
        .child(text.to_string())
}

fn section(text: &str) -> impl IntoElement {
    div()
        .pt_1()
        .text_size(px(10.0))
        .text_color(rgb(0x777777))
        .child(text.to_string())
}

fn space_switcher(workspace: Option<&Workspace>, cx: &mut Context<HerdrGui>) -> impl IntoElement {
    let name = workspace
        .and_then(|workspace| workspace.label.as_deref())
        .or_else(|| workspace.map(|workspace| workspace.workspace_id.as_str()))
        .unwrap_or("space");
    div()
        .h(px(28.0))
        .flex()
        .items_center()
        .gap_2()
        .pl(px(54.0))
        .child(
            div()
                .flex_1()
                .h(px(26.0))
                .px_2()
                .flex()
                .items_center()
                .justify_between()
                .cursor_pointer()
                .hover(|style| style.bg(rgb(0x181818)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.toggle_spaces(&ToggleSpaces, window, cx)
                    }),
                )
                .child(label(name, 0xf0f0f0))
                .child(small("⌄")),
        )
        .child(
            div()
                .w(px(26.0))
                .h(px(26.0))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(14.0))
                .text_color(rgb(0xd0d0d0))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(0x181818)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.new_workspace(&NewWorkspace, window, cx)
                    }),
                )
                .child("+"),
        )
}

fn tab_header(cx: &mut Context<HerdrGui>) -> impl IntoElement {
    div()
        .pt_1()
        .flex()
        .items_center()
        .justify_between()
        .child(section("tabs"))
        .child(
            div()
                .w(px(22.0))
                .h(px(22.0))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(14.0))
                .text_color(rgb(0xd0d0d0))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(0x181818)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.new_tab(&NewTab, window, cx)),
                )
                .child("+"),
        )
}

fn agent_header(cx: &mut Context<HerdrGui>) -> impl IntoElement {
    div()
        .pt_1()
        .flex()
        .items_center()
        .justify_between()
        .child(section("agents"))
        .child(
            div()
                .w(px(22.0))
                .h(px(22.0))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(14.0))
                .text_color(rgb(0xd0d0d0))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(0x181818)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.toggle_agents(&ToggleAgents, window, cx)
                    }),
                )
                .child("⌃"),
        )
}

fn workspace_row(
    workspace: &Workspace,
    active_workspace_id: Option<&str>,
    cx: &mut Context<HerdrGui>,
) -> AnyElement {
    let id = workspace.workspace_id.clone();
    let title = workspace
        .label
        .as_deref()
        .unwrap_or(&workspace.workspace_id)
        .to_string();
    let detail = workspace.cwd.as_deref().unwrap_or("~").to_string();
    let focused = workspace.focused
        || active_workspace_id.is_some_and(|focused| focused == workspace.workspace_id);
    let on_click =
        cx.listener(move |this, _, window, cx| this.focus_workspace_id(id.clone(), window, cx));

    row(title, detail, focused, on_click).into_any_element()
}

fn agent_row(pane: &Pane, state: &HerdrState, cx: &mut Context<HerdrGui>) -> AnyElement {
    let id = pane.pane_id.clone();
    let title = pane.agent.as_deref().unwrap_or(&pane.pane_id).to_string();
    let status = pane
        .agent_status
        .as_deref()
        .unwrap_or("unknown")
        .to_string();
    let focused = pane.focused
        || state
            .focused_pane_id
            .as_deref()
            .is_some_and(|focused| focused == pane.pane_id);
    let on_click =
        cx.listener(move |this, _, window, cx| this.focus_pane_id(id.clone(), window, cx));

    div()
        .bg(if focused {
            rgb(0x2a2a2a)
        } else {
            rgb(0x000000)
        })
        .px_3()
        .py_2()
        .flex()
        .items_center()
        .justify_between()
        .cursor_pointer()
        .hover(|style| style.bg(rgb(0x181818)))
        .on_mouse_down(MouseButton::Left, on_click)
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(label(&title, 0xf0f0f0))
                .child(small(&status)),
        )
        .child(div().w(px(8.0)).h(px(8.0)).bg(rgb(status_color(&status))))
        .into_any_element()
}

fn status_color(status: &str) -> u32 {
    match status {
        "working" => 0xffffff,
        "blocked" => 0xd0d0d0,
        "done" => 0xa0a0a0,
        "idle" => 0x777777,
        _ => 0x555555,
    }
}

fn row(
    title: String,
    detail: String,
    focused: bool,
    on_click: impl Fn(&crepuscularity_gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .bg(if focused {
            rgb(0x2a2a2a)
        } else {
            rgb(0x000000)
        })
        .px_3()
        .py_2()
        .flex()
        .flex_col()
        .gap_0p5()
        .cursor_pointer()
        .hover(|style| style.bg(rgb(0x181818)))
        .on_mouse_down(MouseButton::Left, on_click)
        .child(label(&title, 0xf0f0f0))
        .child(small(&detail))
}

fn tab_row(
    title: String,
    detail: String,
    focused: bool,
    on_click: impl Fn(&crepuscularity_gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&crepuscularity_gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .bg(if focused {
            rgb(0x2a2a2a)
        } else {
            rgb(0x000000)
        })
        .px_3()
        .py_2()
        .flex()
        .items_center()
        .justify_between()
        .cursor_pointer()
        .hover(|style| style.bg(rgb(0x181818)))
        .on_mouse_down(MouseButton::Left, on_click)
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(label(&title, 0xf0f0f0))
                .child(small(&detail)),
        )
        .child(
            div()
                .w(px(18.0))
                .h(px(18.0))
                .flex()
                .items_center()
                .justify_center()
                .text_color(rgb(0xaaaaaa))
                .hover(|style| style.text_color(rgb(0xffffff)))
                .on_mouse_down(MouseButton::Left, on_close)
                .child("x"),
        )
}

fn empty_state(status: &str) -> impl IntoElement {
    div()
        .w(px(560.0))
        .rounded_lg()
        .bg(rgb(0x111111))
        .border_1()
        .border_color(rgb(0x303030))
        .p_5()
        .flex()
        .flex_col()
        .gap_3()
        .child(label("No Herdr panes visible", 0xf8fafc))
        .child(small(status))
        .child(
            div()
                .rounded_lg()
                .bg(rgb(0x080808))
                .border_1()
                .border_color(rgb(0x222222))
                .p_3()
                .font_family("Menlo")
                .text_size(px(12.0))
                .text_color(rgb(0xb8b8b8))
                .child("Open Herdr in a terminal, create a workspace/pane, then press Refresh."),
        )
}

fn terminal_frame(frame: &TerminalFrame) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .children(frame.lines.iter().map(terminal_line))
}

fn terminal_line(line: &TerminalLine) -> impl IntoElement {
    div()
        .h(px(18.0))
        .flex()
        .flex_none()
        .children(line.runs.iter().map(terminal_run))
}

fn terminal_run(run: &TerminalRun) -> impl IntoElement {
    div()
        .text_color(rgb(run.fg))
        .when_some(run.bg, |el, bg| el.bg(rgb(bg)))
        .child(run.text.replace(' ', "\u{00a0}"))
}

fn terminal_size(window: &Window) -> TerminalSize {
    let size = window.bounds().size;
    let width = (size.width.to_f64() - 210.0).max(320.0);
    let height = size.height.to_f64().max(240.0);
    let cols = (width / 8.0).floor().clamp(40.0, 260.0) as u16;
    let rows = (height / 18.0).floor().clamp(12.0, 140.0) as u16;
    (cols, rows, width.round() as u16, height.round() as u16)
}

fn kbd_hint(text: &str) -> impl IntoElement {
    div()
        .rounded_sm()
        .bg(rgb(0x202020))
        .border_1()
        .border_color(rgb(0x383838))
        .px_2()
        .py_1()
        .text_size(px(11.0))
        .text_color(rgb(0xd0d0d0))
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
        .bg(rgb(0x101010))
        .border_1()
        .border_color(rgb(0x303030))
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
        cx.set_menus(vec![
            Menu {
                name: "Herdr".into(),
                items: vec![
                    MenuItem::os_submenu("Services", SystemMenuType::Services),
                    MenuItem::separator(),
                    MenuItem::action("Refresh", Refresh),
                    MenuItem::action("New Workspace", NewWorkspace),
                    MenuItem::action("New Tab", NewTab),
                    MenuItem::action("Close Tab", CloseTab),
                    MenuItem::action("Previous Workspace", PreviousWorkspace),
                    MenuItem::action("Next Workspace", NextWorkspace),
                ],
            },
            Menu {
                name: "Settings".into(),
                items: vec![
                    MenuItem::action("Toggle Help", ToggleHelp),
                    MenuItem::action("Toggle Spaces", ToggleSpaces),
                    MenuItem::action("Toggle Sidebar", ToggleSidebar),
                    MenuItem::action("Toggle Agents", ToggleAgents),
                    MenuItem::action("Toggle Sidebar Layout", ToggleSidebarLayout),
                ],
            },
        ]);

        cx.bind_keys([
            KeyBinding::new("f1", ToggleHelp, None),
            KeyBinding::new("cmd-r", Refresh, None),
            KeyBinding::new("cmd-shift-s", ToggleSpaces, None),
            KeyBinding::new("cmd-b", ToggleSidebar, None),
            KeyBinding::new("cmd-shift-a", ToggleAgents, None),
            KeyBinding::new("cmd-shift-l", ToggleSidebarLayout, None),
            KeyBinding::new("cmd-t", NewTab, None),
            KeyBinding::new("cmd-w", CloseTab, None),
            KeyBinding::new("cmd-]", SplitRight, None),
            KeyBinding::new("cmd-shift-]", SplitDown, None),
            KeyBinding::new("right", FocusRight, None),
            KeyBinding::new("shift-right", ResizeRight, None),
            KeyBinding::new("cmd-shift-w", ClosePane, None),
            KeyBinding::new("cmd-left", PreviousTab, None),
            KeyBinding::new("cmd-right", NextTab, None),
            KeyBinding::new("cmd-shift-left", PreviousWorkspace, None),
            KeyBinding::new("cmd-shift-right", NextWorkspace, None),
        ]);

        let mut options = gpui_window_options(
            "dev.undivisible.herdr-gui",
            "",
            Some(WindowBounds::Windowed(bounds(
                point(px(80.0), px(80.0)),
                size(px(1280.0), px(820.0)),
            ))),
            Some(size(px(920.0), px(600.0))),
        );
        options.titlebar = Some(TitlebarOptions {
            title: None,
            appears_transparent: true,
            traffic_light_position: Some(point(px(12.0), px(12.0))),
        });
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

fn poll_terminal(receiver: Receiver<TerminalFrame>, token: u64, cx: &mut Context<HerdrGui>) {
    cx.spawn(async move |this, cx| loop {
        cx.background_executor()
            .timer(Duration::from_millis(33))
            .await;
        let mut latest = None;
        let mut disconnected = false;
        loop {
            match receiver.try_recv() {
                Ok(text) => latest = Some(text),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        if let Some(frame) = latest {
            if this
                .update(cx, |view, cx| {
                    if view.terminal_token != token {
                        return;
                    }
                    view.terminal_frame = frame;
                    cx.notify();
                })
                .is_err()
            {
                break;
            }
        }
        if disconnected {
            let _ = this.update(cx, |view, cx| {
                if view.terminal_token != token {
                    return;
                }
                view.terminal = None;
                view.terminal_target = None;
                view.terminal_size = None;
                view.refresh_state();
                cx.notify();
            });
            break;
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
