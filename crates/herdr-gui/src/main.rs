mod ghostty;
mod herdr;

use crepuscularity_gpui::prelude::*;
use crepuscularity_gpui::{
    bounds, div, gpui_window_options, point, px, rgb, size, App, Application, Context, IntoElement,
    MouseButton, MouseDownEvent, Render, SharedString, Window, WindowBounds,
};
use ghostty::GhosttyRuntime;
use herdr::{HerdrClient, Pane, Snapshot};

struct HerdrGui {
    client: Option<HerdrClient>,
    ghostty: Result<GhosttyRuntime, String>,
    snapshot: Snapshot,
    status: String,
}

impl HerdrGui {
    fn new(_cx: &mut Context<Self>) -> Self {
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
        }
    }

    fn refresh(&mut self, _: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_snapshot();
        cx.notify();
    }

    fn split_right(&mut self, _: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.with_first_pane(|client, pane| client.split_right(&pane.pane_id));
        self.refresh_snapshot();
        cx.notify();
    }

    fn split_down(&mut self, _: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.with_first_pane(|client, pane| client.split_down(&pane.pane_id));
        self.refresh_snapshot();
        cx.notify();
    }

    fn focus_right(&mut self, _: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.with_client(HerdrClient::focus_right);
        self.refresh_snapshot();
        cx.notify();
    }

    fn resize_right(&mut self, _: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.with_first_pane(|client, pane| client.resize_right(&pane.pane_id));
        self.refresh_snapshot();
        cx.notify();
    }

    fn send_enter(&mut self, _: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.with_first_pane(|client, pane| client.send_key(&pane.pane_id, "enter"));
        self.refresh_snapshot();
        cx.notify();
    }

    fn send_ls(&mut self, _: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.with_first_pane(|client, pane| client.send_text(&pane.pane_id, "ls"));
        self.refresh_snapshot();
        cx.notify();
    }

    fn close_pane(&mut self, _: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
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
            Ok(runtime) => format!("libghostty-vt: {}", runtime.root.display()),
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
            .bg(rgb(0x090b10))
            .text_color(rgb(0xf4f6fb))
            .flex()
            .child(self.sidebar(cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .h_full()
                    .bg(rgb(0x0d1117))
                    .child(self.toolbar(cx, ghostty_status))
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .overflow_hidden()
                            .child(self.pane_grid(panes, pane_text)),
                    ),
            )
    }
}

impl HerdrGui {
    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(260.0))
            .h_full()
            .border_r_1()
            .border_color(rgb(0x252b36))
            .bg(rgb(0x0f141d))
            .p_3()
            .flex()
            .flex_col()
            .gap_3()
            .child(label("HERDR GUI", 0xf8fafc))
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
            .child(button("refresh", cx.listener(Self::refresh)))
    }

    fn toolbar(&self, cx: &mut Context<Self>, ghostty_status: String) -> impl IntoElement {
        div()
            .h(px(58.0))
            .border_b_1()
            .border_color(rgb(0x252b36))
            .px_3()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(button("split right", cx.listener(Self::split_right)))
                    .child(button("split down", cx.listener(Self::split_down)))
                    .child(button("focus right", cx.listener(Self::focus_right)))
                    .child(button("resize", cx.listener(Self::resize_right)))
                    .child(button("send ls", cx.listener(Self::send_ls)))
                    .child(button("enter", cx.listener(Self::send_enter)))
                    .child(button("close pane", cx.listener(Self::close_pane))),
            )
            .child(small(&ghostty_status))
    }

    fn pane_grid(&self, panes: Vec<Pane>, pane_text: String) -> impl IntoElement {
        div()
            .flex()
            .flex_1()
            .h_full()
            .p_3()
            .gap_3()
            .children(panes.into_iter().map(|pane| {
                div()
                    .flex_1()
                    .h_full()
                    .bg(rgb(0x07090d))
                    .border_1()
                    .border_color(status_color(pane.agent_status.as_deref()))
                    .rounded_lg()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(label(&pane.pane_id, 0xf8fafc))
                            .child(status(pane.agent_status.as_deref().unwrap_or("unknown"))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
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
        .pt_2()
        .text_size(px(10.0))
        .text_color(rgb(0x64748b))
        .child(text.to_string())
}

fn item(title: String, detail: String) -> impl IntoElement {
    div()
        .rounded_md()
        .bg(rgb(0x171d27))
        .border_1()
        .border_color(rgb(0x252b36))
        .px_3()
        .py_2()
        .flex()
        .flex_col()
        .gap_0p5()
        .child(label(&title, 0xe2e8f0))
        .child(small(&detail))
}

fn button(
    label: &str,
    on_click: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .rounded_md()
        .bg(rgb(0x1b2430))
        .border_1()
        .border_color(rgb(0x2f3a49))
        .px_3()
        .py_1()
        .text_size(px(12.0))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .child(label.to_string())
        .on_mouse_down(MouseButton::Left, on_click)
}

fn status_band(text: &str) -> impl IntoElement {
    div()
        .rounded_md()
        .bg(rgb(0x151b24))
        .border_1()
        .border_color(rgb(0x252b36))
        .px_3()
        .py_2()
        .text_size(px(12.0))
        .text_color(rgb(0xa7f3d0))
        .child(text.to_string())
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
        let options = gpui_window_options(
            "dev.undivisible.herdr-gui",
            "Herdr GUI",
            Some(WindowBounds::Windowed(bounds(
                point(px(80.0), px(80.0)),
                size(px(1280.0), px(820.0)),
            ))),
            Some(size(px(920.0), px(600.0))),
        );
        let _ = cx.open_window(options, |_window, cx| cx.new(HerdrGui::new));
    });
}
