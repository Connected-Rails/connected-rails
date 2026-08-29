//! People (plan ch. 12): the models a mod ships for the player's walker and for the
//! passengers on the platforms and in the coaches.
//!
//! A mod ships them as `characters/*.ron`, addressed `"<mod>:<file stem>"`. The file
//! names a glTF and says what the person is for; everything else is convention in the
//! model itself, so the app needs no per-character tables:
//!
//! - **Origin and axes:** metres, Y up, the origin on the ground between the feet, the
//!   face towards −Z — the same frame the walker and the vehicles use, so a yaw about Y
//!   turns the person and a translation puts it down where it stands.
//! - **Levels of detail:** the skinned mesh comes as nodes `char_LOD0` … `char_LOD3`
//!   (the `_LOD<n>` convention every model in the game shares), finest first. All of
//!   them hang on one skeleton, so a level switches without a pose change.
//! - **Clips**, in families told apart by name, any number of each:
//!   - `idle`, `idle2`, `idle3`, … — looping stands with a little life in them, ten to
//!     twenty seconds each. A standing person plays one of them, picked by its
//!     [`crate::people::Pose`] variant.
//!   - `sit`, `sit2`, … — seated, feet on the floor, seat about 0.45 m up; looping like
//!     the idles, or a held frame. A passenger plays one.
//!   - `walk_<cm/s>` — one gait cycle, in place, looped, named after the pace it was
//!     made for (`walk_120` covers 1.2 m/s at its own speed); a plain `walk` is taken
//!     for 1.5 m/s. A walking person plays the one nearest its pace, sped up or down
//!     to cover the ground, so a model with a stroll and a hurry uses both.
//!   - `stand`, `stand2`, … — held standing frames, what a model without idles
//!     falls back to.
//!
//!   A character may lack whole families; the app falls back to the nearest one it
//!   has (a stand for a missing idle, an idle for a missing sit) and, failing
//!   everything, plays any clip at all.
//!
//! The shipped `people` mod is generated out of MakeHuman 2 by `tools/characters/`,
//! its clips retargeted from motion capture recordings (`tools/characters/clips.json`).

use serde::{Deserialize, Serialize};

/// A person model out of a mod (`characters/*.ron`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CharacterSpec {
    /// Display name — content, not translated.
    pub name: String,
    /// glTF below the mods directory (`people/assets/f01_lena.glb`), loaded as
    /// `mods://…` — the same path scheme as vehicle and signal models.
    pub model: String,
    #[serde(default)]
    pub gender: Gender,
    /// What the model is used for. A character no role names is loaded but never
    /// picked by the app — a mod's own scenario may still place it.
    #[serde(default = "default_roles")]
    pub roles: Vec<Role>,
    /// Height in metres, as the roster records it (informational: the model is
    /// the truth).
    #[serde(default)]
    pub height: f32,
    /// Free-form tags the mod author gives the entry (`["young", "casual"]`),
    /// lower-case kebab by convention like everywhere else.
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_roles() -> Vec<Role> {
    vec![Role::Player, Role::Passenger]
}

/// Gender of the person modelled, as far as the picker needs it (a walk clip, a
/// balanced crowd).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Gender {
    Female,
    Male,
    #[default]
    Unspecified,
}

/// What a character model may be used as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Role {
    /// The walker's own body, seen from the outside cameras.
    Player,
    /// One of the crowd on a platform, or in a seat of a coach.
    Passenger,
}

impl CharacterSpec {
    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }

    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).expect("serializable")
    }

    pub fn has_role(&self, role: Role) -> bool {
        self.roles.contains(&role)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ron_roundtrip_with_defaults() {
        let full = CharacterSpec {
            name: "Lena".into(),
            model: "people/assets/f01_lena.glb".into(),
            gender: Gender::Female,
            roles: vec![Role::Passenger],
            height: 1.68,
            tags: vec!["young".into(), "casual".into()],
        };
        assert_eq!(CharacterSpec::from_ron(&full.to_ron()).unwrap(), full);
        assert!(full.has_role(Role::Passenger));
        assert!(!full.has_role(Role::Player));

        // A minimal file needs only name and model; such a person does everything.
        let minimal = CharacterSpec::from_ron("(name:\"Max\",model:\"x/assets/max.glb\")").unwrap();
        assert_eq!(minimal.gender, Gender::Unspecified);
        assert!(minimal.has_role(Role::Player) && minimal.has_role(Role::Passenger));
        assert_eq!(minimal.height, 0.0);
        assert!(minimal.tags.is_empty());
    }
}
