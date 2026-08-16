//! Content: vehicle database, line format, scenarios and example line (plan ch. 15).

pub mod compose;
pub mod demo;
pub mod import;
pub mod route;
pub mod scenarios;
pub mod terrain;
pub mod vehicles;

pub use compose::Composition;
pub use demo::musterbahn;
pub use import::{ImportOptions, ImportReport, import_line};
pub use route::{CompiledLine, LineSource, TreeSource};
pub use scenarios::{re_4711, to_musterstadt};
pub use terrain::{
    TerrainBuilder, TerrainEdits, TerrainOptions, TerrainStats, TerrainTile, TileKey, Tree,
    Vegetation,
};
