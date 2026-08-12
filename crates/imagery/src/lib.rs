//! Luftbild-Overlay: Kachelrechnung, Anbieter, Cache und Beschaffung.
//!
//! Ohne Bevy — der Editor bindet das hier nur an sein Rendering an.

pub mod cache;
pub mod config;
pub mod source;
pub mod tiles;

pub use cache::{CacheKey, CacheStats, TileCache};
pub use config::{
    CacheConfig, ImageFormat, ImageryConfig, Provider, RequestConfig, TileUrl, ZoomMode,
    predefined_providers,
};
pub use source::{DecodedTile, ImagerySource, TileState};
pub use tiles::TileId;
