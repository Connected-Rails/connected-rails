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

/// A rail section (Vignol profile) — what the track's rails are rolled as.
/// The renderer extrudes the real cross-section, so the dimensions here are
/// the ones the drawing shows: DB RL 853 / EN 13674.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RailProfile {
    /// 49E1 (DIN S 49): 149 mm high, 67 mm head, 125 mm foot, 49.4 kg/m.
    /// The Reichsbahn standard, still on lightly loaded branch lines.
    R49,
    /// 54E3 (DIN S 54): 154 mm high, 67 mm head, 125 mm foot, 54.6 kg/m.
    /// DB standard from 1963, main lines and station tracks.
    R54,
    /// 60E1 (UIC 60): 172 mm high, 72 mm head, 150 mm foot, 60.2 kg/m.
    /// Heavy and fast lines since 1970 — the current main-line standard, and
    /// what a type is laid with when it says nothing about its rail.
    #[default]
    R60,
}

impl RailProfile {
    /// (height, head width, foot width) \[m\], measured on the real profile.
    pub fn section(&self) -> (f64, f64, f64) {
        match self {
            Self::R49 => (0.149, 0.067, 0.125),
            Self::R54 => (0.154, 0.067, 0.125),
            Self::R60 => (0.172, 0.072, 0.150),
        }
    }
}

/// How the track is supported: concrete sleepers, wooden sleepers, or a slab
/// (Feste Fahrbahn) instead of a ballast bed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SleeperKind {
    /// Reinforced-concrete sleeper (B 70, B 90, …): trapezoid in section.
    #[default]
    Concrete,
    /// Impregnated timber sleeper (26 × 16 cm section, DB standard length
    /// 2.6 m): rectangular, sits lower than a concrete one.
    Wood,
    /// Feste Fahrbahn: a continuous concrete slab replaces sleepers and
    /// ballast. Of the sleeper fields only length (slab width), height (slab
    /// thickness) and the textures are used; spacing and ballast are not.
    Slab,
}

/// The physical build of the track — the Oberbau: rail section, sleepers and
/// ballast bed, in the real dimensions. Defaults are the DB Regeloberbau
/// (60E1 on B 90 at 60 cm, 30 cm ballast); a type that says nothing about its
/// build is laid like that.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Oberbau {
    /// Rail section the two running rails are extruded from.
    #[serde(default)]
    pub rail: RailProfile,
    /// What supports the rails — and whether there is a ballast bed at all.
    #[serde(default)]
    pub sleeper: SleeperKind,
    /// Sleeper length \[m\] across the track (2.6 m DB standard); on a slab,
    /// the slab width.
    #[serde(default = "default_sleeper_length")]
    pub sleeper_length: f64,
    /// Sleeper base width \[m\] along the track (B 90: 0.32, wood: 0.26); on
    /// a slab, the slab thickness.
    #[serde(default = "default_sleeper_width")]
    pub sleeper_width: f64,
    /// Sleeper height \[m\] (B 70/B 90: 0.21, wood: 0.16); on a slab, the
    /// slab thickness — see [`SleeperKind::Slab`].
    #[serde(default = "default_sleeper_height")]
    pub sleeper_height: f64,
    /// Distance between sleeper centres \[m\]; 0.60 m = 1667 per km, what
    /// almost every DB track is laid at.
    #[serde(default = "default_sleeper_spacing")]
    pub sleeper_spacing: f64,
    /// Ballast shoulder beyond the sleeper end \[m\] each side — the
    /// Schotterfuß over the sleeper underside. The bed's top width is
    /// `sleeper_length + 2 × overhang` (2.6 + 1.4 = 4.0 m, RL 853), its sides
    /// fall 1:1.
    #[serde(default = "default_ballast_overhang")]
    pub ballast_overhang: f64,
    /// Ballast under the sleeper \[m\] (30 cm Hauptbahn) — the bed's depth
    /// from sleeper underside to Planum.
    #[serde(default = "default_ballast_depth")]
    pub ballast_depth: f64,
    /// Sleeper texture (`mods://<mod>/assets/…`); on a slab, the slab's. The
    /// texture repeats 2.6 m along the sleeper — wood plank sets read one
    /// plank per sleeper.
    #[serde(default)]
    pub sleeper_texture: Option<String>,
    /// Sleeper normal map, same tiling as the texture.
    #[serde(default)]
    pub sleeper_normal_map: Option<String>,
}

fn default_sleeper_length() -> f64 {
    2.6
}

fn default_sleeper_width() -> f64 {
    0.32
}

fn default_sleeper_height() -> f64 {
    0.21
}

fn default_sleeper_spacing() -> f64 {
    0.60
}

fn default_ballast_overhang() -> f64 {
    0.70
}

fn default_ballast_depth() -> f64 {
    0.30
}

impl Default for Oberbau {
    fn default() -> Self {
        Self {
            rail: RailProfile::default(),
            sleeper: SleeperKind::default(),
            sleeper_length: default_sleeper_length(),
            sleeper_width: default_sleeper_width(),
            sleeper_height: default_sleeper_height(),
            sleeper_spacing: default_sleeper_spacing(),
            ballast_overhang: default_ballast_overhang(),
            ballast_depth: default_ballast_depth(),
            sleeper_texture: None,
            sleeper_normal_map: None,
        }
    }
}

/// A track type: texture, roughness, how much its surroundings ring,
/// superstructure speed limit and whether a line conductor (LZB) belongs on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackType {
    pub name: String,
    /// Ballast/track texture (`mods://<mod>/assets/…`); `None` = plain color.
    #[serde(default)]
    pub texture: Option<String>,
    /// Ballast normal map, same tiling as the texture.
    #[serde(default)]
    pub normal_map: Option<String>,
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
    /// Superstructure speed limit \[km/h\]; caps the line's speed profile
    /// wherever this type is assigned.
    #[serde(default = "default_max_speed")]
    pub max_speed: f64,
    /// A line conductor (LZB) belongs on this track — the editor's rule check
    /// flags a line that uses the type but places no conductor device.
    #[serde(default)]
    pub lzb: bool,
    /// The physical build — rail section, sleepers, ballast (see [`Oberbau`]).
    #[serde(default)]
    pub oberbau: Oberbau,
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
            normal_map: None,
            color: default_color(),
            roughness: default_roughness(),
            reverb: 0.0,
            max_speed: default_max_speed(),
            lzb: false,
            oberbau: Oberbau::default(),
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
            normal_map: Some("mods://example/assets/track_nor.jpg".into()),
            color: (0.3, 0.3, 0.3),
            roughness: 0.9,
            reverb: 0.8,
            max_speed: 250.0,
            lzb: true,
            oberbau: Oberbau {
                rail: RailProfile::R54,
                sleeper: SleeperKind::Wood,
                sleeper_length: 2.6,
                sleeper_width: 0.26,
                sleeper_height: 0.16,
                sleeper_spacing: 0.6,
                ballast_overhang: 0.7,
                ballast_depth: 0.3,
                sleeper_texture: Some("mods://example/assets/sleeper.jpg".into()),
                sleeper_normal_map: None,
            },
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
        // A type that says nothing about its build is laid like the DB
        // Regeloberbau: 60E1 on concrete at 60 cm.
        assert_eq!(minimal.oberbau.rail, RailProfile::R60);
        assert_eq!(minimal.oberbau.sleeper, SleeperKind::Concrete);
        assert_eq!(minimal.oberbau.sleeper_spacing, 0.60);
        assert_eq!(minimal.oberbau.ballast_depth, 0.30);
        assert!(minimal.oberbau.sleeper_texture.is_none());
    }

    /// An old type file, written before the Oberbau fields existed, must keep
    /// loading — the physical build fills in from defaults.
    #[test]
    fn old_type_files_still_load() {
        let old = "
(
    name: \"Altbau\",
    color: (0.38, 0.33, 0.26),
    roughness: 1.4,
    max_speed: 120.0,
    tags: [\"branch-line\"],
)
";
        let ty = TrackType::from_ron(old).expect("parses");
        assert_eq!(ty.oberbau, Oberbau::default());
    }

    /// Real rail sections — the renderer extrudes these, so they have to be
    /// the rolled dimensions (EN 13674 / DB RL 853).
    #[test]
    fn rail_sections_match_the_rolled_profiles() {
        assert_eq!(RailProfile::R49.section(), (0.149, 0.067, 0.125));
        assert_eq!(RailProfile::R54.section(), (0.154, 0.067, 0.125));
        assert_eq!(RailProfile::R60.section(), (0.172, 0.072, 0.150));
    }
}
