//! Track types (superstructure classes) a mod defines — what the track is
//! built like, not where it runs (plan ch. 15).
//!
//! A mod ships them as `track_types/*.ron`, addressed `"<mod>:<name>"` like
//! signal types. A line assigns them per edge as a step profile over the arc
//! length, so one edge can change its type section by section. The network
//! stores resolved specs in [`TrackNetwork::types`]; index 0 is always the
//! default type.
//!
//! [`TrackNetwork::types`]: crate::TrackNetwork

use serde::{Deserialize, Serialize};

/// A track type: texture, roughness, superstructure speed limit and whether a
/// line conductor (LZB) belongs on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackType {
    pub name: String,
    /// Ballast/track texture (`mods://<mod>/assets/…`); `None` = plain color.
    #[serde(default)]
    pub texture: Option<String>,
    /// Untextured track color and the editor's section tint (sRGB 0..1).
    #[serde(default = "default_color")]
    pub color: (f32, f32, f32),
    /// Roughness factor, 1.0 = welded main-line rail. Scales the rolling
    /// noise; jointed or worn track sits above 1, slab track below.
    #[serde(default = "default_roughness")]
    pub roughness: f64,
    /// Superstructure speed limit [km/h]; caps the line's speed profile
    /// wherever this type is assigned.
    #[serde(default = "default_max_speed")]
    pub max_speed: f64,
    /// A line conductor (LZB) belongs on this track — the editor's rule check
    /// flags a line that uses the type but places no conductor device.
    #[serde(default)]
    pub lzb: bool,
}

fn default_color() -> (f32, f32, f32) {
    // The ballast grey the app has always used — the default type must not
    // change the look of a line without types.
    (0.32, 0.30, 0.28)
}

fn default_roughness() -> f64 {
    1.0
}

fn default_max_speed() -> f64 {
    // High enough to never cap; the line's own speed profile rules.
    1000.0
}

impl Default for TrackType {
    fn default() -> Self {
        Self {
            name: "default".into(),
            texture: None,
            color: default_color(),
            roughness: default_roughness(),
            max_speed: default_max_speed(),
            lzb: false,
        }
    }
}

impl TrackType {
    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }

    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).expect("serializable")
    }

    /// Placeholder for a type the runtime has not resolved (unknown mod or
    /// name): default properties, the name kept so it stays addressable.
    pub fn placeholder(name: &str) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ron_roundtrip_with_defaults() {
        let full = TrackType {
            name: "hauptbahn-lzb".into(),
            texture: Some("mods://example/assets/track.png".into()),
            color: (0.3, 0.3, 0.3),
            roughness: 0.9,
            max_speed: 250.0,
            lzb: true,
        };
        let back = TrackType::from_ron(&full.to_ron()).expect("parses");
        assert_eq!(back, full);

        // A minimal file only needs the name.
        let minimal = TrackType::from_ron("(name:\"nebenbahn\")").expect("parses");
        assert_eq!(minimal.max_speed, default_max_speed());
        assert!(!minimal.lzb);
        assert!(minimal.texture.is_none());
    }
}
