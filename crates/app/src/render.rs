//! Floating-origin synchronisation of what the simulator draws itself
//! (plan ch. 4, 12) — the world outside the cab comes from `world-render`.

use bevy::prelude::*;
use world_coords::RenderOrigin;
pub use world_render::{
    Season, TerrainMaterial, WorldAnchored, WorldCatalog, spawn_terrain_tile, spawn_track,
    terrain_material,
};

/// Reference point of the rendering as a Bevy resource.
#[derive(Resource)]
pub struct Origin(pub RenderOrigin);

/// A terrain tile — with its own view distance so that distant tiles are not drawn.
#[derive(Component)]
pub struct TerrainChunk {
    /// Circumscribed radius of the tile [m].
    pub radius: f32,
    pub lod: u8,
}

/// A vehicle in train `train`, vehicle index `vehicle`.
#[derive(Component, Clone, Copy)]
pub struct VehicleView {
    pub train: usize,
    pub vehicle: usize,
}

/// Sets the transforms of all world-anchored objects anew — after an origin rebase.
pub fn resync_anchored(origin: &RenderOrigin, query: &mut Query<(&WorldAnchored, &mut Transform)>) {
    for (anchored, mut transform) in query.iter_mut() {
        let (translation, rotation) = anchored.transform(origin);
        transform.translation = translation;
        transform.rotation = rotation;
    }
}
