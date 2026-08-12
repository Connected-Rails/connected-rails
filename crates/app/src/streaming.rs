//! Terrain streaming (plan 4.3).
//!
//! The world is tiled in the UTM grid of the elevation data. Tiles are built while
//! driving — inside a load radius around the camera and around **every** train — and
//! discarded again once they fall behind the unload radius. The simulation is untouched
//! by this: track graph and timetable stay resident, AI trains keep running in areas
//! that carry no graphics.
//!
//! The build itself runs on the `AsyncComputeTaskPool`, so a tile of a few thousand
//! triangles never blocks a frame.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use content::terrain::{self, TerrainBuilder, TerrainOptions, TerrainStats, TerrainTile, TileKey};
use glam::DVec2;

use crate::render;
use crate::{Origin, SimResource, TerrainInfo, ui};

/// How many tiles are built at the same time. Higher values do not help — the builder
/// is serialised anyway (see below).
const MAX_PENDING: usize = 8;

/// A tile that is currently in the scene.
struct Loaded {
    entity: Entity,
    vertices: usize,
    triangles: usize,
}

#[derive(Resource)]
pub struct TerrainStreamer {
    // ponytail: a single builder behind a mutex — the DGM cache inside it is shared
    // state, and tile builds therefore run one after another. One source per worker if
    // a single tile at a time turns out to be too slow.
    builder: Arc<Mutex<TerrainBuilder>>,
    options: TerrainOptions,
    materials: Vec<Handle<StandardMaterial>>,
    loaded: HashMap<TileKey, Loaded>,
    pending: HashMap<TileKey, Task<Option<(TerrainTile, TerrainStats)>>>,
    /// Keys outside the line corridor — asked for once, never again.
    empty: HashSet<TileKey>,
    /// Tiles inside this radius around camera and trains are built [m].
    pub load_radius: f64,
    /// Beyond this a tile is discarded again [m]. The gap to `load_radius` is the
    /// hysteresis: without it a tile at the boundary would load and unload every frame.
    pub unload_radius: f64,
    missing: usize,
    tile_loads: usize,
}

impl TerrainStreamer {
    pub fn new(
        builder: TerrainBuilder,
        materials: Vec<Handle<StandardMaterial>>,
        load_radius: f64,
    ) -> Self {
        Self {
            options: *builder.options(),
            builder: Arc::new(Mutex::new(builder)),
            materials,
            loaded: HashMap::new(),
            pending: HashMap::new(),
            empty: HashSet::new(),
            load_radius,
            unload_radius: load_radius * 1.25,
            missing: 0,
            tile_loads: 0,
        }
    }

    /// Tiles being built right now.
    pub fn pending_tiles(&self) -> usize {
        self.pending.len()
    }

    fn stats(&self) -> TerrainStats {
        TerrainStats {
            tiles: self.loaded.len(),
            vertices: self.loaded.values().map(|t| t.vertices).sum(),
            triangles: self.loaded.values().map(|t| t.triangles).sum(),
            missing: self.missing,
            tile_loads: self.tile_loads,
        }
    }
}

/// Loads and discards terrain tiles around camera and trains.
pub fn stream_terrain(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut streamer: ResMut<TerrainStreamer>,
    mut info: ResMut<TerrainInfo>,
    sim: Res<SimResource>,
    origin: Res<Origin>,
    camera: Query<&GlobalTransform, With<ui::CabCamera>>,
) {
    let options = streamer.options;
    // Every train counts, not just the player's — otherwise an AI train would drive
    // through a hole in the world as soon as someone looks at it.
    let mut centers: Vec<DVec2> = sim
        .0
        .trains
        .iter()
        .filter_map(|t| t.vehicles.first())
        .map(|v| terrain::to_utm(v.pos.pose(&sim.0.net).pos, &options))
        .collect();
    if let Ok(camera) = camera.single() {
        let eye = origin.0.from_render(camera.translation());
        centers.push(terrain::to_utm(eye, &options));
    }
    if centers.is_empty() {
        return;
    }
    let nearest = |k: TileKey| {
        centers
            .iter()
            .map(|c| terrain::tile_distance(k, *c, &options))
            .fold(f64::INFINITY, f64::min)
    };

    // Discard what has moved out of range.
    let far: Vec<TileKey> = streamer
        .loaded
        .keys()
        .copied()
        .filter(|k| nearest(*k) > streamer.unload_radius)
        .collect();
    for k in far {
        if let Some(tile) = streamer.loaded.remove(&k) {
            commands.entity(tile.entity).despawn();
        }
    }

    // Request what is missing — nearest first, so the tile under the train wins.
    let free = MAX_PENDING.saturating_sub(streamer.pending.len());
    if free > 0 {
        let mut wanted: HashSet<TileKey> = HashSet::new();
        for c in &centers {
            wanted.extend(terrain::keys_near(*c, streamer.load_radius, &options));
        }
        let mut candidates: Vec<(f64, TileKey)> = wanted
            .into_iter()
            .filter(|k| {
                !streamer.loaded.contains_key(k)
                    && !streamer.pending.contains_key(k)
                    && !streamer.empty.contains(k)
            })
            .map(|k| (nearest(k), k))
            .collect();
        candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));

        let pool = AsyncComputeTaskPool::get();
        for (_, k) in candidates.into_iter().take(free) {
            let builder = streamer.builder.clone();
            let task = pool.spawn(async move {
                let mut stats = TerrainStats::default();
                let mut builder = builder.lock().expect("terrain builder");
                let tile = builder.build_key(k, &mut stats)?;
                stats.tile_loads = builder.load_count();
                Some((tile, stats))
            });
            streamer.pending.insert(k, task);
        }
    }

    // Take over what is finished.
    let mut finished: Vec<(TileKey, Option<(TerrainTile, TerrainStats)>)> = Vec::new();
    for (k, task) in streamer.pending.iter_mut() {
        if let Some(result) = block_on(poll_once(&mut *task)) {
            finished.push((*k, result));
        }
    }
    for (k, result) in finished {
        streamer.pending.remove(&k);
        let Some((tile, stats)) = result else {
            streamer.empty.insert(k);
            continue;
        };
        let entity = render::spawn_terrain_tile(
            &mut commands,
            &mut meshes,
            &streamer.materials,
            &tile,
            &origin.0,
        );
        streamer.missing += stats.missing;
        streamer.tile_loads = stats.tile_loads;
        streamer.loaded.insert(
            k,
            Loaded {
                entity,
                vertices: tile.positions.len(),
                triangles: tile.triangles(),
            },
        );
    }

    info.0 = streamer.stats();
}
