//! Displays section of the model panel: screens rendered to texture on a glTF
//! node (plan ch. 12). The editor binds name, node and resolution; the widget
//! list itself is data edited in the vehicle file.

use crate::Editor;
use bevy_egui::egui;
use editor_ui::{colors, space};
use i18n::t;
use sim_core::cab::DisplaySpec;

pub fn panel(ui: &mut egui::Ui, editor: &mut Editor) {
    let names: Vec<String> = editor.nodes.iter().map(|n| n.name.clone()).collect();
    let mut changed = false;

    if ui
        .button(t!("action-add-display"))
        .on_hover_text(t!("action-add-display-hint"))
        .clicked()
    {
        editor.model_mut().displays.push(DisplaySpec {
            name: String::new(),
            node: names.first().cloned().unwrap_or_default(),
            width: 256,
            height: 160,
            widgets: Vec::new(),
            html: None,
        });
        changed = true;
    }
    ui.add_space(space::XS);

    let mut remove = None;
    if let Some(model) = editor.spec.model.as_mut() {
        for (i, display) in model.displays.iter_mut().enumerate() {
            let missing = !names.iter().any(|n| n == &display.node);
            editor_ui::card_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    // The bound node is the card's identity, like on a part card.
                    node_combo(ui, i, &mut display.node, &names, missing, &mut changed);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("×").clicked() {
                            remove = Some(i);
                        }
                    });
                });
                editor_ui::form_grid(&format!("display-{i}")).show(ui, |ui| {
                    crate::ui::row(ui, "disp-name", |ui| {
                        changed |= ui.text_edit_singleline(&mut display.name).changed();
                    });
                    crate::ui::row(ui, "disp-size", |ui| {
                        ui.spacing_mut().interact_size.x = 64.0;
                        changed |= ui
                            .add(egui::DragValue::new(&mut display.width).range(16..=1024))
                            .changed();
                        ui.label(
                            egui::RichText::new("×")
                                .small()
                                .color(colors::TEXT_SECONDARY),
                        );
                        changed |= ui
                            .add(egui::DragValue::new(&mut display.height).range(16..=1024))
                            .changed();
                    });
                    // Optional HTML content path (plan ch. 12): a path below
                    // `mods/`; an empty field means the display keeps its
                    // widget or script content.
                    crate::ui::row(ui, "disp-html", |ui| {
                        let mut html = display.html.clone().unwrap_or_default();
                        if ui
                            .text_edit_singleline(&mut html)
                            .on_hover_text(t!("disp-html-hint"))
                            .changed()
                        {
                            display.html = (!html.is_empty()).then_some(html);
                            changed = true;
                        }
                    });
                });
                // The widget list is edited in the vehicle file, not here —
                // say how many there are so a stale binding is noticed.
                ui.label(
                    egui::RichText::new(t!("disp-widgets", count = display.widgets.len()))
                        .small()
                        .color(colors::TEXT_SECONDARY),
                );
            });
        }
        if let Some(i) = remove {
            model.displays.remove(i);
            changed = true;
        }
    }
    editor.dirty |= changed;
}

/// The glTF node the texture is rendered onto. A binding whose node the
/// current model no longer has is drawn in red — the screen would silently
/// stay dark in the simulator.
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
    let width = ui.available_width() - 40.0;
    let response = egui::ComboBox::from_id_salt(("display-node", id))
        .selected_text(selected)
        .width(width)
        .truncate()
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
        .on_hover_text(t!("disp-node"));
    if missing {
        response.on_hover_text(t!("part-node-missing-hint"));
    }
}
