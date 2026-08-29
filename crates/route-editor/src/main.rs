//! Connected Rails route editor — free 3D view of a line with aerial imagery
//! draped over the ground, track drawing and device placement (plan ch. 15,
//! editor v1).
//!
//! ```text
//! trainsim-route-editor [line.ron] [--imagery <config.ron>] [--frames N] [--height M]
//!                        [--window WxH] [--drawer [objects|signal-types|signal-models|track-types]]
//! ```
//!
//! Without a line file the example line is loaded. The overlay configuration is created
//! on first start and can be reloaded at runtime (F5).

mod areas;
mod content_drawer;
mod envelope;
mod fields;
mod gizmo;
mod new_module;
mod overlay;
mod settings;
mod signals;
mod stake;
mod terrain;
mod thumbnails;
mod tools;
mod ui;
mod view;
mod walkways;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy_egui::{EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext};
use content::LineSource;
use content::route::RuleIssue;
use glam::DVec3;
use i18n::t;
use imagery::{ImageryConfig, ZoomMode};
use overlay::{Overlay, OverlayTile};
use tools::EditorState;
use track_model::{DeviceKind, TrackEdge, TrackNetwork};
use world_coords::{EcefPos, EnuFrame, RenderOrigin, geo};
use world_render::{WorldAnchored, sky};

/// Geographic position of a world point in **degrees** — `geo::from_ecef` returns radians,
/// while both the tile grid and the display work in degrees.
pub fn focus_degrees(position: EcefPos) -> (f64, f64) {
    let (lat, lon, _) = geo::from_ecef(position);
    (lat.to_degrees(), lon.to_degrees())
}

/// Render origin (floating origin).
#[derive(Resource)]
pub struct Origin(pub RenderOrigin);

/// What the viewport looks at: a pivot, a distance and a direction — see
/// [`view`].
#[derive(Resource)]
pub struct Focus {
    pub position: EcefPos,
    /// Distance from the camera to the view point [m].
    pub height: f64,
    /// Compass heading of the camera [rad], 0 = north, clockwise.
    pub yaw: f64,
    /// How far the camera looks down [rad].
    pub pitch: f64,
    /// Unreal's camera speed dial: a step from 1 to 8, each one doubling the
    /// flight speed (see [`Focus::fly_speed`]).
    pub speed_step: i32,
    /// Unreal's camera speed scalar — the fine multiplier on top of the dial,
    /// for when eight steps do not reach.
    pub speed_scalar: f64,
}

/// The document: the line in source form, its compilation, and the save state.
#[derive(Resource)]
pub struct Line {
    pub source: LineSource,
    pub net: TrackNetwork,
    pub path: Option<String>,
    pub dirty: bool,
    /// Recompile the source and respawn track and markers next frame.
    pub needs_rebuild: bool,
    /// What the last edit did to the ground and to what stands on it — the
    /// terrain streaming takes it and rebuilds only the tiles it reached.
    pub terrain_change: terrain::TerrainChange,
    /// Move the view to the line's middle on the next rebuild (after Open).
    pub recenter: bool,
    /// Findings of the rule check, refreshed with every rebuild.
    pub issues: Vec<RuleIssue>,
}

/// A neighbour module drawn as a non-editable ghost, so the builder hits its
/// agreed boundary coordinates (plan ch. 15, module tooling).
#[derive(Resource, Default)]
pub struct Ghost {
    pub path: Option<String>,
    pub net: Option<TrackNetwork>,
    /// Boundary name → world position; drawing clicks snap onto these.
    pub boundaries: Vec<(String, EcefPos)>,
    /// Respawn the ghost track next frame (after load or clear).
    pub respawn: bool,
}

/// Track ribbon of the ghost module — survives document rebuilds.
#[derive(Component)]
struct GhostTrack;

/// Track types of every installed mod (`mods/*/track_types/*.ron`) — the type
/// combo, the section tints and the rule check read from here.
///
/// ponytail: a flat scan with the manifest only supplying the id — the editor
/// shows every installed type, enabled or not; the mod runtime's
/// dependency-ordered loader matters for the simulator, not for a picker.
#[derive(Resource, Default)]
pub struct TrackTypes {
    pub map: std::collections::BTreeMap<String, track_model::TrackType>,
}

/// Scenery objects of every installed mod (`mods/*/objects/*.ron`) — the
/// object tool's picker and the placement defaults it stamps.
#[derive(Resource, Default)]
pub struct TrackObjects {
    pub map: std::collections::BTreeMap<String, track_model::TrackObject>,
}

/// Everything the installed mods brought, as one system parameter. Four
/// separate resources would put `draw` over Bevy's parameter limit, and they
/// are read together anyway — the content drawer lists all of them.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Catalogs<'w> {
    pub types: Res<'w, TrackTypes>,
    pub objects: Res<'w, TrackObjects>,
    pub signal_types: Res<'w, signals::SignalTypes>,
    pub signal_models: Res<'w, signals::SignalModelFiles>,
    /// Rendered previews of their models — asked for while drawing, which is
    /// what schedules the rendering.
    pub thumbnails: ResMut<'w, thumbnails::Thumbnails>,
}

/// Reads `mods/*/<subdir>/*.ron`, keyed `"<mod id>:<file stem>"`.
fn load_mod_ron<T>(
    root: &std::path::Path,
    subdir: &str,
    parse: fn(&str) -> Result<T, ron::error::SpannedError>,
) -> std::collections::BTreeMap<String, T> {
    #[derive(serde::Deserialize)]
    struct ManifestId {
        id: String,
    }
    let mut map = std::collections::BTreeMap::new();
    let Ok(mods) = std::fs::read_dir(root) else {
        return map;
    };
    for dir in mods.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        let Some(id) = std::fs::read_to_string(dir.join("mod.ron"))
            .ok()
            .and_then(|text| ron::from_str::<ManifestId>(&text).ok())
            .map(|m| m.id)
        else {
            continue;
        };
        let Ok(files) = std::fs::read_dir(dir.join(subdir)) else {
            continue;
        };
        for file in files.flatten().map(|e| e.path()) {
            if file.extension().and_then(|e| e.to_str()) != Some("ron") {
                continue;
            }
            let Some(stem) = file.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            match std::fs::read_to_string(&file)
                .map_err(|e| e.to_string())
                .and_then(|t| parse(&t).map_err(|e| e.to_string()))
            {
                Ok(value) => {
                    map.insert(format!("{id}:{stem}"), value);
                }
                Err(e) => warn!("{}: {e}", file.display()),
            }
        }
    }
    map
}

/// Editor tint of track type `index`: schematic colors that stay legible on
/// aerial imagery — the type's own `color` is the simulator's ballast grey,
/// which would vanish over a dark field. Index 0 keeps the classic orange.
pub fn type_color(index: u32) -> Color {
    if index == 0 {
        return Color::srgb(0.95, 0.35, 0.15);
    }
    match (index - 1) % 5 {
        0 => Color::srgb(0.30, 0.62, 0.95),
        1 => Color::srgb(0.35, 0.80, 0.45),
        2 => Color::srgb(0.80, 0.45, 0.95),
        3 => Color::srgb(0.95, 0.80, 0.25),
        _ => Color::srgb(0.25, 0.82, 0.78),
    }
}

/// The same palette for egui (panel swatches).
pub fn type_color32(index: u32) -> bevy_egui::egui::Color32 {
    let [r, g, b, _] = type_color(index).to_srgba().to_u8_array();
    bevy_egui::egui::Color32::from_rgb(r, g, b)
}

/// The document's own world-anchored entities: what a rebuild despawns —
/// overlay tiles, terrain tiles and the ghost stay (the terrain follows the
/// edit through its own streaming).
type DocumentAnchored = (
    With<WorldAnchored>,
    Without<OverlayTile>,
    Without<GhostTrack>,
    Without<terrain::TerrainChunk>,
);

const UNDO_DEPTH: usize = 64;

/// Undo history of the document — one snapshot of the source per interaction.
#[derive(Resource)]
pub struct History {
    pub undo: Vec<LineSource>,
    pub redo: Vec<LineSource>,
    /// The source as it looked after the last frame — the diff detector.
    last: LineSource,
    /// Whether the source changed in the previous frame, too: a drag mutates
    /// it every frame and must still cost one step.
    changing: bool,
}

impl History {
    pub fn new(source: LineSource) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            last: source,
            changing: false,
        }
    }

    /// A new document (Open, New) starts its own history.
    pub fn reset(&mut self, source: &LineSource) {
        self.undo.clear();
        self.redo.clear();
        self.last = source.clone();
        self.changing = false;
    }

    pub fn undo(&mut self, line: &mut Line) {
        if let Some(state) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut line.source, state));
            self.last = line.source.clone();
            self.changing = false;
            line.dirty = true;
            line.needs_rebuild = true;
            line.terrain_change = terrain::TerrainChange::all();
        }
    }

    pub fn redo(&mut self, line: &mut Line) {
        if let Some(state) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut line.source, state));
            self.last = line.source.clone();
            self.changing = false;
            line.dirty = true;
            line.needs_rebuild = true;
            line.terrain_change = terrain::TerrainChange::all();
        }
    }
}

/// Device marker quad — scaled with the view height so it stays clickable.
#[derive(Component)]
struct DeviceMarker;

/// Path of the overlay configuration.
#[derive(Resource)]
struct ConfigPath(String);

#[derive(Resource)]
struct FrameLimit(u32);

/// Target file from `--screenshot <file.png>`.
#[derive(Resource)]
struct ShotPath(String);

/// What the menu and the panel have asked for — applied by `overlay_control`
/// through the same code paths as the keyboard shortcuts.
#[derive(Resource, Default)]
pub struct Request {
    /// Open the "new module" dialog (menu, Ctrl+N) — see [`new_module`].
    pub new_module: bool,
    pub toggle_overlay: bool,
    pub cycle_provider: bool,
    pub toggle_offline: bool,
    pub clear_cache: bool,
    /// Open the field import dialog (menu, see [`fields`]).
    pub import_fields: bool,
    pub retry_failed: bool,
    pub load_config: bool,
    pub save_config: bool,
    /// Configuration edited in the panel; the flag says whether the tiles
    /// must be rebuilt (provider, opacity or offset changed).
    pub config: Option<(ImageryConfig, bool)>,
}

fn main() {
    // Before anything is built: the very first `t!` is the window title, and
    // it has to come out in the remembered language already.
    let editor_settings = settings::Settings::load();
    editor_settings.apply_language();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let line_path = args.first().filter(|a| !a.starts_with("--")).cloned();
    let config_path = flag("--imagery").unwrap_or_else(|| "imagery.ron".into());
    let shot = flag("--screenshot");
    // `--window 1280x2000` — fixed size for reproducible screenshots, the
    // vehicle editor's convention.
    let window_size = flag("--window").and_then(|s| {
        let (w, h) = s.split_once('x')?;
        Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?))
    });
    if let Some(dir) = shot.as_ref().and_then(|p| std::path::Path::new(p).parent()) {
        let _ = std::fs::create_dir_all(dir);
    }
    // Without `--frames`, about a second of run-up is enough for an image.
    let frame_limit = flag("--frames")
        .and_then(|n| n.parse::<u32>().ok())
        .or_else(|| shot.as_ref().map(|_| 60));
    // `--height`: where the view starts above the line [m] — a screenshot of
    // the trees wants 60 m, one of the module 900.
    let start_height = flag("--height")
        .and_then(|h| h.parse::<f64>().ok())
        .unwrap_or(900.0);

    // `--drawer` alone opens the catalogue on its first category; the name of
    // one (`--drawer signal-types`) opens it there.
    let drawer = args.iter().position(|a| a == "--drawer");
    let drawer_category = drawer
        .and_then(|i| args.get(i + 1))
        .filter(|a| !a.starts_with("--"))
        .and_then(|name| content_drawer::Category::parse(name))
        .unwrap_or_default();
    // `--tool draw` starts with that tool in hand — a screenshot run has no
    // keyboard, and the Tool section only shows the active tool's options.
    let tool = flag("--tool")
        .and_then(|name| tools::Tool::parse(&name))
        .unwrap_or_default();
    let category = tools::category_of(tool);

    let mut app = App::new();
    // Models of trees and scenery objects come from the mods: `mods://<mod>/…`.
    // Has to be registered before the asset plugin.
    app.register_asset_source(world_render::MOD_SOURCE, world_render::mod_asset_source());
    let mut window = Window {
        title: t!("window-route-editor"),
        ..default()
    };
    // `--window` overrides the remembered size for this one run.
    if let Some((w, h)) = window_size {
        window.resolution = bevy::window::WindowResolution::new(w, h);
    } else if let Some((w, h)) = editor_settings.window {
        window.resolution = bevy::window::WindowResolution::new(w as u32, h as u32);
    }
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(window),
        // The close button must pass through `confirm_close`, or it is the one
        // route that still throws unsaved work away.
        close_when_requested: false,
        ..default()
    }))
    .add_plugins(app_icon::plugin)
    .add_plugins(EguiPlugin::default())
    // Terrain, trees and objects are drawn with the simulator's own code.
    .add_plugins(world_render::WorldRenderPlugin)
    // Frame time and entity count for the status bar.
    .add_plugins((
        bevy::diagnostic::FrameTimeDiagnosticsPlugin::default(),
        bevy::diagnostic::EntityCountDiagnosticsPlugin::default(),
    ))
    // The UI belongs on our own camera. Left to itself, `bevy_egui` creates a context on a
    // camera without a render graph — depending on which startup system runs first, and the
    // panels then stay invisible.
    .insert_resource(bevy_egui::EguiGlobalSettings {
        auto_create_primary_context: false,
        ..default()
    })
    .insert_resource(ClearColor(Color::srgb(0.08, 0.09, 0.11)))
    .insert_resource(ConfigPath(config_path))
    .insert_resource(LinePath(line_path))
    .insert_resource(StartHeight(start_height))
    .init_resource::<Request>()
    .insert_resource(EditorState {
        // `--drawer [category]`: the drawer is a keyboard away
        // (`Ctrl`+`Space`), and a screenshot run has no keyboard — the
        // simulator's `--overlays` is the same flag for the same reason.
        drawer: content_drawer::Drawer {
            open: drawer.is_some(),
            category: drawer_category,
            ..default()
        },
        tool,
        category,
        ..default()
    })
    .insert_resource(editor_settings)
    .init_resource::<Ghost>()
    .init_resource::<gizmo::GizmoState>()
    .init_resource::<thumbnails::Thumbnails>()
    .init_resource::<new_module::NewModule>()
    .init_resource::<fields::FieldImport>()
    .init_resource::<terrain::Marks>()
    .init_resource::<tools::GhostPreview>()
    // A glTF spawns its own children, and a render layer does not reach them by
    // itself — the content drawer's preview scene would be drawn into the map.
    .add_plugins(bevy::app::HierarchyPropagatePlugin::<
        bevy::camera::visibility::RenderLayers,
    >::new(Update))
    .add_systems(Startup, setup)
    // After `ui::draw`: the theme is installed there, and the modal belongs on
    // top of the panels anyway.
    .add_systems(
        EguiPrimaryContextPass,
        (ui::draw, new_module::draw, fields::draw).chain(),
    )
    .add_systems(
        Update,
        (
            view::camera_control,
            // Before the tools: a handle drag takes the click the select tool
            // would otherwise read as "reselect whatever is underneath".
            gizmo::input,
            tools::tool_input,
            track_changes,
            terrain::update,
            rebuild,
            spawn_ghost,
            overlay_control,
            overlay::update,
            // Both read the terrain the tiles are built from: the ground under
            // the cursor for the readout, and the ground under every map mark.
            (terrain::probe_cursor, terrain::probe_marks),
            world_render::mount_parts,
            world_render::bind_lamps,
            signals::light_lamps,
            signals::show_finest_lod,
            rebase_origin,
            (
                thumbnails::render,
                tools::draw_gizmos,
                tools::placement_preview,
            )
                .chain(),
            gizmo::draw,
            scale_markers,
            // Nested only because a schedule tuple stops at twenty entries.
            (feed_sky, update_title, ui::poll_file_dialog),
            (track_window_size, confirm_close),
        )
            .chain(),
    );
    if let Some(frames) = frame_limit {
        app.insert_resource(FrameLimit(frames))
            .add_systems(Update, exit_after_frames);
    }
    if let Some(path) = shot {
        app.insert_resource(ShotPath(path));
    }
    app.run();
}

#[derive(Resource)]
struct LinePath(Option<String>);

/// View height at start, from `--height`.
#[derive(Resource)]
struct StartHeight(f64);

/// Puts the module's own place under the sky. The date and the clock come from
/// the time panel; latitude and longitude hang off the module's anchor, exactly
/// as they do in the simulator — the same module therefore gets the same sun in
/// both programs. A module that has no anchor yet falls back to where the view is.
fn feed_sky(line: Res<Line>, focus: Res<Focus>, mut sky: ResMut<sky::Sky>) {
    let (lat, lon) = match line.source.anchor {
        Some(anchor) => (anchor.lat, anchor.lon),
        None => focus_degrees(focus.position),
    };
    sky.latitude = lat.to_radians();
    sky.longitude = lon.to_radians();
    // The editor has no simulation to accumulate them, so the weather's own
    // implication is what the ground shows: rain means wet, snow means covered.
    sky.wetness = f32::from(sky.weather.precip.is_liquid());
    sky.snow = f32::from(sky.weather.precip == sim_core::weather::Precip::Snow);
    sky.cloud_shadow = sky.weather.cover;
}

#[allow(clippy::too_many_arguments)]
fn setup(
    mut commands: Commands,
    config_path: Res<ConfigPath>,
    line_path: Res<LinePath>,
    start_height: Res<StartHeight>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut terrain_materials: ResMut<Assets<world_render::TerrainMaterial>>,
    mut media: ResMut<Assets<bevy::light::atmosphere::ScatteringMedium>>,
    mut star_materials: ResMut<Assets<sky::StarMaterial>>,
    mut moon_materials: ResMut<Assets<sky::MoonMaterial>>,
) {
    // Load the line.
    let (source, path) = match &line_path.0 {
        Some(path) => match std::fs::read_to_string(path).map(|t| LineSource::from_ron(&t)) {
            Ok(Ok(source)) => (source, Some(path.clone())),
            _ => {
                warn!("{path} not readable — example line loaded");
                (content::musterbahn(), None)
            }
        },
        None => (content::musterbahn(), None),
    };
    let net = match source.compile() {
        Ok(compiled) => compiled.net,
        Err(e) => {
            warn!("line does not compile ({e:?}) — example line loaded");
            content::musterbahn().compile().expect("compiles").net
        }
    };

    // View point at the middle of the line; an empty line starts over the example area.
    let focus_position = match net.edges().get(net.edges().len() / 2) {
        Some(middle) => middle.eval(middle.length() / 2.0).pos,
        None => geo::to_ecef_deg(52.0, 10.0, 146.0),
    };
    let origin = RenderOrigin::new(focus_position);

    // Load (or create) the overlay configuration.
    let (config, message) = ImageryConfig::load_or_create(&config_path.0);
    if let Some(message) = &message {
        info!("{message}");
    }
    info!(
        "Aerial imagery: {} ({} providers)",
        config
            .provider()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "—".into()),
        config.providers.len()
    );

    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            far: 60_000.0,
            ..default()
        }),
        Transform::default(),
        // The sky lights the module itself; this is the floor that keeps the
        // relief readable when the time panel is set to the middle of the night.
        AmbientLight {
            color: Color::srgb(0.75, 0.82, 1.0),
            brightness: 60.0,
            ..default()
        },
        sky::camera_settings(),
        PrimaryEguiContext,
    ));
    // The same sky the simulator draws, over the same module: the sun where the
    // module's anchor and the date on the time panel put it (`feed_sky`). A
    // builder judging a cutting at eight in the morning in October gets what the
    // run would show.
    sky::spawn(
        &mut commands,
        &mut meshes,
        &mut media,
        &mut star_materials,
        &mut moon_materials,
        false,
    );

    commands.insert_resource(Overlay::new(config, message.unwrap_or_default()));
    commands.insert_resource(Focus {
        position: focus_position,
        height: start_height.0,
        yaw: 0.0,
        pitch: view::DEFAULT_PITCH,
        speed_step: view::DEFAULT_SPEED_STEP,
        speed_scalar: 1.0,
    });
    commands.insert_resource(Origin(origin));
    let mods_dir = std::path::Path::new("mods");
    commands.insert_resource(TrackTypes {
        map: load_mod_ron(mods_dir, "track_types", track_model::TrackType::from_ron),
    });
    commands.insert_resource(TrackObjects {
        map: load_mod_ron(mods_dir, "objects", track_model::TrackObject::from_ron),
    });
    commands.insert_resource(signals::SignalTypes {
        map: load_mod_ron(mods_dir, "signals", |t| ron::from_str(t)),
    });
    commands.insert_resource(signals::SignalModelFiles {
        map: load_mod_ron(mods_dir, "signal_models", |t| ron::from_str(t)),
    });
    commands.init_resource::<signals::LampImages>();
    commands.init_resource::<world_render::SignalModels>();
    // Terrain material and the (still empty) model catalog — the catalog is
    // filled from the line on the first frame. The editor builds in summer
    // (`Season::default`); which season a run shows is the scenario's date.
    commands.insert_resource(terrain::TerrainView::new(
        world_render::terrain_material(&mut images, &mut terrain_materials, default(), default()),
        world_render::WorldCatalog::new(
            &[],
            &[],
            &Default::default(),
            default(),
            &assets,
            &mut meshes,
            &mut materials,
            default(),
        ),
    ));
    commands.insert_resource(History::new(source.clone()));
    // Track and markers are spawned by `rebuild` on the first frame.
    commands.insert_resource(Line {
        source,
        net,
        path,
        dirty: false,
        needs_rebuild: true,
        terrain_change: terrain::TerrainChange::all(),
        recenter: false,
        issues: Vec::new(),
    });
}

/// One undo step per interaction: whoever mutated `Line::source` this frame —
/// a tool click or a panel drag — is picked up here by comparison, so the
/// mutation sites stay plain writes. The same comparison says what the edit
/// reached, so the scene and the terrain rebuild only that.
fn track_changes(mut line: ResMut<Line>, mut history: ResMut<History>, state: Res<EditorState>) {
    record_change(&mut line, &mut history, state.terrain_options().zone);
}

fn record_change(line: &mut Line, history: &mut History, zone: u8) {
    if line.source == history.last {
        history.changing = false;
        return;
    }
    let (scene, terrain) = diff(&history.last, &line.source, &line.net, zone);
    if !history.changing {
        if history.undo.len() == UNDO_DEPTH {
            history.undo.remove(0);
        }
        let last = history.last.clone();
        history.undo.push(last);
        history.redo.clear();
    }
    history.changing = true;
    history.last = line.source.clone();
    line.dirty = true;
    line.needs_rebuild |= scene;
    line.terrain_change.merge(terrain);
}

/// What changed between two states of the source: whether the drawn scene
/// (track, signals, areas, markers) has to be built anew, and where the
/// terrain was touched. Names and metadata change neither.
fn diff(
    last: &LineSource,
    now: &LineSource,
    net: &TrackNetwork,
    zone: u8,
) -> (bool, terrain::TerrainChange) {
    use terrain::{Region, TerrainChange};
    let utm = |lat: f64, lon: f64| {
        let (e, n) = geo::to_utm(lat.to_radians(), lon.to_radians(), zone);
        glam::DVec2::new(e, n)
    };
    let mut change = TerrainChange::default();
    // The track shapes the formation everywhere it runs, and every object
    // stands relative to it.
    let track = last.nodes != now.nodes
        || last.edges != now.edges
        || last.heights != now.heights
        || last.geoid_offset != now.geoid_offset;
    if track {
        change = TerrainChange::all();
    }
    let scene = track
        || last.devices != now.devices
        || last.markers != now.markers
        || last.sections != now.sections
        || last.areas != now.areas
        || last.signals != now.signals
        || last.routes != now.routes
        || last.boundaries != now.boundaries
        || last.electrification != now.electrification
        || last.anchor != now.anchor
        || last.envelope != now.envelope
        // The walkways are drawn as gizmos, but the rule check that reports
        // a vertex outside the envelope runs with the rebuild, like the
        // envelope's own. The fields are checked the same way.
        || last.walk_paths != now.walk_paths
        || last.walk_areas != now.walk_areas
        || last.fields != now.fields;
    if !track {
        // A stroke reaches the ground under its disc, before and after.
        for stroke in changed(&last.terrain, &now.terrain) {
            change
                .ground
                .add_disc(utm(stroke.lat, stroke.lon), stroke.radius.max(1.0));
        }
        // A tree or an object only moves itself — the tile places its trees
        // and objects anew on the ground it has.
        for tree in changed(&last.trees, &now.trees) {
            change.scatter.add_disc(utm(tree.lat, tree.lon), 1.0);
        }
        for object in changed(&last.objects, &now.objects) {
            if let Some(pos) = tools::object_pos(net, object) {
                let (lat, lon, _) = geo::from_ecef(pos);
                change
                    .scatter
                    .add_disc(utm(lat.to_degrees(), lon.to_degrees()), 1.0);
            }
        }
        // A field is not something standing on the ground — it *is* the ground,
        // part of the tile's own mesh, so a changed one rebuilds the tiles
        // under it. An import changes hundreds at once, and marking each of
        // them costs more than rebuilding the module.
        if last.fields != now.fields {
            const MANY: usize = 32;
            let touched: Vec<&content::route::FieldSource> =
                changed(&last.fields, &now.fields).take(MANY + 1).collect();
            if touched.len() > MANY {
                change = TerrainChange::all();
            } else {
                for field in touched {
                    let corners: Vec<glam::DVec2> =
                        field.polygon.iter().map(|p| utm(p.lat, p.lon)).collect();
                    let Some(lo) = corners.iter().copied().reduce(glam::DVec2::min) else {
                        continue;
                    };
                    let hi = corners
                        .iter()
                        .copied()
                        .reduce(glam::DVec2::max)
                        .unwrap_or(lo);
                    change
                        .ground
                        .add_disc((lo + hi) / 2.0, (hi - lo).length() / 2.0 + 1.0);
                }
            }
        }
    }
    if !matches!(change.ground, Region::None) && !matches!(change.ground, Region::All) {
        // Ground that moves takes what stands on it along — the rebuilt tile
        // places its trees anew, nothing to mark separately.
    }
    (scene, change)
}

/// The entries of two lists that differ: the ones that changed in place, and
/// whatever one list has past the end of the other — both the old and the new
/// state, since a moved tree left a place as well as arrived at one.
fn changed<'a, T: PartialEq>(last: &'a [T], now: &'a [T]) -> impl Iterator<Item = &'a T> {
    let shared = last.len().min(now.len());
    last[..shared]
        .iter()
        .zip(&now[..shared])
        .filter(|(a, b)| a != b)
        .flat_map(|(a, b)| [a, b])
        .chain(&last[shared..])
        .chain(&now[shared..])
}

/// Recompiles the source and respawns track and markers after every edit.
#[allow(clippy::too_many_arguments)]
fn rebuild(
    mut line: ResMut<Line>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut origin: ResMut<Origin>,
    mut focus: ResMut<Focus>,
    mut overlay: ResMut<Overlay>,
    types: Res<TrackTypes>,
    objects: Res<TrackObjects>,
    signal_types: Res<signals::SignalTypes>,
    signal_files: Res<signals::SignalModelFiles>,
    mut lamp_images: ResMut<signals::LampImages>,
    mut signal_models: ResMut<world_render::SignalModels>,
    assets: Res<AssetServer>,
    old: Query<Entity, DocumentAnchored>,
) {
    if !line.needs_rebuild {
        return;
    }
    line.needs_rebuild = false;
    match line.source.compile() {
        Ok(compiled) => line.net = compiled.net,
        Err(e) => {
            overlay.status = t!("status-compile-error", error = format!("{e:?}"));
            return;
        }
    }
    line.issues = line.source.check(&types.map, &objects.map);
    for entity in old.iter() {
        commands.entity(entity).despawn();
    }
    if line.recenter {
        line.recenter = false;
        let edges = line.net.edges();
        if let Some(middle) = edges.get(edges.len() / 2) {
            focus.position = middle.eval(middle.length() / 2.0).pos;
            origin.0 = RenderOrigin::new(focus.position);
            overlay.clear(&mut commands);
        }
    }
    // The line is drawn as the run builds it — the ground is always there to
    // draw it on.
    world_render::spawn_track(
        &mut commands,
        &mut meshes,
        &mut materials,
        &assets,
        &line.net,
        &origin.0,
    );
    let (models, images) = signals::spawn(
        &mut commands,
        &mut meshes,
        &mut materials,
        &assets,
        &line,
        &signal_types,
        &signal_files,
        &origin,
    );
    signal_models.0 = models;
    lamp_images.0 = images;
    // The painted areas go over the built track — the marking belongs to the
    // line, not to the way it happens to be drawn.
    spawn_areas(
        &mut commands,
        &mut meshes,
        &mut materials,
        &line.source,
        &line.net,
        &origin.0,
    );
    spawn_markers(
        &mut commands,
        &mut meshes,
        &mut materials,
        &line.source,
        &line.net,
        &origin.0,
        true,
    );
    // Scenery objects come with the terrain tiles, on the ground of the tile
    // each one stands on — the same way the run shows them.
}

/// Track ribbon mesh of one edge between `s0` and `s1` in its anchor frame,
/// `half_width` metres to either side and `lift` metres above the plane.
fn ribbon_mesh(edge: &TrackEdge, half_width: f64, lift: f64, s0: f64, s1: f64) -> Mesh {
    let frame = EnuFrame::at(edge.anchor);
    let steps = (((s1 - s0) / 5.0).ceil() as usize).max(2);
    let mut positions = Vec::with_capacity((steps + 1) * 2);
    for i in 0..=steps {
        let s = s0 + (s1 - s0) * i as f64 / steps as f64;
        let pose = edge.eval(s);
        let center = frame.to_local(pose.pos);
        let tangent = frame.dir_to_local(pose.tangent);
        let up = frame.dir_to_local(pose.up);
        let right = tangent.cross(up).normalize_or_zero() * half_width;
        for side in [-1.0, 1.0] {
            let p = center + right * side + DVec3::new(0.0, 0.0, lift);
            positions.push([p.x as f32, p.z as f32, -p.y as f32]);
        }
    }
    let mut indices = Vec::new();
    for row in 0..steps {
        let a = (row * 2) as u32;
        // Counter-clockwise seen from above — a clockwise strip is a
        // backface to the top-down camera and is culled.
        indices.extend_from_slice(&[a, a + 1, a + 2, a + 2, a + 1, a + 3]);
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    let count = positions.len();
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; count]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0]; count]);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Track ribbon as a colored quad — reference for the position in the aerial
/// imagery, tinted per track-type section ([`type_color`]).
/// The marked areas as what they are: a wide coloured stroke painted over the
/// track, one quad per stretch, in the colour the area wears.
///
/// Drawn a little above the track ribbon and a good deal wider than it, and
/// half transparent so the track underneath still reads — a highlighter over a
/// map, which is exactly the gesture that put it there. Where two areas overlap
/// the later one is drawn last and therefore wins, the same rule the compile
/// uses, so the map shows what the line will get.
fn spawn_areas(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    source: &LineSource,
    net: &TrackNetwork,
    origin: &RenderOrigin,
) {
    for area in &source.areas {
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(area.color.0, area.color.1, area.color.2, 0.55),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        });
        for span in &area.spans {
            let Some(edge) = net.edges().get(span.edge as usize) else {
                continue;
            };
            let (s0, s1) = (
                span.from.clamp(0.0, edge.length()),
                span.to.clamp(0.0, edge.length()),
            );
            if s1 <= s0 {
                continue;
            }
            let frame = EnuFrame::at(edge.anchor);
            let (translation, rotation) = origin.frame_transform(&frame);
            commands.spawn((
                Mesh3d(meshes.add(ribbon_mesh(edge, area.width, tools::AREA_LIFT, s0, s1))),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(translation).with_rotation(rotation),
                WorldAnchored::at(edge.anchor),
            ));
        }
    }
}

/// (Re)spawns the ghost module's track after a load or clear. Grey and a
/// little lower than the edited line, so the line wins where they overlap.
fn spawn_ghost(
    mut ghost: ResMut<Ghost>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    origin: Res<Origin>,
    old: Query<Entity, With<GhostTrack>>,
) {
    if !ghost.respawn {
        return;
    }
    ghost.respawn = false;
    for entity in old.iter() {
        commands.entity(entity).despawn();
    }
    let Some(net) = &ghost.net else {
        return;
    };
    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.55, 0.57, 0.62),
        unlit: true,
        ..default()
    });
    for edge in net.edges() {
        let frame = EnuFrame::at(edge.anchor);
        let (translation, rotation) = origin.0.frame_transform(&frame);
        commands.spawn((
            Mesh3d(meshes.add(ribbon_mesh(edge, 1.5, 0.25, 0.0, edge.length()))),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(translation).with_rotation(rotation),
            WorldAnchored::at(edge.anchor),
            GhostTrack,
        ));
    }
}

/// One colored quad per trackside device, lifted above the ribbon.
#[allow(clippy::too_many_arguments)]
fn spawn_markers(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    source: &LineSource,
    net: &TrackNetwork,
    origin: &RenderOrigin,
    world_view: bool,
) {
    let mesh = meshes
        .add(Mesh::from(Plane3d::default().mesh().size(2.0, 2.0)).translated_by(Vec3::Y * 0.8));
    for device in &source.devices {
        // In the world view a signal stands there as its model — a marker on
        // top of it would be the one thing the run does not show.
        if world_view && device.kind == DeviceKind::Signal {
            continue;
        }
        let Some(pos) = tools::device_pos(net, device) else {
            continue;
        };
        let frame = EnuFrame::at(pos);
        let (translation, rotation) = origin.frame_transform(&frame);
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: marker_color(&device.kind),
                unlit: true,
                ..default()
            })),
            Transform::from_translation(translation).with_rotation(rotation),
            WorldAnchored::at(pos),
            DeviceMarker,
        ));
    }
}

/// Marker colors per device kind — legend colors, not simulation state.
fn marker_color(kind: &DeviceKind) -> Color {
    match kind {
        DeviceKind::Signal => Color::srgb(0.90, 0.22, 0.22),
        DeviceKind::Magnet => Color::srgb(0.95, 0.60, 0.12),
        DeviceKind::LineConductor => Color::srgb(0.65, 0.40, 0.95),
        DeviceKind::Balise => Color::srgb(0.20, 0.80, 0.85),
        DeviceKind::SpeedBoard => Color::srgb(0.95, 0.85, 0.25),
        DeviceKind::Platform => Color::srgb(0.30, 0.55, 0.95),
        DeviceKind::StopBoard => Color::srgb(0.92, 0.92, 0.92),
        DeviceKind::BlockMarker => Color::srgb(0.25, 0.80, 0.55),
        DeviceKind::NeutralSection => Color::srgb(0.85, 0.30, 0.75),
        DeviceKind::Other(_) => Color::srgb(0.60, 0.62, 0.68),
    }
}

/// Keeps device markers a few pixels big at any height.
fn scale_markers(focus: Res<Focus>, mut markers: Query<&mut Transform, With<DeviceMarker>>) {
    let scale = ((focus.height / 250.0) as f32).clamp(1.0, 40.0);
    for mut transform in markers.iter_mut() {
        transform.scale = Vec3::splat(scale);
    }
}

/// All overlay settings via keys.
fn overlay_control(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut overlay: ResMut<Overlay>,
    mut request: ResMut<Request>,
    config_path: Res<ConfigPath>,
    state: Res<EditorState>,
    focus: Res<Focus>,
) {
    let mut config = overlay.config().clone();
    let mut changed = false;
    let mut rebuild = false;

    // Menu entries and panel widgets take the same paths as the keys.
    let menu = std::mem::take(&mut *request);
    if let Some((edited, rebuild_tiles)) = menu.config {
        config = edited;
        changed = true;
        rebuild |= rebuild_tiles;
    }
    if menu.toggle_overlay {
        config.enabled = !config.enabled;
        changed = true;
    }
    if menu.cycle_provider {
        config.cycle_provider();
        changed = true;
        rebuild = true;
    }
    if menu.toggle_offline {
        config.cache.offline = !config.cache.offline;
        changed = true;
    }
    if menu.clear_cache {
        overlay.source.clear_cache();
        overlay.clear(&mut commands);
        overlay.status = t!("status-cache-cleared");
    }
    if menu.retry_failed {
        overlay.source.retry_failed();
        overlay.status = t!("status-retry-reset");
    }
    if menu.save_config {
        overlay.status = match config.save(&config_path.0) {
            Ok(()) => t!("status-saved", file = config_path.0),
            Err(e) => t!("status-save-failed", error = e),
        };
    }
    if menu.load_config {
        let (loaded, message) = ImageryConfig::load_or_create(&config_path.0);
        overlay.status = message.unwrap_or_else(|| t!("status-loaded", file = config_path.0));
        overlay.apply(&mut commands, loaded);
        return;
    }

    // The letter keys are overlay shortcuts only while no text field is typed in.
    if state.typing {
        if changed {
            if rebuild {
                overlay.apply(&mut commands, config);
            } else {
                overlay.source.set_config(config);
            }
        }
        return;
    }

    if keys.just_pressed(KeyCode::KeyO) {
        config.enabled = !config.enabled;
        changed = true;
    }
    if keys.just_pressed(KeyCode::KeyP) {
        config.cycle_provider();
        changed = true;
        rebuild = true;
    }
    if keys.just_pressed(KeyCode::BracketLeft) {
        config.opacity = (config.opacity - 0.1).max(0.0);
        changed = true;
        rebuild = true;
    }
    if keys.just_pressed(KeyCode::BracketRight) {
        config.opacity = (config.opacity + 0.1).min(1.0);
        changed = true;
        rebuild = true;
    }
    // Zoom level: turn the current level into a fixed one and shift it.
    let (lat, _) = focus_degrees(focus.position);
    if keys.just_pressed(KeyCode::Comma) {
        config.zoom = ZoomMode::Fixed(config.zoom_for(lat).saturating_sub(1));
        changed = true;
    }
    if keys.just_pressed(KeyCode::Period) {
        config.zoom = ZoomMode::Fixed(config.zoom_for(lat).saturating_add(1));
        changed = true;
    }
    if keys.just_pressed(KeyCode::KeyZ) {
        config.zoom = ZoomMode::Resolution(0.5);
        changed = true;
    }
    // Image offset against the map (aerial imagery is often off by metres).
    let step = if keys.pressed(KeyCode::ShiftLeft) {
        5.0
    } else {
        0.5
    };
    for (key, delta) in [
        (KeyCode::Numpad8, (0.0, step)),
        (KeyCode::Numpad2, (0.0, -step)),
        (KeyCode::Numpad6, (step, 0.0)),
        (KeyCode::Numpad4, (-step, 0.0)),
    ] {
        if keys.just_pressed(key) {
            config.offset.0 += delta.0;
            config.offset.1 += delta.1;
            changed = true;
            rebuild = true;
        }
    }
    if keys.just_pressed(KeyCode::Numpad5) {
        config.offset = (0.0, 0.0);
        changed = true;
        rebuild = true;
    }
    if keys.just_pressed(KeyCode::KeyL) {
        config.cache.offline = !config.cache.offline;
        changed = true;
    }

    if keys.just_pressed(KeyCode::KeyC) {
        overlay.source.clear_cache();
        overlay.clear(&mut commands);
        overlay.status = t!("status-cache-cleared");
    }
    if keys.just_pressed(KeyCode::KeyR) {
        overlay.source.retry_failed();
        overlay.status = t!("status-retry-reset");
    }
    if keys.just_pressed(KeyCode::F5) {
        let (loaded, message) = ImageryConfig::load_or_create(&config_path.0);
        overlay.status = message.unwrap_or_else(|| t!("status-loaded", file = config_path.0));
        overlay.apply(&mut commands, loaded);
        return;
    }
    if keys.just_pressed(KeyCode::F2) {
        overlay.status = match config.save(&config_path.0) {
            Ok(()) => t!("status-saved", file = config_path.0),
            Err(e) => t!("status-save-failed", error = e),
        };
    }

    if changed {
        if rebuild {
            overlay.apply(&mut commands, config);
        } else {
            overlay.source.set_config(config);
        }
    }
}

/// Follow up the origin and re-align world-anchored objects.
fn rebase_origin(
    focus: Res<Focus>,
    mut origin: ResMut<Origin>,
    mut anchored: Query<(&WorldAnchored, &mut Transform), Without<OverlayTile>>,
    mut tiles: Query<(&OverlayTile, &mut Transform), Without<WorldAnchored>>,
) {
    if !origin.0.rebase_if_needed(focus.position) {
        return;
    }
    for (item, mut transform) in anchored.iter_mut() {
        let (translation, rotation) = item.transform(&origin.0);
        transform.translation = translation;
        transform.rotation = rotation;
    }
    overlay::resync(&origin.0, &mut tiles);
}

/// The window title names the document, plus the unsaved marker.
fn update_title(
    line: Res<Line>,
    mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
) {
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    let key = if line.dirty {
        "window-route-editor-unsaved"
    } else {
        "window-route-editor-named"
    };
    let title = t!(key, name = line.source.name);
    if window.title != title {
        window.title = title;
    }
}

/// Keeps the window size in the settings struct — in memory only. Writing on
/// every frame of a resize drag would hammer the disk; the file is written
/// when the user leaves.
fn track_window_size(
    mut settings: ResMut<settings::Settings>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let size = (window.width(), window.height());
    if settings.window != Some(size) {
        settings.window = Some(size);
    }
}

/// The window's close button goes through the discard guard like Quit does.
fn confirm_close(
    mut requests: MessageReader<bevy::window::WindowCloseRequested>,
    mut line: ResMut<Line>,
    mut state: ResMut<EditorState>,
    mut overlay: ResMut<Overlay>,
    settings: Res<settings::Settings>,
    mut exit: MessageWriter<AppExit>,
) {
    if requests.read().next().is_none() {
        return;
    }
    requests.clear();
    if ui::confirm_discard(&mut line, &mut state, &mut overlay) {
        settings.save();
        exit.write(AppExit::Success);
    }
}

/// Exits the editor after the given number of frames — with `--screenshot` the window is
/// captured beforehand.
fn exit_after_frames(
    limit: Res<FrameLimit>,
    shot: Option<Res<ShotPath>>,
    mut commands: Commands,
    mut count: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    *count += 1;
    let Some(shot) = shot else {
        if *count >= limit.0 {
            exit.write(AppExit::Success);
        }
        return;
    };
    if *count == limit.0 {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(shot.0.clone()));
    }
    // The capture goes through the render thread: it only lands on disk a few frames later.
    if *count >= limit.0 + 10 {
        exit.write(AppExit::Success);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The unit at this seam was already wrong once: `geo::from_ecef` returns radians,
    /// the tile grid expects degrees — the tiles thus ended up thousands of kilometres
    /// off, in the middle of the Atlantic.
    #[test]
    fn view_point_comes_in_degrees() {
        let position = geo::to_ecef_deg(52.0006, 10.0509, 146.0);
        let (lat, lon) = focus_degrees(position);
        assert!((lat - 52.0006).abs() < 1e-6, "{lat}");
        assert!((lon - 10.0509).abs() < 1e-6, "{lon}");

        let tile = imagery::TileId::from_lat_lon(lat, lon, 18);
        let (west, south, east, north) = tile.bounds();
        assert!(west < 10.0509 && 10.0509 < east, "{west}…{east}");
        assert!(south < 52.0006 && 52.0006 < north, "{south}…{north}");
    }

    #[test]
    fn example_line_is_in_view() {
        let compiled = content::musterbahn().compile().expect("compiles");
        assert!(!compiled.net.edges().is_empty());
        let middle = &compiled.net.edges()[compiled.net.edges().len() / 2];
        let (lat, lon) = focus_degrees(middle.eval(middle.length() / 2.0).pos);
        assert!((51.0..53.0).contains(&lat) && (9.0..11.0).contains(&lon));
    }

    /// The undo history records one step per mutation and survives a round trip.
    #[test]
    fn undo_round_trips_an_edit() {
        let source = content::musterbahn();
        let mut line = Line {
            source: source.clone(),
            net: source.compile().unwrap().net,
            path: None,
            dirty: false,
            needs_rebuild: false,
            terrain_change: Default::default(),
            recenter: false,
            issues: Vec::new(),
        };
        let mut history = History::new(source.clone());

        // An edit, picked up by the diff detector: a device is scene, not
        // terrain.
        line.source.devices.pop();
        record_change(&mut line, &mut history, 32);
        assert!(line.dirty && history.undo.len() == 1);
        assert!(line.needs_rebuild);
        assert!(line.terrain_change.is_none());

        history.undo(&mut line);
        assert_eq!(line.source, source);
        history.redo(&mut line);
        assert_eq!(line.source.devices.len(), source.devices.len() - 1);
    }

    /// The diff names what an edit reached: a moved tree only its tile's
    /// scatter, a stroke the ground under it, a track change everything.
    #[test]
    fn the_diff_tells_the_terrain_what_an_edit_reached() {
        use terrain::Region;
        let source = content::musterbahn();
        let net = source.compile().unwrap().net;
        let zone = 32;

        let mut moved = source.clone();
        moved.trees.push(content::TreeSource {
            object: String::new(),
            lat: 52.0,
            lon: 10.0,
            yaw_deg: 0.0,
            scale: 1.0,
        });
        let (scene, change) = diff(&source, &moved, &net, zone);
        assert!(!scene, "a tree is not part of the drawn scene");
        assert!(change.ground.is_none());
        assert!(matches!(change.scatter, Region::Rects(ref r) if r.len() == 1));

        let mut shaped = source.clone();
        shaped.terrain.push(content::route::TerrainEditSource {
            lat: 52.0,
            lon: 10.0,
            radius: 100.0,
            edit: content::route::TerrainEdit::Raise(5.0),
        });
        let (scene, change) = diff(&source, &shaped, &net, zone);
        assert!(!scene);
        assert!(matches!(change.ground, Region::Rects(ref r) if r.len() == 1));
        assert!(change.scatter.is_none());

        let mut renamed = source.clone();
        renamed.name.push('!');
        let (scene, change) = diff(&source, &renamed, &net, zone);
        assert!(!scene && change.is_none(), "a name changes nothing drawn");

        let mut relaid = source.clone();
        relaid.edges.pop();
        let (scene, change) = diff(&source, &relaid, &net, zone);
        assert!(scene);
        assert_eq!(change, terrain::TerrainChange::all());
    }

    /// A field *is* the ground, so adding one has to rebuild the tiles under
    /// it — without this an import would leave the module looking unchanged
    /// until something else happened to touch the terrain.
    #[test]
    fn a_field_rebuilds_the_ground_it_lies_on() {
        use terrain::Region;
        let source = content::musterbahn();
        let net = source.compile().unwrap().net;
        let zone = 32;

        let field = |lat: f64, lon: f64| content::route::FieldSource {
            polygon: [(0.0, 0.0), (0.0, 0.002), (0.002, 0.002), (0.002, 0.0)]
                .into_iter()
                .map(|(dlat, dlon)| content::route::FieldPoint {
                    lat: lat + dlat,
                    lon: lon + dlon,
                })
                .collect(),
            crop: "winter-cereal".into(),
            code: String::new(),
            label: String::new(),
            level: String::new(),
            direction_deg: 0.0,
            source: String::new(),
            year: None,
            seed: 1,
            tags: Vec::new(),
        };

        let mut sown = source.clone();
        sown.fields.push(field(52.0, 10.0));
        let (scene, change) = diff(&source, &sown, &net, zone);
        // The rule check runs with the rebuild, like the walkways'.
        assert!(scene, "a field is checked with the scene");
        assert!(
            matches!(change.ground, Region::Rects(ref r) if r.len() == 1),
            "{:?}",
            change.ground
        );

        // A whole import is not marked field by field — past a few dozen it is
        // cheaper to rebuild the module than to list what moved.
        let mut imported = source.clone();
        for i in 0..200 {
            imported.fields.push(field(52.0 + i as f64 * 0.003, 10.0));
        }
        let (_, change) = diff(&source, &imported, &net, zone);
        assert_eq!(change, terrain::TerrainChange::all());
    }
}
