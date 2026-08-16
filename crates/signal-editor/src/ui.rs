//! Desktop UI of the signal editor: menu bar, parts panel, status bar.
//!
//! Look and feel come from the `editor-ui` crate. The panel edits the three
//! things a signal model is made of: the part list (glTF files chained by
//! mount points), the lamp bindings, and a lamp test for the preview.

use crate::{Editor, PartState, Status};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use editor_ui::{colors, space};
use i18n::t;
use sim_core::interlock::{LampBinding, MotionBinding, SignalModel, SignalPart};
use sim_core::train::{Lod, Motion, lod_level};

const SHORTCUT_NEW: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::N);
const SHORTCUT_OPEN: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::O);
const SHORTCUT_SAVE: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::S);

/// One frame of UI — the same background-`Ui` layout as the other editors.
pub fn draw(
    mut contexts: EguiContexts,
    mut editor: ResMut<Editor>,
    mut themed: Local<bool>,
    mut view: ResMut<crate::View>,
    windows: Query<&bevy::window::RawHandleWrapper, With<bevy::window::PrimaryWindow>>,
) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
    editor.window = windows.single().ok().cloned();
    if !*themed {
        // Fonts installed by `apply` become active with the next pass.
        editor_ui::apply(&ctx);
        *themed = true;
        return Ok(());
    }
    handle_shortcuts(&ctx, &mut editor);

    let mut root = egui::Ui::new(
        ctx.clone(),
        "viewport".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    menu_bar(&mut root, &mut editor);
    let before = editor.model.clone();
    status_bar(&mut root, &editor);
    panel(&mut root, &mut editor);
    // Any edit marks the file unsaved. A change to the parts or motions also
    // rebuilds the preview — a motion edit must put displaced nodes back on
    // their file transforms. Lamp bindings and LOD distances apply live.
    if editor.model != before {
        editor.dirty = true;
        if editor.model.parts != before.parts || editor.model.motions != before.motions {
            editor.revision += 1;
        }
    }
    let free = root.available_rect_before_wrap();
    view.viewport = Rect::new(free.min.x, free.min.y, free.max.x, free.max.y);
    viewport_hint(&ctx, &root, &view);
    Ok(())
}

/// Says how to move the camera until the user has done it.
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

fn handle_shortcuts(ctx: &egui::Context, editor: &mut Editor) {
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

/// The owner of every native dialog — without one, Windows may open them
/// behind the editor window.
fn dialog_parent(editor: &Editor) -> Option<bevy::window::ThreadLockedRawWindowHandleWrapper> {
    // SAFETY: the handle is only handed to rfd as the dialog owner; nothing
    // draws or resizes through it.
    editor.window.as_ref().map(|w| unsafe { w.get_handle() })
}

fn message_dialog(editor: &Editor) -> rfd::MessageDialog {
    match dialog_parent(editor) {
        Some(parent) => rfd::MessageDialog::new().set_parent(&parent),
        None => rfd::MessageDialog::new(),
    }
}

fn file_dialog(editor: &Editor) -> rfd::FileDialog {
    match dialog_parent(editor) {
        Some(parent) => rfd::FileDialog::new().set_parent(&parent),
        None => rfd::FileDialog::new(),
    }
}

/// Asks before unsaved work is thrown away; `true` means go ahead.
pub fn confirm_discard(editor: &mut Editor) -> bool {
    if !editor.dirty {
        return true;
    }
    match message_dialog(editor)
        .set_level(rfd::MessageLevel::Warning)
        .set_title(t!("confirm-unsaved-title"))
        .set_description(t!("confirm-unsaved", name = editor.display_name()))
        .set_buttons(rfd::MessageButtons::YesNoCancel)
        .show()
    {
        rfd::MessageDialogResult::Yes => {
            save(editor);
            !editor.dirty
        }
        rfd::MessageDialogResult::No => true,
        _ => false,
    }
}

/// Whether Save would do anything — writing an unchanged file strips the
/// comments a hand-written one carries.
fn needs_saving(editor: &Editor) -> bool {
    editor.dirty || editor.path.is_none()
}

/// Puts a failure in front of the user; the status bar alone is too quiet.
fn report_failure(editor: &Editor) {
    if editor.status.is_error() {
        message_dialog(editor)
            .set_level(rfd::MessageLevel::Error)
            .set_title(t!("dialog-error-title"))
            .set_description(editor.status.text())
            .show();
    }
}

fn save(editor: &mut Editor) {
    match editor.path.clone() {
        Some(path) => {
            editor.save(path);
            report_failure(editor);
        }
        None => save_as(editor),
    }
}

fn save_as(editor: &mut Editor) {
    if let Some(path) = file_dialog(editor)
        .add_filter(t!("filter-signal-model-ron"), &["ron"])
        .set_file_name("signal_model.ron")
        .save_file()
    {
        editor.save(path);
        report_failure(editor);
    }
}

fn open(editor: &mut Editor) {
    if let Some(path) = file_dialog(editor)
        .add_filter(t!("filter-signal-model-ron"), &["ron"])
        .pick_file()
    {
        editor.open(path);
        report_failure(editor);
    }
}

/// Picks a part file. The path is stored relative to the `mods/` directory,
/// because that is how the simulator finds it later.
fn pick_part_file(editor: &mut Editor) -> Option<String> {
    let path = file_dialog(editor)
        .add_filter(t!("filter-model-gltf"), &["gltf", "glb"])
        .set_directory(crate::mods_dir())
        .pick_file()?;
    match path.strip_prefix(crate::mods_dir()) {
        Ok(relative) => Some(relative.to_string_lossy().replace('\\', "/")),
        Err(_) => {
            editor.status = Status::Error(t!("status-outside-mods", path = path.display()));
            report_failure(editor);
            None
        }
    }
}

fn menu_bar(root: &mut egui::Ui, editor: &mut Editor) {
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
                    if ui.button(t!("action-save-as")).clicked() {
                        save_as(editor);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(t!("action-quit")).clicked() && confirm_discard(editor) {
                        editor.settings.save();
                        std::process::exit(0);
                    }
                });
                ui.menu_button(t!("menu-view"), |ui| {
                    ui.menu_button(t!("menu-language"), |ui| {
                        let current = i18n::language();
                        for (code, name) in i18n::LANGUAGES {
                            if ui.selectable_label(current == *code, *name).clicked() {
                                i18n::set_language(code);
                                editor.settings.set_language(code);
                                ui.close();
                            }
                        }
                    });
                });
                ui.menu_button(t!("menu-help"), |ui| {
                    ui.label(t!("help-mouse"));
                    ui.label(t!("help-signal-conventions"));
                    ui.separator();
                    if ui.button(t!("action-about")).clicked() {
                        message_dialog(editor)
                            .set_title(t!("window-signal-editor"))
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

fn status_bar(root: &mut egui::Ui, editor: &Editor) {
    egui::Panel::bottom("status")
        .frame(editor_ui::bar_frame())
        .show(root, |ui| {
            ui.horizontal(|ui| {
                let colour = if editor.status.is_error() {
                    colors::ERROR
                } else {
                    colors::TEXT_SECONDARY
                };
                ui.label(egui::RichText::new(editor.status.text()).color(colour));
            });
        });
}

/// Short display name of a part for the mount combos: `#1 sig_schirm_ks.gltf`.
fn part_label(index: usize, part: &SignalPart) -> String {
    let name = part.file.rsplit('/').next().unwrap_or(&part.file);
    format!("#{index} {name}")
}

/// Node names with the conventional prefix first — mount combos want `mp_*`,
/// lamp combos want `lamp_*` on top, the full list stays reachable below.
fn ordered_nodes(nodes: &[String], prefix: &str) -> Vec<String> {
    let mut out: Vec<String> = nodes
        .iter()
        .filter(|n| n.starts_with(prefix))
        .cloned()
        .collect();
    out.extend(nodes.iter().filter(|n| !n.starts_with(prefix)).cloned());
    out
}

/// Suggests lamp bindings from the node names: `lamp_<x>` binds the lamp image
/// `<x>`, a `zs*` node binds its own name (`zs1`, `zs3_4`, …) — the two naming
/// conventions of the example parts.
pub fn lamp_suggestions(model: &SignalModel, nodes_per_part: &[Vec<String>]) -> Vec<LampBinding> {
    let mut out: Vec<LampBinding> = Vec::new();
    for (part, nodes) in nodes_per_part.iter().enumerate() {
        for node in nodes {
            let lamp = if let Some(rest) = node.strip_prefix("lamp_") {
                rest.to_string()
            } else if node.starts_with("zs") {
                node.clone()
            } else {
                continue;
            };
            let bound = |l: &LampBinding| l.part as usize == part && &l.node == node;
            if !model.lamps.iter().any(bound) && !out.iter().any(bound) {
                out.push(LampBinding {
                    lamp,
                    part: part as u32,
                    node: node.clone(),
                });
            }
        }
    }
    out
}

/// Left panel: parts, lamp bindings, lamp test.
fn panel(root: &mut egui::Ui, editor: &mut Editor) {
    egui::Panel::left("model")
        .default_size(420.0)
        .resizable(true)
        .frame(editor_ui::panel_frame())
        .show(root, |ui| {
            ui.label(editor_ui::heading(t!("heading-signal-model")));
            ui.add_space(space::S);
            egui::ScrollArea::vertical().show(ui, |ui| {
                editor_ui::section(ui, "parts", t!("group-signal-parts"), |ui| {
                    parts_section(ui, editor);
                });
                editor_ui::section(ui, "lods", t!("group-lods"), |ui| {
                    lods_section(ui, editor);
                });
                editor_ui::section(ui, "motions", t!("group-signal-motions"), |ui| {
                    motions_section(ui, editor);
                });
                editor_ui::section(ui, "lamps", t!("group-signal-lamps"), |ui| {
                    lamps_section(ui, editor);
                });
                editor_ui::section(ui, "test", t!("group-signal-test"), |ui| {
                    test_section(ui, editor);
                });
            });
        });
}

fn parts_section(ui: &mut egui::Ui, editor: &mut Editor) {
    let mut remove = None;
    let mut repick = None;
    for i in 0..editor.model.parts.len() {
        editor_ui::card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("#{i}")).strong());
                let file = editor.model.parts[i].file.clone();
                ui.label(
                    egui::RichText::new(file)
                        .monospace()
                        .color(colors::TEXT_SECONDARY),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("×").clicked() {
                        remove = Some(i);
                    }
                    if ui.button("…").clicked() {
                        repick = Some(i);
                    }
                });
            });
            mount_row(ui, editor, i);
        });
        ui.add_space(space::XS);
    }
    if let Some(i) = remove {
        editor.remove_part(i);
    }
    if let Some(i) = repick
        && let Some(file) = pick_part_file(editor)
    {
        editor.model.parts[i].file = file;
    }
    if ui.button(t!("action-add-part")).clicked()
        && let Some(file) = pick_part_file(editor)
    {
        editor.model.parts.push(SignalPart { file, mount: None });
    }
}

/// Where a part hangs: the signal position, or a mount node of another part.
fn mount_row(ui: &mut egui::Ui, editor: &mut Editor, i: usize) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(t!("sig-mount"))
                .small()
                .color(colors::TEXT_SECONDARY),
        );
        let current = editor.model.parts[i].mount.clone();
        let selected = match &current {
            None => t!("sig-mount-root"),
            Some((p, _)) => editor
                .model
                .parts
                .get(*p as usize)
                .map(|part| part_label(*p as usize, part))
                .unwrap_or_else(|| format!("#{p}")),
        };
        let mut changed = None;
        egui::ComboBox::from_id_salt(("mount-parent", i))
            .selected_text(selected)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(current.is_none(), t!("sig-mount-root"))
                    .clicked()
                {
                    changed = Some(None);
                }
                for j in 0..editor.model.parts.len() {
                    // A part cannot hang below itself.
                    if j == i || editor.would_cycle(i, j) {
                        continue;
                    }
                    let label = part_label(j, &editor.model.parts[j]);
                    let here = current.as_ref().is_some_and(|(p, _)| *p as usize == j);
                    if ui.selectable_label(here, label).clicked() {
                        // Keep the node when only the parent changes and the
                        // node exists there too; otherwise the first mount point.
                        let node = current
                            .as_ref()
                            .map(|(_, n)| n.clone())
                            .filter(|n| editor.parts.get(j).is_some_and(|s| s.nodes.contains(n)))
                            .or_else(|| {
                                editor
                                    .parts
                                    .get(j)
                                    .map(|s| ordered_nodes(&s.nodes, "mp_"))
                                    .and_then(|nodes| nodes.first().cloned())
                            })
                            .unwrap_or_default();
                        changed = Some(Some((j as u32, node)));
                    }
                }
            });
        if let Some(mount) = changed {
            editor.model.parts[i].mount = mount;
        }
        if let Some((parent, node)) = editor.model.parts[i].mount.clone() {
            let nodes = editor
                .parts
                .get(parent as usize)
                .map(|s| ordered_nodes(&s.nodes, "mp_"))
                .unwrap_or_default();
            let mut picked = None;
            egui::ComboBox::from_id_salt(("mount-node", i))
                .selected_text(if node.is_empty() {
                    t!("sig-mount-node")
                } else {
                    node.clone()
                })
                .show_ui(ui, |ui| {
                    for candidate in &nodes {
                        if ui.selectable_label(*candidate == node, candidate).clicked() {
                            picked = Some(candidate.clone());
                        }
                    }
                });
            if let Some(picked) = picked {
                editor.model.parts[i].mount = Some((parent, picked));
            }
        }
    });
}

/// Default distances for freshly detected levels — the vehicle editor's set.
const DEFAULT_LOD_DISTANCES: [f64; 4] = [150.0, 400.0, 1_000.0, 4_000.0];

/// The levels present in the loaded parts' node names, with default distances.
fn detect_lods(parts: &[PartState]) -> Vec<Lod> {
    let levels: std::collections::BTreeSet<u8> = parts
        .iter()
        .flat_map(|s| s.nodes.iter().filter_map(|n| lod_level(n)))
        .collect();
    levels
        .into_iter()
        .enumerate()
        .map(|(i, level)| Lod {
            level,
            distance: DEFAULT_LOD_DISTANCES.get(i).copied().unwrap_or(4_000.0),
        })
        .collect()
}

/// Levels of detail over all parts: read from the node names, distance per level.
fn lods_section(ui: &mut egui::Ui, editor: &mut Editor) {
    let detected = detect_lods(&editor.parts);
    let same = editor
        .model
        .lods
        .iter()
        .map(|l| l.level)
        .eq(detected.iter().map(|l| l.level));
    if ui
        .add_enabled(
            !detected.is_empty() && !same,
            egui::Button::new(t!("action-read-node-names")),
        )
        .on_hover_text(t!("action-read-node-names-hint", count = detected.len()))
        .clicked()
    {
        editor.model.lods = detected;
    }
    let mut remove = None;
    for i in 0..editor.model.lods.len() {
        ui.horizontal(|ui| {
            let level = editor.model.lods[i].level;
            // The type designation stays a literal, as everywhere.
            if ui
                .selectable_label(editor.preview_lod == level, format!("LOD{level}"))
                .on_hover_text(t!("lod-show-hint"))
                .clicked()
            {
                editor.preview_lod = level;
            }
            ui.add(
                egui::DragValue::new(&mut editor.model.lods[i].distance)
                    .speed(10.0)
                    .range(1.0..=100_000.0)
                    .suffix(" m"),
            )
            .on_hover_text(t!("lod-distance-hint"));
            if ui.button("×").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        let gone = editor.model.lods.remove(i);
        // Previewing a removed level would hide everything without a word.
        if editor.preview_lod == gone.level {
            editor.preview_lod = editor.model.lods.iter().map(|l| l.level).min().unwrap_or(0);
        }
    }
}

/// Moving nodes: lamp-image string, node, motion and travel time.
fn motions_section(ui: &mut egui::Ui, editor: &mut Editor) {
    let mut remove = None;
    for i in 0..editor.model.motions.len() {
        editor_ui::card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                let mut lamp = editor.model.motions[i].lamp.clone();
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut lamp)
                            .hint_text(t!("sig-lamp"))
                            .desired_width(90.0),
                    )
                    .changed()
                {
                    editor.model.motions[i].lamp = lamp;
                }
                let part = editor.model.motions[i].part as usize;
                let selected = editor
                    .model
                    .parts
                    .get(part)
                    .map(|p| part_label(part, p))
                    .unwrap_or_else(|| format!("#{part}"));
                egui::ComboBox::from_id_salt(("motion-part", i))
                    .selected_text(selected)
                    .width(120.0)
                    .show_ui(ui, |ui| {
                        for j in 0..editor.model.parts.len() {
                            let label = part_label(j, &editor.model.parts[j]);
                            if ui.selectable_label(part == j, label).clicked() {
                                editor.model.motions[i].part = j as u32;
                            }
                        }
                    });
                let part = editor.model.motions[i].part as usize;
                let nodes = editor
                    .parts
                    .get(part)
                    .map(|s| s.nodes.clone())
                    .unwrap_or_default();
                let node = editor.model.motions[i].node.clone();
                let missing = !nodes.is_empty() && !nodes.contains(&node);
                let mut text = egui::RichText::new(if node.is_empty() {
                    t!("sig-node")
                } else {
                    node.clone()
                });
                if missing {
                    text = text.color(colors::ERROR);
                }
                egui::ComboBox::from_id_salt(("motion-node", i))
                    .selected_text(text)
                    .show_ui(ui, |ui| {
                        for candidate in &nodes {
                            if ui.selectable_label(*candidate == node, candidate).clicked() {
                                editor.model.motions[i].node = candidate.clone();
                            }
                        }
                    });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("×").clicked() {
                        remove = Some(i);
                    }
                });
            });
            motion_row(ui, i, &mut editor.model.motions[i]);
        });
        ui.add_space(space::XS);
    }
    if let Some(i) = remove {
        editor.model.motions.remove(i);
    }
    if ui.button(t!("action-add-motion")).clicked() {
        editor.model.motions.push(MotionBinding {
            lamp: String::new(),
            part: 0,
            node: String::new(),
            motion: Motion::Rotate {
                axis: [0.0, 0.0, 1.0],
                degrees: 45.0,
            },
            seconds: 1.5,
        });
    }
}

/// Motion kind, axis, amount and travel time of one binding.
fn motion_row(ui: &mut egui::Ui, index: usize, binding: &mut MotionBinding) {
    ui.horizontal(|ui| {
        let selected = match binding.motion {
            Motion::Visibility => t!("motion-visible"),
            Motion::Rotate { .. } => t!("motion-rotate"),
            Motion::Translate { .. } => t!("motion-move"),
            // Not offered below: a signal lamp is switched, not dimmed.
            Motion::Emissive => t!("motion-glow"),
        };
        egui::ComboBox::from_id_salt(("motion-kind", index))
            .selected_text(selected)
            .width(100.0)
            .show_ui(ui, |ui| {
                let rotate = Motion::Rotate {
                    axis: [0.0, 0.0, 1.0],
                    degrees: 45.0,
                };
                let translate = Motion::Translate {
                    axis: [0.0, 1.0, 0.0],
                    metres: 0.5,
                };
                for (label, template) in [
                    (t!("motion-visible"), Motion::Visibility),
                    (t!("motion-rotate"), rotate),
                    (t!("motion-move"), translate),
                ] {
                    let here = std::mem::discriminant(&binding.motion)
                        == std::mem::discriminant(&template);
                    if ui.selectable_label(here, label).clicked() && !here {
                        binding.motion = template;
                    }
                }
            });
        match &mut binding.motion {
            Motion::Visibility | Motion::Emissive => {}
            Motion::Rotate { axis, degrees } => {
                axis_drags(ui, axis);
                ui.add(
                    egui::DragValue::new(degrees)
                        .speed(1.0)
                        .range(-360.0..=360.0)
                        .suffix("°"),
                );
            }
            Motion::Translate { axis, metres } => {
                axis_drags(ui, axis);
                ui.add(
                    egui::DragValue::new(metres)
                        .speed(0.01)
                        .range(-10.0..=10.0)
                        .suffix(" m"),
                );
            }
        }
        ui.add(
            egui::DragValue::new(&mut binding.seconds)
                .speed(0.1)
                .range(0.0..=60.0)
                .suffix(" s"),
        )
        .on_hover_text(t!("sig-seconds-hint"));
    });
}

fn axis_drags(ui: &mut egui::Ui, axis: &mut [f32; 3]) {
    for value in axis.iter_mut() {
        ui.add(
            egui::DragValue::new(value)
                .speed(0.05)
                .range(-1.0..=1.0)
                .max_decimals(2),
        );
    }
}

fn lamps_section(ui: &mut egui::Ui, editor: &mut Editor) {
    let mut remove = None;
    for i in 0..editor.model.lamps.len() {
        editor_ui::card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                let mut lamp = editor.model.lamps[i].lamp.clone();
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut lamp)
                            .hint_text(t!("sig-lamp"))
                            .desired_width(90.0),
                    )
                    .changed()
                {
                    editor.model.lamps[i].lamp = lamp;
                }
                let part = editor.model.lamps[i].part as usize;
                let selected = editor
                    .model
                    .parts
                    .get(part)
                    .map(|p| part_label(part, p))
                    .unwrap_or_else(|| format!("#{part}"));
                egui::ComboBox::from_id_salt(("lamp-part", i))
                    .selected_text(selected)
                    .width(120.0)
                    .show_ui(ui, |ui| {
                        for j in 0..editor.model.parts.len() {
                            let label = part_label(j, &editor.model.parts[j]);
                            if ui.selectable_label(part == j, label).clicked() {
                                editor.model.lamps[i].part = j as u32;
                            }
                        }
                    });
                let part = editor.model.lamps[i].part as usize;
                let nodes = editor
                    .parts
                    .get(part)
                    .map(|s| ordered_nodes(&s.nodes, "lamp_"))
                    .unwrap_or_default();
                let node = editor.model.lamps[i].node.clone();
                // Red marks a node the loaded file does not have — a typo, or
                // the part is still loading.
                let missing = !nodes.is_empty() && !nodes.contains(&node);
                let mut text = egui::RichText::new(if node.is_empty() {
                    t!("sig-node")
                } else {
                    node.clone()
                });
                if missing {
                    text = text.color(colors::ERROR);
                }
                egui::ComboBox::from_id_salt(("lamp-node", i))
                    .selected_text(text)
                    .show_ui(ui, |ui| {
                        for candidate in &nodes {
                            if ui.selectable_label(*candidate == node, candidate).clicked() {
                                editor.model.lamps[i].node = candidate.clone();
                            }
                        }
                    });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("×").clicked() {
                        remove = Some(i);
                    }
                });
            });
        });
        ui.add_space(space::XS);
    }
    if let Some(i) = remove {
        editor.model.lamps.remove(i);
    }
    ui.horizontal(|ui| {
        if ui.button(t!("action-add-lamp")).clicked() {
            editor.model.lamps.push(LampBinding {
                lamp: String::new(),
                part: 0,
                node: String::new(),
            });
        }
        let nodes: Vec<Vec<String>> = editor.parts.iter().map(|s| s.nodes.clone()).collect();
        let suggestions = lamp_suggestions(&editor.model, &nodes);
        if ui
            .add_enabled(
                !suggestions.is_empty(),
                egui::Button::new(t!("action-take-suggestions")),
            )
            .on_hover_text(t!(
                "action-take-suggestions-hint",
                count = suggestions.len()
            ))
            .clicked()
        {
            editor.model.lamps.extend(suggestions);
        }
    });
}

/// Toggles per lamp image — what the aspect rules would light, lit by hand.
/// Motion strings are lamp images too: toggling one swings its arm.
fn test_section(ui: &mut egui::Ui, editor: &mut Editor) {
    let images: std::collections::BTreeSet<String> = editor
        .model
        .lamps
        .iter()
        .map(|l| l.lamp.clone())
        .chain(editor.model.motions.iter().map(|m| m.lamp.clone()))
        .filter(|l| !l.is_empty())
        .collect();
    if images.is_empty() {
        ui.label(
            egui::RichText::new(t!("sig-test-empty"))
                .small()
                .color(colors::TEXT_SECONDARY),
        );
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for image in &images {
            let mut lit = editor.lit.contains(image);
            if ui.toggle_value(&mut lit, image).changed() {
                if lit {
                    editor.lit.insert(image.clone());
                } else {
                    editor.lit.remove(image);
                }
            }
        }
    });
    if ui.button(t!("action-lamps-off")).clicked() {
        editor.lit.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions_follow_the_naming_conventions() {
        let model = SignalModel {
            parts: vec![
                SignalPart {
                    file: "a.gltf".into(),
                    mount: None,
                },
                SignalPart {
                    file: "b.gltf".into(),
                    mount: None,
                },
            ],
            lamps: vec![LampBinding {
                lamp: "red".into(),
                part: 0,
                node: "lamp_red".into(),
            }],
            ..Default::default()
        };
        let nodes = vec![
            vec!["lamp_red".into(), "lamp_green".into(), "mast".into()],
            vec!["zs3_4".into(), "board".into()],
        ];
        let got = lamp_suggestions(&model, &nodes);
        // Already-bound nodes stay out; `lamp_` strips its prefix, `zs*` keeps its name.
        assert_eq!(got.len(), 2);
        assert!(
            got.iter()
                .any(|l| l.lamp == "green" && l.part == 0 && l.node == "lamp_green")
        );
        assert!(got.iter().any(|l| l.lamp == "zs3_4" && l.part == 1));
    }

    #[test]
    fn mount_combos_prefer_their_prefix() {
        let nodes = vec!["board".into(), "mp_top".into(), "lamp_red".into()];
        let ordered = ordered_nodes(&nodes, "mp_");
        assert_eq!(ordered[0], "mp_top");
        assert_eq!(ordered.len(), 3);
    }
}
