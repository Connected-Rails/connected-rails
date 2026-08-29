//! Fields in the editor: the import, the list, and the outline on the map.
//!
//! The import is the point of the whole feature, and it is a dialog rather than
//! a menu entry that goes away and comes back: it asks a public service over a
//! network, it can take half a minute, and it must be possible to see what it
//! is doing and to stop it. So it runs on a thread of its own and reports
//! through a channel — stage, count and the state being asked — and the dialog
//! shows a bar, the running tally and a Cancel that means it.
//!
//! **Nothing is written until the user says so.** The import ends in a summary —
//! so many fields, so many hectares, this many of each crop, these warnings —
//! and a Commit button. That is what makes an import safe to try: the answer to
//! "what will this do to my module" is on the screen before it does it, and
//! Commit is a single undo step (see [`crate::History`] — one interaction, one
//! snapshot, so Ctrl+Z takes the whole import back out).
//!
//! Two scopes, as the plan asks: the whole module inside its envelope, or the
//! current selection. The first is what a builder does once when the module is
//! new; the second is for filling a corner in, or for re-importing one field
//! after correcting a crop mapping.

use crate::tools::{EditorState, Selection};
use crate::{Focus, Line};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use content::route::{FieldPoint, FieldSource, FieldSourceStamp};
use editor_ui::{colors, space};
use fields::{
    Area, Clip, CropClass, CropTable, FieldCache, FieldFeature, ImportOptions, ImportProgress,
    ImportReport, Land, Stage,
};
use i18n::t;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use world_coords::{EcefPos, RenderOrigin, geo};

/// Colour of a field's outline on the map — the green of growing things, so it
/// is not mistaken for the envelope's yellow or a walkway's blue.
const COLOR: Color = Color::srgb(0.44, 0.68, 0.32);
/// The same for a field that is not what is being worked on.
const COLOR_IDLE: Color = Color::srgba(0.44, 0.68, 0.32, 0.35);
/// The selected field.
const COLOR_SELECTED: Color = Color::srgb(0.72, 0.90, 0.50);
/// Width of the import dialog [px].
const DIALOG: f32 = 460.0;
/// Crops offered when a field is drawn by hand, in the order a German arable
/// rotation puts them.
const CROPS: [CropClass; 13] = CropClass::ALL;

/// What the import is to cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    /// Everything inside the module envelope. What a new module wants.
    #[default]
    Module,
    /// The selected field's own outline — re-fetch one parcel after a mapping
    /// has been corrected.
    Selection,
}

impl Scope {
    fn key(self) -> &'static str {
        match self {
            Scope::Module => "field-import-scope-module",
            Scope::Selection => "field-import-scope-selection",
        }
    }
}

/// A running import: the thread, what it has said so far, and the switch that
/// stops it.
struct Job {
    /// Behind a mutex so the dialog works as a Bevy resource — a `Receiver` is
    /// `Send` but not `Sync`.
    progress: Mutex<Receiver<ImportProgress>>,
    result: Mutex<Receiver<ImportReport>>,
    stop: Arc<AtomicBool>,
    /// The last thing it said, redrawn every frame.
    latest: ImportProgress,
}

/// The import dialog, and whatever it has found.
#[derive(Resource)]
pub struct FieldImport {
    pub open: bool,
    pub scope: Scope,
    /// Cut the fields at the module boundary, or keep whole the ones whose
    /// middle is inside.
    pub cut: bool,
    /// Smallest field to keep [ha].
    pub min_area_ha: f64,
    /// How far the fields stay clear of the track [m].
    pub clearance: f64,
    /// Ask the services again rather than reading what was fetched before.
    pub refresh: bool,
    job: Option<Job>,
    /// The finished import, waiting for Commit.
    report: Option<ImportReport>,
    /// What went wrong, if the dialog has something to say.
    message: String,
    /// The crop tables, read once.
    table: CropTable,
    cache: FieldCache,
}

impl Default for FieldImport {
    fn default() -> Self {
        Self {
            open: false,
            scope: Scope::Module,
            cut: true,
            min_area_ha: 0.5,
            clearance: 45.0,
            refresh: false,
            job: None,
            report: None,
            message: String::new(),
            table: CropTable::built_in(),
            cache: FieldCache::default(),
        }
    }
}

impl FieldImport {
    pub fn is_running(&self) -> bool {
        self.job.is_some()
    }

    /// Reads the crop mappings a mod or the user has put next to the cache —
    /// `<cache>/crops/nw.csv` and friends (plan ch. 5).
    pub fn load_overrides(&mut self) {
        let dir = self.cache.directory().join("crops");
        if dir.is_dir() {
            self.table.load_overrides(&dir);
        }
    }
}

/// Starts the import on a thread of its own.
fn start(dialog: &mut FieldImport, area: Area, options: ImportOptions) {
    let (progress_out, progress) = std::sync::mpsc::channel();
    let (result_out, result) = std::sync::mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));

    let mut cache = dialog.cache.clone();
    cache.refresh = dialog.refresh;
    let table = dialog.table.clone();
    let flag = stop.clone();
    std::thread::spawn(move || {
        let report = fields::import::run(&area, &options, &cache, &table, &mut |p| {
            // A dropped receiver means the dialog is gone; stop rather than
            // keep asking a service nobody is waiting for.
            progress_out.send(p).is_ok() && !flag.load(Ordering::Relaxed)
        });
        let _ = result_out.send(report);
    });

    dialog.report = None;
    dialog.message.clear();
    dialog.job = Some(Job {
        progress: Mutex::new(progress),
        result: Mutex::new(result),
        stop,
        latest: ImportProgress {
            stage: Stage::Locating,
            done: 0,
            total: 0,
            note: String::new(),
        },
    });
}

/// Takes whatever the thread has said since the last frame.
fn poll(dialog: &mut FieldImport) {
    let Some(job) = &mut dialog.job else {
        return;
    };
    if let Ok(progress) = job.progress.lock() {
        while let Ok(next) = progress.try_recv() {
            job.latest = next;
        }
    }
    let finished = match job.result.lock() {
        Ok(result) => match result.try_recv() {
            Ok(report) => Some(report),
            Err(TryRecvError::Empty) => return,
            // The thread died without an answer.
            Err(TryRecvError::Disconnected) => None,
        },
        Err(_) => None,
    };
    dialog.job = None;
    match finished {
        Some(report) => dialog.report = Some(report),
        None => dialog.message = t!("field-import-failed"),
    }
}

/// The area an import covers, in degrees, with the track to punch out of it.
fn area_of(line: &Line, state: &EditorState, scope: Scope) -> Option<Area> {
    let boundary: Vec<(f64, f64)> = match scope {
        Scope::Module => line
            .source
            .envelope
            .iter()
            .map(|p| (p.lat, p.lon))
            .collect(),
        Scope::Selection => {
            let Selection::Field(index) = state.selection else {
                return None;
            };
            line.source
                .fields
                .get(index)?
                .polygon
                .iter()
                .map(|p| (p.lat, p.lon))
                .collect()
        }
    };
    if boundary.len() < 3 {
        return None;
    }
    // The track, sampled as a polyline per edge: what gets punched out of the
    // fields, so no field lies on the formation the terrain pulls to rail
    // height.
    let mut track = Vec::new();
    for edge in line.net.edges() {
        let length = edge.length();
        if length <= 0.0 {
            continue;
        }
        // Every twenty metres: closer than that adds vertices the corridor
        // quads do not need, further and a tight curve corners.
        let steps = (length / 20.0).ceil().max(1.0) as usize;
        let points: Vec<(f64, f64)> = (0..=steps)
            .map(|i| {
                let s = length * i as f64 / steps as f64;
                let (lat, lon, _) = geo::from_ecef(edge.eval(s).pos);
                (lat.to_degrees(), lon.to_degrees())
            })
            .collect();
        track.push(points);
    }
    Some(Area { boundary, track })
}

/// Writes an import into the line. One call, so it is one undo step.
pub fn commit(line: &mut Line, report: &ImportReport, scope: Scope, selection: &mut Selection) {
    // Re-importing one field replaces that field; importing the module
    // replaces every field that came from an import, and leaves hand-drawn
    // ones alone — those are somebody's work, not the register's.
    match scope {
        Scope::Selection => {
            if let Selection::Field(index) = *selection
                && index < line.source.fields.len()
            {
                line.source.fields.remove(index);
                *selection = Selection::None;
            }
        }
        Scope::Module => {
            line.source.fields.retain(|f| f.source.is_empty());
            if matches!(selection, Selection::Field(_)) {
                *selection = Selection::None;
            }
        }
    }
    for field in &report.fields {
        line.source.fields.push(to_source(field));
    }
    for stamp in &report.stamps {
        let row = FieldSourceStamp {
            land: stamp.land.clone(),
            year: stamp.year,
            fetched: stamp.fetched,
        };
        // One row per state; a second import of the same state replaces it.
        line.source
            .field_sources
            .retain(|existing| existing.land != row.land);
        line.source.field_sources.push(row);
    }
    line.source
        .field_sources
        .sort_by(|a, b| a.land.cmp(&b.land));
}

/// One imported parcel as a line entry.
fn to_source(field: &FieldFeature) -> FieldSource {
    FieldSource {
        polygon: field
            .to_degrees()
            .into_iter()
            .map(|(lat, lon)| FieldPoint { lat, lon })
            .collect(),
        crop: field.crop.id().to_string(),
        code: field.code_raw.clone(),
        label: field.code_text.clone(),
        level: match field.level {
            fields::Level::Declared => "declared",
            fields::Level::Group => "group",
            fields::Level::Drawn => "drawn",
        }
        .to_string(),
        direction_deg: field.direction.to_degrees(),
        // `OSM` where there is no state: a module abroad still has to say
        // where its fields came from.
        source: fields::cache::origin_code(field.land).to_string(),
        year: field.year,
        seed: field.seed(),
        tags: Vec::new(),
    }
}

/// The import dialog. Its own system, like [`crate::new_module`] — `ui::draw`
/// is already at Bevy's system-parameter limit.
pub fn draw(
    mut contexts: EguiContexts,
    mut dialog: ResMut<FieldImport>,
    mut line: ResMut<Line>,
    mut state: ResMut<EditorState>,
    mut overlay: ResMut<crate::overlay::Overlay>,
    mut request: ResMut<crate::Request>,
    sky: Res<world_render::sky::Sky>,
) -> Result {
    // The menu asks through the request, like every other menu entry — the
    // menu bar is drawn by `ui::draw`, which does not have this resource.
    if request.import_fields {
        request.import_fields = false;
        dialog.open = true;
        dialog.load_overrides();
    }
    if !dialog.open {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?.clone();
    poll(&mut dialog);

    let mut close = false;
    egui::Window::new(t!("field-import-title"))
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.viewport_rect().center())
        .show(&ctx, |ui| {
            ui.set_width(DIALOG);
            if dialog.is_running() {
                running(ui, &mut dialog);
            } else if dialog.report.is_some() {
                close |= finished(
                    ui,
                    &mut dialog,
                    &mut line,
                    &mut state,
                    &mut overlay,
                    sky.month,
                    sky.day,
                );
            } else {
                close |= settings(ui, &mut dialog, &line, &state);
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
fn settings(ui: &mut egui::Ui, dialog: &mut FieldImport, line: &Line, state: &EditorState) -> bool {
    ui.label(t!("field-import-intro"));
    ui.add_space(space::S);

    // Which states the module lies in, and what each of them publishes. This is
    // the answer to "why did that come back empty", and it is worth having
    // before the request rather than after it.
    let lands = envelope_lands(line);
    if line.source.envelope.len() < 3 {
        ui.colored_label(colors::WARN, t!("field-import-no-envelope"));
    } else if lands.is_empty() {
        // The module is outside Germany, so no register holds it — say which
        // way the import will go rather than leaving the panel blank.
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("OpenStreetMap").color(colors::TEXT_STRONG));
            ui.label(
                egui::RichText::new(t!("field-source-abroad"))
                    .color(colors::WARN)
                    .small(),
            )
            .on_hover_text(t!("field-source-abroad-hint"));
            ui.label(
                egui::RichText::new(fields::Licence::Odbl.id())
                    .color(colors::TEXT_SECONDARY)
                    .small(),
            );
        });
    } else {
        for land in &lands {
            let service = land.service();
            let (colour, key) = match service.level {
                fields::DataLevel::Gsa => (colors::TEXT, "field-source-gsa"),
                fields::DataLevel::Lpis => (colors::TEXT_SECONDARY, "field-source-lpis"),
                fields::DataLevel::Osm => (colors::TEXT_SECONDARY, "field-source-osm"),
                fields::DataLevel::None => (colors::WARN, "field-source-none"),
            };
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(land.name()).color(colors::TEXT_STRONG));
                ui.label(egui::RichText::new(t!(key)).color(colour).small());
                if service.licence == fields::Licence::Unclear {
                    ui.label(
                        egui::RichText::new(t!("field-licence-unclear"))
                            .color(colors::WARN)
                            .small(),
                    )
                    .on_hover_text(t!("field-licence-unclear-hint"));
                } else if !service.licence.id().is_empty() {
                    ui.label(
                        egui::RichText::new(service.licence.id())
                            .color(colors::TEXT_SECONDARY)
                            .small(),
                    );
                }
            });
        }
    }

    ui.add_space(space::S);
    editor_ui::form_grid("field-import-form")
        .num_columns(2)
        .show(ui, |ui| {
            crate::ui::row(ui, "field-import-scope", |ui| {
                egui::ComboBox::from_id_salt("field-import-scope")
                    .width(space::FIELD * 2.0)
                    .selected_text(t!(dialog.scope.key()))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut dialog.scope,
                            Scope::Module,
                            t!(Scope::Module.key()),
                        );
                        ui.selectable_value(
                            &mut dialog.scope,
                            Scope::Selection,
                            t!(Scope::Selection.key()),
                        );
                    });
            });
            crate::ui::row(ui, "field-import-cut", |ui| {
                ui.checkbox(&mut dialog.cut, "");
            });
            crate::ui::row(ui, "field-import-min-area", |ui| {
                editor_ui::field(ui, &mut dialog.min_area_ha, 0.05, 0.0..=50.0, "ha");
            });
            crate::ui::row(ui, "field-import-clearance", |ui| {
                editor_ui::field(ui, &mut dialog.clearance, 1.0, 0.0..=200.0, "m");
            });
            crate::ui::row(ui, "field-import-refresh", |ui| {
                ui.checkbox(&mut dialog.refresh, "");
            });
        });

    if !dialog.message.is_empty() {
        ui.add_space(space::S);
        ui.colored_label(colors::ERROR, &dialog.message);
    }

    ui.add_space(space::M);
    let mut close = false;
    ui.horizontal(|ui| {
        let ready = area_of(line, state, dialog.scope).is_some();
        let start_button = ui.add_enabled(ready, egui::Button::new(t!("field-import-start")));
        if !ready {
            start_button
                .clone()
                .on_disabled_hover_text(match dialog.scope {
                    Scope::Module => t!("field-import-no-envelope"),
                    Scope::Selection => t!("field-import-no-selection"),
                });
        }
        if start_button.clicked()
            && let Some(area) = area_of(line, state, dialog.scope)
        {
            let options = ImportOptions {
                clip: if dialog.cut { Clip::Cut } else { Clip::Whole },
                min_area: dialog.min_area_ha * 10_000.0,
                track_clearance: dialog.clearance,
                zone: state.terrain_options().zone,
                ..Default::default()
            };
            start(dialog, area, options);
        }
        if ui.button(t!("action-cancel")).clicked() {
            close = true;
        }
    });
    close
}

/// While it runs: the bar, what is happening, and Stop.
fn running(ui: &mut egui::Ui, dialog: &mut FieldImport) {
    let Some(job) = &dialog.job else { return };
    let progress = job.latest.clone();

    let mut bar = egui::ProgressBar::new(progress.fraction().unwrap_or(0.0));
    if progress.fraction().is_none() {
        // A stage whose length is not known yet — an indeterminate bar says
        // "working" without claiming a number it does not have.
        bar = bar.animate(true);
    }
    ui.add(bar.desired_width(DIALOG));
    ui.add_space(space::XS);

    // Always say what is being done, and to whom: an import that sits on
    // "Fetching" for twenty seconds is asking a service, and the user should
    // be able to see which one.
    let what = if progress.note.is_empty() {
        t!(progress.stage.key())
    } else {
        format!("{} · {}", t!(progress.stage.key()), progress.note)
    };
    ui.label(what);
    if progress.total > 0 {
        ui.label(
            egui::RichText::new(t!(
                "field-import-of",
                done = progress.done,
                total = progress.total
            ))
            .color(colors::TEXT_SECONDARY)
            .small(),
        );
    }

    ui.add_space(space::M);
    if ui.button(t!("field-import-stop")).clicked()
        && let Some(job) = &dialog.job
    {
        job.stop.store(true, Ordering::Relaxed);
    }
}

/// The summary, and the decision.
fn finished(
    ui: &mut egui::Ui,
    dialog: &mut FieldImport,
    line: &mut Line,
    state: &mut EditorState,
    overlay: &mut crate::overlay::Overlay,
    month: u32,
    day: u32,
) -> bool {
    let Some(report) = &dialog.report else {
        return false;
    };

    if report.cancelled {
        ui.colored_label(colors::WARN, t!("field-import-stopped"));
        ui.add_space(space::XS);
    }
    ui.label(
        egui::RichText::new(t!(
            "field-import-found",
            fields = report.fields.len(),
            hectares = format!("{:.0}", report.hectares())
        ))
        .color(colors::TEXT_STRONG),
    );
    ui.label(
        egui::RichText::new(t!(
            "field-import-counts",
            parcels = report.parcels,
            small = report.too_small,
            outside = report.outside,
            split = report.split
        ))
        .color(colors::TEXT_SECONDARY)
        .small(),
    );
    ui.label(
        egui::RichText::new(t!(
            "field-import-tiles",
            fetched = report.fetched,
            cached = report.cached
        ))
        .color(colors::TEXT_SECONDARY)
        .small(),
    );

    ui.add_space(space::S);
    egui::ScrollArea::vertical()
        .max_height(160.0)
        .show(ui, |ui| {
            editor_ui::form_grid("field-import-crops")
                .num_columns(2)
                .min_col_width(0.0)
                .show(ui, |ui| {
                    for (crop, count) in report.by_crop() {
                        let [r, g, b] = crop_colour(crop, month, day);
                        ui.label(
                            egui::RichText::new("\u{25a0}").color(egui::Color32::from_rgb(r, g, b)),
                        );
                        ui.label(t!(&crop.key()));
                        ui.label(
                            egui::RichText::new(count.to_string()).color(colors::TEXT_SECONDARY),
                        );
                        ui.end_row();
                    }
                });
        });

    // The source note, verbatim, so it can be copied into the module's credits.
    if !report.attribution.is_empty() {
        ui.add_space(space::S);
        editor_ui::subheading(ui, t!("field-attribution"));
        for credit in report.attribution.credits() {
            ui.label(egui::RichText::new(credit).small());
        }
        for land in report.attribution.unclear() {
            ui.colored_label(
                colors::WARN,
                t!("field-licence-unclear-land", land = land.name()),
            );
        }
    }

    for warning in &report.warnings {
        ui.colored_label(colors::WARN, warning);
    }
    if !report.unknown_codes.is_empty() {
        ui.colored_label(
            colors::WARN,
            t!(
                "field-import-unknown-codes",
                codes = report.unknown_codes.join(", ")
            ),
        );
    }

    ui.add_space(space::M);
    let has_fields = !report.fields.is_empty();
    let (mut close, mut apply, mut again) = (false, false, false);
    ui.horizontal(|ui| {
        apply = ui
            .add_enabled(has_fields, egui::Button::new(t!("field-import-commit")))
            .clicked();
        again = ui.button(t!("field-import-again")).clicked();
        close = ui.button(t!("action-cancel")).clicked();
    });
    // Taken out of the dialog before it is written: the report is borrowed for
    // the summary above, and the write needs the dialog back.
    if apply && let Some(report) = dialog.report.take() {
        let count = report.fields.len();
        commit(line, &report, dialog.scope, &mut state.selection);
        overlay.status = t!("status-fields-imported", count = count);
        return true;
    }
    if again {
        dialog.report = None;
    }
    close
}

/// What a `source` column is called on screen: the state's own name, the
/// fallback's, or the raw code for anything neither knows.
fn origin_name(code: &str) -> String {
    if code == fields::cache::OSM {
        // A project name, so it reads the same in every language.
        return "OpenStreetMap".into();
    }
    Land::from_code(code).map_or_else(|| code.to_string(), |l| l.name().into())
}

/// The states the module's envelope reaches into.
fn envelope_lands(line: &Line) -> Vec<Land> {
    let corners = &line.source.envelope;
    if corners.len() < 3 {
        return Vec::new();
    }
    let (mut min_lat, mut min_lon) = (f64::MAX, f64::MAX);
    let (mut max_lat, mut max_lon) = (f64::MIN, f64::MIN);
    for corner in corners {
        min_lat = min_lat.min(corner.lat);
        min_lon = min_lon.min(corner.lon);
        max_lat = max_lat.max(corner.lat);
        max_lon = max_lon.max(corner.lon);
    }
    Land::touching(min_lat, min_lon, max_lat, max_lon)
}

// ---------------------------------------------------------------------------
// The panel
// ---------------------------------------------------------------------------

/// The list of fields, and the properties of the one selected.
///
/// `month`/`day` are the date the map is being shown on: the swatches are the
/// crops *as they look today*, so a colour picked out of the window is found in
/// the list. In late June that makes half of them the same green, which is the
/// truth about late June — the name is what tells the rows apart, and in
/// October the swatches do it themselves.
pub fn rows(
    ui: &mut egui::Ui,
    line: &mut Line,
    state: &mut EditorState,
    focus: &mut Focus,
    import: &mut FieldImport,
    month: u32,
    day: u32,
) {
    ui.horizontal(|ui| {
        if ui.button(t!("action-import-fields")).clicked() {
            import.open = true;
            import.load_overrides();
        }
        ui.label(
            egui::RichText::new(t!("field-count", count = line.source.fields.len()))
                .color(colors::TEXT_SECONDARY),
        );
    });

    // What the module was built against — the state of the register, which is
    // what makes a line reproducible and its licences honourable.
    if !line.source.field_sources.is_empty() {
        ui.add_space(space::XS);
        for stamp in &line.source.field_sources {
            let name = origin_name(&stamp.land);
            let text = match stamp.year {
                Some(year) => t!("field-source-row", land = name, year = year.to_string()),
                None => name,
            };
            ui.label(
                egui::RichText::new(text)
                    .small()
                    .color(colors::TEXT_SECONDARY),
            );
        }
    }

    if line.source.fields.is_empty() {
        ui.add_space(space::S);
        ui.small(t!("field-list-empty"));
        return;
    }

    ui.add_space(space::S);
    // Not every field, one by one: a module can hold thousands, and a list that
    // long is not a list. The crops are, and clicking one jumps to the first
    // field of it.
    let mut counts: std::collections::BTreeMap<String, (usize, usize)> =
        std::collections::BTreeMap::new();
    for (index, field) in line.source.fields.iter().enumerate() {
        let entry = counts.entry(field.crop.clone()).or_insert((0, index));
        entry.0 += 1;
    }
    editor_ui::form_grid("field-crop-list")
        .num_columns(3)
        .min_col_width(0.0)
        .show(ui, |ui| {
            for (crop, (count, first)) in &counts {
                let class = CropClass::from_id(crop);
                let [r, g, b] = class.map_or([128, 128, 128], |c| crop_colour(c, month, day));
                ui.label(egui::RichText::new("\u{25a0}").color(egui::Color32::from_rgb(r, g, b)));
                let label = class.map_or_else(|| crop.clone(), |c| t!(&c.key()));
                if ui.selectable_label(false, label).clicked() {
                    state.selection = Selection::Field(*first);
                    state.jump_to = Some("selection");
                    if let Some(field) = line.source.fields.get(*first) {
                        let (lat, lon) = field.centre();
                        focus.position = geo::to_ecef_deg(lat, lon, focus.height);
                    }
                }
                ui.label(
                    egui::RichText::new(count.to_string())
                        .small()
                        .color(colors::TEXT_SECONDARY),
                );
                ui.end_row();
            }
        });
}

/// The selected field's own properties.
///
/// `month`/`day` are the date the map is being shown on, so the panel can say
/// what the field is *doing* — a crop name alone does not answer "why is that
/// one brown". That is the whole of the phenology, in one row.
pub fn selection_rows(
    ui: &mut egui::Ui,
    line: &mut Line,
    state: &EditorState,
    zone: u8,
    month: u32,
    day: u32,
) {
    let Selection::Field(index) = state.selection else {
        return;
    };
    let Some(field) = line.source.fields.get_mut(index) else {
        return;
    };
    editor_ui::form_grid("field-properties")
        .num_columns(2)
        .show(ui, |ui| {
            crate::ui::row(ui, "field-crop", |ui| {
                let current = CropClass::from_id(&field.crop);
                let label = current.map_or_else(|| field.crop.clone(), |c| t!(&c.key()));
                egui::ComboBox::from_id_salt("field-crop")
                    .width(space::FIELD * 2.0)
                    .selected_text(label)
                    .show_ui(ui, |ui| {
                        for crop in CROPS {
                            let selected = current == Some(crop);
                            if ui.selectable_label(selected, t!(&crop.key())).clicked() {
                                field.crop = crop.id().to_string();
                                // A crop changed by hand is no longer what the
                                // register said, and the label would lie.
                                field.label.clear();
                            }
                        }
                    });
            });
            crate::ui::row(ui, "field-direction", |ui| {
                editor_ui::field(ui, &mut field.direction_deg, 1.0, -90.0..=90.0, "°");
            });
            crate::ui::row(ui, "field-area", |ui| {
                ui.label(format!("{:.2} ha", field.area(zone) / 10_000.0));
            });
            // What it looks like today. Not stored anywhere — the crop, the
            // date and the field's seed are all it takes.
            if let Some(crop) = CropClass::from_id(&field.crop) {
                let growth = fields::phenology::growth(crop, month, day, field.seed);
                crate::ui::row(ui, "field-growth", |ui| {
                    ui.label(t!(growth.stage.key()));
                    ui.label(
                        egui::RichText::new(t!(
                            "field-growth-detail",
                            cover = format!("{:.0}", growth.cover * 100.0),
                            height = format!("{:.2}", growth.height)
                        ))
                        .small()
                        .color(colors::TEXT_SECONDARY),
                    );
                });
            }
        });

    // How the crop came to be known. A drawn one is a plausible guess, and a
    // builder correcting a module by hand has to be able to see which those are.
    let level = match field.level.as_str() {
        "declared" => Some(("field-level-declared", colors::TEXT_SECONDARY)),
        "group" => Some(("field-level-group", colors::WARN)),
        "drawn" => Some(("field-level-drawn", colors::WARN)),
        _ => None,
    };
    if let Some((key, colour)) = level {
        ui.add_space(space::XS);
        ui.label(egui::RichText::new(t!(key)).small().color(colour));
    }

    // Where the row came from. Read-only, and the reason a wrong crop can be
    // traced to a code rather than argued about.
    if !field.source.is_empty() {
        ui.add_space(space::XS);
        let land = origin_name(&field.source);
        let mut origin = match field.year {
            Some(year) => t!("field-source-row", land = land, year = year.to_string()),
            None => land,
        };
        if !field.code.is_empty() {
            origin.push_str(&format!(" · {}", field.code));
        }
        if !field.label.is_empty() {
            origin.push_str(&format!(" · {}", field.label));
        }
        ui.label(
            egui::RichText::new(origin)
                .small()
                .color(colors::TEXT_SECONDARY),
        );
    }
    ui.add_space(space::XS);
    editor_ui::tag_editor(ui, "field-tags", &mut field.tags);
}

/// The swatch for a crop on a day: the crop's colour over bare soil, mixed by
/// how much of the ground it covers — the same sum the shader does, so the
/// square beside a row is the field and not an idea of it.
pub fn crop_colour(crop: CropClass, month: u32, day: u32) -> [u8; 3] {
    let growth = fields::phenology::growth(crop, month, day, 0);
    let soil = [0.29f32, 0.22, 0.16];
    std::array::from_fn(|i| {
        let mixed = soil[i] + (growth.color[i] - soil[i]) * growth.cover;
        (mixed.clamp(0.0, 1.0) * 255.0) as u8
    })
}

// ---------------------------------------------------------------------------
// Drawing one by hand
// ---------------------------------------------------------------------------

/// The field under a world point, if any — a plain point-in-polygon test over
/// the outlines. Cheap enough for thousands of fields, and it does not need
/// the screen: a field is a piece of ground, and what is wanted is the piece
/// the click landed on.
pub fn pick(line: &Line, p: EcefPos) -> Option<usize> {
    let (lat, lon, _) = geo::from_ecef(p);
    let (lat, lon) = (lat.to_degrees(), lon.to_degrees());
    let point = glam::DVec2::new(lon, lat);
    // The smallest one that holds the point: fields do not overlap, but a
    // re-import that left one behind should still be reachable.
    line.source
        .fields
        .iter()
        .enumerate()
        .filter(|(_, field)| {
            let ring: Vec<glam::DVec2> = field
                .polygon
                .iter()
                .map(|c| glam::DVec2::new(c.lon, c.lat))
                .collect();
            content::terrain::point_in_polygon(point, &ring)
        })
        .min_by(|a, b| {
            let size = |f: &FieldSource| {
                let (lo, hi) = f.polygon.iter().fold(
                    ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN)),
                    |(lo, hi), c| {
                        (
                            (lo.0.min(c.lat), lo.1.min(c.lon)),
                            (hi.0.max(c.lat), hi.1.max(c.lon)),
                        )
                    },
                );
                (hi.0 - lo.0) * (hi.1 - lo.1)
            };
            size(a.1).total_cmp(&size(b.1))
        })
        .map(|(index, _)| index)
}

/// Closes the polygon being drawn into a field. `Some` is a message for the
/// status bar — too few corners, and nothing was made.
pub fn finish(line: &mut Line, state: &mut EditorState) -> Option<String> {
    if state.tool != crate::tools::Tool::PlaceField {
        state.walk_points.clear();
        return None;
    }
    if state.walk_points.len() < 3 {
        return Some(t!("status-field-points"));
    }
    let polygon: Vec<FieldPoint> = state
        .walk_points
        .drain(..)
        .map(|p| {
            let (lat, lon, _) = geo::from_ecef(p);
            FieldPoint {
                lat: lat.to_degrees(),
                lon: lon.to_degrees(),
            }
        })
        .collect();
    // The working direction is the outline's own long axis, exactly as the
    // import takes it — a hand-drawn field furrows the way it is shaped.
    let zone = state.terrain_options().zone;
    let ring: Vec<glam::DVec2> = polygon
        .iter()
        .map(|p| {
            let (e, n) = geo::to_utm(p.lat.to_radians(), p.lon.to_radians(), zone);
            glam::DVec2::new(e, n)
        })
        .collect();
    let direction_deg = fields::geometry::min_area_rect(&ring)
        .map(|rect| rect.angle.to_degrees())
        .unwrap_or(0.0);
    // A seed from the outline, so two hand-drawn fields differ and the same
    // one keeps its tint when the module is reopened.
    let seed = fields::model::hash(
        &ring
            .iter()
            .flat_map(|p| {
                let mut bytes = ((p.x * 100.0).round() as i64).to_le_bytes().to_vec();
                bytes.extend_from_slice(&((p.y * 100.0).round() as i64).to_le_bytes());
                bytes
            })
            .collect::<Vec<u8>>(),
    );
    line.source.fields.push(FieldSource {
        polygon,
        crop: state
            .field_crop
            .unwrap_or(CropClass::WinterCereal)
            .id()
            .to_string(),
        code: String::new(),
        label: String::new(),
        level: String::new(),
        direction_deg,
        source: String::new(),
        year: None,
        seed,
        tags: Vec::new(),
    });
    state.selection = Selection::Field(line.source.fields.len() - 1);
    state.walk_vertex = None;
    None
}

/// The field tool's own options: which crop the next one gets.
pub fn tool_rows(ui: &mut egui::Ui, state: &mut EditorState) {
    editor_ui::form_grid("field-tool")
        .num_columns(2)
        .show(ui, |ui| {
            crate::ui::row(ui, "field-crop", |ui| {
                let current = state.field_crop.unwrap_or(CropClass::WinterCereal);
                egui::ComboBox::from_id_salt("field-tool-crop")
                    .width(space::FIELD * 2.0)
                    .selected_text(t!(&current.key()))
                    .show_ui(ui, |ui| {
                        for crop in CROPS {
                            if ui
                                .selectable_label(current == crop, t!(&crop.key()))
                                .clicked()
                            {
                                state.field_crop = Some(crop);
                            }
                        }
                    });
            });
        });
    if !state.walk_points.is_empty() {
        ui.add_space(space::XS);
        ui.small(t!("field-active", corners = state.walk_points.len()));
    }
}

// ---------------------------------------------------------------------------
// The map
// ---------------------------------------------------------------------------

/// The corners of a field in world coordinates, at the module's own height —
/// like the envelope, and for the same reason: an outline is a closed line that
/// has to keep its shape, and a corner taking the ground under it would drag
/// the line into every hollow.
pub fn positions(line: &Line, focus: &Focus, index: usize) -> Vec<EcefPos> {
    let height = crate::envelope::height(line, focus);
    line.source
        .fields
        .get(index)
        .map(|field| {
            field
                .polygon
                .iter()
                .map(|p| geo::to_ecef_deg(p.lat, p.lon, height))
                .collect()
        })
        .unwrap_or_default()
}

/// Where a field's corner is — what the rule check's "centre" button flies to.
pub fn vertex_pos(line: &Line, focus: &Focus, index: usize, vertex: usize) -> Option<EcefPos> {
    positions(line, focus, index).get(vertex).copied()
}

/// Outlines every field on the map.
///
/// The surfaces themselves are drawn by the terrain (`world_render::farmland`);
/// this is the editable object on top of them — thin, and only fully lit for
/// the field being worked on, so a module with two thousand fields is not a
/// green mesh of lines.
pub fn draw_outlines(
    gizmos: &mut Gizmos,
    line: &Line,
    state: &EditorState,
    focus: &Focus,
    origin: &RenderOrigin,
) {
    if line.source.fields.is_empty() {
        return;
    }
    let selected = match state.selection {
        Selection::Field(index) => Some(index),
        _ => None,
    };
    // Only what is near the view point: the outline of a field ten kilometres
    // off is a pixel, and there are thousands of them.
    let (view_lat, view_lon, _) = geo::from_ecef(focus.position);
    let reach = (focus.height * 2.0).clamp(500.0, 6_000.0);
    let per_degree = 111_320.0;

    for (index, field) in line.source.fields.iter().enumerate() {
        let here = selected == Some(index);
        if !here {
            let (lat, lon) = field.centre();
            let dlat = (lat - view_lat.to_degrees()) * per_degree;
            let dlon = (lon - view_lon.to_degrees()) * per_degree * view_lat.cos().abs();
            if dlat * dlat + dlon * dlon > reach * reach {
                continue;
            }
        }
        let colour = if here {
            COLOR_SELECTED
        } else if state.tool == crate::tools::Tool::PlaceField {
            COLOR
        } else {
            COLOR_IDLE
        };
        let ring = positions(line, focus, index);
        for pair in ring
            .iter()
            .zip(ring.iter().cycle().skip(1))
            .take(ring.len())
        {
            gizmos.line(origin.to_render(*pair.0), origin.to_render(*pair.1), colour);
        }
        // The working direction, as a stroke across the middle of the selected
        // field — the one property that is impossible to judge from a number.
        if here && ring.len() >= 3 {
            let (lat, lon) = field.centre();
            let height = crate::envelope::height(line, focus);
            let centre = geo::to_ecef_deg(lat, lon, height);
            let span = ring
                .iter()
                .map(|p| p.distance(centre))
                .fold(0.0f64, f64::max)
                * 0.8;
            let angle = field.direction_deg.to_radians();
            let frame = world_coords::EnuFrame::at(centre);
            let offset = glam::DVec3::new(angle.cos() * span, angle.sin() * span, 0.0);
            gizmos.line(
                origin.to_render(frame.to_ecef(offset)),
                origin.to_render(frame.to_ecef(-offset)),
                COLOR_SELECTED,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use content::route::{EnvelopePoint, LineSource};

    fn field(lat: f64, lon: f64, size: f64, crop: &str, source: &str) -> FieldSource {
        FieldSource {
            polygon: vec![
                FieldPoint { lat, lon },
                FieldPoint {
                    lat,
                    lon: lon + size,
                },
                FieldPoint {
                    lat: lat + size,
                    lon: lon + size,
                },
                FieldPoint {
                    lat: lat + size,
                    lon,
                },
            ],
            crop: crop.into(),
            code: String::new(),
            label: String::new(),
            level: String::new(),
            direction_deg: 0.0,
            source: source.into(),
            year: Some(2026),
            seed: 1,
            tags: Vec::new(),
        }
    }

    fn line_with(fields: Vec<FieldSource>) -> Line {
        Line {
            source: LineSource {
                fields,
                envelope: vec![
                    EnvelopePoint {
                        lat: 51.5,
                        lon: 8.0,
                    },
                    EnvelopePoint {
                        lat: 51.5,
                        lon: 8.2,
                    },
                    EnvelopePoint {
                        lat: 51.7,
                        lon: 8.2,
                    },
                    EnvelopePoint {
                        lat: 51.7,
                        lon: 8.0,
                    },
                ],
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

    fn report_with(fields: Vec<FieldFeature>) -> ImportReport {
        ImportReport {
            fields,
            ..Default::default()
        }
    }

    fn imported(id: &str) -> FieldFeature {
        FieldFeature {
            polygon: vec![
                glam::DVec2::new(440_000.0, 5_715_000.0),
                glam::DVec2::new(440_200.0, 5_715_000.0),
                glam::DVec2::new(440_200.0, 5_715_200.0),
                glam::DVec2::new(440_000.0, 5_715_200.0),
            ],
            zone: 32,
            land: Some(Land::Nw),
            year: Some(2026),
            code_raw: "115".into(),
            code_text: "Winterweichweizen".into(),
            crop: CropClass::WinterCereal,
            level: fields::Level::Declared,
            direction: 0.25,
            area_ha: 4.0,
            organic: Some(false),
            id: id.into(),
        }
    }

    #[test]
    fn committing_a_module_import_keeps_hand_drawn_fields() {
        let mut line = line_with(vec![
            field(51.55, 8.10, 0.001, "maize", ""),
            field(51.56, 8.10, 0.001, "grassland", "NW"),
        ]);
        let mut selection = Selection::None;
        commit(
            &mut line,
            &report_with(vec![imported("a")]),
            Scope::Module,
            &mut selection,
        );
        // The imported grassland went; the hand-drawn maize stayed.
        assert_eq!(line.source.fields.len(), 2);
        assert_eq!(line.source.fields[0].crop, "maize");
        assert!(line.source.fields[0].source.is_empty());
        assert_eq!(line.source.fields[1].crop, "winter-cereal");
        assert_eq!(line.source.fields[1].source, "NW");
    }

    #[test]
    fn committing_a_selection_replaces_only_that_field() {
        let mut line = line_with(vec![
            field(51.55, 8.10, 0.001, "maize", "NW"),
            field(51.56, 8.10, 0.001, "grassland", "NW"),
        ]);
        let mut selection = Selection::Field(0);
        commit(
            &mut line,
            &report_with(vec![imported("a")]),
            Scope::Selection,
            &mut selection,
        );
        assert_eq!(line.source.fields.len(), 2);
        // The grassland is untouched; the maize was replaced by the import.
        assert_eq!(line.source.fields[0].crop, "grassland");
        assert_eq!(line.source.fields[1].crop, "winter-cereal");
        assert_eq!(selection, Selection::None);
    }

    #[test]
    fn a_committed_field_keeps_what_it_came_from() {
        let mut line = line_with(Vec::new());
        let mut selection = Selection::None;
        commit(
            &mut line,
            &report_with(vec![imported("a")]),
            Scope::Module,
            &mut selection,
        );
        let field = &line.source.fields[0];
        assert_eq!(field.source, "NW");
        assert_eq!(field.year, Some(2026));
        assert_eq!(field.code, "115");
        assert_eq!(field.label, "Winterweichweizen");
        assert_eq!(field.polygon.len(), 4);
        // The direction survives as degrees, and the seed is the parcel's.
        assert!((field.direction_deg - 0.25f64.to_degrees()).abs() < 1e-9);
        assert_ne!(field.seed, 0);
    }

    #[test]
    fn a_second_import_of_a_state_replaces_its_stamp() {
        let mut line = line_with(Vec::new());
        let mut selection = Selection::None;
        let mut report = report_with(vec![imported("a")]);
        report.stamps.push(fields::cache::Stamp {
            land: "NW".into(),
            year: Some(2025),
            fetched: 1,
        });
        commit(&mut line, &report, Scope::Module, &mut selection);
        report.stamps[0].year = Some(2026);
        report.stamps[0].fetched = 2;
        commit(&mut line, &report, Scope::Module, &mut selection);
        assert_eq!(line.source.field_sources.len(), 1);
        assert_eq!(line.source.field_sources[0].year, Some(2026));
    }

    #[test]
    fn the_module_scope_reaches_the_states_under_the_envelope() {
        let line = line_with(Vec::new());
        assert_eq!(envelope_lands(&line), vec![Land::Nw]);
        // A module with no envelope has nothing to import into.
        let mut bare = line_with(Vec::new());
        bare.source.envelope.clear();
        assert!(envelope_lands(&bare).is_empty());
    }

    #[test]
    fn a_swatch_follows_the_date() {
        // Winter cereal is green in May and bare soil in September; a swatch
        // that did not move with the calendar would not match the map.
        let may = crop_colour(CropClass::WinterCereal, 5, 15);
        let september = crop_colour(CropClass::WinterCereal, 9, 20);
        assert_ne!(may, september);
        assert!(may[1] > may[0], "May is green: {may:?}");
        assert!(
            september[0] > september[1],
            "September is soil: {september:?}"
        );
    }

    #[test]
    fn autumn_tells_the_crops_apart() {
        // Late June is a wall of green and honestly so; by October the beet is
        // still standing, the cereal is bare and the rape is up again.
        let mut seen = std::collections::HashSet::new();
        for crop in CropClass::ALL {
            seen.insert(crop_colour(crop, 10, 10));
        }
        assert!(seen.len() >= 10, "{} distinct swatches", seen.len());
    }
}
