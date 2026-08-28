//! The module's terrain in the editor (plan ch. 14, 15).
//!
//! The DGM import puts height tiles inside the module; this reads them back and
//! draws the ground with the **same** builder, mesh and splat material the
//! simulator uses (`world-render`) — what is shaped here is what the run
//! shows, including every brush stroke, the cutting/embankment at the track,
//! the ground textures, and the trees and objects standing on it.
//!
//! The ground is always drawn. The aerial imagery is not an alternative to it
//! but a layer on top: switched on, it is draped over the terrain's shape (see
//! `overlay`), so a builder sees the photo and the relief at once.
//!
//! An edit does not rebuild the world. `track_changes` works out what it
//! reached ([`TerrainChange`]): a brush stroke asks for the ground of the tiles
//! under its radius, a moved tree or object only for the trees and objects of
//! its tile — those are placed onto the standing ground again, a microsecond's
//! work, while the tile's mesh stays where it is. Only a change to the track
//! itself, which shapes the formation everywhere it runs, asks for everything.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::diagnostic::{
    DiagnosticPath, DiagnosticsStore, EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin,
};
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use content::import::dgm::TerrainSource;
use content::route::{MarkerSource, TerrainEditSource, TreeSource};
use content::terrain::{
    self, Scenery, TerrainBuilder, TerrainEdits, TerrainStats, TerrainTile, Vegetation,
};
use content::{TerrainOptions, TileKey};
use glam::DVec2;
use world_coords::{EcefPos, geo};
use world_render::{Scattered, TerrainMaterial, WorldCatalog};

use crate::tools::{self, EditorState};
use crate::{Focus, Line, Origin, TrackObjects, overlay::Overlay};

/// Tiles built at the same time — one per worker, the builder is shared
/// read-only and the elevation data keeps its own short lock.
const MAX_PENDING: usize = 6;
/// Upper bound on tiles in the scene. Without it a 20 km view height would ask
/// for the whole corridor at once.
const MAX_TILES: usize = 64;
/// How far terrain is built around the view point [m]. A shallow view looks
/// to the horizon however near its pivot is, so the radius is fixed rather
/// than tied to the camera distance — that would end the ground at the
/// camera's feet. The corridor is 1.2 km wide anyway.
const LOAD_RADIUS: f64 = 3_000.0;
/// Beyond this a tile is let go of again [m]. The gap to [`LOAD_RADIUS`] is
/// the hysteresis: a tile at the boundary would otherwise be built and
/// dropped with every nudge of the camera.
const UNLOAD_RADIUS: f64 = LOAD_RADIUS * 1.25;

/// A terrain tile of the editor — despawned by the streaming, not by a document
/// rebuild.
#[derive(Component)]
pub struct TerrainChunk;

/// An area of the module in UTM, as an edit reaches it.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Region {
    #[default]
    None,
    All,
    /// Axis-aligned rectangles `(min, max)`.
    Rects(Vec<(DVec2, DVec2)>),
}

impl Region {
    pub fn is_none(&self) -> bool {
        matches!(self, Region::None)
    }

    /// Adds a disc around `p`.
    pub fn add_disc(&mut self, p: DVec2, radius: f64) {
        let r = DVec2::splat(radius);
        match self {
            Region::All => {}
            Region::None => *self = Region::Rects(vec![(p - r, p + r)]),
            Region::Rects(rects) => rects.push((p - r, p + r)),
        }
    }

    /// Whether the region reaches into the tile at `min`.
    pub fn touches(&self, min: DVec2, size: f64) -> bool {
        match self {
            Region::None => false,
            Region::All => true,
            Region::Rects(rects) => {
                let max = min + DVec2::splat(size);
                rects.iter().any(|(lo, hi)| {
                    lo.x <= max.x && hi.x >= min.x && lo.y <= max.y && hi.y >= min.y
                })
            }
        }
    }

    fn merge(&mut self, other: Region) {
        match (&mut *self, other) {
            (_, Region::None) => {}
            (_, Region::All) => *self = Region::All,
            (Region::All, _) => {}
            (Region::None, rects) => *self = rects,
            (Region::Rects(mine), Region::Rects(theirs)) => mine.extend(theirs),
        }
    }
}

/// What an edit did to the terrain: where the ground itself has to be built
/// anew, and where only the trees and objects have to be placed again.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TerrainChange {
    pub ground: Region,
    pub scatter: Region,
}

impl TerrainChange {
    /// Everything — a new document, an undo, a moved track.
    pub fn all() -> Self {
        Self {
            ground: Region::All,
            scatter: Region::All,
        }
    }

    pub fn is_none(&self) -> bool {
        self.ground.is_none() && self.scatter.is_none()
    }

    pub fn merge(&mut self, other: TerrainChange) {
        self.ground.merge(other.ground);
        self.scatter.merge(other.scatter);
    }
}

/// Per tile, the builder generation its ground and its scatter have to come
/// from at least. A floor stands for every tile at once — an edit that
/// reached everything — so it costs nothing per tile.
#[derive(Default)]
struct Staleness {
    ground_wanted: HashMap<TileKey, u64>,
    ground_floor: u64,
    scatter_wanted: HashMap<TileKey, u64>,
    scatter_floor: u64,
}

impl Staleness {
    fn mark(&mut self, change: &TerrainChange, generation: u64, options: &TerrainOptions) {
        let size = options.tile_size;
        let mark =
            |region: &Region, wanted: &mut HashMap<TileKey, u64>, floor: &mut u64| match region {
                Region::None => {}
                Region::All => {
                    *floor = generation;
                    wanted.clear();
                }
                Region::Rects(rects) => {
                    for (lo, hi) in rects {
                        let (ax, ay) = terrain::tile_at(*lo, options);
                        let (bx, by) = terrain::tile_at(*hi, options);
                        for y in ay..=by {
                            for x in ax..=bx {
                                if region.touches(terrain::tile_min((x, y), size), size) {
                                    wanted.insert((x, y), generation);
                                }
                            }
                        }
                    }
                }
            };
        mark(
            &change.ground,
            &mut self.ground_wanted,
            &mut self.ground_floor,
        );
        mark(
            &change.scatter,
            &mut self.scatter_wanted,
            &mut self.scatter_floor,
        );
    }

    fn ground(&self, k: TileKey) -> u64 {
        self.ground_floor
            .max(self.ground_wanted.get(&k).copied().unwrap_or(0))
    }

    fn scatter(&self, k: TileKey) -> u64 {
        self.scatter_floor
            .max(self.scatter_wanted.get(&k).copied().unwrap_or(0))
    }

    /// New sources or a new grid: nothing in the scene is built from them.
    fn all(&mut self, generation: u64) {
        self.ground_floor = generation;
        self.scatter_floor = generation;
        self.ground_wanted.clear();
        self.scatter_wanted.clear();
    }
}

/// A tile in the scene.
struct LoadedTile {
    entity: Entity,
    /// The builder generation its ground was built from.
    ground: u64,
    /// The builder generation its trees and objects were placed from.
    scatter: u64,
    /// The tile without its mesh data — its height grid, for the rescatter.
    tile: TerrainTile,
}

#[derive(Resource)]
pub struct TerrainView {
    /// Ground height under the cursor [m], for the status bar.
    pub cursor_height: Option<f64>,
    /// Whether any elevation data was found; without it the ground is flat and
    /// only the brush strokes shape it.
    pub has_heights: bool,
    material: Handle<TerrainMaterial>,
    /// The line's tree and object models as render assets — rebuilt when a
    /// name appears that no entry has yet.
    catalog: WorldCatalog,
    catalog_names: (Vec<String>, Vec<String>),
    /// Elevation data, line and strokes — it answers the height readout too.
    /// A new one for every edit, sharing the sources (`with_line`).
    builder: Option<Arc<TerrainBuilder>>,
    /// Which height sources it was built from (`(path, zone)`) — a changed
    /// reference reloads them, an edit does not.
    sources: Vec<(String, u8)>,
    options: TerrainOptions,
    loaded: HashMap<TileKey, LoadedTile>,
    /// Tiles being built, with the generation they were requested for.
    pending: HashMap<TileKey, (u64, Task<Option<TerrainTile>>)>,
    /// Keys outside the line corridor — asked for once, never again (until
    /// the track moves).
    empty: HashSet<TileKey>,
    /// Bumped with every new builder.
    generation: u64,
    /// Which tiles an edit has reached since they were built.
    stale: Staleness,
    /// The tile the view point stood on when `wanted` was laid out.
    center_tile: Option<TileKey>,
    /// Tiles in the load radius, nearest first, capped.
    wanted: Vec<TileKey>,
    /// Bumped whenever a tile enters or leaves the scene — what the marks
    /// watch to put themselves back on the ground.
    pub version: u64,
}

impl TerrainView {
    pub fn new(material: Handle<TerrainMaterial>, catalog: WorldCatalog) -> Self {
        Self {
            cursor_height: None,
            has_heights: false,
            material,
            catalog,
            catalog_names: (Vec::new(), Vec::new()),
            builder: None,
            sources: Vec::new(),
            options: TerrainOptions::default(),
            loaded: HashMap::new(),
            pending: HashMap::new(),
            empty: HashSet::new(),
            generation: 0,
            stale: Staleness::default(),
            center_tile: None,
            wanted: Vec::new(),
            version: 0,
        }
    }

    /// The builder, for whoever needs a ground height — the aerial imagery
    /// samples a height grid per tile through it, on a worker thread. `None`
    /// before the first frame has built it.
    pub fn builder(&self) -> Option<Arc<TerrainBuilder>> {
        self.builder.clone()
    }

    /// Tiles in the scene, and tiles being built.
    pub fn tiles(&self) -> (usize, usize) {
        (self.loaded.len(), self.pending.len())
    }

    /// Ground height at a UTM point from the tiles in the scene — the height
    /// the map shows there. `None` off the loaded tiles.
    pub fn height_at(&self, p: DVec2) -> Option<f64> {
        let tile = self.loaded.get(&terrain::tile_at(p, &self.options))?;
        Some(grid_height(&tile.tile, p, self.options.tile_size))
    }

    /// Ground height of a world point from the tiles in the scene.
    pub fn height_at_pos(&self, pos: EcefPos) -> Option<f64> {
        self.height_at(terrain::to_utm(pos, &self.options))
    }

    /// Ground height a tool reads: the tiles in the scene first, the
    /// builder's blended surface where no tile is loaded yet.
    pub fn ground_height(&self, pos: EcefPos) -> Option<f64> {
        self.height_at_pos(pos)
            .or_else(|| self.builder().map(|b| b.surface_height(pos)))
    }

    /// Takes an edit over: the tiles it reached are marked for a new ground
    /// or a new scatter, and built from the next builder generation.
    fn invalidate(&mut self, change: &TerrainChange) {
        self.stale.mark(change, self.generation, &self.options);
        if matches!(change.ground, Region::All) {
            // The track may have moved into a tile that was empty before.
            self.empty.clear();
        }
    }

    /// Lays the ring of wanted tiles out anew if the view point has moved to
    /// another tile.
    fn refresh_wanted(&mut self, center: DVec2) {
        let tile = terrain::tile_at(center, &self.options);
        if self.center_tile == Some(tile) {
            return;
        }
        self.center_tile = Some(tile);
        self.wanted = wanted_keys(center, LOAD_RADIUS, &self.options);
    }
}

/// Terrain of the edited module: takes edits over, streams tiles around the
/// view point, and answers `T`.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut view: ResMut<TerrainView>,
    mut overlay: ResMut<Overlay>,
    assets: Res<AssetServer>,
    state: Res<EditorState>,
    mut line: ResMut<Line>,
    objects: Res<TrackObjects>,
    focus: Res<Focus>,
    origin: Res<Origin>,
    scattered: Query<(), With<Scattered>>,
    children: Query<&Children>,
) {
    // Taken only when there is something to take: a write through `ResMut`
    // marks the line changed, and the marks and the title watch for that.
    let change = if line.terrain_change.is_none() {
        TerrainChange::default()
    } else {
        std::mem::take(&mut line.terrain_change)
    };
    if !change.is_none() || view.builder.is_none() {
        view.generation += 1;
        refresh_builder(&mut view, &line, state.terrain_options());
        if view.builder.is_none() {
            return;
        }
        view.invalidate(&change);
        // The object names the line uses — a new one needs its model loaded.
        let builder = view.builder.clone().expect("just refreshed");
        let names = (
            builder.tree_objects().to_vec(),
            builder.scenery_objects().to_vec(),
        );
        if names != view.catalog_names {
            view.catalog = WorldCatalog::new(
                &names.0,
                &names.1,
                &objects.map,
                // The editor shows no crowd (`TerrainBuilder::with_line`).
                default(),
                &assets,
                &mut meshes,
                &mut materials,
                default(),
            );
            view.catalog_names = names;
        }
    }
    if !view.has_heights && overlay.status.is_empty() {
        overlay.status = i18n::t!("status-terrain-flat");
    }

    let options = view.options;
    let center = terrain::to_utm(focus.position, &options);
    view.refresh_wanted(center);

    // Discard what has left the view: past the unload radius, or, with the
    // scene full beyond its cap, the farthest.
    let mut far: Vec<(f64, TileKey)> = view
        .loaded
        .keys()
        .map(|k| (terrain::tile_distance(*k, center, &options), *k))
        .collect();
    far.sort_by(|a, b| b.0.total_cmp(&a.0));
    let over = view.loaded.len().saturating_sub(MAX_TILES + MAX_TILES / 4);
    let drop: Vec<TileKey> = far
        .iter()
        .enumerate()
        .filter(|(i, (d, _))| *d > UNLOAD_RADIUS || *i < over)
        .map(|(_, (_, k))| *k)
        .collect();
    for k in drop {
        if let Some(tile) = view.loaded.remove(&k) {
            commands.entity(tile.entity).try_despawn();
            view.version += 1;
        }
    }

    // Trees and objects placed anew on tiles whose ground stands.
    let builder = view.builder.clone().expect("refreshed above");
    let rescatter: Vec<TileKey> = view
        .loaded
        .iter()
        .filter(|(k, t)| t.scatter < view.stale.scatter(**k) && t.ground >= view.stale.ground(**k))
        .map(|(k, _)| *k)
        .collect();
    let generation = view.generation;
    let view = &mut *view;
    for k in rescatter {
        let Some(loaded) = view.loaded.get_mut(&k) else {
            continue;
        };
        let (trees, objects, people) = builder.rescatter(&loaded.tile);
        let old: Vec<Entity> = children
            .get(loaded.entity)
            .map(|c| c.iter().filter(|e| scattered.contains(*e)).collect())
            .unwrap_or_default();
        world_render::respawn_scatter(
            &mut commands,
            loaded.entity,
            old,
            trees,
            &objects,
            &people,
            &view.catalog,
        );
        loaded.scatter = generation;
    }

    // Request what is missing or stale — nearest first (`wanted` is sorted).
    let free = MAX_PENDING.saturating_sub(view.pending.len());
    if free > 0 {
        let pool = AsyncComputeTaskPool::get();
        let stale: Vec<TileKey> = view
            .wanted
            .iter()
            .copied()
            .filter(|k| {
                !view.empty.contains(k)
                    && view
                        .pending
                        .get(k)
                        .is_none_or(|(requested, _)| *requested < view.stale.ground(*k))
                    && view
                        .loaded
                        .get(k)
                        .is_none_or(|t| t.ground < view.stale.ground(*k))
            })
            .take(free)
            .collect();
        for k in stale {
            let builder = builder.clone();
            let task = pool.spawn(async move {
                let mut stats = TerrainStats::default();
                builder.build_key(k, &mut stats)
            });
            // A build already running for an older generation is superseded:
            // its task is dropped with the entry, which cancels it.
            view.pending.insert(k, (generation, task));
        }
    }

    // Take over what is finished. The old tile stays until its replacement is
    // there, so an edit does not tear a hole into the map.
    let mut finished: Vec<(TileKey, u64, Option<TerrainTile>)> = Vec::new();
    for (k, (built, task)) in view.pending.iter_mut() {
        if let Some(result) = block_on(poll_once(&mut *task)) {
            finished.push((*k, *built, result));
        }
    }
    for (k, built, result) in finished {
        view.pending.remove(&k);
        let Some(mut tile) = result else {
            // Outside the corridor — but only for the line it was built from:
            // an edit may have moved the track into this tile.
            if built == generation {
                view.empty.insert(k);
            }
            continue;
        };
        let entity = world_render::spawn_terrain_tile(
            &mut commands,
            &mut meshes,
            &view.material,
            &view.catalog,
            &tile,
            &origin.0,
        );
        commands.entity(entity).insert(TerrainChunk);
        // The mesh is on its way to the GPU; what stays is the height grid.
        tile.positions = Vec::new();
        tile.indices = Vec::new();
        tile.splat = Vec::new();
        tile.trees = Vec::new();
        tile.objects = Vec::new();
        let replaced = view.loaded.insert(
            k,
            LoadedTile {
                entity,
                ground: built,
                scatter: built,
                tile,
            },
        );
        if let Some(old) = replaced {
            commands.entity(old.entity).try_despawn();
        }
        view.version += 1;
    }
}

/// Height of a tile's grid at the UTM point `p` (bilinear) — the same reading
/// the builder took when it placed the tile's trees.
fn grid_height(tile: &TerrainTile, p: DVec2, tile_size: f64) -> f64 {
    let n = (tile_size / tile.step).round().max(1.0) as usize;
    let row = n + 1;
    let gx = ((p.x - tile.min.x) / tile.step).clamp(0.0, n as f64 - 1e-9);
    let gy = ((p.y - tile.min.y) / tile.step).clamp(0.0, n as f64 - 1e-9);
    let (ix, iy) = (gx as usize, gy as usize);
    let (fx, fy) = (gx - ix as f64, gy - iy as f64);
    let h = |ix: usize, iy: usize| tile.heights[iy * row + ix] as f64;
    h(ix, iy) * (1.0 - fx) * (1.0 - fy)
        + h(ix + 1, iy) * fx * (1.0 - fy)
        + h(ix, iy + 1) * (1.0 - fx) * fy
        + h(ix + 1, iy + 1) * fx * fy
}

/// Ground height under the cursor — the number a builder shaping terrain over
/// aerial imagery has no other way of seeing.
pub fn probe_cursor(
    mut view: ResMut<TerrainView>,
    state: Res<EditorState>,
    focus: Res<Focus>,
    origin: Res<Origin>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
) {
    let cursor = windows
        .single()
        .ok()
        .and_then(|w| w.cursor_position())
        .filter(|p| state.over_viewport(*p));
    let Some((cursor, (camera, transform))) = cursor.zip(camera.single().ok()) else {
        view.cursor_height = None;
        return;
    };
    // ponytail: the cursor is projected onto the map plane, not onto the
    // terrain surface — at editor view heights the parallax of the near-vertical
    // ray is centimetres. A ray march against the tiles steps in if someone
    // works on a cliff face.
    let Some(p) = tools::pick_ground(camera, transform, cursor, &origin.0, &focus) else {
        return;
    };
    // The tile under the cursor says what the map shows; off the tiles the
    // builder says what it would show.
    view.cursor_height = view
        .height_at_pos(p)
        .or_else(|| view.builder().map(|b| b.surface_height(p)));
}

/// Where the map marks stand: trees, reference markers and terrain strokes on
/// the terrain surface.
///
/// A source file gives a mark its latitude and longitude only — the ground
/// decides its height, exactly as it decides the height of the tree a tile
/// bakes in. The heights come out of the tiles in the scene, the same grid
/// the trees were placed on, so a cross sits on the ground under the tree it
/// marks; a mark off the loaded tiles keeps the module's fallback height
/// until its tile arrives.
#[derive(Resource, Default)]
pub struct Marks {
    trees: Vec<f64>,
    markers: Vec<f64>,
    strokes: Vec<f64>,
    /// What a mark stands at until the terrain has answered for it — the
    /// height the builder itself falls back to where no DGM covers the module.
    fallback: f64,
    /// The terrain version the marks were last sampled against.
    version: Option<u64>,
}

impl Marks {
    /// Where tree `i` stands.
    pub fn tree(&self, i: usize, tree: &TreeSource) -> EcefPos {
        geo::to_ecef_deg(tree.lat, tree.lon, self.height(&self.trees, i))
    }

    /// Where reference marker `i` stands.
    pub fn marker(&self, i: usize, marker: &MarkerSource) -> EcefPos {
        geo::to_ecef_deg(marker.lat, marker.lon, self.height(&self.markers, i))
    }

    /// Where terrain stroke `i` is centred.
    pub fn stroke(&self, i: usize, edit: &TerrainEditSource) -> EcefPos {
        geo::to_ecef_deg(edit.lat, edit.lon, self.height(&self.strokes, i))
    }

    /// A mark placed since the last sample has no height yet; the fallback
    /// keeps it in the module's ground plane for the one frame it takes.
    fn height(&self, sampled: &[f64], i: usize) -> f64 {
        sampled.get(i).copied().unwrap_or(self.fallback)
    }
}

/// The ground as the UI reads it: the tiles' own view and where the marks
/// stand on them. One parameter for the same reason as [`crate::Catalogs`] —
/// two more resources would put `ui::draw` over Bevy's limit, and they are
/// read together anyway.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Ground<'w> {
    pub view: Res<'w, TerrainView>,
    pub marks: Res<'w, Marks>,
    /// Bevy's frame time and entity count, for the status bar.
    diagnostics: Res<'w, DiagnosticsStore>,
}

impl Ground<'_> {
    /// Frames per second and entities in the world, smoothed.
    pub fn perf(&self) -> (f64, usize) {
        let smoothed = |path: &DiagnosticPath| {
            self.diagnostics
                .get(path)
                .and_then(|d| d.smoothed())
                .unwrap_or_default()
        };
        (
            smoothed(&FrameTimeDiagnosticsPlugin::FPS),
            smoothed(&EntityCountDiagnosticsPlugin::ENTITY_COUNT) as usize,
        )
    }
}

/// Puts the marks back on the ground after an edit, and after a tile has
/// come or gone.
pub fn probe_marks(
    mut marks: ResMut<Marks>,
    view: Res<TerrainView>,
    line: Res<Line>,
    state: Res<EditorState>,
) {
    if !line.is_changed() && marks.version == Some(view.version) {
        return;
    }
    let options = state.terrain_options();
    marks.fallback = options.fallback_height + options.geoid_offset;
    let fallback = marks.fallback;
    let ground = |lat: f64, lon: f64| {
        let (e, n) = geo::to_utm(lat.to_radians(), lon.to_radians(), options.zone);
        view.height_at(DVec2::new(e, n)).unwrap_or(fallback)
    };
    marks.trees = line
        .source
        .trees
        .iter()
        .map(|t| ground(t.lat, t.lon))
        .collect();
    marks.markers = line
        .source
        .markers
        .iter()
        .map(|m| ground(m.lat, m.lon))
        .collect();
    marks.strokes = line
        .source
        .terrain
        .iter()
        .map(|e| ground(e.lat, e.lon))
        .collect();
    marks.version = Some(view.version);
}

/// Takes the edited line over: a new builder sharing the height sources,
/// or one with fresh sources when the references changed — re-indexing the
/// DGM on every stroke would read the delivery off disk again.
fn refresh_builder(view: &mut TerrainView, line: &Line, options: TerrainOptions) {
    let wanted: Vec<(String, u8)> = line
        .source
        .heights
        .iter()
        .map(|h| (h.path.clone(), h.zone))
        .collect();
    let vegetation = Vegetation::from_line(&line.source, options.zone);
    let scenery = Scenery::from_line(&line.source, &line.net, options.zone);
    let edits = TerrainEdits::from_line(&line.source, options.zone);

    if let Some(builder) = &view.builder
        && view.sources == wanted
        && view.options == options
    {
        view.builder = Some(Arc::new(
            builder.with_line(&line.net, vegetation, scenery, edits),
        ));
        return;
    }

    let mut sources = Vec::new();
    for (path, zone) in &wanted {
        let Some(dir) = mod_path(path) else {
            warn!("height data {path}: mod not installed");
            continue;
        };
        match TerrainSource::from_dir(&dir, *zone) {
            Ok(source) => {
                info!(
                    "module heights: {} tiles from {} (zone {zone})",
                    source.tile_count(),
                    dir.display()
                );
                sources.push(source);
            }
            Err(e) => warn!("height data {} not readable: {e}", dir.display()),
        }
    }
    view.has_heights = sources.iter().any(|s| s.tile_count() > 0);
    view.sources = wanted;
    view.options = options;
    view.builder = Some(Arc::new(
        TerrainBuilder::new(&line.net, sources, options)
            .with_vegetation(vegetation)
            .with_scenery(scenery)
            .with_edits(edits),
    ));
    view.stale.all(view.generation);
    view.empty.clear();
    view.center_tile = None;
}

/// `"<mod id>:<relative path>"` → the directory under `mods/`.
fn mod_path(spec: &str) -> Option<std::path::PathBuf> {
    #[derive(serde::Deserialize)]
    struct ManifestId {
        id: String,
    }
    let (id, rest) = spec.split_once(':')?;
    for dir in std::fs::read_dir("mods")
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
    {
        let found = std::fs::read_to_string(dir.join("mod.ron"))
            .ok()
            .and_then(|text| ron::from_str::<ManifestId>(&text).ok())
            .is_some_and(|m| m.id == id);
        if found {
            return Some(dir.join(rest));
        }
    }
    None
}

/// Tile keys around the view point, nearest first and capped.
fn wanted_keys(center: DVec2, radius: f64, options: &TerrainOptions) -> Vec<TileKey> {
    let mut keys = terrain::keys_near(center, radius, options);
    keys.sort_by(|a, b| {
        terrain::tile_distance(*a, center, options)
            .total_cmp(&terrain::tile_distance(*b, center, options))
            .then(a.cmp(b))
    });
    keys.truncate(MAX_TILES);
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The view asks for the tiles under the cursor first and never for more
    /// than it can hold — an overview flight would otherwise request a whole
    /// line's corridor at once.
    #[test]
    fn wanted_tiles_are_nearest_first_and_capped() {
        let options = TerrainOptions::default();
        // Well inside a tile — a point on the grid line lies in two of them.
        let center = DVec2::new(600_300.0, 5_760_100.0);

        let near = wanted_keys(center, 700.0, &options);
        assert_eq!(near[0], terrain::tile_at(center, &options));
        let mut distance = 0.0;
        for k in &near {
            let d = terrain::tile_distance(*k, center, &options);
            assert!(d >= distance - 1e-9, "{d} after {distance}");
            assert!(d <= 700.0);
            distance = d;
        }

        // 20 km view height: the cap holds, and the kept tiles are the nearest.
        let wide = wanted_keys(center, 3_000.0, &options);
        assert_eq!(wide.len(), MAX_TILES);
        assert_eq!(wide[0], near[0]);
    }

    /// A stroke reaches the tiles under its disc and no others; a change to
    /// everything reaches every tile.
    #[test]
    fn a_region_touches_the_tiles_it_covers() {
        let size = 512.0;
        let mut region = Region::default();
        assert!(!region.touches(DVec2::ZERO, size));
        region.add_disc(DVec2::new(600.0, 600.0), 150.0);
        assert!(region.touches(DVec2::new(512.0, 512.0), size));
        assert!(
            region.touches(DVec2::new(0.0, 512.0), size),
            "disc reaches west"
        );
        assert!(!region.touches(DVec2::new(1024.0, 1024.0), size));
        region.merge(Region::All);
        assert!(region.touches(DVec2::new(1024.0, 1024.0), size));
    }

    /// A ground change marks the tiles it reaches, and only those, for the
    /// current generation; an all-change raises the floor for every tile.
    #[test]
    fn invalidation_marks_the_reached_tiles() {
        let options = TerrainOptions::default();
        let mut stale = Staleness::default();
        let mut change = TerrainChange::default();
        change.ground.add_disc(DVec2::new(600.0, 600.0), 10.0);
        stale.mark(&change, 3, &options);
        assert_eq!(stale.ground((1, 1)), 3);
        assert_eq!(stale.ground((0, 0)), 0);
        assert_eq!(stale.scatter((1, 1)), 0, "only the ground was asked for");

        stale.mark(&TerrainChange::all(), 4, &options);
        assert_eq!(stale.ground((7, 7)), 4);
        assert_eq!(stale.scatter((7, 7)), 4);
    }
}
