//! Desktop UI of the vehicle editor: menu bar, data panel, model panel, status bar.

use crate::{Editor, PointerOverUi, model};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
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
        editor.status = format!("{file} loading…");
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
            ui.menu_button("File", |ui| {
                if ui.button("New").clicked() {
                    *editor = Editor::default();
                    ui.close();
                }
                if ui.button("Open…").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Vehicle (RON)", &["ron"])
                        .pick_file()
                    {
                        editor.open(path);
                    }
                    ui.close();
                }
                let has_path = editor.path.is_some();
                if ui
                    .add_enabled(has_path, egui::Button::new("Save"))
                    .clicked()
                {
                    if let Some(path) = editor.path.clone() {
                        editor.save(path);
                    }
                    ui.close();
                }
                if ui.button("Save as…").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Vehicle (RON)", &["ron"])
                        .set_file_name("vehicle.ron")
                        .save_file()
                    {
                        editor.save(path);
                    }
                    ui.close();
                }
                ui.separator();
                if ui.button("Import model…").clicked() {
                    import_model(editor, assets);
                    ui.close();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    std::process::exit(0);
                }
            });
            ui.menu_button("View", |ui| {
                ui.checkbox(&mut editor.show_reference, "Reference body (LÜP)");
            });
            ui.menu_button("Help", |ui| {
                ui.label("Right mouse button: rotate · Wheel: zoom");
                ui.label("Model conventions: see MODS.md");
            });
        });
    });
}

/// Opens a glTF file. The path is stored relative to the `mods/` directory, because that is
/// how the simulator finds it later (`mods://<mod>/assets/…`).
fn import_model(editor: &mut Editor, assets: &mut AssetServer) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("Model (glTF)", &["gltf", "glb"])
        .set_directory(crate::mods_dir())
        .pick_file()
    else {
        return;
    };
    let Ok(relative) = path.strip_prefix(crate::mods_dir()) else {
        editor.status = format!(
            "{} lies outside mods/ — copy the model into your mod first",
            path.display()
        );
        return;
    };
    let file = relative.to_string_lossy().replace('\\', "/");
    editor.model_mut().file = file.clone();
    editor.loaded_file = file.clone();
    editor.nodes.clear();
    editor.gltf = Some(assets.load(format!("{}://{file}", crate::MOD_SOURCE)));
    editor.dirty = true;
    editor.status = format!("{file} loading…");
}

/// Left panel: the vehicle's base data (plan 15.2).
fn data_panel(root: &mut egui::Ui, editor: &mut Editor) {
    egui::Panel::left("data")
        .default_size(340.0)
        .resizable(true)
        .show(root, |ui| {
            let before = editor.spec.clone();
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Vehicle");
                ui.add(egui::TextEdit::singleline(&mut editor.spec.name).hint_text("Name"));
                ui.separator();

                let spec = &mut editor.spec;
                egui::Grid::new("base").num_columns(2).show(ui, |ui| {
                    row(
                        ui,
                        "Length over buffers",
                        "m — official LÜP; draw the buffers 1–2 cm compressed",
                        |ui| {
                            ui.add(
                                egui::DragValue::new(&mut spec.length)
                                    .speed(0.1)
                                    .range(1.0..=100.0),
                            );
                        },
                    );
                    row(
                        ui,
                        "Gauge",
                        "m — checked against the infrastructure",
                        |ui| {
                            ui.add(
                                egui::DragValue::new(&mut spec.gauge)
                                    .speed(0.001)
                                    .range(0.6..=2.0),
                            );
                        },
                    );
                    row(ui, "v max", "km/h — running gear limit", |ui| {
                        ui.add(
                            egui::DragValue::new(&mut spec.v_max)
                                .speed(1.0)
                                .range(0.0..=400.0),
                        );
                    });
                    row(ui, "Mass", "kg — tare mass", |ui| {
                        ui.add(
                            egui::DragValue::new(&mut spec.mass_empty)
                                .speed(100.0)
                                .range(1_000.0..=200_000.0),
                        );
                    });
                    row(ui, "Max payload", "kg — passenger coach about 5 t", |ui| {
                        ui.add(
                            egui::DragValue::new(&mut spec.max_payload)
                                .speed(100.0)
                                .range(0.0..=120_000.0),
                        );
                    });
                    ui.end_row();
                });

                ui.separator();
                ui.label(egui::RichText::new("Running gear").strong());
                egui::Grid::new("gear").num_columns(2).show(ui, |ui| {
                    row(
                        ui,
                        "Rotating mass",
                        "share of the mass — E loco 0.15–0.25, coach 0.06–0.09",
                        |ui| {
                            ui.add(
                                egui::DragValue::new(&mut spec.rotating_mass_factor)
                                    .speed(0.005)
                                    .range(0.0..=0.5),
                            );
                        },
                    );
                    row(ui, "Axles", "information for consist lists", |ui| {
                        ui.add(egui::DragValue::new(&mut spec.axles).range(0..=32));
                    });
                    row(
                        ui,
                        "Axle base sum",
                        "m — sum over all bogies, basis of the curve resistance",
                        |ui| {
                            ui.add(
                                egui::DragValue::new(&mut spec.axle_base_sum)
                                    .speed(0.1)
                                    .range(0.0..=40.0),
                            );
                        },
                    );
                    row(ui, "Tilt angle", "° — 0 conventional, ~8 tilting", |ui| {
                        ui.add(
                            egui::DragValue::new(&mut spec.tilt_angle_deg)
                                .speed(0.5)
                                .range(0.0..=12.0),
                        );
                    });
                    row(
                        ui,
                        "Hunting",
                        "−1 none … 0 standard … 1 strong",
                        |ui| {
                            ui.add(egui::Slider::new(&mut spec.hunting, -1.0..=1.0));
                        },
                    );
                    ui.end_row();
                });

                ui.separator();
                ui.label(egui::RichText::new("Running resistance").strong());
                egui::Grid::new("resistance").num_columns(2).show(ui, |ui| {
                    ui.label("Rolling resistance a");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut spec.davis.a)
                                .speed(10.0)
                                .range(0.0..=20_000.0),
                        );
                        if ui
                            .button("Suggest")
                            .on_hover_text("about 2 ‰ of the weight")
                            .clicked()
                        {
                            spec.davis.a =
                                VehicleSpec::suggested_rolling_resistance(spec.mass_empty);
                        }
                    });
                    ui.end_row();
                    row(ui, "Speed term b", "N/(m/s)", |ui| {
                        ui.add(
                            egui::DragValue::new(&mut spec.davis.b)
                                .speed(1.0)
                                .range(0.0..=500.0),
                        );
                    });

                    let mut use_cw_a = spec.cw_a.is_some();
                    ui.label("Air resistance");
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut use_cw_a, "cw·A").changed() {
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
                                .on_hover_text("quadratic Davis term c");
                            }
                        }
                    });
                    ui.end_row();
                });
                ui.small(format!(
                    "Resistance at 100 km/h: {:.0} N",
                    spec.resistance(100.0 / 3.6)
                ));

                ui.separator();
                ui.label(egui::RichText::new("Behaviour").strong());
                let mut script = spec.script.clone().unwrap_or_default();
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut script)
                            .hint_text("Lua script <mod>:<name>"),
                    )
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
        && a.script == b.script
}

/// Right panel: model file, levels of detail, moving parts.
fn model_panel(root: &mut egui::Ui, editor: &mut Editor, assets: &mut AssetServer) {
    egui::Panel::right("model")
        .default_size(380.0)
        .resizable(true)
        .show(root, |ui| {
            ui.heading("Model");
            let file = editor
                .spec
                .model
                .as_ref()
                .map(|m| m.file.clone())
                .unwrap_or_default();
            ui.horizontal(|ui| {
                if ui.button("Import glTF…").clicked() {
                    import_model(editor, assets);
                }
                ui.label(if file.is_empty() {
                    "—".to_string()
                } else {
                    file
                });
            });
            ui.separator();

            if editor.nodes.is_empty() {
                ui.label("No model loaded.");
                ui.small(
                    "Levels of detail: node names ending in _LOD0, _LOD1, …\n\
                     Moving parts: prefixes door_, pant_, sw_, gauge_, lamp_, wheel_,\n\
                     or the Blender custom property ts_function.",
                );
                return;
            }

            ui.label(egui::RichText::new("Levels of detail").strong());
            if ui.button("Read from node names").clicked() {
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
                            .on_hover_text("show in the viewport")
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
            ui.label(egui::RichText::new("Moving parts").strong());
            if ui.button("Take over all suggestions").clicked() {
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
                                    .hint_text("function"),
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

            ui.label(egui::RichText::new("Nodes in the file").strong());
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
                            .on_hover_text("bind as a moving part")
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
    let label = match motion {
        Motion::Visibility => "visible",
        Motion::Rotate { .. } => "rotate",
        Motion::Translate { .. } => "move",
    };
    egui::ComboBox::from_id_salt(("motion", id))
        .selected_text(label)
        .width(90.0)
        .show_ui(ui, |ui| {
            if ui.selectable_label(label == "visible", "visible").clicked() {
                *motion = Motion::Visibility;
                changed = true;
            }
            if ui.selectable_label(label == "rotate", "rotate").clicked() {
                *motion = Motion::Rotate {
                    axis: [1.0, 0.0, 0.0],
                    degrees: 90.0,
                };
                changed = true;
            }
            if ui.selectable_label(label == "move", "move").clicked() {
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
                    ui.label("• unsaved");
                }
                ui.label(
                    editor
                        .path
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "(new)".into()),
                );
            });
        });
    });
}

/// A labelled row with a tooltip.
fn row(ui: &mut egui::Ui, label: &str, hint: &str, widget: impl FnOnce(&mut egui::Ui)) {
    ui.label(label).on_hover_text(hint);
    widget(ui);
    ui.end_row();
}
