//! Desktop UI of the route editor: menu bar, tool panel, status bar.
//!
//! The editor is an application, not a game screen — everything reachable through the
//! keyboard is in the menu as well, and the file dialogs are the operating system's own.

use crate::overlay::Overlay;
use crate::tools::{self, EditorState, Selection, Tool};
use crate::{Focus, History, Line, Request, focus_degrees};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use content::LineSource;
use editor_ui::{colors, space};
use i18n::t;
use imagery::ZoomMode;
use sim_core::interlock::BlockMarkerPayload;
use sim_core::safety::de::{LzbSection, MagnetFrequency, MagnetPayload};
use std::path::{Path, PathBuf};
use track_model::{DeviceKind, Facing};

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
    handle_shortcuts(&ctx, &mut line, &mut history, &mut state, &mut overlay);

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
    status_bar(&mut root, &line, &state, &overlay, &focus);
    left_panel(
        &mut root,
        &mut line,
        &mut state,
        &overlay,
        &mut request,
        &mut focus,
        &mut active,
    );

    // The rect the panels leave free, and whether a text field owns the
    // keyboard — the input systems read both from here: the hand-built panel
    // layout is invisible to egui's own pointer hit test.
    let free = root.available_rect_before_wrap();
    state.viewport = Rect::new(free.min.x, free.min.y, free.max.x, free.max.y);
    state.typing = ctx.memory(|m| m.focused().is_some());
    viewport_hint(&ctx, &root, &state);
    Ok(())
}

/// Says how to move the map, in the viewport, until the user has done it.
///
/// The map is the largest thing on screen and the only region with no visible
/// control at all; middle-drag and wheel zoom are map conventions, not
/// something a modder arriving from a text editor knows. Once the map has
/// moved or a tool has been used, the hint has done its job and goes.
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
        // On the panel background, not bare: the hint sits on the aerial
        // imagery, and secondary grey vanishes over a sunlit field.
        ui.label(
            egui::RichText::new(t!("help-map"))
                .small()
                .color(colors::TEXT_SECONDARY)
                .background_color(colors::BG_PANEL),
        );
    });
}

fn handle_shortcuts(
    ctx: &egui::Context,
    line: &mut Line,
    history: &mut History,
    state: &mut EditorState,
    overlay: &mut Overlay,
) {
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
        open(line, history, state, overlay);
    }
    if ctx.input_mut(|i| i.consume_shortcut(&SHORTCUT_NEW)) && confirm_discard(line, state, overlay)
    {
        new_line(line, history, state);
    }
}

/// Stepping through the history ends whatever interaction was running and
/// drops a selection that may now point at something else.
fn undo(line: &mut Line, history: &mut History, state: &mut EditorState) {
    history.undo(line);
    state.selection = Selection::None;
    state.drawing = None;
}

fn redo(line: &mut Line, history: &mut History, state: &mut EditorState) {
    history.redo(line);
    state.selection = Selection::None;
    state.drawing = None;
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

fn open(line: &mut Line, history: &mut History, state: &mut EditorState, overlay: &mut Overlay) {
    let Some(path) = file_dialog(state)
        .add_filter(t!("filter-line-ron"), &["ron"])
        .pick_file()
    else {
        return;
    };
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
                line.recenter = true;
                history.reset(&line.source);
                state.selection = Selection::None;
                state.drawing = None;
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

fn new_line(line: &mut Line, history: &mut History, state: &mut EditorState) {
    line.source = LineSource {
        name: "Line".into(),
        geoid_offset: 46.0,
        nodes: vec![],
        edges: vec![],
        devices: vec![],
        sections: vec![],
        signals: vec![],
        routes: vec![],
        boundaries: vec![],
        script: None,
    };
    line.path = None;
    line.dirty = false;
    line.needs_rebuild = true;
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
                            new_line(line, history, state);
                        }
                    }
                    if ui.button(t!("action-open-line")).clicked() {
                        ui.close();
                        if confirm_discard(line, state, overlay) {
                            open(line, history, state, overlay);
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
                    let delete_button = egui::Button::new(t!("action-delete"));
                    if ui
                        .add_enabled(state.selection != Selection::None, delete_button)
                        .clicked()
                    {
                        tools::delete_selection(line, state);
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
                ui.menu_button(t!("menu-view"), language_menu);
                ui.menu_button(t!("menu-help"), |ui| {
                    ui.label(t!("help-pan"));
                    ui.label(t!("help-opacity"));
                    ui.label(t!("help-offset"));
                    ui.label(t!("help-draw"));
                });
            });
        });
}

fn status_bar(
    root: &mut egui::Ui,
    line: &Line,
    state: &EditorState,
    overlay: &Overlay,
    focus: &Focus,
) {
    egui::Panel::bottom("status")
        .frame(editor_ui::bar_frame())
        .show(root, |ui| {
            ui.horizontal(|ui| {
                // While a track is being drawn, the drawing is the status.
                let status = match &state.drawing {
                    Some(drawing) => t!("draw-active", segments = drawing.segments.len()),
                    None if overlay.status.is_empty() => t!("status-ready"),
                    None => overlay.status.clone(),
                };
                ui.add(egui::Label::new(status).truncate());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
                });
            });
        });
}

/// The sections of the panel, in the order they are drawn: id and the i18n
/// key of the title. The jump bar sits above the scroll area and has to name
/// them before the first one has been laid out. Editing first, template
/// configuration after, diagnostics last.
const SECTIONS: [(&str, &str); 4] = [
    ("tools", "heading-tools"),
    ("selection", "heading-selection"),
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
    overlay: &Overlay,
    request: &mut Request,
    focus: &mut Focus,
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
            let mut jump = None;
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
                        if let Some(drawing) = &state.drawing {
                            ui.add_space(space::XS);
                            ui.small(t!("draw-active", segments = drawing.segments.len()));
                        }
                    });

                    nav_section(
                        ui,
                        jump,
                        &mut current,
                        "selection",
                        "heading-selection",
                        |ui| {
                            selection_panel(ui, line, state, focus);
                        },
                    );

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

fn tool_chips(ui: &mut egui::Ui, state: &mut EditorState) {
    ui.horizontal_wrapped(|ui| {
        for (tool, key) in [
            (Tool::Select, "tool-select"),
            (Tool::DrawTrack, "tool-draw"),
            (Tool::PlaceDevice, "tool-device"),
        ] {
            let mut chip = ui.selectable_label(state.tool == tool, t!(key));
            if let Some(hint) = i18n::maybe(&format!("{key}-hint")) {
                chip = chip.on_hover_text(hint);
            }
            if chip.clicked() && state.tool != tool {
                state.tool = tool;
                state.drawing = None;
            }
        }
    });
}

fn selection_panel(ui: &mut egui::Ui, line: &mut Line, state: &mut EditorState, focus: &mut Focus) {
    match state.selection {
        Selection::None => {
            ui.small(t!("sel-none"));
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
    }
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
        ui.small(format!("© {}", provider.attribution));
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
fn row(ui: &mut egui::Ui, key: &str, widget: impl FnOnce(&mut egui::Ui)) {
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
