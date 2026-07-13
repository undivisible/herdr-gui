use crepuscularity_gpui::prelude::*;
use crepuscularity_gpui::{cached_view, div, px, rgb, Entity, IntoElement, Render, Window};
use std::sync::Arc;

use crate::ghostty::{TerminalFrame, TerminalLine, TerminalRun};

/// Own entity so parent chrome re-renders (spaces dropdown, etc.) can reuse
/// previous layout/paint — Zed-style via [`AnyView::cached`].
pub struct TerminalPane {
    frame: Arc<TerminalFrame>,
    bg: u32,
}

impl TerminalPane {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            frame: Arc::new(TerminalFrame::default()),
            bg: 0x0a0a0a,
        }
    }

    pub fn set_frame(&mut self, frame: Arc<TerminalFrame>, cx: &mut Context<Self>) {
        if Arc::ptr_eq(&self.frame, &frame) || self.frame.as_ref() == frame.as_ref() {
            return;
        }
        let runs: usize = frame.lines.iter().map(|l| l.runs.len()).sum();
        eprintln!(
            "[terminal_pane] set_frame lines={} runs={runs} -> notify",
            frame.lines.len()
        );
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/tmp/herdr-gui-lag.log")
        {
            use std::io::Write;
            let _ = writeln!(
                file,
                "[terminal_pane] set_frame lines={} runs={runs} -> notify",
                frame.lines.len()
            );
        }
        self.frame = frame;
        cx.notify();
    }

    pub fn set_bg(&mut self, bg: u32, cx: &mut Context<Self>) {
        if self.bg == bg {
            return;
        }
        self.bg = bg;
        cx.notify();
    }
}

impl Render for TerminalPane {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let bg = self.bg;
        div().w_full().h_full().flex().flex_col().children(
            self.frame
                .lines
                .iter()
                .map(move |line| terminal_line(line, bg)),
        )
    }
}

/// Embed terminal entity with GPUI paint recycling when parent re-renders.
pub fn cached_terminal(entity: Entity<TerminalPane>) -> impl IntoElement {
    cached_view(entity)
}

fn terminal_line(line: &TerminalLine, bg: u32) -> impl IntoElement {
    div()
        .w_full()
        .h(px(18.0))
        .flex()
        .flex_none()
        .overflow_hidden()
        .bg(rgb(bg))
        .children(line.runs.iter().map(terminal_run))
}

fn terminal_run(run: &TerminalRun) -> impl IntoElement {
    div()
        .flex_none()
        .text_color(rgb(run.fg))
        .when_some(run.bg, |el, bg| el.bg(rgb(bg)))
        .child(run.text.replace(' ', "\u{00a0}"))
}
