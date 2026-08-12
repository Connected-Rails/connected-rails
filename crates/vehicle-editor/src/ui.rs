//! Desktop UI of the vehicle editor: menu bar, data panel, model panel, status bar.

use crate::{Editor, PointerOverUi, model, powertrain};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use i18n::t;
use sim_core::doors::DoorSystem;
use sim_core::safety::SafetyEquipment;
use sim_core::safety::de::{PzbVariant, SifaKind, TrainType};
use sim_core::train::{Motion, Part, VehicleSpec};

/// One frame of UI.
///
/// Since egui 0.35 panels live inside a `Ui`, not on the context: the whole viewport
/// becomes one background `Ui` into which the panels are docked.
pub fn draw(
    mut contexts: EguiContexts,
    mut editor: ResMut<Editor>,
    mut assets: ResMut<AssetServer>,
    mut over_ui: ResMut<PointerOverUi>,
) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
    over_ui.0 = ctx.egui_wants_pointer_input();

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

fn menu_bar(root: &mut egui::Ui, editor: &mut Editor, assets: &mut AssetServer) {
    egui::Panel::top("menu").show(root, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(t!("menu-file"), |ui| {
                if ui.button(t!("action-new")).clicked() {
                    *editor = Editor::default();
                    ui.close();
                }
                if ui.button(t!("action-open")).clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter(t!("filter-vehicle-ron"), &["ron"])
                        .pick_file()
                    {
                        editor.open(path);
                    }
                    ui.close();
                }
                let has_path = editor.path.is_some();
                if ui
                    .add_enabled(has_path, egui::Button::new(t!("action-save")))
                    .clicked()
                {
                    if let Some(path) = editor.path.clone() {
                        editor.save(path);
                    }
                    ui.close();
                }
                if ui.button(t!("action-save-as")).clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter(t!("filter-vehicle-ron"), &["ron"])
                        .set_file_name("vehicle.ron")
                        .save_file()
                    {
                        editor.save(path);
                    }
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
        .default_size(340.0)
        .resizable(true)
        .show(root, |ui| {
            let before = editor.spec.clone();
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading(t!("heading-vehicle"));
                ui.add(
                    egui::TextEdit::singleline(&mut editor.spec.name).hint_text(t!("field-name")),
                );
                ui.separator();

                let spec = &mut editor.spec;
                egui::Grid::new("base").num_columns(2).show(ui, |ui| {
                    row(ui, "veh-length", |ui| {
                        ui.add(
                            egui::DragValue::new(&mut spec.length)
                                .speed(0.1)
                                .range(1.0..=100.0),
                        );
                    });
                    row(ui, "veh-gauge", |ui| {
                        ui.add(
                            egui::DragValue::new(&mut spec.gauge)
                                .speed(0.001)
                                .range(0.6..=2.0),
                        );
                    });
                    row(ui, "veh-vmax", |ui| {
                        ui.add(
                            egui::DragValue::new(&mut spec.v_max)
                                .speed(1.0)
                                .range(0.0..=400.0),
                        );
                    });
                    row(ui, "veh-mass", |ui| {
                        ui.add(
                            egui::DragValue::new(&mut spec.mass_empty)
                                .speed(100.0)
                                .range(1_000.0..=200_000.0),
                        );
                    });
                    row(ui, "veh-payload", |ui| {
                        ui.add(
                            egui::DragValue::new(&mut spec.max_payload)
                                .speed(100.0)
                                .range(0.0..=120_000.0),
                        );
                    });
                    ui.end_row();
                });

                ui.separator();
                ui.label(egui::RichText::new(t!("group-running-gear")).strong());
                egui::Grid::new("gear").num_columns(2).show(ui, |ui| {
                    row(ui, "veh-rotating-mass", |ui| {
                        ui.add(
                            egui::DragValue::new(&mut spec.rotating_mass_factor)
                                .speed(0.005)
                                .range(0.0..=0.5),
                        );
                    });
                    row(ui, "veh-axles", |ui| {
                        ui.add(egui::DragValue::new(&mut spec.axles).range(0..=32));
                    });
                    row(ui, "veh-axle-base", |ui| {
                        ui.add(
                            egui::DragValue::new(&mut spec.axle_base_sum)
                                .speed(0.1)
                                .range(0.0..=40.0),
                        );
                    });
                    row(ui, "veh-tilt", |ui| {
                        ui.add(
                            egui::DragValue::new(&mut spec.tilt_angle_deg)
                                .speed(0.5)
                                .range(0.0..=12.0),
                        );
                    });
                    row(ui, "veh-hunting", |ui| {
                        ui.add(egui::Slider::new(&mut spec.hunting, -1.0..=1.0));
                    });
                    ui.end_row();
                });

                ui.separator();
                ui.label(egui::RichText::new(t!("group-resistance")).strong());
                egui::Grid::new("resistance").num_columns(2).show(ui, |ui| {
                    ui.label(t!("res-rolling"));
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut spec.davis.a)
                                .speed(10.0)
                                .range(0.0..=20_000.0),
                        );
                        if ui
                            .button(t!("action-suggest"))
                            .on_hover_text(t!("res-rolling-suggest-hint"))
                            .clicked()
                        {
                            spec.davis.a =
                                VehicleSpec::suggested_rolling_resistance(spec.mass_empty);
                        }
                    });
                    ui.end_row();
                    row(ui, "res-speed-term", |ui| {
                        ui.add(
                            egui::DragValue::new(&mut spec.davis.b)
                                .speed(1.0)
                                .range(0.0..=500.0),
                        );
                    });

                    let mut use_cw_a = spec.cw_a.is_some();
                    ui.label(t!("res-air"));
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut use_cw_a, t!("res-cw-a")).changed() {
                            spec.cw_a = use_cw_a.then_some(6.0);
                        }
                        match &mut spec.cw_a {
                            Some(cw_a) => {
                                ui.add(
                                    egui::DragValue::new(cw_a)
                                        .speed(0.1)
                                        .range(0.1..=40.0)
                                        .suffix(" m²"),
                                );
                            }
                            None => {
                                ui.add(
                                    egui::DragValue::new(&mut spec.davis.c)
                                        .speed(0.1)
                                        .range(0.0..=100.0),
                                )
                                .on_hover_text(t!("res-davis-c-hint"));
                            }
                        }
                    });
                    ui.end_row();
                });
                ui.horizontal(|ui| {
                    ui.label(t!("res-curve"))
                        .on_hover_text(t!("res-curve-hint"));
                    ui.add(
                        egui::DragValue::new(&mut spec.curve_resistance_factor)
                            .speed(0.05)
                            .range(0.0..=3.0),
                    );
                });
                ui.small(t!(
                    "res-at-100",
                    newtons = format!("{:.0}", spec.resistance(100.0 / 3.6))
                ));

                ui.separator();
                powertrain::brake_panel(ui, &mut spec.brake, &mut spec.slip_protection);
                ui.separator();
                powertrain::drive_panel(ui, &mut spec.traction);

                ui.separator();
                equipment_panel(ui, spec);

                ui.separator();
                ui.label(egui::RichText::new(t!("group-behaviour")).strong());
                let mut script = spec.script.clone().unwrap_or_default();
                if ui
                    .add(egui::TextEdit::singleline(&mut script).hint_text(t!("field-script-hint")))
                    .changed()
                {
                    spec.script = (!script.is_empty()).then_some(script);
                }
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
    ui.label(egui::RichText::new(t!("group-equipment")).strong());

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
    if let SafetyEquipment::De {
        pzb,
        lzb,
        sifa,
        train_type,
    } = &mut spec.safety
    {
        egui::Grid::new("safety").num_columns(2).show(ui, |ui| {
            ui.label(t!("eq-pzb")).on_hover_text(t!("eq-pzb-hint"));
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
            ui.end_row();

            ui.label(t!("eq-train-type"))
                .on_hover_text(t!("eq-train-type-hint"));
            combo(
                ui,
                "train_type",
                train_type,
                &[
                    (TrainType::O, t!("train-type-o")),
                    (TrainType::M, t!("train-type-m")),
                    (TrainType::U, t!("train-type-u")),
                ],
            );
            ui.end_row();

            ui.label(t!("eq-sifa")).on_hover_text(t!("eq-sifa-hint"));
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
            ui.end_row();

            ui.label(t!("eq-lzb"));
            ui.checkbox(lzb, t!("eq-lzb-on-board"))
                .on_hover_text(t!("eq-lzb-hint"));
            ui.end_row();
        });
    }

    ui.checkbox(&mut spec.passenger_doors, t!("eq-passenger-doors"))
        .on_hover_text(t!("eq-passenger-doors-hint"));
    ui.horizontal(|ui| {
        ui.label(t!("eq-doors")).on_hover_text(t!("eq-doors-hint"));
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
        .default_size(380.0)
        .resizable(true)
        .show(root, |ui| {
            ui.heading(t!("heading-model"));
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
                ui.label(if file.is_empty() {
                    t!("common-none")
                } else {
                    file
                });
            });
            ui.separator();

            if editor.nodes.is_empty() {
                ui.label(t!("model-none-loaded"));
                ui.small(t!("model-conventions"));
                return;
            }

            ui.label(egui::RichText::new(t!("group-lods")).strong());
            if ui.button(t!("action-read-node-names")).clicked() {
                let lods = model::detect_lods(&editor.nodes);
                editor.model_mut().lods = lods;
                editor.dirty = true;
            }
            let mut remove_lod = None;
            let mut preview = editor.preview_lod;
            let lods = editor.spec.model.as_mut().map(|m| &mut m.lods);
            if let Some(lods) = lods {
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
                        ui.add(
                            egui::DragValue::new(&mut lod.distance)
                                .speed(10.0)
                                .range(10.0..=20_000.0)
                                .suffix(" m"),
                        );
                        if ui.small_button("✕").clicked() {
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

            ui.separator();
            ui.label(egui::RichText::new(t!("group-parts")).strong());
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

            let mut remove_part = None;
            let mut changed = false;
            if let Some(model) = editor.spec.model.as_mut() {
                for (i, part) in model.parts.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(&part.node);
                        if ui.small_button("✕").clicked() {
                            remove_part = Some(i);
                        }
                    });
                    ui.horizontal(|ui| {
                        changed |= ui
                            .add(
                                egui::TextEdit::singleline(&mut part.function)
                                    .desired_width(140.0)
                                    .hint_text(t!("part-function-hint")),
                            )
                            .changed();
                        changed |= motion_editor(ui, i, &mut part.motion);
                    });
                    ui.separator();
                }
            }
            if let Some(i) = remove_part {
                editor.model_mut().parts.remove(i);
                editor.dirty = true;
            }
            editor.dirty |= changed;

            ui.label(egui::RichText::new(t!("group-nodes")).strong());
            egui::ScrollArea::vertical().show(ui, |ui| {
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
                        let mut label = node.name.clone();
                        if let Some(level) = node.lod {
                            label.push_str(&format!("  · LOD{level}"));
                        }
                        if let Some(hint) = &node.suggestion {
                            label.push_str(&format!("  · {}", hint.function));
                        }
                        ui.label(label);
                    });
                }
            });
        });
}

/// Motion of a part: kind plus axis and amount.
fn motion_editor(ui: &mut egui::Ui, id: usize, motion: &mut Motion) -> bool {
    let mut changed = false;
    let key = match motion {
        Motion::Visibility => "motion-visible",
        Motion::Rotate { .. } => "motion-rotate",
        Motion::Translate { .. } => "motion-move",
    };
    egui::ComboBox::from_id_salt(("motion", id))
        .selected_text(t!(key))
        .width(90.0)
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
    match motion {
        Motion::Visibility => {}
        Motion::Rotate { axis, degrees } => {
            changed |= axis_editor(ui, axis);
            changed |= ui
                .add(egui::DragValue::new(degrees).speed(1.0).suffix(" °"))
                .changed();
        }
        Motion::Translate { axis, metres } => {
            changed |= axis_editor(ui, axis);
            changed |= ui
                .add(egui::DragValue::new(metres).speed(0.01).suffix(" m"))
                .changed();
        }
    }
    changed
}

fn axis_editor(ui: &mut egui::Ui, axis: &mut [f32; 3]) -> bool {
    let mut changed = false;
    for value in axis.iter_mut() {
        changed |= ui
            .add(egui::DragValue::new(value).speed(0.1).range(-1.0..=1.0))
            .changed();
    }
    changed
}

fn status_bar(root: &mut egui::Ui, editor: &Editor) {
    egui::Panel::bottom("status").show(root, |ui| {
        ui.horizontal(|ui| {
            ui.label(&editor.status);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if editor.dirty {
                    ui.label(t!("status-unsaved"));
                }
                ui.label(
                    editor
                        .path
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| t!("status-new-file")),
                );
            });
        });
    });
}

/// A labelled row: `key` names the label, `key`-hint the tooltip.
pub fn row(ui: &mut egui::Ui, key: &str, widget: impl FnOnce(&mut egui::Ui)) {
    ui.label(t!(key)).on_hover_text(t!(&format!("{key}-hint")));
    widget(ui);
    ui.end_row();
}
