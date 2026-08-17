//! Translations of everything the user reads (Fluent, `locales/<lang>/main.ftl`).
//!
//! ```
//! # use i18n::t;
//! i18n::set_language("en");
//! assert_eq!(t!("menu-file"), "File");
//! assert_eq!(t!("status-loaded", file = "br101.ron"), "br101.ron loaded");
//! ```
//!
//! The language comes from `TRAINSIM_LANG`, otherwise from the operating system,
//! otherwise English. Unknown keys yield the key itself — a missing translation
//! shows up in the UI instead of taking the program down.

use fluent_templates::{Loader, static_loader};
use std::sync::RwLock;
use unic_langid::{LanguageIdentifier, langid};

pub use fluent_templates::fluent_bundle::FluentValue;

static_loader! {
    static LOCALES = {
        locales: "./locales",
        fallback_language: "en",
        // Fluent wraps placeholders in bidi isolates by default; egui draws them as boxes.
        customise: |bundle| bundle.set_use_isolating(false),
    };
}

/// Languages shipped with the game, in menu order.
pub const LANGUAGES: &[(&str, &str)] = &[("en", "English"), ("de", "Deutsch")];

const EN: LanguageIdentifier = langid!("en");

static CURRENT: RwLock<Option<LanguageIdentifier>> = RwLock::new(None);

fn current() -> LanguageIdentifier {
    if let Some(lang) = CURRENT.read().ok().and_then(|l| l.clone()) {
        return lang;
    }
    let lang = detect();
    set_language(&lang.to_string());
    lang
}

/// `TRAINSIM_LANG`, else the system language, else English.
fn detect() -> LanguageIdentifier {
    std::env::var("TRAINSIM_LANG")
        .ok()
        .or_else(sys_locale::get_locale)
        .and_then(|tag| parse(&tag))
        .unwrap_or(EN)
}

/// Accepts full tags (`de-DE`) and matches them against the languages we ship.
fn parse(tag: &str) -> Option<LanguageIdentifier> {
    let base = tag.split(['-', '_']).next()?;
    LANGUAGES
        .iter()
        .find(|(code, _)| *code == base)
        .and_then(|(code, _)| code.parse().ok())
}

/// Switches the language; unknown ones fall back to English.
pub fn set_language(tag: &str) {
    let lang = parse(tag).unwrap_or(EN);
    if let Ok(mut current) = CURRENT.write() {
        *current = Some(lang);
    }
}

/// The language currently in use, as a code from [`LANGUAGES`].
pub fn language() -> String {
    current().to_string()
}

/// A number with the decimal separator of the current language — German writes
/// `7,0 km` where English writes `7.0 km`.
///
/// Numbers are formatted in Rust and handed to Fluent as text (that is what keeps the
/// HUD's columns lined up), so the separator cannot come out of the message itself.
pub fn decimal(value: f64, decimals: usize) -> String {
    let text = format!("{value:.decimals$}");
    match language().as_str() {
        "de" => text.replace('.', ","),
        _ => text,
    }
}

/// Looks a message up; use [`t!`] instead.
pub fn lookup(key: &str) -> String {
    LOCALES
        .try_lookup(&current(), key)
        .unwrap_or_else(|| key.to_string())
}

/// Looks a message up, or `None` if no language has it.
///
/// For text a caller may leave out — a field tooltip, say. [`lookup`] would
/// hand back the key itself, which is right for a label (it shows up as an
/// obvious defect) but wrong for a tooltip, where it would put `veh-mass-hint`
/// on screen instead of simply not opening one.
pub fn maybe(key: &str) -> Option<String> {
    LOCALES.try_lookup(&current(), key)
}

/// Arguments of a message: placeholder name → value.
pub type Args<'a> = std::collections::HashMap<std::borrow::Cow<'static, str>, FluentValue<'a>>;

/// Looks a message with placeholders up; use [`t!`] instead.
pub fn lookup_args(key: &str, args: &Args) -> String {
    LOCALES
        .try_lookup_with_args(&current(), key, args)
        .unwrap_or_else(|| key.to_string())
}

/// `t!("menu-file")` or `t!("status-loaded", file = path)`.
#[macro_export]
macro_rules! t {
    ($key:expr) => {
        $crate::lookup($key)
    };
    ($key:expr, $($name:ident = $value:expr),+ $(,)?) => {{
        let mut args = $crate::Args::new();
        $(args.insert(
            std::borrow::Cow::Borrowed(stringify!($name)),
            $crate::FluentValue::from($value.to_string()),
        );)+
        $crate::lookup_args($key, &args)
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Message identifiers of an `.ftl` file — a message starts at the left margin,
    /// continuation lines and comments are indented or begin with `#`.
    fn keys(ftl: &str) -> Vec<&str> {
        ftl.lines()
            .filter(|l| l.starts_with(|c: char| c.is_ascii_lowercase()))
            .filter_map(|l| l.split_once(" ="))
            .map(|(key, _)| key)
            .collect()
    }

    /// Every language must answer every key of the source — otherwise the UI
    /// silently drops back to English in the middle of a panel.
    #[test]
    fn all_languages_are_complete() {
        let english = keys(include_str!("../locales/en/main.ftl"));
        assert!(english.len() > 100, "{} keys only", english.len());
        let german = keys(include_str!("../locales/de/main.ftl"));
        let missing: Vec<&&str> = english.iter().filter(|k| !german.contains(k)).collect();
        assert!(missing.is_empty(), "de is missing {missing:?}");
        let extra: Vec<&&str> = german.iter().filter(|k| !english.contains(k)).collect();
        assert!(extra.is_empty(), "de has orphans {extra:?}");
    }

    /// One test for everything that touches the global language — the tests of a
    /// crate share it and would otherwise pull it out from under each other.
    #[test]
    fn language_switches_and_fills_placeholders_in() {
        set_language("en");
        assert_eq!(t!("status-loaded", file = "br101.ron"), "br101.ron loaded");
        set_language("de-DE");
        assert_eq!(language(), "de");
        assert_eq!(t!("status-loaded", file = "br101.ron"), "br101.ron geladen");
        assert_eq!(decimal(7.0, 1), "7,0", "German writes the comma");
        set_language("fr");
        assert_eq!(language(), "en", "unknown languages fall back to English");
        assert_eq!(decimal(7.0, 1), "7.0");
        assert_eq!(t!("no-such-key"), "no-such-key");
    }
}
