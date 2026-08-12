//! Editor panels for brake equipment and drive (plan ch. 7, 8).
//!
//! Everything the simulation needs is a field of the data sheet, so everything here is a
//! form: control valve, friction pairing, reservoir volumes on one side, motor, engine map
//! and torque converters on the other.

use bevy_egui::egui;
use sim_core::brakes::{BrakeKind, BrakePosition, BrakeSpec, ControlValve, SlipProtection};
use sim_core::drive::{
    Circuit, CircuitKind, DieselEngine, DynamicBrake, Governor, HydrodynamicBrake, MAX_CIRCUITS,
    SeriesMotor, TractionSpec, Transmission,
};

/// A labelled row with a tooltip.
fn row(ui: &mut egui::Ui, label: &str, hint: &str, widget: impl FnOnce(&mut egui::Ui)) {
    ui.label(label).on_hover_text(hint);
    widget(ui);
    ui.end_row();
}

fn drag(ui: &mut egui::Ui, value: &mut f64, speed: f64, range: std::ops::RangeInclusive<f64>) {
    ui.add(egui::DragValue::new(value).speed(speed).range(range));
}

/// Brake equipment.
pub fn brake_panel(ui: &mut egui::Ui, brake: &mut BrakeSpec, slip: &mut SlipProtection) {
    ui.label(egui::RichText::new("Brake").strong());
    egui::Grid::new("brake").num_columns(2).show(ui, |ui| {
        row(ui, "Control valve", "which control valve is fitted", |ui| {
            valve_combo(ui, &mut brake.valve);
        });
        row(
            ui,
            "Brake position",
            "G freight · P passenger · R rapid · R+Mg with magnetic track brake",
            |ui| {
                position_combo(ui, &mut brake.position);
            },
        );
        row(
            ui,
            "Friction pairing",
            "how the friction coefficient runs over speed",
            |ui| {
                kind_combo(ui, &mut brake.kind);
            },
        );
        row(
            ui,
            "Braked weight",
            "t — from the vehicle's anscriptions",
            |ui| {
                drag(ui, &mut brake.brake_weight, 0.5, 0.0..=200.0);
            },
        );
        row(
            ui,
            "Brake force",
            "N at full cylinder pressure and standstill",
            |ui| {
                ui.horizontal(|ui| {
                    drag(ui, &mut brake.max_force, 500.0, 0.0..=1_000_000.0);
                    if ui
                        .button("Suggest")
                        .on_hover_text("from the braked weight")
                        .clicked()
                    {
                        brake.max_force =
                            BrakeSpec::from_brake_weight(brake.brake_weight, brake.kind.clone())
                                .max_force;
                    }
                });
            },
        );
        row(ui, "Cylinder pressure", "bar at a full application", |ui| {
            drag(ui, &mut brake.max_cylinder, 0.05, 0.5..=6.0);
        });
        row(
            ui,
            "Cylinder / reservoir",
            "volume ratio — decides how quickly the brake exhausts itself",
            |ui| {
                drag(ui, &mut brake.cylinder_to_reservoir, 0.01, 0.05..=1.0);
            },
        );
    });

    ui.separator();
    ui.label(egui::RichText::new("Additional brakes").strong());
    ui.checkbox(&mut brake.has_mg, "Magnetic track brake");
    if brake.has_mg {
        ui.horizontal(|ui| {
            ui.label("Force");
            drag(ui, &mut brake.mg_force, 500.0, 0.0..=400_000.0);
            ui.label("N");
        });
    }
    ui.checkbox(&mut brake.has_direct, "Direct (additional) brake");
    if brake.has_direct {
        ui.horizontal(|ui| {
            ui.label("Cylinder pressure")
                .on_hover_text("bar; 0 = same as the automatic brake");
            drag(ui, &mut brake.direct_max_cylinder, 0.05, 0.0..=6.0);
        });
    }
    ui.horizontal(|ui| {
        ui.label("Parking brake");
        drag(ui, &mut brake.parking_force, 500.0, 0.0..=400_000.0);
        ui.label("N");
    });
    ui.checkbox(&mut brake.spring_parking, "Spring-applied (Federspeicher)")
        .on_hover_text("held off by air — applies by itself when the main reservoir runs empty");
    ui.checkbox(&mut brake.pilot_controlled, "Pre-controlled cylinder")
        .on_hover_text("relay valve fed from the main reservoir: fills faster, cannot exhaust");
    ui.checkbox(&mut brake.supplement_brake, "Air supplement brake")
        .on_hover_text("fills up whatever the dynamic brake falls short of");
    ui.checkbox(&mut brake.angleicher, "Equalising device (Angleicher)")
        .on_hover_text("makes up brake pipe leakage in lap position; without a memory");

    ui.separator();
    ui.label(egui::RichText::new("Air").strong());
    egui::Grid::new("air").num_columns(2).show(ui, |ui| {
        row(ui, "Auxiliary reservoir", "l", |ui| {
            drag(ui, &mut brake.aux_volume, 5.0, 10.0..=500.0);
        });
        row(ui, "Brake pipe", "l — this vehicle's share", |ui| {
            drag(ui, &mut brake.pipe_volume, 1.0, 1.0..=200.0);
        });
        row(ui, "Main reservoir", "l — 0 = none", |ui| {
            drag(ui, &mut brake.main_volume, 50.0, 0.0..=5_000.0);
        });
        row(ui, "Compressor", "l/min of free air — 0 = none", |ui| {
            drag(ui, &mut brake.compressor_delivery, 50.0, 0.0..=6_000.0);
        });
        row(ui, "Leakage", "l/min of free air", |ui| {
            drag(ui, &mut brake.leakage, 0.5, 0.0..=60.0);
        });
        row(
            ui,
            "Wheel slip protection",
            "how the vehicle answers a spinning or sliding wheelset",
            |ui| {
                slip_combo(ui, slip);
            },
        );
    });
}

fn valve_combo(ui: &mut egui::Ui, valve: &mut ControlValve) {
    let options = [
        (ControlValve::KGp, "K-GP"),
        (ControlValve::KeGp, "KE-GP"),
        (ControlValve::KeGpr, "KE-GPR"),
        (ControlValve::KeTm, "KE-Tm"),
        (ControlValve::KeL2a, "KE-L2a"),
        (ControlValve::KeL2d, "KE-L2d"),
    ];
    let label = options
        .iter()
        .find(|(v, _)| v == valve)
        .map(|(_, l)| *l)
        .unwrap_or("KE-GP");
    egui::ComboBox::from_id_salt("valve")
        .selected_text(label)
        .show_ui(ui, |ui| {
            for (value, text) in options {
                ui.selectable_value(valve, value, text);
            }
        });
}

fn position_combo(ui: &mut egui::Ui, position: &mut BrakePosition) {
    let options = [
        (BrakePosition::G, "G"),
        (BrakePosition::P, "P"),
        (BrakePosition::R, "R"),
        (BrakePosition::RMg, "R + Mg"),
    ];
    let label = options
        .iter()
        .find(|(v, _)| v == position)
        .map(|(_, l)| *l)
        .unwrap_or("P");
    egui::ComboBox::from_id_salt("position")
        .selected_text(label)
        .show_ui(ui, |ui| {
            for (value, text) in options {
                ui.selectable_value(position, value, text);
            }
        });
}

fn kind_combo(ui: &mut egui::Ui, kind: &mut BrakeKind) {
    let label = match kind {
        BrakeKind::Block => "Cast iron block",
        BrakeKind::Disc => "Disc",
        BrakeKind::CompositeK => "K block",
        BrakeKind::CompositeLl => "LL block",
        BrakeKind::Magnetic => "Magnetic rail",
        BrakeKind::Custom(_) => "Own characteristic",
    };
    egui::ComboBox::from_id_salt("friction")
        .selected_text(label)
        .show_ui(ui, |ui| {
            for value in [
                BrakeKind::Block,
                BrakeKind::Disc,
                BrakeKind::CompositeK,
                BrakeKind::CompositeLl,
                BrakeKind::Magnetic,
            ] {
                let text = match value {
                    BrakeKind::Block => "Cast iron block",
                    BrakeKind::Disc => "Disc",
                    BrakeKind::CompositeK => "K block",
                    BrakeKind::CompositeLl => "LL block",
                    _ => "Magnetic rail",
                };
                if ui.selectable_label(*kind == value, text).clicked() {
                    *kind = value;
                }
            }
            let is_custom = matches!(kind, BrakeKind::Custom(_));
            if ui
                .selectable_label(is_custom, "Own characteristic")
                .clicked()
                && !is_custom
            {
                *kind = BrakeKind::Custom(vec![(0.0, 0.35), (100.0, 0.25), (200.0, 0.18)]);
            }
        });
}

fn slip_combo(ui: &mut egui::Ui, slip: &mut SlipProtection) {
    let options = [
        (SlipProtection::None, "none"),
        (SlipProtection::SlipBrake, "wheel slip brake"),
        (SlipProtection::TractionCutback, "traction cutback"),
        (SlipProtection::CreepControl, "creep control"),
    ];
    let label = options
        .iter()
        .find(|(v, _)| v == slip)
        .map(|(_, l)| *l)
        .unwrap_or("none");
    egui::ComboBox::from_id_salt("slip")
        .selected_text(label)
        .show_ui(ui, |ui| {
            for (value, text) in options {
                ui.selectable_value(slip, value, text);
            }
        });
}

/// Points of a friction or tractive effort table, editable row by row.
fn table_editor(ui: &mut egui::Ui, id: &str, x_unit: &str, points: &mut Vec<(f64, f64)>) {
    let mut remove = None;
    for (i, (x, y)) in points.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(x).speed(1.0).suffix(x_unit));
            ui.add(egui::DragValue::new(y).speed(0.01));
            if ui.small_button("✕").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        points.remove(i);
    }
    if ui.button(format!("+ point ({id})")).clicked() {
        let last = points.last().copied().unwrap_or((0.0, 0.0));
        points.push((last.0 + 20.0, last.1));
    }
}

/// Drive.
pub fn drive_panel(ui: &mut egui::Ui, traction: &mut Option<TractionSpec>) {
    ui.label(egui::RichText::new("Drive").strong());
    type_combo(ui, traction);
    let Some(spec) = traction else {
        ui.small("Unpowered vehicle.");
        return;
    };
    match spec {
        TractionSpec::Curve {
            force,
            v_max,
            brake,
            ramp_time,
        } => {
            ui.small("Tractive effort straight off the diagram — no motor, no gearbox.");
            egui::Grid::new("curve").num_columns(2).show(ui, |ui| {
                row(ui, "v max", "km/h", |ui| drag(ui, v_max, 1.0, 0.0..=400.0));
                row(ui, "Rise time", "s from 0 to full effort", |ui| {
                    drag(ui, ramp_time, 0.1, 0.1..=30.0)
                });
            });
            ui.label("Tractive effort (km/h → N)");
            table_editor(ui, "traction", " km/h", force);
            ui.label("Dynamic brake (km/h → N)");
            table_editor(ui, "brake", " km/h", brake);
        }
        TractionSpec::TapChanger {
            steps,
            max_force,
            max_power,
            v_max,
            step_time,
            motor,
            dynamic_brake,
        } => {
            egui::Grid::new("tap").num_columns(2).show(ui, |ui| {
                row(ui, "Notches", "of the tap changer", |ui| {
                    ui.add(egui::DragValue::new(steps).range(1..=64));
                });
                row(ui, "Time per notch", "s", |ui| {
                    drag(ui, step_time, 0.05, 0.05..=5.0)
                });
                row(ui, "Starting effort", "N", |ui| {
                    drag(ui, max_force, 1000.0, 0.0..=800_000.0)
                });
                row(ui, "Power", "W at the wheel", |ui| {
                    drag(ui, max_power, 10_000.0, 0.0..=12_000_000.0)
                });
                row(ui, "v max", "km/h", |ui| drag(ui, v_max, 1.0, 0.0..=400.0));
            });
            optional(
                ui,
                "Series-wound motor data",
                motor,
                series_motor_defaults,
                series_motor_editor,
            );
            optional(
                ui,
                "Rheostatic brake",
                dynamic_brake,
                || DynamicBrake {
                    max_force: 100_000.0,
                    max_power: 1_500_000.0,
                    fade_out_kmh: 20.0,
                    regenerative: false,
                    ramp_time: 2.0,
                },
                dynamic_brake_editor,
            );
        }
        TractionSpec::Converter {
            max_force,
            max_power,
            v_max,
            brake_force,
            brake_power,
            ramp_time,
            v_pullout,
            regenerative,
            brake_fade_kmh,
        } => {
            egui::Grid::new("conv").num_columns(2).show(ui, |ui| {
                row(ui, "Starting effort", "N", |ui| {
                    drag(ui, max_force, 1000.0, 0.0..=800_000.0)
                });
                row(ui, "Power", "W at the wheel", |ui| {
                    drag(ui, max_power, 10_000.0, 0.0..=12_000_000.0)
                });
                row(ui, "v max", "km/h", |ui| drag(ui, v_max, 1.0, 0.0..=400.0));
                row(
                    ui,
                    "Pull-out speed",
                    "km/h — above it the effort falls with 1/v²; 0 = no limit",
                    |ui| drag(ui, v_pullout, 1.0, 0.0..=400.0),
                );
                row(ui, "Rise time", "s", |ui| {
                    drag(ui, ramp_time, 0.1, 0.1..=30.0)
                });
                row(ui, "Brake force", "N", |ui| {
                    drag(ui, brake_force, 1000.0, 0.0..=800_000.0)
                });
                row(ui, "Brake power", "W", |ui| {
                    drag(ui, brake_power, 10_000.0, 0.0..=12_000_000.0)
                });
                row(ui, "Brake fade-out", "km/h", |ui| {
                    drag(ui, brake_fade_kmh, 1.0, 0.0..=60.0)
                });
            });
            ui.checkbox(regenerative, "Regenerative")
                .on_hover_text("feeds back into the contact line — dead without line voltage");
        }
        TractionSpec::Diesel {
            max_force,
            max_power,
            v_max,
            ramp_time,
            start_time,
            engine,
            transmission,
            hydrodynamic_brake,
        } => {
            egui::Grid::new("diesel").num_columns(2).show(ui, |ui| {
                row(ui, "Starting effort", "N — without an engine map", |ui| {
                    drag(ui, max_force, 1000.0, 0.0..=800_000.0)
                });
                row(ui, "Power", "W at the wheel", |ui| {
                    drag(ui, max_power, 10_000.0, 0.0..=6_000_000.0)
                });
                row(ui, "v max", "km/h", |ui| drag(ui, v_max, 1.0, 0.0..=250.0));
                row(ui, "Rise time", "s", |ui| {
                    drag(ui, ramp_time, 0.1, 0.1..=30.0)
                });
                row(ui, "Cranking time", "s", |ui| {
                    drag(ui, start_time, 0.5, 0.5..=60.0)
                });
            });
            optional(ui, "Engine map", engine, engine_defaults, |ui, e| {
                engine_editor(ui, e)
            });
            optional(
                ui,
                "Hydraulic transmission",
                transmission,
                transmission_defaults,
                transmission_editor,
            );
            optional(
                ui,
                "Hydrodynamic brake",
                hydrodynamic_brake,
                || HydrodynamicBrake {
                    absorption: 0.05,
                    ratio: 4.0,
                    wheel_diameter: 1.0,
                    max_force: 80_000.0,
                    max_power: 1_000_000.0,
                    fill_time: 1.5,
                    fade_out_kmh: 15.0,
                },
                retarder_editor,
            );
        }
    }
}

/// A section that can be switched on; switching it on creates the defaults.
fn optional<T>(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut Option<T>,
    defaults: impl FnOnce() -> T,
    body: impl FnOnce(&mut egui::Ui, &mut T),
) {
    ui.separator();
    let mut on = value.is_some();
    if ui.checkbox(&mut on, label).changed() {
        *value = on.then(defaults);
    }
    if let Some(inner) = value {
        body(ui, inner);
    }
}

fn type_combo(ui: &mut egui::Ui, traction: &mut Option<TractionSpec>) {
    let label = match traction {
        None => "unpowered",
        Some(TractionSpec::Curve { .. }) => "tractive effort curve",
        Some(TractionSpec::TapChanger { .. }) => "tap changer (series-wound)",
        Some(TractionSpec::Converter { .. }) => "converter (three-phase)",
        Some(TractionSpec::Diesel { .. }) => "diesel",
    };
    egui::ComboBox::from_id_salt("traction")
        .selected_text(label)
        .width(220.0)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(traction.is_none(), "unpowered")
                .clicked()
            {
                *traction = None;
            }
            if ui
                .selectable_label(
                    matches!(traction, Some(TractionSpec::Curve { .. })),
                    "tractive effort curve",
                )
                .clicked()
            {
                *traction = Some(TractionSpec::Curve {
                    force: vec![(0.0, 200_000.0), (50.0, 120_000.0), (150.0, 40_000.0)],
                    v_max: 160.0,
                    brake: Vec::new(),
                    ramp_time: 2.0,
                });
            }
            if ui
                .selectable_label(
                    matches!(traction, Some(TractionSpec::TapChanger { .. })),
                    "tap changer (series-wound)",
                )
                .clicked()
            {
                *traction = Some(TractionSpec::TapChanger {
                    steps: 28,
                    max_force: 275_000.0,
                    max_power: 3_620_000.0,
                    v_max: 150.0,
                    step_time: 0.8,
                    motor: None,
                    dynamic_brake: None,
                });
            }
            if ui
                .selectable_label(
                    matches!(traction, Some(TractionSpec::Converter { .. })),
                    "converter (three-phase)",
                )
                .clicked()
            {
                *traction = Some(TractionSpec::Converter {
                    max_force: 300_000.0,
                    max_power: 6_400_000.0,
                    v_max: 220.0,
                    brake_force: 150_000.0,
                    brake_power: 2_600_000.0,
                    ramp_time: 2.5,
                    v_pullout: 150.0,
                    regenerative: true,
                    brake_fade_kmh: 10.0,
                });
            }
            if ui
                .selectable_label(
                    matches!(traction, Some(TractionSpec::Diesel { .. })),
                    "diesel",
                )
                .clicked()
            {
                *traction = Some(TractionSpec::Diesel {
                    max_force: 235_000.0,
                    max_power: 1_840_000.0,
                    v_max: 140.0,
                    ramp_time: 4.0,
                    start_time: 8.0,
                    engine: None,
                    transmission: None,
                    hydrodynamic_brake: None,
                });
            }
        });
}

fn series_motor_defaults() -> SeriesMotor {
    SeriesMotor {
        count: 4,
        resistance: 0.05,
        flux_constant: 0.0289,
        saturation_current: 600.0,
        max_current: 1600.0,
        max_voltage: 1000.0,
        field_steps: vec![1.0, 0.85, 0.7],
        gear_ratio: 2.17,
        wheel_diameter: 1.25,
        efficiency: 0.95,
    }
}

fn series_motor_editor(ui: &mut egui::Ui, m: &mut SeriesMotor) {
    egui::Grid::new("motor").num_columns(2).show(ui, |ui| {
        row(ui, "Motors", "number in the vehicle", |ui| {
            ui.add(egui::DragValue::new(&mut m.count).range(1..=12));
        });
        row(
            ui,
            "Resistance",
            "Ω — armature and field together",
            |ui| drag(ui, &mut m.resistance, 0.005, 0.001..=2.0),
        );
        row(
            ui,
            "Machine constant",
            "V·s/A — flux linkage per ampere, unsaturated",
            |ui| drag(ui, &mut m.flux_constant, 0.001, 0.0001..=1.0),
        );
        row(ui, "Saturation current", "A", |ui| {
            drag(ui, &mut m.saturation_current, 10.0, 10.0..=5_000.0)
        });
        row(ui, "Max current", "A — the current limit relay", |ui| {
            drag(ui, &mut m.max_current, 10.0, 10.0..=5_000.0)
        });
        row(ui, "Max voltage", "V at the top notch", |ui| {
            drag(ui, &mut m.max_voltage, 10.0, 50.0..=4_000.0)
        });
        row(ui, "Gear ratio", "motor : wheelset", |ui| {
            drag(ui, &mut m.gear_ratio, 0.01, 0.5..=10.0)
        });
        row(ui, "Wheel diameter", "m", |ui| {
            drag(ui, &mut m.wheel_diameter, 0.01, 0.3..=2.0)
        });
        row(ui, "Efficiency", "motor and gearing", |ui| {
            drag(ui, &mut m.efficiency, 0.01, 0.3..=1.0)
        });
    });
    ui.label("Field weakening stages (1 = full field)");
    let mut remove = None;
    for (i, field) in m.field_steps.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(field).speed(0.01).range(0.2..=1.0));
            if ui.small_button("✕").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        m.field_steps.remove(i);
    }
    if ui.button("+ stage").clicked() {
        let last = m.field_steps.last().copied().unwrap_or(1.0);
        m.field_steps.push((last - 0.15).max(0.2));
    }
}

fn dynamic_brake_editor(ui: &mut egui::Ui, b: &mut DynamicBrake) {
    egui::Grid::new("dynbrake").num_columns(2).show(ui, |ui| {
        row(ui, "Brake force", "N", |ui| {
            drag(ui, &mut b.max_force, 1000.0, 0.0..=800_000.0)
        });
        row(ui, "Brake power", "W", |ui| {
            drag(ui, &mut b.max_power, 10_000.0, 0.0..=12_000_000.0)
        });
        row(ui, "Fade-out", "km/h", |ui| {
            drag(ui, &mut b.fade_out_kmh, 1.0, 0.0..=60.0)
        });
        row(ui, "Rise time", "s", |ui| {
            drag(ui, &mut b.ramp_time, 0.1, 0.1..=30.0)
        });
    });
    ui.checkbox(&mut b.regenerative, "Regenerative");
}

fn engine_defaults() -> DieselEngine {
    DieselEngine {
        idle_rpm: 600.0,
        rated_rpm: 1500.0,
        max_rpm: 1650.0,
        torque_curve: vec![(600.0, 9_000.0), (1000.0, 13_500.0), (1500.0, 13_115.0)],
        governor: Governor::Speed { steps: 0 },
        inertia: 60.0,
        response_time: 1.0,
    }
}

fn engine_editor(ui: &mut egui::Ui, e: &mut DieselEngine) {
    egui::Grid::new("engine").num_columns(2).show(ui, |ui| {
        row(ui, "Idle", "1/min", |ui| {
            drag(ui, &mut e.idle_rpm, 10.0, 100.0..=1_500.0)
        });
        row(ui, "Rated speed", "1/min", |ui| {
            drag(ui, &mut e.rated_rpm, 10.0, 200.0..=4_000.0)
        });
        row(ui, "Overspeed", "1/min", |ui| {
            drag(ui, &mut e.max_rpm, 10.0, 200.0..=4_500.0)
        });
        row(ui, "Inertia", "kg·m² incl. flywheel", |ui| {
            drag(ui, &mut e.inertia, 1.0, 0.5..=500.0)
        });
        row(ui, "Rack travel time", "s from idle to full load", |ui| {
            drag(ui, &mut e.response_time, 0.05, 0.05..=10.0)
        });
    });
    // Governor: speed-governed vehicles set an engine speed, fill-governed ones the rack.
    let speed_governed = matches!(e.governor, Governor::Speed { .. });
    ui.horizontal(|ui| {
        if ui
            .selectable_label(speed_governed, "speed-governed")
            .on_hover_text("the power controller sets the engine speed, the governor holds it")
            .clicked()
        {
            e.governor = Governor::Speed { steps: 0 };
        }
        if ui
            .selectable_label(!speed_governed, "fill-governed")
            .on_hover_text("the power controller is the fuel rack, the speed follows the load")
            .clicked()
        {
            e.governor = Governor::Fill;
        }
    });
    if let Governor::Speed { steps } = &mut e.governor {
        ui.horizontal(|ui| {
            ui.label("Notches").on_hover_text("0 = continuous");
            ui.add(egui::DragValue::new(steps).range(0..=32));
        });
    }
    ui.label("Full load torque (1/min → N·m)");
    table_editor(ui, "torque", " rpm", &mut e.torque_curve);
}

fn transmission_defaults() -> Transmission {
    Transmission {
        circuits: vec![Circuit {
            kind: CircuitKind::Converter,
            ratio: 3.9,
            stall_ratio: 2.4,
            coupling_nu: 0.85,
            absorption: 0.53,
            shift_up_kmh: 70.0,
        }],
        fill_steps: 0,
        fill_time: 1.2,
        hysteresis_kmh: 10.0,
        final_ratio: 1.0,
        wheel_diameter: 1.0,
        count: 1,
        efficiency: 0.95,
    }
}

fn transmission_editor(ui: &mut egui::Ui, t: &mut Transmission) {
    egui::Grid::new("gearbox").num_columns(2).show(ui, |ui| {
        row(
            ui,
            "Filling steps",
            "0 = continuous, 1 = fill/empty only, higher = partial filling to the original",
            |ui| {
                ui.add(egui::DragValue::new(&mut t.fill_steps).range(0..=32));
            },
        );
        row(ui, "Filling time", "s to fill or empty a circuit", |ui| {
            drag(ui, &mut t.fill_time, 0.05, 0.05..=10.0)
        });
        row(
            ui,
            "Change hysteresis",
            "km/h below the change-up point at which it changes back",
            |ui| drag(ui, &mut t.hysteresis_kmh, 0.5, 0.0..=40.0),
        );
        row(ui, "Final drive", "output : wheelset", |ui| {
            drag(ui, &mut t.final_ratio, 0.01, 0.1..=10.0)
        });
        row(ui, "Wheel diameter", "m", |ui| {
            drag(ui, &mut t.wheel_diameter, 0.01, 0.3..=2.0)
        });
        row(ui, "Transmissions", "number in the vehicle", |ui| {
            ui.add(egui::DragValue::new(&mut t.count).range(1..=4));
        });
        row(ui, "Efficiency", "gearing behind the circuit", |ui| {
            drag(ui, &mut t.efficiency, 0.01, 0.3..=1.0)
        });
    });

    ui.label(egui::RichText::new("Circuits").strong());
    let mut remove = None;
    for (i, circuit) in t.circuits.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let converter = circuit.kind == CircuitKind::Converter;
            ui.label(format!("{}.", i + 1));
            if ui.selectable_label(converter, "converter").clicked() {
                circuit.kind = CircuitKind::Converter;
            }
            if ui.selectable_label(!converter, "coupling").clicked() {
                circuit.kind = CircuitKind::Coupling;
                circuit.stall_ratio = 1.0;
            }
            if ui.small_button("✕").clicked() {
                remove = Some(i);
            }
        });
        egui::Grid::new(("circuit", i))
            .num_columns(2)
            .show(ui, |ui| {
                row(ui, "Ratio", "turbine : output", |ui| {
                    drag(ui, &mut circuit.ratio, 0.01, 0.1..=10.0)
                });
                row(ui, "Stall torque ratio", "µ at ν = 0", |ui| {
                    drag(ui, &mut circuit.stall_ratio, 0.05, 1.0..=6.0)
                });
                row(ui, "Coupling point", "ν at which µ has reached 1", |ui| {
                    drag(ui, &mut circuit.coupling_nu, 0.01, 0.1..=1.0)
                });
                row(
                    ui,
                    "Absorption λ",
                    "N·m/(rad/s)² — the pump's rated torque at rated speed",
                    |ui| drag(ui, &mut circuit.absorption, 0.005, 0.0001..=10.0),
                );
                row(
                    ui,
                    "Change-up point",
                    "km/h — the last circuit ignores it",
                    |ui| drag(ui, &mut circuit.shift_up_kmh, 1.0, 0.0..=250.0),
                );
            });
    }
    if let Some(i) = remove
        && t.circuits.len() > 1
    {
        t.circuits.remove(i);
    }
    if t.circuits.len() < MAX_CIRCUITS && ui.button("+ circuit").clicked() {
        let last = *t.circuits.last().unwrap();
        t.circuits.push(Circuit {
            ratio: last.ratio / 2.0,
            shift_up_kmh: 0.0,
            ..last
        });
    }
}

fn retarder_editor(ui: &mut egui::Ui, b: &mut HydrodynamicBrake) {
    egui::Grid::new("retarder").num_columns(2).show(ui, |ui| {
        row(ui, "Absorption λ", "N·m/(rad/s)² at full filling", |ui| {
            drag(ui, &mut b.absorption, 0.005, 0.0001..=10.0)
        });
        row(ui, "Ratio", "rotor : wheelset", |ui| {
            drag(ui, &mut b.ratio, 0.05, 0.1..=12.0)
        });
        row(ui, "Wheel diameter", "m", |ui| {
            drag(ui, &mut b.wheel_diameter, 0.01, 0.3..=2.0)
        });
        row(ui, "Brake force", "N — mechanical limit", |ui| {
            drag(ui, &mut b.max_force, 1000.0, 0.0..=400_000.0)
        });
        row(
            ui,
            "Brake power",
            "W — what the cooler can carry off",
            |ui| drag(ui, &mut b.max_power, 10_000.0, 0.0..=6_000_000.0),
        );
        row(ui, "Filling time", "s", |ui| {
            drag(ui, &mut b.fill_time, 0.05, 0.05..=10.0)
        });
        row(ui, "Fade-out", "km/h", |ui| {
            drag(ui, &mut b.fade_out_kmh, 1.0, 0.0..=60.0)
        });
    });
}
