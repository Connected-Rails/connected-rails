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

/// A track type: texture, roughness, how much its surroundings ring,
/// superstructure speed limit and whether a line conductor (LZB) belongs on it.
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
    /// How much the surroundings ring, 0 = open line, 1 = tunnel. Drives the
    /// reverb the app mixes under everything the player hears; a station hall,
    /// a cutting or an overbridge sit in between. Modelling it on the track
    /// type rather than on the terrain is the same trade `roughness` makes:
    /// a line says where its tunnels are by assigning the type, and no one has
    /// to trace geometry at run time.
    #[serde(default)]
    pub reverb: f64,
    /// Superstructure speed limit [km/h]; caps the line's speed profile
    /// wherever this type is assigned.
    #[serde(default = "default_max_speed")]
    pub max_speed: f64,
    /// A line conductor (LZB) belongs on this track — the editor's rule check
    /// flags a line that uses the type but places no conductor device.
    #[serde(default)]
    pub lzb: bool,
    /// Free-form tags the mod author gives the entry, for finding it again in
    /// a catalogue of thousands: `["mast", "catenary", "epoch-4"]`. Lower-case
    /// kebab by convention — the editors normalise what is typed, and the
    /// content drawer lower-cases when it groups, so a hand-written `Mast`
    /// still lands on the same tag.
    #[serde(default)]
    pub tags: Vec<String>,
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
            reverb: 0.0,
            max_speed: default_max_speed(),
            lzb: false,
            tags: Vec::new(),
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
            reverb: 0.8,
            max_speed: 250.0,
            lzb: true,
            tags: vec!["hauptbahn".into()],
        };
        let back = TrackType::from_ron(&full.to_ron()).expect("parses");
        assert_eq!(back, full);

        // A minimal file only needs the name.
        let minimal = TrackType::from_ron("(name:\"nebenbahn\")").expect("parses");
        assert_eq!(minimal.max_speed, default_max_speed());
        // A type that says nothing about its surroundings is open line, not a tunnel.
        assert_eq!(minimal.reverb, 0.0);
        assert!(!minimal.lzb);
        assert!(minimal.texture.is_none());
    }
}
