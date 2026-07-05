mod ghostty;
mod help;
mod herdr;
mod input;
mod settings;
mod terminal_view;
mod theme;

use crepuscularity_core::{parse_template, TemplateContext, TemplateValue};
use crepuscularity_gpui as gpui;
use crepuscularity_gpui::prelude::*;
use crepuscularity_gpui::{
    actions, bounds, div, gpui_window_options, point, px, rgb, size, AnyElement, App, Application,
    Context, FocusHandle, Icon, IntoElement, KeyBinding, Keystroke, Menu, MenuItem, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Render, ScrollWheelEvent, SystemMenuType,
    TitlebarOptions, Window, WindowAppearance, WindowBounds,
};
use crepuscularity_runtime::render_nodes;
use ghostty::{TerminalFrame, TerminalLine, TerminalRun, TerminalSession};
use help::help_overlay;
use herdr::{Agent, HerdrClient, HerdrState, Pane, Tab, Workspace};
use input::key_name;
use std::collections::HashMap;
use std::sync::Arc;
use std::{
    sync::mpsc::{Receiver, TryRecvError},
    time::Duration,
};
use terminal_view::terminal_frame;
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

#[derive(Clone, Copy, Eq, PartialEq)]
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
    terminal: Option<TerminalSession>,
    terminal_target: Option<String>,
    terminal_size: Option<TerminalSize>,
    terminal_token: u64,
    terminal_frame: Arc<TerminalFrame>,
    terminal_frames: HashMap<String, Arc<TerminalFrame>>,
    state: HerdrState,
    status: String,
    show_help: bool,
    show_spaces: bool,
    sidebar_collapsed: bool,
    sidebar_hovered: bool,
    agents_collapsed: bool,
    sidebar_layout: SidebarLayout,
    sidebar_resizing: bool,
    sidebar_width_px: f64,
    sidebar_width_start: f64,
    sidebar_width_target: f64,
    theme_mode: ThemeMode,
    swipe_progress: f64,
    focus_handle: FocusHandle,
    settings: settings::Settings,
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
        let sidebar_width = settings.sidebar_width.clamp(180.0, 360.0);
        Self {
            client,
            terminal: None,
            terminal_target: None,
            terminal_size: None,
            terminal_token: 0,
            terminal_frame: Arc::new(TerminalFrame::default()),
            terminal_frames: HashMap::new(),
            state,
            status,
            show_help: false,
            show_spaces: settings.show_spaces,
            sidebar_collapsed: settings.sidebar_collapsed,
            sidebar_hovered: false,
            agents_collapsed: settings.agents_collapsed,
            sidebar_layout,
            sidebar_resizing: false,
            sidebar_width_px: sidebar_width,
            sidebar_width_start: sidebar_width,
            sidebar_width_target: sidebar_width,
            theme_mode,
            swipe_progress: 0.0,
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
        self.save_settings();
        cx.notify();
    }

    fn toggle_spaces(&mut self, _: &ToggleSpaces, _window: &mut Window, cx: &mut Context<Self>) {
        self.show_spaces = !self.show_spaces;
        self.save_settings();
        cx.notify();
    }

    fn toggle_sidebar(&mut self, _: &ToggleSidebar, _window: &mut Window, cx: &mut Context<Self>) {
        self.transition_sidebar_width(|this| this.sidebar_collapsed = !this.sidebar_collapsed, cx);
    }

    fn transition_sidebar_width<F>(&mut self, f: F, cx: &mut Context<Self>)
    where
        F: FnOnce(&mut Self),
    {
        let old_width = self.sidebar_width();
        f(self);
        let new_width = self.sidebar_width();
        self.sidebar_width_start = old_width;
        self.sidebar_width_target = new_width;
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
        self.settings.sidebar_width = self.sidebar_width_px;
        self.settings.sidebar_collapsed = self.sidebar_collapsed;
        self.settings.show_spaces = self.show_spaces;
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
                    self.terminal_frame = self
                        .terminal_frames
                        .get(&target)
                        .cloned()
                        .unwrap_or_default();
                    if self.terminal_frame.lines.is_empty() {
                        if let (Some(client), Some(pane)) = (&self.client, pane.as_ref()) {
                            if let Ok(ansi) = client.read_pane_ansi(&pane.pane_id) {
                                if let Ok(frame) = TerminalFrame::from_ansi(size.0, size.1, &ansi) {
                                    self.terminal_frame = Arc::new(frame);
                                }
                            }
                        }
                    }
                    self.terminal = Some(session);
                    self.terminal_target = Some(target.clone());
                    self.terminal_size = Some(size);
                    self.status = "connected".to_string();
                    poll_terminal(receiver, token, target, cx);
                }
            }
            Err(err) => {
                self.terminal = None;
                self.terminal_target = None;
                self.terminal_size = None;
                self.terminal_frame = Arc::new(TerminalFrame {
                    lines: vec![TerminalLine {
                        runs: vec![TerminalRun {
                            text: err.clone(),
                            fg: 0xfca5a5,
                            bg: None,
                        }],
                    }],
                });
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

    fn handle_keystroke(&mut self, key: &Keystroke) {
        if let (Some(client), Some(pane)) = (&self.client, self.focused_pane().cloned()) {
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
            let result = if key.modifiers.alt || is_named {
                client.send_key(&pane.pane_id, &key_name(key))
            } else if let Some(text) = key.key_char.as_deref() {
                client.send_text(&pane.pane_id, text)
            } else {
                client.send_key(&pane.pane_id, &key_name(key))
            };
            if let Err(err) = result {
                self.status = err.to_string();
            }
        }
    }

    #[allow(dead_code)]
    fn handle_workspace_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Only vertical scroll — terminal scroll.
        // Horizontal scroll is ignored (user request: "terminal shouldnt scroll horizontally").
        let delta = event.delta.pixel_delta(px(18.0));
        let rows = (delta.y.to_f64() / 18.0).round() as isize;
        if rows != 0 {
            if let Some(terminal) = &self.terminal {
                terminal.scroll(-rows);
                cx.notify();
            }
        }
    }

    fn set_theme(&mut self, name: String, cx: &mut Context<Self>) {
        self.theme_mode = ThemeMode::Herdr(name);
        self.save_settings();
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
            self.sidebar_width_px = width.clamp(180.0, 360.0);
            self.sidebar_width_start = self.sidebar_width_px;
            self.sidebar_width_target = self.sidebar_width_px;
            self.terminal_size = None;
            cx.notify();
            return;
        }
        if self.sidebar_collapsed && self.sidebar_hovered && event.position.x.to_f64() > 220.0 {
            self.transition_sidebar_width(|this| this.sidebar_hovered = false, cx);
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

    fn resize_handle(&self, theme: UiTheme, cx: &mut Context<Self>) -> impl IntoElement {
        let _ = cx;
        view! {r#"
            div #resize-handle w-[4px] h-full flex-none cursor-col-resize @mousedown=start_resize
        "#}
        .hover(move |style| style.bg(rgb(theme.hover)))
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
        self.attach_focused_terminal(window, cx);
        let theme = self.theme(window);
        let panes = self.visible_panes();
        let pane_frame = self.terminal_frame.clone();

        let root = view! {r#"
            div #herdr-root w-full h-full flex overflow-hidden font-['Inter'] text-[14px] text-{theme.text} bg-{theme.bg}
                @scroll=handle_workspace_scroll
                @mousemove=handle_mouse_move
                @mouseup=handle_mouse_up
                {self.sidebar(theme, cx)}
                {self.resize_handle(theme, cx)}
                {self.terminal_area(panes, pane_frame, theme, cx)}
        "#};

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

        view! {r#"
            div #terminal-area flex-1 h-full overflow-hidden flex flex-col font-['Inter'] bg-{theme.terminal}
                if {show_top_tabs}
                    {self.top_tabs(tabs, theme, cx)}
                {pane_container}
                if {show_swipe}
                    {swipe_hint(self.swipe_progress, theme)}
                if {show_help}
                    {help_overlay()}
        "#}
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
        if self.sidebar_collapsed && !self.sidebar_hovered {
            6.0
        } else {
            self.sidebar_width_px
        }
    }

    fn terminal_size(&self, window: &Window) -> TerminalSize {
        let size = window.bounds().size;
        let width = (size.width.to_f64() - self.sidebar_width() - 4.0).max(320.0);
        let top_tabs_height = if self.sidebar_layout == SidebarLayout::Arc {
            34.0
        } else {
            0.0
        };
        let height = (size.height.to_f64() - top_tabs_height).max(240.0);
        let cols = (width / 7.2).floor().clamp(40.0, 500.0) as u16;
        let rows = (height / 18.0).floor().clamp(12.0, 180.0) as u16;
        (cols, rows, width.round() as u16, height.round() as u16)
    }

    fn sidebar(&self, theme: UiTheme, cx: &mut Context<Self>) -> impl IntoElement {
        let start = self.sidebar_width_start;
        let target = self.sidebar_width_target;
        let id = gpui::ElementId::Name(format!("sidebar-{:.0}-to-{:.0}", start, target).into());
        let el = div()
            .h_full()
            .when(self.sidebar_layout == SidebarLayout::Arc, |el| {
                el.pt(px(40.0))
            })
            .bg(rgb(theme.panel))
            .flex()
            .flex_col()
            .overflow_hidden()
            .on_mouse_move(cx.listener(|this, _, _, cx| {
                if this.sidebar_collapsed && !this.sidebar_hovered {
                    this.transition_sidebar_width(|this| this.sidebar_hovered = true, cx);
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
        animation::width(
            el,
            id,
            Duration::from_millis(250),
            start as f32,
            target as f32,
        )
        .into_any_element()
    }

    fn warp_sidebar(&self, theme: UiTheme, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_full()
            .flex()
            .flex_col()
            .gap_1()
            .child(tab_header(theme, cx))
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
            .child(agent_header(self.agents_collapsed, theme, cx))
            .when(!self.agents_collapsed, |el| {
                el.children(self.agent_rows(theme, cx))
            })
    }

    fn right_workspace_sidebar(&self, theme: UiTheme, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_full()
            .flex()
            .flex_col()
            .gap_1()
            .child(section("spaces", theme))
            .children(
                self.state.workspaces.iter().map(|workspace| {
                    workspace_row(workspace, self.active_workspace_id(), theme, cx)
                }),
            )
            .child(div().h(px(1.0)).bg(rgb(theme.border)))
            .child(agent_header(self.agents_collapsed, theme, cx))
            .when(!self.agents_collapsed, |el| {
                el.children(self.agent_rows(theme, cx))
            })
    }

    fn top_tabs(&self, tabs: Vec<Tab>, theme: UiTheme, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w_full()
            .h(px(34.0))
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
    ) -> impl IntoElement {
        if panes.is_empty() {
            return div()
                .flex()
                .flex_1()
                .h_full()
                .items_center()
                .justify_center()
                .child(empty_state(&self.status, theme));
        }

        let pane = panes
            .iter()
            .find(|pane| pane.focused)
            .or_else(|| panes.first())
            .cloned();
        let Some(pane) = pane else {
            return div().flex().flex_1().h_full().bg(rgb(theme.terminal));
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
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .text_size(px(12.0))
                    .font_family("Menlo")
                    .line_height(px(18.0))
                    .text_color(rgb(theme.text))
                    .child(terminal_frame(pane_frame.as_ref(), theme.terminal)),
            )
    }
}

fn label(text: &str, color: u32) -> AnyElement {
    crepus_render(
        "div text-[14px] font-semibold text-{color}\n    \"{text}\"",
        [
            ("color", TemplateValue::Str(format!("{:06x}", color))),
            ("text", TemplateValue::Str(text.to_string())),
        ],
    )
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

fn small(text: &str, theme: UiTheme) -> AnyElement {
    crepus_render(
        "div text-[11px] text-{theme.muted}\n    \"{text}\"",
        [
            ("theme", TemplateValue::Scope(theme_ctx(theme))),
            ("text", TemplateValue::Str(text.to_string())),
        ],
    )
}

fn section(text: &str, theme: UiTheme) -> AnyElement {
    crepus_render(
        "div px-2 pt-1 text-[10px] text-{theme.muted}\n    \"{text}\"",
        [
            ("theme", TemplateValue::Scope(theme_ctx(theme))),
            ("text", TemplateValue::Str(text.to_string())),
        ],
    )
}

fn icon(name: &'static str, size: f32, theme: UiTheme) -> impl IntoElement {
    Icon::new(name).size(px(size)).text_color(theme.label)
}

fn theme_ctx(theme: UiTheme) -> TemplateContext {
    let mut ctx = TemplateContext::default();
    ctx.vars.insert(
        "bg".to_string(),
        TemplateValue::Str(format!("{:06x}", theme.bg)),
    );
    ctx.vars.insert(
        "panel".to_string(),
        TemplateValue::Str(format!("{:06x}", theme.panel)),
    );
    ctx.vars.insert(
        "terminal".to_string(),
        TemplateValue::Str(format!("{:06x}", theme.terminal)),
    );
    ctx.vars.insert(
        "text".to_string(),
        TemplateValue::Str(format!("{:06x}", theme.text)),
    );
    ctx.vars.insert(
        "label".to_string(),
        TemplateValue::Str(format!("{:06x}", theme.label)),
    );
    ctx.vars.insert(
        "muted".to_string(),
        TemplateValue::Str(format!("{:06x}", theme.muted)),
    );
    ctx.vars.insert(
        "hover".to_string(),
        TemplateValue::Str(format!("{:06x}", theme.hover)),
    );
    ctx.vars.insert(
        "active".to_string(),
        TemplateValue::Str(format!("{:06x}", theme.active)),
    );
    ctx.vars.insert(
        "border".to_string(),
        TemplateValue::Str(format!("{:06x}", theme.border)),
    );
    ctx
}

fn crepus_render(
    template: &str,
    vars: impl IntoIterator<Item = (&'static str, TemplateValue)>,
) -> AnyElement {
    let mut ctx = TemplateContext::default();
    for (key, value) in vars {
        ctx.vars.insert(key.to_string(), value);
    }
    let nodes = parse_template(template).unwrap_or_default();
    render_nodes(&nodes, &ctx)
}

fn swipe_hint(progress: f64, _theme: UiTheme) -> AnyElement {
    let width = (progress.abs() * 80.0).max(8.0) as f32;
    crepus_render(
        "div #swipe-hint absolute bottom-0 left-0 h-[2px] w-[{width}px] bg-white opacity-35",
        [("width", TemplateValue::Float(width as f64))],
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

fn tab_header(theme: UiTheme, cx: &mut Context<HerdrGui>) -> impl IntoElement {
    div()
        .pt_1()
        .flex()
        .items_center()
        .justify_between()
        .child(section("tabs", theme))
        .child(
            div()
                .w(px(22.0))
                .h(px(22.0))
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

fn agent_header(collapsed: bool, theme: UiTheme, cx: &mut Context<HerdrGui>) -> impl IntoElement {
    div()
        .pt_1()
        .flex()
        .items_center()
        .justify_between()
        .child(section("agents", theme))
        .child(
            div()
                .w(px(22.0))
                .h(px(22.0))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(move |style| style.bg(rgb(theme.hover)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.toggle_agents(&ToggleAgents, window, cx)
                    }),
                )
                .child(icon(
                    if collapsed {
                        "chevron.down"
                    } else {
                        "chevron.up"
                    },
                    11.0,
                    theme,
                )),
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
    let detail = workspace.cwd.as_deref().unwrap_or("~").to_string();
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
                .child(label(&title, theme.label))
                .child(small(&detail, theme)),
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

    div()
        .px_3()
        .py_2()
        .flex()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .hover(move |style| style.bg(rgb(theme.hover)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, window, cx| this.focus_tab_id(id.clone(), window, cx)),
        )
        .when(focused, |el| el.bg(rgb(theme.active)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(label(&title, theme.label))
                .child(small(&detail, theme)),
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
                        this.with_client(|client| client.close_tab(&close_id));
                        this.refresh_state();
                        this.attach_focused_terminal(window, cx);
                        cx.notify();
                    }),
                )
                .child(icon("xmark", 9.0, theme)),
        )
        .into_any_element()
}

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
                .child(label(&title, theme.label))
                .child(small(&subtitle, theme)),
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
    view! {r#"
        div #empty-state w-[560px] rounded-lg bg-{theme.panel} border border-{theme.border} p-5 flex flex-col gap-3
            div text-{theme.label}
                "No Herdr panes visible"
            div text-{theme.muted}
                "{status}"
            div rounded-lg bg-{theme.terminal} border border-{theme.border} p-3 font-mono text-[12px] text-{theme.text}
                "Open Herdr in a terminal, create a workspace/pane, then press Refresh."
    "#}
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

fn poll_terminal(
    receiver: Receiver<TerminalFrame>,
    token: u64,
    target: String,
    cx: &mut Context<HerdrGui>,
) {
    cx.spawn(async move |this, cx| loop {
        cx.background_executor()
            .timer(Duration::from_millis(16))
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

                    let frame = Arc::new(frame);
                    view.terminal_frames
                        .insert(target.clone(), Arc::clone(&frame));
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
                view.close_workspace_after_terminal_exit(&target);
                cx.notify();
            });
            break;
        }
    })
    .detach();
}
