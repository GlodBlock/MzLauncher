use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GuiPreferences {
    pub language: String,
    pub theme: ThemePreference,
    pub storage_dir: Option<String>,
    pub log_dir: Option<String>,
}

impl Default for GuiPreferences {
    fn default() -> Self {
        Self {
            language: "en".into(),
            theme: ThemePreference::System,
            storage_dir: None,
            log_dir: None,
        }
    }
}

impl GuiPreferences {
    pub fn load() -> Self {
        let Some(path) = preferences_file() else {
            return Self::default();
        };
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path =
            preferences_file().ok_or_else(|| "preferences directory unavailable".to_owned())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        std::fs::write(path, bytes).map_err(|error| error.to_string())
    }
}

fn preferences_file() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "MzLauncher", "MzLauncher")
        .map(|dirs| dirs.config_dir().join("gui").join("preferences.json"))
}
