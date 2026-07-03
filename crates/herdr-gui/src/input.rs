use crepuscularity_gpui::Keystroke;

pub fn key_name(key: &Keystroke) -> &str {
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
