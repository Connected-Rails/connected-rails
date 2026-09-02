//! Track types (superstructure classes) a mod defines — what the track is
//! built like, not where it runs (plan ch. 15).
//!
//! A mod ships them as `track_types/*.ron`, addressed `"<mod>:<name>"` like
//! signal types. A line assigns them per edge as a step profile over the arc
//! length, so one edge can change its type section by section, and it has to
//! name one — there is no built-in type a track falls back on. The network
//! stores resolved specs in [`TrackNetwork::types`], in the order the line
//! first names them.
//!
//! The physical build the type describes — rail section, sleepers, ballast —
//! lives in [`crate::oberbau`], in the dimensions the DB drawings give.
//!
//! [`TrackNetwork::types`]: crate::TrackNetwork

use serde::{Deserialize, Serialize};

pub use crate::oberbau::{Fastening, Oberbau, RailProfile, RailSection, SleeperKind};

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
    /// Ballast height map (white = high). With one, the bed is drawn with
    /// parallax mapping, which is what turns a photograph of ballast from a
    /// grey plane into stones that stand between the sleepers.
    #[serde(default)]
    pub depth_map: Option<String>,
    /// Ballast ambient occlusion (white = open sky). Ballast is mostly
    /// shadow: the gaps between the stones see almost no sky, and without
    /// that the bed lights up evenly and reads as gravel-coloured paint.
    #[serde(default)]
    pub occlusion_map: Option<String>,
    /// How many metres one repeat of those three covers, along the track and
    /// across it. A ballast scan is a photograph of a known patch of ground;
    /// getting this wrong is what makes a bed read as asphalt (too small) or
    /// as rubble (too large). 31.5/63 mm ballast wants the scan's own size —
    /// 1.5 m for most ambientCG gravel.
    #[serde(default = "default_texture_scale")]
    pub texture_scale: f64,
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
    // Ballast grey: what a type is skinned in that names no texture of its own.
    (0.32, 0.30, 0.28)
}

fn default_texture_scale() -> f64 {
    // What the example mod's scans cover; also close enough for anything
    // hand-made that nobody has measured.
    1.5
}

fn default_roughness() -> f64 {
    1.0
}

fn default_max_speed() -> f64 {
    // High enough to never cap; the line's own speed profile rules.
    1000.0
}

/// The field defaults a `track_types/*.ron` may leave out — **not** a type
/// anything falls back on: a track names its type or does not compile
/// (`content::route::CompileError::MissingTrackType`).
impl Default for TrackType {
    fn default() -> Self {
        Self {
            name: "default".into(),
            texture: None,
            normal_map: None,
            depth_map: None,
            occlusion_map: None,
            texture_scale: default_texture_scale(),
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
            depth_map: Some("mods://example/assets/track_disp.jpg".into()),
            occlusion_map: Some("mods://example/assets/track_ao.jpg".into()),
            texture_scale: 1.5,
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
                sleeper_top_width: None,
                sleeper_height: 0.16,
                sleeper_mid_height: None,
                sleeper_spacing: 0.6,
                rail_pad: 0.01,
                fastening: Some(Fastening::K),
                ballast_overhang: 0.4,
                ballast_depth: 0.3,
                ballast_slope: 1.5,
                crib_drop: 0.03,
                sleeper_texture: Some("mods://example/assets/sleeper.jpg".into()),
                sleeper_normal_map: None,
                sleeper_texture_scale: Some(2.6),
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
        // The shape fields nobody set follow the sleeper kind: a B 70 is
        // cast with draft and shallower in the middle, and it is fastened
        // with W 14.
        assert!((minimal.oberbau.top_width() - 0.22).abs() < 1e-9);
        assert!((minimal.oberbau.mid_height() - 0.175).abs() < 1e-9);
        assert_eq!(minimal.oberbau.fastening(), Fastening::W14);
        assert_eq!(minimal.oberbau.texture_scale(), 1.0);
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
}
