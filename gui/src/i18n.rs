use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Deserialize)]
struct LanguageFile {
    name: String,
    messages: HashMap<String, String>,
}

struct Locale {
    id: String,
    name: String,
    messages: HashMap<String, String>,
}

pub struct I18n {
    locales: Vec<Locale>,
    current: String,
}

impl I18n {
    pub fn load() -> Self {
        let mut locales = Vec::new();
        let english: LanguageFile = serde_json::from_str(include_str!("../lang/en.json"))
            .expect("embedded English language file must be valid");
        locales.push(Locale {
            id: "en".into(),
            name: english.name,
            messages: english.messages,
        });
        for directory in lang_dirs() {
            let Ok(entries) = std::fs::read_dir(&directory) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                    continue;
                }
                let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else {
                    continue;
                };
                if locales.iter().any(|locale| locale.id == id) {
                    continue;
                }
                let Ok(bytes) = std::fs::read(&path) else {
                    continue;
                };
                let Ok(file) = serde_json::from_slice::<LanguageFile>(&bytes) else {
                    continue;
                };
                locales.push(Locale {
                    id: id.to_owned(),
                    name: file.name,
                    messages: file.messages,
                });
            }
        }
        let current = load_language_preference()
            .filter(|preferred| locales.iter().any(|locale| locale.id == *preferred))
            .unwrap_or_else(|| "en".into());
        Self { locales, current }
    }

    pub fn current(&self) -> &str {
        &self.current
    }

    pub fn available(&self) -> Vec<(String, String)> {
        self.locales
            .iter()
            .map(|locale| (locale.id.clone(), locale.name.clone()))
            .collect()
    }

    pub fn set_current(&mut self, id: &str) {
        if self.locales.iter().any(|locale| locale.id == id) {
            self.current = id.to_owned();
            save_language_preference(id);
        }
    }

    pub fn tr(&self, key: &str) -> String {
        self.trf(key, &[])
    }

    pub fn trf(&self, key: &str, params: &[(&str, &str)]) -> String {
        let template = self
            .locales
            .iter()
            .find(|locale| locale.id == self.current)
            .and_then(|locale| locale.messages.get(key))
            .or_else(|| {
                self.locales
                    .first()
                    .and_then(|locale| locale.messages.get(key))
            })
            .cloned()
            .unwrap_or_else(|| key.to_owned());
        params.iter().fold(template, |text, (name, value)| {
            text.replace(&format!("{{{name}}}"), value)
        })
    }
}

fn lang_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.join("lang"));
        }
    }
    dirs.push(PathBuf::from("lang"));
    dirs.push(PathBuf::from("gui/lang"));
    dirs
}

fn preferences_file() -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("com", "MzLauncher", "MzLauncher")?;
    Some(dirs.config_dir().join("gui").join("preferences.json"))
}

fn load_language_preference() -> Option<String> {
    let bytes = std::fs::read(preferences_file()?).ok()?;
    serde_json::from_slice::<HashMap<String, String>>(&bytes)
        .ok()?
        .get("language")
        .cloned()
}

fn save_language_preference(language: &str) {
    let Some(file) = preferences_file() else {
        return;
    };
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let preferences = HashMap::from([("language", language)]);
    if let Ok(json) = serde_json::to_vec_pretty(&preferences) {
        let _ = std::fs::write(file, json);
    }
}
