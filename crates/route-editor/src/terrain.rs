//! The module's terrain in the editor (plan ch. 14, 15).
//!
//! The DGM import puts height tiles inside the module; this reads them back and
//! draws the ground with the **same** builder, mesh and splat material the
//! simulator uses (`world-render`) — what is shaped here is what the run
//! shows, including every brush stroke, the cutting/embankment at the track and
//! the ground textures.
//!
//! Terrain and aerial imagery lie in the same place, so only one of them is
//! drawn: `T` switches. The height readout under the cursor works either way —
//! the builder stays, only the tiles come and go.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use content::import::dgm::TerrainSource;
use content::terrain::{self, TerrainBuilder, TerrainEdits, TerrainStats, TerrainTile, Vegetation};
use content::{TerrainOptions, TileKey};
use glam::DVec2;
use world_render::{TerrainMaterial, TreeCatalog};

use crate::tools::{self, EditorState};
use crate::{Focus, Line, Origin, TrackObjects, overlay::Overlay};

/// Tiles built at the same time — the builder is serialised behind its mutex
/// anyway (the same reason as in the simulator's streamer).
const MAX_PENDING: usize = 4;
/// Upper bound on tiles in the scene. Without it a 20 km view height would ask
/// for the whole corridor at once.
const MAX_TILES: usize = 64;

/// A terrain tile of the editor — despawned by the streaming, not by a document
/// rebuild.
#[derive(Component)]
pub struct TerrainChunk;

#[derive(Resource)]
pub struct TerrainView {
    /// Draw the terrain instead of the aerial imagery.
    pub enabled: bool,
    /// The line changed — the builder takes the new state over.
    pub dirty: bool,
    /// Ground height under the cursor [m], for the status bar.
    pub cursor_height: Option<f64>,
    /// Whether any elevation data was found; without it the ground is flat and
    /// only the brush strokes shape it.
    pub has_heights: bool,
    material: Handle<TerrainMaterial>,
    /// The line's tree objects as render assets — rebuilt with the builder,
    /// because an added tree may name an object no catalog entry has yet.
    trees: TreeCatalog,
    /// Elevation data, line and strokes — it answers the height readout too.
    builder: Option<Arc<Mutex<TerrainBuilder>>>,
    /// Which height sources it was built from (`(path, zone)`) — a changed
    /// reference reloads them, an edit does not.
    sources: Vec<(String, u8)>,
    options: TerrainOptions,
    loaded: HashMap<TileKey, (Entity, u64)>,
    /// Tiles being built, with the generation they were requested for — one
    /// that finishes after the next edit is stale and is asked for again.
    pending: HashMap<TileKey, (u64, Task<Option<TerrainTile>>)>,
    /// Keys outside the line corridor — asked for once, never again.
    empty: HashSet<TileKey>,
    /// Bumped on every edit; a tile built for an older one is built anew.
    generation: u64,
    /// Whether the first look at the line is still to come — a module that
    /// brings height data shows it, one without stays on the imagery.
    first: bool,
}

impl TerrainView {
    pub fn new(material: Handle<TerrainMaterial>, trees: TreeCatalog) -> Self {
        Self {
            enabled: false,
            dirty: true,
            cursor_height: None,
            has_heights: false,
            material,
            trees,
            builder: None,
            sources: Vec::new(),
            options: TerrainOptions::default(),
            loaded: HashMap::new(),
            pending: HashMap::new(),
            empty: HashSet::new(),
            generation: 0,
            first: true,
        }
    }

    pub fn tiles_shown(&self) -> usize {
        self.loaded.len()
    }

    /// The builder, for whoever needs a ground height outside this module —
    /// the scenery objects that snap to the terrain. `None` before the first
    /// frame has built it.
    pub fn builder_lock(&self) -> Option<std::sync::MutexGuard<'_, TerrainBuilder>> {
        self.builder.as_ref()?.lock().ok()
    }

    /// Drops every tile in the scene; the builder stays.
    fn clear(&mut self, commands: &mut Commands) {
        for (entity, _) in self.loaded.drain().map(|(_, v)| v) {
            commands.entity(entity).despawn();
        }
        self.pending.clear();
    }
}

/// Terrain of the edited module: takes over edits, streams tiles around the
/// view point, and answers `T`.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut view: ResMut<TerrainView>,
    mut overlay: ResMut<Overlay>,
    assets: Res<AssetServer>,
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<EditorState>,
    mut line: ResMut<Line>,
    objects: Res<TrackObjects>,
    focus: Res<Focus>,
    origin: Res<Origin>,
) {
    if keys.just_pressed(KeyCode::KeyT) && !state.typing {
        view.enabled = !view.enabled;
        // Track and signals are drawn differently in each view.
        line.needs_rebuild = true;
    }
    if view.dirty {
        view.dirty = false;
        refresh_builder(&mut view, &line, state.terrain_options());
        let names: Vec<String> = view
            .builder_lock()
            .map(|b| b.tree_objects().to_vec())
            .unwrap_or_default();
        view.trees =
            world_render::tree_catalog(&names, &objects.map, &assets, &mut meshes, &mut materials);
        view.generation += 1;
        view.empty.clear();
        // A module that carries its ground shows it; one without would only
        // put a flat plane over the aerial imagery.
        if std::mem::take(&mut view.first) {
            view.enabled = view.has_heights;
            line.needs_rebuild = true;
        }
    }
    if !view.enabled {
        if view.tiles_shown() > 0 || !view.pending.is_empty() {
            view.clear(&mut commands);
        }
        return;
    }
    if !view.has_heights && overlay.status.is_empty() {
        overlay.status = i18n::t!("status-terrain-flat");
    }

    // How far terrain is built around the view point. The corridor is 1.2 km
    // wide anyway, so a low view sees all of it; the cap keeps an overview
    // flight from asking for a whole line.
    let options = view.options;
    let center = terrain::to_utm(focus.position, &options);
    let radius = (focus.height * 0.8).clamp(700.0, 3_000.0);
    let wanted = wanted_keys(center, radius, &options);

    // Discard what has left the view.
    let keep: HashSet<TileKey> = wanted.iter().copied().collect();
    let far: Vec<TileKey> = view
        .loaded
        .keys()
        .copied()
        .filter(|k| !keep.contains(k))
        .collect();
    for k in far {
        if let Some((entity, _)) = view.loaded.remove(&k) {
            commands.entity(entity).despawn();
        }
    }

    // Request what is missing or stale — nearest first (`wanted` is sorted).
    let generation = view.generation;
    let free = MAX_PENDING.saturating_sub(view.pending.len());
    if free > 0
        && let Some(builder) = view.builder.clone()
    {
        let pool = AsyncComputeTaskPool::get();
        let stale: Vec<TileKey> = wanted
            .into_iter()
            .filter(|k| {
                !view.pending.contains_key(k)
                    && !view.empty.contains(k)
                    && view
                        .loaded
                        .get(k)
                        .is_none_or(|(_, built)| *built != generation)
            })
            .take(free)
            .collect();
        for k in stale {
            let builder = builder.clone();
            let task = pool.spawn(async move {
                let mut stats = TerrainStats::default();
                builder.lock().ok()?.build_key(k, &mut stats)
            });
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
        let Some(tile) = result else {
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
            &view.trees,
            &tile,
            &origin.0,
        );
        commands.entity(entity).insert(TerrainChunk);
        if let Some((old, _)) = view.loaded.insert(k, (entity, built)) {
            commands.entity(old).despawn();
        }
    }
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
        .filter(|p| state.viewport.contains(*p));
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
    let Some(builder) = view.builder.clone() else {
        view.cursor_height = None;
        return;
    };
    // A tile build holds the lock for tens of milliseconds — the readout keeps
    // its last value rather than stalling the frame for it.
    if let Ok(mut builder) = builder.try_lock() {
        view.cursor_height = Some(builder.surface_height(p));
    }
}

/// Takes the edited line over: a new builder when the height sources changed,
/// otherwise the standing one — re-indexing the DGM on every stroke would read
/// the delivery off disk again.
fn refresh_builder(view: &mut TerrainView, line: &Line, options: TerrainOptions) {
    let wanted: Vec<(String, u8)> = line
        .source
        .heights
        .iter()
        .map(|h| (h.path.clone(), h.zone))
        .collect();
    let vegetation = Vegetation::from_line(&line.source, options.zone);
    let edits = TerrainEdits::from_line(&line.source, options.zone);

    if view.builder.is_some() && view.sources == wanted && view.options == options {
        if let Some(builder) = &view.builder {
            builder
                .lock()
                .expect("terrain builder")
                .set_line(&line.net, vegetation, edits);
        }
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
    view.builder = Some(Arc::new(Mutex::new(
        TerrainBuilder::new(&line.net, sources, options)
            .with_vegetation(vegetation)
            .with_edits(edits),
    )));
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
}
