//! Cab section of the model panel: eye point and the interactive controls
//! (plan ch. 12). Lives in the model panel because a cab is model data — it
//! binds glTF nodes and states its eye point in model space.

use crate::Editor;
use crate::ui::{motion_combo, motion_params};
use bevy_egui::egui;
use editor_ui::{colors, space};
use i18n::t;
use sim_core::cab::{CabControl, CabControlSpec, CabSpec};
use sim_core::train::Motion;

/// Width of the input combo — the card's identity, so it gets the wide slot.
const INPUT_COMBO_W: f32 = 200.0;

pub fn panel(ui: &mut egui::Ui, editor: &mut Editor) {
    if editor.spec.model.as_ref().and_then(|m| m.cab.as_ref()).is_none() {
        ui.label(
            egui::RichText::new(t!("cab-none"))
                .small()
                .color(colors::TEXT_SECONDARY),
        );
        ui.add_space(space::XS);
        if ui
            .button(t!("action-add-cab"))
            .on_hover_text(t!("action-add-cab-hint"))
            .clicked()
        {
            editor.model_mut().cab = Some(CabSpec::default());
            editor.dirty = true;
        }
        return;
    }

    let names: Vec<String> = editor.nodes.iter().map(|n| n.name.clone()).collect();
    let mut changed = false;
    let mut remove = None;
    let tests = &mut editor.cab_test;
    let Some(cab) = editor.spec.model.as_mut().and_then(|m| m.cab.as_mut()) else {
        return;
    };

    editor_ui::form_grid("cab-eye").show(ui, |ui| {
        crate::ui::row(ui, "cab-eye", |ui| {
            ui.spacing_mut().interact_size.x = 64.0;
            for (label, value) in ["X", "Y", "Z"].iter().zip(cab.eye.iter_mut()) {
                ui.label(
                    egui::RichText::new(*label)
                        .small()
                        .color(colors::TEXT_SECONDARY),
                );
                changed |= ui
                    .add(egui::DragValue::new(value).speed(0.05).suffix("\u{A0}m"))
                    .changed();
            }
        });
    });
    ui.add_space(space::S);

    if ui
        .button(t!("action-add-control"))
        .on_hover_text(t!("action-add-control-hint"))
        .clicked()
    {
        cab.controls.push(CabControlSpec {
            node: names.first().cloned().unwrap_or_default(),
            input: CabControl::Throttle,
            motion: Motion::Rotate {
                axis: [1.0, 0.0, 0.0],
                degrees: 30.0,
            },
        });
        changed = true;
    }
    ui.add_space(space::XS);

    for (i, control) in cab.controls.iter_mut().enumerate() {
        let missing = !names.iter().any(|n| n == &control.node);
        editor_ui::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                // The bound input is the card's identity, like the node name
                // on a part card.
                input_combo(ui, i, &mut control.input, &mut changed);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("×").clicked() {
                        remove = Some(i);
                    }
                });
            });
            ui.horizontal(|ui| {
                node_combo(ui, i, &mut control.node, &names, missing, &mut changed);
                changed |= motion_combo(ui, ("cab", i), &mut control.motion);
            });
            changed |= motion_params(ui, &mut control.motion);
            // Transient preview value — moves the node in the viewport, not saved.
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(t!("cab-control-test"))
                        .small()
                        .color(colors::TEXT_SECONDARY),
                )
                .on_hover_text(t!("cab-control-test-hint"));
                let value = tests.entry(i).or_insert(0.0);
                ui.add(egui::Slider::new(value, 0.0..=1.0).show_value(false));
            });
        });
    }

    if let Some(i) = remove {
        cab.controls.remove(i);
        // The map is keyed by index; after a removal every key past it would
        // point at the wrong card.
        tests.clear();
        changed = true;
    }
    editor.dirty |= changed;
}

/// Which simulation input the control operates — closed list, like the sound
/// table's quantity combo.
fn input_combo(ui: &mut egui::Ui, id: usize, input: &mut CabControl, changed: &mut bool) {
    egui::ComboBox::from_id_salt(("cab-input", id))
        .selected_text(t!(input.key()))
        .width(INPUT_COMBO_W)
        .show_ui(ui, |ui| {
            for option in CabControl::ALL {
                if ui
                    .selectable_label(*input == option, t!(option.key()))
                    .clicked()
                {
                    *input = option;
                    *changed = true;
                }
            }
        })
        .response
        .on_hover_text(t!("cab-control-input"));
}

/// The glTF node the control grabs. A binding whose node the current model no
/// longer has is drawn in red — it would silently do nothing in the simulator.
fn node_combo(
    ui: &mut egui::Ui,
    id: usize,
    node: &mut String,
    names: &[String],
    missing: bool,
    changed: &mut bool,
) {
    let selected = egui::RichText::new(node.as_str()).monospace();
    let selected = if missing {
        selected.color(colors::ERROR)
    } else {
        selected
    };
    let width = ui.available_width() - crate::ui::MOTION_COMBO_W - space::S;
    let response = egui::ComboBox::from_id_salt(("cab-node", id))
        .selected_text(selected)
        .width(width)
        .show_ui(ui, |ui| {
            for name in names {
                if ui
                    .selectable_label(node == name, egui::RichText::new(name).monospace())
                    .clicked()
                {
                    *node = name.clone();
                    *changed = true;
                }
            }
        })
        .response
        .on_hover_text(t!("cab-control-node"));
    if missing {
        response.on_hover_text(t!("part-node-missing-hint"));
    }
}
