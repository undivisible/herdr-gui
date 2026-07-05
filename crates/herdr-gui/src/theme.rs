use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct UiTheme {
    pub bg: u32,
    pub panel: u32,
    pub terminal: u32,
    pub text: u32,
    pub label: u32,
    pub muted: u32,
    pub hover: u32,
    pub active: u32,
    pub border: u32,
}

pub fn agent_status_background(status: &str, theme: UiTheme) -> u32 {
    let light = theme.bg > 0x808080;
    match status {
        "working" => {
            if light {
                0xfff3d6
            } else {
                0x2a210f
            }
        }
        "blocked" => {
            if light {
                0xffe1e1
            } else {
                0x2b1515
            }
        }
        "done" => {
            if light {
                0xdff6e6
            } else {
                0x102615
            }
        }
        _ => theme.panel,
    }
}

pub fn agent_status_accent(status: &str) -> u32 {
    match status {
        "working" => 0xf59e0b,
        "blocked" => 0xef4444,
        "done" => 0x22c55e,
        _ => 0x8a8a8a,
    }
}

pub fn status_color(status: &str) -> u32 {
    match status {
        "working" => 0xf59e0b,
        "blocked" => 0xef4444,
        "done" => 0x22c55e,
        "idle" => 0x8a8a8a,
        _ => 0x555555,
    }
}

const DEFAULT_THEMES_JSON: &str = r#"{
  "catppuccin-latte": ["f5f5f5", "eff1f5", "ffffff", "4c4f69", "6c6f85", "e6e9ef", "ccd0da"],
  "terminal": ["0b0b0b", "0b0b0b", "080808", "f2f2f2", "8a8a8a", "181818", "202020"],
  "tokyo-night": ["1a1b26", "1a1b26", "11121a", "c0caf5", "a9b1d6", "24283b", "414868"],
  "tokyo-night-day": ["e1e2e7", "e1e2e7", "f8f8fb", "3760bf", "6172b0", "d2d3da", "c4c8da"],
  "dracula": ["282a36", "282a36", "15161c", "f8f8f2", "d2d2dc", "44475a", "6272a4"],
  "nord": ["2e3440", "2e3440", "20242d", "eceff4", "d8dee9", "3b4252", "434c5e"],
  "gruvbox": ["282828", "282828", "1d2021", "ebdbb2", "d5c4a1", "3c3836", "504945"],
  "gruvbox-light": ["fbf1c7", "fbf1c7", "fffff0", "3c3836", "504945", "f2e5bc", "ebdbb2"],
  "one-dark": ["282c34", "282c34", "1f2329", "abb2bf", "969ca8", "2c313a", "3e4451"],
  "one-light": ["fafafa", "fafafa", "ffffff", "383a42", "686b77", "f5f5f6", "e5e5e6"],
  "solarized": ["002b36", "002b36", "001f27", "93a1a1", "839496", "073642", "586e75"],
  "solarized-light": ["fdf6e3", "fdf6e3", "fffff4", "657b83", "839496", "eee8d5", "93a1a1"],
  "kanagawa": ["1f1f28", "1f1f28", "16161d", "dcd7ba", "c8c3aa", "2a2a37", "363646"],
  "kanagawa-lotus": ["f2ecbc", "f2ecbc", "fffae0", "545464", "43436c", "d5cea3", "dcd5ac"],
  "rose-pine": ["191724", "191724", "111019", "e0def4", "c8c5dc", "1f1d2e", "26233a"],
  "rose-pine-dawn": ["faf4ed", "faf4ed", "fffbf5", "464261", "797593", "f2e9e1", "fffaf3"],
  "vesper": ["1a1a1a", "1a1a1a", "101010", "ffffff", "a0a0a0", "232323", "282828"],
  "oled": ["000000", "000000", "000000", "ffffff", "888888", "1a1a1a", "2a2a2a"]
}"#;

static THEMES: OnceLock<HashMap<String, UiTheme>> = OnceLock::new();

pub fn herdr_theme(name: &str) -> UiTheme {
    THEMES
        .get_or_init(load_themes)
        .get(name)
        .copied()
        .unwrap_or_else(default_theme)
}

fn load_themes() -> HashMap<String, UiTheme> {
    let path = themes_path();
    if !path.exists() {
        write_default_themes(&path);
    }
    let json = std::fs::read_to_string(&path).unwrap_or_else(|_| DEFAULT_THEMES_JSON.to_string());
    parse_themes(&json).unwrap_or_else(|_| {
        parse_themes(DEFAULT_THEMES_JSON).unwrap_or_else(|_| {
            let mut themes = HashMap::new();
            themes.insert("oled".to_string(), default_theme());
            themes
        })
    })
}

fn parse_themes(json: &str) -> Result<HashMap<String, UiTheme>, String> {
    let raw: HashMap<String, Vec<String>> =
        serde_json::from_str(json).map_err(|err| err.to_string())?;
    let mut themes = HashMap::new();
    for (name, colors) in raw {
        if colors.len() != 7 {
            continue;
        }
        let parse = |s: &str| u32::from_str_radix(s, 16).unwrap_or(0);
        let text = parse(&colors[3]);
        let active = parse(&colors[6]);
        themes.insert(
            name,
            UiTheme {
                bg: parse(&colors[0]),
                panel: parse(&colors[1]),
                terminal: parse(&colors[2]),
                text,
                label: text,
                muted: parse(&colors[4]),
                hover: parse(&colors[5]),
                active,
                border: active,
            },
        );
    }
    Ok(themes)
}

fn write_default_themes(path: &std::path::Path) {
    let dir = path.parent().unwrap_or(std::path::Path::new("."));
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(path, DEFAULT_THEMES_JSON);
}

fn themes_path() -> PathBuf {
    crate::settings::settings_dir().join("themes.json")
}

fn default_theme() -> UiTheme {
    UiTheme {
        bg: 0x000000,
        panel: 0x000000,
        terminal: 0x000000,
        text: 0xffffff,
        label: 0xffffff,
        muted: 0x888888,
        hover: 0x1a1a1a,
        active: 0x2a2a2a,
        border: 0x2a2a2a,
    }
}
