use crepuscularity_gpui::prelude::*;
use crepuscularity_gpui::{div, px, rgb, IntoElement};

pub fn help_overlay() -> impl IntoElement {
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
        .child(key_row("->", "focus right"))
        .child(key_row("Shift ->", "resize right"))
        .child(key_row("Cmd <-", "previous tab"))
        .child(key_row("Cmd ->", "next tab"))
        .child(key_row("Cmd W", "close pane"))
        .child(label("Terminal", 0xc0caf5))
        .child(key_row("Cmd V", "paste"))
        .child(key_row("Cmd Backspace", "delete line backward"))
        .child(key_row("Ctrl U", "kill line backward"))
        .child(key_row("Ctrl W", "delete word backward"))
        .child(key_row("Ctrl A / E", "line start / end"))
        .child(key_row("Alt Backspace", "delete word backward"))
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
