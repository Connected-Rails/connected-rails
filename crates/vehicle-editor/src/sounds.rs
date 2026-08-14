//! Sound table of the vehicle (plan ch. 13) — the editor side of [`sim_core::sound`].
//!
//! One card per entry, and each card is the three parts of the entry in the order they are
//! asked about: **trigger** (what starts it), **conditions** (when it may be heard),
//! **dependencies** (which quantity moves volume and pitch). A sparkline under each curve
//! answers "does this look right" — a support point typed one digit wrong reads as a kink
//! there and as a plausible number in the field.

use crate::ui::row;
use bevy_egui::egui;
use editor_ui::{colors, field, space};
use i18n::t;
use sim_core::sound::{Condition, Curve, Quantity, SoundSpec, Trigger};
use sim_core::train::VehicleSpec;

/// Width of the trigger and quantity combos.
const COMBO_W: f32 = 150.0;

pub fn panel(ui: &mut egui::Ui, spec: &mut VehicleSpec) {
    ui.horizontal(|ui| {
        if ui
            .button(t!("action-add-sound"))
            .on_hover_text(t!("action-add-sound-hint"))
            .clicked()
        {
            spec.sounds.push(blank());
        }
    });
    if spec.sounds.is_empty() {
        ui.add_space(space::XS);
        ui.label(
            egui::RichText::new(t!("snd-default-table"))
                .small()
                .color(colors::TEXT_SECONDARY),
        );
        return;
    }

    let mut remove = None;
    for (i, entry) in spec.sounds.iter_mut().enumerate() {
        editor_ui::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut entry.name)
                        .desired_width(ui.available_width() - 30.0)
                        .hint_text(t!("snd-name-placeholder")),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("×").clicked() {
                        remove = Some(i);
                    }
                });
            });
            ui.add(
                egui::TextEdit::singleline(&mut entry.file)
                    .desired_width(f32::INFINITY)
                    .hint_text(t!("snd-file-placeholder")),
            )
            .on_hover_text(t!("snd-file-hint"));

            editor_ui::form_grid(&format!("snd-head-{i}")).show(ui, |ui| {
                row(ui, "snd-trigger", |ui| {
                    trigger_combo(ui, i, &mut entry.trigger)
                });
                trigger_params(ui, i, &mut entry.trigger);
                row(ui, "snd-positional", |ui| {
                    ui.checkbox(&mut entry.positional, "");
                });
            });

            conditions(ui, i, &mut entry.conditions);
            curve(
                ui,
                ("vol", i),
                t!("snd-volume"),
                &mut entry.volume,
                0.0..=1.0,
            );
            curve(
                ui,
                ("pitch", i),
                t!("snd-pitch"),
                &mut entry.pitch,
                sim_core::sound::PITCH_RANGE,
            );
        });
    }
    if let Some(i) = remove {
        spec.sounds.remove(i);
    }
}

/// A fresh entry: a loop at full volume, which is audible at once — an entry that starts out
/// silent looks broken until three more fields have been filled in.
fn blank() -> SoundSpec {
    SoundSpec {
        name: String::new(),
        file: String::new(),
        trigger: Trigger::Loop,
        conditions: Vec::new(),
        volume: None,
        pitch: None,
        positional: true,
    }
}

/// The quantity a trigger, condition or curve depends on.
fn quantity_combo(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    quantity: &mut Quantity,
) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(t!(quantity.key()))
        .width(COMBO_W)
        .show_ui(ui, |ui| {
            for option in Quantity::ALL {
                if ui
                    .selectable_label(*quantity == option, t!(option.key()))
                    .clicked()
                {
                    *quantity = option;
                }
            }
        });
}

/// Kind of trigger. Switching keeps the quantity — the user picked it for a reason.
fn trigger_combo(ui: &mut egui::Ui, id: usize, trigger: &mut Trigger) {
    let quantity = trigger_quantity(*trigger);
    let kinds = [
        ("snd-trigger-loop", Trigger::Loop),
        (
            "snd-trigger-rises",
            Trigger::Rises {
                quantity,
                threshold: 1.0,
            },
        ),
        (
            "snd-trigger-falls",
            Trigger::Falls {
                quantity,
                threshold: 1.0,
            },
        ),
        (
            "snd-trigger-every",
            Trigger::Every {
                quantity,
                interval: 30.0,
            },
        ),
    ];
    let current = trigger_key(*trigger);
    egui::ComboBox::from_id_salt(("trigger", id))
        .selected_text(t!(current))
        .width(COMBO_W)
        .show_ui(ui, |ui| {
            for (key, kind) in kinds {
                if ui.selectable_label(current == key, t!(key)).clicked() {
                    *trigger = kind;
                }
            }
        });
}

/// The rows a trigger needs on top of its kind — nothing for a loop.
fn trigger_params(ui: &mut egui::Ui, id: usize, trigger: &mut Trigger) {
    match trigger {
        Trigger::Loop => {}
        Trigger::Rises {
            quantity,
            threshold,
        }
        | Trigger::Falls {
            quantity,
            threshold,
        } => {
            row(ui, "snd-quantity", |ui| {
                quantity_combo(ui, ("trigger-q", id), quantity)
            });
            row(ui, "snd-threshold", |ui| {
                field(ui, threshold, 0.1, -10_000.0..=10_000.0, "");
            });
        }
        Trigger::Every { quantity, interval } => {
            row(ui, "snd-quantity", |ui| {
                quantity_combo(ui, ("trigger-q", id), quantity)
            });
            row(ui, "snd-interval", |ui| {
                field(ui, interval, 0.5, 0.01..=10_000.0, "");
            });
        }
    }
}

fn trigger_key(trigger: Trigger) -> &'static str {
    match trigger {
        Trigger::Loop => "snd-trigger-loop",
        Trigger::Rises { .. } => "snd-trigger-rises",
        Trigger::Falls { .. } => "snd-trigger-falls",
        Trigger::Every { .. } => "snd-trigger-every",
    }
}

fn trigger_quantity(trigger: Trigger) -> Quantity {
    match trigger {
        Trigger::Loop => Quantity::Speed,
        Trigger::Rises { quantity, .. }
        | Trigger::Falls { quantity, .. }
        | Trigger::Every { quantity, .. } => quantity,
    }
}

/// State predicates. All of them have to hold — brake squeal is a speed window and a brake
/// force threshold on a loop, not an event.
fn conditions(ui: &mut egui::Ui, id: usize, list: &mut Vec<Condition>) {
    editor_ui::subheading(ui, t!("snd-conditions"));
    let mut remove = None;
    egui::Grid::new(format!("snd-cond-{id}"))
        .num_columns(4)
        .spacing(egui::vec2(space::S, 6.0))
        .show(ui, |ui| {
            for (i, condition) in list.iter_mut().enumerate() {
                quantity_combo(ui, ("cond", id, i), &mut condition.quantity);
                field(ui, &mut condition.min, 0.1, -10_000.0..=10_000.0, "")
                    .on_hover_text(t!("snd-min"));
                field(ui, &mut condition.max, 0.1, -10_000.0..=10_000.0, "")
                    .on_hover_text(t!("snd-max"));
                if ui.small_button("×").clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
    if let Some(i) = remove {
        list.remove(i);
    }
    if ui
        .small_button(t!("action-add-condition"))
        .on_hover_text(t!("snd-conditions-hint"))
        .clicked()
    {
        list.push(Condition {
            quantity: Quantity::Speed,
            min: 0.0,
            max: 200.0,
        });
    }
}

/// One dependency: quantity, support points, and the curve they make.
fn curve(
    ui: &mut egui::Ui,
    id: (&str, usize),
    title: String,
    slot: &mut Option<Curve>,
    range: std::ops::RangeInclusive<f64>,
) {
    editor_ui::subheading(ui, title.clone());
    let mut present = slot.is_some();
    if ui
        .checkbox(&mut present, t!("snd-curve-follows"))
        .on_hover_text(t!("snd-curve-follows-hint"))
        .changed()
    {
        // A fresh curve is a ramp over speed: the commonest dependency there is, and one
        // whose shape is visible in the sparkline straight away.
        let top = range.end().min(1.0);
        *slot = present.then(|| Curve::ramp(Quantity::Speed, 0.0, *range.start(), 100.0, top));
    }
    let Some(curve) = slot else {
        return;
    };
    quantity_combo(ui, ("curve", id), &mut curve.quantity);
    editor_ui::curve_editor(
        ui,
        &editor_ui::CurveSpec {
            id: egui::Id::new(("snd-curve", id)),
            title,
            x_unit: "",
            y_unit: "",
            x_speed: 0.5,
            y_speed: 0.01,
            x_range: -10_000.0..=10_000.0,
            y_range: range,
        },
        &mut curve.points,
    );
}
