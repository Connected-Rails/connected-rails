//! Parametric buildings: editable source parameters and their compact runtime form.
//!
//! A route stores [`BuildingSource`]. Terrain compilation validates and normalises its
//! [`BuildingSpec`] once, then hands the renderer a [`BakedBuilding`]. The simulator never
//! edits these values; it only turns the baked specification into cached LOD meshes.

use serde::{Deserialize, Serialize};

/// The broad use controls the facade rhythm and sensible editor defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum BuildingUse {
    #[default]
    Residential,
    Commercial,
    Industrial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum RoofStyle {
    #[default]
    Gable,
    Hip,
    Flat,
    Shed,
    Mansard,
    /// Repeating north-light teeth, characteristic of workshops and factories.
    Sawtooth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum FacadeMaterial {
    #[default]
    Plaster,
    RedBrick,
    YellowBrick,
    Concrete,
    MetalPanel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum RoofMaterial {
    #[default]
    ClayTile,
    Slate,
    StandingSeam,
    Bitumen,
}

/// The complete authoring recipe for one building.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BuildingSpec {
    pub use_kind: BuildingUse,
    /// Outside dimensions [m]. Width is the front, length runs away from it.
    pub width: f32,
    pub length: f32,
    pub floors: u8,
    pub floor_height: f32,
    pub roof_style: RoofStyle,
    pub roof_height: f32,
    pub facade: FacadeMaterial,
    /// Linear sRGB tint multiplied with the shared facade material.
    pub facade_color: [f32; 3],
    pub roof: RoofMaterial,
    /// Approximate horizontal spacing between window centres [m].
    pub window_spacing: f32,
    pub window_width: f32,
    pub window_height: f32,
    /// Windows whose stable hash is below this share light at night.
    pub lit_window_share: f32,
    pub balconies: bool,
    pub balcony_every: u8,
    pub balcony_depth: f32,
    /// Number of masonry chimney stacks distributed over the roof.
    pub chimneys: u8,
    /// Number of metal extraction vents distributed over the roof.
    pub roof_vents: u8,
    /// Number of glazed roof lights. Sawtooth roofs use this as the number of
    /// glazed north-light bays; other roofs receive individual skylights.
    pub skylights: u8,
    /// Eaves and downpipes generated as shared metal detail geometry.
    pub rain_gutters: bool,
    /// Projecting entrance/loading canopy on the front facade.
    pub entrance_canopy: bool,
    /// Front entrances for industrial buildings. For the other uses there is
    /// always one ordinary entrance door.
    pub loading_doors: u8,
    /// Stable design and night-light seed. Copy/paste deliberately preserves it.
    pub seed: u64,
}

impl Default for BuildingSpec {
    fn default() -> Self {
        Self {
            use_kind: BuildingUse::Residential,
            width: 12.0,
            length: 10.0,
            floors: 3,
            floor_height: 2.9,
            roof_style: RoofStyle::Gable,
            roof_height: 3.2,
            facade: FacadeMaterial::Plaster,
            facade_color: [0.82, 0.72, 0.58],
            roof: RoofMaterial::ClayTile,
            window_spacing: 2.6,
            window_width: 1.25,
            window_height: 1.45,
            lit_window_share: 0.38,
            balconies: true,
            balcony_every: 2,
            balcony_depth: 1.4,
            chimneys: 0,
            roof_vents: 0,
            skylights: 0,
            rain_gutters: false,
            entrance_canopy: false,
            loading_doors: 1,
            seed: 1,
        }
    }
}

impl BuildingSpec {
    /// Bounds unsafe or degenerate file values before mesh generation.
    pub fn normalised(&self) -> Self {
        let mut out = self.clone();
        out.width = out.width.clamp(3.0, 150.0);
        out.length = out.length.clamp(3.0, 250.0);
        out.floors = out.floors.clamp(1, 40);
        out.floor_height = out.floor_height.clamp(2.2, 8.0);
        out.roof_height = out.roof_height.clamp(0.0, 20.0);
        out.window_spacing = out.window_spacing.clamp(1.2, 12.0);
        out.window_width = out.window_width.clamp(0.5, out.window_spacing * 0.82);
        out.window_height = out.window_height.clamp(0.5, out.floor_height * 0.72);
        out.lit_window_share = out.lit_window_share.clamp(0.0, 1.0);
        out.balcony_every = out.balcony_every.max(1);
        out.balcony_depth = out.balcony_depth.clamp(0.6, 4.0);
        out.chimneys = out.chimneys.min(8);
        out.roof_vents = out.roof_vents.min(24);
        out.skylights = out.skylights.min(24);
        out.loading_doors = out.loading_doors.clamp(1, 10);
        out
    }

    pub fn wall_height(&self) -> f32 {
        self.floors as f32 * self.floor_height
    }

    pub fn total_height(&self) -> f32 {
        self.wall_height()
            + if self.roof_style == RoofStyle::Flat {
                0.35
            } else {
                self.roof_height
            }
    }

    /// Stable key for the renderer's geometry cache. It includes every value that
    /// changes geometry, vertex tint or the baked window pattern.
    pub fn mesh_key(&self) -> u64 {
        let s = self.normalised();
        let mut h = 0xcbf29ce484222325u64;
        let mut add = |v: u64| {
            h ^= v;
            h = h.wrapping_mul(0x100000001b3);
        };
        add(s.use_kind as u64);
        add(s.width.to_bits() as u64);
        add(s.length.to_bits() as u64);
        add(s.floors as u64);
        add(s.floor_height.to_bits() as u64);
        add(s.roof_style as u64);
        add(s.roof_height.to_bits() as u64);
        add(s.window_spacing.to_bits() as u64);
        add(s.window_width.to_bits() as u64);
        add(s.window_height.to_bits() as u64);
        add(s.lit_window_share.to_bits() as u64);
        add(s.balconies as u64);
        add(s.balcony_every as u64);
        add(s.balcony_depth.to_bits() as u64);
        add(s.chimneys as u64);
        add(s.roof_vents as u64);
        add(s.skylights as u64);
        add(s.rain_gutters as u64);
        add(s.entrance_canopy as u64);
        add(s.loading_doors as u64);
        add(s.seed);
        for channel in s.facade_color {
            add(channel.to_bits() as u64);
        }
        h
    }
}

/// Ready-to-use German building recipes. They are editor conveniences rather
/// than opaque assets: applying one copies every value into [`BuildingSpec`],
/// after which each property remains independently editable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuildingPreset {
    DetachedHouse,
    Farmhouse,
    TownHouse,
    ApartmentBlock,
    OfficeBlock,
    RetailRow,
    Workshop,
    Warehouse,
    FactoryHall,
    LogisticsHall,
}

impl BuildingPreset {
    pub const ALL: [Self; 10] = [
        Self::DetachedHouse,
        Self::Farmhouse,
        Self::TownHouse,
        Self::ApartmentBlock,
        Self::OfficeBlock,
        Self::RetailRow,
        Self::Workshop,
        Self::Warehouse,
        Self::FactoryHall,
        Self::LogisticsHall,
    ];

    pub fn spec(self) -> BuildingSpec {
        match self {
            Self::DetachedHouse => BuildingSpec {
                width: 11.0,
                length: 9.0,
                floors: 2,
                floor_height: 2.8,
                roof_style: RoofStyle::Gable,
                roof_height: 3.1,
                facade_color: [0.91, 0.84, 0.70],
                chimneys: 1,
                rain_gutters: true,
                balconies: false,
                ..Default::default()
            },
            Self::Farmhouse => BuildingSpec {
                width: 18.0,
                length: 11.5,
                floors: 2,
                floor_height: 2.85,
                roof_style: RoofStyle::Gable,
                roof_height: 4.2,
                facade: FacadeMaterial::RedBrick,
                facade_color: [0.94, 0.91, 0.86],
                roof: RoofMaterial::ClayTile,
                window_spacing: 3.0,
                chimneys: 2,
                rain_gutters: true,
                balconies: false,
                ..Default::default()
            },
            Self::TownHouse => BuildingSpec {
                width: 9.0,
                length: 13.0,
                floors: 4,
                floor_height: 3.0,
                roof_style: RoofStyle::Mansard,
                roof_height: 3.6,
                facade_color: [0.78, 0.88, 0.82],
                roof: RoofMaterial::Slate,
                window_spacing: 2.25,
                chimneys: 2,
                rain_gutters: true,
                balconies: false,
                ..Default::default()
            },
            Self::ApartmentBlock => BuildingSpec {
                width: 22.0,
                length: 13.0,
                floors: 5,
                floor_height: 2.85,
                roof_style: RoofStyle::Hip,
                roof_height: 3.0,
                facade_color: [0.82, 0.86, 0.76],
                window_spacing: 2.7,
                chimneys: 3,
                rain_gutters: true,
                balconies: true,
                balcony_every: 1,
                balcony_depth: 1.6,
                ..Default::default()
            },
            Self::OfficeBlock => BuildingSpec {
                use_kind: BuildingUse::Commercial,
                width: 30.0,
                length: 18.0,
                floors: 5,
                floor_height: 3.5,
                roof_style: RoofStyle::Flat,
                roof_height: 0.0,
                facade: FacadeMaterial::Concrete,
                facade_color: [0.78, 0.82, 0.84],
                roof: RoofMaterial::Bitumen,
                window_spacing: 3.0,
                window_width: 1.8,
                window_height: 1.75,
                roof_vents: 4,
                skylights: 4,
                entrance_canopy: true,
                balconies: false,
                ..Default::default()
            },
            Self::RetailRow => BuildingSpec {
                use_kind: BuildingUse::Commercial,
                width: 34.0,
                length: 12.0,
                floors: 2,
                floor_height: 3.7,
                roof_style: RoofStyle::Flat,
                roof_height: 0.0,
                facade_color: [0.82, 0.76, 0.66],
                roof: RoofMaterial::Bitumen,
                window_spacing: 3.4,
                window_width: 2.0,
                window_height: 1.85,
                roof_vents: 2,
                entrance_canopy: true,
                balconies: false,
                ..Default::default()
            },
            Self::Workshop => BuildingSpec {
                use_kind: BuildingUse::Industrial,
                width: 26.0,
                length: 18.0,
                floors: 1,
                floor_height: 5.4,
                roof_style: RoofStyle::Gable,
                roof_height: 3.0,
                facade: FacadeMaterial::RedBrick,
                facade_color: [0.92, 0.90, 0.85],
                roof: RoofMaterial::StandingSeam,
                window_spacing: 4.2,
                window_width: 1.8,
                window_height: 1.8,
                chimneys: 1,
                roof_vents: 2,
                rain_gutters: true,
                entrance_canopy: true,
                loading_doors: 2,
                balconies: false,
                ..Default::default()
            },
            Self::Warehouse => BuildingSpec {
                use_kind: BuildingUse::Industrial,
                width: 52.0,
                length: 30.0,
                floors: 1,
                floor_height: 7.0,
                roof_style: RoofStyle::Shed,
                roof_height: 2.4,
                facade: FacadeMaterial::MetalPanel,
                facade_color: [0.68, 0.75, 0.78],
                roof: RoofMaterial::StandingSeam,
                window_spacing: 6.5,
                window_width: 2.4,
                window_height: 1.8,
                roof_vents: 5,
                skylights: 6,
                rain_gutters: true,
                entrance_canopy: true,
                loading_doors: 4,
                balconies: false,
                ..Default::default()
            },
            Self::FactoryHall => BuildingSpec {
                use_kind: BuildingUse::Industrial,
                width: 64.0,
                length: 38.0,
                floors: 1,
                floor_height: 7.5,
                roof_style: RoofStyle::Sawtooth,
                roof_height: 3.2,
                facade: FacadeMaterial::YellowBrick,
                facade_color: [0.86, 0.84, 0.78],
                roof: RoofMaterial::StandingSeam,
                window_spacing: 6.0,
                window_width: 2.6,
                window_height: 2.1,
                roof_vents: 8,
                skylights: 5,
                rain_gutters: true,
                loading_doors: 3,
                balconies: false,
                ..Default::default()
            },
            Self::LogisticsHall => BuildingSpec {
                use_kind: BuildingUse::Industrial,
                width: 78.0,
                length: 42.0,
                floors: 1,
                floor_height: 8.0,
                roof_style: RoofStyle::Flat,
                roof_height: 0.0,
                facade: FacadeMaterial::MetalPanel,
                facade_color: [0.76, 0.78, 0.74],
                roof: RoofMaterial::Bitumen,
                window_spacing: 7.5,
                window_width: 2.6,
                window_height: 2.0,
                roof_vents: 10,
                skylights: 12,
                rain_gutters: true,
                entrance_canopy: true,
                loading_doors: 8,
                balconies: false,
                ..Default::default()
            },
        }
    }
}

/// A dynamic building placed relative to a track, matching static scenery placement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildingSource {
    pub edge: u32,
    pub s: f64,
    #[serde(default)]
    pub lateral_offset: f64,
    #[serde(default)]
    pub yaw_deg: f64,
    #[serde(default)]
    pub height: f64,
    #[serde(default = "default_true")]
    pub snap_to_terrain: bool,
    #[serde(default)]
    pub spec: BuildingSpec,
}

fn default_true() -> bool {
    true
}

/// Runtime-only product of the terrain bake, already posed in its tile frame.
#[derive(Debug, Clone, PartialEq)]
pub struct BakedBuilding {
    pub pos: [f32; 3],
    pub rotation: [f32; 4],
    pub source_index: u32,
    pub spec: BuildingSpec,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalisation_bounds_file_values() {
        let spec = BuildingSpec {
            width: -1.0,
            floors: 0,
            lit_window_share: 4.0,
            balcony_every: 0,
            chimneys: u8::MAX,
            roof_vents: u8::MAX,
            skylights: u8::MAX,
            loading_doors: 0,
            ..Default::default()
        }
        .normalised();
        assert_eq!(spec.width, 3.0);
        assert_eq!(spec.floors, 1);
        assert_eq!(spec.lit_window_share, 1.0);
        assert_eq!(spec.balcony_every, 1);
        assert_eq!(spec.chimneys, 8);
        assert_eq!(spec.roof_vents, 24);
        assert_eq!(spec.skylights, 24);
        assert_eq!(spec.loading_doors, 1);
    }

    #[test]
    fn copies_have_the_same_mesh_and_light_pattern() {
        let a = BuildingSpec::default();
        let b = a.clone();
        assert_eq!(a.mesh_key(), b.mesh_key());
    }

    #[test]
    fn every_preset_is_valid_and_independently_editable() {
        for preset in BuildingPreset::ALL {
            let spec = preset.spec();
            assert_eq!(spec, spec.normalised());
        }
        let mut factory = BuildingPreset::FactoryHall.spec();
        assert_eq!(factory.roof_style, RoofStyle::Sawtooth);
        assert_eq!(factory.loading_doors, 3);
        factory.width += 4.0;
        assert_ne!(factory, BuildingPreset::FactoryHall.spec());
    }
}
