//! Desktop UI of the route editor: menu bar, tool panel, status bar.
//!
//! The editor is an application, not a game screen — everything reachable through the
//! keyboard is in the menu as well, and the file dialogs are the operating system's own.

use crate::overlay::Overlay;
use crate::tools::{self, EditorState, Highlight, Selection, Tool};
use crate::{Focus, Ghost, History, Line, Request, TrackObjects, TrackTypes, focus_degrees};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use content::LineSource;
use content::route::{
    BoundarySource, FlankSource, NodeSource, RouteSource, RuleIssue, SectionSource, SignalSource,
};
use editor_ui::{colors, space};
use i18n::t;
use imagery::ZoomMode;
use sim_core::interlock::{BlockMarkerPayload, SignalKind, SignalSystem};
use sim_core::safety::de::{LzbSection, MagnetFrequency, MagnetPayload};
use std::path::{Path, PathBuf};
use track_model::{DeviceKind, Facing, SwitchPosition};
use world_coords::EcefPos;

const SHORTCUT_NEW: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::N);
const SHORTCUT_OPEN: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::O);
const SHORTCUT_SAVE: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::S);
const SHORTCUT_UNDO: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Z);
/// Both spellings of redo — Ctrl+Y is the Windows one, Ctrl+Shift+Z comes
/// from everywhere else.
const SHORTCUT_REDO: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Y);
const SHORTCUT_REDO_ALT: egui::KeyboardShortcut = egui::KeyboardShortcut::new(
    egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT),
    egui::Key::Z,
);
/// The content drawer, on Unreal's own binding.
const SHORTCUT_DRAWER: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Space);

/// One frame of UI. Panels live inside a background `Ui` (egui 0.35).
#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut contexts: EguiContexts,
    mut request: ResMut<Request>,
    mut overlay: ResMut<Overlay>,
    mut focus: ResMut<Focus>,
    mut line: ResMut<Line>,
    mut history: ResMut<History>,
    mut state: ResMut<EditorState>,
    mut ghost: ResMut<Ghost>,
    ground: crate::terrain::Ground,
    mut gizmo: ResMut<crate::gizmo::GizmoState>,
    mut sky: ResMut<world_render::sky::Sky>,
    mut catalogs: crate::Catalogs,
    mut themed: Local<bool>,
    mut active: Local<Option<&'static str>>,
    windows: Query<&bevy::window::RawHandleWrapper, With<bevy::window::PrimaryWindow>>,
    mut exit: MessageWriter<AppExit>,
) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
    if !*themed {
        // Fonts installed by `apply` become active with the next pass — skip
        // one frame so nothing draws with a font family that is not there yet.
        editor_ui::apply(&ctx);
        *themed = true;
        return Ok(());
    }
    state.window = windows.single().ok().cloned();
    handle_shortcuts(
        &ctx,
        &mut line,
        &mut history,
        &mut state,
        &mut overlay,
        &mut request,
    );

    let mut root = egui::Ui::new(
        ctx.clone(),
        "viewport".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );

    menu_bar(
        &mut root,
        &mut line,
        &mut history,
        &mut state,
        &mut overlay,
        &mut request,
        &mut exit,
    );
    status_bar(
        &mut root, &line, &mut state, &overlay, &focus, &ground, &mut sky,
    );
    // Over the status bar and under the side panel, so it spans the window the
    // way Unreal's does — the catalogue is not a property of the selection.
    crate::content_drawer::draw(&mut root, &mut state, &mut catalogs);
    left_panel(
        &mut root,
        &mut line,
        &mut state,
        &mut ghost,
        &catalogs.types,
        &catalogs.objects,
        &mut overlay,
        &mut request,
        &mut focus,
        &mut sky,
        &ground.marks,
        &mut active,
    );

    // Docked into the space the side panel leaves, so it takes its width from
    // the viewport and its clicks never reach the tools underneath.
    viewport_bar(&mut root, &mut focus, &mut gizmo, &overlay, &mut request);

    // The rect the panels leave free, and whether a text field owns the
    // keyboard — the input systems read both from here: the hand-built panel
    // layout is invisible to egui's own pointer hit test.
    let free = root.available_rect_before_wrap();
    state.viewport = Rect::new(free.min.x, free.min.y, free.max.x, free.max.y);
    // A floating window — the new-module dialog, an open menu, a tooltip — is
    // not part of the hand-built layout, so it is not cut out of `free`. egui
    // knows where it is; without asking, a wheel over the dialog zooms the map
    // underneath it as well.
    state.pointer_over_ui = ctx
        .pointer_interact_pos()
        .and_then(|p| ctx.layer_id_at(p))
        .is_some_and(|layer| layer.order != egui::Order::Background);
    state.typing = ctx.memory(|m| m.focused().is_some());
    viewport_hint(&ctx, &root, &state);
    Ok(())
}

/// Says how to move the camera, in the viewport, until the user has done it.
///
/// The viewport is the largest thing on screen and the only region with no
/// visible control at all; right-drag to look and WASD to fly are 3D-editor
/// conventions, not something a modder arriving from a text editor knows. Once
/// the camera has moved or a tool has been used, the hint has done its job and
/// goes.
fn viewport_hint(ctx: &egui::Context, root: &egui::Ui, state: &EditorState) {
    let free = root.available_rect_before_wrap();
    if state.map_used || free.width() < 240.0 {
        return;
    }
    let mut ui = egui::Ui::new(
        ctx.clone(),
        "viewport-hint".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(free.shrink(space::M)),
    );
    ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
        // On a card, not on a text highlight: the hint sits over the aerial
        // imagery, where secondary grey vanishes against a sunlit field, and
        // `background_color` paints the glyph boxes only — a ragged bar that
        // reads as a selection rather than as a note.
        editor_ui::card_frame().show(ui, |ui| {
            ui.label(
                egui::RichText::new(t!("help-fly"))
                    .small()
                    .color(colors::TEXT_SECONDARY),
            );
        });
    });
}

/// The viewport's own toolbar: gizmo mode, what the ground shows, and the
/// camera speed. These belong to *looking* rather than to the document,
/// so they sit on the viewport instead of in the form panel — and docking the
/// bar into the free space rather than floating it over the map means
/// `state.viewport` shrinks by itself, so a click on it can never also reach
/// the tool underneath.
fn viewport_bar(
    root: &mut egui::Ui,
    focus: &mut Focus,
    gizmo: &mut crate::gizmo::GizmoState,
    overlay: &Overlay,
    request: &mut Request,
) {
    use crate::gizmo::GizmoMode;
    use editor_ui::{Icon, bar_divider, icon_button, icon_label};

    egui::Panel::top("viewport-bar")
        .frame(editor_ui::bar_frame())
        .show(root, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::XS;
                // What the selection's handles do.
                for (icon, mode, key) in [
                    (Icon::Move, GizmoMode::Translate, "gizmo-move"),
                    (Icon::Rotate, GizmoMode::Rotate, "gizmo-rotate"),
                ] {
                    if icon_button(ui, icon, gizmo.mode == mode, t!(key)).clicked() {
                        gizmo.mode = mode;
                    }
                }
                bar_divider(ui);
                // What lies on the ground. The imagery is switched often enough
                // while building that burying it in a panel section costs a
                // scroll every time — the ground it drapes over is always drawn.
                let shown = overlay.config().enabled;
                if icon_button(ui, Icon::Imagery, shown, t!("view-imagery")).clicked() {
                    let mut config = overlay.config().clone();
                    config.enabled = !shown;
                    request.config = Some((config, false));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    camera_speed(ui, focus);
                    icon_label(ui, Icon::Speed);
                });
            });
        });
}

/// Unreal's camera speed dial: the step on the button, the eight steps and the
/// fine multiplier in the menu behind it.
///
/// The dial is on the bar and not in the form panel because the mouse already
/// carries it — right button plus wheel is the same value, and someone who
/// found it there needs to see the number it landed on without leaving the
/// viewport.
fn camera_speed(ui: &mut egui::Ui, focus: &mut Focus) {
    use crate::view::{DEFAULT_SPEED_STEP, MAX_SPEED_SCALAR, SPEED_STEPS};

    let button = editor_ui::bar_menu(
        ui,
        focus.speed_step.to_string(),
        t!(
            "camera-speed-hint",
            speed = format!("{:.0}", focus.fly_speed())
        ),
    );
    egui::Popup::menu(&button)
        .layout(egui::Layout::top_down(egui::Align::Min))
        .show(|ui| {
            ui.set_min_width(space::LABEL_COL + space::XL);
            ui.label(egui::RichText::new(t!("camera-speed")).color(colors::TEXT_SECONDARY));
            ui.add(egui::Slider::new(&mut focus.speed_step, 1..=SPEED_STEPS).integer());
            ui.label(egui::RichText::new(t!("camera-speed-scalar")).color(colors::TEXT_SECONDARY));
            // Logarithmic, because the useful part of 1…128 is its bottom end:
            // a linear rail spends nine tenths of its travel above 12x.
            ui.add(
                egui::Slider::new(&mut focus.speed_scalar, 1.0..=MAX_SPEED_SCALAR)
                    .logarithmic(true)
                    .max_decimals(1),
            );
            ui.add_space(space::XS);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(t!(
                        "camera-speed-value",
                        speed = format!("{:.0}", focus.fly_speed())
                    ))
                    .color(colors::TEXT),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(t!("action-reset")).clicked() {
                        focus.speed_step = DEFAULT_SPEED_STEP;
                        focus.speed_scalar = 1.0;
                    }
                });
            });
        });
}

fn handle_shortcuts(
    ctx: &egui::Context,
    line: &mut Line,
    history: &mut History,
    state: &mut EditorState,
    overlay: &mut Overlay,
    request: &mut Request,
) {
    if ctx.input_mut(|i| i.consume_shortcut(&SHORTCUT_DRAWER)) {
        state.drawer.open = !state.drawer.open;
    }
    // Redo first: Ctrl+Shift+Z must not be eaten by the plain Ctrl+Z.
    if ctx
        .input_mut(|i| i.consume_shortcut(&SHORTCUT_REDO) || i.consume_shortcut(&SHORTCUT_REDO_ALT))
    {
        redo(line, history, state);
    }
    if ctx.input_mut(|i| i.consume_shortcut(&SHORTCUT_UNDO)) {
        undo(line, history, state);
    }
    if ctx.input_mut(|i| i.consume_shortcut(&SHORTCUT_SAVE)) && needs_saving(line) {
        save(line, state, overlay);
    }
    if ctx.input_mut(|i| i.consume_shortcut(&SHORTCUT_OPEN))
        && confirm_discard(line, state, overlay)
    {
        open(state);
    }
    if ctx.input_mut(|i| i.consume_shortcut(&SHORTCUT_NEW)) && confirm_discard(line, state, overlay)
    {
        request.new_module = true;
    }
}

/// Stepping through the history ends whatever interaction was running and
/// drops a selection that may now point at something else.
fn undo(line: &mut Line, history: &mut History, state: &mut EditorState) {
    history.undo(line);
    state.selection = Selection::None;
    state.drawing = None;
    state.drag = None;
    state.marked.clear();
}

fn redo(line: &mut Line, history: &mut History, state: &mut EditorState) {
    history.redo(line);
    state.selection = Selection::None;
    state.drawing = None;
    state.drag = None;
    state.marked.clear();
}

/// The owner of every native dialog. Without one, Windows is free to open the
/// dialog behind the editor window — where a modal that blocks all input reads
/// as a hang.
fn dialog_parent(state: &EditorState) -> Option<bevy::window::ThreadLockedRawWindowHandleWrapper> {
    // SAFETY: the handle is only handed to rfd as the dialog owner; nothing
    // draws or resizes through it.
    state.window.as_ref().map(|w| unsafe { w.get_handle() })
}

fn message_dialog(state: &EditorState) -> rfd::MessageDialog {
    match dialog_parent(state) {
        Some(parent) => rfd::MessageDialog::new().set_parent(&parent),
        None => rfd::MessageDialog::new(),
    }
}

fn file_dialog(state: &EditorState) -> rfd::FileDialog {
    match dialog_parent(state) {
        Some(parent) => rfd::FileDialog::new().set_parent(&parent),
        None => rfd::FileDialog::new(),
    }
}

/// What the file dialog that is up was opened for — the answer arrives frames
/// later, and this is what [`poll_file_dialog`] does with it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileAsk {
    Open,
    ImportForest,
    ImportMarkers,
    Ghost,
    DgmFolder,
}

/// Puts a native file dialog up on a thread of its own and remembers what its
/// answer is for.
///
/// The Windows file dialog runs a message loop of its own while it is open.
/// Started on winit's thread — the one the whole editor runs on — that loop is
/// nested inside the event handling the editor is already in, and the editor
/// stops answering; the dialog itself may never appear. On its own thread it
/// is only a dialog, and the editor keeps drawing behind it.
fn ask_for_file(
    state: &mut EditorState,
    ask: FileAsk,
    pick: impl FnOnce(rfd::FileDialog) -> Option<PathBuf> + Send + 'static,
) {
    // One at a time: a second dialog would be owned by a window the first one
    // has already disabled.
    if state.pending_file.is_some() {
        return;
    }
    let window = state.window.clone();
    let (answer, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // SAFETY: the handle is only handed to rfd as the dialog owner, which
        // is a window handle used as a number — nothing draws or resizes
        // through it, and Windows owns dialogs across threads.
        let parent = window.as_ref().map(|w| unsafe { w.get_handle() });
        let mut dialog = rfd::FileDialog::new();
        if let Some(parent) = &parent {
            dialog = dialog.set_parent(parent);
        }
        answer.send(pick(dialog)).ok();
    });
    state.pending_file = Some((ask, std::sync::Mutex::new(receiver)));
}

/// Takes the answer of the file dialog, once it has arrived, and runs what it
/// was asked for.
pub fn poll_file_dialog(
    mut line: ResMut<Line>,
    mut history: ResMut<History>,
    mut state: ResMut<EditorState>,
    mut overlay: ResMut<Overlay>,
    mut ghost: ResMut<Ghost>,
) {
    let Some((_, receiver)) = &state.pending_file else {
        return;
    };
    let answer = match receiver.lock() {
        Ok(receiver) => receiver.try_recv(),
        // Poisoned: the dialog thread panicked, so no answer is coming.
        Err(_) => Err(std::sync::mpsc::TryRecvError::Disconnected),
    };
    let path = match answer {
        Ok(path) => path,
        Err(std::sync::mpsc::TryRecvError::Empty) => return,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => None,
    };
    let Some((ask, _)) = state.pending_file.take() else {
        return;
    };
    // No path means the dialog was called off, which is not an event.
    let Some(path) = path else {
        return;
    };
    match ask {
        FileAsk::Open => opened(path, &mut line, &mut history, &mut state, &mut overlay),
        FileAsk::ImportForest => forest_imported(path, &mut line, &mut state, &mut overlay),
        FileAsk::ImportMarkers => markers_imported(path, &mut line, &mut state, &mut overlay),
        FileAsk::Ghost => ghost_loaded(path, &mut ghost, &state, &mut overlay),
        FileAsk::DgmFolder => state.dgm_source = Some(path.display().to_string()),
    }
}

/// Asks before unsaved work is thrown away; `true` means go ahead.
pub fn confirm_discard(line: &mut Line, state: &mut EditorState, overlay: &mut Overlay) -> bool {
    if !line.dirty {
        return true;
    }
    match message_dialog(state)
        .set_level(rfd::MessageLevel::Warning)
        .set_title(t!("confirm-unsaved-title"))
        .set_description(t!("confirm-unsaved", name = line.source.name))
        .set_buttons(rfd::MessageButtons::YesNoCancel)
        .show()
    {
        // Saving can still be called off in the file dialog. Nothing was
        // written then, so the answer is no longer yes.
        rfd::MessageDialogResult::Yes => {
            save(line, state, overlay);
            !line.dirty
        }
        rfd::MessageDialogResult::No => true,
        _ => false,
    }
}

/// Whether Save would do anything. Re-writing an unchanged file is not
/// harmless: `ron::ser` re-serialises the struct, so the comments a
/// hand-written line file carries are stripped for nothing.
fn needs_saving(line: &Line) -> bool {
    line.dirty || line.path.is_none()
}

/// Puts a failure in front of the user — the status bar sits in the corner
/// furthest from the menu the action came from.
fn report_failure(state: &EditorState, overlay: &mut Overlay, text: String) {
    overlay.status = text.clone();
    message_dialog(state)
        .set_level(rfd::MessageLevel::Error)
        .set_title(t!("dialog-error-title"))
        .set_description(text)
        .show();
}

/// Warns before a hand-written file loses its comments; asked once per session.
fn confirm_comment_loss(state: &mut EditorState, path: &Path) -> bool {
    if state.warned_about_comments {
        return true;
    }
    let has_comments = std::fs::read_to_string(path)
        .map(|text| text.lines().any(|l| l.trim_start().starts_with("//")))
        .unwrap_or(false);
    if !has_comments {
        return true;
    }
    let go_ahead = message_dialog(state)
        .set_level(rfd::MessageLevel::Warning)
        .set_title(t!("confirm-comments-title"))
        .set_description(t!("confirm-comments", file = path.display()))
        .set_buttons(rfd::MessageButtons::OkCancel)
        .show()
        == rfd::MessageDialogResult::Ok;
    state.warned_about_comments |= go_ahead;
    go_ahead
}

/// Save to the known path, or fall back to the save dialog.
fn save(line: &mut Line, state: &mut EditorState, overlay: &mut Overlay) {
    match line.path.clone() {
        Some(path) => {
            let path = PathBuf::from(path);
            if !confirm_comment_loss(state, &path) {
                return;
            }
            write_line(line, state, overlay, path);
        }
        None => save_as(line, state, overlay),
    }
}

fn save_as(line: &mut Line, state: &mut EditorState, overlay: &mut Overlay) {
    if let Some(path) = file_dialog(state)
        .add_filter(t!("filter-line-ron"), &["ron"])
        .set_file_name("line.ron")
        .save_file()
    {
        write_line(line, state, overlay, path);
    }
}

fn write_line(line: &mut Line, state: &EditorState, overlay: &mut Overlay, path: PathBuf) {
    match std::fs::write(&path, line.source.to_ron()) {
        Ok(()) => {
            overlay.status = t!("status-written", file = path.display());
            line.path = Some(path.display().to_string());
            line.dirty = false;
        }
        Err(e) => report_failure(
            state,
            overlay,
            t!("status-error", file = path.display(), error = e),
        ),
    }
}

/// File ▸ Open module: asks for the file, [`opened`] takes it from there.
fn open(state: &mut EditorState) {
    let filter = t!("filter-line-ron");
    ask_for_file(state, FileAsk::Open, move |dialog| {
        dialog.add_filter(filter, &["ron"]).pick_file()
    });
}

/// The module the user picked, read and put in place of the current one.
fn opened(
    path: PathBuf,
    line: &mut Line,
    history: &mut History,
    state: &mut EditorState,
    overlay: &mut Overlay,
) {
    let parsed = std::fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|text| LineSource::from_ron(&text).map_err(|e| e.to_string()));
    match parsed {
        Ok(source) => match source.compile() {
            Ok(_) => {
                line.source = source;
                line.path = Some(path.display().to_string());
                line.dirty = false;
                line.needs_rebuild = true;
                line.terrain_change = crate::terrain::TerrainChange::all();
                line.recenter = true;
                history.reset(&line.source);
                state.selection = Selection::None;
                state.drawing = None;
                state.picked_tiles.clear();
                // Which tiles this module already has heights for — read once
                // per open, not per frame.
                state.dgm_present = height_dir(line)
                    .map(|(dir, _)| present_tiles(&dir))
                    .unwrap_or_default();
                overlay.status = t!("status-loaded", file = path.display());
            }
            Err(e) => report_failure(
                state,
                overlay,
                t!("status-compile-error", error = format!("{e:?}")),
            ),
        },
        Err(e) => report_failure(
            state,
            overlay,
            t!("status-error", file = path.display(), error = e),
        ),
    }
}

/// File ▸ Import forest: reads an Overpass JSON extract and **bakes** its
/// `landuse=forest` / `natural=wood` polygons into single trees — the same
/// fill as the forest brush, so every imported tree is an ordinary [`content::
/// TreeSource`] that can be moved or deleted like a hand-set one. An optional
/// aid; whoever wants every tree hand-set simply never uses it. Species and
/// density come from the vegetation tool options.
fn import_forest(state: &mut EditorState) {
    let filter = t!("filter-overpass-json");
    ask_for_file(state, FileAsk::ImportForest, move |dialog| {
        dialog.add_filter(filter, &["json"]).pick_file()
    });
}

/// The Overpass extract the user picked, baked into trees.
fn forest_imported(path: PathBuf, line: &mut Line, state: &mut EditorState, overlay: &mut Overlay) {
    let parsed = std::fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|text| content::import::parse_forests(&text).map_err(|e| e.to_string()));
    match parsed {
        Ok(polygons) if polygons.is_empty() => {
            overlay.status = t!("status-forest-import-empty", file = path.display());
        }
        Ok(polygons) => {
            let areas = polygons.len();
            let objects: Vec<String> = state.tree_object.iter().cloned().collect();
            let mut baked = 0;
            let mut dropped = 0;
            for polygon in polygons {
                let trees = content::terrain::fill_polygon(
                    &polygon,
                    &objects,
                    state.forest_area.unwrap_or(500.0),
                    line.source.trees.len() as u64,
                    tools::utm_zone_of(polygon[0].1),
                    |lat, lon| tools::clear_of_track(&line.net, lat, lon),
                );
                // An imported wood knows nothing of the module — it is cut to
                // the envelope, or the neighbour inherits a forest.
                let before = trees.len();
                let trees: Vec<_> = trees
                    .into_iter()
                    .filter(|t| line.source.envelope_contains(t.lat, t.lon))
                    .collect();
                dropped += before - trees.len();
                baked += trees.len();
                line.source.trees.extend(trees);
            }
            overlay.status = if dropped == 0 {
                t!("status-forest-imported", count = baked, areas = areas)
            } else {
                t!(
                    "status-forest-imported-clipped",
                    count = baked,
                    areas = areas,
                    dropped = dropped
                )
            };
        }
        Err(e) => report_failure(
            state,
            overlay,
            t!("status-error", file = path.display(), error = e),
        ),
    }
}

/// Where a line's own height tiles live: `<mod>/heights/<line>/`, and the
/// mod-qualified path that goes into the file. `None` when the line has not
/// been saved into a mod yet — height data belongs to a mod, not to a loose
/// file somewhere.
fn height_dir(line: &Line) -> Option<(std::path::PathBuf, String)> {
    let path = std::path::Path::new(line.path.as_ref()?);
    let stem = path.file_stem()?.to_str()?.to_string();
    // `<mod>/lines/<line>.ron` — the mod directory is two levels up.
    let mod_dir = path.parent()?.parent()?;
    let manifest = std::fs::read_to_string(mod_dir.join("mod.ron")).ok()?;
    let id = manifest
        .lines()
        .find_map(|l| l.trim().strip_prefix("id:")?.trim().split('"').nth(1))?
        .to_string();
    Some((
        mod_dir.join("heights").join(&stem),
        format!("{id}:heights/{stem}"),
    ))
}

/// The tiles the module already carries — file name `x<kx>_y<ky>.asc`.
fn present_tiles(dir: &std::path::Path) -> Vec<content::TileKey> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let name = e.path();
            let stem = name.file_stem()?.to_str()?;
            let (x, y) = stem.strip_prefix('x')?.split_once("_y")?;
            Some((x.parse().ok()?, y.parse().ok()?))
        })
        .collect()
}

/// Cuts the module's own height tiles out of a DGM delivery — the whole
/// corridor, or the tiles picked with the tile tool. Every tile becomes one
/// ESRI ASCII grid next to the line, and the line records where they are, so
/// the module carries its ground with it instead of needing `--dgm` at
/// runtime. Tiles the delivery has no data for are skipped rather than shipped
/// as a plate of zeros.
fn import_heights(line: &mut Line, state: &mut EditorState, overlay: &mut Overlay, all: bool) {
    let Some((dir, qualified)) = height_dir(line) else {
        overlay.status = t!("status-heights-need-mod");
        return;
    };
    let Some(source_path) = state.dgm_source.clone() else {
        overlay.status = t!("status-heights-no-source");
        return;
    };
    let zone = state.dgm_zone();
    let path = std::path::Path::new(&source_path);
    let source = if path.is_dir() {
        content::import::dgm::TerrainSource::from_dir(path, zone).map_err(|e| e.to_string())
    } else {
        std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|t| {
                content::import::dgm::HeightTile::parse(&t, zone)
                    .map(content::import::dgm::TerrainSource::from_tile)
                    .map_err(|e| e.to_string())
            })
    };
    let mut source = match source {
        Ok(s) => s,
        Err(e) => {
            report_failure(
                state,
                overlay,
                t!("status-error", file = source_path, error = e),
            );
            return;
        }
    };

    let options = state.terrain_options();
    let tiles = if all || state.picked_tiles.is_empty() {
        tools::corridor_tiles(line, options)
    } else {
        state.picked_tiles.clone()
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        report_failure(
            state,
            overlay,
            t!("status-error", file = dir.display(), error = e.to_string()),
        );
        return;
    }

    let cell = state.dgm_cell();
    let (mut written, mut empty) = (0usize, 0usize);
    for key in tiles {
        let min = content::terrain::tile_min(key, options.tile_size);
        let tile = content::import::dgm::HeightTile::sample(
            std::slice::from_mut(&mut source),
            zone,
            (min.x, min.y),
            options.tile_size,
            cell,
        );
        if tile.is_empty() {
            empty += 1;
            continue;
        }
        let file = dir.join(format!("x{}_y{}.asc", key.0, key.1));
        match std::fs::write(&file, tile.to_asc()) {
            Ok(()) => written += 1,
            Err(e) => {
                report_failure(
                    state,
                    overlay,
                    t!("status-error", file = file.display(), error = e.to_string()),
                );
                return;
            }
        }
    }

    let entry = content::route::HeightSource {
        path: qualified,
        zone,
    };
    if written > 0 && !line.source.heights.contains(&entry) {
        line.source.heights = vec![entry];
    }
    state.dgm_present = present_tiles(&dir);
    state.picked_tiles.clear();
    overlay.status = t!("status-heights-imported", tiles = written, empty = empty);
}

/// File ▸ Import reference markers: reads an Overpass JSON extract and turns
/// the tags it knows into markers, each in the layer of its tag — level
/// crossings, platforms, kilometre marks. They are drawing aids, not
/// equipment: nothing is wired, and every layer can be hidden or deleted
/// again in the marker panel.
fn import_markers(state: &mut EditorState) {
    let filter = t!("filter-overpass-json");
    ask_for_file(state, FileAsk::ImportMarkers, move |dialog| {
        dialog.add_filter(filter, &["json"]).pick_file()
    });
}

/// The Overpass extract the user picked, turned into reference markers.
fn markers_imported(
    path: PathBuf,
    line: &mut Line,
    state: &mut EditorState,
    overlay: &mut Overlay,
) {
    let parsed = std::fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|text| content::import::parse_markers(&text).map_err(|e| e.to_string()));
    match parsed {
        Ok(markers) if markers.is_empty() => {
            overlay.status = t!("status-marker-import-empty", file = path.display());
        }
        Ok(markers) => {
            let count = markers.len();
            let layers: std::collections::BTreeSet<&str> =
                markers.iter().map(|m| m.layer.as_str()).collect();
            let layer_count = layers.len();
            // An imported layer that was hidden before shows itself again —
            // otherwise the import looks like it did nothing.
            for layer in layers {
                state.hidden_layers.remove(layer);
            }
            line.source.markers.extend(markers);
            overlay.status = t!(
                "status-markers-imported",
                count = count,
                layers = layer_count
            );
        }
        Err(e) => report_failure(
            state,
            overlay,
            t!("status-error", file = path.display(), error = e),
        ),
    }
}

/// Empties the document and starts a new module: the name and the anchor from
/// the dialog, and the square envelope built around that anchor.
#[allow(clippy::too_many_arguments)]
pub(crate) fn new_line(
    line: &mut Line,
    history: &mut History,
    state: &mut EditorState,
    name: String,
    anchor: content::route::GeoPoint,
    half_size: f64,
    year: u32,
    fictional: bool,
) {
    line.source = LineSource {
        name,
        year: Some(year),
        fictional,
        anchor: Some(anchor),
        envelope: content::route::default_envelope(anchor, half_size),
        geoid_offset: 46.0,
        electrification: String::new(),
        nodes: vec![],
        edges: vec![],
        devices: vec![],
        objects: vec![],
        trees: vec![],
        markers: vec![],
        terrain: vec![],
        heights: vec![],
        sections: vec![],
        areas: Vec::new(),
        signals: vec![],
        routes: vec![],
        boundaries: vec![],
        script: None,
    };
    line.path = None;
    line.dirty = false;
    line.needs_rebuild = true;
    line.terrain_change = crate::terrain::TerrainChange::all();
    // No recenter: the new track is drawn wherever the view already is.
    line.recenter = false;
    history.reset(&line.source);
    state.selection = Selection::None;
    state.drawing = None;
}

#[allow(clippy::too_many_arguments)]
fn menu_bar(
    root: &mut egui::Ui,
    line: &mut Line,
    history: &mut History,
    state: &mut EditorState,
    overlay: &mut Overlay,
    request: &mut Request,
    exit: &mut MessageWriter<AppExit>,
) {
    egui::Panel::top("menu")
        .frame(editor_ui::bar_frame())
        .show(root, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button(t!("menu-file"), |ui| {
                    if ui.button(t!("action-new-line")).clicked() {
                        ui.close();
                        if confirm_discard(line, state, overlay) {
                            request.new_module = true;
                        }
                    }
                    if ui.button(t!("action-open-line")).clicked() {
                        ui.close();
                        if confirm_discard(line, state, overlay) {
                            open(state);
                        }
                    }
                    ui.separator();
                    let save_button = egui::Button::new(t!("action-save"));
                    if ui.add_enabled(needs_saving(line), save_button).clicked() {
                        ui.close();
                        save(line, state, overlay);
                    }
                    if ui.button(t!("action-save-as")).clicked() {
                        ui.close();
                        save_as(line, state, overlay);
                    }
                    ui.separator();
                    if ui.button(t!("action-import-forest")).clicked() {
                        ui.close();
                        import_forest(state);
                    }
                    if ui.button(t!("action-import-markers")).clicked() {
                        ui.close();
                        import_markers(state);
                    }
                    ui.separator();
                    if ui.button(t!("action-load-imagery")).clicked() {
                        request.load_config = true;
                        ui.close();
                    }
                    if ui.button(t!("action-save-imagery")).clicked() {
                        request.save_config = true;
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(t!("action-quit")).clicked() {
                        ui.close();
                        if confirm_discard(line, state, overlay) {
                            exit.write(AppExit::Success);
                        }
                    }
                });
                ui.menu_button(t!("menu-edit"), |ui| {
                    let undo_button = egui::Button::new(t!("action-undo"));
                    if ui
                        .add_enabled(!history.undo.is_empty(), undo_button)
                        .clicked()
                    {
                        undo(line, history, state);
                        ui.close();
                    }
                    let redo_button = egui::Button::new(t!("action-redo"));
                    if ui
                        .add_enabled(!history.redo.is_empty(), redo_button)
                        .clicked()
                    {
                        redo(line, history, state);
                        ui.close();
                    }
                    ui.separator();
                    let has_target = state.selection != Selection::None || !state.marked.is_empty();
                    let delete_button = egui::Button::new(t!("action-delete"));
                    if ui.add_enabled(has_target, delete_button).clicked() {
                        if state.marked.is_empty() {
                            tools::delete_selection(line, state);
                        } else {
                            tools::delete_marked(line, state);
                        }
                        ui.close();
                    }
                });
                ui.menu_button(t!("menu-overlay"), |ui| {
                    if ui.button(t!("overlay-toggle")).clicked() {
                        request.toggle_overlay = true;
                        ui.close();
                    }
                    if ui.button(t!("overlay-next-provider")).clicked() {
                        request.cycle_provider = true;
                        ui.close();
                    }
                    if ui.button(t!("overlay-offline")).clicked() {
                        request.toggle_offline = true;
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(t!("overlay-clear-cache")).clicked() {
                        request.clear_cache = true;
                        ui.close();
                    }
                    if ui.button(t!("overlay-retry")).clicked() {
                        request.retry_failed = true;
                        ui.close();
                    }
                });
                ui.menu_button(t!("menu-view"), |ui| {
                    language_menu(ui);
                });
                ui.menu_button(t!("menu-help"), |ui| {
                    ui.label(t!("help-fly"));
                    ui.label(t!("help-gizmo"));
                    ui.label(t!("help-opacity"));
                    ui.label(t!("help-offset"));
                    ui.label(t!("help-draw"));
                });
            });
        });
}

#[allow(clippy::too_many_arguments)]
fn status_bar(
    root: &mut egui::Ui,
    line: &Line,
    state: &mut EditorState,
    overlay: &Overlay,
    focus: &Focus,
    ground: &crate::terrain::Ground,
    sky: &mut world_render::sky::Sky,
) {
    let terrain = &ground.view;
    egui::Panel::bottom("status")
        .frame(editor_ui::bar_frame())
        .show(root, |ui| {
            ui.horizontal(|ui| {
                // Bottom left, where the drawer comes out, so the catalogue is
                // reachable without a menu.
                if editor_ui::icon_button(
                    ui,
                    editor_ui::Icon::Drawer,
                    state.drawer.open,
                    t!("action-content-drawer"),
                )
                .clicked()
                {
                    state.drawer.open = !state.drawer.open;
                }
                editor_ui::bar_divider(ui);
                // Date and time of day: the light over the module is looked at
                // on the map, not in a form, so its two controls sit where the
                // map is. Both write the same fields as the sky section.
                let mut hours = sky.seconds / 3600.0;
                if editor_ui::day_controls(
                    ui,
                    &mut sky.year,
                    &mut sky.month,
                    &mut sky.day,
                    &mut hours,
                ) {
                    sky.seconds = hours * 3600.0;
                }
                editor_ui::bar_divider(ui);
                // While a track is being drawn, the drawing is the status.
                let status = match &state.drawing {
                    Some(drawing) => t!(
                        if drawing.branch_of.is_some() {
                            "draw-branch"
                        } else {
                            "draw-active"
                        },
                        segments = drawing.segments.len()
                    ),
                    None if overlay.status.is_empty() => t!("status-ready"),
                    None => overlay.status.clone(),
                };
                // The readouts on the right take their width first; the
                // message gets what is left and is cut to it — laid out the
                // other way round, a long message runs under the readouts.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Frame time, entities and tiles: the numbers that say
                    // whether the streaming keeps up with the flight.
                    let (tiles, pending) = terrain.tiles();
                    let perf = ground.perf();
                    ui.label(
                        egui::RichText::new(t!(
                            "status-perf",
                            fps = format!("{:.0}", perf.0),
                            entities = perf.1,
                            tiles = tiles,
                            pending = pending,
                        ))
                        .color(colors::TEXT_SECONDARY),
                    )
                    .on_hover_text(t!("status-perf-hint"));
                    editor_ui::bar_divider(ui);
                    // The ground under the cursor — the height the run will
                    // have there, brush strokes and embankment included.
                    if let Some(height) = terrain.cursor_height {
                        ui.label(t!("status-ground-height", height = format!("{height:.1}")));
                    }
                    let (lat, lon) = focus_degrees(focus.position);
                    ui.label(t!(
                        "status-position",
                        lat = format!("{lat:.5}"),
                        lon = format!("{lon:.5}"),
                        height = format!("{:.0}", focus.height),
                    ));
                    if line.dirty {
                        ui.label(egui::RichText::new(t!("status-unsaved")).color(colors::WARN));
                    }
                    if let Some(path) = &line.path {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(path).color(colors::TEXT_SECONDARY),
                            )
                            .truncate(),
                        );
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(status).truncate());
                    });
                });
            });
        });
}

/// The sections of the panel, in the order they are drawn: id and the i18n
/// key of the title. The jump bar sits above the scroll area and has to name
/// them before the first one has been laid out. Editing first, template
/// configuration after, diagnostics last.
const SECTIONS: [(&str, &str); 9] = [
    ("tools", "heading-tools"),
    ("selection", "heading-selection"),
    ("areas", "heading-areas"),
    ("interlock", "heading-interlock"),
    ("module", "heading-module"),
    ("sky", "heading-sky"),
    ("checks", "heading-checks"),
    ("imagery", "heading-imagery"),
    ("cache", "heading-cache"),
];

/// A section the jump bar can scroll to; also reports itself in `current`
/// while its header is at or above the top of the visible area — the sections
/// are drawn in order, so the last one to do so is the one being read.
/// (Same mechanics as the vehicle editor's data panel.)
fn nav_section(
    ui: &mut egui::Ui,
    jump: Option<&str>,
    current: &mut Option<&'static str>,
    id: &'static str,
    key: &str,
    body: impl FnOnce(&mut egui::Ui),
) {
    let section = editor_ui::section(ui, id, t!(key), body);
    if section.header_response.rect.top() <= ui.clip_rect().top() + space::XL {
        *current = Some(id);
    }
    if jump == Some(id) {
        section.header_response.scroll_to_me(Some(egui::Align::TOP));
    }
}

#[allow(clippy::too_many_arguments)]
fn left_panel(
    root: &mut egui::Ui,
    line: &mut Line,
    state: &mut EditorState,
    ghost: &mut Ghost,
    types: &TrackTypes,
    objects: &TrackObjects,
    overlay: &mut Overlay,
    request: &mut Request,
    focus: &mut Focus,
    sky: &mut world_render::sky::Sky,
    marks: &crate::terrain::Marks,
    active: &mut Option<&'static str>,
) {
    egui::Panel::left("info")
        .default_size(360.0)
        .resizable(true)
        .frame(editor_ui::panel_frame())
        .show(root, |ui| {
            // Heading, name and jump bar stay out of the scroll area, so the
            // name of the line being edited never leaves the screen and every
            // section is findable without scrolling past it.
            ui.label(editor_ui::heading(t!("heading-line")));
            ui.add_space(space::XS);
            ui.add(
                egui::TextEdit::singleline(&mut line.source.name)
                    .hint_text(t!("line-name"))
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new(t!(
                    "line-counts",
                    edges = line.source.edges.len(),
                    devices = line.source.devices.len()
                ))
                .small()
                .color(colors::TEXT_SECONDARY),
            );
            ui.add_space(space::S);
            // A row of another panel may have asked for a section last frame.
            let mut jump = state.jump_to.take();
            // Both the selection and the interlocking panel point at things on
            // the map; whatever the mouse is over this frame wins.
            state.highlight = None;
            ui.horizontal_wrapped(|ui| {
                for (id, key) in SECTIONS {
                    // The section being read wears the "widget pressed" fill
                    // and strong text; the accent stays for real selections.
                    let here = *active == Some(id);
                    let mut label = egui::RichText::new(t!(key));
                    if here {
                        label = label.color(colors::TEXT_STRONG);
                    }
                    let mut chip = egui::Button::new(label).small();
                    if here {
                        chip = chip.fill(colors::BG_ACTIVE);
                    }
                    if ui.add(chip).clicked() {
                        jump = Some(id);
                    }
                }
            });

            let mut current = None;
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    nav_section(ui, jump, &mut current, "tools", "heading-tools", |ui| {
                        tool_chips(ui, state);
                        if state.tool == Tool::PlaceDevice {
                            ui.add_space(space::XS);
                            editor_ui::form_grid("place").show(ui, |ui| {
                                row(ui, "dev-kind", |ui| {
                                    let mut kind = state.device_kind();
                                    kind_combo(ui, "place-kind", &mut kind);
                                    state.device_kind = Some(kind);
                                });
                            });
                        }
                        if state.tool == Tool::MarkArea {
                            ui.add_space(space::XS);
                            ui.small(t!("tool-area-drag"));
                            editor_ui::form_grid("area-brush").show(ui, |ui| {
                                row(ui, "area-width", |ui| {
                                    let mut width = state.area_width.unwrap_or(2.5);
                                    if editor_ui::field(ui, &mut width, 0.1, 0.5..=20.0, "m")
                                        .changed()
                                    {
                                        state.area_width = Some(width);
                                    }
                                });
                            });
                            if let Selection::TrackArea(i) = state.selection
                                && let Some(area) = line.source.areas.get(i)
                            {
                                ui.small(t!(
                                    "tool-area-joins",
                                    name = if area.name.is_empty() {
                                        t!("area-unnamed")
                                    } else {
                                        area.name.clone()
                                    }
                                ));
                            }
                        }
                        if state.tool == Tool::PlaceSwitch {
                            ui.add_space(space::XS);
                            editor_ui::form_grid("place-switch").show(ui, |ui| {
                                row(ui, "switch-orientation", |ui| {
                                    for (trailing, key) in
                                        [(false, "switch-facing"), (true, "switch-trailing")]
                                    {
                                        if ui
                                            .selectable_label(
                                                state.switch_trailing == trailing,
                                                t!(key),
                                            )
                                            .on_hover_text(t!(&format!("{key}-hint")))
                                            .clicked()
                                        {
                                            state.switch_trailing = trailing;
                                        }
                                    }
                                });
                            });
                        }
                        if state.tool == Tool::PlaceObject {
                            ui.add_space(space::XS);
                            if objects.map.is_empty() {
                                ui.small(t!("status-no-objects"));
                            } else {
                                editor_ui::form_grid("place-object").show(ui, |ui| {
                                    row(ui, "obj-kind", |ui| {
                                        object_combo(ui, "place-object-kind", objects, state);
                                    });
                                });
                            }
                        }
                        if matches!(state.tool, Tool::PlaceTree | Tool::PlaceForest) {
                            ui.add_space(space::XS);
                            editor_ui::form_grid("place-tree").show(ui, |ui| {
                                row(ui, "veg-species", |ui| {
                                    let mut species = state.tree_object.clone().unwrap_or_default();
                                    species_combo(ui, "place-tree-kind", objects, &mut species);
                                    state.tree_object = (!species.is_empty()).then_some(species);
                                });
                                if state.tool == Tool::PlaceForest {
                                    row(ui, "forest-area", |ui| {
                                        let mut area = state.forest_area.unwrap_or(500.0);
                                        let field = editor_ui::field(
                                            ui,
                                            &mut area,
                                            10.0,
                                            10.0..=10_000.0,
                                            "m²",
                                        );
                                        if field.changed() {
                                            state.forest_area = Some(area);
                                        }
                                    });
                                }
                            });
                        }
                        if state.tool == Tool::Brush {
                            ui.add_space(space::XS);
                            editor_ui::form_grid("brush").show(ui, |ui| {
                                row(ui, "brush-radius", |ui| {
                                    let mut radius = state.brush_radius.unwrap_or(30.0);
                                    if editor_ui::field(ui, &mut radius, 1.0, 2.0..=500.0, "m")
                                        .changed()
                                    {
                                        state.brush_radius = Some(radius);
                                    }
                                });
                            });
                            ui.small(t!("brush-marked", count = state.marked.len()));
                            ui.horizontal(|ui| {
                                let delete = egui::Button::new(t!("action-delete-marked"));
                                if ui.add_enabled(!state.marked.is_empty(), delete).clicked() {
                                    tools::delete_marked(line, state);
                                }
                                let clear = egui::Button::new(t!("action-clear-marked"));
                                if ui.add_enabled(!state.marked.is_empty(), clear).clicked() {
                                    state.marked.clear();
                                }
                            });
                        }
                        if state.tool == Tool::PlaceMarker {
                            ui.add_space(space::XS);
                            editor_ui::form_grid("place-marker").show(ui, |ui| {
                                row(ui, "marker-layer", |ui| {
                                    // The raw value, not `marker_layer()` — an
                                    // emptied field would refill itself with
                                    // the default under the typing hands.
                                    let mut layer = state
                                        .marker_layer
                                        .clone()
                                        .unwrap_or_else(|| tools::DEFAULT_MARKER_LAYER.into());
                                    if ui
                                        .add(
                                            egui::TextEdit::singleline(&mut layer)
                                                .desired_width(space::FIELD),
                                        )
                                        .changed()
                                    {
                                        state.marker_layer = Some(layer);
                                    }
                                });
                                row(ui, "marker-label", |ui| {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut state.marker_label)
                                            .desired_width(space::FIELD),
                                    );
                                });
                            });
                        }
                        if state.tool == Tool::TerrainBrush {
                            ui.add_space(space::XS);
                            editor_ui::form_grid("terrain-brush").show(ui, |ui| {
                                row(ui, "terrain-radius", |ui| {
                                    let mut radius = state.terrain_radius.unwrap_or(60.0);
                                    if editor_ui::field(ui, &mut radius, 5.0, 5.0..=2_000.0, "m")
                                        .changed()
                                    {
                                        state.terrain_radius = Some(radius);
                                    }
                                });
                                row(ui, "terrain-mode", |ui| {
                                    for (level, key) in
                                        [(false, "terrain-raise"), (true, "terrain-level")]
                                    {
                                        if ui
                                            .selectable_label(state.terrain_level == level, t!(key))
                                            .on_hover_text(t!(&format!("{key}-hint")))
                                            .clicked()
                                        {
                                            state.terrain_level = level;
                                        }
                                    }
                                });
                                if !state.terrain_level {
                                    row(ui, "terrain-amount", |ui| {
                                        let mut amount = state.terrain_amount.unwrap_or(2.0);
                                        if editor_ui::field(
                                            ui,
                                            &mut amount,
                                            0.5,
                                            -100.0..=100.0,
                                            "m",
                                        )
                                        .changed()
                                        {
                                            state.terrain_amount = Some(amount);
                                        }
                                    });
                                }
                            });
                            ui.small(t!("terrain-count", count = line.source.terrain.len()));
                        }
                        if let Some(drawing) = &state.drawing {
                            ui.add_space(space::XS);
                            ui.small(t!("draw-active", segments = drawing.segments.len()));
                        }
                        if !state.forest_points.is_empty() {
                            ui.add_space(space::XS);
                            ui.small(t!("forest-active", corners = state.forest_points.len()));
                        }
                    });

                    nav_section(
                        ui,
                        jump,
                        &mut current,
                        "selection",
                        "heading-selection",
                        |ui| {
                            selection_panel(ui, line, state, types, objects, focus, marks, overlay);
                        },
                    );

                    nav_section(ui, jump, &mut current, "areas", "heading-areas", |ui| {
                        crate::areas::area_list(ui, line, state, focus);
                    });

                    nav_section(
                        ui,
                        jump,
                        &mut current,
                        "interlock",
                        "heading-interlock",
                        |ui| {
                            interlock_section(ui, line, state, overlay);
                        },
                    );

                    nav_section(ui, jump, &mut current, "markers", "heading-markers", |ui| {
                        marker_section(ui, line, state, focus, marks);
                    });

                    nav_section(ui, jump, &mut current, "heights", "heading-heights", |ui| {
                        height_section(ui, line, state, overlay);
                    });

                    nav_section(ui, jump, &mut current, "module", "heading-module", |ui| {
                        module_section(ui, line, state, ghost, focus);
                    });

                    nav_section(ui, jump, &mut current, "sky", "heading-sky", |ui| {
                        sky_section(ui, sky);
                    });

                    nav_section(ui, jump, &mut current, "checks", "heading-checks", |ui| {
                        checks_section(ui, line, state, focus);
                    });

                    nav_section(ui, jump, &mut current, "imagery", "heading-imagery", |ui| {
                        imagery_section(ui, overlay, request);
                    });

                    nav_section(ui, jump, &mut current, "cache", "heading-cache", |ui| {
                        cache_section(ui, overlay, request);
                    });
                });
            *active = current;
        });
}

/// Picker of the Place-object tool: every installed `objects/*.ron`.
fn object_combo(ui: &mut egui::Ui, id: &str, objects: &TrackObjects, state: &mut EditorState) {
    let current = state
        .object
        .clone()
        .or_else(|| objects.map.keys().next().cloned())
        .unwrap_or_default();
    egui::ComboBox::from_id_salt(id)
        .width(space::FIELD)
        .selected_text(current.clone())
        .show_ui(ui, |ui| {
            for name in objects.map.keys() {
                if ui.selectable_label(&current == name, name).clicked() {
                    state.object = Some(name.clone());
                }
            }
        });
}

fn tool_chips(ui: &mut egui::Ui, state: &mut EditorState) {
    for (group, tools) in tools::TOOL_GROUPS {
        editor_ui::subheading(ui, t!(group));
        // Two to a row, so the palette is a grid with one left edge rather
        // than twelve chips breaking wherever their text happens to end.
        for pair in tools.chunks(2) {
            ui.horizontal(|ui| {
                for (tool, key, icon) in pair {
                    // The number key, appended to whatever the hint says — an
                    // accelerator nobody can see is one nobody uses.
                    let hint = match (
                        i18n::maybe(&format!("{key}-hint")),
                        tools::tool_digit(*tool),
                    ) {
                        (Some(hint), Some(digit)) => Some(format!("{hint} ({digit})")),
                        (Some(hint), None) => Some(hint),
                        (None, Some(digit)) => Some(digit.to_string()),
                        (None, None) => None,
                    };
                    let response =
                        editor_ui::tool_button(ui, *icon, t!(key), state.tool == *tool, hint);
                    if response.clicked() && state.tool != *tool {
                        state.tool = *tool;
                        state.drawing = None;
                        state.forest_points.clear();
                    }
                }
            });
        }
        ui.add_space(space::XS);
    }
}

/// Species picker of the vegetation tools and panels: the placeholder tree
/// (empty string), or any installed `objects/*.ron`.
fn species_combo(ui: &mut egui::Ui, id: &str, objects: &TrackObjects, value: &mut String) {
    let placeholder = t!("veg-placeholder");
    egui::ComboBox::from_id_salt(id)
        .width(space::FIELD)
        .selected_text(if value.is_empty() {
            placeholder.clone()
        } else {
            value.clone()
        })
        .show_ui(ui, |ui| {
            if ui.selectable_label(value.is_empty(), placeholder).clicked() {
                value.clear();
            }
            for name in objects.map.keys() {
                if ui.selectable_label(value == name, name).clicked() {
                    *value = name.clone();
                }
            }
        });
}

#[allow(clippy::too_many_arguments)]
fn selection_panel(
    ui: &mut egui::Ui,
    line: &mut Line,
    state: &mut EditorState,
    types: &TrackTypes,
    objects: &TrackObjects,
    focus: &mut Focus,
    marks: &crate::terrain::Marks,
    overlay: &mut Overlay,
) {
    match state.selection {
        Selection::None => {
            ui.small(t!("sel-none"));
        }
        Selection::TrackArea(i) => {
            crate::areas::area_rows(ui, line, i, types, focus, state);
        }
        Selection::Edge(i) => {
            let Some(edge) = line.source.edges.get(i) else {
                return;
            };
            let length = line.net.edges().get(i).map(|e| e.length()).unwrap_or(0.0);
            ui.label(t!(
                "sel-edge-summary",
                index = i,
                length = format!("{length:.0}"),
                segments = edge.segments.len()
            ));
            let devices = line
                .source
                .devices
                .iter()
                .filter(|d| d.edge as usize == i)
                .count();
            ui.small(t!("sel-edge-devices", devices = devices));
            // Where the geometry can be edited, say how; where it cannot, why.
            ui.small(if tools::support_points(line, i).is_empty() {
                t!("sel-edge-fixed")
            } else {
                t!("sel-edge-handles")
            });
            // What a marked area lays over this track wins on compile, so a value edited
            // here that never shows up on the line is worth saying out loud.
            let covering: Vec<String> = line
                .source
                .areas
                .iter()
                .filter(|a| a.sets_anything() && a.spans.iter().any(|s| s.edge as usize == i))
                .map(|a| {
                    if a.name.is_empty() {
                        t!("area-unnamed")
                    } else {
                        a.name.clone()
                    }
                })
                .collect();
            if !covering.is_empty() {
                ui.small(
                    egui::RichText::new(t!("sel-edge-covered", areas = covering.join(", ")))
                        .color(colors::TEXT_SECONDARY),
                );
            }
            track_type_rows(ui, line, i, length, types);
            electrification_rows(ui, line, i, length);
            switch_rows(ui, line, i);
            ui.add_space(space::XS);
            ui.horizontal(|ui| {
                if ui.button(t!("action-center")).clicked()
                    && let Some(edge) = line.net.edges().get(i)
                {
                    focus.position = edge.eval(edge.length() / 2.0).pos;
                }
                if ui.button(t!("action-delete")).clicked() {
                    tools::delete_selection(line, state);
                }
            });
        }
        Selection::Device(i) => {
            let Some(device) = line.source.devices.get(i) else {
                return;
            };
            let edge_length = line
                .net
                .edges()
                .get(device.edge as usize)
                .map(|e| e.length())
                .unwrap_or(f64::MAX);
            let position = tools::device_pos(&line.net, device);
            ui.label(t!("sel-device-summary", index = i, edge = device.edge));
            let device = &mut line.source.devices[i];
            editor_ui::form_grid("sel-device").show(ui, |ui| {
                row(ui, "dev-kind", |ui| {
                    kind_combo(ui, "sel-kind", &mut device.kind);
                });
                row(ui, "dev-s", |ui| {
                    editor_ui::field(ui, &mut device.s, 1.0, 0.0..=edge_length, "m");
                });
                row(ui, "dev-facing", |ui| {
                    facing_combo(ui, &mut device.facing);
                });
                row(ui, "dev-lateral", |ui| {
                    editor_ui::field(ui, &mut device.lateral_offset, 0.1, -20.0..=20.0, "m");
                });
            });
            let label = editor_ui::form_label(ui, t!("dev-payload"));
            if let Some(hint) = i18n::maybe("dev-payload-hint") {
                label.on_hover_text(hint);
            }
            ui.add(
                egui::TextEdit::multiline(&mut device.payload)
                    .font(egui::TextStyle::Monospace)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY),
            );
            payload_presets(ui, device);
            let is_signal = device.kind == DeviceKind::Signal;
            if is_signal {
                signal_rows(ui, line, state, overlay, i);
            }
            ui.add_space(space::XS);
            ui.horizontal(|ui| {
                if ui.button(t!("action-center")).clicked()
                    && let Some(p) = position
                {
                    focus.position = p;
                }
                if ui.button(t!("action-delete")).clicked() {
                    tools::delete_selection(line, state);
                }
            });
        }
        Selection::Object(i) => {
            let Some(object) = line.source.objects.get(i) else {
                return;
            };
            let edge_length = line
                .net
                .edges()
                .get(object.edge as usize)
                .map(|e| e.length())
                .unwrap_or(f64::MAX);
            let position = tools::object_pos(&line.net, object);
            ui.label(t!("sel-object-summary", index = i, edge = object.edge));
            let object = &mut line.source.objects[i];
            ui.label(
                egui::RichText::new(object.object.clone())
                    .monospace()
                    .color(colors::TEXT_SECONDARY),
            );
            editor_ui::form_grid("sel-object").show(ui, |ui| {
                row(ui, "obj-s", |ui| {
                    editor_ui::field(ui, &mut object.s, 1.0, 0.0..=edge_length, "m");
                });
                row(ui, "obj-lateral", |ui| {
                    editor_ui::field(ui, &mut object.lateral_offset, 0.1, -50.0..=50.0, "m");
                });
                row(ui, "obj-yaw", |ui| {
                    editor_ui::field(ui, &mut object.yaw_deg, 1.0, -360.0..=360.0, "°");
                });
                row(ui, "obj-height", |ui| {
                    editor_ui::field(ui, &mut object.height, 0.1, -10.0..=50.0, "m");
                });
                row(ui, "obj-snap", |ui| {
                    ui.checkbox(&mut object.snap_to_terrain, "");
                });
            });
            repeat_rows(ui, line, state, i, edge_length);
            ui.add_space(space::XS);
            ui.horizontal(|ui| {
                if ui.button(t!("action-center")).clicked()
                    && let Some(p) = position
                {
                    focus.position = p;
                }
                if ui.button(t!("action-delete")).clicked() {
                    tools::delete_selection(line, state);
                }
            });
        }
        Selection::Tree(i) => {
            let Some(tree) = line.source.trees.get(i) else {
                return;
            };
            let position = marks.tree(i, tree);
            ui.label(t!("sel-tree-summary", index = i));
            let tree = &mut line.source.trees[i];
            editor_ui::form_grid("sel-tree").show(ui, |ui| {
                row(ui, "veg-species", |ui| {
                    species_combo(ui, "sel-tree-species", objects, &mut tree.object);
                });
                row(ui, "tree-yaw", |ui| {
                    editor_ui::field(ui, &mut tree.yaw_deg, 1.0, -360.0..=360.0, "°");
                });
                row(ui, "tree-scale", |ui| {
                    editor_ui::field(ui, &mut tree.scale, 0.05, 0.2..=5.0, "");
                });
            });
            ui.add_space(space::XS);
            ui.horizontal(|ui| {
                if ui.button(t!("action-center")).clicked() {
                    focus.position = position;
                }
                if ui.button(t!("action-delete")).clicked() {
                    tools::delete_selection(line, state);
                }
            });
        }
        Selection::TerrainEdit(i) => {
            let Some(edit) = line.source.terrain.get(i) else {
                return;
            };
            let position = marks.stroke(i, edit);
            ui.label(t!("sel-terrain-summary", index = i));
            let edit = &mut line.source.terrain[i];
            editor_ui::form_grid("sel-terrain").show(ui, |ui| {
                row(ui, "terrain-radius", |ui| {
                    editor_ui::field(ui, &mut edit.radius, 5.0, 5.0..=2_000.0, "m");
                });
                match &mut edit.edit {
                    content::route::TerrainEdit::Raise(by) => {
                        row(ui, "terrain-amount", |ui| {
                            editor_ui::field(ui, by, 0.5, -100.0..=100.0, "m");
                        });
                    }
                    content::route::TerrainEdit::Level(to) => {
                        row(ui, "terrain-target", |ui| {
                            editor_ui::field(ui, to, 0.5, -500.0..=5_000.0, "m");
                        });
                    }
                }
            });
            ui.add_space(space::XS);
            ui.horizontal(|ui| {
                if ui.button(t!("action-center")).clicked() {
                    focus.position = position;
                }
                if ui.button(t!("action-delete")).clicked() {
                    tools::delete_selection(line, state);
                }
            });
        }
        Selection::Marker(i) => {
            let Some(marker) = line.source.markers.get(i) else {
                return;
            };
            let position = marks.marker(i, marker);
            ui.label(t!("sel-marker-summary", index = i));
            let marker = &mut line.source.markers[i];
            editor_ui::form_grid("sel-marker").show(ui, |ui| {
                // Retyping the layer moves the marker into another one — that
                // is the whole layer management a marker needs.
                row(ui, "marker-layer", |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut marker.layer).desired_width(space::FIELD),
                    );
                });
                row(ui, "marker-label", |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut marker.label).desired_width(space::FIELD),
                    );
                });
            });
            ui.add_space(space::XS);
            ui.horizontal(|ui| {
                if ui.button(t!("action-center")).clicked() {
                    focus.position = position;
                }
                if ui.button(t!("action-delete")).clicked() {
                    tools::delete_selection(line, state);
                }
            });
        }
        // A corner of the module envelope: the two coordinates it is, and the
        // one thing a corner can be besides moved — removed.
        Selection::EnvelopePoint(i) => {
            let Some(point) = line.source.envelope.get(i).copied() else {
                return;
            };
            let position = crate::envelope::point_pos(&point, crate::envelope::height(line, focus));
            ui.label(t!(
                "sel-envelope-summary",
                index = i + 1,
                count = line.source.envelope.len()
            ));
            let point = &mut line.source.envelope[i];
            editor_ui::form_grid("sel-envelope").show(ui, |ui| {
                row(ui, "new-module-lat", |ui| {
                    editor_ui::field(ui, &mut point.lat, 0.0001, -85.0..=85.0, "°");
                });
                row(ui, "new-module-lon", |ui| {
                    editor_ui::field(ui, &mut point.lon, 0.0001, -180.0..=180.0, "°");
                });
            });
            ui.add_space(space::XS);
            ui.horizontal(|ui| {
                if ui.button(t!("action-center")).clicked() {
                    focus.position = position;
                }
                // Three corners are a polygon; two are a line, which bounds
                // nothing.
                let removable = line.source.envelope.len() > 3;
                if ui
                    .add_enabled(removable, egui::Button::new(t!("action-delete")))
                    .on_disabled_hover_text(t!("envelope-min-points"))
                    .clicked()
                {
                    tools::delete_selection(line, state);
                }
            });
        }
    }
}

/// Repeats the selected object along its edge — the Zusi editor function
/// "insert one every x metres", as a row of stamped, individually editable
/// instances rather than a construct of its own in the file.
fn repeat_rows(
    ui: &mut egui::Ui,
    line: &mut Line,
    state: &mut EditorState,
    index: usize,
    edge_length: f64,
) {
    editor_ui::subheading(ui, t!("obj-repeat"));
    let mut interval = state.repeat_interval.unwrap_or(65.0);
    let mut until = state.repeat_until.unwrap_or(edge_length).min(edge_length);
    editor_ui::form_grid("obj-repeat").show(ui, |ui| {
        row(ui, "obj-repeat-interval", |ui| {
            if editor_ui::field(ui, &mut interval, 1.0, 1.0..=5000.0, "m").changed() {
                state.repeat_interval = Some(interval);
            }
        });
        row(ui, "obj-repeat-until", |ui| {
            if editor_ui::field(ui, &mut until, 10.0, 0.0..=edge_length, "m").changed() {
                state.repeat_until = Some(until);
            }
        });
    });
    let start = line.source.objects[index].s;
    let count = tools::repeat_positions(start, interval, until.min(edge_length)).len();
    let button = ui.add_enabled(count > 0, egui::Button::new(t!("action-repeat-object")));
    let button = button.on_hover_text(t!("obj-repeat-hint", count = count));
    let button = button.on_disabled_hover_text(t!("obj-repeat-empty"));
    if button.clicked() {
        tools::repeat_object(line, index, interval, until);
    }
}

/// Superstructure sections of the selected track: `(s, type)` rows over the
/// arc length — the map tints the ribbon per section in the same colors.
fn track_type_rows(ui: &mut egui::Ui, line: &mut Line, i: usize, length: f64, types: &TrackTypes) {
    editor_ui::subheading(ui, t!("sel-track-type"));
    // Section tints, resolved against the compiled net before the source is
    // borrowed mutably; one frame behind after an edit, like the jump bar.
    let swatches: Vec<egui::Color32> = line.source.edges[i]
        .track_type
        .iter()
        .map(|(_, name)| {
            let index = match name.as_str() {
                "default" => Some(0),
                _ => line.net.types().iter().position(|t| &t.name == name),
            };
            crate::type_color32(index.unwrap_or(0) as u32)
        })
        .collect();
    let known: Vec<String> = types.map.keys().cloned().collect();
    let edge = &mut line.source.edges[i];
    if edge.track_type.is_empty() {
        ui.small(t!("sel-track-type-none"));
    }
    let mut remove = None;
    editor_ui::form_grid(&format!("edge-types-{i}"))
        .num_columns(4)
        .show(ui, |ui| {
            for (k, step) in edge.track_type.iter_mut().enumerate() {
                ui.label(egui::RichText::new("■").color(swatches[k]));
                editor_ui::field(ui, &mut step.0, 10.0, 0.0..=length, "m")
                    .on_hover_text(t!("sel-track-type-from"));
                let unknown = step.1 != "default" && !types.map.contains_key(&step.1);
                let label = if step.1 == "default" {
                    t!("track-type-default")
                } else {
                    step.1.clone()
                };
                let mut text = egui::RichText::new(label);
                if unknown {
                    // A name no installed mod answers — visible before the run.
                    text = text.color(colors::ERROR);
                }
                egui::ComboBox::from_id_salt(("edge-type", i, k))
                    .width(space::FIELD)
                    .selected_text(text)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(step.1 == "default", t!("track-type-default"))
                            .clicked()
                        {
                            step.1 = "default".into();
                        }
                        for name in &known {
                            if ui.selectable_label(&step.1 == name, name).clicked() {
                                step.1 = name.clone();
                            }
                        }
                    });
                if ui.small_button("×").clicked() {
                    remove = Some(k);
                }
                ui.end_row();
            }
        });
    if let Some(k) = remove {
        edge.track_type.remove(k);
    }
    if ui
        .small_button(t!("action-add-type-section"))
        .on_hover_text(t!("sel-track-type-hint"))
        .clicked()
    {
        let s = edge
            .track_type
            .last()
            .map(|(s, _)| (s + 100.0).min(length))
            .unwrap_or(0.0);
        let name = known.first().cloned().unwrap_or_else(|| "default".into());
        edge.track_type.push((s, name));
    }
}

/// What hangs over the selected track: `(s, system)` rows over the arc length.
/// The wire belongs to the track, so this is the only place it is set; no rows
/// means no wire, unless the file still carries the legacy line-wide value.
fn electrification_rows(ui: &mut egui::Ui, line: &mut Line, i: usize, length: f64) {
    editor_ui::subheading(ui, t!("sel-power"));
    let default_label = power_label(track_model::electrification_from_id(
        &line.source.electrification,
    ));
    let edge = &mut line.source.edges[i];
    if edge.electrification.is_empty() {
        ui.small(t!("sel-power-default", system = default_label));
    }
    let mut remove = None;
    editor_ui::form_grid(&format!("edge-power-{i}"))
        .num_columns(3)
        .show(ui, |ui| {
            for (k, step) in edge.electrification.iter_mut().enumerate() {
                editor_ui::field(ui, &mut step.0, 10.0, 0.0..=length, "m")
                    .on_hover_text(t!("sel-power-from"));
                let current = track_model::electrification_from_id(&step.1);
                egui::ComboBox::from_id_salt(("edge-power", i, k))
                    .width(space::FIELD)
                    .selected_text(power_label(current))
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(current.is_none(), power_label(None))
                            .clicked()
                        {
                            step.1 = "none".into();
                        }
                        for system in track_model::PowerSystem::ALL {
                            if ui
                                .selectable_label(
                                    current == Some(system),
                                    power_label(Some(system)),
                                )
                                .clicked()
                            {
                                step.1 = system.id().into();
                            }
                        }
                    });
                if ui.small_button("×").clicked() {
                    remove = Some(k);
                }
                ui.end_row();
            }
        });
    if let Some(k) = remove {
        edge.electrification.remove(k);
    }
    if ui
        .small_button(t!("action-add-power-section"))
        .on_hover_text(t!("sel-power-hint"))
        .clicked()
    {
        let s = edge
            .electrification
            .last()
            .map(|(s, _)| (s + 100.0).min(length))
            .unwrap_or(0.0);
        // A new section is the useful one: the gap under a system boundary or a
        // siding, which is what anybody adds a section for.
        edge.electrification.push((s, "none".into()));
    }
}

/// Name of a supply system for the editor — the systems are type designations,
/// so only "no wire" needs translating.
pub(crate) fn power_label(value: track_model::Electrification) -> String {
    match value {
        None => t!("power-none"),
        Some(track_model::PowerSystem::Ac15kv) => "AC 15 kV 16,7 Hz".into(),
        Some(track_model::PowerSystem::Ac25kv) => "AC 25 kV 50 Hz".into(),
        Some(track_model::PowerSystem::Dc3kv) => "DC 3 kV".into(),
        Some(track_model::PowerSystem::Dc1500v) => "DC 1,5 kV".into(),
        Some(track_model::PowerSystem::ThirdRail) => "DC 750 V".into(),
    }
}

/// Switch fields of the nodes the selected track hangs on. A turnout is a
/// node, and the editor has no node picking — the tracks meeting there are how
/// the map addresses it, and every leg of a switch is one of them.
fn switch_rows(ui: &mut egui::Ui, line: &mut Line, edge: usize) {
    let Some(source) = line.source.edges.get(edge) else {
        return;
    };
    let mut nodes = vec![source.from, source.to];
    nodes.dedup();
    nodes.retain(|n| {
        matches!(
            line.source.nodes.get(*n as usize),
            Some(NodeSource::Switch { .. })
        )
    });
    if nodes.is_empty() {
        return;
    }
    editor_ui::subheading(ui, t!("sel-switch"));
    editor_ui::form_grid(&format!("edge-switch-{edge}")).show(ui, |ui| {
        for node in nodes {
            let Some(NodeSource::Switch {
                root,
                straight,
                throw_time,
                ..
            }) = line.source.nodes.get_mut(node as usize)
            else {
                continue;
            };
            let leg = if root.0 as usize == edge {
                "switch-leg-root"
            } else if straight.0 as usize == edge {
                "switch-leg-straight"
            } else {
                "switch-leg-diverging"
            };
            editor_ui::form_label(ui, t!("sel-switch-node", node = node, leg = t!(leg)))
                .on_hover_text(t!("sel-switch-hint"));
            ui.horizontal(|ui| {
                editor_ui::field(ui, throw_time, 0.5, 0.5..=120.0, "s");
            });
            ui.end_row();
        }
    });
}

/// Short label of a signal for the pickers: index, kind and the device it
/// stands on — enough to tell two main signals apart on the same track.
fn signal_labels(source: &LineSource) -> Vec<String> {
    source
        .signals
        .iter()
        .enumerate()
        .map(|(i, s)| {
            t!(
                "signal-label",
                index = i,
                kind = signal_kind_label(s.kind),
                device = s.device
            )
        })
        .collect()
}

/// The signal table entry of a placed Signal device — what the interlocking
/// actually reads. Without it the device is a mast with nothing behind it,
/// which is why the entry is edited here rather than in a table of its own.
fn signal_rows(
    ui: &mut egui::Ui,
    line: &mut Line,
    state: &mut EditorState,
    overlay: &mut Overlay,
    device: usize,
) {
    editor_ui::subheading(ui, t!("sel-signal"));
    let Some(index) = line
        .source
        .signals
        .iter()
        .position(|s| s.device == device as u32)
    else {
        if ui
            .button(t!("action-add-signal"))
            .on_hover_text(t!("sel-signal-hint"))
            .clicked()
        {
            line.source.signals.push(SignalSource {
                kind: SignalKind::Main,
                system: SignalSystem::Ks,
                device: device as u32,
                next: None,
                guarded: Vec::new(),
                requires_route: false,
                diverging_speed: None,
                signal_type: None,
                model: None,
            });
        }
        return;
    };
    // Both pickers read from tables the signal itself lives in — resolved
    // before the entry is borrowed mutably.
    let labels = signal_labels(&line.source);
    let sections = line.source.sections.len();
    let signal = &mut line.source.signals[index];
    editor_ui::form_grid(&format!("signal-{index}")).show(ui, |ui| {
        row(ui, "sig-kind", |ui| {
            signal_kind_combo(ui, index, &mut signal.kind);
        });
        row(ui, "sig-system", |ui| {
            signal_system_combo(ui, index, &mut signal.system);
        });
        row(ui, "sig-next", |ui| {
            egui::ComboBox::from_id_salt(("sig-next", index))
                .width(space::FIELD)
                .selected_text(match signal.next {
                    Some(n) => labels
                        .get(n as usize)
                        .cloned()
                        .unwrap_or_else(|| n.to_string()),
                    None => t!("common-none"),
                })
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(signal.next.is_none(), t!("common-none"))
                        .clicked()
                    {
                        signal.next = None;
                    }
                    for (n, label) in labels.iter().enumerate() {
                        if n == index {
                            continue; // a signal does not announce itself
                        }
                        if ui
                            .selectable_label(signal.next == Some(n as u32), label)
                            .clicked()
                        {
                            signal.next = Some(n as u32);
                        }
                    }
                });
        });
        row(ui, "sig-requires-route", |ui| {
            ui.checkbox(&mut signal.requires_route, "");
        });
        row(ui, "sig-diverging-speed", |ui| {
            let mut set = signal.diverging_speed.is_some();
            if ui.checkbox(&mut set, "").changed() {
                signal.diverging_speed = set.then_some(40.0);
            }
            if let Some(speed) = &mut signal.diverging_speed {
                editor_ui::field(ui, speed, 5.0, 10.0..=160.0, "km/h");
            }
        });
        row(ui, "sig-type", |ui| {
            optional_text(ui, ("sig-type", index), &mut signal.signal_type);
        });
        row(ui, "sig-model", |ui| {
            optional_text(ui, ("sig-model", index), &mut signal.model);
        });
    });
    index_chips(
        ui,
        ("sig-guarded", index),
        t!("sig-guarded"),
        &mut signal.guarded,
        sections,
    );
    let starts_routes = signal.kind.ends_a_route();
    ui.add_space(space::XS);
    if ui
        .small_button(t!("action-delete-signal"))
        .on_hover_text(t!("action-delete-signal-hint"))
        .clicked()
    {
        line.source.remove_signal(index);
        return;
    }
    // Routes start where a train move is authorised — a distant signal
    // announces, a track lock secures, neither begins one.
    if starts_routes {
        signal_routes(ui, line, state, overlay, index, &labels);
    }
}

/// The routes that start at this signal. Zusi carries them on the signal, and
/// so does this: what leaves here, where it ends, and one button that finds
/// every route the track allows — one per leg of every turnout ahead, each
/// ending at the next signal on it. Editing stays in the interlocking panel,
/// which every row jumps to.
fn signal_routes(
    ui: &mut egui::Ui,
    line: &mut Line,
    state: &mut EditorState,
    overlay: &mut Overlay,
    signal: usize,
    labels: &[String],
) {
    editor_ui::subheading(ui, t!("sig-routes"));
    let mine: Vec<usize> = line
        .source
        .routes
        .iter()
        .enumerate()
        .filter(|(_, r)| r.entry == signal as u32)
        .map(|(i, _)| i)
        .collect();
    if mine.is_empty() {
        ui.small(t!("sig-routes-none"));
    }
    let mut remove = None;
    for i in mine {
        let route = &line.source.routes[i];
        let exit = labels
            .get(route.exit as usize)
            .cloned()
            .unwrap_or_else(|| route.exit.to_string());
        let summary = t!(
            "sig-route-row",
            exit = exit,
            sections = route.sections.len(),
            switches = route.switches.len()
        );
        let row = ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(summary).color(colors::TEXT_SECONDARY));
            if ui.small_button(t!("action-edit-route")).clicked() {
                state.jump_to = Some("interlock");
            }
            if ui.small_button("×").clicked() {
                remove = Some(i);
            }
        });
        if ui.rect_contains_pointer(row.response.rect) {
            state.highlight = Some(Highlight::Route(i));
        }
    }
    if let Some(i) = remove {
        line.source.routes.remove(i);
    }
    if ui
        .small_button(t!("action-find-routes"))
        .on_hover_text(t!("action-find-routes-hint"))
        .clicked()
    {
        let found = line.source.routes_from(signal as u32, state.overlap_length);
        let (mut added, mut known) = (0, 0);
        for route in found {
            // A route that is already in the file keeps whatever the builder
            // did to it — this adds what is missing, it does not overwrite.
            if line
                .source
                .routes
                .iter()
                .any(|r| r.entry == route.entry && r.exit == route.exit)
            {
                known += 1;
            } else {
                line.source.routes.push(route);
                added += 1;
            }
        }
        overlay.status = t!("status-routes-found", added = added, known = known);
    }
}

/// An optional name field: empty text means the option is `None`, which is
/// what "no signal type of its own" looks like in the file.
fn optional_text(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &mut Option<String>,
) {
    let mut text = value.clone().unwrap_or_default();
    let field = egui::TextEdit::singleline(&mut text)
        .id_salt(id)
        .hint_text(t!("common-none"))
        .desired_width(space::FIELD);
    if ui.add(field).changed() {
        *value = (!text.trim().is_empty()).then_some(text);
    }
}

/// A list of indices into another table, as removable chips plus a picker that
/// appends one. `count` is how large that table is — an empty one leaves the
/// picker with nothing to offer, which is the honest state of the file.
fn index_chips(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    label: String,
    list: &mut Vec<u32>,
    count: usize,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new(label).color(colors::TEXT_SECONDARY));
        let mut remove = None;
        for (k, value) in list.iter().enumerate() {
            if ui.small_button(format!("{value} ×")).clicked() {
                remove = Some(k);
            }
        }
        if let Some(k) = remove {
            list.remove(k);
        }
        egui::ComboBox::from_id_salt(id)
            .width(48.0)
            .selected_text("+")
            .show_ui(ui, |ui| {
                for candidate in 0..count as u32 {
                    if !list.contains(&candidate)
                        && ui.selectable_label(false, candidate.to_string()).clicked()
                    {
                        list.push(candidate);
                    }
                }
            });
    });
}

/// Switch positions a route sets: node index plus which way it lies. Clicking
/// a chip flips it — with two positions a picker would only cost a click.
fn switch_chips(
    ui: &mut egui::Ui,
    route: usize,
    list: &mut Vec<(u32, SwitchPosition)>,
    nodes: &[u32],
) {
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new(t!("route-switches")).color(colors::TEXT_SECONDARY));
        let mut remove = None;
        for (k, (node, position)) in list.iter_mut().enumerate() {
            let label = match position {
                SwitchPosition::Straight => t!("switch-straight"),
                SwitchPosition::Diverging => t!("switch-diverging"),
            };
            if ui
                .small_button(format!("{node} {label}"))
                .on_hover_text(t!("route-switch-hint"))
                .clicked()
            {
                *position = match position {
                    SwitchPosition::Straight => SwitchPosition::Diverging,
                    SwitchPosition::Diverging => SwitchPosition::Straight,
                };
            }
            if ui.small_button("×").clicked() {
                remove = Some(k);
            }
        }
        if let Some(k) = remove {
            list.remove(k);
        }
        egui::ComboBox::from_id_salt(("route-switch", route))
            .width(48.0)
            .selected_text("+")
            .show_ui(ui, |ui| {
                for node in nodes {
                    if !list.iter().any(|(n, _)| n == node)
                        && ui.selectable_label(false, node.to_string()).clicked()
                    {
                        list.push((*node, SwitchPosition::Straight));
                    }
                }
            });
    });
}

/// Flank protection of a route: what keeps a vehicle off its path where a
/// track joins it. A protecting turnout carries the position it has to lie in
/// (click to flip), a protecting signal is held at stop while the route is set.
fn flank_chips(
    ui: &mut egui::Ui,
    route: usize,
    list: &mut Vec<FlankSource>,
    nodes: &[u32],
    labels: &[String],
    holds: &[u32],
) {
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new(t!("route-flank")).color(colors::TEXT_SECONDARY));
        let mut remove = None;
        for (k, guard) in list.iter_mut().enumerate() {
            let text = match guard {
                FlankSource::Switch(node, SwitchPosition::Straight) => {
                    t!(
                        "flank-switch",
                        node = node,
                        position = t!("switch-straight")
                    )
                }
                FlankSource::Switch(node, SwitchPosition::Diverging) => {
                    t!(
                        "flank-switch",
                        node = node,
                        position = t!("switch-diverging")
                    )
                }
                FlankSource::Signal(signal) => t!(
                    "flank-signal",
                    signal = labels
                        .get(*signal as usize)
                        .cloned()
                        .unwrap_or_else(|| signal.to_string())
                ),
            };
            let chip = ui.small_button(text);
            if let FlankSource::Switch(_, position) = guard {
                if chip.on_hover_text(t!("route-switch-hint")).clicked() {
                    *position = match position {
                        SwitchPosition::Straight => SwitchPosition::Diverging,
                        SwitchPosition::Diverging => SwitchPosition::Straight,
                    };
                }
            } else {
                chip.on_hover_text(t!("flank-signal-hint"));
            }
            if ui.small_button("×").clicked() {
                remove = Some(k);
            }
        }
        if let Some(k) = remove {
            list.remove(k);
        }
        egui::ComboBox::from_id_salt(("flank-switch", route))
            .width(72.0)
            .selected_text(t!("flank-add-switch"))
            .show_ui(ui, |ui| {
                for node in nodes {
                    if ui.selectable_label(false, node.to_string()).clicked() {
                        list.push(FlankSource::Switch(*node, SwitchPosition::Straight));
                    }
                }
            });
        egui::ComboBox::from_id_salt(("flank-signal", route))
            .width(72.0)
            .selected_text(t!("flank-add-signal"))
            .show_ui(ui, |ui| {
                // Only signals that can hold a movement — a distant signal
                // announces the one ahead of it and stops nothing.
                for (i, label) in labels.iter().enumerate() {
                    if !holds.contains(&(i as u32)) {
                        continue;
                    }
                    if ui.selectable_label(false, label).clicked() {
                        list.push(FlankSource::Signal(i as u32));
                    }
                }
            });
    });
}

/// Occupancy sections and routes — the two interlocking tables of the file,
/// which until now were typed into the RON by hand. Both are index-addressed,
/// so the editor shows the indices rather than inventing names for them, and
/// the row under the mouse is drawn on the map (`EditorState::highlight`).
fn interlock_section(
    ui: &mut egui::Ui,
    line: &mut Line,
    state: &mut EditorState,
    overlay: &mut Overlay,
) {
    let selected_edge = match state.selection {
        Selection::Edge(i) => Some(i as u32),
        _ => None,
    };

    editor_ui::subheading(ui, t!("il-sections"));
    if line.source.sections.is_empty() {
        ui.small(t!("il-sections-none"));
    }
    let mut remove = None;
    for (i, section) in line.source.sections.iter_mut().enumerate() {
        let row = ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new(t!("il-section-row", index = i)).color(colors::TEXT_SECONDARY),
            );
            let mut drop_edge = None;
            for (k, edge) in section.edges.iter().enumerate() {
                if ui.small_button(format!("{edge} ×")).clicked() {
                    drop_edge = Some(k);
                }
            }
            if let Some(k) = drop_edge {
                section.edges.remove(k);
            }
            let addable = selected_edge.filter(|e| !section.edges.contains(e));
            let add = ui.add_enabled(
                addable.is_some(),
                egui::Button::new(t!("action-add-track")).small(),
            );
            if add
                .on_disabled_hover_text(t!("il-add-track-hint"))
                .clicked()
                && let Some(edge) = addable
            {
                section.edges.push(edge);
            }
            if ui.small_button("×").clicked() {
                remove = Some(i);
            }
        });
        if ui.rect_contains_pointer(row.response.rect) {
            state.highlight = Some(Highlight::Section(i));
        }
    }
    if let Some(i) = remove {
        line.source.remove_section(i);
    }
    if ui
        .small_button(t!("action-add-section"))
        .on_hover_text(t!("il-sections-hint"))
        .clicked()
    {
        line.source.sections.push(SectionSource {
            edges: selected_edge.into_iter().collect(),
        });
    }

    ui.add_space(space::S);
    editor_ui::subheading(ui, t!("il-routes"));
    if line.source.signals.len() < 2 {
        ui.small(t!("il-routes-need-signals"));
        return;
    }
    let labels = signal_labels(&line.source);
    let switches: Vec<u32> = line
        .source
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| matches!(n, NodeSource::Switch { .. }))
        .map(|(i, _)| i as u32)
        .collect();
    let sections = line.source.sections.len();
    // Signals that can be held at stop for flank protection.
    let holding: Vec<u32> = line
        .source
        .signals
        .iter()
        .enumerate()
        .filter(|(_, s)| s.kind.holds_a_flank())
        .map(|(i, _)| i as u32)
        .collect();
    // What "Derive path" walks out behind the exit signal. The rulebook length
    // depends on the speed each route ends at, so the regular case is a state
    // of its own rather than a number sitting in the field.
    editor_ui::form_grid("il-overlap").show(ui, |ui| {
        row(ui, "route-overlap-length", |ui| {
            let mut by_rule = state.overlap_length.is_none();
            if ui
                .checkbox(&mut by_rule, t!("overlap-by-rule"))
                .on_hover_text(t!("overlap-by-rule-hint"))
                .changed()
            {
                state.overlap_length = (!by_rule).then_some(200.0);
            }
            if let Some(length) = &mut state.overlap_length {
                editor_ui::field(ui, length, 10.0, 0.0..=1000.0, "m");
            }
        });
    });
    if line.source.routes.is_empty() {
        ui.small(t!("il-routes-none"));
    }
    let mut remove = None;
    let mut derive = None;
    for (i, route) in line.source.routes.iter_mut().enumerate() {
        let block = ui.scope(|ui| {
            editor_ui::form_grid(&format!("route-{i}")).show(ui, |ui| {
                row(ui, "route-entry", |ui| {
                    signal_combo(ui, ("route-entry", i), &mut route.entry, &labels);
                });
                row(ui, "route-exit", |ui| {
                    signal_combo(ui, ("route-exit", i), &mut route.exit, &labels);
                });
                row(ui, "route-diverging", |ui| {
                    ui.checkbox(&mut route.diverging, "");
                });
            });
            index_chips(
                ui,
                ("route-sections", i),
                t!("route-sections"),
                &mut route.sections,
                sections,
            );
            index_chips(
                ui,
                ("route-overlap", i),
                t!("route-overlap"),
                &mut route.overlap,
                sections,
            );
            switch_chips(ui, i, &mut route.switches, &switches);
            flank_chips(ui, i, &mut route.flank, &switches, &labels, &holding);
            ui.horizontal(|ui| {
                if ui
                    .small_button(t!("action-derive-route"))
                    .on_hover_text(t!("action-derive-route-hint"))
                    .clicked()
                {
                    derive = Some(i);
                }
                if ui.small_button(t!("action-delete-route")).clicked() {
                    remove = Some(i);
                }
            });
        });
        if ui.rect_contains_pointer(block.response.rect) {
            state.highlight = Some(Highlight::Route(i));
        }
        ui.add_space(space::XS);
    }
    // Both act on the table the loop just borrowed.
    if let Some(i) = derive {
        derive_route(line, i, state.overlap_length, overlay);
    }
    if let Some(i) = remove {
        line.source.routes.remove(i);
    }
    if ui
        .small_button(t!("action-add-route"))
        .on_hover_text(t!("il-routes-hint"))
        .clicked()
    {
        line.source.routes.push(RouteSource {
            entry: 0,
            exit: 1,
            switches: Vec::new(),
            sections: Vec::new(),
            overlap: Vec::new(),
            flank: Vec::new(),
            diverging: false,
        });
    }
}

/// Fills route `index` in from the geometry: the path across the track graph
/// from its entry to its exit signal decides the sections, the switch
/// positions and — walked on behind the exit signal — the overlap.
fn derive_route(line: &mut Line, index: usize, overlap: Option<f64>, overlay: &mut Overlay) {
    let Some((entry, exit)) = line.source.routes.get(index).map(|r| (r.entry, r.exit)) else {
        return;
    };
    match line.source.route_between(entry, exit, overlap) {
        Some(found) => {
            let route = &mut line.source.routes[index];
            route.switches = found.switches;
            route.sections = found.sections;
            route.overlap = found.overlap;
            route.flank = found.flank;
            route.diverging = found.diverging;
            overlay.status = t!(
                "status-route-derived",
                sections = route.sections.len(),
                overlap = route.overlap.len(),
                switches = route.switches.len()
            );
        }
        None => overlay.status = t!("status-no-route-path"),
    }
}

/// Picker over the signal table for the entry and exit of a route.
fn signal_combo(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &mut u32,
    labels: &[String],
) {
    let current = labels
        .get(*value as usize)
        .cloned()
        .unwrap_or_else(|| value.to_string());
    egui::ComboBox::from_id_salt(id)
        .width(space::FIELD)
        .selected_text(current)
        .show_ui(ui, |ui| {
            for (i, label) in labels.iter().enumerate() {
                if ui.selectable_label(*value == i as u32, label).clicked() {
                    *value = i as u32;
                }
            }
        });
}

fn signal_kind_label(kind: SignalKind) -> String {
    match kind {
        SignalKind::Main => t!("sig-kind-main"),
        SignalKind::Distant => t!("sig-kind-distant"),
        SignalKind::Combined => t!("sig-kind-combined"),
        SignalKind::Shunting => t!("sig-kind-shunting"),
        SignalKind::TrackLock => t!("sig-kind-track-lock"),
    }
}

fn signal_kind_combo(ui: &mut egui::Ui, index: usize, kind: &mut SignalKind) {
    egui::ComboBox::from_id_salt(("sig-kind", index))
        .width(space::FIELD)
        .selected_text(signal_kind_label(*kind))
        .show_ui(ui, |ui| {
            for candidate in [
                SignalKind::Main,
                SignalKind::Distant,
                SignalKind::Combined,
                SignalKind::Shunting,
                SignalKind::TrackLock,
            ] {
                if ui
                    .selectable_label(*kind == candidate, signal_kind_label(candidate))
                    .clicked()
                {
                    *kind = candidate;
                }
            }
        });
}

/// H/V, Ks and Hl are designations, not prose — they stay literal.
pub(crate) fn signal_system_label(system: SignalSystem) -> &'static str {
    match system {
        SignalSystem::HV => "H/V",
        SignalSystem::Ks => "Ks",
        SignalSystem::Hl => "Hl",
    }
}

fn signal_system_combo(ui: &mut egui::Ui, index: usize, system: &mut SignalSystem) {
    let label = signal_system_label;
    egui::ComboBox::from_id_salt(("sig-system", index))
        .width(space::FIELD)
        .selected_text(label(*system))
        .show_ui(ui, |ui| {
            for candidate in [SignalSystem::HV, SignalSystem::Ks, SignalSystem::Hl] {
                if ui
                    .selectable_label(*system == candidate, label(candidate))
                    .clicked()
                {
                    *system = candidate;
                }
            }
        });
}

/// One-click starting payloads for the kinds whose RON a modder would
/// otherwise have to type from memory. Serialised from the `sim-core` types,
/// so the templates cannot drift from what the simulator parses.
fn payload_presets(ui: &mut egui::Ui, device: &mut content::route::DeviceSource) {
    let presets: Vec<(String, String)> = match device.kind {
        DeviceKind::Magnet => [
            MagnetPayload::hz1000(0),
            MagnetPayload::hz500(0),
            MagnetPayload::hz2000(0),
        ]
        .iter()
        .map(|p| {
            // Frequencies are designations, not prose — they stay literal.
            let label = match p.frequency {
                MagnetFrequency::Hz1000 => "1000 Hz",
                MagnetFrequency::Hz500 => "500 Hz",
                MagnetFrequency::Hz2000 => "2000 Hz",
            };
            (label.to_string(), ron::to_string(p).expect("serializable"))
        })
        .collect(),
        DeviceKind::LineConductor => vec![(
            t!("action-payload-template"),
            ron::to_string(&LzbSection {
                length: 4000.0,
                cir_elke: false,
                end: false,
            })
            .expect("serializable"),
        )],
        DeviceKind::BlockMarker => vec![(
            t!("action-payload-template"),
            ron::to_string(&BlockMarkerPayload { section: 0 }).expect("serializable"),
        )],
        DeviceKind::Platform => vec![(
            t!("action-payload-template"),
            "(name:\"\",length:210.0)".into(),
        )],
        _ => return,
    };
    ui.horizontal_wrapped(|ui| {
        for (label, payload) in presets {
            if ui.small_button(label).on_hover_text(&payload).clicked() {
                device.payload = payload;
            }
        }
    });
}

/// Module tooling: the line's boundaries (named buffer nodes another module
/// may attach to) and the neighbour module drawn as a ghost.
/// Edge length the envelope is reset to when nothing else is dialled in [km].
const DEFAULT_ENVELOPE_KM: f64 = content::route::DEFAULT_ENVELOPE_HALF_SIZE * 2.0 / 1000.0;

/// The module's own extent: where it sits, and the envelope around it.
///
/// The envelope is edited on the map ([`crate::envelope`]); what belongs here is
/// what the map cannot say — how many corners it has, and the way back to a
/// square when dragging has gone wrong. A module from before envelopes has none
/// at all, and this is where it gets one.
fn envelope_rows(ui: &mut egui::Ui, line: &mut Line, state: &mut EditorState, focus: &mut Focus) {
    editor_ui::subheading(ui, t!("module-envelope"));
    if let Some(anchor) = line.source.anchor.as_mut() {
        editor_ui::form_grid("module-anchor").show(ui, |ui| {
            row(ui, "envelope-anchor-lat", |ui| {
                editor_ui::field(ui, &mut anchor.lat, 0.0001, -85.0..=85.0, "°");
            });
            row(ui, "envelope-anchor-lon", |ui| {
                editor_ui::field(ui, &mut anchor.lon, 0.0001, -180.0..=180.0, "°");
            });
        });
    }
    ui.small(t!("envelope-points", count = line.source.envelope.len()));
    ui.add_space(space::XS);
    let mut size_km = state.envelope_size.unwrap_or(DEFAULT_ENVELOPE_KM);
    ui.horizontal(|ui| {
        if ui.button(t!("action-edit-envelope")).clicked() {
            state.tool = tools::Tool::EditEnvelope;
        }
        // Resetting needs a place to build the square around: the module's
        // anchor, or — for a module that never had one — where the view is.
        let reset = ui
            .button(t!("action-reset-envelope"))
            .on_hover_text(t!("action-reset-envelope-hint"));
        if reset.clicked() {
            let anchor = line.source.anchor.unwrap_or_else(|| {
                let (lat, lon) = crate::focus_degrees(focus.position);
                content::route::GeoPoint {
                    lat,
                    lon,
                    height: 0.0,
                }
            });
            line.source.anchor = Some(anchor);
            line.source.envelope = content::route::default_envelope(anchor, size_km * 500.0);
            state.selection = Selection::None;
        }
        ui.add(editor_ui::drag(&mut size_km, 0.1, 0.2..=60.0, "km"))
            .on_hover_text(t!("new-module-size-hint"));
        if let Some(anchor) = line.source.anchor
            && ui.button(t!("action-center")).clicked()
        {
            focus.position = world_coords::geo::to_ecef_deg(anchor.lat, anchor.lon, anchor.height);
        }
    });
    state.envelope_size = Some(size_km);
}

/// Time of day over the module: the date and the clock the sky is drawn for.
///
/// Latitude and longitude are deliberately *not* here — they are the module's
/// anchor, three sections up, and the simulator reads the very same pair. The
/// sun that comes over the hill at half past six in this panel is the one the
/// run shows, because both are the same function of the same two numbers.
///
/// The slider is the point of the whole thing: dragging a day past in one
/// second is how a builder finds out that the platform lies in the shadow of
/// its own canopy all morning.
fn sky_section(ui: &mut egui::Ui, sky: &mut world_render::sky::Sky) {
    editor_ui::form_grid("sky").show(ui, |ui| {
        row(ui, "sky-date", |ui| {
            // Not `field`: three of those at the shared width run off the panel.
            ui.add(editor_ui::drag(&mut sky.day, 0.05, 1.0..=31.0, ""));
            ui.add(editor_ui::drag(&mut sky.month, 0.03, 1.0..=12.0, ""));
            ui.add(editor_ui::drag(&mut sky.year, 0.05, 1970.0..=2200.0, ""));
        });
        let (mut hour, mut minute) = (sky.hour(), sky.minute());
        let mut clock_changed = false;
        row(ui, "sky-time", |ui| {
            clock_changed = editor_ui::field(ui, &mut hour, 0.03, 0.0..=23.0, "h").changed()
                | editor_ui::field(ui, &mut minute, 0.1, 0.0..=59.0, "min").changed();
        });
        if clock_changed {
            sky.set_clock(hour, minute);
        }
        row(ui, "sky-zone", |ui| {
            editor_ui::field(ui, &mut sky.utc_offset, 0.02, -12.0..=14.0, "h");
        });
        row(ui, "sky-weather", |ui| {
            weather_combo(ui, &mut sky.weather);
        });
        row(ui, "sky-overcast", |ui| {
            editor_ui::field(ui, &mut sky.weather.cover, 0.01, 0.0..=1.0, "");
        });
        row(ui, "sky-visibility", |ui| {
            editor_ui::field(ui, &mut sky.weather.visibility, 20.0, 50.0..=40_000.0, "m");
        });
    });

    let mut hours = sky.seconds / 3600.0;
    if ui
        .add(
            egui::Slider::new(&mut hours, 0.0..=24.0)
                .show_value(false)
                .text(t!("sky-scrub")),
        )
        .changed()
    {
        sky.seconds = hours * 3600.0;
    }

    // Where the two bodies actually stand — the check that the date, the clock
    // and the anchor together mean what the builder thought they meant.
    let julian = sky.julian_date();
    let (azimuth, elevation) = world_coords::sun::sun_position(julian, sky.latitude, sky.longitude);
    let (_, moon_elevation, phase) =
        world_coords::sun::moon_position(julian, sky.latitude, sky.longitude);
    ui.add_space(space::XS);
    ui.small(t!(
        "sky-sun-at",
        elevation = format!("{:.0}", elevation.to_degrees()),
        azimuth = format!("{:.0}", azimuth.to_degrees()),
    ));
    ui.small(t!(
        "sky-moon-at",
        elevation = format!("{:.0}", moon_elevation.to_degrees()),
        phase = format!("{:.0}", phase * 100.0),
    ));
    ui.small(t!(
        "sky-place",
        lat = format!("{:.3}", sky.latitude.to_degrees()),
        lon = format!("{:.3}", sky.longitude.to_degrees()),
    ));
}

fn module_section(
    ui: &mut egui::Ui,
    line: &mut Line,
    state: &mut EditorState,
    ghost: &mut Ghost,
    focus: &mut Focus,
) {
    envelope_rows(ui, line, state, focus);
    ui.add_space(space::S);

    editor_ui::subheading(ui, t!("module-boundaries"));
    if line.source.boundaries.is_empty() {
        ui.small(t!("boundary-none"));
    } else {
        let mut remove = None;
        let mut center = None;
        editor_ui::form_grid("boundaries")
            .num_columns(4)
            .show(ui, |ui| {
                for (i, boundary) in line.source.boundaries.iter_mut().enumerate() {
                    ui.add(
                        egui::TextEdit::singleline(&mut boundary.name).desired_width(space::FIELD),
                    );
                    ui.label(
                        egui::RichText::new(t!("boundary-node", node = boundary.node))
                            .color(colors::TEXT_SECONDARY),
                    );
                    if ui.small_button(t!("action-center")).clicked() {
                        center = Some(boundary.node);
                    }
                    if ui.small_button("×").clicked() {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
        if let Some(node) = center
            && let Some(p) = tools::node_pos(&line.source, &line.net, node)
        {
            focus.position = p;
        }
        if let Some(i) = remove {
            line.source.boundaries.remove(i);
        }
    }
    // Boundaries live on the open ends of the selected track.
    if let Selection::Edge(i) = state.selection
        && let Some(edge) = line.source.edges.get(i)
    {
        let (from, to) = (edge.from, edge.to);
        ui.add_space(space::XS);
        ui.horizontal_wrapped(|ui| {
            for (node, key) in [
                (from, "action-add-boundary-start"),
                (to, "action-add-boundary-end"),
            ] {
                let is_buffer = matches!(
                    line.source.nodes.get(node as usize),
                    Some(NodeSource::Buffer)
                );
                let taken = line.source.boundaries.iter().any(|b| b.node == node);
                let button = ui.add_enabled(is_buffer && !taken, egui::Button::new(t!(key)));
                let button = button.on_disabled_hover_text(t!(if taken {
                    "boundary-taken"
                } else {
                    "boundary-needs-buffer"
                }));
                if button.clicked() {
                    line.source.boundaries.push(BoundarySource {
                        name: format!("b{node}"),
                        node,
                    });
                }
            }
        });
    } else {
        ui.small(t!("boundary-select-edge"));
    }

    ui.add_space(space::S);
    editor_ui::subheading(ui, t!("module-ghost"));
    if let Some(hint) = i18n::maybe("module-ghost-hint") {
        ui.small(hint);
    }
    ui.horizontal(|ui| {
        if ui.button(t!("action-load-ghost")).clicked() {
            load_ghost(state);
        }
        if ghost.net.is_some() && ui.button(t!("action-clear-ghost")).clicked() {
            ghost.path = None;
            ghost.net = None;
            ghost.boundaries.clear();
            ghost.respawn = true;
        }
    });
    if let Some(path) = &ghost.path {
        ui.add(
            egui::Label::new(
                egui::RichText::new(path)
                    .small()
                    .color(colors::TEXT_SECONDARY),
            )
            .truncate(),
        );
        ui.small(t!("ghost-boundaries", count = ghost.boundaries.len()));
    }
}

/// Loads another module read-only: its track becomes the grey ghost, its
/// boundaries become snap targets for the drawing tools.
fn load_ghost(state: &mut EditorState) {
    let filter = t!("filter-line-ron");
    ask_for_file(state, FileAsk::Ghost, move |dialog| {
        dialog.add_filter(filter, &["ron"]).pick_file()
    });
}

/// The neighbouring module the user picked, read as the grey ghost.
fn ghost_loaded(path: PathBuf, ghost: &mut Ghost, state: &EditorState, overlay: &mut Overlay) {
    let parsed = std::fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|text| LineSource::from_ron(&text).map_err(|e| e.to_string()))
        .and_then(|source| match source.compile() {
            Ok(compiled) => Ok((source, compiled)),
            Err(e) => Err(format!("{e:?}")),
        });
    match parsed {
        Ok((source, compiled)) => {
            ghost.boundaries = source
                .boundaries
                .iter()
                .filter_map(|b| {
                    Some((
                        b.name.clone(),
                        tools::node_pos(&source, &compiled.net, b.node)?,
                    ))
                })
                .collect();
            ghost.net = Some(compiled.net);
            ghost.path = Some(path.display().to_string());
            ghost.respawn = true;
            overlay.status = t!(
                "status-ghost-loaded",
                file = path.display(),
                boundaries = ghost.boundaries.len()
            );
        }
        Err(e) => report_failure(
            state,
            overlay,
            t!("status-error", file = path.display(), error = e),
        ),
    }
}

/// What the issue is about, for the center button: a world position to jump to
/// and the selection that puts its fields on screen.
fn issue_target(line: &Line, issue: &RuleIssue, focus: &Focus) -> (Option<EcefPos>, Selection) {
    let device_target = |device: u32| {
        let position = line
            .source
            .devices
            .get(device as usize)
            .and_then(|d| tools::device_pos(&line.net, d));
        (position, Selection::Device(device as usize))
    };
    match issue {
        RuleIssue::DeviceOffEdge { device }
        | RuleIssue::MagnetPayloadInvalid { device }
        | RuleIssue::BlockMarkerPayloadInvalid { device } => device_target(*device),
        RuleIssue::DistantWithout1000Hz { signal }
        | RuleIssue::MainWithout2000Hz { signal }
        | RuleIssue::DistantWithoutNext { signal }
        | RuleIssue::SignalDeviceMismatch { signal } => {
            match line.source.signals.get(*signal as usize) {
                Some(s) => device_target(s.device),
                None => (None, Selection::None),
            }
        }
        RuleIssue::BoundaryInvalid { boundary } => (
            line.source
                .boundaries
                .get(*boundary as usize)
                .and_then(|b| tools::node_pos(&line.source, &line.net, b.node)),
            Selection::None,
        ),
        RuleIssue::UnknownTrackType { edge } | RuleIssue::LzbTypeWithoutConductor { edge } => (
            line.net
                .edges()
                .get(*edge as usize)
                .map(|e| e.eval(e.length() / 2.0).pos),
            Selection::Edge(*edge as usize),
        ),
        RuleIssue::AreaOffTrack { area }
        | RuleIssue::AreaWithoutEffect { area }
        | RuleIssue::AreaUnknownTrackType { area } => (
            line.source
                .areas
                .get(*area as usize)
                .and_then(|a| a.spans.first())
                .and_then(|span| {
                    let edge = line.net.edges().get(span.edge as usize)?;
                    let s = ((span.from + span.to) / 2.0).clamp(0.0, edge.length());
                    Some(edge.eval(s).pos)
                }),
            Selection::TrackArea(*area as usize),
        ),
        RuleIssue::FlankGuardInvalid { route } => match line.source.routes.get(*route as usize) {
            Some(route) => match line.source.signals.get(route.entry as usize) {
                Some(signal) => device_target(signal.device),
                None => (None, Selection::None),
            },
            None => (None, Selection::None),
        },
        RuleIssue::ObjectOffEdge { object } | RuleIssue::UnknownObject { object } => (
            line.source
                .objects
                .get(*object as usize)
                .and_then(|o| tools::object_pos(&line.net, o)),
            Selection::Object(*object as usize),
        ),
        // The first corner of the envelope: the boundary is what has to move,
        // and this is where looking at it starts.
        RuleIssue::OutsideEnvelope { .. } | RuleIssue::EnvelopeSelfIntersects => (
            line.source
                .envelope
                .first()
                .map(|c| crate::envelope::point_pos(c, crate::envelope::height(line, focus))),
            Selection::None,
        ),
    }
}

fn issue_text(issue: &RuleIssue) -> String {
    match issue {
        RuleIssue::DeviceOffEdge { device } => t!("check-device-off-edge", device = device),
        RuleIssue::MagnetPayloadInvalid { device } => t!("check-magnet-payload", device = device),
        RuleIssue::BlockMarkerPayloadInvalid { device } => {
            t!("check-blockmarker-payload", device = device)
        }
        RuleIssue::DistantWithout1000Hz { signal } => {
            t!("check-distant-no-1000hz", signal = signal)
        }
        RuleIssue::MainWithout2000Hz { signal } => t!("check-main-no-2000hz", signal = signal),
        RuleIssue::DistantWithoutNext { signal } => t!("check-distant-no-next", signal = signal),
        RuleIssue::SignalDeviceMismatch { signal } => t!("check-signal-device", signal = signal),
        RuleIssue::BoundaryInvalid { boundary } => {
            t!("check-boundary-invalid", boundary = boundary)
        }
        RuleIssue::UnknownTrackType { edge } => t!("check-unknown-track-type", edge = edge),
        RuleIssue::AreaOffTrack { area } => t!("check-area-off-track", area = area),
        RuleIssue::AreaWithoutEffect { area } => t!("check-area-no-effect", area = area),
        RuleIssue::AreaUnknownTrackType { area } => t!("check-area-track-type", area = area),
        RuleIssue::LzbTypeWithoutConductor { edge } => t!("check-lzb-no-conductor", edge = edge),
        RuleIssue::ObjectOffEdge { object } => t!("check-object-off-edge", object = object),
        RuleIssue::UnknownObject { object } => t!("check-unknown-object", object = object),
        RuleIssue::FlankGuardInvalid { route } => t!("check-flank-guard", route = route),
        RuleIssue::EnvelopeSelfIntersects => t!("check-envelope-crossed"),
        RuleIssue::OutsideEnvelope {
            trees,
            terrain,
            markers,
        } => t!(
            "check-outside-envelope",
            trees = trees,
            terrain = terrain,
            markers = markers
        ),
    }
}

/// Findings of the rule check, refreshed with every rebuild — each one jumps
/// to the thing it is about.
fn checks_section(ui: &mut egui::Ui, line: &mut Line, state: &mut EditorState, focus: &mut Focus) {
    if line.issues.is_empty() {
        ui.small(t!("check-ok"));
        return;
    }
    let issues = line.issues.clone();
    for issue in &issues {
        ui.horizontal(|ui| {
            let (position, selection) = issue_target(line, issue, focus);
            if ui
                .add_enabled(
                    position.is_some(),
                    egui::Button::new(t!("action-center")).small(),
                )
                .clicked()
            {
                if let Some(p) = position {
                    focus.position = p;
                }
                if selection != Selection::None {
                    state.selection = selection;
                }
            }
            ui.add(
                egui::Label::new(egui::RichText::new(issue_text(issue)).color(colors::WARN)).wrap(),
            );
        });
    }
}

/// Height data of the module: which DGM delivery it is cut from, at what grid
/// spacing, and how much of the corridor is covered. The tile tool picks single
/// tiles on the map; without a pick the import covers the whole corridor.
fn height_section(
    ui: &mut egui::Ui,
    line: &mut Line,
    state: &mut EditorState,
    overlay: &mut Overlay,
) {
    editor_ui::form_grid("heights").show(ui, |ui| {
        row(ui, "dgm-source", |ui| {
            if ui.button(t!("action-choose-dgm")).clicked() {
                ask_for_file(state, FileAsk::DgmFolder, |dialog| dialog.pick_folder());
            }
        });
        row(ui, "dgm-zone", |ui| {
            let mut zone = state.dgm_zone() as f64;
            if editor_ui::field(ui, &mut zone, 1.0, 32.0..=33.0, "").changed() {
                state.dgm_zone = Some(zone as u8);
            }
        });
        row(ui, "dgm-cell", |ui| {
            let mut cell = state.dgm_cell();
            if editor_ui::field(ui, &mut cell, 1.0, 1.0..=100.0, "m").changed() {
                state.dgm_cell = Some(cell);
            }
        });
    });
    if let Some(source) = &state.dgm_source {
        ui.label(
            egui::RichText::new(source.clone())
                .monospace()
                .color(colors::TEXT_SECONDARY),
        );
    }

    ui.add_space(space::XS);
    let corridor = tools::corridor_tiles(line, state.terrain_options()).len();
    ui.small(t!(
        "dgm-coverage",
        have = state.dgm_present.len(),
        total = corridor
    ));
    if !state.picked_tiles.is_empty() {
        ui.small(t!("dgm-picked", count = state.picked_tiles.len()));
    }

    ui.add_space(space::XS);
    ui.horizontal(|ui| {
        if ui.button(t!("action-import-heights-all")).clicked() {
            import_heights(line, state, overlay, true);
        }
        let picked = egui::Button::new(t!("action-import-heights-picked"));
        if ui
            .add_enabled(!state.picked_tiles.is_empty(), picked)
            .clicked()
        {
            import_heights(line, state, overlay, false);
        }
    });
    ui.horizontal(|ui| {
        let clear = egui::Button::new(t!("action-clear-picked"));
        if ui
            .add_enabled(!state.picked_tiles.is_empty(), clear)
            .clicked()
        {
            state.picked_tiles.clear();
        }
        let drop = egui::Button::new(t!("action-drop-heights"));
        if ui
            .add_enabled(!line.source.heights.is_empty(), drop)
            .clicked()
        {
            // Only the reference goes; the tiles stay on disk, so a mistaken
            // click costs a re-import at worst, not the cut-out.
            line.source.heights.clear();
        }
    });
}

/// Reference markers by layer: a checkbox that hides the layer on the map, its
/// marker count, and a button that deletes the whole layer. Hiding is session
/// state, deleting is an edit — the two live in the same row because that is
/// where the question is asked ("do I still need this?").
fn marker_section(
    ui: &mut egui::Ui,
    line: &mut Line,
    state: &mut EditorState,
    focus: &mut Focus,
    marks: &crate::terrain::Marks,
) {
    let layers = tools::marker_layers(line);
    if layers.is_empty() {
        ui.small(t!("marker-none"));
        return;
    }
    // Deleting inside the loop would shift the indices the rows are drawn from.
    let mut delete: Option<String> = None;
    for (layer, count) in &layers {
        ui.horizontal(|ui| {
            let mut visible = state.layer_visible(layer);
            if ui.checkbox(&mut visible, "").changed() {
                if visible {
                    state.hidden_layers.remove(layer);
                } else {
                    state.hidden_layers.insert(layer.clone());
                }
            }
            ui.label(layer);
            ui.label(
                egui::RichText::new(format!("{count}"))
                    .monospace()
                    .color(colors::TEXT_SECONDARY),
            );
            // First marker of the layer as the place to look at.
            if ui.button(t!("action-center")).clicked()
                && let Some((i, marker)) = line
                    .source
                    .markers
                    .iter()
                    .enumerate()
                    .find(|(_, m)| &m.layer == layer)
            {
                focus.position = marks.marker(i, marker);
            }
            if ui.button(t!("action-delete-layer")).clicked() {
                delete = Some(layer.clone());
            }
        });
    }
    if let Some(layer) = delete {
        tools::delete_layer(line, state, &layer);
    }
    ui.add_space(space::XS);
    ui.small(t!("marker-total", count = line.source.markers.len()));
}

/// The aerial imagery template, editable in place. Edits go through
/// `Request::config` so the panel, the menu and the letter keys all apply
/// changes on the same code path in `overlay_control`.
fn imagery_section(ui: &mut egui::Ui, overlay: &Overlay, request: &mut Request) {
    let mut config = overlay.config().clone();
    let mut changed = false;
    let mut rebuild = false;

    editor_ui::form_grid("imagery").show(ui, |ui| {
        row(ui, "img-enabled", |ui| {
            changed |= ui.checkbox(&mut config.enabled, "").changed();
        });
        row(ui, "img-provider", |ui| {
            let current = config
                .provider()
                .map(|p| p.name.clone())
                .unwrap_or_else(|| t!("common-none"));
            let choices: Vec<(String, String)> = config
                .providers
                .iter()
                .map(|p| (p.id.clone(), p.name.clone()))
                .collect();
            egui::ComboBox::from_id_salt("img-provider")
                .width(space::FIELD)
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for (id, name) in choices {
                        if ui.selectable_label(config.active == id, name).clicked()
                            && config.active != id
                        {
                            config.active = id;
                            changed = true;
                            rebuild = true;
                        }
                    }
                });
        });
        row(ui, "img-opacity", |ui| {
            let mut percent = (config.opacity * 100.0).round() as f64;
            if editor_ui::field(ui, &mut percent, 1.0, 0.0..=100.0, "%").changed() {
                config.opacity = (percent / 100.0) as f32;
                changed = true;
                rebuild = true;
            }
        });
        row(ui, "img-zoom", |ui| {
            let is_fixed = matches!(config.zoom, ZoomMode::Fixed(_));
            egui::ComboBox::from_id_salt("img-zoom-mode")
                .width(space::FIELD)
                .selected_text(if is_fixed {
                    t!("zoom-fixed")
                } else {
                    t!("zoom-auto")
                })
                .show_ui(ui, |ui| {
                    if ui.selectable_label(!is_fixed, t!("zoom-auto")).clicked() && is_fixed {
                        config.zoom = ZoomMode::Resolution(0.5);
                        changed = true;
                    }
                    if ui.selectable_label(is_fixed, t!("zoom-fixed")).clicked() && !is_fixed {
                        // Freeze at the level the automatic just used.
                        config.zoom = ZoomMode::Fixed(overlay.zoom.max(1));
                        changed = true;
                    }
                });
            if let ZoomMode::Fixed(level) = &mut config.zoom {
                changed |= ui
                    .add(editor_ui::drag(level, 1.0, 1.0..=20.0, ""))
                    .changed();
            } else {
                ui.small(t!("zoom-current", level = overlay.zoom));
            }
        });
        row(ui, "img-radius", |ui| {
            if editor_ui::field(ui, &mut config.radius, 25.0, 200.0..=2_000.0, "m")
                .on_hover_text(t!("img-radius-hint"))
                .changed()
            {
                changed = true;
            }
        });
        row(ui, "img-offset", |ui| {
            let east = ui.add(editor_ui::drag(
                &mut config.offset.0,
                0.5,
                -200.0..=200.0,
                "m",
            ));
            let north = ui.add(editor_ui::drag(
                &mut config.offset.1,
                0.5,
                -200.0..=200.0,
                "m",
            ));
            if east.changed() || north.changed() {
                changed = true;
                rebuild = true;
            }
            if config.offset != (0.0, 0.0) && ui.small_button(t!("action-reset")).clicked() {
                config.offset = (0.0, 0.0);
                changed = true;
                rebuild = true;
            }
        });
        row(ui, "img-offline", |ui| {
            changed |= ui.checkbox(&mut config.cache.offline, "").changed();
        });
    });
    ui.add_space(space::XS);
    ui.small(t!(
        "tiles-summary",
        shown = overlay.tiles_shown(),
        pending = overlay.source.pending()
    ));
    if let Some(provider) = overlay.config().provider()
        && !provider.attribution.is_empty()
    {
        // The credit already carries its own © where the provider uses one.
        match &provider.attribution_url {
            Some(url) => ui.hyperlink_to(
                egui::RichText::new(&provider.attribution).small(),
                url.clone(),
            ),
            None => ui.small(&provider.attribution),
        };
    }

    if changed {
        request.config = Some((config, rebuild));
    }
}

/// Cache read-outs plus the two actions that were menu-only before.
fn cache_section(ui: &mut egui::Ui, overlay: &Overlay, request: &mut Request) {
    let stats = overlay.source.cache_stats();
    ui.label(t!(
        "cache-summary",
        hits = stats.hits_memory + stats.hits_disk,
        disk = stats.hits_disk,
        stored = stats.stored,
        evicted = stats.evicted
    ));
    ui.label(t!(
        "cache-size",
        megabytes = format!("{:.1}", overlay.source.disk_usage() as f64 / 1e6),
        directory = overlay.config().cache.directory.display()
    ));
    ui.add_space(space::XS);
    ui.horizontal(|ui| {
        if ui.button(t!("overlay-clear-cache")).clicked() {
            request.clear_cache = true;
        }
        if ui.button(t!("overlay-retry")).clicked() {
            request.retry_failed = true;
        }
    });
    let errors: Vec<&String> = overlay.source.errors.iter().rev().take(3).collect();
    if !errors.is_empty() {
        ui.add_space(space::XS);
        ui.label(egui::RichText::new(t!("group-errors")).strong());
        for error in errors {
            ui.small(error);
        }
    }
}

/// Form row: i18n label (tooltip from `<key>-hint` when present), then the widget.
pub(crate) fn row(ui: &mut egui::Ui, key: &str, widget: impl FnOnce(&mut egui::Ui)) {
    let label = editor_ui::form_label(ui, t!(key));
    if let Some(hint) = i18n::maybe(&format!("{key}-hint")) {
        label.on_hover_text(hint);
    }
    ui.horizontal(|ui| widget(ui));
    ui.end_row();
}

fn kinds() -> [DeviceKind; 9] {
    [
        DeviceKind::Signal,
        DeviceKind::Magnet,
        DeviceKind::LineConductor,
        DeviceKind::Balise,
        DeviceKind::SpeedBoard,
        DeviceKind::Platform,
        DeviceKind::StopBoard,
        DeviceKind::BlockMarker,
        DeviceKind::NeutralSection,
    ]
}

fn kind_label(kind: &DeviceKind) -> String {
    match kind {
        DeviceKind::Signal => t!("kind-signal"),
        DeviceKind::Magnet => t!("kind-magnet"),
        DeviceKind::LineConductor => t!("kind-line-conductor"),
        DeviceKind::Balise => t!("kind-balise"),
        DeviceKind::SpeedBoard => t!("kind-speed-board"),
        DeviceKind::Platform => t!("kind-platform"),
        DeviceKind::StopBoard => t!("kind-stop-board"),
        DeviceKind::BlockMarker => t!("kind-block-marker"),
        DeviceKind::NeutralSection => t!("kind-neutral-section"),
        // A country-package kind keeps its name — it is an identifier, not prose.
        DeviceKind::Other(name) => name.clone(),
    }
}

fn kind_combo(ui: &mut egui::Ui, id: &str, kind: &mut DeviceKind) {
    egui::ComboBox::from_id_salt(id)
        .width(space::FIELD)
        .selected_text(kind_label(kind))
        .show_ui(ui, |ui| {
            for candidate in kinds() {
                if ui
                    .selectable_label(*kind == candidate, kind_label(&candidate))
                    .clicked()
                {
                    *kind = candidate;
                }
            }
        });
}

fn facing_label(facing: Facing) -> String {
    match facing {
        Facing::Forward => t!("facing-forward"),
        Facing::Backward => t!("facing-backward"),
        Facing::Both => t!("facing-both"),
    }
}

/// Weather picker. The presets are `sim_core::weather`'s own list, and choosing one
/// writes the numbers behind it — cover, sight and what falls — into the sky. Edit
/// any of them afterwards and the box says so.
fn weather_combo(ui: &mut egui::Ui, weather: &mut sim_core::weather::Weather) {
    use sim_core::weather::Preset;
    let current = Preset::of(*weather);
    egui::ComboBox::from_id_salt("sky-weather")
        .width(space::FIELD)
        .selected_text(current.map_or_else(|| t!("weather-custom"), weather_label))
        .show_ui(ui, |ui| {
            for preset in Preset::ALL {
                if ui
                    .selectable_label(current == Some(preset), weather_label(preset))
                    .clicked()
                {
                    *weather = preset.weather();
                }
            }
        });
}

fn weather_label(preset: sim_core::weather::Preset) -> String {
    use sim_core::weather::Preset;
    match preset {
        Preset::Clear => t!("weather-clear"),
        Preset::Cloudy => t!("weather-cloudy"),
        Preset::Overcast => t!("weather-overcast"),
        Preset::Fog => t!("weather-fog"),
        Preset::Drizzle => t!("weather-drizzle"),
        Preset::Rain => t!("weather-rain"),
        Preset::Storm => t!("weather-storm"),
        Preset::Thunderstorm => t!("weather-thunderstorm"),
        Preset::Sleet => t!("weather-sleet"),
        Preset::Snow => t!("weather-snow"),
        Preset::Blizzard => t!("weather-blizzard"),
        Preset::Hail => t!("weather-hail"),
        Preset::Frost => t!("weather-frost"),
    }
}

fn facing_combo(ui: &mut egui::Ui, facing: &mut Facing) {
    egui::ComboBox::from_id_salt("facing")
        .width(space::FIELD)
        .selected_text(facing_label(*facing))
        .show_ui(ui, |ui| {
            for candidate in [Facing::Forward, Facing::Backward, Facing::Both] {
                if ui
                    .selectable_label(*facing == candidate, facing_label(candidate))
                    .clicked()
                {
                    *facing = candidate;
                }
            }
        });
}

/// Language picker.
fn language_menu(ui: &mut egui::Ui) {
    ui.menu_button(t!("menu-language"), |ui| {
        let current = i18n::language();
        for (code, name) in i18n::LANGUAGES {
            if ui.selectable_label(current == *code, *name).clicked() {
                i18n::set_language(code);
                ui.close();
            }
        }
    });
}
