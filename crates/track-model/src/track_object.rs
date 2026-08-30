//! Scenery objects with a track reference (plan ch. 15) — a mod's 3D object
//! that is placed *relative to the track*: catenary masts, kilometre boards,
//! platform lamps.
//!
//! A mod ships them as `objects/*.ron`, addressed `"<mod>:<name>"`. The object
//! carries its own placement defaults — lateral offset, rotation, height — so
//! the route editor drops it at the distance and orientation its author meant;
//! every placed instance stores concrete values and can deviate.

use serde::{Deserialize, Serialize};

/// A placeable 3D object and the pose its author defined relative to the track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackObject {
    pub name: String,
    /// glTF below the mods directory (`example/assets/mast.gltf`), loaded as
    /// `mods://…` — the same path scheme as vehicle and signal models.
    pub model: String,
    /// Default lateral offset [m], positive = right of increasing arc length.
    #[serde(default)]
    pub lateral_offset: f64,
    /// Default rotation about the up axis [deg], clockwise seen from above;
    /// 0 = the model's front (−Z, the convention every vehicle and character uses)
    /// points along increasing arc length.
    #[serde(default)]
    pub yaw_deg: f64,
    /// Default height above the railhead [m].
    #[serde(default)]
    pub height: f64,
    /// Optional seasonal variants of the model — a mod that has an autumn or a
    /// winter version of a tree names it here, with its own textures inside the
    /// glTF. Whatever is missing falls back to `model`, and an object that
    /// looks the same all year (mast, board, lamp) names neither.
    #[serde(default)]
    pub autumn_model: Option<String>,
    #[serde(default)]
    pub winter_model: Option<String>,
    /// How far each level of detail of the model reaches \[m\], finest first —
    /// `crown_LOD0` up to the first entry, `crown_LOD1` up to the second, and
    /// the last entry is where the object stops being drawn at all.
    ///
    /// Empty means the renderer's own bands, which is what a mast or a hut
    /// wants. Vegetation names its own, because the distance at which a level
    /// stops paying for itself is a matter of how big the plant is: a two metre
    /// blackthorn is a smudge where a forty metre fir is still a tree, and a
    /// wood of blackthorn drawn to two kilometres is tens of thousands of draw
    /// calls for nothing.
    #[serde(default)]
    pub lod_distances: Vec<f32>,
    /// Free-form tags the mod author gives the entry, for finding it again in
    /// a catalogue of thousands: `["mast", "catenary", "epoch-4"]`. Lower-case
    /// kebab by convention — the editors normalise what is typed, and the
    /// content drawer lower-cases when it groups, so a hand-written `Mast`
    /// still lands on the same tag.
    #[serde(default)]
    pub tags: Vec<String>,
}

impl TrackObject {
    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }

    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).expect("serializable")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ron_roundtrip_with_defaults() {
        let full = TrackObject {
            name: "Oberleitungsmast".into(),
            model: "example/assets/mast.gltf".into(),
            lateral_offset: -3.5,
            yaw_deg: 90.0,
            height: 0.0,
            autumn_model: None,
            winter_model: Some("example/assets/mast_winter.gltf".into()),
            lod_distances: vec![80.0, 260.0, 800.0, 2_500.0],
            tags: vec!["mast".into(), "epoch-4".into()],
        };
        assert_eq!(TrackObject::from_ron(&full.to_ron()).unwrap(), full);

        // A minimal file needs only name and model; seasonal variants and the
        // level bands are optional.
        let minimal =
            TrackObject::from_ron("(name:\"Baum\",model:\"x/assets/tree.gltf\")").unwrap();
        assert_eq!(minimal.lateral_offset, 0.0);
        assert_eq!(minimal.yaw_deg, 0.0);
        assert_eq!(minimal.winter_model, None);
        assert!(minimal.lod_distances.is_empty());
    }
}
