//! Roads in the editor: the import, the tool, and the line on the map.
//!
//! The import is the point of the whole feature. A road network is far too
//! much to draw by hand, and OSM has surveyed every street of the country —
//! so the import asks Overpass for the module envelope's `highway=*` ways
//! and turns them into [`RoadSource`]s, the same way the field import asks
//! the registers: on a thread of its own, progress on screen, a Cancel that
//! means it, and nothing written until the user says so. The report ends in
//! a summary and a Commit — one undo step for the whole import.
//!
//! Drawing a road by hand is for the track the import did not carry: clicks
//! collect the centre line, Enter finishes it, and the preset picker above
//! decides what the road is made of (see [`crate::tools::Tool::PlaceRoad`]).

use crate::tools::{EditorState, Selection};
use crate::{Focus, Line};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use content::route::{CenterLine, RoadPoint, RoadSource, RoadSurface};
use editor_ui::{colors, space};
use fields::RequestConfig;
use i18n::t;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use world_coords::{EcefPos, geo};

/// Colour of a road's line on the map — a grey of its own, so it is never
/// mistaken for the walkways' teal or the fields' green.
const COLOR: Color = Color::srgb(0.70, 0.72, 0.76);
/// The same, dimmed: while the road tool is not up, the roads are context.
const COLOR_IDLE: Color = Color::srgba(0.70, 0.72, 0.74, 0.35);
/// The selected road.
const COLOR_SELECTED: Color = Color::srgb(0.96, 0.97, 1.0);
/// Width of the import dialog [px].
const DIALOG: f32 = 460.0;
/// How far a click may miss a centre line and still take it [m].
const PICK_METRES: f64 = 8.0;

/// What the import asks Overpass for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RoadOptions {
    /// Import the field tracks (`highway=track`) too — what the Börde's farm
    /// roads are. Off by default: a module's agricultural tracks are many and
    /// thin, and the road import is usually after the real streets.
    pub tracks: bool,
    /// Import the access ways (`service`, `living_street`, `pedestrian`)
    /// too — the narrowest class, and many of them.
    pub narrow: bool,
}

/// Whether an OSM class passes the dialog's filters. The narrow classes are
/// opt-in, and so are the tracks — both are many, and thin.
fn allowed(options: &RoadOptions, class: &str) -> bool {
    match class {
        "track" => options.tracks,
        "living_street" | "pedestrian" => options.narrow,
        "service" => options.tracks || options.narrow,
        _ => true,
    }
}

/// A running import: the thread's channels and the switch that stops it.
struct Job {
    /// Behind a mutex so the dialog works as a Bevy resource — a `Receiver`
    /// is `Send` but not `Sync`.
    progress: Mutex<Receiver<&'static str>>,
    result: Mutex<Receiver<Result<Vec<RoadSource>, String>>>,
    stop: Arc<AtomicBool>,
}

/// The import dialog, and whatever it has found.
#[derive(Resource)]
pub struct RoadImport {
    pub open: bool,
    pub options: RoadOptions,
    job: Option<Job>,
    /// The last thing the thread said, redrawn every frame.
    stage: &'static str,
    /// The finished import, waiting for Commit.
    report: Option<Vec<RoadSource>>,
    /// What went wrong, if the dialog has something to say.
    message: String,
}

impl Default for RoadImport {
    fn default() -> Self {
        Self {
            open: false,
            options: RoadOptions::default(),
            job: None,
            stage: "road-import-fetching",
            report: None,
            message: String::new(),
        }
    }
}

/// The envelope's box, south-west to north-east — what the query asks for.
fn envelope_bbox(line: &Line) -> Option<(f64, f64, f64, f64)> {
    let corners = &line.source.envelope;
    (corners.len() >= 3).then(|| {
        (
            corners.iter().map(|p| p.lat).fold(f64::MAX, f64::min),
            corners.iter().map(|p| p.lon).fold(f64::MAX, f64::min),
            corners.iter().map(|p| p.lat).fold(f64::MIN, f64::max),
            corners.iter().map(|p| p.lon).fold(f64::MIN, f64::max),
        )
    })
}

/// Starts the import on a thread of its own: the envelope's box, the road
/// query, Overpass, the parser.
fn start(dialog: &mut RoadImport, bbox: (f64, f64, f64, f64)) {
    let query = content::import::roads_query(bbox.0, bbox.1, bbox.2, bbox.3);
    let config = RequestConfig::default();
    let options = dialog.options;

    let (progress_out, progress) = std::sync::mpsc::channel();
    let (result_out, result) = std::sync::mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::spawn(move || {
        let _ = progress_out.send("road-import-fetching");
        let json = match fields::osm::fetch_raw(&query, &config) {
            Ok(json) => json,
            Err(e) => {
                let _ = result_out.send(Err(e.to_string()));
                return;
            }
        };
        if flag.load(Ordering::Relaxed) {
            let _ = result_out.send(Ok(Vec::new()));
            return;
        }
        let _ = progress_out.send("road-import-parsing");
        let parsed = content::import::parse_roads(&json)
            .map(|roads| {
                roads
                    .into_iter()
                    .filter(|road| {
                        road.tags
                            .first()
                            .and_then(|t| t.strip_prefix("highway-"))
                            .is_some_and(|class| allowed(&options, class))
                    })
                    .collect()
            })
            .map_err(|e| e.to_string());
        let _ = result_out.send(parsed);
    });

    dialog.report = None;
    dialog.message.clear();
    dialog.job = Some(Job {
        progress: Mutex::new(progress),
        result: Mutex::new(result),
        stop,
    });
}

/// The import dialog. Its own system, like [`crate::new_module`] — `ui::draw`
/// is already at Bevy's system-parameter limit.
pub fn draw(
    mut contexts: EguiContexts,
    mut dialog: ResMut<RoadImport>,
    mut line: ResMut<Line>,
    mut state: ResMut<EditorState>,
    mut overlay: ResMut<crate::overlay::Overlay>,
    mut request: ResMut<crate::Request>,
) -> Result {
    // The menu asks through the request, like every other menu entry — the
    // menu bar is drawn by `ui::draw`, which does not have this resource.
    if request.import_roads {
        request.import_roads = false;
        dialog.open = true;
    }
    if !dialog.open {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?.clone();

    // Whatever the thread has said since the last frame. The channels sit
    // behind mutexes, so the reads are copies: whatever the result was is
    // taken out before the dialog itself is touched again.
    if dialog.job.is_some() {
        let mut finished: Option<Result<Vec<RoadSource>, String>> = None;
        let mut failed = false;
        let mut stage_out: Option<&'static str> = None;
        if let Some(job) = &dialog.job {
            let mut stage = dialog.stage;
            if let Ok(progress) = job.progress.lock() {
                while let Ok(next) = progress.try_recv() {
                    stage = next;
                }
            }
            if let Ok(result) = job.result.lock() {
                match result.try_recv() {
                    Ok(report) => finished = Some(report),
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => failed = true,
                }
            } else {
                failed = true;
            }
            stage_out = Some(stage);
        }
        // The borrow of `dialog.job` ends here; the dialog may move again.
        if let Some(stage_out) = stage_out {
            dialog.stage = stage_out;
        }
        if let Some(finished) = finished {
            dialog.job = None;
            match finished {
                Ok(roads) => dialog.report = Some(roads),
                Err(e) => dialog.message = e,
            }
        } else if failed {
            // The thread died without an answer.
            dialog.job = None;
            dialog.message = t!("field-import-failed");
        }
    }

    let mut close = false;
    egui::Window::new(t!("road-import-title"))
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.viewport_rect().center())
        .show(&ctx, |ui| {
            ui.set_width(DIALOG);
            let dialog: &mut RoadImport = &mut dialog;
            if dialog.job.is_some() {
                running(ui, dialog);
            } else if dialog.report.is_some() {
                close |= finished(ui, dialog, &mut line, &mut state, &mut overlay);
            } else {
                close |= settings(ui, dialog, &line);
            }
        });
    if close {
        dialog.open = false;
        dialog.report = None;
        dialog.message.clear();
    }
    Ok(())
}

/// The form: what to import and how. Shown before the first run and after a
/// commit.
fn settings(ui: &mut egui::Ui, dialog: &mut RoadImport, line: &Line) -> bool {
    ui.label(t!("road-import-intro"));
    ui.add_space(space::S);

    if line.source.envelope.len() < 3 {
        ui.colored_label(colors::WARN, t!("field-import-no-envelope"));
    }

    ui.add_space(space::S);
    editor_ui::form_grid("road-import-form")
        .num_columns(2)
        .show(ui, |ui| {
            crate::ui::row(ui, "road-import-tracks", |ui| {
                ui.checkbox(&mut dialog.options.tracks, "");
            });
            crate::ui::row(ui, "road-import-narrow", |ui| {
                ui.checkbox(&mut dialog.options.narrow, "");
            });
        });

    if !dialog.message.is_empty() {
        ui.add_space(space::S);
        ui.colored_label(colors::ERROR, &dialog.message);
    }

    ui.add_space(space::M);
    let mut close = false;
    let ready = line.source.envelope.len() >= 3;
    ui.horizontal(|ui| {
        let start_button = ui.add_enabled(ready, egui::Button::new(t!("field-import-start")));
        if !ready {
            start_button
                .clone()
                .on_disabled_hover_text(t!("field-import-no-envelope"));
        }
        if start_button.clicked() && ready && let Some(bbox) = envelope_bbox(line) {
            start(dialog, bbox);
        }
        if ui.button(t!("action-cancel")).clicked() {
            close = true;
        }
    });
    close
}

/// While it runs: the bar, what is happening, and Stop.
fn running(ui: &mut egui::Ui, dialog: &mut RoadImport) {
    let bar = egui::ProgressBar::new(0.0).animate(true);
    ui.add(bar.desired_width(DIALOG));
    ui.add_space(space::XS);
    ui.label(t!(dialog.stage));
    ui.add_space(space::M);
    if let Some(job) = &dialog.job
        && ui.button(t!("field-import-stop")).clicked()
    {
        job.stop.store(true, Ordering::Relaxed);
    }
}

/// The summary, and the decision.
fn finished(
    ui: &mut egui::Ui,
    dialog: &mut RoadImport,
    line: &mut Line,
    state: &mut EditorState,
    overlay: &mut crate::overlay::Overlay,
) -> bool {
    let Some(roads) = &dialog.report else {
        return false;
    };
    ui.label(
        egui::RichText::new(t!("road-import-found", roads = roads.len()))
            .color(colors::TEXT_STRONG),
    );

    ui.add_space(space::S);
    egui::ScrollArea::vertical()
        .max_height(160.0)
        .show(ui, |ui| {
            editor_ui::form_grid("road-import-classes")
                .num_columns(2)
                .min_col_width(0.0)
                .show(ui, |ui| {
                    let mut counts: std::collections::BTreeMap<&str, usize> =
                        std::collections::BTreeMap::new();
                    for road in roads {
                        if let Some(class) = road.tags.first().and_then(|t| t.strip_prefix("highway-"))
                        {
                            *counts.entry(class).or_insert(0) += 1;
                        }
                    }
                    for (class, count) in counts {
                        ui.label(
                            egui::RichText::new(count.to_string()).color(colors::TEXT_SECONDARY),
                        );
                        ui.label(t!(&format!("road-class-{class}")));
                        ui.end_row();
                    }
                });
        });

    ui.add_space(space::M);
    let has_roads = !roads.is_empty();
    let (mut close, mut apply, mut again) = (false, false, false);
    ui.horizontal(|ui| {
        apply = ui
            .add_enabled(has_roads, egui::Button::new(t!("field-import-commit")))
            .clicked();
        again = ui.button(t!("field-import-again")).clicked();
        close = ui.button(t!("action-cancel")).clicked();
    });
    if apply {
        let count = roads.len();
        line.source.roads.extend(roads.clone());
        overlay.status = t!("status-roads-imported", count = count);
        state.selection = Selection::None;
        return true;
    }
    if again {
        dialog.report = None;
    }
    close
}

// ---------------------------------------------------------------------------
// Drawing one by hand
// ---------------------------------------------------------------------------

/// The UTM zone the line sits in — from the anchor where there is one, else
/// the middle of what has been placed, else the middle of Germany. Only the
/// metre-scale correctness matters here, and the zone boundary is 6° apart.
fn zone_of(line: &Line) -> u8 {
    let lon = line
        .source
        .anchor
        .map(|a| a.lon)
        .or_else(|| line.source.roads.first().map(|r| r.centre().1))
        .unwrap_or(10.0);
    fields::land::utm_zone_at(lon)
}
/// The pick, in degrees — distance to the centre line, in the line's own UTM
/// zone. The first road within reach wins, in line order; the click tests
/// every road, so a crossing is the one whose centre line is nearest.
pub fn pick_at(line: &Line, lat: f64, lon: f64) -> Option<usize> {
    let zone = zone_of(line);
    let (qe, qn) = world_coords::geo::to_utm(lat.to_radians(), lon.to_radians(), zone);
    let query = glam::DVec2::new(qe, qn);
    let mut best = (PICK_METRES, None);
    for (index, road) in line.source.roads.iter().enumerate() {
        let points: Vec<glam::DVec2> = road
            .points
            .iter()
            .map(|p| {
                let (e, n) = world_coords::geo::to_utm(p.lat.to_radians(), p.lon.to_radians(), zone);
                glam::DVec2::new(e, n)
            })
            .collect();
        for pair in points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let d = b - a;
            if d.length_squared() < 1e-12 {
                continue;
            }
            let t = ((query - a).dot(d) / d.length_squared()).clamp(0.0, 1.0);
            let dist = query.distance(a + d * t);
            if dist < best.0 {
                best = (dist, Some(index));
            }
        }
    }
    best.1
}

/// Closes the centre line being drawn into a road. `Some` is a message for
/// the status bar — too few points, and nothing was made.
pub fn finish(line: &mut Line, state: &mut EditorState) -> Option<String> {
    if state.tool != crate::tools::Tool::PlaceRoad {
        state.walk_points.clear();
        return None;
    }
    if state.walk_points.len() < 2 {
        return Some(t!("status-road-points"));
    }
    let points: Vec<RoadPoint> = state
        .walk_points
        .drain(..)
        .map(|p| {
            let (lat, lon, _) = world_coords::geo::from_ecef(p);
            RoadPoint {
                lat: lat.to_degrees(),
                lon: lon.to_degrees(),
            }
        })
        .collect();
    let preset = preset_of(state);
    line.source.roads.push(RoadSource {
        name: String::new(),
        points,
        width: state.road_width.unwrap_or(preset.width),
        surface: preset.surface,
        center_line: preset.center_line,
        edge_lines: preset.edge_lines,
        // A hand-drawn road starts on the ground; the panel flags bridges.
        bridge: false,
        tags: Vec::new(),
    });
    state.selection = Selection::Road(line.source.roads.len() - 1);
    state.walk_vertex = None;
    None
}

/// The preset the tool has in hand — `None` means the plain country road,
/// the commonest thing a hand-drawn road is.
pub fn preset_of(state: &EditorState) -> &'static content::roads::RoadPreset {
    state
        .road_preset
        .and_then(content::roads::preset)
        .unwrap_or(&content::roads::PRESETS[3])
}

/// The road tool's own options: which road the next one is. The preset gives
/// width, surface and markings — and the width stays editable, because the
/// presets are planning values and the module may disagree.
pub fn tool_rows(ui: &mut egui::Ui, line: &Line, state: &mut EditorState) {
    let preset = preset_of(state);
    editor_ui::form_grid("road-tool").show(ui, |ui| {
        crate::ui::row(ui, "road-preset", |ui| {
            egui::ComboBox::from_id_salt("road-tool-preset")
                .width(space::FIELD * 2.0)
                .selected_text(t!(&format!("road-preset-{}", preset.id)))
                .show_ui(ui, |ui| {
                    for candidate in content::roads::PRESETS {
                        if ui
                            .selectable_label(
                                preset.id == candidate.id,
                                t!(&format!("road-preset-{}", candidate.id)),
                            )
                            .clicked()
                        {
                            state.road_preset = Some(candidate.id);
                            state.road_width = Some(candidate.width);
                        }
                    }
                });
        });
        crate::ui::row(ui, "road-width", |ui| {
            let mut width = state.road_width.unwrap_or(preset.width);
            if editor_ui::field(ui, &mut width, 0.5, 1.0..=30.0, "m").changed() {
                state.road_width = Some(width);
            }
        });
    });
    ui.small(t!("road-count", roads = line.source.roads.len()));
    if !state.walk_points.is_empty() {
        ui.add_space(space::XS);
        ui.small(t!("road-active", points = state.walk_points.len()));
    }
}

/// The selected road's own properties: its name, and the numbers the
/// carriageway is made of — width, surface, markings.
pub fn selection_rows(ui: &mut egui::Ui, line: &mut Line, state: &mut EditorState) {
    let Selection::Road(index) = state.selection else {
        return;
    };
    let zone = zone_of(line);
    let Some(road) = line.source.roads.get_mut(index) else {
        return;
    };
    ui.label(if road.name.is_empty() {
        t!("sel-road-summary", index = index)
    } else {
        t!("sel-road-named", index = index, name = road.name)
    });
    ui.small(
        egui::RichText::new(t!(
            "road-length",
            length = format!("{:.0}", road.length(zone))
        ))
        .small()
        .color(colors::TEXT_SECONDARY),
    );
    editor_ui::form_grid("sel-road").show(ui, |ui| {
        crate::ui::row(ui, "road-name", |ui| {
            ui.add(egui::TextEdit::singleline(&mut road.name).desired_width(space::FIELD));
        });
        crate::ui::row(ui, "road-width", |ui| {
            editor_ui::field(ui, &mut road.width, 0.1, 1.0..=30.0, "m");
        });
        crate::ui::row(ui, "road-surface", |ui| {
            surface_combo(ui, "sel-road-surface", &mut road.surface);
        });
        crate::ui::row(ui, "road-center-line", |ui| {
            centre_combo(ui, "sel-road-centre", &mut road.center_line);
        });
        crate::ui::row(ui, "road-edge-lines", |ui| {
            ui.checkbox(&mut road.edge_lines, "");
        });
        crate::ui::row(ui, "road-bridge", |ui| {
            ui.checkbox(&mut road.bridge, "");
        });
    });
    if !road.tags.is_empty() {
        ui.small(
            egui::RichText::new(road.tags.join(", "))
                .small()
                .color(colors::TEXT_SECONDARY),
        );
    }
    ui.add_space(space::XS);
    ui.horizontal(|ui| {
        // The delete takes the whole road; a corner is edited on the map.
        if ui.button(t!("action-delete")).clicked() {
            state.selection = Selection::None;
            if index < line.source.roads.len() {
                line.source.roads.remove(index);
            }
        }
    });
}

/// Which material the carriageway wears.
fn surface_combo(ui: &mut egui::Ui, id: &str, surface: &mut RoadSurface) {
    egui::ComboBox::from_id_salt(id)
        .width(space::FIELD * 2.0)
        .selected_text(t!(&format!("road-surface-{}", surface.id())))
        .show_ui(ui, |ui| {
            for candidate in [RoadSurface::Asphalt, RoadSurface::Concrete] {
                if ui
                    .selectable_label(
                        *surface == candidate,
                        t!(&format!("road-surface-{}", candidate.id())),
                    )
                    .clicked()
                {
                    *surface = candidate;
                }
            }
        });
}

/// What runs along the middle of the carriageway.
fn centre_combo(ui: &mut egui::Ui, id: &str, centre: &mut CenterLine) {
    egui::ComboBox::from_id_salt(id)
        .width(space::FIELD * 2.0)
        .selected_text(t!(&format!("road-center-{}", centre.id())))
        .show_ui(ui, |ui| {
            for candidate in [
                CenterLine::None,
                CenterLine::Dashed,
                CenterLine::DashedUrban,
                CenterLine::Solid,
            ] {
                if ui
                    .selectable_label(
                        *centre == candidate,
                        t!(&format!("road-center-{}", candidate.id())),
                    )
                    .clicked()
                {
                    *centre = candidate;
                }
            }
        });
}

// ---------------------------------------------------------------------------
// The map
// ---------------------------------------------------------------------------

/// The road under a world point, if any — distance to the centre line.
pub fn pick(line: &Line, p: EcefPos) -> Option<usize> {
    let (lat, lon, _) = geo::from_ecef(p);
    pick_at(line, lat.to_degrees(), lon.to_degrees())
}

/// Where a road's centre line is in world coordinates, at the module's own
/// height — like the envelope, and for the same reason: a centre line is a
/// line that has to keep its shape, and a point taking the ground under it
/// would drag it into every hollow.
pub fn positions(line: &Line, focus: &Focus, index: usize) -> Vec<EcefPos> {
    let height = crate::envelope::height(line, focus);
    line.source
        .roads
        .get(index)
        .map(|road| {
            road.points
                .iter()
                .map(|p| geo::to_ecef_deg(p.lat, p.lon, height))
                .collect()
        })
        .unwrap_or_default()
}

/// Outlines every road's centre line on the map.
///
/// The carriageways themselves are drawn by the terrain
/// (`world_render::roads`); this is the editable object on top of them —
/// thin, and only fully lit for the road being worked on.
pub fn draw_outlines(
    gizmos: &mut Gizmos,
    line: &Line,
    state: &EditorState,
    focus: &Focus,
    origin: &world_coords::RenderOrigin,
) {
    if line.source.roads.is_empty() {
        return;
    }
    let selected = match state.selection {
        Selection::Road(index) => Some(index),
        _ => None,
    };
    // Only what is near the view point: the line of a road ten kilometres
    // off is a pixel, and there are thousands of them.
    let (view_lat, view_lon, _) = geo::from_ecef(focus.position);
    let reach = (focus.height * 2.0).clamp(500.0, 6_000.0);
    let per_degree = 111_320.0;

    for (index, road) in line.source.roads.iter().enumerate() {
        let here = selected == Some(index);
        if !here {
            let (lat, lon) = road.centre();
            let dlat = (lat - view_lat.to_degrees()) * per_degree;
            let dlon = (lon - view_lon.to_degrees()) * per_degree * view_lat.cos().abs();
            if dlat * dlat + dlon * dlon > reach * reach {
                continue;
            }
        }
        let colour = if here {
            COLOR_SELECTED
        } else if state.tool == crate::tools::Tool::PlaceRoad {
            COLOR
        } else {
            COLOR_IDLE
        };
        let ring = positions(line, focus, index);
        for pair in ring.iter().zip(ring.iter().skip(1)) {
            gizmos.line(origin.to_render(*pair.0), origin.to_render(*pair.1), colour);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use content::route::{LineSource, RoadPoint};

    fn road(lat: f64, lon: f64, width: f64) -> RoadSource {
        RoadSource {
            name: "Landstraße".into(),
            points: vec![
                RoadPoint { lat, lon },
                RoadPoint {
                    lat,
                    lon: lon + 0.002,
                },
            ],
            width,
            surface: RoadSurface::Asphalt,
            center_line: CenterLine::Dashed,
            edge_lines: true,
            bridge: false,
            tags: vec!["highway-primary".into()],
        }
    }

    fn line_with(roads: Vec<RoadSource>) -> Line {
        Line {
            source: LineSource {
                roads,
                ..Default::default()
            },
            net: Default::default(),
            path: None,
            dirty: false,
            needs_rebuild: false,
            terrain_change: Default::default(),
            recenter: false,
            issues: Vec::new(),
        }
    }

    /// A click within the pick reach takes the road; a click beside every
    /// centre line starts a new one.
    #[test]
    fn a_click_on_the_centre_line_takes_the_road() {
        let line = line_with(vec![road(52.0, 10.0, 6.0)]);
        // On the line: the middle of the way.
        assert_eq!(pick_at(&line, 52.0, 10.001), Some(0));
        // Four metres beside it — within the 8 m reach.
        assert_eq!(pick_at(&line, 52.00004, 10.001), Some(0));
        // Forty metres: nothing.
        assert_eq!(pick_at(&line, 52.0004, 10.001), None);
        // An empty module has nothing to pick.
        assert_eq!(pick_at(&line_with(Vec::new()), 52.0, 10.0), None);
    }

    /// The nearest road wins at a crossing.
    #[test]
    fn the_nearest_centre_line_wins() {
        let mut line = line_with(vec![road(52.0, 10.0, 6.0), road(52.0005, 10.0, 6.0)]);
        line.source.roads.reverse();
        // Both cross the query longitude; the click is nearer the second.
        assert_eq!(pick_at(&line, 52.00045, 10.001), Some(0));
    }

    /// Finishing takes the tool's options and selects the new road; too few
    /// points are reported and the drawing goes on.
    #[test]
    fn finishing_needs_two_points_and_selects_the_new_road() {
        let mut line = line_with(Vec::new());
        let mut state = EditorState {
            tool: crate::tools::Tool::PlaceRoad,
            road_preset: Some("residential"),
            road_width: Some(4.0),
            ..Default::default()
        };
        state.walk_points.push(world_coords::geo::to_ecef_deg(52.0, 10.0, 0.0));
        assert!(
            finish(&mut line, &mut state).is_some(),
            "one point is no road"
        );
        assert!(line.source.roads.is_empty());
        state
            .walk_points
            .push(world_coords::geo::to_ecef_deg(52.0, 10.001, 0.0));
        assert!(finish(&mut line, &mut state).is_none());
        assert!(state.walk_points.is_empty());
        assert_eq!(state.selection, Selection::Road(0));
        let road = &line.source.roads[0];
        assert_eq!(road.width, 4.0, "the tool's width");
        assert_eq!(road.surface, RoadSurface::Asphalt, "the preset's surface");
        assert_eq!(
            road.center_line,
            CenterLine::DashedUrban,
            "the preset's markings"
        );
        assert_eq!(road.points.len(), 2);
    }

    /// The class filter of the dialog: the narrow classes are opt-in, and so
    /// are the tracks.
    #[test]
    fn the_dialog_filters_the_thin_classes() {
        let plain = RoadOptions::default();
        assert!(allowed(&plain, "primary"));
        assert!(!allowed(&plain, "track"), "field tracks are opt-in");
        assert!(!allowed(&plain, "living_street"));
        assert!(allowed(&RoadOptions { tracks: true, ..plain }, "track"));
        assert!(allowed(&RoadOptions { tracks: true, ..plain }, "service"));
        assert!(
            allowed(&RoadOptions { narrow: true, ..plain }, "living_street"),
            "access ways are the narrow option's own"
        );
    }
}
