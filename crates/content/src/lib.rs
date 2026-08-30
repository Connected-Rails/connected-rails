//! Content: vehicle database, line format, scenarios and example line (plan ch. 15).

#[cfg(test)]
mod area_tests;
pub mod characters;
pub mod compose;
pub mod demo;
pub mod farmland;
pub mod import;
pub mod people;
pub mod roads;
pub mod route;
pub mod scenarios;
pub mod terrain;
pub mod vehicles;
pub mod water;

pub use characters::{CharacterSpec, Gender, Role};
pub use compose::Composition;
pub use demo::musterbahn;
pub use farmland::{FieldPatch, Fields};
pub use import::{ImportOptions, ImportReport, import_line};
pub use people::{
    Crowd, PersonInstance, Pose, StrollAgent, StrollPose, Walkway, WalkwayKind, WalkwayNode,
    embedded_walkways, stroll_pose,
};
pub use roads::{RoadPatch, Roads};
pub use route::{CompiledLine, FieldSource, LineSource, TreeSource};
pub use scenarios::{musterbahn_day, re_4711, to_musterstadt};
pub use terrain::{
    Scenery, SceneryInstance, TerrainBuilder, TerrainEdits, TerrainOptions, TerrainStats,
    TerrainTile, TileKey, Tree, Vegetation,
};
pub use water::{WaterPatch, Waters};
