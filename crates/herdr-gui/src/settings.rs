use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Settings {
    pub theme: String,
    pub sidebar_layout: String,
    pub sidebar_width: f64,
    pub sidebar_collapsed: bool,
    pub show_spaces: bool,
    pub agents_collapsed: bool,
}

impl Settings {
    pub fn load() -> Self {
        let path = settings_path();
        if !path.exists() {
            return Self::default();
        }
        std::fs::read_to_string(path)
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let dir = settings_dir();
        if let Err(err) = std::fs::create_dir_all(&dir) {
            eprintln!(
                "failed to create settings directory {}: {err}",
                dir.display()
            );
            return;
        }
        let path = dir.join("settings.json");
        match serde_json::to_string_pretty(self) {
            Ok(json) => {
                if let Err(err) = std::fs::write(&path, json) {
                    eprintln!("failed to write settings {}: {err}", path.display());
                }
            }
            Err(err) => eprintln!("failed to serialize settings: {err}"),
        }
    }
}

fn settings_dir() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".herdrgui")
}

fn settings_path() -> PathBuf {
    settings_dir().join("settings.json")
}
