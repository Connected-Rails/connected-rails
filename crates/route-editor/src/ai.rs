//! Reading the aerial imagery with a local model, and putting what it finds
//! on the module.
//!
//! The imagery a route editor already drapes over the ground is a survey of
//! the real place. A builder spends hours transcribing it by hand — every car
//! in the station car park, one click each. This is the dialog that has a
//! model do it: pick a model, say *where* to look, run it, look at the
//! summary, commit. One undo step for the whole run, like every other import.
//!
//! Two things make it usable rather than a toy:
//!
//! * **It is never let loose on the whole module.** The area is either a
//!   corridor of a stated width along the track, or the area drawn in the
//!   viewport — and in both, nothing is placed within the stated clearance of
//!   a rail. That is [`vision::Region`], and it is what decides which windows
//!   are fetched at all.
//! * **Nothing is written until the user says so.** The run happens on a
//!   thread with a Stop that means it; the report is a summary, and Commit is
//!   what touches the line.
//!
//! What is placed is decided by the model's own class list: a class names a
//! **tag** ([`vision::ClassSpec::place`]), and the editor places an object
//! carrying that tag from whatever mods are installed. So a model that finds
//! lorries and a mod full of lorries meet without either knowing about the
//! other, and the next detector is an entry in `ai.ron` rather than a change
//! here.
//!
//! Car parks are not detected — they are inferred from the cars, and paved
//! with the road the module already has (see [`vision::parking`]).
//!
//! Trees are the same walk with a different answer at the end of it. A class
//! marked [`Placement::Tree`] does not become an object against the track: it
//! becomes a row in the line's own tree list, at the place the crown was, in a
//! species carrying the tag the model gave it, grown to the size the crown was
//! measured at. That matters beyond tidiness — a tree in that list is drawn by
//! the vegetation instancer, is picked by the select tool, joins a
//! multi-selection and goes with one Delete, exactly like a tree planted by
//! hand or baked by the forest brush. Nothing about a wood the model found is
//! any harder to take back than a wood a person drew.

use crate::tools::{EditorState, Selection};
use crate::{AiPath, Line, TrackObjects};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use content::route::{CenterLine, ObjectSource, RoadPoint, RoadSource, RoadSurface, TreeSource};
use editor_ui::{colors, space};
use i18n::t;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use track_model::Footprint;
use track_model::TrackNetwork;
use vision::{GeoDetection, Lot, Placement, Progress, Region, Shape, VisionConfig};
use world_coords::{EcefPos, EnuFrame, geo};

/// Width of the dialog [px] — the road import's, so the two look alike.
const DIALOG: f32 = 460.0;
/// How finely the track is sampled for the region [m]. The clearance is
/// measured against the straight line between two samples, and at ten metres
/// that line is inside a centimetre of the curve on anything a train runs on.
const TRACK_STEP: f64 = 10.0;
/// Tiles held in memory while a run walks the region.
const TILE_CACHE: usize = 96;
/// How far apart two cars may be and still be in the same car park [m].
const LOT_RADIUS: f64 = 18.0;
/// How close a find may come to something already standing before it is taken
/// as already placed [m]. Makes a second run over the same ground a no-op
/// rather than a doubling.
const OCCUPIED: f64 = 2.0;
/// Tags of the paved ribbon a recognised car park becomes.
/// What marks a paved ribbon as a car park rather than a street — both to a
/// builder reading the file and to `streets`, which must not take one for a
/// road that has to be kept clear.
const LOT_TAG: &str = "parking";
const LOT_TAGS: [&str; 2] = [LOT_TAG, "ai"];
/// The tag whose finds are clustered into car parks. A car park is cars; a
/// row of lorries at a goods shed is not one, and neither is anything else a
/// model may learn to find later.
const CAR: &str = "car";
/// How far from the crown that was measured a species may be and still be
/// planted on it, as a factor either way.
///
/// Wider than the cars' rule, and deliberately: a car park is a grid of
/// stated sizes, a wood is not. Half again in each direction spans the three
/// individuals every species in `mods/trees` ships — the same species young,
/// grown and old — so a crown of any size finds something to be.
const SPREAD: f64 = 1.5;
/// How far a planted tree may be grown or shrunk from its own size to stand as
/// wide as the crown says.
///
/// The clamp is the point rather than a safety net. A thirty-metre spruce
/// squeezed to a third is not a young spruce: the trunk stays as thick as a
/// mature one and the needles come out the size of branches. Past the band the
/// species was the wrong choice and the size is the lesser error of the two.
const GROWTH: (f64, f64) = (0.6, 1.5);

/// Which ground the run covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Area {
    /// A stated width either side of the track.
    #[default]
    Corridor,
    /// The area drawn in the viewport — the AI area tool's polygon, or the
    /// circle the select tool last grew.
    Selection,
}

/// What the dialog asks for.
#[derive(Debug, Clone, PartialEq)]
pub struct AiOptions {
    /// Id of the model to run — [`vision::ModelSpec::id`].
    pub model: String,
    pub area: Area,
    /// Half-width of the corridor [m].
    pub corridor: f64,
    /// How close to a rail anything may be placed [m].
    pub keep_clear: f64,
    /// Place what the model finds.
    pub place_objects: bool,
    /// Plant the trees it finds — the tree classes of the model, into the
    /// line's own tree list.
    pub place_trees: bool,
    /// Which species the crowns become: `None` is what the model said, and
    /// `Some("nadelwald")` is the stand of that name whatever it said.
    ///
    /// The escape hatch for the one thing the imagery cannot settle. A crown
    /// detector says *tree*; whether it is a fir or a lime is read off the
    /// crown's own look ([`vision::canopy`]) and at the resolution a provider
    /// gives that is a hint, not a finding. A builder looking at the same
    /// photograph can see it at a glance, and this is where they say so —
    /// once, for the run, instead of correcting four hundred trees.
    pub species: Option<String>,
    /// How close a planted tree may come to a tree that already stands there
    /// [m]. What makes a second run over the same wood a no-op rather than a
    /// doubling of it.
    pub tree_spacing: f64,
    /// Pave the car parks the cars stand in.
    pub pave_lots: bool,
    /// How many cars make a car park.
    pub min_lot_cars: usize,
}

impl Default for AiOptions {
    fn default() -> Self {
        Self {
            model: String::new(),
            area: Area::Corridor,
            // Eighty metres reaches the station forecourt and the goods yard
            // without walking the fields behind them.
            corridor: 80.0,
            // Clear of the six-foot, the cable route and the cess.
            keep_clear: 8.0,
            place_objects: true,
            place_trees: true,
            species: None,
            // A crown is metres across; two trunks three metres apart are two
            // trees anywhere but in a hedge, and a hedge is not what a crown
            // detector finds.
            tree_spacing: 3.0,
            pave_lots: true,
            min_lot_cars: 6,
        }
    }
}

/// What a finished run has to show for itself: the walk's own outcome, plus
/// the car parks the cars in it stand on.
pub struct Report {
    outcome: vision::Outcome,
    lots: Vec<Lot>,
}

/// A running detection: what the thread says, and the switch that stops it.
struct Job {
    /// Behind a mutex so the dialog works as a Bevy resource — a `Receiver`
    /// is `Send` but not `Sync`.
    progress: Mutex<Receiver<Progress>>,
    result: Mutex<Receiver<Result<Report, String>>>,
    stop: Arc<AtomicBool>,
}

/// The dialog and everything it holds between frames.
#[derive(Resource, Default)]
pub struct AiImport {
    pub open: bool,
    pub options: AiOptions,
    /// The model registry, loaded from `ai.ron` on first open.
    pub config: Option<VisionConfig>,
    job: Option<Job>,
    progress: Option<Progress>,
    report: Option<Report>,
    message: String,
}

impl AiImport {
    /// Up from the first frame — what `--detect` inserts, so a screenshot run
    /// can look at the dialog it has no keyboard to open.
    pub fn opened() -> Self {
        Self {
            open: true,
            ..Default::default()
        }
    }
}

// ---------------------------------------------------------------------------
// The area drawn in the viewport
// ---------------------------------------------------------------------------

/// Closes the polygon the AI area tool was drawing — Enter or right-click,
/// through the same dispatch the field and road tools use.
pub fn finish(state: &mut EditorState) -> Option<String> {
    let points = std::mem::take(&mut state.walk_points);
    if points.len() < 3 {
        return Some(t!("status-ai-area-points"));
    }
    let corners = points.len();
    state.ai_area = Some(points);
    Some(t!("status-ai-area-set", corners = corners))
}

/// Takes the circle the select tool has just grown as the area, so the
/// gesture a builder already uses to say "this bit here" says it to the model
/// as well. A circle is stored as the polygon that fits it — the run knows
/// one shape, and one shape is one thing to get right.
pub fn area_from_circle(state: &mut EditorState, centre: EcefPos, radius: f64) {
    const CORNERS: usize = 24;
    let frame = EnuFrame::at(centre);
    state.ai_area = Some(
        (0..CORNERS)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / CORNERS as f64;
                frame.to_ecef(glam::DVec3::new(radius * a.sin(), radius * a.cos(), 0.0))
            })
            .collect(),
    );
}

/// The tool's own options: what area stands, and the way to the dialog that
/// uses it. Returns `true` when the dialog should open.
pub fn tool_rows(ui: &mut egui::Ui, state: &mut EditorState) -> bool {
    match state.ai_area.as_ref().map(Vec::len) {
        Some(corners) if corners >= 3 => {
            ui.label(t!("ai-drawn-corners", corners = corners));
        }
        _ => {
            ui.small(t!("ai-area-none"));
        }
    }
    let mut open = false;
    ui.horizontal(|ui| {
        open = ui.button(t!("ai-open-dialog")).clicked();
        if ui
            .add_enabled(
                state.ai_area.is_some(),
                egui::Button::new(t!("ai-clear-area")),
            )
            .clicked()
        {
            state.ai_area = None;
        }
    });
    open
}

/// The area in degrees, as the region wants it.
fn area_degrees(state: &EditorState) -> Option<Vec<(f64, f64)>> {
    let points = state.ai_area.as_ref()?;
    (points.len() >= 3).then(|| {
        points
            .iter()
            .map(|p| {
                let (lat, lon, _) = geo::from_ecef(*p);
                (lat.to_degrees(), lon.to_degrees())
            })
            .collect()
    })
}

// ---------------------------------------------------------------------------
// The run
// ---------------------------------------------------------------------------

/// The track as polylines in degrees — what the region measures against.
fn track_lines(net: &TrackNetwork) -> Vec<Vec<(f64, f64)>> {
    net.edges()
        .iter()
        .map(|edge| {
            let length = edge.length();
            let steps = (length / TRACK_STEP).ceil().max(1.0) as usize;
            (0..=steps)
                .map(|i| {
                    let s = length * i as f64 / steps as f64;
                    let (lat, lon, _) = geo::from_ecef(edge.eval(s).pos);
                    (lat.to_degrees(), lon.to_degrees())
                })
                .collect()
        })
        .collect()
}

/// The UTM zone the module sits in — the anchor where there is one, else the
/// first thing placed, else the middle of Germany.
fn zone_of(line: &Line) -> u8 {
    let lon = line
        .source
        .anchor
        .map(|a| a.lon)
        .or_else(|| {
            line.net
                .edges()
                .first()
                .map(|e| geo::from_ecef(e.eval(0.0).pos).1.to_degrees())
        })
        .unwrap_or(10.0);
    fields::land::utm_zone_at(lon)
}

/// `--detect-run`: the whole thing without a window and without a click.
///
/// The dialog is the way this is meant to be used — a builder reads the report
/// before anything is written, and that is the right order for a decision that
/// puts a few hundred objects into a module. This is for the other case: a
/// module being rebuilt from its sources, where the detection is one step of a
/// script beside `import-module`, and where nobody is sitting in front of it.
///
/// It runs the same [`plan`], the same [`run`] and the same [`commit`] as the
/// button does, prints what it found, and writes the line file back.
pub fn headless(
    line: &mut Line,
    state: &EditorState,
    objects: &TrackObjects,
    imagery: imagery::ImageryConfig,
    ai_path: &str,
    options: AiOptions,
) -> Result<String, String> {
    let (config, message) = VisionConfig::load_or_create(std::path::Path::new(ai_path));
    if let Some(message) = message {
        info!("{message}");
    }
    let mut options = options;
    if options.model.is_empty() {
        options.model = config.active.clone();
    }
    let config_dir = std::path::Path::new(ai_path)
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .to_path_buf();
    let plan = plan(&options, Some(&config), line, state, &config_dir)?;

    // The progress goes to the log rather than a bar: a run over a long module
    // is minutes of tile fetching and inference, and a script that prints
    // nothing looks hung. About twenty lines whatever the size of the run —
    // a fixed stride says nothing on a short module and floods a long one.
    let mut last = 0;
    let report = run(&plan, imagery, &mut |p| {
        let stride = (p.windows / 20).max(1);
        if p.window >= last + stride || p.window == p.windows {
            info!(
                "window {}/{} — {} found, {} tiles",
                p.window, p.windows, p.found, p.tiles
            );
            last = p.window;
        }
        true
    })?;

    let placed = commit(line, &report, &options, objects, true);
    let path = line
        .path
        .clone()
        .ok_or_else(|| "the line has no file to be written back to".to_string())?;
    std::fs::write(&path, header_of(&path) + &line.source.to_ron())
        .map_err(|e| format!("{path}: {e}"))?;
    line.dirty = false;

    let what: Vec<String> = placed
        .by_tag
        .iter()
        .map(|(tag, count)| format!("{count} × {tag}"))
        .collect();
    Ok(format!(
        "{} found, {} placed and {} planted ({}), {} car parks paved — written to {path}",
        report.outcome.found.len(),
        placed.objects,
        placed.trees,
        if what.is_empty() {
            "nothing installed to place them with".to_string()
        } else {
            what.join(", ")
        },
        placed.lots,
    ))
}

/// The file's leading comment block, which `to_ron` cannot carry.
///
/// The dialog asks before it drops a file's comments and lets you say no. A
/// headless run has nobody to ask, and the head of a module file is where this
/// project keeps its provenance: `boerde.ron` opens with the licences of the
/// field register, of OpenStreetMap and of the DGM it was built from, and a
/// script that quietly deleted those would be worse than one that never ran.
///
/// Only the head. Comments further into the file still go, exactly as they do
/// when the editor saves — carrying those would mean a RON writer that keeps
/// them, which is a different piece of work.
fn header_of(path: &str) -> String {
    let Ok(text) = std::fs::read_to_string(path) else {
        return String::new();
    };
    let head: Vec<&str> = text
        .lines()
        .take_while(|l| {
            let l = l.trim_start();
            l.starts_with("//") || l.is_empty()
        })
        .collect();
    if !head.iter().any(|l| l.trim_start().starts_with("//")) {
        return String::new();
    }
    // Trailing blank lines belong to what follows, not to the header.
    let end = head.iter().rposition(|l| !l.trim().is_empty()).unwrap_or(0);
    let mut out = head[..=end].join("\n");
    out.push('\n');
    out
}

/// Everything [`run`] needs, worked out from the line and the chosen options.
///
/// Split out of [`start`] so that the dialog and the headless `--detect-run`
/// settle on the area, the model and the rules by the same arithmetic. Two
/// copies of this would drift, and the one that drifted would be the one
/// nobody watches.
struct Plan {
    spec: vision::ModelSpec,
    weights: PathBuf,
    region: Region,
    /// Middle of the region \[deg\]: it is what decides how many metres a
    /// pixel of the imagery is worth, and so which zoom level is fetched.
    latitude: f64,
    zone: u8,
    min_lot_cars: usize,
}

fn plan(
    options: &AiOptions,
    config: Option<&VisionConfig>,
    line: &Line,
    state: &EditorState,
    config_dir: &std::path::Path,
) -> Result<Plan, String> {
    let config = config.ok_or_else(|| t!("ai-no-models"))?;
    let spec = config
        .model_by_id(&options.model)
        .cloned()
        .ok_or_else(|| t!("ai-no-models"))?;
    let weights = spec.path(config_dir);
    if !weights.is_file() {
        return Err(t!("ai-model-missing", file = weights.display()));
    }

    let zone = zone_of(line);
    let track = track_lines(&line.net);
    let shape = match options.area {
        Area::Corridor => Shape::Corridor {
            radius: options.corridor,
        },
        Area::Selection => Shape::Polygon(area_degrees(state).ok_or_else(|| t!("ai-area-none"))?),
    };
    if matches!(shape, Shape::Corridor { .. }) && track.is_empty() {
        return Err(t!("ai-no-track"));
    }
    let region =
        Region::new(shape, &track, options.keep_clear, zone).off_the_carriageway(&streets(line));
    let (_, south, _, north) = region.bounds().ok_or_else(|| t!("ai-area-none"))?;

    Ok(Plan {
        spec,
        weights,
        region,
        latitude: (south + north) / 2.0,
        zone,
        min_lot_cars: options.min_lot_cars,
    })
}

/// Starts a run on a thread of its own.
fn start(
    dialog: &mut AiImport,
    line: &Line,
    state: &EditorState,
    imagery: imagery::ImageryConfig,
    config_dir: PathBuf,
) {
    let plan = match plan(
        &dialog.options,
        dialog.config.as_ref(),
        line,
        state,
        &config_dir,
    ) {
        Ok(plan) => plan,
        Err(message) => {
            dialog.message = message;
            return;
        }
    };

    let (progress_out, progress) = std::sync::mpsc::channel();
    let (result_out, result) = std::sync::mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::spawn(move || {
        let result = run(&plan, imagery, &mut |p| {
            let _ = progress_out.send(p);
            !flag.load(Ordering::Relaxed)
        });
        let _ = result_out.send(result);
    });

    dialog.report = None;
    dialog.progress = None;
    dialog.message.clear();
    dialog.job = Some(Job {
        progress: Mutex::new(progress),
        result: Mutex::new(result),
        stop,
    });
}

/// The run itself, off the main thread: the model, the imagery, the walk.
fn run(
    plan: &Plan,
    imagery: imagery::ImageryConfig,
    progress: &mut dyn FnMut(Progress) -> bool,
) -> Result<Report, String> {
    let Plan {
        spec,
        weights,
        region,
        latitude,
        zone,
        min_lot_cars,
    } = plan;
    let mut source = imagery::BlockingSource::new(imagery);
    let (tile_size, min_zoom, max_zoom) = source
        .provider()
        .map(|p| (p.tile_size, p.min_zoom, p.max_zoom))
        .ok_or_else(|| t!("ai-no-imagery"))?;
    let zoom =
        vision::sheet::zoom_for(spec.ground_sample, *latitude, tile_size, min_zoom, max_zoom);
    let mut sheet = vision::Sheet::new(zoom, tile_size, TILE_CACHE, move |tile| source.tile(tile));

    let mut detector = vision::load_detector(spec, weights)?;

    let outcome = vision::run(&mut sheet, detector.as_mut(), spec, region, progress)?;
    let cars: Vec<GeoDetection> = outcome
        .found
        .iter()
        .filter(|d| d.place == CAR)
        .cloned()
        .collect();
    let lots = vision::lots(&cars, LOT_RADIUS, *min_lot_cars, *zone);
    Ok(Report { outcome, lots })
}

// ---------------------------------------------------------------------------
// Writing it into the line
// ---------------------------------------------------------------------------

/// Every installed object carrying a tag, with how much ground it covers.
fn tagged(objects: &TrackObjects, tag: &str) -> Vec<(String, Option<Footprint>)> {
    objects
        .map
        .iter()
        .filter(|(_, object)| object.tags.iter().any(|t| t == tag))
        .map(|(name, object)| (name.clone(), object.footprint))
        .collect()
}

/// A number from a place, stable across runs.
///
/// Which of six car models stands in a bay, and which way round it faces, has
/// to be the same every time the same imagery is read — otherwise re-running
/// a corridor after widening it reshuffles the cars a builder has already
/// looked at.
fn seed(lat: f64, lon: f64) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in lat
        .to_bits()
        .to_le_bytes()
        .iter()
        .chain(lon.to_bits().to_le_bytes().iter())
    {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// How much longer than the space a model may be and still be put in it.
///
/// A box measured off a satellite photograph is not a survey: the outline of a
/// car at fifteen centimetres a pixel is a few pixels of guesswork at each
/// end, and the model that drew it was trained to find vehicles, not to
/// measure them. Half a metre is about what that costs.
const SLACK: f64 = 0.5;

/// How much smaller than the space a model may be before it looks lost in it.
///
/// The counterpart to [`SLACK`], and the reason it is a share rather than a
/// distance: three metres spare in a bay meant for a lorry is a different
/// thing from three metres spare in one meant for a car.
const FILLS: f64 = 0.6;

/// Which of the objects carrying the class's tag to put on a find.
///
/// The tag alone stopped being enough as soon as a tag held more than one size
/// of thing. `lorry` on these mods is a 4.82 m Transporter *and* a 6.30 m
/// Sprinter, and taking either at random puts the Sprinter across the aisle in
/// half the bays it lands in — a van standing a metre and a half out of a
/// marked space is the first thing anybody notices in a car park.
///
/// **Length only.** The width of a box measured off imagery is its minor axis,
/// which is the noisier of the two and which nothing in a car park depends on
/// anyway: what decides whether a van fits a bay is how far down the bay it
/// reaches.
///
/// **Still at random among what suits.** Taking the longest that fits would be
/// worse than taking any: an ordinary car reads as four and a half metres give
/// or take, every model in the `car` tag clears that, and a car park would
/// come out as row upon row of the single largest estate. The pick stays the
/// one it always was — the place decides, so the same imagery always draws the
/// same car — over a list that no longer holds anything the wrong size.
///
/// The two ways nothing suits are not the same and are not answered the same
/// way. Where everything is too *long*, the shortest goes in, because whatever
/// goes in will overhang and it should overhang least. Where everything is too
/// *short* — a twelve-metre lorry with nothing but vans installed — the
/// longest goes in, for the same reason read the other way. In both the
/// detection saw a vehicle there, and the length it guessed at is the less
/// trustworthy half of what it said.
///
/// An object without a footprint is not measured and stays eligible
/// throughout. Most scenery has none, and a mod that never states its sizes
/// should behave as it did before there was anywhere to state them.
///
/// `band` is how big the thing that goes there may be, and it is the one part
/// of this that differs between a car park and a wood — see [`band_for`].
fn fitting(
    candidates: &[(String, Option<Footprint>)],
    band: (f64, f64),
    seed: u64,
) -> Option<&str> {
    let length = |c: &(String, Option<Footprint>)| c.1.map_or(f64::NAN, |f| f.length);
    let unmeasured = |c: &&(String, Option<Footprint>)| c.1.is_none();
    let (floor, room) = band;

    let suits: Vec<&(String, Option<Footprint>)> = candidates
        .iter()
        .filter(|c| unmeasured(c) || (length(c) <= room && length(c) >= floor))
        .collect();
    if !suits.is_empty() {
        return Some(suits[(seed as usize) % suits.len()].0.as_str());
    }

    // Past that point every candidate carries a footprint: an object without
    // one is in `suits` by definition, so reaching here means there were none.
    let fits: Vec<&(String, Option<Footprint>)> =
        candidates.iter().filter(|c| length(c) <= room).collect();
    // Everything is too short for the space: the longest of them fills it best.
    if !fits.is_empty() {
        return fits
            .iter()
            .max_by(|a, b| length(a).total_cmp(&length(b)))
            .map(|c| c.0.as_str());
    }
    // Everything is too long for it: the shortest overhangs least.
    candidates
        .iter()
        .min_by(|a, b| length(a).total_cmp(&length(b)))
        .map(|(name, _)| name.as_str())
}

/// How big a thing may be to go on this find.
///
/// The two kinds are measured against different things and the difference is
/// real. A bay is a *space*, and what goes in it must not stick out of it:
/// the room is what was measured plus the slack the measurement is worth, and
/// nothing may be so much smaller that it looks lost. A crown is not a space
/// but the thing itself — the tree that goes there should be about that wide,
/// too big and too small are the same error, and the range is symmetrical
/// because a wood is.
fn band_for(detection: &GeoDetection) -> (f64, f64) {
    match detection.kind {
        Placement::Object => (detection.length * FILLS, detection.length + SLACK),
        Placement::Tree => (detection.length / SPREAD, detection.length * SPREAD),
    }
}

/// How much bigger or smaller than its own size a tree is planted, so that it
/// stands as wide as the crown in the photograph — see [`GROWTH`].
///
/// A species that states no crown is planted at its own size: `mods/trees`
/// states one for every tree it has, and a mod that states none is saying it
/// does not know, not that its trees are a metre across.
fn grown(crown: f64, model: Option<Footprint>) -> f64 {
    match model {
        Some(model) if model.length > 0.1 => (crown / model.length).clamp(GROWTH.0, GROWTH.1),
        _ => 1.0,
    }
}

/// One find as a row in the line's own tree list.
///
/// No track reference and no heading. A tree stands at a place on the ground,
/// and which way round it stands is a matter of taste — but not of chance: the
/// turn is drawn from the place, so the same imagery always plants the same
/// wood, and re-running a corridor after widening it does not spin every tree
/// a builder has already looked at.
fn tree_for(detection: &GeoDetection, object: &str, scale: f64, seed: u64) -> TreeSource {
    TreeSource {
        object: object.to_string(),
        lat: detection.lat,
        lon: detection.lon,
        // A different part of the hash than the species pick uses, so that the
        // two do not move together — a wood in which every lime faces north
        // and every fir south is a wood nobody believes.
        yaw_deg: (seed >> 32) as f64 % 360.0,
        scale,
    }
}

/// The size a tree is planted at where nothing measured it: seven tenths to
/// one and three tenths of the model's own, which is the spread the forest
/// brush bakes a wood with.
fn natural(seed: u64) -> f64 {
    0.7 + ((seed >> 16) & 0xffff) as f64 / 65_535.0 * 0.6
}

/// Where something already stands, in cells big enough that only the
/// neighbouring ones have to be looked at.
///
/// The list this replaced was walked in full for every find, which a car park
/// never noticed: fifty cars against a hundred objects is nothing. A wood is
/// tens of thousands of rows and a run over a forested corridor finds
/// thousands more, and the same code would be a hundred million distance tests
/// on the main thread with the editor frozen in front of the user in the
/// middle of a click.
///
/// ECEF throughout, and every point put in at the same height: the cells are
/// then a metric box grid, the distance is the ordinary one, and a tree read
/// off a photograph can be compared with a tree already in the file without
/// either of them having to know how far below the rails the ellipsoid is.
struct Occupied {
    /// The distance the grid answers about, and the size of a cell — the two
    /// are the same number on purpose, because that is what makes the
    /// neighbouring twenty-seven cells the whole of the search.
    within: f64,
    at: std::collections::HashMap<(i64, i64, i64), Vec<EcefPos>>,
}

impl Occupied {
    fn new(within: f64) -> Self {
        Self {
            within: within.max(0.5),
            at: Default::default(),
        }
    }

    fn key(&self, p: EcefPos) -> (i64, i64, i64) {
        (
            (p.0.x / self.within).floor() as i64,
            (p.0.y / self.within).floor() as i64,
            (p.0.z / self.within).floor() as i64,
        )
    }

    fn add(&mut self, p: EcefPos) {
        self.at.entry(self.key(p)).or_default().push(p);
    }

    fn taken(&self, p: EcefPos) -> bool {
        let (x, y, z) = self.key(p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let Some(cell) = self.at.get(&(x + dx, y + dy, z + dz)) else {
                        continue;
                    };
                    if cell.iter().any(|q| q.distance(p) < self.within) {
                        return true;
                    }
                }
            }
        }
        false
    }
}

/// The compass bearing of the track at a pose \[deg\], from north through
/// east — the same convention `crates/vision` gives a find its heading in, so
/// the two can be subtracted.
fn track_bearing(pose: &track_model::TrackPose) -> f64 {
    let frame = EnuFrame::at(pose.pos);
    let east = pose.tangent.dot(frame.east);
    let north = pose.tangent.dot(frame.north);
    east.atan2(north).to_degrees().rem_euclid(360.0)
}

fn object_for(
    line: &Line,
    detection: &GeoDetection,
    name: String,
    snap: bool,
) -> Option<ObjectSource> {
    let flat = geo::to_ecef_deg(detection.lat, detection.lon, 0.0);
    let (edge_index, start, _) = crate::tools::nearest_on_network(&line.net, flat)?;
    let edge = line.net.edges().get(edge_index)?;

    // Where along the track the find sits, worked out **at the height of the
    // rails**, and worked out until nothing is left over.
    //
    // Two things make that necessary, and together they were the offset
    // anybody could see. `nearest_on_network` measures in three dimensions
    // from a probe on the ellipsoid, which under a German module is some
    // hundred and forty metres below the track; the foot of that perpendicular
    // is not the foot of the one from the same point lifted to the rails, and
    // the two differ by about the drop times the gradient. And an object is
    // rebuilt from its arc length and its lateral offset alone, so whatever is
    // left *along* the track is not merely inaccurate, it is discarded.
    //
    // On the Börde's own grades, which swing from −12 to +16 per mille over
    // five kilometres, that came to between half a metre and one and three
    // quarters — a third of a car's length, ahead of the photograph beside a
    // climb and behind it beside a fall, which is exactly the "sometimes"
    // in a fault that looks intermittent and is not.
    //
    // Four passes is generous: the coupling between the two is weak, and it
    // is a centimetre after two.
    let mut s = start;
    let mut pose = edge.eval(s.clamp(0.0, edge.length()));
    let mut at = flat;
    for _ in 0..4 {
        let (_, _, height) = geo::from_ecef(pose.pos);
        at = geo::to_ecef(
            detection.lat.to_radians(),
            detection.lon.to_radians(),
            height,
        );
        let along = (at.0 - pose.pos.0).dot(pose.tangent);
        if along.abs() < 0.01 {
            break;
        }
        s = (s + along).clamp(0.0, edge.length());
        pose = edge.eval(s);
    }

    let right = pose.tangent.cross(pose.up).normalize_or_zero();
    let lateral_offset = (at.0 - pose.pos.0).dot(right);

    // A photograph cannot say which end of a parked car is the front, so the
    // choice is made once and always the same way for the same place.
    let flip = seed(detection.lat, detection.lon) % 2 == 1;
    let heading = detection.heading + if flip { 180.0 } else { 0.0 };
    Some(ObjectSource {
        object: name,
        edge: edge_index as u32,
        s,
        lateral_offset,
        yaw_deg: (heading - track_bearing(&pose)).rem_euclid(360.0),
        height: 0.0,
        snap_to_terrain: snap,
    })
}

/// The module's streets, as centre line and width, for the rule that keeps
/// finds off them.
///
/// A car park this dialog paved on an earlier run is a road in the file like
/// any other, and it is left out: it carries `parking`, it is the one place a
/// car most belongs, and taking it for a street would mean a second run over
/// the same ground could no longer fill the bays it had just found.
fn streets(line: &Line) -> Vec<(Vec<(f64, f64)>, f64)> {
    line.source
        .roads
        .iter()
        .filter(|road| !road.tags.iter().any(|t| t == LOT_TAG))
        .map(|road| {
            (
                road.points.iter().map(|p| (p.lat, p.lon)).collect(),
                road.width,
            )
        })
        .collect()
}

/// A recognised car park as a paved ribbon.
///
/// A car park is a rectangle of tarmac, and a road *is* a rectangle of tarmac
/// where it is straight — so the ribbon runs along the rows, as wide as the
/// rows are deep, with no centre line and no edge lines. It needs no new file
/// format, no new mesh and no new tool, and a builder can drag its ends
/// afterwards like any other road.
fn road_for(lot: &Lot) -> RoadSource {
    let point = |(lat, lon): (f64, f64)| RoadPoint { lat, lon };
    RoadSource {
        name: String::new(),
        points: vec![point(lot.line.0), point(lot.line.1)],
        width: lot.width,
        surface: RoadSurface::Asphalt,
        center_line: CenterLine::None,
        edge_lines: false,
        bridge: false,
        tags: LOT_TAGS.map(String::from).to_vec(),
    }
}

/// What a commit did — for the status line, for the log of a headless run,
/// and for the report the user reads before deciding.
#[derive(Default)]
pub struct Placed {
    /// How many went in per tag. Objects and trees together: the tag is what
    /// the model named and what the user recognises them by, and whether a
    /// `laubbaum` became a row in one list or the other is this module's
    /// business rather than the reader's.
    pub by_tag: std::collections::BTreeMap<String, usize>,
    pub objects: usize,
    pub trees: usize,
    pub lots: usize,
}

/// Every installed object carrying a tag, worked out once per tag and kept.
///
/// `laubbaum` on the shipped mods is sixty entries and a run finds thousands
/// of crowns; scanning the catalogue for each of them is the same answer
/// several thousand times.
#[derive(Default)]
struct Pool(std::collections::BTreeMap<String, Vec<(String, Option<Footprint>)>>);

impl Pool {
    fn of(&mut self, objects: &TrackObjects, tag: &str) -> &[(String, Option<Footprint>)] {
        self.0
            .entry(tag.to_string())
            .or_insert_with(|| tagged(objects, tag))
    }
}

/// Writes the report into the line.
fn commit(
    line: &mut Line,
    report: &Report,
    options: &AiOptions,
    objects: &TrackObjects,
    snap: bool,
) -> Placed {
    let mut placed = Placed::default();
    let mut pool = Pool::default();

    if options.place_objects {
        // Where something already stands, nothing is added: a second run over
        // ground that has been done is then a no-op rather than a doubling.
        // What this run places counts as standing too, from the moment it is
        // decided on.
        let mut taken = Occupied::new(OCCUPIED);
        for object in &line.source.objects {
            if let Some(p) = crate::tools::object_pos(&line.net, object) {
                taken.add(p);
            }
        }
        let mut fresh = Vec::new();
        for detection in report.outcome.found.iter().filter(|d| is_object(d)) {
            let names = pool.of(objects, &detection.place);
            if names.is_empty() {
                continue;
            }
            let Some(name) = fitting(
                names,
                band_for(detection),
                seed(detection.lat, detection.lon),
            ) else {
                continue;
            };
            let Some(object) = object_for(line, detection, name.to_string(), snap) else {
                continue;
            };
            let Some(at) = crate::tools::object_pos(&line.net, &object) else {
                continue;
            };
            if taken.taken(at) {
                continue;
            }
            taken.add(at);
            *placed.by_tag.entry(detection.place.clone()).or_insert(0) += 1;
            placed.objects += 1;
            fresh.push(object);
        }
        line.source.objects.extend(fresh);
    }

    if options.place_trees {
        // The same rule as the objects', measured against the trees that are
        // already there — including the ones this loop plants, so a stand of
        // crowns the model reported twice does not become two woods in one.
        let spacing = options.tree_spacing.max(0.0);
        let mut standing = Occupied::new(spacing);
        if spacing > 0.0 {
            for tree in &line.source.trees {
                standing.add(geo::to_ecef_deg(tree.lat, tree.lon, 0.0));
            }
        }
        let mut fresh = Vec::new();
        for detection in report.outcome.found.iter().filter(|d| !is_object(d)) {
            // A wood is cut to the module's own outline, exactly as the forest
            // brush's bake is: the run's area is drawn on the map by hand and
            // can reach over the edge of what this module portrays.
            if !line.source.envelope_contains(detection.lat, detection.lon) {
                continue;
            }
            let at = geo::to_ecef_deg(detection.lat, detection.lon, 0.0);
            if spacing > 0.0 && standing.taken(at) {
                continue;
            }
            let tag = match &options.species {
                Some(stand) => format!("{}{stand}", crate::STAND_TAG),
                None => detection.place.clone(),
            };
            let species = pool.of(objects, &tag);
            if species.is_empty() {
                continue;
            }
            let seed = seed(detection.lat, detection.lon);
            // Which tree goes here and how big, and there are two answers
            // because there are two people being believed.
            //
            // Where the species come from the **model**, the crown decides
            // both: the member whose own crown suits the one that was
            // measured, grown the rest of the way. That is the whole point of
            // measuring it.
            //
            // Where a **builder has named a stand**, they have overruled the
            // photograph about what the wood is — and the size has to go with
            // it. A crown detector reading a provider's imagery reports the
            // sunlit top of a young spruce as two metres across; planting a
            // spruce wood at that would be a wood of saplings, and picking
            // the member that suits two metres would be a wood of junipers.
            // So a named stand is planted the way the forest brush plants
            // one: any of its members, at its own size, give or take a third.
            let (name, scale) = match &options.species {
                Some(_) => {
                    let pick = species[(seed as usize) % species.len()].0.as_str();
                    (pick, natural(seed))
                }
                None => {
                    let Some(name) = fitting(species, band_for(detection), seed) else {
                        continue;
                    };
                    let crown = species
                        .iter()
                        .find(|(candidate, _)| candidate == name)
                        .and_then(|(_, footprint)| *footprint);
                    (name, grown(detection.length, crown))
                }
            };
            // Only now: a crown whose tag no mod carries plants nothing, and
            // must not hold the ground against the tree of another tag beside
            // it either.
            if spacing > 0.0 {
                standing.add(at);
            }
            *placed.by_tag.entry(tag).or_insert(0) += 1;
            placed.trees += 1;
            fresh.push(tree_for(detection, name, scale, seed));
        }
        line.source.trees.extend(fresh);
    }

    if options.pave_lots {
        for lot in &report.lots {
            line.source.roads.push(road_for(lot));
            placed.lots += 1;
        }
    }
    placed
}

/// Whether a find is placed against the track rather than planted on it.
fn is_object(detection: &GeoDetection) -> bool {
    detection.kind == Placement::Object
}

// ---------------------------------------------------------------------------
// The dialog
// ---------------------------------------------------------------------------

/// Its own system, like the other import dialogs — `ui::draw` is already at
/// Bevy's system-parameter limit.
pub fn draw(
    mut contexts: EguiContexts,
    mut dialog: ResMut<AiImport>,
    mut line: ResMut<Line>,
    mut state: ResMut<EditorState>,
    mut overlay: ResMut<crate::overlay::Overlay>,
    mut request: ResMut<crate::Request>,
    // Grouped so the system stays inside Bevy's parameter count.
    (objects, ai_path, mut themed): (Res<TrackObjects>, Res<AiPath>, Local<bool>),
) -> Result {
    // `ui::draw` installs the theme on the very first pass and draws nothing
    // itself; the font families it registers are only bound from the next one.
    // A dialog that is up on that first pass — which `--detect` makes it — has
    // to sit that pass out as well: a heading in a family that is not there
    // yet is a panic inside egui, not a fallback.
    if !*themed {
        *themed = true;
        return Ok(());
    }
    if request.detect_imagery {
        request.detect_imagery = false;
        dialog.open = true;
        // An area drawn on the map is an answer to "where" already given. A
        // builder who has just ringed a copse and opened this should not have
        // to say so a second time in a radio button — and the corridor is one
        // click away for whoever wanted that instead.
        if state.ai_area.as_ref().is_some_and(|a| a.len() >= 3) {
            dialog.options.area = Area::Selection;
        }
    }
    if !dialog.open {
        return Ok(());
    }
    // The registry is read the first time the dialog is opened, not at start:
    // an editor session that never asks for a model never writes `ai.ron`.
    if dialog.config.is_none() {
        let (config, message) = VisionConfig::load_or_create(&ai_path.0);
        if let Some(message) = message {
            overlay.status = message;
        }
        if dialog.options.model.is_empty() {
            dialog.options.model = config.model().map(|m| m.id.clone()).unwrap_or_default();
        }
        dialog.config = Some(config);
    }
    let ctx = contexts.ctx_mut()?.clone();

    // Whatever the thread has said since the last frame; the channels sit
    // behind mutexes, so the reads are copies.
    if dialog.job.is_some() {
        let mut finished: Option<Result<Report, String>> = None;
        let mut failed = false;
        let mut progress = dialog.progress;
        if let Some(job) = &dialog.job {
            if let Ok(channel) = job.progress.lock() {
                while let Ok(next) = channel.try_recv() {
                    progress = Some(next);
                }
            }
            if let Ok(channel) = job.result.lock() {
                match channel.try_recv() {
                    Ok(report) => finished = Some(report),
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => failed = true,
                }
            } else {
                failed = true;
            }
        }
        dialog.progress = progress;
        if let Some(finished) = finished {
            dialog.job = None;
            match finished {
                Ok(report) => dialog.report = Some(report),
                Err(e) => dialog.message = e,
            }
        } else if failed {
            dialog.job = None;
            dialog.message = t!("ai-failed");
        }
    }

    let config_dir = PathBuf::from(&ai_path.0)
        .parent()
        .map(PathBuf::from)
        .unwrap_or_default();
    let mut close = false;
    let mut begin = false;
    let mut draw_area = false;
    egui::Window::new(t!("ai-title"))
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.viewport_rect().center())
        .show(&ctx, |ui| {
            ui.set_width(DIALOG);
            let dialog: &mut AiImport = &mut dialog;
            if dialog.job.is_some() {
                running(ui, dialog);
            } else if dialog.report.is_some() {
                close |= finished_panel(ui, dialog, &mut line, &mut state, &mut overlay, &objects);
            } else {
                let answer = settings(ui, dialog, &line, &state, &config_dir, &objects);
                close |= answer.close;
                begin = answer.start;
                draw_area = answer.draw_area;
            }
        });
    if begin {
        let imagery = overlay.source.config().clone();
        start(&mut dialog, &line, &state, imagery, config_dir);
    }
    if draw_area {
        crate::tools::select_tool(&mut state, crate::tools::Tool::AiArea);
        state.ai_area = None;
        overlay.status = t!("status-ai-area-draw");
        close = true;
    }
    if close {
        dialog.open = false;
        dialog.report = None;
        dialog.message.clear();
    }
    Ok(())
}

/// What the settings panel decided this frame.
#[derive(Default)]
struct Answer {
    close: bool,
    start: bool,
    draw_area: bool,
}

/// The form: which model, where, and what to do with what it finds.
fn settings(
    ui: &mut egui::Ui,
    dialog: &mut AiImport,
    line: &Line,
    state: &EditorState,
    config_dir: &std::path::Path,
    objects: &TrackObjects,
) -> Answer {
    let mut answer = Answer::default();
    ui.label(t!("ai-intro"));
    ui.add_space(space::S);

    let models = dialog
        .config
        .as_ref()
        .map(|c| c.models.clone())
        .unwrap_or_default();
    let chosen = models
        .iter()
        .find(|m| m.id == dialog.options.model)
        .cloned();
    // Only what this model can do. A crown detector has no car parks to pave
    // and a car model has nothing to plant, and a row that decides nothing is
    // a question the user has to answer for no reason.
    let plants = chosen
        .as_ref()
        .is_some_and(|m| m.placing().any(|(_, class)| class.is_tree()));
    let places = chosen
        .as_ref()
        .is_none_or(|m| m.placing().any(|(_, class)| !class.is_tree()))
        || !plants;

    editor_ui::form_grid("ai-form")
        .num_columns(2)
        .show(ui, |ui| {
            crate::ui::row(ui, "ai-model", |ui| {
                let label = chosen
                    .as_ref()
                    .map(|m| m.name.clone())
                    .unwrap_or_else(|| t!("ai-no-models"));
                egui::ComboBox::from_id_salt("ai-model-pick")
                    .selected_text(label)
                    .width(space::FIELD * 2.0)
                    .show_ui(ui, |ui| {
                        for model in &models {
                            ui.selectable_value(
                                &mut dialog.options.model,
                                model.id.clone(),
                                &model.name,
                            );
                        }
                    });
            });
            crate::ui::row(ui, "ai-area", |ui| {
                ui.selectable_value(
                    &mut dialog.options.area,
                    Area::Corridor,
                    t!("ai-area-track"),
                );
                ui.selectable_value(
                    &mut dialog.options.area,
                    Area::Selection,
                    t!("ai-area-drawn"),
                );
            });
            match dialog.options.area {
                Area::Corridor => crate::ui::row(ui, "ai-corridor", |ui| {
                    editor_ui::field(ui, &mut dialog.options.corridor, 1.0, 10.0..=2_000.0, "m");
                }),
                Area::Selection => crate::ui::row(ui, "ai-drawn", |ui| {
                    match state.ai_area.as_ref().map(Vec::len) {
                        Some(corners) if corners >= 3 => {
                            ui.label(t!("ai-drawn-corners", corners = corners));
                        }
                        _ => {
                            ui.colored_label(colors::WARN, t!("ai-area-none"));
                        }
                    }
                    if ui.button(t!("ai-draw-area")).clicked() {
                        answer.draw_area = true;
                    }
                }),
            }
            crate::ui::row(ui, "ai-keep-clear", |ui| {
                editor_ui::field(ui, &mut dialog.options.keep_clear, 0.5, 0.0..=200.0, "m");
            });
            if places {
                crate::ui::row(ui, "ai-place-objects", |ui| {
                    ui.checkbox(&mut dialog.options.place_objects, "");
                });
            }
            if plants {
                crate::ui::row(ui, "ai-place-trees", |ui| {
                    ui.checkbox(&mut dialog.options.place_trees, "");
                });
                if dialog.options.place_trees {
                    // What the crowns become. The model finds where a tree is
                    // and how wide; which tree it is, is the one thing a
                    // photograph at this resolution cannot settle, so the
                    // stands of the installed mods are offered beside it.
                    crate::ui::row(ui, "ai-species", |ui| {
                        let stands = objects.stands();
                        let label = match &dialog.options.species {
                            Some(stand) => stand.clone(),
                            None => t!("ai-species-detected"),
                        };
                        egui::ComboBox::from_id_salt("ai-species-pick")
                            .selected_text(label)
                            .width(space::FIELD * 2.0)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut dialog.options.species,
                                    None,
                                    t!("ai-species-detected"),
                                );
                                for (stand, members) in &stands {
                                    ui.selectable_value(
                                        &mut dialog.options.species,
                                        Some(stand.clone()),
                                        format!("{stand} ({})", members.len()),
                                    );
                                }
                            });
                    });
                    crate::ui::row(ui, "ai-tree-spacing", |ui| {
                        editor_ui::field(
                            ui,
                            &mut dialog.options.tree_spacing,
                            0.5,
                            0.0..=50.0,
                            "m",
                        );
                    });
                }
            }
            if places {
                crate::ui::row(ui, "ai-pave-lots", |ui| {
                    ui.checkbox(&mut dialog.options.pave_lots, "");
                });
                if dialog.options.pave_lots {
                    crate::ui::row(ui, "ai-min-lot-cars", |ui| {
                        let mut cars = dialog.options.min_lot_cars as f64;
                        editor_ui::field(ui, &mut cars, 1.0, 3.0..=60.0, "");
                        dialog.options.min_lot_cars = cars.round() as usize;
                    });
                }
            }
        });

    // What the model will do on this module, said before it is started.
    ui.add_space(space::S);
    if let Some(model) = &chosen {
        // Both tags of a tree class. What a crown detector plants is a
        // broadleaf *or* a fir depending on how the crown reads, and a line
        // that named only the first would be telling half the truth about
        // which mods have to be installed for the run to do anything.
        let mut tags: Vec<String> = Vec::new();
        for (_, class) in model.placing() {
            for tag in [&class.place, &class.conifer] {
                if !tag.is_empty() && !tags.contains(tag) {
                    tags.push(tag.clone());
                }
            }
        }
        ui.small(t!("ai-model-places", tags = tags.join(", ")));
        if !model.note.is_empty() {
            ui.small(&model.note);
        }
        if model.missing(config_dir) {
            ui.colored_label(
                colors::WARN,
                t!("ai-model-missing", file = model.path(config_dir).display()),
            );
        }
    }
    if vision::backend() == vision::Backend::Missing {
        ui.colored_label(colors::ERROR, t!("ai-no-backend"));
    }
    if !dialog.message.is_empty() {
        ui.add_space(space::S);
        ui.colored_label(colors::ERROR, &dialog.message);
    }

    ui.add_space(space::M);
    let ready = chosen.as_ref().is_some_and(|m| !m.missing(config_dir))
        && vision::backend() != vision::Backend::Missing
        && match dialog.options.area {
            Area::Corridor => !line.net.edges().is_empty(),
            Area::Selection => state.ai_area.as_ref().is_some_and(|a| a.len() >= 3),
        };
    ui.horizontal(|ui| {
        let start = ui.add_enabled(ready, egui::Button::new(t!("ai-start")));
        if !ready {
            start.clone().on_disabled_hover_text(t!("ai-not-ready"));
        }
        answer.start = start.clicked();
        answer.close = ui.button(t!("action-cancel")).clicked();
    });
    answer
}

/// While it runs: how far along, and Stop.
fn running(ui: &mut egui::Ui, dialog: &mut AiImport) {
    let share = dialog
        .progress
        .filter(|p| p.windows > 0)
        .map(|p| p.window as f32 / p.windows as f32);
    let bar = match share {
        Some(share) => egui::ProgressBar::new(share).show_percentage(),
        None => egui::ProgressBar::new(0.0).animate(true),
    };
    ui.add(bar.desired_width(DIALOG));
    ui.add_space(space::XS);
    match dialog.progress {
        Some(p) => ui.label(t!(
            "ai-running",
            window = p.window + 1,
            windows = p.windows,
            found = p.found,
            tiles = p.tiles
        )),
        None => ui.label(t!("ai-loading")),
    };
    ui.add_space(space::M);
    if let Some(job) = &dialog.job
        && ui.button(t!("field-import-stop")).clicked()
    {
        job.stop.store(true, Ordering::Relaxed);
    }
}

/// The summary, and the decision.
fn finished_panel(
    ui: &mut egui::Ui,
    dialog: &mut AiImport,
    line: &mut Line,
    state: &mut EditorState,
    overlay: &mut crate::overlay::Overlay,
    objects: &TrackObjects,
) -> bool {
    let Some(report) = &dialog.report else {
        return false;
    };
    let mut counts: std::collections::BTreeMap<&str, usize> = Default::default();
    for detection in &report.outcome.found {
        *counts.entry(detection.place.as_str()).or_insert(0) += 1;
    }
    ui.label(
        egui::RichText::new(t!("ai-found", found = report.outcome.found.len()))
            .color(colors::TEXT_STRONG),
    );
    if !report.lots.is_empty() {
        ui.label(t!("ai-found-lots", lots = report.lots.len()));
    }
    ui.small(t!(
        "ai-cost",
        windows = report.outcome.windows,
        tiles = report.outcome.tiles
    ));
    // The one failure that looks like a result: the walk happened and no
    // window carried a picture. Offline mode with a cold cache does it, and so
    // does a provider without coverage of this corner of the country.
    if report.outcome.blank > 0 {
        ui.colored_label(colors::WARN, t!("ai-blank", blank = report.outcome.blank));
    }
    // The rule that keeps finds out of the running lanes can only hold to the
    // roads that are in the file. Without them a car park and a carriageway
    // are the same grey rectangle in a photograph, and the run says so rather
    // than quietly parking a lorry in the fast lane. Nothing to say on a run
    // that only planted trees: a tree is not put in a lane by a missing road,
    // it is kept out of one by the crown not being there in the photograph.
    if report.outcome.found.iter().any(is_object) && streets(line).is_empty() {
        ui.colored_label(colors::WARN, t!("ai-no-roads"));
    }

    ui.add_space(space::S);
    egui::ScrollArea::vertical()
        .max_height(160.0)
        .show(ui, |ui| {
            editor_ui::form_grid("ai-classes")
                .num_columns(3)
                .min_col_width(0.0)
                .show(ui, |ui| {
                    for (tag, count) in &counts {
                        let installed = tagged(objects, tag).len();
                        ui.label(
                            egui::RichText::new(count.to_string()).color(colors::TEXT_SECONDARY),
                        );
                        ui.label(*tag);
                        if installed == 0 {
                            ui.colored_label(colors::WARN, t!("ai-no-objects", tag = *tag));
                        } else {
                            ui.small(t!("ai-objects-installed", count = installed));
                        }
                        ui.end_row();
                    }
                });
        });

    ui.add_space(space::M);
    let anything = !report.outcome.found.is_empty() || !report.lots.is_empty();
    let (mut close, mut apply, mut again) = (false, false, false);
    ui.horizontal(|ui| {
        apply = ui
            .add_enabled(anything, egui::Button::new(t!("field-import-commit")))
            .clicked();
        again = ui.button(t!("field-import-again")).clicked();
        close = ui.button(t!("action-cancel")).clicked();
    });
    if apply {
        let report = dialog.report.take().expect("checked above");
        let snap = state.place_snap_to_terrain;
        let placed = commit(line, &report, &dialog.options, objects, snap);
        overlay.status = t!(
            "status-ai-placed",
            objects = placed.objects,
            trees = placed.trees,
            lots = placed.lots
        );
        line.dirty = true;
        line.needs_rebuild = true;
        // The indices of everything below the new rows are unchanged, but a
        // multi-selection made before a run means something else after one —
        // and a wood of two thousand fresh trees is not what anybody wants
        // held when they next press Delete.
        state.selection = Selection::None;
        state.marked.clear();
        return true;
    }
    if again {
        dialog.report = None;
    }
    close
}

#[cfg(test)]
mod tests {
    use super::*;
    use content::LineSource;
    use content::route::{EdgeSource, EdgeStart, GeoPoint, NodeSource, STARTER_TRACK_TYPE};
    use track_model::Segment;

    fn detection(lat: f64, lon: f64, heading: f64) -> GeoDetection {
        GeoDetection {
            class: 0,
            place: "car".into(),
            kind: Placement::Object,
            score: 0.9,
            lat,
            lon,
            length: 4.4,
            width: 1.8,
            heading,
        }
    }

    fn van(name: &str, length: f64) -> (String, Option<Footprint>) {
        (name.into(), Some(Footprint { length, width: 2.3 }))
    }

    /// The Sprinter is 6.30 m and the Transporter 4.82 m, and both answer to
    /// `lorry`. A find five metres long has room for one of them.
    #[test]
    fn a_van_too_long_for_the_space_is_not_put_in_it() {
        let pool = vec![van("cars:kastenwagen", 6.30), van("cars:transporter", 4.82)];
        let mut found = detection(51.0, 7.0, 0.0);
        found.length = 5.0;
        // Every place, not one: the pick is by position, and a rule that only
        // holds for some positions is not a rule.
        for step in 0..64 {
            let seeded = seed(51.0 + step as f64 * 1e-4, 7.0);
            assert_eq!(
                fitting(&pool, band_for(&found), seeded),
                Some("cars:transporter"),
                "a 6.30 m van in a 5.00 m space",
            );
        }
    }

    #[test]
    fn a_space_that_holds_the_long_one_may_draw_either() {
        let pool = vec![van("cars:kastenwagen", 6.30), van("cars:transporter", 4.82)];
        let mut found = detection(51.0, 7.0, 0.0);
        found.length = 6.4;
        let drawn: std::collections::BTreeSet<&str> = (0..64)
            .filter_map(|step| {
                fitting(
                    &pool,
                    band_for(&found),
                    seed(51.0 + step as f64 * 1e-4, 7.0),
                )
            })
            .collect();
        assert_eq!(drawn.len(), 2, "both fit, so both should turn up");
    }

    /// Otherwise a car park is a row of the same estate: an ordinary car reads
    /// as about four and a half metres, which every model in the `car` tag
    /// clears, so taking the longest that fits would take the same one always.
    #[test]
    fn among_what_fits_the_choice_is_still_the_places_own() {
        let pool = vec![
            van("cars:kleinwagen", 3.93),
            van("cars:kompaktwagen", 4.28),
            van("cars:gelaendewagen", 4.61),
        ];
        let mut found = detection(51.0, 7.0, 0.0);
        found.length = 4.5;
        let drawn: std::collections::BTreeSet<&str> = (0..64)
            .filter_map(|step| {
                fitting(
                    &pool,
                    band_for(&found),
                    seed(51.0 + step as f64 * 1e-4, 7.0),
                )
            })
            .collect();
        assert_eq!(drawn.len(), 3, "all three fit within the slack");
        // And the same place twice is the same car.
        let once = fitting(&pool, band_for(&found), seed(51.0, 7.0));
        assert_eq!(once, fitting(&pool, band_for(&found), seed(51.0, 7.0)));
    }

    #[test]
    fn an_object_that_states_no_size_is_never_ruled_out() {
        let pool = vec![van("cars:kastenwagen", 6.30), ("mod:hut".into(), None)];
        let mut found = detection(51.0, 7.0, 0.0);
        found.length = 2.0;
        let drawn: std::collections::BTreeSet<&str> = (0..64)
            .filter_map(|step| {
                fitting(
                    &pool,
                    band_for(&found),
                    seed(51.0 + step as f64 * 1e-4, 7.0),
                )
            })
            .collect();
        assert_eq!(drawn, ["mod:hut"].into_iter().collect());
    }

    /// The detection saw *something* there; the length it guessed at is the
    /// less trustworthy half of what it said.
    #[test]
    fn where_nothing_fits_the_shortest_goes_in() {
        let pool = vec![van("cars:kastenwagen", 6.30), van("cars:transporter", 4.82)];
        let mut found = detection(51.0, 7.0, 0.0);
        found.length = 2.0;
        assert_eq!(
            fitting(&pool, band_for(&found), seed(51.0, 7.0)),
            Some("cars:transporter"),
        );
    }

    /// The other way nothing suits, and it wants the opposite answer: a real
    /// lorry with nothing but vans installed gets the longest van, not the
    /// shortest.
    #[test]
    fn where_everything_is_too_short_the_longest_goes_in() {
        let pool = vec![van("cars:kastenwagen", 6.30), van("cars:transporter", 4.82)];
        let mut found = detection(51.0, 7.0, 0.0);
        found.length = 12.0;
        assert_eq!(
            fitting(&pool, band_for(&found), seed(51.0, 7.0)),
            Some("cars:kastenwagen"),
        );
    }

    #[test]
    fn nothing_installed_places_nothing() {
        let found = detection(51.0, 7.0, 0.0);
        assert_eq!(fitting(&[], band_for(&found), 7), None);
    }

    #[test]
    fn the_same_place_always_draws_the_same_car() {
        let a = seed(51.0, 7.0);
        assert_eq!(a, seed(51.0, 7.0));
        assert_ne!(a, seed(51.000001, 7.0));
    }

    fn line_with_roads(roads: Vec<RoadSource>) -> Line {
        let mut line = straight_line();
        line.source.roads = roads;
        line
    }

    fn street(name: &str, tags: &[&str]) -> RoadSource {
        RoadSource {
            name: name.into(),
            points: vec![
                RoadPoint {
                    lat: 51.0,
                    lon: 7.0,
                },
                RoadPoint {
                    lat: 51.0,
                    lon: 7.01,
                },
            ],
            width: 8.0,
            surface: RoadSurface::Asphalt,
            center_line: CenterLine::None,
            edge_lines: false,
            bridge: false,
            tags: tags.iter().map(|t| (*t).to_string()).collect(),
        }
    }

    // -----------------------------------------------------------------
    // Trees
    // -----------------------------------------------------------------

    /// A crown as the walk reports one: round, so the two axes agree, and with
    /// no heading worth the name.
    fn tree_crown(lat: f64, lon: f64, across: f64) -> GeoDetection {
        GeoDetection {
            class: 0,
            place: "laubbaum".into(),
            kind: Placement::Tree,
            score: 0.9,
            lat,
            lon,
            length: across,
            width: across * 0.95,
            heading: 0.0,
        }
    }

    fn species(name: &str, crown: f64) -> (String, Option<Footprint>) {
        (
            name.into(),
            Some(Footprint {
                length: crown,
                width: crown * 0.97,
            }),
        )
    }

    /// The three individuals `mods/trees` ships of one species, and a fir for
    /// the tag that is not asked for.
    fn tree_catalogue() -> TrackObjects {
        let object = |crown: f64, tags: &[&str]| track_model::TrackObject {
            name: "x".into(),
            model: "trees/assets/x.gltf".into(),
            lateral_offset: 0.0,
            yaw_deg: 0.0,
            height: 0.0,
            autumn_model: None,
            winter_model: None,
            lod_distances: Vec::new(),
            footprint: Some(Footprint {
                length: crown,
                width: crown * 0.97,
            }),
            tags: tags.iter().map(|t| (*t).to_string()).collect(),
        };
        let mut map = std::collections::BTreeMap::new();
        // Tagged as the shipped mod tags them: the kind, and the stands the
        // species belongs to.
        let broadleaf = ["laubbaum", "stand-laubwald", "stand-mischwald"];
        let conifer = ["nadelbaum", "stand-nadelwald", "stand-mischwald"];
        map.insert("trees:rotbuche_a".into(), object(9.0, &broadleaf));
        map.insert("trees:rotbuche_b".into(), object(13.0, &broadleaf));
        map.insert("trees:rotbuche_c".into(), object(18.0, &broadleaf));
        map.insert("trees:fichte_a".into(), object(11.0, &conifer));
        TrackObjects { map }
    }

    fn report_of(found: Vec<GeoDetection>) -> Report {
        Report {
            outcome: vision::Outcome {
                found,
                windows: 1,
                blank: 0,
                tiles: 1,
            },
            lots: Vec::new(),
        }
    }

    fn tree_options() -> AiOptions {
        AiOptions {
            place_objects: true,
            place_trees: true,
            pave_lots: false,
            ..Default::default()
        }
    }

    /// The whole point of the tree kind: a crown does not become an object
    /// bolted to an edge of the track graph, it becomes a row in the tree list
    /// — which is the list the select tool picks from and Delete empties.
    #[test]
    fn a_crown_becomes_a_tree_and_not_an_object() {
        let mut line = straight_line();
        let report = report_of(vec![tree_crown(51.0005, 7.0005, 12.0)]);
        let placed = commit(
            &mut line,
            &report,
            &tree_options(),
            &tree_catalogue(),
            false,
        );
        assert_eq!(placed.trees, 1);
        assert_eq!(placed.objects, 0);
        assert_eq!(placed.by_tag.get("laubbaum"), Some(&1));
        assert!(line.source.objects.is_empty(), "nothing against the track");
        let tree = &line.source.trees[0];
        assert!(tree.object.starts_with("trees:rotbuche"), "{}", tree.object);
        assert!((tree.lat - 51.0005).abs() < 1e-9 && (tree.lon - 7.0005).abs() < 1e-9);
        assert!((0.0..360.0).contains(&tree.yaw_deg));
    }

    /// A twelve-metre crown is planted as a twelve-metre tree, whichever of
    /// the species the place happened to draw.
    #[test]
    fn a_tree_is_grown_to_the_crown_that_was_measured() {
        let objects = tree_catalogue();
        for step in 0..48 {
            let mut line = straight_line();
            let found = tree_crown(51.0 + step as f64 * 2e-4, 7.001, 12.0);
            commit(
                &mut line,
                &report_of(vec![found]),
                &tree_options(),
                &objects,
                false,
            );
            let tree = &line.source.trees[0];
            let model = objects.map[&tree.object].footprint.unwrap().length;
            let standing = model * tree.scale;
            assert!(
                (standing - 12.0).abs() < 0.5,
                "{} at {:.2} stands {standing:.1} m across, not 12",
                tree.object,
                tree.scale,
            );
        }
    }

    /// Beyond the growth band the species was simply the wrong choice, and a
    /// tree squeezed to a third of itself looks like one.
    #[test]
    fn a_tree_is_never_squeezed_past_recognition() {
        assert!(
            (grown(
                12.0,
                Some(Footprint {
                    length: 12.0,
                    width: 12.0
                })
            ) - 1.0)
                .abs()
                < 1e-9
        );
        assert_eq!(
            grown(
                2.0,
                Some(Footprint {
                    length: 18.0,
                    width: 18.0
                })
            ),
            GROWTH.0
        );
        assert_eq!(
            grown(
                40.0,
                Some(Footprint {
                    length: 9.0,
                    width: 9.0
                })
            ),
            GROWTH.1
        );
        // A mod that states no crown is not guessed at.
        assert_eq!(grown(12.0, None), 1.0);
    }

    /// A builder who names a stand has overruled the photograph about what the
    /// wood is, and the size goes with it: any member of the stand, at its own
    /// size give or take a third. Planting the stand at the size the crowns
    /// measured would be a spruce wood of saplings — a crown detector on a
    /// provider's imagery reports the sunlit top of a young conifer, not its
    /// spread.
    #[test]
    fn a_named_stand_is_planted_the_way_the_forest_brush_plants_one() {
        let objects = tree_catalogue();
        let options = AiOptions {
            species: Some("nadelwald".into()),
            ..tree_options()
        };
        let mut line = straight_line();
        // Crowns of two metres — smaller than anything in the catalogue.
        let found: Vec<GeoDetection> = (0..24)
            .map(|i| tree_crown(51.0 + i as f64 * 3e-4, 7.004, 2.0))
            .collect();
        let placed = commit(&mut line, &report_of(found), &options, &objects, false);
        assert_eq!(placed.trees, 24);
        assert_eq!(
            placed.by_tag.keys().collect::<Vec<_>>(),
            vec!["stand-nadelwald"],
            "counted under the stand that was asked for, not the model's tag"
        );
        for tree in &line.source.trees {
            assert_eq!(
                tree.object, "trees:fichte_a",
                "the stand's only member, whatever the crown measured"
            );
            assert!(
                (0.7..=1.3).contains(&tree.scale),
                "planted at its own size, not shrunk to a two-metre crown: {}",
                tree.scale
            );
        }

        // Without the override the same crowns take the model's tag, and the
        // measurement decides — which for a two-metre crown means the smallest
        // broadleaf there is, shrunk as far as it may be.
        let mut line = straight_line();
        let found: Vec<GeoDetection> = (0..24)
            .map(|i| tree_crown(51.0 + i as f64 * 3e-4, 7.004, 2.0))
            .collect();
        commit(
            &mut line,
            &report_of(found),
            &tree_options(),
            &objects,
            false,
        );
        assert!(
            line.source
                .trees
                .iter()
                .all(|t| t.object == "trees:rotbuche_a" && t.scale == GROWTH.0),
            "the shortest of the tag, at the floor of the growth band"
        );
    }

    /// The imagery has not changed, so the wood must not either: a second run
    /// over ground that was done adds nothing.
    #[test]
    fn a_second_run_over_the_same_wood_plants_nothing() {
        let mut line = straight_line();
        let objects = tree_catalogue();
        let found: Vec<GeoDetection> = (0..12)
            .map(|i| tree_crown(51.0 + i as f64 * 1e-4, 7.002, 10.0))
            .collect();
        let first = commit(
            &mut line,
            &report_of(found.clone()),
            &tree_options(),
            &objects,
            false,
        );
        assert_eq!(first.trees, 12);
        let again = commit(
            &mut line,
            &report_of(found),
            &tree_options(),
            &objects,
            false,
        );
        assert_eq!(again.trees, 0, "every crown already has its tree");
        assert_eq!(line.source.trees.len(), 12);
    }

    /// Two crowns a metre apart are one tree; the spacing is what says so, and
    /// turning it off plants both.
    #[test]
    fn two_crowns_on_the_same_spot_become_one_tree() {
        let objects = tree_catalogue();
        let pair = vec![
            tree_crown(51.0, 7.003, 10.0),
            tree_crown(51.000009, 7.003, 10.0),
        ];
        let mut line = straight_line();
        let placed = commit(
            &mut line,
            &report_of(pair.clone()),
            &tree_options(),
            &objects,
            false,
        );
        assert_eq!(placed.trees, 1);

        let mut line = straight_line();
        let options = AiOptions {
            tree_spacing: 0.0,
            ..tree_options()
        };
        let placed = commit(&mut line, &report_of(pair), &options, &objects, false);
        assert_eq!(placed.trees, 2, "no spacing asked for, none applied");
    }

    /// A drawn area can reach over the edge of what the module portrays, and
    /// the forest brush cuts its wood to the envelope for the same reason.
    #[test]
    fn a_crown_outside_the_envelope_is_not_planted() {
        let mut line = straight_line();
        line.source.envelope = vec![
            content::route::EnvelopePoint {
                lat: 51.0,
                lon: 7.0,
            },
            content::route::EnvelopePoint {
                lat: 51.0,
                lon: 7.01,
            },
            content::route::EnvelopePoint {
                lat: 51.01,
                lon: 7.01,
            },
            content::route::EnvelopePoint {
                lat: 51.01,
                lon: 7.0,
            },
        ];
        let report = report_of(vec![
            tree_crown(51.005, 7.005, 10.0),
            tree_crown(51.5, 7.005, 10.0),
        ]);
        let placed = commit(
            &mut line,
            &report,
            &tree_options(),
            &tree_catalogue(),
            false,
        );
        assert_eq!(placed.trees, 1, "only the one inside the module");
        assert!(line.source.trees[0].lat < 51.01);
    }

    /// A run whose tag no installed mod carries plants nothing at all, rather
    /// than a tree called "nadelbaum".
    #[test]
    fn a_tag_no_mod_carries_plants_nothing() {
        let mut line = straight_line();
        let mut found = tree_crown(51.0005, 7.0005, 12.0);
        found.place = "mangrove".into();
        let placed = commit(
            &mut line,
            &report_of(vec![found]),
            &tree_options(),
            &tree_catalogue(),
            false,
        );
        assert_eq!(placed.trees, 0);
        assert!(line.source.trees.is_empty());
    }

    /// Re-running a corridor after widening it must not reshuffle the wood a
    /// builder has already looked at.
    #[test]
    fn the_same_place_always_plants_the_same_tree() {
        let objects = tree_catalogue();
        let plant = || {
            let mut line = straight_line();
            let report = report_of(vec![tree_crown(51.0007, 7.0009, 11.0)]);
            commit(&mut line, &report, &tree_options(), &objects, false);
            line.source.trees[0].clone()
        };
        assert_eq!(plant(), plant());
    }

    /// The species is drawn by the place from those that suit the crown — so a
    /// small crown never draws the big individual, and the pick is still not
    /// always the same tree.
    #[test]
    fn the_crown_decides_which_of_the_species_is_planted() {
        let pool = vec![
            species("trees:rotbuche_a", 9.0),
            species("trees:rotbuche_b", 13.0),
            species("trees:rotbuche_c", 18.0),
        ];
        let mut small = tree_crown(51.0, 7.0, 8.0);
        small.length = 8.0;
        let drawn: std::collections::BTreeSet<&str> = (0..64)
            .filter_map(|step| {
                let mut at = small.clone();
                at.lat = 51.0 + step as f64 * 1e-4;
                fitting(&pool, band_for(&at), seed(at.lat, at.lon))
            })
            .collect();
        assert!(
            !drawn.contains("trees:rotbuche_c"),
            "an 18 m crown has no business on an 8 m one: {drawn:?}",
        );
        assert!(drawn.contains("trees:rotbuche_a"));
    }

    /// The band is the one thing that differs between a bay and a crown, and
    /// it differs in both directions: a bay may not be overfilled, a crown may
    /// be met from either side.
    #[test]
    fn a_crown_is_measured_symmetrically_and_a_bay_is_not() {
        let (floor, room) = band_for(&tree_crown(51.0, 7.0, 10.0));
        assert!((floor - 10.0 / SPREAD).abs() < 1e-9);
        assert!((room - 10.0 * SPREAD).abs() < 1e-9);
        let car = band_for(&detection(51.0, 7.0, 0.0));
        assert!(
            car.1 < 4.4 + 1.0,
            "a bay gives half a metre, not half again"
        );
    }

    /// The registry and the mod have to be talking about the same woods. A
    /// class that says it covers crowns from two and a half metres to
    /// twenty-six has to find something to plant at either end of that, or the
    /// run quietly does nothing over half its own range — and the tags it
    /// names have to be tags the installed trees actually carry.
    ///
    /// Reads `mods/trees` itself, like the pylon tests read `mods/pylons`:
    /// the point is the shipped data, and a fixture would only prove that the
    /// fixture agrees with itself.
    #[test]
    fn the_shipped_trees_cover_the_span_the_registry_claims() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods/trees/objects");
        let mut map = std::collections::BTreeMap::new();
        for entry in std::fs::read_dir(dir).expect("mods/trees ships with the repository") {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("ron") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let object = track_model::TrackObject::from_ron(&text)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let stem = path.file_stem().unwrap().to_str().unwrap();
            let crown = object
                .footprint
                .unwrap_or_else(|| panic!("{stem} states no crown, so it can never be planted"))
                .length;
            assert!(
                (0.1..=40.0).contains(&crown),
                "{stem} states a crown of {crown} m"
            );
            map.insert(format!("trees:{stem}"), object);
        }
        let objects = TrackObjects { map };
        assert_eq!(objects.map.len(), 153, "the whole catalogue was read");

        for id in ["tree-crowns", "tree-species"] {
            let spec = VisionConfig::default().model_by_id(id).unwrap().clone();
            for (_, class) in spec.placing() {
                let (low, high) = class.span.expect("a tree class states its span");
                for tag in [&class.place, &class.conifer] {
                    if tag.is_empty() {
                        continue;
                    }
                    let pool = tagged(&objects, tag);
                    assert!(!pool.is_empty(), "{id}: no tree tagged {tag} is installed");
                    let crowns: Vec<f64> =
                        pool.iter().filter_map(|c| c.1).map(|f| f.length).collect();
                    let (smallest, largest) = (
                        crowns.iter().copied().fold(f64::MAX, f64::min),
                        crowns.iter().copied().fold(0.0, f64::max),
                    );
                    let mut crown = low;
                    while crown <= high {
                        let mut found = tree_crown(51.0, 7.0, crown);
                        found.place = tag.clone();
                        let name = fitting(&pool, band_for(&found), seed(crown, 7.0))
                            .unwrap_or_else(|| panic!("{id}/{tag}: nothing to plant on {crown} m"));
                        let model = pool.iter().find(|(c, _)| c == name).unwrap().1;
                        let standing = model.unwrap().length * grown(crown, model);
                        let error = (standing - crown).abs() / crown;
                        // Inside what the tag actually offers, the size that
                        // was measured is the size that goes in — the species
                        // is drawn from those that suit and then scaled the
                        // rest of the way.
                        let limit = if (smallest..=largest).contains(&crown) {
                            0.01
                        } else {
                            // Outside it, the nearest the mod can manage: a
                            // one-metre bush where the smallest installed is
                            // two is planted at its floor, not skipped.
                            1.0 / 3.0
                        };
                        assert!(
                            error <= limit,
                            "{id}/{tag}: a {crown:.2} m crown was planted as {name} \
                             standing {standing:.2} m ({:.0}% out)",
                            error * 100.0,
                        );
                        crown += 0.25;
                    }
                }
            }
        }
    }

    /// The grid has to answer for a neighbour that fell on the other side of a
    /// cell edge — the failure a hand-rolled bucket makes, and the one that
    /// shows up as a doubled tree every few metres.
    #[test]
    fn the_occupancy_grid_sees_across_its_own_cell_edges() {
        let mut taken = Occupied::new(3.0);
        let at = |lat: f64, lon: f64| geo::to_ecef_deg(lat, lon, 0.0);
        taken.add(at(51.0, 7.0));
        // A metre north, whichever cell that lands in.
        const METRE: f64 = 1.0 / 111_132.0;
        for step in 0..200 {
            let probe = at(51.0 + step as f64 * METRE * 0.05, 7.0);
            let apart = at(51.0, 7.0).distance(probe);
            assert_eq!(taken.taken(probe), apart < 3.0, "{apart:.2} m apart",);
        }
    }

    /// Otherwise a second run over ground this dialog has already paved could
    /// not put a car in the bays it found the first time.
    #[test]
    fn a_car_park_this_dialog_paved_is_not_taken_for_a_street() {
        let line = line_with_roads(vec![street("Bahnhofstraße", &[]), street("", &LOT_TAGS)]);
        let kept = streets(&line);
        assert_eq!(kept.len(), 1, "only the street, not the car park");
        assert_eq!(kept[0].1, 8.0, "and it keeps its width");
        assert_eq!(kept[0].0.len(), 2);
    }

    /// The head of a module file is where this project keeps its provenance,
    /// and a headless run has nobody to ask before it drops it.
    #[test]
    fn a_headless_write_keeps_the_files_header() {
        let file = std::env::temp_dir().join("cr-header-test.ron");
        std::fs::write(
            &file,
            "// Soester Boerde — the fields are dl-de/by-2-0.\n// The roads are ODbL.\n\n(\n    name: \"x\",\n)\n",
        )
        .unwrap();
        let head = header_of(&file.display().to_string());
        assert!(head.contains("dl-de/by-2-0"));
        assert!(head.contains("ODbL"));
        assert!(!head.contains("name:"), "the data is written by to_ron");
        assert!(head.ends_with('\n'));
        std::fs::remove_file(&file).ok();
    }

    #[test]
    fn a_file_that_opens_with_data_gets_no_header() {
        let file = std::env::temp_dir().join("cr-header-none.ron");
        std::fs::write(&file, "(\n    name: \"x\", // a comment further down\n)\n").unwrap();
        assert_eq!(header_of(&file.display().to_string()), "");
        std::fs::remove_file(&file).ok();
    }

    #[test]
    fn a_module_without_roads_forbids_nothing() {
        assert!(streets(&straight_line()).is_empty());
    }

    #[test]
    fn a_car_park_becomes_an_unmarked_ribbon_along_its_rows() {
        let lot = Lot {
            lat: 51.0,
            lon: 7.0,
            polygon: Vec::new(),
            line: ((51.0, 6.999), (51.0, 7.001)),
            width: 17.0,
            length: 140.0,
            row_heading: 90.0,
            car_heading: 0.0,
            cars: 30,
        };
        let road = road_for(&lot);
        assert_eq!(road.points.len(), 2);
        assert_eq!(road.center_line, CenterLine::None);
        assert!(!road.edge_lines, "a car park has no Seitenlinien");
        assert!((road.width - 17.0).abs() < 1e-9);
        assert!(road.tags.iter().any(|t| t == "parking"));
    }

    #[test]
    fn an_area_from_the_select_circle_is_a_closed_ring_of_the_right_size() {
        let mut state = EditorState::default();
        let centre = geo::to_ecef_deg(51.0, 7.0, 0.0);
        area_from_circle(&mut state, centre, 50.0);
        let ring = state.ai_area.expect("the circle became an area");
        assert_eq!(ring.len(), 24);
        for point in &ring {
            let d = centre.distance(*point);
            assert!((d - 50.0).abs() < 0.5, "{d}");
        }
    }

    #[test]
    fn an_area_of_two_corners_is_refused() {
        let mut state = EditorState {
            walk_points: vec![
                geo::to_ecef_deg(51.0, 7.0, 0.0),
                geo::to_ecef_deg(51.0, 7.001, 0.0),
            ],
            ..Default::default()
        };
        finish(&mut state);
        assert!(state.ai_area.is_none());
        assert!(
            state.walk_points.is_empty(),
            "the half-drawn ring is dropped either way"
        );
    }

    /// A module with one straight kilometre of track running east, so an
    /// anchored object can be checked against a position that is known by
    /// construction.
    fn straight_line() -> Line {
        let source = LineSource {
            name: "bench".into(),
            nodes: vec![NodeSource::Buffer, NodeSource::Buffer],
            edges: vec![EdgeSource {
                from: 0,
                to: 1,
                start: EdgeStart::Geo {
                    point: GeoPoint {
                        lat: 51.0,
                        lon: 7.0,
                        height: 100.0,
                    },
                    heading_deg: 90.0,
                },
                segments: vec![Segment::straight(1_000.0)],
                grade: vec![],
                cant: vec![],
                speed: vec![],
                track_type: vec![(0.0, STARTER_TRACK_TYPE.into())],
                electrification: vec![],
                formation: true,
            }],
            ..Default::default()
        };
        let net = source.compile().expect("the bench line compiles").net;
        Line {
            source,
            net,
            path: None,
            dirty: false,
            needs_rebuild: false,
            terrain_change: Default::default(),
            recenter: false,
            issues: Vec::new(),
        }
    }

    /// The same line, but climbing and up where a German module actually sits
    /// — a hundred and forty metres above the ellipsoid the probe starts from.
    fn climbing_line(per_mille: f64, height: f64) -> Line {
        let mut line = straight_line();
        line.source.edges[0].grade = vec![(0.0, per_mille)];
        if let EdgeStart::Geo { point, .. } = &mut line.source.edges[0].start {
            point.height = height;
        }
        line.net = line.source.compile().expect("it compiles").net;
        line
    }

    /// The offset anybody could see in the Börde module: the arc length was
    /// taken from a probe on the ellipsoid, a hundred and forty metres below
    /// the rails, and whatever that left along the track was thrown away
    /// rather than folded back in. It is about the drop times the gradient, so
    /// it changes sign with the gradient — which is what made it look
    /// intermittent.
    #[test]
    fn a_find_beside_a_climbing_track_lands_where_it_was_seen() {
        // Per mille, and the range is the Börde's own: −11.9 to +16.1.
        for per_mille in [0.0, -11.9, 4.6, 11.6, 16.1] {
            let line = climbing_line(per_mille, 140.0);
            let edge = &line.net.edges()[0];
            let pose = edge.eval(edge.length() / 2.0);
            let right = pose.tangent.cross(pose.up).normalize_or_zero();
            let beside = EcefPos(pose.pos.0 + right * 30.0);
            let (lat, lon, _) = geo::from_ecef(beside);
            let object = object_for(
                &line,
                &detection(lat.to_degrees(), lon.to_degrees(), 90.0),
                "cars:golf".into(),
                true,
            )
            .expect("the find anchors to the track");
            let back = crate::tools::object_pos(&line.net, &object).expect("it has a position");
            assert!(
                back.distance(beside) < 0.05,
                "{per_mille} per mille: {:.2} m off",
                back.distance(beside)
            );
        }
    }

    #[test]
    fn an_anchored_object_lands_back_where_the_model_saw_it() {
        let line = straight_line();
        let edge = &line.net.edges()[0];
        let pose = edge.eval(edge.length() / 2.0);
        // Thirty metres to one side of the middle of the first track.
        let frame = EnuFrame::at(pose.pos);
        let right = pose.tangent.cross(pose.up).normalize_or_zero();
        let beside = EcefPos(pose.pos.0 + right * 30.0);
        let (lat, lon, _) = geo::from_ecef(beside);
        let _ = frame;

        let object = object_for(
            &line,
            &detection(lat.to_degrees(), lon.to_degrees(), 90.0),
            "cars:golf".into(),
            true,
        )
        .expect("the find anchors to the track");
        assert!(
            (object.lateral_offset - 30.0).abs() < 0.5,
            "{}",
            object.lateral_offset
        );
        let back = crate::tools::object_pos(&line.net, &object).expect("it has a position");
        assert!(
            back.distance(beside) < 0.5,
            "{} m off",
            back.distance(beside)
        );
    }

    #[test]
    fn the_yaw_is_measured_against_the_track_not_against_north() {
        let line = straight_line();
        let edge = &line.net.edges()[0];
        let pose = edge.eval(edge.length() / 2.0);
        let bearing = track_bearing(&pose);
        let right = pose.tangent.cross(pose.up).normalize_or_zero();
        let beside = EcefPos(pose.pos.0 + right * 30.0);
        let (lat, lon, _) = geo::from_ecef(beside);
        // A car lying along the track has a yaw of nothing at all — or half a
        // turn, where the coin came down the other way.
        let object = object_for(
            &line,
            &detection(
                lat.to_degrees(),
                lon.to_degrees(),
                bearing.rem_euclid(180.0),
            ),
            "cars:golf".into(),
            true,
        )
        .expect("the find anchors to the track");
        let yaw = object.yaw_deg.rem_euclid(180.0);
        assert!(yaw < 1.0 || yaw > 179.0, "{}", object.yaw_deg);
    }
}
