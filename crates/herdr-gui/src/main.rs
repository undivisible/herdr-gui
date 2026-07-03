mod ghostty;
mod herdr;

use crepuscularity_gpui as gpui;
use crepuscularity_gpui::prelude::*;
use crepuscularity_gpui::{
    actions, bounds, div, gpui_window_options, point, px, rgb, size, App, Application, Context,
    FocusHandle, IntoElement, KeyBinding, Render, SharedString, Window, WindowBounds,
};
use ghostty::GhosttyRuntime;
use herdr::{HerdrClient, Pane, Snapshot};

actions!(
    herdr_gui,
    [
        ToggleHelp,
        Refresh,
        SplitRight,
        SplitDown,
        FocusRight,
        ResizeRight,
        SendEnter,
        ClosePane
    ]
);

struct HerdrGui {
    client: Option<HerdrClient>,
    ghostty: Result<GhosttyRuntime, String>,
    snapshot: Snapshot,
    status: String,
    show_help: bool,
    focus_handle: FocusHandle,
}

impl HerdrGui {
    fn new(cx: &mut Context<Self>) -> Self {
        let ghostty = GhosttyRuntime::detect();
        let (client, snapshot, status) = match HerdrClient::bootstrap() {
            Ok(client) => match client.snapshot() {
                Ok(snapshot) => (Some(client), snapshot, "connected".to_string()),
                Err(err) => (Some(client), Snapshot::default(), err.to_string()),
            },
            Err(err) => (None, Snapshot::default(), err.to_string()),
        };
        Self {
            client,
            ghostty,
            snapshot,
            status,
            show_help: false,
            focus_handle: cx.focus_handle(),
        }
    }

    fn toggle_help(&mut self, _: &ToggleHelp, _window: &mut Window, cx: &mut Context<Self>) {
        self.show_help = !self.show_help;
        cx.notify();
    }

    fn refresh(&mut self, _: &Refresh, _window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_snapshot();
        cx.notify();
    }

    fn split_right(&mut self, _: &SplitRight, _window: &mut Window, cx: &mut Context<Self>) {
        self.with_first_pane(|client, pane| client.split_right(&pane.pane_id));
        self.refresh_snapshot();
        cx.notify();
    }

    fn split_down(&mut self, _: &SplitDown, _window: &mut Window, cx: &mut Context<Self>) {
        self.with_first_pane(|client, pane| client.split_down(&pane.pane_id));
        self.refresh_snapshot();
        cx.notify();
    }

    fn focus_right(&mut self, _: &FocusRight, _window: &mut Window, cx: &mut Context<Self>) {
        self.with_client(HerdrClient::focus_right);
        self.refresh_snapshot();
        cx.notify();
    }

    fn resize_right(&mut self, _: &ResizeRight, _window: &mut Window, cx: &mut Context<Self>) {
        self.with_first_pane(|client, pane| client.resize_right(&pane.pane_id));
        self.refresh_snapshot();
        cx.notify();
    }

    fn send_enter(&mut self, _: &SendEnter, _window: &mut Window, cx: &mut Context<Self>) {
        self.with_first_pane(|client, pane| client.send_key(&pane.pane_id, "enter"));
        self.refresh_snapshot();
        cx.notify();
    }

    fn close_pane(&mut self, _: &ClosePane, _window: &mut Window, cx: &mut Context<Self>) {
        self.with_first_pane(|client, pane| client.close_pane(&pane.pane_id));
        self.refresh_snapshot();
        cx.notify();
    }

    fn refresh_snapshot(&mut self) {
        if let Some(client) = &self.client {
            match client.snapshot() {
                Ok(snapshot) => {
                    self.snapshot = snapshot;
                    self.status = "connected".to_string();
                }
                Err(err) => self.status = err.to_string(),
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
        let pane = self.snapshot.panes.first().cloned();
        if let (Some(client), Some(pane)) = (&self.client, pane) {
            if let Err(err) = f(client, &pane) {
                self.status = err.to_string();
            }
        }
    }
}

impl Render for HerdrGui {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ghostty_status = match &self.ghostty {
            Ok(runtime) => format!("Ghostty VT · {}", runtime.root.display()),
            Err(err) => err.clone(),
        };
        let panes = self.snapshot.panes.clone();
        let pane_text = if self.ghostty.is_ok() {
            self.snapshot.pane_text.clone()
        } else {
            ghostty_status.clone()
        };

        div()
            .w_full()
            .h_full()
            .bg(rgb(0x07080d))
            .text_color(rgb(0xf4f6fb))
            .flex()
            .key_context("HerdrGui")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::toggle_help))
            .on_action(cx.listener(Self::refresh))
            .on_action(cx.listener(Self::split_right))
            .on_action(cx.listener(Self::split_down))
            .on_action(cx.listener(Self::focus_right))
            .on_action(cx.listener(Self::resize_right))
            .on_action(cx.listener(Self::send_enter))
            .on_action(cx.listener(Self::close_pane))
            .child(self.sidebar(cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .h_full()
                    .bg(rgb(0x0a0d14))
                    .child(self.toolbar(ghostty_status))
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .overflow_hidden()
                            .child(self.pane_grid(panes, pane_text))
                            .when(self.show_help, |el| el.child(help_overlay())),
                    ),
            )
    }
}

impl HerdrGui {
    fn sidebar(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(236.0))
            .h_full()
            .border_r_1()
            .border_color(rgb(0x202633))
            .bg(rgb(0x0d1118))
            .p_3()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(label("Sessions", 0xf8fafc))
                    .child(status_dot(&self.status)),
            )
            .child(status_band(&self.status))
            .child(section("Workspaces"))
            .children(self.snapshot.workspaces.iter().map(|workspace| {
                item(
                    workspace
                        .label
                        .as_deref()
                        .unwrap_or(&workspace.workspace_id)
                        .to_string(),
                    workspace.cwd.as_deref().unwrap_or("").to_string(),
                )
            }))
            .child(section("Tabs"))
            .children(self.snapshot.tabs.iter().map(|tab| {
                item(
                    tab.label.as_deref().unwrap_or(&tab.tab_id).to_string(),
                    tab.tab_id.clone(),
                )
            }))
    }

    fn toolbar(&self, ghostty_status: String) -> impl IntoElement {
        div()
            .h(px(42.0))
            .border_b_1()
            .border_color(rgb(0x202633))
            .px_4()
            .flex()
            .items_center()
            .justify_between()
            .child(kbd_hint("F1 help"))
            .child(small(&ghostty_status))
    }

    fn pane_grid(&self, panes: Vec<Pane>, pane_text: String) -> impl IntoElement {
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
            .p_5()
            .gap_4()
            .children(panes.into_iter().map(|pane| {
                div()
                    .flex_1()
                    .h_full()
                    .bg(rgb(0x080a0f))
                    .border_1()
                    .border_color(rgb(0x2a3240))
                    .rounded_xl()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(42.0))
                            .px_4()
                            .flex()
                            .items_center()
                            .justify_between()
                            .border_b_1()
                            .border_color(rgb(0x202633))
                            .child(label(&pane.pane_id, 0xf8fafc))
                            .child(status(pane.agent_status.as_deref().unwrap_or("unknown"))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .p_4()
                            .text_size(px(12.0))
                            .font_family("Menlo")
                            .line_height(px(18.0))
                            .text_color(rgb(0xcbd5e1))
                            .child(SharedString::from(pane_text.clone())),
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

fn item(title: String, detail: String) -> impl IntoElement {
    div()
        .rounded_md()
        .bg(rgb(0x111722))
        .border_1()
        .border_color(rgb(0x222a38))
        .px_3()
        .py_2()
        .flex()
        .flex_col()
        .gap_0p5()
        .child(label(&title, 0xe2e8f0))
        .child(small(&detail))
}

fn status_band(text: &str) -> impl IntoElement {
    div()
        .rounded_lg()
        .bg(rgb(0x111722))
        .border_1()
        .border_color(rgb(0x222a38))
        .px_3()
        .py_2()
        .text_size(px(12.0))
        .text_color(if text == "connected" {
            rgb(0xa7f3d0)
        } else {
            rgb(0xfca5a5)
        })
        .child(text.to_string())
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
        .rounded_xl()
        .bg(rgb(0x111722))
        .border_1()
        .border_color(rgb(0x293241))
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
        .rounded_md()
        .bg(rgb(0x111722))
        .border_1()
        .border_color(rgb(0x222a38))
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
        .top(px(56.0))
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
        .child(key_row("R", "refresh"))
        .child(key_row("V", "split right"))
        .child(key_row("H", "split down"))
        .child(key_row("→", "focus right"))
        .child(key_row("Shift →", "resize right"))
        .child(key_row("Enter", "send enter"))
        .child(key_row("X", "close pane"))
}

fn status(text: &str) -> impl IntoElement {
    div()
        .rounded_md()
        .bg(status_color(Some(text)))
        .px_2()
        .py_1()
        .text_size(px(11.0))
        .text_color(rgb(0x030712))
        .child(text.to_string())
}

fn status_color(status: Option<&str>) -> crepuscularity_gpui::Rgba {
    match status {
        Some("working") => rgb(0xf59e0b),
        Some("blocked") => rgb(0xef4444),
        Some("done") => rgb(0x22c55e),
        Some("idle") => rgb(0x38bdf8),
        _ => rgb(0x475569),
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("f1", ToggleHelp, None),
            KeyBinding::new("r", Refresh, None),
            KeyBinding::new("v", SplitRight, None),
            KeyBinding::new("h", SplitDown, None),
            KeyBinding::new("right", FocusRight, None),
            KeyBinding::new("shift-right", ResizeRight, None),
            KeyBinding::new("enter", SendEnter, None),
            KeyBinding::new("x", ClosePane, None),
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
        let _ = cx.open_window(options, |_window, cx| cx.new(HerdrGui::new));
        cx.activate(true);
    });
}
