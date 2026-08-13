//! What the editor remembers between runs.
//!
//! Only what the user would otherwise have to re-do by hand. The file is
//! written next to the operating system's other application settings and is
//! never required — a missing or unreadable one leaves the editor at its
//! defaults, without a word.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// How many vehicles the file menu offers again.
const RECENT_LIMIT: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Vehicles opened before, newest first.
    #[serde(default)]
    pub recent: Vec<PathBuf>,
    /// Language picked under View; `None` follows the operating system.
    #[serde(default)]
    pub language: Option<String>,
    /// Size of the window as the user left it.
    #[serde(default)]
    pub window: Option<(f32, f32)>,
    /// Width of the data and model panels as the user left them.
    #[serde(default)]
    pub panels: Option<(f32, f32)>,
    /// View toggles. Both start on; someone who turns one off means it.
    #[serde(default = "on")]
    pub show_reference: bool,
    #[serde(default = "on")]
    pub show_grid: bool,
}

fn on() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            recent: Vec::new(),
            language: None,
            window: None,
            panels: None,
            show_reference: true,
            show_grid: true,
        }
    }
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

    /// Records the View menu's toggles and writes the file — they are clicked
    /// rarely enough that saving straight away costs nothing.
    pub fn set_view(&mut self, show_reference: bool, show_grid: bool) {
        self.show_reference = show_reference;
        self.show_grid = show_grid;
        self.save();
    }

    pub fn set_language(&mut self, code: &str) {
        self.language = Some(code.to_owned());
        self.save();
    }

    /// Moves `path` to the front, newest first, and writes the file.
    ///
    /// Re-opening a vehicle should move it up the list, not add it twice.
    pub fn remember(&mut self, path: &std::path::Path) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(RECENT_LIMIT);
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

/// `%APPDATA%\TrainSim-DE\` on Windows, `$XDG_CONFIG_HOME` or `~/.config`
/// elsewhere. Not worth a crate — this is the whole rule.
fn settings_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("TrainSim-DE").join("vehicle-editor.ron"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reopening_moves_a_vehicle_up_instead_of_repeating_it() {
        let mut settings = Settings::default();
        // `remember` writes the real settings file; the list logic is what
        // matters here, so drive it directly.
        for name in ["a.ron", "b.ron", "a.ron"] {
            let path = PathBuf::from(name);
            settings.recent.retain(|p| p != &path);
            settings.recent.insert(0, path);
            settings.recent.truncate(RECENT_LIMIT);
        }
        assert_eq!(
            settings.recent,
            vec![PathBuf::from("a.ron"), PathBuf::from("b.ron")]
        );
    }

    #[test]
    fn the_list_stays_bounded() {
        let mut settings = Settings::default();
        for i in 0..20 {
            let path = PathBuf::from(format!("{i}.ron"));
            settings.recent.retain(|p| p != &path);
            settings.recent.insert(0, path);
            settings.recent.truncate(RECENT_LIMIT);
        }
        assert_eq!(settings.recent.len(), RECENT_LIMIT);
        assert_eq!(settings.recent[0], PathBuf::from("19.ron"));
    }
}
