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
    if key.modifiers.platform {
        name.push_str("cmd+");
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

/// GPUI still delivers keystrokes to `observe_keystrokes` after `KeyBinding` actions run.
pub fn should_swallow_gui_keystroke(key: &Keystroke) -> bool {
    if key.key == "f1" {
        return true;
    }
    if !key.modifiers.platform {
        return false;
    }
    let shift = key.modifiers.shift;
    match key.key.as_str() {
        "v" => !shift,
        "r" => true,
        "s" => shift,
        "b" => !shift,
        "t" => !shift,
        "w" => true,
        "]" => true,
        "left" | "right" => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crepuscularity_gpui::Keystroke;

    fn ks(key: &str, platform: bool, shift: bool) -> Keystroke {
        Keystroke {
            key: key.to_string(),
            key_char: None,
            modifiers: crepuscularity_gpui::Modifiers {
                control: false,
                alt: false,
                shift,
                platform,
                function: false,
            },
        }
    }

    #[test]
    fn cmd_backspace_encodes_platform_modifier() {
        assert_eq!(key_name(&ks("backspace", true, false)), "cmd+backspace");
    }

    #[test]
    fn cmd_v_is_swallowed_for_terminal_observer() {
        assert!(should_swallow_gui_keystroke(&ks("v", true, false)));
        assert!(!should_swallow_gui_keystroke(&ks("backspace", true, false)));
    }
}
