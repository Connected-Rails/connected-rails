//! Terrain streaming (plan 4.3).
//!
//! The world is tiled in the UTM grid of the elevation data. Tiles are built while
//! driving — inside a load radius around the camera and around **every** train — and
//! discarded again once they fall behind the unload radius. The simulation is untouched
//! by this: track graph and timetable stay resident, AI trains keep running in areas
//! that carry no graphics.
//!
//! A tile carries its ground, its trees and its scenery objects; all three
//! stream together (`world_render::spawn_terrain_tile`). The build itself runs
//! on the `AsyncComputeTaskPool` — several at a time, the builder is shared
//! read-only and the elevation data keeps its own short lock — so a tile of a
//! few thousand triangles never blocks a frame.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use content::terrain::{self, TerrainBuilder, TerrainOptions, TerrainStats, TerrainTile, TileKey};
use glam::DVec2;

use crate::render;
use crate::{Origin, SimResource, TerrainInfo, ui};

/// How many tiles are built at the same time. One per worker thread is the
/// most that can run; the rest only queue.
const MAX_PENDING: usize = 8;

/// A tile that is currently in the scene.
struct Loaded {
    entity: Entity,
    vertices: usize,
    triangles: usize,
}

#[derive(Resource)]
pub struct TerrainStreamer {
    builder: Arc<TerrainBuilder>,
    options: TerrainOptions,
    material: Handle<render::TerrainMaterial>,
    catalog: render::WorldCatalog,
    loaded: HashMap<TileKey, Loaded>,
    pending: HashMap<TileKey, Task<Option<(TerrainTile, TerrainStats)>>>,
    /// Keys outside the line corridor — asked for once, never again.
    empty: HashSet<TileKey>,
    /// Tiles inside this radius around camera and trains are built [m].
    pub load_radius: f64,
    /// Beyond this a tile is discarded again [m]. The gap to `load_radius` is the
    /// hysteresis: without it a tile at the boundary would load and unload every frame.
    pub unload_radius: f64,
    /// The tiles the centres stood on when `wanted` was last worked out. A
    /// centre that has not left its tile wants the same tiles as before, so
    /// the ring around every train is not laid out again each frame.
    center_tiles: Vec<TileKey>,
    /// Tiles in the load radius, nearest first.
    wanted: Vec<TileKey>,
    missing: usize,
    tile_loads: usize,
}

impl TerrainStreamer {
    pub fn new(
        builder: TerrainBuilder,
        material: Handle<render::TerrainMaterial>,
        catalog: render::WorldCatalog,
        load_radius: f64,
    ) -> Self {
        Self {
            options: *builder.options(),
            builder: Arc::new(builder),
            material,
            catalog,
            loaded: HashMap::new(),
            pending: HashMap::new(),
            empty: HashSet::new(),
            load_radius,
            // Hysteresis; kept in step by `set_load_radius`.
            unload_radius: load_radius * 1.25,
            center_tiles: Vec::new(),
            wanted: Vec::new(),
            missing: 0,
            tile_loads: 0,
        }
    }

    /// Moves the load radius while the run is going, keeping the hysteresis with it —
    /// the view distance is a setting, and a setting that needs a restart is an excuse.
    /// Raising it streams the new ring in; lowering it lets `stream_terrain` discard
    /// whatever now falls outside the unload radius.
    pub fn set_load_radius(&mut self, radius: f64) {
        self.load_radius = radius;
        self.unload_radius = radius * 1.25;
        self.center_tiles.clear();
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

    /// Lays the ring of wanted tiles out anew if a centre has moved to
    /// another tile since the last time.
    fn refresh_wanted(&mut self, centers: &[DVec2]) {
        let tiles: Vec<TileKey> = centers
            .iter()
            .map(|c| terrain::tile_at(*c, &self.options))
            .collect();
        if tiles == self.center_tiles {
            return;
        }
        self.center_tiles = tiles;
        let options = self.options;
        let mut wanted: HashSet<TileKey> = HashSet::new();
        for c in centers {
            wanted.extend(terrain::keys_near(*c, self.load_radius, &options));
        }
        let nearest = |k: TileKey| {
            centers
                .iter()
                .map(|c| terrain::tile_distance(k, *c, &options))
                .fold(f64::INFINITY, f64::min)
        };
        let mut ranked: Vec<(f64, TileKey)> = wanted.into_iter().map(|k| (nearest(k), k)).collect();
        ranked.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        self.wanted = ranked.into_iter().map(|(_, k)| k).collect();
    }
}

/// Loads and discards terrain tiles around camera and trains.
#[allow(clippy::too_many_arguments)]
pub fn stream_terrain(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut streamer: ResMut<TerrainStreamer>,
    mut info: ResMut<TerrainInfo>,
    sim: Res<SimResource>,
    origin: Res<Origin>,
    camera: Query<&GlobalTransform, With<ui::CabCamera>>,
    // The farmland of a tile and the day it is drawn on — one material per
    // crop, shared by every tile (see `world_render::farmland`).
    (mut field_materials, mut field_assets, sky): (
        ResMut<world_render::FieldMaterials>,
        ResMut<Assets<world_render::FieldMaterial>>,
        Res<world_render::sky::Sky>,
    ),
    // The water of a tile — one material shared by every surface of the line
    // (see `world_render::water`).
    (mut water_materials, mut water_assets): (
        ResMut<world_render::WaterMaterials>,
        ResMut<Assets<world_render::WaterMaterial>>,
    ),
    // The roads of a tile — one material per surface kind (see
    // `world_render::roads`).
    (mut road_materials, mut road_assets, server): (
        ResMut<world_render::RoadMaterials>,
        ResMut<Assets<world_render::RoadMaterial>>,
        Res<AssetServer>,
    ),
    // The overhead line conductors of a tile — one material for every wire on
    // the line (see `world_render::conductors`).
    (mut conductor_materials, mut conductor_assets): (
        ResMut<world_render::ConductorMaterials>,
        ResMut<Assets<world_render::ConductorMaterial>>,
    ),
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
            commands.entity(tile.entity).try_despawn();
        }
    }

    // Request what is missing — nearest first, so the tile under the train wins.
    streamer.refresh_wanted(&centers);
    let free = MAX_PENDING.saturating_sub(streamer.pending.len());
    if free > 0 {
        let candidates: Vec<TileKey> = streamer
            .wanted
            .iter()
            .copied()
            .filter(|k| {
                !streamer.loaded.contains_key(k)
                    && !streamer.pending.contains_key(k)
                    && !streamer.empty.contains(k)
            })
            .take(free)
            .collect();
        let pool = AsyncComputeTaskPool::get();
        for k in candidates {
            let builder = streamer.builder.clone();
            let task = pool.spawn(async move {
                let mut stats = TerrainStats::default();
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
            &streamer.material,
            &streamer.catalog,
            &tile,
            &origin.0,
        );
        // The view distance is the streamer's business, not the tile's.
        commands.entity(entity).insert(render::TerrainChunk {
            radius: tile.radius,
            lod: tile.lod,
        });
        // The fields hang under the tile, so they stream out with it.
        world_render::spawn_fields(
            &mut commands,
            &mut meshes,
            &mut world_render::FieldDraw {
                materials: &mut field_materials,
                assets: &mut field_assets,
                month: sky.month,
                day: sky.day,
            },
            entity,
            &tile.fields,
        );
        // The water hangs under the tile with it.
        world_render::spawn_waters(
            &mut commands,
            &mut meshes,
            &mut water_materials,
            &mut water_assets,
            entity,
            &tile.waters,
        );
        // The carriageways hang under the tile with it.
        world_render::spawn_roads(
            &mut commands,
            &mut meshes,
            &mut world_render::RoadDraw {
                materials: &mut road_materials,
                assets: &mut road_assets,
                server: &server,
            },
            entity,
            &tile.roads,
        );
        // The conductors hang under the tile like everything else on it.
        world_render::spawn_conductors(
            &mut commands,
            &mut meshes,
            &mut conductor_assets,
            &mut conductor_materials,
            entity,
            &tile.conductors,
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
