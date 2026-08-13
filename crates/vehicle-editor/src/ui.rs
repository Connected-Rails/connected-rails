//! Desktop UI of the vehicle editor: menu bar, data panel, model panel, status bar.
//!
//! Look and feel come from the `editor-ui` crate; this file only lays out the
//! forms. Every labelled field goes through [`row`], every section through
//! `editor_ui::section`, so labels and fields line up across the whole panel.

use crate::{Editor, model, powertrain};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use editor_ui::{colors, field, space};
use i18n::t;
use sim_core::doors::DoorSystem;
use sim_core::safety::SafetyEquipment;
use sim_core::safety::de::{PzbVariant, SifaKind, TrainType};
use sim_core::train::{Motion, Part, VehicleSpec};

const SHORTCUT_NEW: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::N);
const SHORTCUT_OPEN: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::O);
const SHORTCUT_SAVE: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::S);

/// One frame of UI.
///
/// Since egui 0.35 panels live inside a `Ui`, not on the context: the whole viewport
/// becomes one background `Ui` into which the panels are docked.
pub fn draw(
    mut contexts: EguiContexts,
    mut editor: ResMut<Editor>,
    mut assets: ResMut<AssetServer>,
    mut themed: Local<bool>,
) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
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
        editor.status = t!("status-loading", file = file);
    }
    let mut root = egui::Ui::new(
        ctx.clone(),
        "viewport".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    menu_bar(&mut root, &mut editor, &mut assets);
    status_bar(&mut root, &editor);
    data_panel(&mut root, &mut editor);
    model_panel(&mut root, &mut editor, &mut assets);
    Ok(())
}

fn handle_shortcuts(ctx: &egui::Context, editor: &mut Editor) {
    if ctx.input_mut(|i| i.consume_shortcut(&SHORTCUT_SAVE)) {
        save(editor);
    }
    if ctx.input_mut(|i| i.consume_shortcut(&SHORTCUT_OPEN)) {
        open(editor);
    }
    if ctx.input_mut(|i| i.consume_shortcut(&SHORTCUT_NEW)) {
        *editor = Editor::default();
    }
}

/// Save to the known path, or fall back to the save dialog.
fn save(editor: &mut Editor) {
    match editor.path.clone() {
        Some(path) => editor.save(path),
        None => save_as(editor),
    }
}

fn save_as(editor: &mut Editor) {
    if let Some(path) = rfd::FileDialog::new()
        .add_filter(t!("filter-vehicle-ron"), &["ron"])
        .set_file_name("vehicle.ron")
        .save_file()
    {
        editor.save(path);
    }
}

fn open(editor: &mut Editor) {
    if let Some(path) = rfd::FileDialog::new()
        .add_filter(t!("filter-vehicle-ron"), &["ron"])
        .pick_file()
    {
        editor.open(path);
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
                        *editor = Editor::default();
                        ui.close();
                    }
                    if ui
                        .add(
                            egui::Button::new(t!("action-open"))
                                .shortcut_text(ctx.format_shortcut(&SHORTCUT_OPEN)),
                        )
                        .clicked()
                    {
                        open(editor);
                        ui.close();
                    }
                    if ui
                        .add(
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
                    if ui.button(t!("action-import-model")).clicked() {
                        import_model(editor, assets);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(t!("action-quit")).clicked() {
                        std::process::exit(0);
                    }
                });
                ui.menu_button(t!("menu-view"), |ui| {
                    ui.checkbox(&mut editor.show_reference, t!("view-reference-body"));
                    ui.separator();
                    language_menu(ui);
                });
                ui.menu_button(t!("menu-help"), |ui| {
                    ui.label(t!("help-mouse"));
                    ui.label(t!("help-model-conventions"));
                });
            });
        });
}

/// Language picker — the same submenu in both editors.
pub fn language_menu(ui: &mut egui::Ui) {
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

/// Opens a glTF file. The path is stored relative to the `mods/` directory, because that is
/// how the simulator finds it later (`mods://<mod>/assets/…`).
fn import_model(editor: &mut Editor, assets: &mut AssetServer) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter(t!("filter-model-gltf"), &["gltf", "glb"])
        .set_directory(crate::mods_dir())
        .pick_file()
    else {
        return;
    };
    let Ok(relative) = path.strip_prefix(crate::mods_dir()) else {
        editor.status = t!("status-outside-mods", path = path.display());
        return;
    };
    let file = relative.to_string_lossy().replace('\\', "/");
    editor.model_mut().file = file.clone();
    editor.loaded_file = file.clone();
    editor.nodes.clear();
    editor.gltf = Some(assets.load(format!("{}://{file}", crate::MOD_SOURCE)));
    editor.dirty = true;
    editor.status = t!("status-loading", file = file);
}

/// Left panel: the vehicle's base data (plan 15.2).
fn data_panel(root: &mut egui::Ui, editor: &mut Editor) {
    egui::Panel::left("data")
        .default_size(450.0)
        .resizable(true)
        .frame(editor_ui::panel_frame())
        .show(root, |ui| {
            let before = editor.spec.clone();
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    ui.label(editor_ui::heading(t!("heading-vehicle")));
                    ui.add_space(space::XS);
                    ui.add(
                        egui::TextEdit::singleline(&mut editor.spec.name)
                            .hint_text(t!("field-name"))
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(space::S);

                    let spec = &mut editor.spec;
                    editor_ui::section(ui, "base", t!("group-base-data"), |ui| {
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

                    editor_ui::section(ui, "gear", t!("group-running-gear"), |ui| {
                        editor_ui::form_grid("gear").show(ui, |ui| {
                            row(ui, "veh-rotating-mass", |ui| {
                                field(ui, &mut spec.rotating_mass_factor, 0.005, 0.0..=0.5, "");
                            });
                            row(ui, "veh-axles", |ui| {
                                field(ui, &mut spec.axles, 1.0, 0.0..=32.0, "");
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

                    editor_ui::section(ui, "resistance", t!("group-resistance"), |ui| {
                        editor_ui::form_grid("resistance").show(ui, |ui| {
                            row(ui, "res-rolling", |ui| {
                                field(ui, &mut spec.davis.a, 10.0, 0.0..=20_000.0, "N");
                                if ui
                                    .button(t!("action-suggest"))
                                    .on_hover_text(t!("res-rolling-suggest-hint"))
                                    .clicked()
                                {
                                    spec.davis.a =
                                        VehicleSpec::suggested_rolling_resistance(spec.mass_empty);
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
                                        field(ui, &mut spec.davis.c, 0.1, 0.0..=100.0, "")
                                            .on_hover_text(t!("res-davis-c-hint"));
                                    }
                                }
                                if ui.checkbox(&mut use_cw_a, t!("res-cw-a")).changed() {
                                    spec.cw_a = use_cw_a.then_some(6.0);
                                }
                            });
                            ui.end_row();

                            row(ui, "res-curve", |ui| {
                                field(ui, &mut spec.curve_resistance_factor, 0.05, 0.0..=3.0, "");
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
                    });

                    editor_ui::section(ui, "brake", t!("group-brake"), |ui| {
                        powertrain::brake_panel(ui, &mut spec.brake, &mut spec.slip_protection);
                    });

                    editor_ui::section(ui, "drive", t!("group-drive"), |ui| {
                        powertrain::drive_panel(ui, &mut spec.traction);
                    });

                    editor_ui::section(ui, "equipment", t!("group-equipment"), |ui| {
                        equipment_panel(ui, spec);
                    });

                    editor_ui::section(ui, "behaviour", t!("group-behaviour"), |ui| {
                        let mut script = spec.script.clone().unwrap_or_default();
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut script)
                                    .hint_text(t!("field-script-hint"))
                                    .desired_width(f32::INFINITY),
                            )
                            .changed()
                        {
                            spec.script = (!script.is_empty()).then_some(script);
                        }
                    });
                });
            if editor.spec.name != before.name || !dataless_eq(&editor.spec, &before) {
                editor.dirty = true;
            }
        });
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

/// Compares everything the form can change, without pulling in `PartialEq` for the spec.
fn dataless_eq(a: &VehicleSpec, b: &VehicleSpec) -> bool {
    a.length == b.length
        && a.gauge == b.gauge
        && a.v_max == b.v_max
        && a.mass_empty == b.mass_empty
        && a.max_payload == b.max_payload
        && a.rotating_mass_factor == b.rotating_mass_factor
        && a.axles == b.axles
        && a.axle_base_sum == b.axle_base_sum
        && a.tilt_angle_deg == b.tilt_angle_deg
        && a.hunting == b.hunting
        && a.davis == b.davis
        && a.cw_a == b.cw_a
        && a.curve_resistance_factor == b.curve_resistance_factor
        && a.brake == b.brake
        && a.traction == b.traction
        && a.slip_protection == b.slip_protection
        && a.safety == b.safety
        && a.doors == b.doors
        && a.passenger_doors == b.passenger_doors
        && a.script == b.script
}

/// Right panel: model file, levels of detail, moving parts.
fn model_panel(root: &mut egui::Ui, editor: &mut Editor, assets: &mut AssetServer) {
    egui::Panel::right("model")
        .default_size(400.0)
        .resizable(true)
        .frame(editor_ui::panel_frame())
        .show(root, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
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

                    editor_ui::section(ui, "lods", t!("group-lods"), |ui| {
                        lod_list(ui, editor);
                    });
                    editor_ui::section(ui, "parts", t!("group-parts"), |ui| {
                        parts_list(ui, editor);
                    });
                    editor_ui::section(ui, "nodes", t!("group-nodes"), |ui| {
                        node_list(ui, editor);
                    });
                });
        });
}

fn lod_list(ui: &mut egui::Ui, editor: &mut Editor) {
    if ui.button(t!("action-read-node-names")).clicked() {
        let lods = model::detect_lods(&editor.nodes);
        editor.model_mut().lods = lods;
        editor.dirty = true;
    }
    let mut remove_lod = None;
    let mut preview = editor.preview_lod;
    if let Some(lods) = editor.spec.model.as_mut().map(|m| &mut m.lods) {
        for (i, lod) in lods.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                // Radio button: which level the viewport shows.
                if ui
                    .selectable_label(preview == lod.level, format!("LOD{}", lod.level))
                    .on_hover_text(t!("lod-show-hint"))
                    .clicked()
                {
                    preview = lod.level;
                }
                field(ui, &mut lod.distance, 10.0, 10.0..=20_000.0, "m");
                if ui.small_button("×").clicked() {
                    remove_lod = Some(i);
                }
            });
        }
    }
    editor.preview_lod = preview;
    if let Some(i) = remove_lod {
        editor.model_mut().lods.remove(i);
        editor.dirty = true;
    }
}

fn parts_list(ui: &mut egui::Ui, editor: &mut Editor) {
    if ui.button(t!("action-take-suggestions")).clicked() {
        let parts: Vec<Part> = editor
            .nodes
            .iter()
            .filter_map(|n| n.suggestion.clone())
            .collect();
        let model = editor.model_mut();
        for part in parts {
            if !model.parts.iter().any(|p| p.node == part.node) {
                model.parts.push(part);
            }
        }
        editor.dirty = true;
    }
    ui.add_space(space::XS);

    let mut remove_part = None;
    let mut changed = false;
    if let Some(model) = editor.spec.model.as_mut() {
        for (i, part) in model.parts.iter_mut().enumerate() {
            editor_ui::card_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&part.node).monospace());
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
                                .hint_text(t!("part-function-hint")),
                        )
                        .changed();
                    changed |= motion_combo(ui, i, &mut part.motion);
                });
                changed |= motion_params(ui, &mut part.motion);
            });
        }
    }
    if let Some(i) = remove_part {
        editor.model_mut().parts.remove(i);
        editor.dirty = true;
    }
    editor.dirty |= changed;
}

fn node_list(ui: &mut egui::Ui, editor: &mut Editor) {
    let nodes = editor.nodes.clone();
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
                editor.dirty = true;
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

/// Width of the motion combo inside a part card.
const MOTION_COMBO_W: f32 = 110.0;

/// Kind of motion of a part.
fn motion_combo(ui: &mut egui::Ui, id: usize, motion: &mut Motion) -> bool {
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
fn motion_params(ui: &mut egui::Ui, motion: &mut Motion) -> bool {
    let mut changed = false;
    match motion {
        Motion::Visibility => {}
        Motion::Rotate { axis, degrees } => {
            ui.horizontal(|ui| {
                ui.spacing_mut().interact_size.x = 64.0;
                changed |= axis_editor(ui, axis);
                changed |= ui
                    .add(egui::DragValue::new(degrees).speed(1.0).suffix("\u{A0}°"))
                    .changed();
            });
        }
        Motion::Translate { axis, metres } => {
            ui.horizontal(|ui| {
                ui.spacing_mut().interact_size.x = 64.0;
                changed |= axis_editor(ui, axis);
                changed |= ui
                    .add(egui::DragValue::new(metres).speed(0.01).suffix("\u{A0}m"))
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
                ui.label(&editor.status);
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
pub fn row(ui: &mut egui::Ui, key: &str, widget: impl FnOnce(&mut egui::Ui)) {
    editor_ui::form_label(ui, t!(key)).on_hover_text(t!(&format!("{key}-hint")));
    ui.horizontal(|ui| widget(ui));
    ui.end_row();
}
