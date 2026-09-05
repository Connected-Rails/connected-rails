//! Wind turbines in the editor: the import.
//!
//! Like the road and the overhead line imports, and for the same reason — a
//! module in the Börde or in Dithmarschen has dozens of them, they stand on
//! the horizon of every shot, and both OpenStreetMap and the
//! Marktstammdatenregister have surveyed every one. So the import asks
//! Overpass for the module envelope's turbines
//! ([`content::import::parse_wind_turbines`]) and then the register for what
//! those machines are ([`fields::mastr`]), matches the two
//! ([`content::wind::match_register`]) and writes the result into the line.
//!
//! **The dialog has filters, not a list.** Two questions decide what an import
//! is worth: whether the farmyard machines come too — many, small, and rarely
//! what a module is after — and whether the register is asked at all, which
//! costs a second request and answers for the machine where OpenStreetMap's
//! mappers left the tags empty. Both are on the form before the start; the
//! report afterwards is the summary and the decision, like everywhere else.
//!
//! **Nothing stands up yet.** The turbine models are still to come, so every
//! entry lands in the line file with an empty object and the tile pipeline
//! passes over it (see [`content::wind`]). What the import writes is the whole
//! truth about each machine — where it stands, how high its hub is, how wide
//! its rotor, which machine it is and its number in the register — and the day
//! the models ship, `content::wind::PRESETS` names them and the turbines stand
//! up without another import.

use crate::Line;
use crate::tools::{EditorState, Selection};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use content::RegisterMatch;
use content::route::WindTurbineSource;
use editor_ui::{colors, space};
use fields::RequestConfig;
use i18n::t;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};

/// Width of the import dialog [px].
const DIALOG: f32 = 460.0;

/// What the import asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindOptions {
    /// Take the small machines too (see [`content::wind::SMALL_ROTOR`]) — the
    /// mast in a farmyard rather than the turbine on the horizon. Off by
    /// default: they are many and they are furniture.
    pub small: bool,
    /// Ask the Marktstammdatenregister what the machines are. On by default —
    /// it is the difference between a third of the turbines knowing their
    /// dimensions and all of them.
    pub register: bool,
}

impl Default for WindOptions {
    fn default() -> Self {
        Self {
            small: false,
            register: true,
        }
    }
}

/// What one import found.
struct Report {
    turbines: Vec<WindTurbineSource>,
    /// What the register answered for — zero when it was not asked.
    matched: RegisterMatch,
}

/// A running import: the thread's channels and the switch that stops it.
struct Job {
    /// Behind a mutex so the dialog works as a Bevy resource — a `Receiver`
    /// is `Send` but not `Sync`.
    progress: Mutex<Receiver<&'static str>>,
    result: Mutex<Receiver<Result<Report, String>>>,
    stop: Arc<AtomicBool>,
}

/// The import dialog, and whatever it has found.
#[derive(Resource)]
pub struct WindImport {
    pub open: bool,
    pub options: WindOptions,
    job: Option<Job>,
    /// The last thing the thread said, redrawn every frame.
    stage: &'static str,
    /// The finished import, waiting for Commit.
    report: Option<Report>,
    /// What went wrong, if the dialog has something to say.
    message: String,
}

impl Default for WindImport {
    fn default() -> Self {
        Self {
            open: false,
            options: WindOptions::default(),
            job: None,
            stage: "wind-import-fetching",
            report: None,
            message: String::new(),
        }
    }
}

impl WindImport {
    /// Up from the first frame — what `--import-wind` inserts, so a screenshot
    /// run can look at the dialog it has no keyboard to open.
    pub fn opened() -> Self {
        Self {
            open: true,
            ..Default::default()
        }
    }
}

/// The envelope's box, south-west to north-east — what the queries ask for.
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

/// Starts the import on a thread of its own: Overpass for the positions, the
/// register for the machines, and the match between them.
fn start(dialog: &mut WindImport, bbox: (f64, f64, f64, f64)) {
    let query = content::import::wind_query(bbox.0, bbox.1, bbox.2, bbox.3);
    let config = RequestConfig::default();
    let options = dialog.options;

    let (progress_out, progress) = std::sync::mpsc::channel();
    let (result_out, result) = std::sync::mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::spawn(move || {
        let _ = progress_out.send("wind-import-fetching");
        let json = match fields::osm::fetch_raw(&query, &config) {
            Ok(json) => json,
            Err(e) => {
                let _ = result_out.send(Err(e.to_string()));
                return;
            }
        };
        if flag.load(Ordering::Relaxed) {
            let _ = result_out.send(Ok(Report {
                turbines: Vec::new(),
                matched: RegisterMatch::default(),
            }));
            return;
        }
        let _ = progress_out.send("wind-import-parsing");
        let mut turbines = match content::import::parse_wind_turbines(&json) {
            Ok(turbines) => turbines,
            Err(e) => {
                let _ = result_out.send(Err(e.to_string()));
                return;
            }
        };

        // The register is the second question, and the one that answers what
        // the machines are. A turbine the register cannot place keeps what
        // OpenStreetMap said about it.
        let mut matched = RegisterMatch::default();
        if options.register && !flag.load(Ordering::Relaxed) {
            let _ = progress_out.send("wind-import-asking-register");
            match fields::mastr::fetch_wind(bbox.0, bbox.1, bbox.2, bbox.3, &config) {
                Ok(units) => matched = content::wind::match_register(&mut turbines, &units),
                Err(e) => {
                    let _ = result_out.send(Err(e.to_string()));
                    return;
                }
            }
        }
        if !options.small {
            turbines.retain(|t| !content::wind::is_small(t));
        }
        let _ = result_out.send(Ok(Report { turbines, matched }));
    });

    dialog.report = None;
    dialog.message.clear();
    dialog.job = Some(Job {
        progress: Mutex::new(progress),
        result: Mutex::new(result),
        stop,
    });
}

/// The import dialog. Its own system, like [`crate::power`] — `ui::draw` is
/// already at Bevy's system-parameter limit.
pub fn draw(
    mut contexts: EguiContexts,
    mut dialog: ResMut<WindImport>,
    mut line: ResMut<Line>,
    mut state: ResMut<EditorState>,
    mut overlay: ResMut<crate::overlay::Overlay>,
    mut request: ResMut<crate::Request>,
    mut themed: Local<bool>,
) -> Result {
    // `ui::draw` installs the theme on the very first pass and draws nothing
    // itself; the font families it registers are only bound from the next one.
    // A dialog that is up on that first pass — which `--import-wind` makes it —
    // has to sit that pass out as well: a heading in a family that is not there
    // yet is a panic inside egui, not a fallback. The same guard the imagery
    // dialog carries for `--detect`.
    if !*themed {
        *themed = true;
        return Ok(());
    }
    // The menu asks through the request, like every other menu entry.
    if request.import_wind {
        request.import_wind = false;
        dialog.open = true;
    }
    if !dialog.open {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?.clone();

    // Whatever the thread has said since the last frame; the channels sit
    // behind mutexes, so the reads are copies.
    if dialog.job.is_some() {
        let mut finished_report: Option<Result<Report, String>> = None;
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
                    Ok(report) => finished_report = Some(report),
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => failed = true,
                }
            } else {
                failed = true;
            }
            stage_out = Some(stage);
        }
        if let Some(stage_out) = stage_out {
            dialog.stage = stage_out;
        }
        if let Some(report) = finished_report {
            dialog.job = None;
            match report {
                Ok(report) => dialog.report = Some(report),
                Err(e) => dialog.message = e,
            }
        } else if failed {
            // The thread died without an answer.
            dialog.job = None;
            dialog.message = t!("field-import-failed");
        }
    }

    let mut close = false;
    egui::Window::new(t!("wind-import-title"))
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.viewport_rect().center())
        .show(&ctx, |ui| {
            ui.set_width(DIALOG);
            let dialog: &mut WindImport = &mut dialog;
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

/// The form: what to import and whom to ask. Shown before the first run and
/// after a commit.
fn settings(ui: &mut egui::Ui, dialog: &mut WindImport, line: &Line) -> bool {
    ui.label(t!("wind-import-intro"));
    ui.add_space(space::S);

    if line.source.envelope.len() < 3 {
        ui.colored_label(colors::WARN, t!("field-import-no-envelope"));
    }

    ui.add_space(space::S);
    editor_ui::form_grid("wind-import-form")
        .num_columns(2)
        .show(ui, |ui| {
            crate::ui::row(ui, "wind-import-register", |ui| {
                ui.checkbox(&mut dialog.options.register, "");
            });
            crate::ui::row(ui, "wind-import-small", |ui| {
                ui.checkbox(&mut dialog.options.small, "");
            });
        });

    ui.add_space(space::S);
    ui.colored_label(colors::TEXT_SECONDARY, t!("wind-import-no-models"));

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
        if start_button.clicked()
            && ready
            && let Some(bbox) = envelope_bbox(line)
        {
            start(dialog, bbox);
        }
        if ui.button(t!("action-cancel")).clicked() {
            close = true;
        }
    });
    close
}

/// While it runs: the bar, what is happening, and Stop.
fn running(ui: &mut egui::Ui, dialog: &mut WindImport) {
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

/// The summary, and the decision. The list is by machine, not by turbine: what
/// a builder wants to see before committing is whether the register recognised
/// the park — twenty-four Enercon E-115 in one line is an answer, twenty-four
/// unknown machines is a reason to look again.
fn finished(
    ui: &mut egui::Ui,
    dialog: &mut WindImport,
    line: &mut Line,
    state: &mut EditorState,
    overlay: &mut crate::overlay::Overlay,
) -> bool {
    let Some(report) = &dialog.report else {
        return false;
    };
    let turbines = &report.turbines;
    ui.label(
        egui::RichText::new(t!(
            "wind-import-found",
            turbines = turbines.len(),
            named = report.matched.matched
        ))
        .color(colors::TEXT_STRONG),
    );
    if report.matched.spare > 0 {
        ui.colored_label(
            colors::TEXT_SECONDARY,
            t!("wind-import-spare", count = report.matched.spare),
        );
    }

    ui.add_space(space::S);
    egui::ScrollArea::vertical()
        .max_height(160.0)
        .show(ui, |ui| {
            editor_ui::form_grid("wind-import-machines")
                .num_columns(2)
                .min_col_width(0.0)
                .show(ui, |ui| {
                    // By machine, and within a machine by how many there are:
                    // the park a module is really about is the biggest row.
                    let mut counts: std::collections::BTreeMap<String, usize> =
                        std::collections::BTreeMap::new();
                    for turbine in turbines {
                        let name = if turbine.model.is_empty() {
                            t!("wind-import-unknown-machine")
                        } else {
                            let hub = format!("{:.0}", turbine.hub_height);
                            let rotor = format!("{:.0}", turbine.rotor_diameter);
                            t!(
                                "wind-import-machine",
                                model = turbine.model.clone(),
                                hub = hub,
                                rotor = rotor
                            )
                        };
                        *counts.entry(name).or_insert(0) += 1;
                    }
                    for (name, count) in counts {
                        ui.label(
                            egui::RichText::new(count.to_string()).color(colors::TEXT_SECONDARY),
                        );
                        ui.label(name);
                        ui.end_row();
                    }
                });
        });

    ui.add_space(space::M);
    let has_turbines = !turbines.is_empty();
    let (mut close, mut apply, mut again) = (false, false, false);
    ui.horizontal(|ui| {
        apply = ui
            .add_enabled(has_turbines, egui::Button::new(t!("field-import-commit")))
            .clicked();
        again = ui.button(t!("field-import-again")).clicked();
        close = ui.button(t!("action-cancel")).clicked();
    });
    if apply {
        let count = turbines.len();
        line.source.wind_turbines.extend(turbines.clone());
        // The turbines ride in with the vegetation, so the corridor's scatter
        // has to be laid out again — the same invalidation the overhead line
        // import asks for. Nothing is drawn while the models are missing, but
        // the day they are there this is what puts them on the tiles.
        line.needs_rebuild = true;
        line.terrain_change = crate::terrain::TerrainChange::all();
        line.dirty = true;
        overlay.status = t!("status-wind-imported", count = count);
        state.selection = Selection::None;
        return true;
    }
    if again {
        dialog.report = None;
    }
    close
}
