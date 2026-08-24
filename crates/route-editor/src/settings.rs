//! What the editor remembers between runs — the vehicle editor's pattern.
//!
//! Only what the user would otherwise have to re-do by hand. The file is
//! written next to the operating system's other application settings and is
//! never required — a missing or unreadable one leaves the editor at its
//! defaults, without a word. Layout is tracked in memory every frame and
//! written only when the user leaves; a `--frames` screenshot run never
//! writes, so its throwaway window size stays out of the real settings.

use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize, Resource)]
pub struct Settings {
    /// Language picked under View; `None` follows the operating system.
    #[serde(default)]
    pub language: Option<String>,
    /// Size of the window as the user left it.
    #[serde(default)]
    pub window: Option<(f32, f32)>,
    /// Width of the properties panel as the user left it.
    #[serde(default)]
    pub panel: Option<f32>,
}

impl Settings {
    pub fn load() -> Self {
        settings_path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| ron::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Applies a remembered language.
    ///
    /// `TRAINSIM_LANG` wins: it is an explicit instruction for this one run,
    /// while the setting is a standing preference.
    pub fn apply_language(&self) {
        if std::env::var_os("TRAINSIM_LANG").is_some() {
            return;
        }
        if let Some(language) = &self.language {
            i18n::set_language(language);
        }
    }

    /// Records the language and writes the file — it is picked rarely enough
    /// that saving straight away costs nothing.
    pub fn set_language(&mut self, code: &str) {
        self.language = Some(code.to_owned());
        self.save();
    }

    /// Best effort: settings are a convenience, and failing to store them is
    /// no reason to interrupt whatever the user was doing.
    pub fn save(&self) {
        let Some(path) = settings_path() else {
            return;
        };
        let Ok(text) = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()) else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, text);
    }
}

/// `%APPDATA%\Connected Rails\` on Windows, `$XDG_CONFIG_HOME` or `~/.config`
/// elsewhere. Not worth a crate — this is the whole rule.
fn settings_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("Connected Rails").join("route-editor.ron"))
}
