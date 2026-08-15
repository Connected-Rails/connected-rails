//! Desktop UI of the vehicle editor: menu bar, data panel, model panel, status bar.
//!
//! Look and feel come from the `editor-ui` crate; this file only lays out the
//! forms. Every labelled field goes through [`row`], every section through
//! `editor_ui::section`, so labels and fields line up across the whole panel.

use crate::{Editor, Status, model, powertrain, sounds};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use editor_ui::{colors, field, space};
use i18n::t;
use sim_core::doors::DoorSystem;
use sim_core::safety::SafetyEquipment;
use sim_core::safety::de::{PzbVariant, SifaKind, TrainType};
use sim_core::train::{CouplerSpec, Motion, Part, VehicleSpec};

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

/// One frame of UI.
///
/// Since egui 0.35 panels live inside a `Ui`, not on the context: the whole viewport
/// becomes one background `Ui` into which the panels are docked.
pub fn draw(
    mut contexts: EguiContexts,
    mut editor: ResMut<Editor>,
    mut assets: ResMut<AssetServer>,
    mut themed: Local<bool>,
    mut active: Local<Option<&'static str>>,
    mut view: ResMut<crate::View>,
    windows: Query<&bevy::window::RawHandleWrapper, With<bevy::window::PrimaryWindow>>,
) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
    // Kept on the editor so every dialog site (including the close handler in
    // `main.rs`) can name the window as its owner.
    editor.window = windows.single().ok().cloned();
    if !*themed {
        // Fonts installed by `apply` become active with the next pass — skip
        // one frame so nothing draws with a font family that is not there yet.
        editor_ui::apply(&ctx);
        *themed = true;
        return Ok(());
    }
    handle_shortcuts(&ctx, &mut editor);

    // A vehicle that was just opened brings its model along.
    let file = editor
        .spec
        .model
        .as_ref()
        .map(|m| m.file.clone())
        .unwrap_or_default();
    if !file.is_empty() && editor.loaded_file != file {
        editor.loaded_file = file.clone();
        editor.nodes.clear();
        editor.gltf = Some(assets.load(format!("{}://{file}", crate::MOD_SOURCE)));
        editor.status = Status::Info(t!("status-loading", file = file));
    }
    let mut root = egui::Ui::new(
        ctx.clone(),
        "viewport".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    menu_bar(&mut root, &mut editor, &mut assets);
    // Snapshot after the menu bar: opening a file or starting a new vehicle
    // replaces the spec wholesale, which is not an edit and has no business in
    // the history. Everything below is.
    let before = editor.spec.clone();
    status_bar(&mut root, &editor);
    let left = data_panel(&mut root, &mut editor, &mut active);
    let right = model_panel(&mut root, &mut editor, &mut assets);
    // In memory only; written when the user leaves.
    editor.settings.panels = Some((left, right));
    track_changes(&mut editor, before);
    // What the panels left free is the 3D viewport; the camera in
    // `orbit_camera` only takes the mouse inside this rect.
    let free = root.available_rect_before_wrap();
    view.viewport = Rect::new(free.min.x, free.min.y, free.max.x, free.max.y);
    viewport_hint(&ctx, &root, &view);
    Ok(())
}

/// Says how to move the camera, in the viewport, until the user has done it.
///
/// The 3D view is the largest thing on screen and the only one with no visible
/// control at all; right-drag to orbit is a convention of modelling tools, not
/// something a modder coming from a text editor knows. Two clicks deep in the
/// Help menu it is findable only by someone who already suspects it exists.
/// Once the camera has moved the hint has done its job and goes.
fn viewport_hint(ctx: &egui::Context, root: &egui::Ui, view: &crate::View) {
    let free = root.available_rect_before_wrap();
    if view.used || free.width() < 240.0 {
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
        ui.label(
            egui::RichText::new(t!("help-mouse"))
                .small()
                .color(colors::TEXT_SECONDARY),
        );
    });
}

/// Turns "the spec differs from the start of the frame" into the unsaved flag
/// and into undo steps.
///
/// One step per continuous interaction, not per frame: dragging a value
/// changes the spec in every frame of the drag, so only the first of them
/// records the state the user left.
fn track_changes(editor: &mut Editor, before: VehicleSpec) {
    let changed = editor.spec != before;
    if changed && !editor.changing {
        editor.remember(before);
    }
    editor.dirty |= changed;
    editor.changing = changed;
}

fn handle_shortcuts(ctx: &egui::Context, editor: &mut Editor) {
    // Redo first: Ctrl+Shift+Z must not be eaten by the plain Ctrl+Z.
    if ctx
        .input_mut(|i| i.consume_shortcut(&SHORTCUT_REDO) || i.consume_shortcut(&SHORTCUT_REDO_ALT))
    {
        editor.redo();
    }
    if ctx.input_mut(|i| i.consume_shortcut(&SHORTCUT_UNDO)) {
        editor.undo();
    }
    if ctx.input_mut(|i| i.consume_shortcut(&SHORTCUT_SAVE)) && needs_saving(editor) {
        save(editor);
    }
    if ctx.input_mut(|i| i.consume_shortcut(&SHORTCUT_OPEN)) && confirm_discard(editor) {
        open(editor);
    }
    if ctx.input_mut(|i| i.consume_shortcut(&SHORTCUT_NEW)) && confirm_discard(editor) {
        *editor = Editor::default();
    }
}

/// The owner of every native dialog. Without one, Windows is free to open the
/// dialog behind the editor window — where a modal that blocks all input reads
/// as a hang.
fn dialog_parent(editor: &Editor) -> Option<bevy::window::ThreadLockedRawWindowHandleWrapper> {
    // SAFETY: the handle is only handed to rfd as the dialog owner; nothing
    // draws or resizes through it.
    editor.window.as_ref().map(|w| unsafe { w.get_handle() })
}

/// Starts a message dialog owned by the editor window.
fn message_dialog(editor: &Editor) -> rfd::MessageDialog {
    match dialog_parent(editor) {
        Some(parent) => rfd::MessageDialog::new().set_parent(&parent),
        None => rfd::MessageDialog::new(),
    }
}

/// Starts a file dialog owned by the editor window.
fn file_dialog(editor: &Editor) -> rfd::FileDialog {
    match dialog_parent(editor) {
        Some(parent) => rfd::FileDialog::new().set_parent(&parent),
        None => rfd::FileDialog::new(),
    }
}

/// Asks before unsaved work is thrown away; `true` means go ahead.
///
/// The status bar reports the unsaved state but does not defend it — before
/// this, quitting, opening another vehicle or starting a new one dropped an
/// afternoon of numbers without a word.
pub fn confirm_discard(editor: &mut Editor) -> bool {
    if !editor.dirty {
        return true;
    }
    match message_dialog(editor)
        .set_level(rfd::MessageLevel::Warning)
        .set_title(t!("confirm-unsaved-title"))
        .set_description(t!("confirm-unsaved", name = editor.spec.name))
        .set_buttons(rfd::MessageButtons::YesNoCancel)
        .show()
    {
        // Saving can still be called off in the file dialog. Nothing was
        // written then, so the answer is no longer yes.
        rfd::MessageDialogResult::Yes => {
            save(editor);
            !editor.dirty
        }
        rfd::MessageDialogResult::No => true,
        _ => false,
    }
}

/// Whether Save would do anything.
///
/// Writing an unchanged file is not harmless here: `ron::ser` re-serialises
/// the struct, so anything the editor does not model — the comments a
/// hand-written vehicle file carries — is gone. Open, Ctrl+S out of habit,
/// and the file is stripped for nothing. A vehicle that has no file yet is a
/// different case: there, Save has something to do.
fn needs_saving(editor: &Editor) -> bool {
    editor.dirty || editor.path.is_none()
}

/// Puts a failure in front of the user.
///
/// The status bar sits in the corner furthest from the menu the action came
/// from, and a RON file with a syntax error fails silently as far as the eye
/// is concerned: the previous vehicle simply stays on screen. Only the paths
/// the user triggered report this way — `main` opens the file named on the
/// command line without one, so a headless run never waits on a modal.
fn report_failure(editor: &Editor) {
    if editor.status.is_error() {
        message_dialog(editor)
            .set_level(rfd::MessageLevel::Error)
            .set_title(t!("dialog-error-title"))
            .set_description(editor.status.text())
            .show();
    }
}

/// Warns before a hand-written file loses its comments; `true` means go ahead.
///
/// `ron::ser` writes the struct, not the file — every comment in a vehicle
/// kept by hand is gone the first time the editor saves over it. That is how
/// the example vehicle in this repository lost its own. Asked once per
/// session: whoever has said yes knows.
fn confirm_comment_loss(editor: &mut Editor, path: &std::path::Path) -> bool {
    if editor.warned_about_comments {
        return true;
    }
    // A line whose first non-blank characters are `//`. A string value could
    // in principle start that way; a vehicle file does not.
    let has_comments = std::fs::read_to_string(path)
        .map(|text| text.lines().any(|line| line.trim_start().starts_with("//")))
        .unwrap_or(false);
    if !has_comments {
        return true;
    }
    let go_ahead = message_dialog(editor)
        .set_level(rfd::MessageLevel::Warning)
        .set_title(t!("confirm-comments-title"))
        .set_description(t!("confirm-comments", file = path.display()))
        .set_buttons(rfd::MessageButtons::OkCancel)
        .show()
        == rfd::MessageDialogResult::Ok;
    editor.warned_about_comments |= go_ahead;
    go_ahead
}

/// Save to the known path, or fall back to the save dialog.
fn save(editor: &mut Editor) {
    match editor.path.clone() {
        Some(path) => {
            if !confirm_comment_loss(editor, &path) {
                return;
            }
            editor.save(path);
            report_failure(editor);
        }
        None => save_as(editor),
    }
}

fn save_as(editor: &mut Editor) {
    if let Some(path) = file_dialog(editor)
        .add_filter(t!("filter-vehicle-ron"), &["ron"])
        .set_file_name("vehicle.ron")
        .save_file()
    {
        editor.save(path);
        report_failure(editor);
    }
}

fn open(editor: &mut Editor) {
    if let Some(path) = file_dialog(editor)
        .add_filter(t!("filter-vehicle-ron"), &["ron"])
        .pick_file()
    {
        editor.open(path);
        report_failure(editor);
    }
}

fn menu_bar(root: &mut egui::Ui, editor: &mut Editor, assets: &mut AssetServer) {
    egui::Panel::top("menu")
        .frame(editor_ui::bar_frame())
        .show(root, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                let ctx = ui.ctx().clone();
                ui.menu_button(t!("menu-file"), |ui| {
                    if ui
                        .add(
                            egui::Button::new(t!("action-new"))
                                .shortcut_text(ctx.format_shortcut(&SHORTCUT_NEW)),
                        )
                        .clicked()
                    {
                        if confirm_discard(editor) {
                            *editor = Editor::default();
                        }
                        ui.close();
                    }
                    if ui
                        .add(
                            egui::Button::new(t!("action-open"))
                                .shortcut_text(ctx.format_shortcut(&SHORTCUT_OPEN)),
                        )
                        .clicked()
                    {
                        if confirm_discard(editor) {
                            open(editor);
                        }
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            needs_saving(editor),
                            egui::Button::new(t!("action-save"))
                                .shortcut_text(ctx.format_shortcut(&SHORTCUT_SAVE)),
                        )
                        .clicked()
                    {
                        save(editor);
                        ui.close();
                    }
                    // File names, not paths: the path is long and the same
                    // for most of them. The full one is on hover.
                    let recent = editor.settings.recent.clone();
                    ui.add_enabled_ui(!recent.is_empty(), |ui| {
                        ui.menu_button(t!("menu-recent"), |ui| {
                            for path in recent {
                                let name = path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| path.display().to_string());
                                // A vehicle that has been moved or deleted
                                // stays in the list; offering it as if it
                                // would open is the same empty promise as an
                                // enabled Save with nothing to save.
                                let there = path.is_file();
                                if ui
                                    .add_enabled(there, egui::Button::new(name))
                                    .on_hover_text(path.display().to_string())
                                    .on_disabled_hover_text(t!("recent-missing"))
                                    .clicked()
                                {
                                    if confirm_discard(editor) {
                                        editor.open(path);
                                        report_failure(editor);
                                    }
                                    ui.close();
                                }
                            }
                        });
                    });
                    if ui.button(t!("action-save-as")).clicked() {
                        save_as(editor);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(t!("action-import-model")).clicked() {
                        import_model(editor, assets);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(t!("action-quit")).clicked() && confirm_discard(editor) {
                        editor.settings.save();
                        std::process::exit(0);
                    }
                });
                // Undo lives in a menu as well as on the keyboard: a command
                // reachable only by shortcut is a command most users never
                // learn they have.
                ui.menu_button(t!("menu-edit"), |ui| {
                    if ui
                        .add_enabled(
                            !editor.undo.is_empty(),
                            egui::Button::new(t!("action-undo"))
                                .shortcut_text(ctx.format_shortcut(&SHORTCUT_UNDO)),
                        )
                        .clicked()
                    {
                        editor.undo();
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            !editor.redo.is_empty(),
                            egui::Button::new(t!("action-redo"))
                                .shortcut_text(ctx.format_shortcut(&SHORTCUT_REDO)),
                        )
                        .clicked()
                    {
                        editor.redo();
                        ui.close();
                    }
                });
                ui.menu_button(t!("menu-view"), |ui| {
                    let reference =
                        ui.checkbox(&mut editor.show_reference, t!("view-reference-body"));
                    let grid = ui.checkbox(&mut editor.show_grid, t!("view-grid"));
                    if reference.changed() || grid.changed() {
                        let (reference, grid) = (editor.show_reference, editor.show_grid);
                        editor.settings.set_view(reference, grid);
                    }
                    ui.separator();
                    language_menu(ui, &mut editor.settings);
                });
                ui.menu_button(t!("menu-help"), |ui| {
                    // Reference, not onboarding: the viewport hint is gone
                    // once the camera has been used, and this is where someone
                    // looks the controls up again.
                    ui.label(t!("help-mouse"));
                    ui.label(t!("help-model-conventions"));
                    ui.separator();
                    // The first question on any bug report from a modder.
                    if ui.button(t!("action-about")).clicked() {
                        message_dialog(editor)
                            .set_title(t!("window-vehicle-editor"))
                            .set_description(t!(
                                "about-version",
                                version = env!("CARGO_PKG_VERSION")
                            ))
                            .show();
                        ui.close();
                    }
                });
            });
        });
}

/// Language picker. The choice is a standing preference, not a mood — it has
/// to survive the next start, or the user makes it again every time.
fn language_menu(ui: &mut egui::Ui, settings: &mut crate::settings::Settings) {
    ui.menu_button(t!("menu-language"), |ui| {
        let current = i18n::language();
        for (code, name) in i18n::LANGUAGES {
            if ui.selectable_label(current == *code, *name).clicked() {
                i18n::set_language(code);
                settings.set_language(code);
                ui.close();
            }
        }
    });
}

/// Opens a glTF file. The path is stored relative to the `mods/` directory, because that is
/// how the simulator finds it later (`mods://<mod>/assets/…`).
fn import_model(editor: &mut Editor, assets: &mut AssetServer) {
    let Some(path) = file_dialog(editor)
        .add_filter(t!("filter-model-gltf"), &["gltf", "glb"])
        .set_directory(crate::mods_dir())
        .pick_file()
    else {
        return;
    };
    let Ok(relative) = path.strip_prefix(crate::mods_dir()) else {
        editor.status = Status::Error(t!("status-outside-mods", path = path.display()));
        report_failure(editor);
        return;
    };
    let file = relative.to_string_lossy().replace('\\', "/");
    editor.model_mut().file = file.clone();
    editor.loaded_file = file.clone();
    editor.nodes.clear();
    editor.gltf = Some(assets.load(format!("{}://{file}", crate::MOD_SOURCE)));
    editor.dirty = true;
    editor.status = Status::Info(t!("status-loading", file = file));
}

/// The sections of the data panel, in the order they are drawn: id and the
/// i18n key of the title. The jump bar sits above the scroll area and has to
/// name them before the first one has been laid out.
const SECTIONS: [(&str, &str); 9] = [
    ("base", "group-base-data"),
    ("gear", "group-running-gear"),
    ("coupler", "group-coupler"),
    ("resistance", "group-resistance"),
    ("brake", "group-brake"),
    ("drive", "group-drive"),
    ("equipment", "group-equipment"),
    ("sounds", "group-sounds"),
    ("behaviour", "group-behaviour"),
];

/// A section of the data panel that the jump bar can scroll to.
///
/// A collapsed section stays collapsed when jumped to — the user closed it on
/// purpose, and the header they land on is enough to open it again.
///
/// Also reports itself in `current` while its header is at or above the top of
/// the visible area; the sections are drawn in order, so the last one to do so
/// is the one being read.
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

/// Left panel: the vehicle's base data (plan 15.2).
///
/// `active` is the section the jump bar marks as the one being read. It comes
/// from the previous frame, because the bar is drawn before the sections whose
/// position decides it — a frame of lag no one can see.
fn data_panel(root: &mut egui::Ui, editor: &mut Editor, active: &mut Option<&'static str>) -> f32 {
    let width = editor
        .settings
        .panels
        .map(|(left, _)| left)
        .unwrap_or(450.0);
    egui::Panel::left("data")
        .default_size(width)
        .resizable(true)
        .frame(editor_ui::panel_frame())
        .show(root, |ui| {
            // Heading, name and jump bar stay out of the scroll area. The form
            // is two to three panel-heights tall, so scrolling would otherwise
            // take the name of the vehicle being edited off screen, and the
            // only way to learn that a section exists would be to scroll past
            // it.
            ui.label(editor_ui::heading(t!("heading-vehicle")));
            ui.add_space(space::XS);
            ui.add(
                egui::TextEdit::singleline(&mut editor.spec.name)
                    .hint_text(t!("field-name"))
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(space::S);
            let mut jump = None;
            ui.horizontal_wrapped(|ui| {
                for (id, key) in SECTIONS {
                    // The section being read wears the "widget pressed" fill —
                    // enough to find at a glance, quiet enough not to read as a
                    // choice the user made. The accent stays for real
                    // selections (the LOD shown in the viewport).
                    // Fill *and* text colour: one step of fill alone is hard to
                    // pick out of seven chips on a dark bar.
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
                    let spec = &mut editor.spec;
                    nav_section(ui, jump, &mut current, "base", "group-base-data", |ui| {
                        editor_ui::form_grid("base").show(ui, |ui| {
                            row(ui, "veh-length", |ui| {
                                field(ui, &mut spec.length, 0.1, 1.0..=100.0, "m");
                            });
                            row(ui, "veh-gauge", |ui| {
                                field(ui, &mut spec.gauge, 0.001, 0.6..=2.0, "m");
                            });
                            row(ui, "veh-vmax", |ui| {
                                field(ui, &mut spec.v_max, 1.0, 0.0..=400.0, "km/h");
                            });
                            row(ui, "veh-mass", |ui| {
                                field(ui, &mut spec.mass_empty, 100.0, 1_000.0..=200_000.0, "kg");
                            });
                            row(ui, "veh-payload", |ui| {
                                field(ui, &mut spec.max_payload, 100.0, 0.0..=120_000.0, "kg");
                            });
                        });
                    });

                    nav_section(ui, jump, &mut current, "gear", "group-running-gear", |ui| {
                        editor_ui::form_grid("gear").show(ui, |ui| {
                            row(ui, "veh-rotating-mass", |ui| {
                                field(ui, &mut spec.rotating_mass_factor, 0.005, 0.0..=0.5, "");
                            });
                            row(ui, "veh-axles", |ui| {
                                field(ui, &mut spec.axles, 1.0, 0.0..=32.0, "");
                            });
                            // Nothing offered this before, and it defaults to
                            // zero: a locomotive built entirely in the editor
                            // could not transmit a newton.
                            row(ui, "veh-adhesive", |ui| {
                                field(ui, &mut spec.adhesive_mass_fraction, 0.05, 0.0..=1.0, "");
                            });
                            row(ui, "veh-axle-base", |ui| {
                                field(ui, &mut spec.axle_base_sum, 0.1, 0.0..=40.0, "m");
                            });
                            row(ui, "veh-tilt", |ui| {
                                field(ui, &mut spec.tilt_angle_deg, 0.5, 0.0..=12.0, "°");
                            });
                            row(ui, "veh-hunting", |ui| {
                                // Slider plus its value box span exactly one
                                // field width, keeping the column's right edge.
                                ui.spacing_mut().slider_width = 88.0;
                                ui.spacing_mut().interact_size.x = 54.0;
                                ui.add(egui::Slider::new(&mut spec.hunting, -1.0..=1.0));
                            });
                        });
                    });

                    nav_section(ui, jump, &mut current, "coupler", "group-coupler", |ui| {
                        editor_ui::form_grid("coupler").show(ui, |ui| {
                            row(ui, "cpl-type", |ui| {
                                coupler_combo(ui, &mut spec.coupler);
                            });
                            row(ui, "cpl-slack", |ui| {
                                field(ui, &mut spec.coupler.slack, 0.005, 0.0..=0.3, "m");
                            });
                            row(ui, "cpl-draw", |ui| {
                                field(
                                    ui,
                                    &mut spec.coupler.draw_stiffness,
                                    100_000.0,
                                    100_000.0..=100_000_000.0,
                                    "N/m",
                                );
                            });
                            row(ui, "cpl-buffer", |ui| {
                                field(
                                    ui,
                                    &mut spec.coupler.buffer_stiffness,
                                    100_000.0,
                                    100_000.0..=100_000_000.0,
                                    "N/m",
                                );
                            });
                            row(ui, "cpl-damping", |ui| {
                                field(
                                    ui,
                                    &mut spec.coupler.damping,
                                    10_000.0,
                                    0.0..=2_000_000.0,
                                    "N·s/m",
                                );
                            });
                            row(ui, "cpl-breaking", |ui| {
                                field(
                                    ui,
                                    &mut spec.coupler.breaking_force,
                                    50_000.0,
                                    100_000.0..=5_000_000.0,
                                    "N",
                                );
                            });
                        });
                    });

                    nav_section(
                        ui,
                        jump,
                        &mut current,
                        "resistance",
                        "group-resistance",
                        |ui| {
                            editor_ui::form_grid("resistance").show(ui, |ui| {
                                row(ui, "res-rolling", |ui| {
                                    field(ui, &mut spec.davis.a, 10.0, 0.0..=20_000.0, "N");
                                    // The tooltip names the figure the button would
                                    // write, so pressing it is not a guess.
                                    let suggestion =
                                        VehicleSpec::suggested_rolling_resistance(spec.mass_empty);
                                    if ui
                                        .button(t!("action-suggest"))
                                        .on_hover_text(t!(
                                            "res-rolling-suggest-hint",
                                            value = editor_ui::group_digits(suggestion)
                                        ))
                                        .clicked()
                                    {
                                        spec.davis.a = suggestion;
                                    }
                                });
                                row(ui, "res-speed-term", |ui| {
                                    field(ui, &mut spec.davis.b, 1.0, 0.0..=500.0, "N·s/m");
                                });

                                let mut use_cw_a = spec.cw_a.is_some();
                                editor_ui::form_label(ui, t!("res-air"));
                                ui.horizontal(|ui| {
                                    // Field first so it keeps the column edge; the
                                    // checkbox toggles between cw·A and Davis c.
                                    match &mut spec.cw_a {
                                        Some(cw_a) => {
                                            field(ui, cw_a, 0.1, 0.1..=40.0, "m²");
                                        }
                                        None => {
                                            // Its unit is awkward, which is no
                                            // reason to be the one field without
                                            // one — the b term next to it has hers.
                                            field(
                                                ui,
                                                &mut spec.davis.c,
                                                0.1,
                                                0.0..=100.0,
                                                "N·s²/m²",
                                            )
                                            .on_hover_text(t!("res-davis-c-hint"));
                                        }
                                    }
                                    if ui.checkbox(&mut use_cw_a, t!("res-cw-a")).changed() {
                                        spec.cw_a = use_cw_a.then_some(6.0);
                                    }
                                });
                                ui.end_row();

                                row(ui, "res-curve", |ui| {
                                    field(
                                        ui,
                                        &mut spec.curve_resistance_factor,
                                        0.05,
                                        0.0..=3.0,
                                        "",
                                    );
                                });
                            });
                            ui.add_space(space::XS);
                            ui.label(
                                egui::RichText::new(t!(
                                    "res-at-100",
                                    newtons = editor_ui::group_digits(spec.resistance(100.0 / 3.6))
                                ))
                                .small()
                                .color(colors::TEXT_SECONDARY),
                            );
                            // Three coefficients of a quadratic; nobody pictures
                            // their sum. A vehicle with no stated v max still has
                            // a curve worth seeing.
                            let top = if spec.v_max > 0.0 { spec.v_max } else { 160.0 };
                            editor_ui::subheading(ui, t!("res-plot"));
                            editor_ui::sparkline_fn(ui, top, "km/h", "N", |kmh| {
                                spec.resistance(kmh / 3.6)
                            });
                        },
                    );

                    nav_section(ui, jump, &mut current, "brake", "group-brake", |ui| {
                        let top = if spec.v_max > 0.0 { spec.v_max } else { 160.0 };
                        // The tare vehicle — the load belongs to the consist, not
                        // to the data sheet.
                        let axle_load = spec.axle_load_t(spec.mass_empty);
                        powertrain::brake_panel(
                            ui,
                            &mut spec.brake,
                            &mut spec.slip_protection,
                            top,
                            axle_load,
                        );
                        ui.add_space(space::XS);
                        // Braked weight and mass sit in different sections, and
                        // the figure a brake sheet is actually read in is
                        // neither of them. The editor should not leave that
                        // division to the user.
                        ui.label(
                            egui::RichText::new(t!(
                                "brk-percentage",
                                percent = format!("{:.0}", spec.brake_percentage())
                            ))
                            .small()
                            .color(colors::TEXT_SECONDARY),
                        );
                    });

                    nav_section(ui, jump, &mut current, "drive", "group-drive", |ui| {
                        powertrain::drive_panel(ui, &mut spec.traction);
                    });

                    nav_section(
                        ui,
                        jump,
                        &mut current,
                        "equipment",
                        "group-equipment",
                        |ui| {
                            equipment_panel(ui, spec);
                        },
                    );

                    nav_section(ui, jump, &mut current, "sounds", "group-sounds", |ui| {
                        sounds::panel(ui, spec);
                    });

                    nav_section(
                        ui,
                        jump,
                        &mut current,
                        "behaviour",
                        "group-behaviour",
                        |ui| {
                            // A labelled row like every other value in the panel.
                            // Free-floating, it was the one control the eye could
                            // not find at the column it has learnt.
                            editor_ui::form_grid("behaviour").show(ui, |ui| {
                                row(ui, "veh-script", |ui| {
                                    let mut script = spec.script.clone().unwrap_or_default();
                                    if ui
                                        .add(
                                            egui::TextEdit::singleline(&mut script)
                                                .hint_text(t!("field-script-hint"))
                                                .desired_width(space::FIELD),
                                        )
                                        .changed()
                                    {
                                        spec.script = (!script.is_empty()).then_some(script);
                                    }
                                });
                            });
                        },
                    );
                });
            // Above the first header nothing has reported in yet.
            *active = current.or(Some(SECTIONS[0].0));
        })
        .response
        .rect
        .width()
}

/// Equipment of the vehicle: train protection and door control (plan 9.1, 9.5a).
///
/// What the equipment achieves also depends on the line — the LZB needs a conductor cable,
/// the PZB needs track magnets.
fn equipment_panel(ui: &mut egui::Ui, spec: &mut VehicleSpec) {
    let mut fitted = matches!(spec.safety, SafetyEquipment::De { .. });
    if ui
        .checkbox(&mut fitted, t!("eq-german-protection"))
        .on_hover_text(t!("eq-german-protection-hint"))
        .changed()
    {
        spec.safety = if fitted {
            SafetyEquipment::De {
                pzb: Some(PzbVariant::Pzb90V20),
                lzb: false,
                sifa: Some(SifaKind::TimeTime),
                train_type: TrainType::O,
            }
        } else {
            SafetyEquipment::None
        };
    }
    // The train category (Zugart) is deliberately not vehicle data here: the
    // driver sets it in the cab from the brake sheet (train type switch).
    if let SafetyEquipment::De { pzb, lzb, sifa, .. } = &mut spec.safety {
        editor_ui::form_grid("safety").show(ui, |ui| {
            row(ui, "eq-pzb", |ui| {
                // Type designations of the equipment are names, not prose — they stay as they are.
                combo(
                    ui,
                    "pzb",
                    pzb,
                    &[
                        (None, t!("opt-not-fitted")),
                        (Some(PzbVariant::I54), "Indusi I 54".into()),
                        (Some(PzbVariant::I60), "Indusi I 60".into()),
                        (Some(PzbVariant::I60M), "Indusi I 60M".into()),
                        (Some(PzbVariant::I60R), "Indusi I 60R".into()),
                        (Some(PzbVariant::Pzb60), "ÖBB PZB 60".into()),
                        (Some(PzbVariant::Pzb90V15), "PZB 90 V1.5".into()),
                        (Some(PzbVariant::Pzb90V20), "PZB 90 V2.0".into()),
                    ],
                );
            });
            row(ui, "eq-sifa", |ui| {
                combo(
                    ui,
                    "sifa",
                    sifa,
                    &[
                        (None, t!("opt-not-fitted")),
                        (Some(SifaKind::TimeTime), t!("sifa-time-time")),
                        (Some(SifaKind::TimeDistance), t!("sifa-time-distance")),
                        (Some(SifaKind::Rzm), "RZM".into()),
                    ],
                );
            });
            editor_ui::form_label(ui, t!("eq-lzb"));
            ui.checkbox(lzb, t!("eq-lzb-on-board"))
                .on_hover_text(t!("eq-lzb-hint"));
            ui.end_row();
        });
        ui.add_space(space::XS);
    }

    // AFB is vehicle equipment like the door control, not a train protection system.
    ui.checkbox(&mut spec.afb, t!("eq-afb"))
        .on_hover_text(t!("eq-afb-hint"));
    ui.checkbox(&mut spec.passenger_doors, t!("eq-passenger-doors"))
        .on_hover_text(t!("eq-passenger-doors-hint"));
    editor_ui::form_grid("doors").show(ui, |ui| {
        row(ui, "eq-doors", |ui| {
            combo(
                ui,
                "doors",
                &mut spec.doors,
                &[
                    (DoorSystem::None, t!("opt-not-fitted")),
                    (DoorSystem::Tb0, "TB0".into()),
                    (DoorSystem::Tav, "TAV".into()),
                    (DoorSystem::UicWtb, "UIC-WTB".into()),
                ],
            );
        });
    });
}

/// The two standard couplers, or "own values" when the numbers match neither.
///
/// Picking one fills all five fields; they stay editable underneath, so the
/// presets are a starting point and not a cage.
fn coupler_combo(ui: &mut egui::Ui, coupler: &mut CouplerSpec) {
    let screw = CouplerSpec::screw();
    let centre = CouplerSpec::center_buffer();
    let key = if *coupler == screw {
        "cpl-screw"
    } else if *coupler == centre {
        "cpl-centre"
    } else {
        "cpl-custom"
    };
    egui::ComboBox::from_id_salt("coupler")
        .selected_text(t!(key))
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(key == "cpl-screw", t!("cpl-screw"))
                .clicked()
            {
                *coupler = screw;
            }
            if ui
                .selectable_label(key == "cpl-centre", t!("cpl-centre"))
                .clicked()
            {
                *coupler = centre;
            }
        });
}

/// Combo box over a fixed set of values.
fn combo<T: Copy + PartialEq>(ui: &mut egui::Ui, id: &str, value: &mut T, options: &[(T, String)]) {
    let selected = options
        .iter()
        .find(|(v, _)| v == value)
        .map(|(_, label)| label.as_str())
        .unwrap_or("—");
    egui::ComboBox::from_id_salt(id)
        .selected_text(selected)
        .show_ui(ui, |ui| {
            for (v, label) in options {
                if ui.selectable_label(value == v, label).clicked() {
                    *value = *v;
                }
            }
        });
}

/// Right panel: model file, levels of detail, moving parts.
fn model_panel(root: &mut egui::Ui, editor: &mut Editor, assets: &mut AssetServer) -> f32 {
    let width = editor
        .settings
        .panels
        .map(|(_, right)| right)
        .unwrap_or(400.0);
    egui::Panel::right("model")
        .default_size(width)
        .resizable(true)
        .frame(editor_ui::panel_frame())
        .show(root, |ui| {
            // Heading and file stay put like the left panel's do: a model with
            // a few hundred nodes scrolls for pages, and which file is being
            // edited must not be one of the things that scrolls away.
            ui.label(editor_ui::heading(t!("heading-model")));
            ui.add_space(space::XS);
            let file = editor
                .spec
                .model
                .as_ref()
                .map(|m| m.file.clone())
                .unwrap_or_default();
            ui.horizontal(|ui| {
                if ui.button(t!("action-import-gltf")).clicked() {
                    import_model(editor, assets);
                }
                let label = if file.is_empty() {
                    egui::RichText::new(t!("common-none")).color(colors::TEXT_SECONDARY)
                } else {
                    egui::RichText::new(file)
                        .monospace()
                        .color(colors::TEXT_SECONDARY)
                };
                ui.add(egui::Label::new(label).truncate());
            });
            ui.add_space(space::S);

            if editor.nodes.is_empty() {
                ui.label(t!("model-none-loaded"));
                ui.add_space(space::XS);
                ui.label(
                    egui::RichText::new(t!("model-conventions"))
                        .small()
                        .color(colors::TEXT_SECONDARY),
                );
                return;
            }

            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    editor_ui::section(ui, "lods", t!("group-lods"), |ui| {
                        lod_list(ui, editor);
                    });
                    editor_ui::section(ui, "parts", t!("group-parts"), |ui| {
                        parts_list(ui, editor);
                    });
                    editor_ui::section(ui, "cab", t!("group-cab"), |ui| {
                        crate::cab::panel(ui, editor);
                    });
                    editor_ui::section(ui, "displays", t!("group-displays"), |ui| {
                        crate::displays::panel(ui, editor);
                    });
                    editor_ui::section(ui, "nodes", t!("group-nodes"), |ui| {
                        node_list(ui, editor);
                    });
                });
        })
        .response
        .rect
        .width()
}

fn lod_list(ui: &mut egui::Ui, editor: &mut Editor) {
    // Offering a button that would change nothing costs the user a click to
    // find that out, and the file a needless "unsaved" mark.
    let detected = model::detect_lods(&editor.nodes);
    let current = editor.spec.model.as_ref().map(|m| m.lods.as_slice());
    let would_change = current != Some(detected.as_slice());
    if ui
        .add_enabled(
            would_change,
            egui::Button::new(t!("action-read-node-names")),
        )
        .on_hover_text(t!("action-read-node-names-hint", count = detected.len()))
        .on_disabled_hover_text(t!("action-read-node-names-same"))
        .clicked()
    {
        editor.model_mut().lods = detected;
    }
    ui.add_space(space::XS);
    let mut remove_lod = None;
    let mut preview = editor.preview_lod;
    if let Some(lods) = editor.spec.model.as_mut().map(|m| &mut m.lods) {
        // A grid rather than one horizontal per row: the level chips are not
        // all the same width — the selected one is a filled button, and "1" is
        // a narrower glyph than "0" — which would push each row's field to its
        // own x. The grid gives the column the widest chip and lines them up.
        editor_ui::form_grid("lods").num_columns(3).show(ui, |ui| {
            for (i, lod) in lods.iter_mut().enumerate() {
                // Radio button: which level the viewport shows.
                if ui
                    .selectable_label(preview == lod.level, format!("LOD{}", lod.level))
                    .on_hover_text(t!("lod-show-hint"))
                    .clicked()
                {
                    preview = lod.level;
                }
                field(ui, &mut lod.distance, 10.0, 10.0..=20_000.0, "m")
                    .on_hover_text(t!("lod-distance-hint"));
                if ui.small_button("×").clicked() {
                    remove_lod = Some(i);
                }
                ui.end_row();
            }
        });
    }
    editor.preview_lod = preview;
    if let Some(i) = remove_lod {
        editor.remove_lod(i);
    }
}

fn parts_list(ui: &mut egui::Ui, editor: &mut Editor) {
    let bound: std::collections::HashSet<&str> = editor
        .spec
        .model
        .as_ref()
        .map(|m| m.parts.iter().map(|p| p.node.as_str()).collect())
        .unwrap_or_default();
    let fresh: Vec<Part> = editor
        .nodes
        .iter()
        .filter_map(|n| n.suggestion.clone())
        .filter(|p| !bound.contains(p.node.as_str()))
        .collect();
    if ui
        .add_enabled(
            !fresh.is_empty(),
            egui::Button::new(t!("action-take-suggestions")),
        )
        .on_hover_text(t!("action-take-suggestions-hint", count = fresh.len()))
        .on_disabled_hover_text(t!("action-take-suggestions-none"))
        .clicked()
    {
        let model = editor.model_mut();
        model.parts.extend(fresh);
    }
    ui.add_space(space::XS);

    let mut remove_part = None;
    let mut changed = false;
    // Bindings survive a model swap; the nodes they name may not. Such a part
    // does nothing in the simulator and says nothing here — it looked exactly
    // like a working one until the vehicle was driven.
    let present: std::collections::HashSet<&str> =
        editor.nodes.iter().map(|n| n.name.as_str()).collect();
    if let Some(model) = editor.spec.model.as_mut() {
        for (i, part) in model.parts.iter_mut().enumerate() {
            let missing = !present.contains(part.node.as_str());
            editor_ui::card_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    let name = egui::RichText::new(&part.node).monospace();
                    let name = if missing {
                        name.color(colors::ERROR)
                    } else {
                        name
                    };
                    let label = ui.label(name);
                    if missing {
                        label.on_hover_text(t!("part-node-missing-hint"));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("×").clicked() {
                            remove_part = Some(i);
                        }
                    });
                });
                ui.horizontal(|ui| {
                    // The function field takes what the motion combo leaves.
                    changed |= ui
                        .add(
                            egui::TextEdit::singleline(&mut part.function)
                                .desired_width(ui.available_width() - MOTION_COMBO_W - space::S)
                                .hint_text(t!("part-function-placeholder")),
                        )
                        // The naming conventions are on screen only while no
                        // model is loaded — that is, everywhere except where
                        // this field asks the user to follow them.
                        .on_hover_text(t!("part-function-hint"))
                        .changed();
                    changed |= motion_combo(ui, i, &mut part.motion);
                });
                changed |= motion_params(ui, &mut part.motion);
            });
        }
    }
    if let Some(i) = remove_part {
        editor.model_mut().parts.remove(i);
    }
    editor.dirty |= changed;
}

/// The nodes of the file, narrowed by a substring filter.
///
/// Unlike the form's sections, this list is unbounded and its entries are named
/// by whoever built the model — a real locomotive brings a few hundred, sorted
/// alphabetically, most of them scenery that will never be bound. A filter is
/// the right tool here precisely because the user already knows the name they
/// gave the object in Blender.
fn node_list(ui: &mut egui::Ui, editor: &mut Editor) {
    ui.add(
        egui::TextEdit::singleline(&mut editor.node_filter)
            .hint_text(t!("node-filter-hint"))
            .desired_width(f32::INFINITY),
    );
    let needle = editor.node_filter.to_lowercase();
    let nodes: Vec<model::Node> = editor
        .nodes
        .iter()
        .filter(|n| needle.is_empty() || n.name.to_lowercase().contains(&needle))
        .cloned()
        .collect();
    // Always say how many, so a filter that hides most of the file cannot be
    // mistaken for a short file.
    let total = editor.nodes.len();
    let count = if needle.is_empty() {
        t!("node-count", total = total)
    } else {
        t!("node-count-filtered", shown = nodes.len(), total = total)
    };
    ui.label(
        egui::RichText::new(count)
            .small()
            .color(colors::TEXT_SECONDARY),
    );
    ui.add_space(space::XS);
    for node in nodes {
        ui.horizontal(|ui| {
            let bound = editor
                .spec
                .model
                .as_ref()
                .is_some_and(|m| m.parts.iter().any(|p| p.node == node.name));
            if ui
                .add_enabled(!bound, egui::Button::new("+").small())
                .on_hover_text(t!("node-bind-hint"))
                .clicked()
            {
                let part = node.suggestion.clone().unwrap_or(Part {
                    node: node.name.clone(),
                    function: String::new(),
                    motion: Motion::Visibility,
                });
                editor.model_mut().parts.push(part);
            }
            ui.label(egui::RichText::new(&node.name).monospace());
            let mut extra = String::new();
            if let Some(level) = node.lod {
                extra.push_str(&format!(" · LOD{level}"));
            }
            if let Some(hint) = &node.suggestion {
                extra.push_str(&format!(" · {}", hint.function));
            }
            if !extra.is_empty() {
                ui.label(
                    egui::RichText::new(extra)
                        .small()
                        .color(colors::TEXT_SECONDARY),
                );
            }
        });
    }
}

/// Width of the motion combo inside a part or cab control card.
pub(crate) const MOTION_COMBO_W: f32 = 110.0;

/// Kind of motion of a part.
pub(crate) fn motion_combo(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    motion: &mut Motion,
) -> bool {
    let mut changed = false;
    let key = match motion {
        Motion::Visibility => "motion-visible",
        Motion::Rotate { .. } => "motion-rotate",
        Motion::Translate { .. } => "motion-move",
    };
    egui::ComboBox::from_id_salt(("motion", id))
        .selected_text(t!(key))
        .width(MOTION_COMBO_W)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(key == "motion-visible", t!("motion-visible"))
                .clicked()
            {
                *motion = Motion::Visibility;
                changed = true;
            }
            if ui
                .selectable_label(key == "motion-rotate", t!("motion-rotate"))
                .clicked()
            {
                *motion = Motion::Rotate {
                    axis: [1.0, 0.0, 0.0],
                    degrees: 90.0,
                };
                changed = true;
            }
            if ui
                .selectable_label(key == "motion-move", t!("motion-move"))
                .clicked()
            {
                *motion = Motion::Translate {
                    axis: [0.0, 0.0, 1.0],
                    metres: 1.0,
                };
                changed = true;
            }
        });
    changed
}

/// Axis and amount of a rotating or translating part: four equal-width drag
/// fields, so the row reads as one line of coordinates.
pub(crate) fn motion_params(ui: &mut egui::Ui, motion: &mut Motion) -> bool {
    let mut changed = false;
    match motion {
        Motion::Visibility => {}
        Motion::Rotate { axis, degrees } => {
            ui.horizontal(|ui| {
                ui.spacing_mut().interact_size.x = 64.0;
                changed |= axis_editor(ui, axis);
                changed |= ui
                    .add(egui::DragValue::new(degrees).speed(1.0).suffix("\u{A0}°"))
                    .on_hover_text(t!("part-amount-hint"))
                    .changed();
            });
        }
        Motion::Translate { axis, metres } => {
            ui.horizontal(|ui| {
                ui.spacing_mut().interact_size.x = 64.0;
                changed |= axis_editor(ui, axis);
                changed |= ui
                    .add(egui::DragValue::new(metres).speed(0.01).suffix("\u{A0}m"))
                    .on_hover_text(t!("part-amount-hint"))
                    .changed();
            });
        }
    }
    changed
}

fn axis_editor(ui: &mut egui::Ui, axis: &mut [f32; 3]) -> bool {
    let mut changed = false;
    // Axis letters are names, not prose.
    for (label, value) in ["X", "Y", "Z"].iter().zip(axis.iter_mut()) {
        ui.label(
            egui::RichText::new(*label)
                .small()
                .color(colors::TEXT_SECONDARY),
        );
        changed |= ui
            .add(egui::DragValue::new(value).speed(0.1).range(-1.0..=1.0))
            .changed();
    }
    changed
}

fn status_bar(root: &mut egui::Ui, editor: &Editor) {
    egui::Panel::bottom("status")
        .frame(editor_ui::bar_frame())
        .show(root, |ui| {
            ui.horizontal(|ui| {
                let message = egui::RichText::new(editor.status.text());
                let message = if editor.status.is_error() {
                    message.color(colors::ERROR)
                } else {
                    message
                };
                // A long path in an error would otherwise wrap and grow the
                // bar; the whole text stays available on hover.
                ui.add(egui::Label::new(message).truncate())
                    .on_hover_text(editor.status.text());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(
                            editor
                                .path
                                .as_ref()
                                .map(|p| p.display().to_string())
                                .unwrap_or_else(|| t!("status-new-file")),
                        )
                        .color(colors::TEXT_SECONDARY),
                    );
                    if editor.dirty {
                        ui.label(egui::RichText::new(t!("status-unsaved")).color(colors::WARN));
                    }
                });
            });
        });
}

/// A labelled row of a form grid: `key` names the label, `key`-hint the tooltip.
///
/// The tooltip is optional. A field whose label and unit already say everything
/// gets none — a tooltip that only repeats the unit teaches the user that
/// hovering here is not worth it, and the ones that do carry something stop
/// being found.
pub fn row(ui: &mut egui::Ui, key: &str, widget: impl FnOnce(&mut egui::Ui)) {
    let label = editor_ui::form_label(ui, t!(key));
    if let Some(hint) = i18n::maybe(&format!("{key}-hint")) {
        label.on_hover_text(hint);
    }
    ui.horizontal(|ui| widget(ui));
    ui.end_row();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Guards the early return of [`confirm_discard`]. Without it every save,
    /// every new file and every quit would stop at a modal — and this test
    /// would be the first to notice, by opening one.
    /// One frame of the editor: whatever the user did, then the bookkeeping.
    fn frame(editor: &mut Editor, edit: impl FnOnce(&mut Editor)) {
        let before = editor.spec.clone();
        edit(editor);
        track_changes(editor, before);
    }

    #[test]
    fn a_drag_costs_one_undo_step_not_one_per_frame() {
        let mut editor = Editor::default();
        let start = editor.spec.clone();

        // Three frames of a single drag on the mass field.
        for mass in [41_000.0, 42_000.0, 43_000.0] {
            frame(&mut editor, |e| e.spec.mass_empty = mass);
        }
        assert_eq!(editor.undo.len(), 1);
        assert!(editor.dirty);

        // A frame in which nothing moves ends the interaction…
        frame(&mut editor, |_| {});
        // …so the next drag is a step of its own.
        frame(&mut editor, |e| e.spec.mass_empty = 50_000.0);
        assert_eq!(editor.undo.len(), 2);

        editor.undo();
        assert_eq!(editor.spec.mass_empty, 43_000.0);
        editor.undo();
        assert_eq!(editor.spec, start);
        editor.redo();
        assert_eq!(editor.spec.mass_empty, 43_000.0);

        // Editing after an undo abandons the branch that was undone.
        frame(&mut editor, |e| e.spec.axles = 6);
        assert!(editor.redo.is_empty());
    }

    /// The model panel writes to the same spec, so its edits are undoable too —
    /// that is the whole reason the snapshot wraps both panels.
    #[test]
    fn model_edits_are_undoable() {
        let mut editor = Editor::default();
        frame(&mut editor, |e| {
            e.model_mut().file = "example/assets/br101.gltf".into();
        });
        assert_eq!(editor.undo.len(), 1);
        editor.undo();
        assert!(editor.spec.model.is_none());
    }

    /// A wrong key or a mistyped placeholder would leave the vehicle out of
    /// the title without failing anywhere else.
    #[test]
    fn the_window_title_names_the_vehicle() {
        let editor = Editor::default();
        for key in [
            "window-vehicle-editor-named",
            "window-vehicle-editor-unsaved",
        ] {
            let title = t!(key, name = editor.spec.name);
            assert!(title.contains(&editor.spec.name), "{key}: {title}");
        }
    }

    /// Re-writing an unchanged file strips whatever `ron::ser` does not model
    /// — the comments of a hand-written vehicle. Save must not be reachable
    /// then.
    #[test]
    fn save_does_nothing_for_an_unchanged_file() {
        let mut editor = Editor::default();
        // No file yet: saving it is how it gets one.
        assert!(needs_saving(&editor));

        editor.path = Some(std::path::PathBuf::from("br101.ron"));
        editor.dirty = false;
        assert!(!needs_saving(&editor));

        editor.dirty = true;
        assert!(needs_saving(&editor));
    }

    /// Guards the early return of [`report_failure`]: a successful save must
    /// not put a modal in the way. This test would be the one to find out.
    #[test]
    fn success_opens_no_dialog() {
        let editor = Editor::default();
        assert!(!editor.status.is_error());
        report_failure(&editor);
    }

    /// The point of these hints is the figure. A mistyped placeholder would
    /// silently drop it and leave a tooltip that says nothing new.
    #[test]
    fn suggestion_hints_name_the_value() {
        for key in ["res-rolling-suggest-hint", "brk-force-suggest-hint"] {
            let hint = t!(key, value = "12345");
            assert!(hint.contains("12345"), "{key}: {hint}");
        }
    }

    /// `-hint` is the tooltip, and a placeholder needs its own key. Swapping
    /// the two puts three lines of conventions inside the text field.
    #[test]
    fn the_function_placeholder_is_not_the_tooltip() {
        let placeholder = t!("part-function-placeholder");
        let tooltip = t!("part-function-hint");
        assert!(placeholder.len() < 20, "{placeholder}");
        assert!(tooltip.contains("pantograph"), "{tooltip}");
    }

    /// A file with no comments must save without a word, and a session that
    /// has already answered the warning must not see it again — both are the
    /// early returns, and breaking either puts a modal in a test.
    #[test]
    fn the_comment_warning_stays_out_of_the_way() {
        let mut editor = Editor::default();
        let plain = std::env::temp_dir().join("trainsim-plain.ron");
        std::fs::write(&plain, "(name: \"x\")").expect("scratch file");
        assert!(confirm_comment_loss(&mut editor, &plain));
        assert!(!editor.warned_about_comments, "nothing to warn about");

        editor.warned_about_comments = true;
        let commented = std::env::temp_dir().join("trainsim-commented.ron");
        std::fs::write(
            &commented,
            "// a note
(name: \"x\")",
        )
        .expect("scratch file");
        assert!(confirm_comment_loss(&mut editor, &commented));

        let _ = std::fs::remove_file(plain);
        let _ = std::fs::remove_file(commented);
    }

    /// Deleting the level being previewed must move the preview, not leave it
    /// pointing at a level the model no longer has.
    #[test]
    fn removing_the_previewed_level_moves_the_preview() {
        use sim_core::train::Lod;
        let mut editor = Editor::default();
        editor.model_mut().lods = vec![
            Lod {
                level: 0,
                distance: 150.0,
            },
            Lod {
                level: 1,
                distance: 2_000.0,
            },
        ];
        editor.preview_lod = 1;
        editor.remove_lod(1);
        assert_eq!(editor.preview_lod, 0, "falls back to a level that exists");

        // Removing the last one leaves nothing to fall back to.
        editor.remove_lod(0);
        assert_eq!(editor.preview_lod, 0);

        // A level that was not being previewed leaves the preview alone.
        editor.model_mut().lods = vec![
            Lod {
                level: 0,
                distance: 150.0,
            },
            Lod {
                level: 2,
                distance: 900.0,
            },
        ];
        editor.preview_lod = 2;
        editor.remove_lod(0);
        assert_eq!(editor.preview_lod, 2);
    }

    #[test]
    fn saved_work_is_discarded_without_asking() {
        let mut editor = Editor::default();
        assert!(!editor.dirty);
        assert!(confirm_discard(&mut editor));
    }
}
