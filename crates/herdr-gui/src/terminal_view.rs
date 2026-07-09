use crepuscularity_gpui::prelude::*;
use crepuscularity_gpui::{div, px, rgb, IntoElement};

use crate::ghostty::{TerminalFrame, TerminalLine, TerminalRun};

pub fn terminal_frame(frame: &TerminalFrame, terminal_bg: u32) -> impl IntoElement {
    let bg = terminal_bg;
    div()
        .w_full()
        .h_full()
        .flex()
        .flex_col()
        .children(frame.lines.iter().map(|line| terminal_line(line, bg)))
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
