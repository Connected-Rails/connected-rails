//! Overhead lines in the editor: the import.
//!
//! Like the road import, and for the same reason — a power line is dozens of
//! masts three hundred metres apart, and OSM has surveyed every one of them.
//! The import asks Overpass for the module envelope's `power=line` and
//! `power=minor_line` ways, [`content::import::parse_power_lines`] picks the
//! mast type off the `design=*`, `voltage=*` and `frequency=*` tags, and the
//! atlas preset stamps the mast objects and the crossarms into the line
//! ([`content::power`]).
//!
//! There is nothing to configure. The road import has filters because a
//! module's field tracks are many and thin and usually unwanted; a power line
//! is never in that position — a module either has one crossing it or it does
//! not. So the dialog is the report and the decision, and nothing else.
//!
//! What comes out is editable like everything else: the mast object is a string
//! in the line file, so a route that wants a Tonnenmast where the import chose
//! a Donaumast changes it there.

use crate::Line;
use crate::tools::{EditorState, Selection};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use content::route::PowerLineSource;
use editor_ui::{colors, space};
use fields::RequestConfig;
use i18n::t;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};

/// Width of the import dialog [px].
const DIALOG: f32 = 460.0;

struct Job {
    /// Behind a mutex so the dialog works as a Bevy resource — a `Receiver`
    /// is `Send` but not `Sync`.
    progress: Mutex<Receiver<&'static str>>,
    result: Mutex<Receiver<Result<Vec<PowerLineSource>, String>>>,
    stop: Arc<AtomicBool>,
}

/// The import dialog, and whatever it has found.
#[derive(Resource)]
pub struct PowerImport {
    pub open: bool,
    job: Option<Job>,
    /// The last thing the thread said, redrawn every frame.
    stage: &'static str,
    /// The finished import, waiting for Commit.
    report: Option<Vec<PowerLineSource>>,
    /// What went wrong, if the dialog has something to say.
    message: String,
}

impl Default for PowerImport {
    fn default() -> Self {
        Self {
            open: false,
            job: None,
            stage: "power-import-fetching",
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

/// Starts the import on a thread of its own: the envelope's box, the power
/// query, Overpass, the parser.
fn start(dialog: &mut PowerImport, bbox: (f64, f64, f64, f64)) {
    let query = content::import::power_query(bbox.0, bbox.1, bbox.2, bbox.3);
    let config = RequestConfig::default();

    let (progress_out, progress) = std::sync::mpsc::channel();
    let (result_out, result) = std::sync::mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::spawn(move || {
        let _ = progress_out.send("power-import-fetching");
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
        let _ = progress_out.send("power-import-parsing");
        let _ =
            result_out.send(content::import::parse_power_lines(&json).map_err(|e| e.to_string()));
    });

    dialog.report = None;
    dialog.message.clear();
    dialog.job = Some(Job {
        progress: Mutex::new(progress),
        result: Mutex::new(result),
        stop,
    });
}

/// The import dialog. Its own system, like [`crate::roads`] — `ui::draw` is
/// already at Bevy's system-parameter limit.
pub fn draw(
    mut contexts: EguiContexts,
    mut dialog: ResMut<PowerImport>,
    mut line: ResMut<Line>,
    mut state: ResMut<EditorState>,
    mut overlay: ResMut<crate::overlay::Overlay>,
    mut request: ResMut<crate::Request>,
) -> Result {
    // The menu asks through the request, like every other menu entry.
    if request.import_power {
        request.import_power = false;
        dialog.open = true;
    }
    if !dialog.open {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?.clone();

    // Whatever the thread has said since the last frame; the channels sit
    // behind mutexes, so the reads are copies.
    if dialog.job.is_some() {
        let mut finished_report: Option<Result<Vec<PowerLineSource>, String>> = None;
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
                Ok(lines) => dialog.report = Some(lines),
                Err(e) => dialog.message = e,
            }
        } else if failed {
            // The thread died without an answer.
            dialog.job = None;
            dialog.message = t!("field-import-failed");
        }
    }

    let mut close = false;
    egui::Window::new(t!("power-import-title"))
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.viewport_rect().center())
        .show(&ctx, |ui| {
            ui.set_width(DIALOG);
            let dialog: &mut PowerImport = &mut dialog;
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

/// What it is about to do, and Start. Shown before the first run and after a
/// commit.
fn settings(ui: &mut egui::Ui, dialog: &mut PowerImport, line: &Line) -> bool {
    ui.label(t!("power-import-intro"));
    ui.add_space(space::S);

    if line.source.envelope.len() < 3 {
        ui.colored_label(colors::WARN, t!("field-import-no-envelope"));
    }
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
fn running(ui: &mut egui::Ui, dialog: &mut PowerImport) {
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

/// The summary, and the decision. The list is by mast type, not by line: what
/// a builder wants to see before committing is whether the import guessed
/// Donaumast where the photographs show Donaumast.
fn finished(
    ui: &mut egui::Ui,
    dialog: &mut PowerImport,
    line: &mut Line,
    state: &mut EditorState,
    overlay: &mut crate::overlay::Overlay,
) -> bool {
    let Some(lines) = &dialog.report else {
        return false;
    };
    let masts: usize = lines.iter().map(|l| l.points.len()).sum();
    ui.label(
        egui::RichText::new(t!("power-import-found", lines = lines.len(), masts = masts))
            .color(colors::TEXT_STRONG),
    );

    ui.add_space(space::S);
    egui::ScrollArea::vertical()
        .max_height(160.0)
        .show(ui, |ui| {
            editor_ui::form_grid("power-import-types")
                .num_columns(2)
                .min_col_width(0.0)
                .show(ui, |ui| {
                    let mut counts: std::collections::BTreeMap<&str, usize> =
                        std::collections::BTreeMap::new();
                    for source in lines {
                        let id = source.tags.first().map(String::as_str).unwrap_or("other");
                        *counts.entry(id).or_insert(0) += source.points.len();
                    }
                    for (id, count) in counts {
                        ui.label(
                            egui::RichText::new(count.to_string()).color(colors::TEXT_SECONDARY),
                        );
                        ui.label(t!(&format!("pylon-{id}")));
                        ui.end_row();
                    }
                });
        });

    ui.add_space(space::M);
    let has_lines = !lines.is_empty();
    let (mut close, mut apply, mut again) = (false, false, false);
    ui.horizontal(|ui| {
        apply = ui
            .add_enabled(has_lines, egui::Button::new(t!("field-import-commit")))
            .clicked();
        again = ui.button(t!("field-import-again")).clicked();
        close = ui.button(t!("action-cancel")).clicked();
    });
    if apply {
        let count = masts;
        line.source.power_lines.extend(lines.clone());
        // The masts ride in with the vegetation, so the whole corridor's
        // scatter has to be laid out again — the same invalidation a forest
        // import asks for.
        line.needs_rebuild = true;
        line.terrain_change = crate::terrain::TerrainChange::all();
        line.dirty = true;
        overlay.status = t!("status-power-imported", count = count);
        state.selection = Selection::None;
        return true;
    }
    if again {
        dialog.report = None;
    }
    close
}
