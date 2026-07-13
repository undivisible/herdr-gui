mod ghostty;
mod help;
mod herdr;
mod input;
mod settings;
mod terminal_view;
mod theme;

use crepuscularity_gpui as gpui;
use crepuscularity_gpui::prelude::*;
use crepuscularity_gpui::{
    actions, bounce, bounds, div, gpui_window_options, linear, point, px, rgb, size, AnyElement,
    AnyWindowHandle, App, Application, Context, Entity, FocusHandle, Icon, IntoElement, KeyBinding,
    Keystroke, Menu, MenuItem, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Render,
    ScrollWheelEvent, SystemMenuType, TitlebarOptions, Window, WindowAppearance, WindowBounds,
};
use ghostty::{TerminalFrame, TerminalLine, TerminalRun, TerminalSession};
use help::help_overlay;
use herdr::{Agent, HerdrClient, HerdrState, Pane, Tab, Workspace};
use input::key_name;
use std::sync::{Arc, Mutex};
use std::{
    sync::mpsc::{Receiver, TryRecvError},
    time::{Duration, Instant},
};
use terminal_view::{cached_terminal, TerminalPane};
use theme::{agent_status_accent, agent_status_background, herdr_theme, status_color, UiTheme};

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
        ToggleSidebarLayout,
        ThemeCatppuccin,
        ThemeCatppuccinLatte,
        ThemeTerminal,
        ThemeTokyoNight,
        ThemeTokyoNightDay,
        ThemeDracula,
        ThemeNord,
        ThemeGruvbox,
        ThemeGruvboxLight,
        ThemeOneDark,
        ThemeOneLight,
        ThemeSolarized,
        ThemeSolarizedLight,
        ThemeKanagawa,
        ThemeKanagawaLotus,
        ThemeRosePine,
        ThemeRosePineDawn,
        ThemeVesper,
        ThemeOled,
        ThemeSystem,
        ThemeSystemDark,
        ThemeSystemLight,
        ReloadHerdrConfig
    ]
);

macro_rules! set_theme {
    ($name:ident, $action:ty, $theme:literal) => {
        fn $name(&mut self, _: &$action, _window: &mut Window, cx: &mut Context<Self>) {
            self.set_theme($theme.to_string(), cx);
        }
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SidebarLayout {
    Warp,
    Arc,
}

#[derive(Clone, Eq, PartialEq)]
enum ThemeMode {
    Herdr(String),
    System,
    SystemDark,
    SystemLight,
}

struct HerdrGui {
    client: Option<HerdrClient>,
    terminal: Option<Arc<Mutex<TerminalSession>>>,
    terminal_target: Option<String>,
    terminal_size: Option<TerminalSize>,
    terminal_token: u64,
    terminal_frame: Arc<TerminalFrame>,
    terminal_pane: Entity<TerminalPane>,
    last_terminal_frame_at: Option<Instant>,
    terminal_pending_frame: bool,
    terminal_frame_in_flight: bool,
    terminal_bg: u32,
    state: HerdrState,
    status: String,
    show_help: bool,
    show_spaces: bool,
    sidebar_collapsed: bool,
    agents_collapsed: bool,
    sidebar_layout: SidebarLayout,
    sidebar_resizing: bool,
    sidebar_animation: SidebarAnimation,
    theme_mode: ThemeMode,
    swipe_progress: f64,
    scroll_x: f64,
    focus_handle: FocusHandle,
    settings: settings::Settings,
}

#[derive(Clone, Copy, Debug)]
struct SidebarAnimation {
    width: f64,
    start: f64,
    target: f64,
    hovered: bool,
}

impl SidebarAnimation {
    fn new(width: f64) -> Self {
        Self {
            width,
            start: width,
            target: width,
            hovered: false,
        }
    }
}

const SIDEBAR_MIN_WIDTH: f64 = 180.0;
const SIDEBAR_MAX_WIDTH: f64 = 360.0;
const SIDEBAR_COLLAPSED_WIDTH: f64 = 6.0;
const TERMINAL_MIN_WIDTH: f64 = 320.0;
const TERMINAL_MIN_HEIGHT: f64 = 240.0;
const TERMINAL_CELL_WIDTH: f64 = 7.2;
const TERMINAL_CELL_HEIGHT: f64 = 18.0;
const TERMINAL_MIN_COLS: f64 = 40.0;
const TERMINAL_MAX_COLS: f64 = 500.0;
const TERMINAL_MIN_ROWS: f64 = 12.0;
const TERMINAL_MAX_ROWS: f64 = 180.0;
const RESIZE_HANDLE_WIDTH: f64 = 4.0;
const TOP_TAB_BAR_HEIGHT: f64 = 34.0;
const TRAFFIC_LIGHT_PADDING: f64 = 40.0;

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
        let settings = settings::Settings::load();
        let sidebar_layout = match settings.sidebar_layout.as_str() {
            "warp" => SidebarLayout::Warp,
            _ => SidebarLayout::Arc,
        };
        let theme_mode = match settings.theme.as_str() {
            "system" => ThemeMode::System,
            "system-dark" => ThemeMode::SystemDark,
            "system-light" => ThemeMode::SystemLight,
            name => ThemeMode::Herdr(name.to_string()),
        };
        let sidebar_width = settings
            .sidebar_width
            .clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
        Self {
            client,
            terminal: None,
            terminal_target: None,
            terminal_size: None,
            terminal_token: 0,
            terminal_frame: Arc::new(TerminalFrame::default()),
            terminal_pane: cx.new(TerminalPane::new),
            last_terminal_frame_at: None,
            terminal_pending_frame: false,
            terminal_frame_in_flight: false,
            terminal_bg: 0x0a0a0a,
            state,
            status,
            show_help: false,
            show_spaces: false,
            sidebar_collapsed: settings.sidebar_collapsed,
            agents_collapsed: settings.agents_collapsed,
            sidebar_layout,
            sidebar_resizing: false,
            sidebar_animation: SidebarAnimation::new(sidebar_width),
            theme_mode,
            swipe_progress: 0.0,
            scroll_x: 0.0,
            focus_handle: cx.focus_handle(),
            settings,
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
        self.show_spaces = false;
        self.save_settings();
        cx.notify();
    }

    fn toggle_spaces(&mut self, _: &ToggleSpaces, _window: &mut Window, cx: &mut Context<Self>) {
        // Ephemeral UI chrome — do not block the click path on settings I/O.
        let started = Instant::now();
        self.show_spaces = !self.show_spaces;
        cx.notify();
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        if ms > 5.0 {
            eprintln!(
                "toggle_spaces handler {ms:.1}ms show_spaces={}",
                self.show_spaces
            );
        }
    }

    fn toggle_sidebar(&mut self, _: &ToggleSidebar, _window: &mut Window, cx: &mut Context<Self>) {
        self.transition_sidebar_width(
            |this| {
                this.sidebar_animation.hovered = false;
                this.sidebar_collapsed = !this.sidebar_collapsed;
            },
            cx,
        );
    }

    fn transition_sidebar_width<F>(&mut self, f: F, cx: &mut Context<Self>)
    where
        F: FnOnce(&mut Self),
    {
        let old_width = self.sidebar_width();
        f(self);
        let new_width = self.sidebar_width();
        self.sidebar_animation.start = old_width;
        self.sidebar_animation.target = new_width;
        self.save_settings();
        cx.notify();
    }

    fn toggle_agents(&mut self, _: &ToggleAgents, _window: &mut Window, cx: &mut Context<Self>) {
        self.agents_collapsed = !self.agents_collapsed;
        self.save_settings();
        cx.notify();
    }

    fn theme_system(&mut self, _: &ThemeSystem, _window: &mut Window, cx: &mut Context<Self>) {
        self.theme_mode = ThemeMode::System;
        self.save_settings();
        cx.notify();
    }

    fn theme_system_dark(
        &mut self,
        _: &ThemeSystemDark,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.theme_mode = ThemeMode::SystemDark;
        self.save_settings();
        cx.notify();
    }

    fn theme_system_light(
        &mut self,
        _: &ThemeSystemLight,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.theme_mode = ThemeMode::SystemLight;
        self.save_settings();
        cx.notify();
    }

    fn reload_herdr_config(
        &mut self,
        _: &ReloadHerdrConfig,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.with_client(HerdrClient::reload_config);
        self.refresh_state();
        cx.notify();
    }

    fn save_settings(&mut self) {
        self.settings.theme = match &self.theme_mode {
            ThemeMode::System => "system".to_string(),
            ThemeMode::SystemDark => "system-dark".to_string(),
            ThemeMode::SystemLight => "system-light".to_string(),
            ThemeMode::Herdr(name) => name.clone(),
        };
        self.settings.sidebar_layout = match self.sidebar_layout {
            SidebarLayout::Warp => "warp".to_string(),
            SidebarLayout::Arc => "arc".to_string(),
        };
        self.settings.sidebar_width = self.sidebar_animation.width;
        self.settings.sidebar_collapsed = self.sidebar_collapsed;
        self.settings.show_spaces = false;
        self.settings.agents_collapsed = self.agents_collapsed;
        self.settings.save();
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

    fn close_pane(&mut self, _: &ClosePane, window: &mut Window, cx: &mut Context<Self>) {
        self.with_first_pane(|client, pane| client.close_pane(&pane.pane_id));
        self.refresh_state();
        self.close_active_workspace_if_empty();
        self.attach_focused_terminal(window, cx);
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

    fn close_workspace_id(
        &mut self,
        workspace_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.with_client(|client| client.close_workspace(&workspace_id));
        self.refresh_state();
        self.terminal_target = None;
        self.terminal_size = None;
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

    fn focus_target(
        &mut self,
        workspace_id: Option<String>,
        tab_id: Option<String>,
        pane_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(workspace_id) = workspace_id {
            self.with_client(|client| client.focus_workspace(&workspace_id));
        }
        if let Some(tab_id) = tab_id {
            self.with_client(|client| client.focus_tab(&tab_id));
        }
        if let Some(pane_id) = pane_id {
            self.with_client(|client| client.focus_pane(&pane_id));
        }
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

    fn set_terminal_frame(&mut self, frame: Arc<TerminalFrame>, cx: &mut Context<Self>) {
        if Arc::ptr_eq(&self.terminal_frame, &frame)
            || self.terminal_frame.as_ref() == frame.as_ref()
        {
            return;
        }
        self.terminal_frame = frame.clone();
        self.last_terminal_frame_at = Some(Instant::now());
        self.terminal_pending_frame = false;
        self.terminal_pane
            .update(cx, |pane, cx| pane.set_frame(frame, cx));
    }

    fn maybe_refresh_terminal_frame(&mut self, cx: &mut Context<Self>, force: bool) {
        let Some(terminal) = self.terminal.clone() else {
            return;
        };
        const FRAME_MIN_INTERVAL: Duration = Duration::from_millis(50);
        let due = force
            || self
                .last_terminal_frame_at
                .is_none_or(|at| at.elapsed() >= FRAME_MIN_INTERVAL);
        if !due || self.terminal_frame_in_flight {
            self.terminal_pending_frame = true;
            return;
        }
        // Extract off the UI thread — ghostty frame() walks every cell via FFI.
        self.terminal_frame_in_flight = true;
        self.terminal_pending_frame = false;
        self.last_terminal_frame_at = Some(Instant::now());
        let token = self.terminal_token;
        cx.spawn(async move |this, cx| {
            let started = Instant::now();
            let frame = cx
                .background_executor()
                .spawn(async move {
                    terminal
                        .lock()
                        .map_err(|err| err.to_string())
                        .and_then(|mut terminal| terminal.frame())
                })
                .await;
            let ms = started.elapsed().as_secs_f64() * 1000.0;
            if ms > 20.0 {
                eprintln!("terminal.frame extract {ms:.1}ms (bg)");
            }
            let _ = this.update(cx, |view, cx| {
                view.terminal_frame_in_flight = false;
                if view.terminal_token != token {
                    return;
                }
                if let Ok(frame) = frame {
                    view.set_terminal_frame(Arc::new(frame), cx);
                }
                if view.terminal_pending_frame {
                    view.maybe_refresh_terminal_frame(cx, false);
                }
            });
        })
        .detach();
    }

    fn attach_focused_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let size = self.terminal_size(window);
        let pane = self.focused_pane().cloned();
        let target = pane
            .as_ref()
            .and_then(|pane| pane.terminal_id.clone())
            .or_else(|| pane.as_ref().map(|pane| pane.pane_id.clone()));
        if target.is_none() {
            return;
        }
        let target = match target {
            Some(target) => target,
            None => return,
        };
        if self.terminal_target.as_deref() == Some(target.as_str()) {
            if self.terminal_size != Some(size) {
                if let Some(terminal) = self.terminal.clone() {
                    let frame = terminal.lock().ok().and_then(|mut terminal| {
                        terminal.resize(size.0, size.1, size.2, size.3).ok()
                    });
                    if let Some(frame) = frame {
                        self.set_terminal_frame(Arc::new(frame), cx);
                    }
                }
                self.terminal_size = Some(size);
            }
            return;
        }
        match TerminalSession::attach(&target, size.0, size.1) {
            Ok(mut session) => {
                if let Ok(frame) = session.resize(size.0, size.1, size.2, size.3) {
                    self.set_terminal_frame(Arc::new(frame), cx);
                }
                if let Some(receiver) = session.output.take() {
                    self.terminal_token = self.terminal_token.wrapping_add(1);
                    let token = self.terminal_token;
                    if self.terminal_frame.lines.is_empty() {
                        if let (Some(client), Some(pane)) = (&self.client, pane.as_ref()) {
                            if let Ok(ansi) = client.read_pane_ansi(&pane.pane_id) {
                                if let Ok(frame) = TerminalFrame::from_ansi(size.0, size.1, &ansi) {
                                    self.set_terminal_frame(Arc::new(frame), cx);
                                }
                            }
                        }
                    }
                    self.terminal = Some(Arc::new(Mutex::new(session)));
                    self.terminal_target = Some(target.clone());
                    self.terminal_size = Some(size);
                    self.status = "connected".to_string();
                    if let Some(window) = cx.windows().first().copied() {
                        poll_terminal(receiver, token, target, window, cx);
                    }
                }
            }
            Err(err) => {
                self.terminal = None;
                self.terminal_target = None;
                self.terminal_size = None;
                self.set_terminal_frame(
                    Arc::new(TerminalFrame {
                        lines: vec![TerminalLine {
                            runs: vec![TerminalRun {
                                text: err.clone(),
                                fg: 0xfca5a5,
                                bg: None,
                            }],
                        }],
                    }),
                    cx,
                );
                self.status = err;
            }
        }
    }

    fn close_active_workspace_if_empty(&mut self) {
        let Some(workspace_id) = self.active_workspace_id().map(str::to_string) else {
            return;
        };
        let has_panes = self
            .state
            .panes
            .iter()
            .any(|pane| pane.workspace_id.as_deref() == Some(workspace_id.as_str()));
        if !has_panes {
            self.with_client(|client| client.close_workspace(&workspace_id));
            self.refresh_state();
        }
    }

    fn close_workspace_after_terminal_exit(&mut self, target: &str) {
        let Some(workspace_id) = self.active_workspace_id().map(str::to_string) else {
            return;
        };
        let visible = self
            .state
            .panes
            .iter()
            .filter(|pane| pane.workspace_id.as_deref() == Some(workspace_id.as_str()))
            .collect::<Vec<_>>();
        let target_was_visible = visible.iter().any(|pane| {
            pane.terminal_id.as_deref() == Some(target) || pane.pane_id.as_str() == target
        });
        if target_was_visible && visible.len() <= 1 {
            self.with_client(|client| client.close_workspace(&workspace_id));
            self.refresh_state();
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

    fn handle_keystroke(&mut self, key: &Keystroke, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let Some(pane) = self.focused_pane().cloned() else {
            return;
        };
        // NAMED keys always send as key events, not text.
        // Enter as text=\n gets misinterpreted by the terminal (e.g. as shift+enter).
        let is_named = matches!(
            key.key.as_str(),
            "enter"
                | "backspace"
                | "tab"
                | "escape"
                | "up"
                | "down"
                | "left"
                | "right"
                | "delete"
                | "home"
                | "end"
                | "pageup"
                | "pagedown"
                | "insert"
        );
        let pane_id = pane.pane_id;
        if key.modifiers.alt || is_named {
            let key_str = key_name(key);
            cx.spawn(async move |this, cx| {
                if let Err(err) = client.send_key(&pane_id, &key_str) {
                    let _ = this.update(cx, |view, _cx| view.status = err.to_string());
                }
            })
            .detach();
        } else if let Some(text) = key.key_char.as_deref() {
            let text = text.to_string();
            cx.spawn(async move |this, cx| {
                if let Err(err) = client.send_text(&pane_id, &text) {
                    let _ = this.update(cx, |view, _cx| view.status = err.to_string());
                }
            })
            .detach();
        } else {
            let key_str = key_name(key);
            cx.spawn(async move |this, cx| {
                if let Err(err) = client.send_key(&pane_id, &key_str) {
                    let _ = this.update(cx, |view, _cx| view.status = err.to_string());
                }
            })
            .detach();
        }
    }

    #[allow(dead_code)]
    fn handle_workspace_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let delta = event.delta.pixel_delta(px(18.0));
        let horizontal = delta.x.abs() > delta.y.abs() + px(20.0);
        if horizontal {
            self.scroll_x += delta.x.to_f64();
            self.swipe_progress = (self.scroll_x / 80.0).clamp(-1.0, 1.0);
            cx.notify();
            if self.scroll_x.abs() < 80.0 {
                return;
            }
            let offset = if self.scroll_x < 0.0 { 1 } else { -1 };
            self.scroll_x = 0.0;
            self.swipe_progress = 0.0;
            self.focus_workspace_offset(offset, window, cx);
            return;
        }
        let rows = if delta.y.to_f64() > 0.0 {
            (delta.y.to_f64() / 18.0).round().max(1.0) as isize
        } else {
            -(delta.y.to_f64().abs() / 18.0).round().max(1.0) as isize
        };
        if let Some(terminal) = self.terminal.clone() {
            let frame = terminal
                .lock()
                .ok()
                .and_then(|mut terminal| terminal.scroll(-rows).ok());
            if let Some(frame) = frame {
                self.set_terminal_frame(Arc::new(frame), cx);
            }
            cx.notify();
        }
    }

    fn set_theme(&mut self, name: String, cx: &mut Context<Self>) {
        let theme = herdr_theme(&name);
        self.theme_mode = ThemeMode::Herdr(name);
        self.save_settings();
        self.sync_terminal_theme(theme, cx);
        cx.notify();
    }

    set_theme!(theme_catppuccin, ThemeCatppuccin, "catppuccin");
    set_theme!(
        theme_catppuccin_latte,
        ThemeCatppuccinLatte,
        "catppuccin-latte"
    );
    set_theme!(theme_terminal, ThemeTerminal, "terminal");
    set_theme!(theme_tokyo_night, ThemeTokyoNight, "tokyo-night");
    set_theme!(theme_tokyo_night_day, ThemeTokyoNightDay, "tokyo-night-day");
    set_theme!(theme_dracula, ThemeDracula, "dracula");
    set_theme!(theme_nord, ThemeNord, "nord");
    set_theme!(theme_gruvbox, ThemeGruvbox, "gruvbox");
    set_theme!(theme_gruvbox_light, ThemeGruvboxLight, "gruvbox-light");
    set_theme!(theme_one_dark, ThemeOneDark, "one-dark");
    set_theme!(theme_one_light, ThemeOneLight, "one-light");
    set_theme!(theme_solarized, ThemeSolarized, "solarized");
    set_theme!(
        theme_solarized_light,
        ThemeSolarizedLight,
        "solarized-light"
    );
    set_theme!(theme_kanagawa, ThemeKanagawa, "kanagawa");
    set_theme!(theme_kanagawa_lotus, ThemeKanagawaLotus, "kanagawa-lotus");
    set_theme!(theme_rose_pine, ThemeRosePine, "rose-pine");
    set_theme!(theme_rose_pine_dawn, ThemeRosePineDawn, "rose-pine-dawn");
    set_theme!(theme_vesper, ThemeVesper, "vesper");
    set_theme!(theme_oled, ThemeOled, "oled");

    #[allow(dead_code)]
    fn handle_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.sidebar_resizing && event.dragging() {
            let width = event.position.x.to_f64();
            self.sidebar_animation.width = width.clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
            self.sidebar_animation.start = self.sidebar_animation.width;
            self.sidebar_animation.target = self.sidebar_animation.width;
            let size = self.terminal_size(_window);
            if self.terminal_size != Some(size) {
                if let Some(terminal) = self.terminal.clone() {
                    let frame = terminal.lock().ok().and_then(|mut terminal| {
                        terminal.resize(size.0, size.1, size.2, size.3).ok()
                    });
                    if let Some(frame) = frame {
                        self.set_terminal_frame(Arc::new(frame), cx);
                    }
                }
                self.terminal_size = Some(size);
            }
            cx.notify();
            return;
        }
        if self.sidebar_collapsed
            && self.sidebar_animation.hovered
            && event.position.x.to_f64() > 220.0
        {
            self.transition_sidebar_width(|this| this.sidebar_animation.hovered = false, cx);
        }
    }

    #[allow(dead_code)]
    fn handle_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.sidebar_resizing {
            self.sidebar_resizing = false;
            self.save_settings();
            cx.notify();
        }
    }

    fn start_resize(&mut self, _: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_resizing = true;
        cx.notify();
    }

    #[allow(dead_code)]
    fn new_tab_mouse(&mut self, _: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.new_tab(&NewTab, window, cx);
    }

    fn toggle_agents_mouse(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_agents(&ToggleAgents, window, cx);
    }

    fn tab_header(&self, theme: UiTheme, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(22.0))
            .flex()
            .items_center()
            .justify_between()
            .child(ui_text(
                "tabs",
                10,
                theme.muted,
                false,
                "px-2 h-[18px] flex items-center",
            ))
            .child(
                div()
                    .w(px(22.0))
                    .h(px(22.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(move |style| style.bg(rgb(theme.hover)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| this.new_tab(&NewTab, window, cx)),
                    )
                    .child(icon("plus", 12.0, theme)),
            )
    }

    fn agent_header(
        &self,
        collapsed: bool,
        theme: UiTheme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let chevron_icon = if collapsed {
            "chevron.down"
        } else {
            "chevron.up"
        };
        div()
            .h(px(22.0))
            .flex()
            .items_center()
            .justify_between()
            .child(ui_text(
                "agents",
                10,
                theme.muted,
                false,
                "px-2 h-[18px] flex items-center",
            ))
            .child(
                div()
                    .w(px(22.0))
                    .h(px(22.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(move |style| style.bg(rgb(theme.hover)))
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::toggle_agents_mouse))
                    .child(icon(chevron_icon, 11.0, theme)),
            )
    }

    fn theme(&self, window: &Window) -> UiTheme {
        match &self.theme_mode {
            ThemeMode::Herdr(name) => herdr_theme(name),
            ThemeMode::SystemDark => herdr_theme("oled"),
            ThemeMode::SystemLight => herdr_theme("catppuccin-latte"),
            ThemeMode::System => match window.appearance() {
                WindowAppearance::Light | WindowAppearance::VibrantLight => {
                    herdr_theme("catppuccin-latte")
                }
                WindowAppearance::Dark | WindowAppearance::VibrantDark => herdr_theme("catppuccin"),
            },
        }
    }
}

impl Render for HerdrGui {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let render_started = Instant::now();
        let theme = self.theme(window);
        if self.terminal_bg != theme.terminal {
            self.terminal_bg = theme.terminal;
            self.sync_terminal_theme(theme, cx);
        }
        let panes = self.visible_panes();
        let pane_frame = self.terminal_frame.clone();
        let after_state = render_started.elapsed();

        let root = view_file!("ui/ui.crepus");
        let after_tree = render_started.elapsed();
        let total_ms = after_tree.as_secs_f64() * 1000.0;
        if total_ms > 5.0 {
            eprintln!(
                "render tree {total_ms:.1}ms state={:.1}ms tree={:.1}ms show_spaces={} workspaces={} tabs={} panes={} agents={} term_lines={}",
                after_state.as_secs_f64() * 1000.0,
                (after_tree - after_state).as_secs_f64() * 1000.0,
                self.show_spaces,
                self.state.workspaces.len(),
                self.state.tabs.len(),
                self.state.panes.len(),
                self.state.agents.len(),
                self.terminal_frame.lines.len(),
            );
        }

        root.key_context("HerdrGui")
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
            .on_action(cx.listener(Self::theme_catppuccin))
            .on_action(cx.listener(Self::theme_catppuccin_latte))
            .on_action(cx.listener(Self::theme_terminal))
            .on_action(cx.listener(Self::theme_tokyo_night))
            .on_action(cx.listener(Self::theme_tokyo_night_day))
            .on_action(cx.listener(Self::theme_dracula))
            .on_action(cx.listener(Self::theme_nord))
            .on_action(cx.listener(Self::theme_gruvbox))
            .on_action(cx.listener(Self::theme_gruvbox_light))
            .on_action(cx.listener(Self::theme_one_dark))
            .on_action(cx.listener(Self::theme_one_light))
            .on_action(cx.listener(Self::theme_solarized))
            .on_action(cx.listener(Self::theme_solarized_light))
            .on_action(cx.listener(Self::theme_kanagawa))
            .on_action(cx.listener(Self::theme_kanagawa_lotus))
            .on_action(cx.listener(Self::theme_rose_pine))
            .on_action(cx.listener(Self::theme_rose_pine_dawn))
            .on_action(cx.listener(Self::theme_vesper))
            .on_action(cx.listener(Self::theme_oled))
            .on_action(cx.listener(Self::theme_system))
            .on_action(cx.listener(Self::theme_system_dark))
            .on_action(cx.listener(Self::theme_system_light))
            .on_action(cx.listener(Self::reload_herdr_config))
    }
}

impl HerdrGui {
    fn terminal_area(
        &self,
        panes: Vec<Pane>,
        pane_frame: Arc<TerminalFrame>,
        theme: UiTheme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let started = Instant::now();
        let tabs = self.visible_tabs();
        let pane_container = div()
            .flex_1()
            .overflow_hidden()
            .ml(px((self.swipe_progress * 28.0) as f32))
            .child(self.pane_grid(panes, pane_frame, theme, cx))
            .into_any_element();
        let show_top_tabs = self.sidebar_layout == SidebarLayout::Arc;
        let show_swipe = self.swipe_progress.abs() > 0.01;
        let show_help = self.show_help;
        let tab_count = tabs.len();

        let el = view_file!("ui/terminal_area.crepus");
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        if ms > 2.0 {
            eprintln!(
                "terminal_area build {ms:.1}ms tabs={} panes={} term_lines={}",
                tab_count,
                self.state.panes.len(),
                self.terminal_frame.lines.len()
            );
        }
        el
    }

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

    fn tab_title(&self, tab: &Tab) -> String {
        tab.terminal_title
            .as_deref()
            .or(tab.title.as_deref())
            .or(tab.label.as_deref())
            .or_else(|| {
                self.state
                    .panes
                    .iter()
                    .find(|pane| pane.tab_id.as_deref() == Some(tab.tab_id.as_str()))
                    .and_then(|pane| {
                        pane.terminal_title
                            .as_deref()
                            .or(pane.title.as_deref())
                            .or(pane.label.as_deref())
                    })
            })
            .unwrap_or(&tab.tab_id)
            .to_string()
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

    fn sidebar_width(&self) -> f64 {
        if self.sidebar_collapsed && !self.sidebar_animation.hovered {
            SIDEBAR_COLLAPSED_WIDTH
        } else {
            self.sidebar_animation.width
        }
    }

    fn terminal_size(&self, window: &Window) -> TerminalSize {
        let size = window.bounds().size;
        let width = (size.width.to_f64() - self.sidebar_width() - RESIZE_HANDLE_WIDTH)
            .max(TERMINAL_MIN_WIDTH);
        let top_tabs_height = if self.sidebar_layout == SidebarLayout::Arc {
            TOP_TAB_BAR_HEIGHT
        } else {
            0.0
        };
        let height = (size.height.to_f64() - top_tabs_height).max(TERMINAL_MIN_HEIGHT);
        let cols = (width / TERMINAL_CELL_WIDTH)
            .floor()
            .clamp(TERMINAL_MIN_COLS, TERMINAL_MAX_COLS) as u16;
        let rows = (height / TERMINAL_CELL_HEIGHT)
            .floor()
            .clamp(TERMINAL_MIN_ROWS, TERMINAL_MAX_ROWS) as u16;
        (cols, rows, width.round() as u16, height.round() as u16)
    }

    fn sidebar(&self, theme: UiTheme, cx: &mut Context<Self>) -> impl IntoElement {
        let started = Instant::now();
        let start = self.sidebar_animation.start;
        let target = self.sidebar_animation.target;
        let width = self.sidebar_width() as f32;
        let el = div()
            .h_full()
            .w(px(width))
            .when(self.sidebar_layout == SidebarLayout::Arc, |el| {
                el.pt(px(TRAFFIC_LIGHT_PADDING as f32))
            })
            .bg(rgb(theme.panel))
            .flex()
            .flex_col()
            .overflow_hidden()
            .on_mouse_move(cx.listener(|this, _, _, cx| {
                if this.sidebar_collapsed && !this.sidebar_animation.hovered {
                    this.transition_sidebar_width(|this| this.sidebar_animation.hovered = true, cx);
                }
            }))
            .when(self.sidebar_layout != SidebarLayout::Arc, |el| {
                el.child(space_switcher(self.active_workspace(), theme, cx))
            })
            .when(self.show_spaces, |el| {
                el.child(div().flex().flex_col().gap_2().children(
                    self.state.workspaces.iter().map(|workspace| {
                        workspace_row(workspace, self.active_workspace_id(), theme, cx)
                    }),
                ))
            })
            .when(self.sidebar_layout == SidebarLayout::Warp, |el| {
                el.child(self.warp_sidebar(theme, cx))
            })
            .when(self.sidebar_layout == SidebarLayout::Arc, |el| {
                el.child(self.right_workspace_sidebar(theme, cx))
            });
        let built = if (start - target).abs() < f64::EPSILON {
            el.into_any_element()
        } else {
            let id = gpui::ElementId::Name(format!("sidebar-{:.0}-to-{:.0}", start, target).into());
            animation::width(
                el,
                id,
                Duration::from_millis(250),
                start as f32,
                target as f32,
            )
            .into_any_element()
        };
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        if ms > 2.0 {
            eprintln!(
                "sidebar build {ms:.1}ms show_spaces={} layout={:?} agents={}",
                self.show_spaces,
                self.sidebar_layout,
                self.state.agents.len()
            );
        }
        built
    }

    fn warp_sidebar(&self, theme: UiTheme, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_full()
            .flex()
            .flex_col()
            .gap_1()
            .child(self.tab_header(theme, cx))
            .children(self.visible_tabs().into_iter().map(|tab| {
                tab_sidebar_row(
                    tab.clone(),
                    self.tab_title(&tab),
                    self.state.focused_tab_id.as_deref(),
                    theme,
                    cx,
                )
            }))
            .child(div().flex_1())
            .child(self.agent_header(self.agents_collapsed, theme, cx))
            .when(!self.agents_collapsed, |el| {
                el.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .children(self.agent_rows(theme, cx)),
                )
            })
    }

    fn right_workspace_sidebar(&self, theme: UiTheme, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_full()
            .flex()
            .flex_col()
            .gap_1()
            .child(ui_text(
                "spaces",
                10,
                theme.muted,
                false,
                "px-2 h-[18px] flex items-center",
            ))
            .children(
                self.state.workspaces.iter().map(|workspace| {
                    workspace_row(workspace, self.active_workspace_id(), theme, cx)
                }),
            )
            .child(div().h(px(1.0)).bg(rgb(theme.border)))
            .child(self.agent_header(self.agents_collapsed, theme, cx))
            .when(!self.agents_collapsed, |el| {
                el.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .children(self.agent_rows(theme, cx)),
                )
            })
    }

    #[allow(dead_code)]
    fn sidebar_tab_rows(&self, theme: UiTheme, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .children(self.visible_tabs().into_iter().map(|tab| {
                tab_sidebar_row(
                    tab.clone(),
                    self.tab_title(&tab),
                    self.state.focused_tab_id.as_deref(),
                    theme,
                    cx,
                )
            }))
            .into_any_element()
    }

    #[allow(dead_code)]
    fn sidebar_workspace_rows(&self, theme: UiTheme, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .children(
                self.state.workspaces.iter().map(|workspace| {
                    workspace_row(workspace, self.active_workspace_id(), theme, cx)
                }),
            )
            .into_any_element()
    }

    fn top_tabs(&self, tabs: Vec<Tab>, theme: UiTheme, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w_full()
            .h(px(TOP_TAB_BAR_HEIGHT as f32))
            .flex()
            .items_center()
            .gap_1()
            .bg(rgb(theme.panel))
            .overflow_hidden()
            .children(tabs.into_iter().enumerate().map(|(index, tab)| {
                let number = index + 1;
                let id = tab.tab_id.clone();
                let close_id = tab.tab_id.clone();
                tab_chip(
                    number.to_string(),
                    tab.focused
                        || self
                            .state
                            .focused_tab_id
                            .as_deref()
                            .is_some_and(|focused| focused == tab.tab_id),
                    theme,
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
            .child(
                div()
                    .h_full()
                    .px_2()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(move |style| style.bg(rgb(theme.hover)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| this.new_tab(&NewTab, window, cx)),
                    )
                    .child(icon("plus", 12.0, theme)),
            )
    }

    fn close_tab_by_id(&mut self, tab_id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.with_client(|client| client.close_tab(&tab_id));
        self.refresh_state();
        self.attach_focused_terminal(window, cx);
        cx.notify();
    }

    #[allow(dead_code)]
    fn agent_rows(&self, theme: UiTheme, cx: &mut Context<Self>) -> Vec<AnyElement> {
        fn project_name(cwd: Option<&str>) -> String {
            cwd.and_then(|c| std::path::Path::new(c).file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("~")
                .to_string()
        }
        // Collect all agent items (agents + panes-with-agents)
        struct AgentItem {
            agent: Agent,
            project: String,
        }
        let mut seen: std::collections::HashSet<&str> = self
            .state
            .agents
            .iter()
            .filter_map(|a| a.pane_id.as_deref())
            .collect();
        let mut items: Vec<AgentItem> = self
            .state
            .agents
            .iter()
            .map(|a| AgentItem {
                agent: a.clone(),
                project: project_name(a.cwd.as_deref()),
            })
            .collect();
        for pane in &self.state.panes {
            if pane.agent.is_some() && seen.insert(pane.pane_id.as_str()) {
                let agent = Agent::from_pane(pane);
                let project = project_name(agent.cwd.as_deref());
                items.push(AgentItem { agent, project });
            }
        }
        // Count per project for numbering
        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for item in &items {
            *counts.entry(item.project.clone()).or_insert(0) += 1;
        }
        let mut current: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        let mut rows = Vec::new();
        for item in &items {
            let n = {
                let c = current.entry(item.project.clone()).or_insert(0);
                *c += 1;
                *c
            };
            let total = counts[&item.project];
            let title = if total > 1 {
                format!("{} \u{00b7} {}", item.project, n)
            } else {
                item.project.clone()
            };
            let subtitle = item
                .agent
                .agent
                .as_deref()
                .or(item.agent.name.as_deref())
                .unwrap_or("agent")
                .to_string();
            rows.push(agent_row(
                &item.agent,
                &self.state,
                theme,
                cx,
                title,
                subtitle,
            ));
        }
        rows
    }

    fn pane_grid(
        &self,
        panes: Vec<Pane>,
        pane_frame: Arc<TerminalFrame>,
        theme: UiTheme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if panes.is_empty() && pane_frame.lines.is_empty() {
            return div()
                .flex()
                .flex_1()
                .h_full()
                .items_center()
                .justify_center()
                .child(empty_state(&self.status, theme))
                .into_any_element();
        }

        if panes.is_empty() {
            return self.terminal_only_view(theme).into_any_element();
        }

        let pane = panes
            .iter()
            .find(|pane| pane.focused)
            .or_else(|| panes.first())
            .cloned();
        let Some(pane) = pane else {
            return div()
                .flex()
                .flex_1()
                .h_full()
                .bg(rgb(theme.terminal))
                .into_any_element();
        };
        let pane_id = pane.pane_id.clone();
        div()
            .flex()
            .flex_1()
            .h_full()
            .bg(rgb(theme.terminal))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    this.focus_pane_id(pane_id.clone(), window, cx)
                }),
            )
            .child(self.terminal_only_view(theme))
            .into_any_element()
    }

    fn terminal_only_view(&self, theme: UiTheme) -> AnyElement {
        // Keep bg in sync without forcing a full parent re-render tree of terminal cells.
        // set_bg is no-op when unchanged.
        let _ = theme;
        div()
            .flex_1()
            .overflow_hidden()
            .text_size(px(12.0))
            .font_family("Menlo")
            .line_height(px(18.0))
            .text_color(rgb(theme.text))
            .child(cached_terminal(self.terminal_pane.clone()))
            .into_any_element()
    }

    fn sync_terminal_theme(&self, theme: UiTheme, cx: &mut Context<Self>) {
        self.terminal_pane
            .update(cx, |pane, cx| pane.set_bg(theme.terminal, cx));
    }
}

fn workspace_detail(cwd: Option<&str>) -> String {
    let Some(path) = cwd else {
        return "~".to_string();
    };
    // ponytail: thread-local mtime cache; refresh on HEAD change only
    thread_local! {
        static BRANCH_CACHE: std::cell::RefCell<
            std::collections::HashMap<String, (Option<std::time::SystemTime>, String)>
        > = std::cell::RefCell::new(std::collections::HashMap::new());
    }
    let head_path = std::path::Path::new(path).join(".git/HEAD");
    let mtime = std::fs::metadata(&head_path)
        .and_then(|meta| meta.modified())
        .ok();
    BRANCH_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some((cached_mtime, detail)) = cache.get(path) {
            if *cached_mtime == mtime {
                return detail.clone();
            }
        }
        let detail = if let Ok(head) = std::fs::read_to_string(&head_path) {
            let head = head.trim();
            if let Some(ref_path) = head.strip_prefix("ref: refs/heads/") {
                ref_path.to_string()
            } else if head.len() >= 7 {
                head[..7].to_string()
            } else {
                dir_name(path)
            }
        } else {
            dir_name(path)
        };
        cache.insert(path.to_string(), (mtime, detail.clone()));
        detail
    })
}

fn dir_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("~")
        .to_string()
}

fn ui_text(text: &str, size: u32, color: u32, bold: bool, classes: &str) -> AnyElement {
    // Pure GPUI — avoid runtime parse_template/render_nodes on every label paint.
    let mut el = div()
        .text_size(px(size as f32))
        .text_color(rgb(color))
        .when(bold, |el| el.font_weight(FontWeight::SEMIBOLD));
    if classes.contains("px-2") {
        el = el.px_2();
    }
    if classes.contains("h-[18px]") {
        el = el.h(px(18.0)).flex().items_center();
    }
    el.child(text.to_string()).into_any_element()
}

fn truncate_label(text: &str, max_chars: usize) -> String {
    let mut output = String::new();
    let mut chars = text.chars();
    for _ in 0..max_chars {
        let Some(ch) = chars.next() else {
            return text.to_string();
        };
        output.push(ch);
    }
    if chars.next().is_some() {
        output.push_str("...");
        output
    } else {
        text.to_string()
    }
}

fn icon(name: &'static str, size: f32, theme: UiTheme) -> impl IntoElement {
    Icon::new(name)
        .size(px(size))
        .text_color(theme.label)
        .weight(SymbolWeight::Semibold)
}

fn swipe_hint(progress: f64, _theme: UiTheme) -> impl IntoElement {
    let _width = (progress.abs() * 80.0).max(8.0) as f32;
    view_file!("ui/widgets.crepus#SwipeHint").with_animation(
        "swipe-hint-pulse",
        Animation::new(Duration::from_millis(800))
            .repeat()
            .with_easing(bounce(linear)),
        |el, delta| el.opacity(0.35 + 0.2 * delta),
    )
}

fn space_switcher(
    workspace: Option<&Workspace>,
    theme: UiTheme,
    cx: &mut Context<HerdrGui>,
) -> impl IntoElement {
    let name = workspace
        .and_then(|workspace| workspace.label.as_deref())
        .or_else(|| workspace.map(|workspace| workspace.workspace_id.as_str()))
        .unwrap_or("space");
    div()
        .w_full()
        .h(px(38.0))
        .flex()
        .items_center()
        .pl(px(82.0))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h(px(32.0))
                .px_2()
                .flex()
                .items_center()
                .cursor_pointer()
                .overflow_hidden()
                .text_size(px(14.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(theme.label))
                .hover(move |style| style.bg(rgb(theme.hover)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.toggle_spaces(&ToggleSpaces, window, cx)
                    }),
                )
                .child(truncate_label(name, 64)),
        )
        .child(
            div()
                .w(px(32.0))
                .h(px(32.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(move |style| style.bg(rgb(theme.hover)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.new_workspace(&NewWorkspace, window, cx)
                    }),
                )
                .child(icon("plus", 13.0, theme)),
        )
}

#[allow(dead_code)]
fn tab_chip(
    title: String,
    focused: bool,
    theme: UiTheme,
    on_click: impl Fn(&crepuscularity_gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&crepuscularity_gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .h_full()
        .max_w(px(180.0))
        .px_2()
        .flex()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .overflow_hidden()
        .hover(move |style| style.bg(rgb(theme.hover)))
        .on_mouse_down(MouseButton::Left, on_click)
        .when(focused, |el| el.bg(rgb(theme.active)))
        .child(
            div()
                .min_w_0()
                .text_size(px(12.0))
                .text_color(rgb(theme.label))
                .child(truncate_label(&title, 22)),
        )
        .child(
            div()
                .w(px(16.0))
                .h(px(16.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .text_color(rgb(theme.muted))
                .hover(move |style| style.text_color(rgb(theme.text)))
                .on_mouse_down(MouseButton::Left, on_close)
                .child(icon("xmark", 9.0, theme)),
        )
}

fn workspace_row(
    workspace: &Workspace,
    active_workspace_id: Option<&str>,
    theme: UiTheme,
    cx: &mut Context<HerdrGui>,
) -> AnyElement {
    let id = workspace.workspace_id.clone();
    let close_id = workspace.workspace_id.clone();
    let title = workspace
        .label
        .as_deref()
        .unwrap_or(&workspace.workspace_id)
        .to_string();
    let detail = workspace_detail(workspace.cwd.as_deref());
    let focused = workspace.focused
        || active_workspace_id.is_some_and(|focused| focused == workspace.workspace_id);
    let on_click =
        cx.listener(move |this, _, window, cx| this.focus_workspace_id(id.clone(), window, cx));
    div()
        .px_3()
        .py_2()
        .flex()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .hover(move |style| style.bg(rgb(theme.hover)))
        .on_mouse_down(MouseButton::Left, on_click)
        .when(focused, |el| el.bg(rgb(theme.active)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(ui_text(&title, 14, theme.label, true, ""))
                .child(ui_text(&detail, 11, theme.muted, false, "")),
        )
        .child(
            div()
                .w(px(20.0))
                .h(px(20.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .text_color(rgb(theme.muted))
                .hover(move |style| style.text_color(rgb(theme.text)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        this.close_workspace_id(close_id.clone(), window, cx)
                    }),
                )
                .child(icon("xmark", 9.0, theme)),
        )
        .into_any_element()
}

fn tab_sidebar_row(
    tab: Tab,
    title: String,
    focused_tab_id: Option<&str>,
    theme: UiTheme,
    cx: &mut Context<HerdrGui>,
) -> AnyElement {
    let id = tab.tab_id.clone();
    let close_id = tab.tab_id.clone();
    let focused = tab.focused || focused_tab_id.is_some_and(|focused| focused == tab.tab_id);
    let detail = tab
        .agent_status
        .as_deref()
        .unwrap_or(if focused { "active" } else { "idle" })
        .to_string();
    let on_click =
        cx.listener(move |this, _, window, cx| this.focus_tab_id(id.clone(), window, cx));
    div()
        .px_3()
        .py_2()
        .flex()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .hover(move |style| style.bg(rgb(theme.hover)))
        .on_mouse_down(MouseButton::Left, on_click)
        .when(focused, |el| el.bg(rgb(theme.active)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(ui_text(&title, 14, theme.label, true, ""))
                .child(ui_text(&detail, 11, theme.muted, false, "")),
        )
        .child(
            div()
                .w(px(20.0))
                .h(px(20.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .text_color(rgb(theme.muted))
                .hover(move |style| style.text_color(rgb(theme.text)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        this.close_tab_by_id(close_id.clone(), window, cx)
                    }),
                )
                .child(icon("xmark", 9.0, theme)),
        )
        .into_any_element()
}

#[allow(dead_code)]
fn agent_row(
    agent: &Agent,
    state: &HerdrState,
    theme: UiTheme,
    cx: &mut Context<HerdrGui>,
    title: String,
    subtitle: String,
) -> AnyElement {
    let pane_id = agent.pane_id.clone();
    let workspace_id = agent.workspace_id.clone();
    let tab_id = agent.tab_id.clone();
    let status_key = agent
        .agent_status
        .as_deref()
        .unwrap_or("unknown")
        .to_string();
    let focused = agent.focused
        || pane_id.as_deref().is_some_and(|pane_id| {
            state
                .focused_pane_id
                .as_deref()
                .is_some_and(|focused| focused == pane_id)
        });
    let on_click = cx.listener(move |this, _, window, cx| {
        this.focus_target(
            workspace_id.clone(),
            tab_id.clone(),
            pane_id.clone(),
            window,
            cx,
        );
    });

    agent_row_element(title, subtitle, status_key, focused, theme, on_click).into_any_element()
}

#[allow(dead_code)]
fn agent_row_element(
    title: String,
    subtitle: String,
    status_key: String,
    focused: bool,
    theme: UiTheme,
    on_click: impl Fn(&crepuscularity_gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .px_3()
        .py_2()
        .flex()
        .items_center()
        .justify_between()
        .cursor_pointer()
        .bg(rgb(agent_status_background(&status_key, theme)))
        .hover(move |style| style.bg(rgb(theme.hover)))
        .on_mouse_down(MouseButton::Left, on_click)
        .when(focused, |el| {
            el.border_l_2()
                .border_color(rgb(agent_status_accent(&status_key)))
        })
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(ui_text(&title, 14, theme.label, true, ""))
                .child(ui_text(&subtitle, 11, theme.muted, false, "")),
        )
        .child(
            div()
                .w(px(8.0))
                .h(px(8.0))
                .bg(rgb(status_color(&status_key))),
        )
        .into_any_element()
}

fn empty_state(status: &str, theme: UiTheme) -> impl IntoElement {
    let _ = (status, theme);
    view_file!("ui/widgets.crepus#EmptyState")
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
                    MenuItem::action("Reload Config", ReloadHerdrConfig),
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
                    MenuItem::separator(),
                    MenuItem::submenu(Menu {
                        name: "Themes".into(),
                        items: vec![
                            MenuItem::submenu(Menu {
                                name: "System".into(),
                                items: vec![
                                    MenuItem::action("Auto", ThemeSystem),
                                    MenuItem::action("Dark", ThemeSystemDark),
                                    MenuItem::action("Light", ThemeSystemLight),
                                ],
                            }),
                            MenuItem::separator(),
                            MenuItem::action("catppuccin", ThemeCatppuccin),
                            MenuItem::action("catppuccin-latte", ThemeCatppuccinLatte),
                            MenuItem::action("terminal", ThemeTerminal),
                            MenuItem::action("tokyo-night", ThemeTokyoNight),
                            MenuItem::action("tokyo-night-day", ThemeTokyoNightDay),
                            MenuItem::action("dracula", ThemeDracula),
                            MenuItem::action("nord", ThemeNord),
                            MenuItem::action("gruvbox", ThemeGruvbox),
                            MenuItem::action("gruvbox-light", ThemeGruvboxLight),
                            MenuItem::action("one-dark", ThemeOneDark),
                            MenuItem::action("one-light", ThemeOneLight),
                            MenuItem::action("solarized", ThemeSolarized),
                            MenuItem::action("solarized-light", ThemeSolarizedLight),
                            MenuItem::action("kanagawa", ThemeKanagawa),
                            MenuItem::action("kanagawa-lotus", ThemeKanagawaLotus),
                            MenuItem::action("rose-pine", ThemeRosePine),
                            MenuItem::action("rose-pine-dawn", ThemeRosePineDawn),
                            MenuItem::action("vesper", ThemeVesper),
                        ],
                    }),
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
            KeyBinding::new("cmd-shift-r", ReloadHerdrConfig, None),
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
            let _ = window.update(cx, |view, window, cx| {
                view.attach_focused_terminal(window, cx)
            });
            let view = window.update(cx, |_, _, cx| cx.entity());
            if let Ok(view) = view {
                cx.observe_keystrokes(move |event, _, cx| {
                    view.update(cx, |view, view_cx| {
                        view.handle_keystroke(&event.keystroke, view_cx);
                    });
                })
                .detach();
            }
        }
        cx.activate(true);
    });
}

fn poll_terminal(
    receiver: Receiver<Vec<u8>>,
    token: u64,
    target: String,
    window: AnyWindowHandle,
    cx: &mut Context<HerdrGui>,
) {
    // Feed VT continuously; extract paint frames at most ~20fps so agent output
    // cannot monopolize the UI thread (ghostty frame() walks every cell).
    cx.spawn(async move |this, cx| loop {
        cx.background_executor()
            .timer(Duration::from_millis(16))
            .await;
        let mut accumulated = Vec::new();
        let mut disconnected = false;
        loop {
            match receiver.try_recv() {
                Ok(bytes) => accumulated.extend_from_slice(&bytes),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        if this
            .update(cx, |view, cx| {
                if view.terminal_token != token {
                    return;
                }
                if !accumulated.is_empty() {
                    if let Some(terminal) = view.terminal.clone() {
                        if let Ok(mut terminal) = terminal.lock() {
                            terminal.write_bytes(&accumulated);
                        }
                        view.terminal_pending_frame = true;
                    }
                }
                if view.terminal_pending_frame {
                    view.maybe_refresh_terminal_frame(cx, false);
                }
            })
            .is_err()
        {
            break;
        }
        if disconnected {
            let _ = this.update(cx, |view, cx| {
                if view.terminal_token == token {
                    view.terminal = None;
                    view.terminal_target = None;
                    view.terminal_size = None;
                    view.refresh_state();
                    view.close_workspace_after_terminal_exit(&target);
                }
                cx.notify();
            });
            let _ = cx.update_window(window, |_, window, cx| {
                let _ = this.update(cx, |view, cx| {
                    view.attach_focused_terminal(window, cx);
                });
            });
            break;
        }
    })
    .detach();
}

#[cfg(test)]
mod workspace_detail_tests {
    use super::workspace_detail;
    use std::fs;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[test]
    fn workspace_detail_reads_branch_and_refreshes_on_head_change() {
        let nanos = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.as_nanos(),
            Err(err) => panic!("{err}"),
        };
        let root = std::env::temp_dir().join(format!("herdr-gui-branch-{nanos}"));
        let git = root.join(".git");
        if let Err(err) = fs::create_dir_all(&git) {
            panic!("{err}");
        }
        if let Err(err) = fs::write(git.join("HEAD"), "ref: refs/heads/feature/lag\n") {
            panic!("{err}");
        }

        let Some(path) = root.to_str() else {
            panic!("temp path not utf8");
        };
        assert_eq!(workspace_detail(Some(path)), "feature/lag");
        assert_eq!(workspace_detail(Some(path)), "feature/lag");

        std::thread::sleep(Duration::from_millis(1100));
        if let Err(err) = fs::write(git.join("HEAD"), "ref: refs/heads/main\n") {
            panic!("{err}");
        }
        assert_eq!(workspace_detail(Some(path)), "main");

        let _ = fs::remove_dir_all(root);
    }
}
