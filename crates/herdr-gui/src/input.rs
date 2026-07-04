use crepuscularity_gpui::Keystroke;

pub fn key_name(key: &Keystroke) -> String {
    let mut name = String::new();
    if key.modifiers.control {
        name.push_str("ctrl+");
    }
    if key.modifiers.alt {
        name.push_str("alt+");
    }
    if key.modifiers.shift {
        name.push_str("shift+");
    }
    name.push_str(match key.key.as_str() {
        "enter" => "enter",
        "backspace" => "backspace",
        "tab" => "tab",
        "escape" => "escape",
        "up" => "up",
        "down" => "down",
        "right" => "right",
        "left" => "left",
        "delete" => "delete",
        "home" => "home",
        "end" => "end",
        "pageup" => "pageup",
        "pagedown" => "pagedown",
        "insert" => "insert",
        other => other,
    });
    name
}
