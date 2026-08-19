//! Aerial imagery overlay: tile math, providers, cache and fetching.
//!
//! No Bevy — the editor merely hooks this up to its rendering.

pub mod cache;
pub mod config;
pub mod geocode;
pub mod source;
pub mod tiles;

pub use cache::{CacheKey, CacheStats, TileCache};
pub use config::{
    CacheConfig, ImageFormat, ImageryConfig, Provider, RequestConfig, TileUrl, ZoomMode,
    predefined_providers,
};
pub use geocode::Place;
pub use source::{DecodedTile, ImagerySource, TileState};
pub use tiles::TileId;
