//! Editor panels for brake equipment and drive (plan ch. 7, 8).
//!
//! Everything the simulation needs is a field of the data sheet, so everything here is a
//! form: control valve, friction pairing, reservoir volumes on one side, motor, engine map
//! and torque converters on the other.

use crate::ui::row;
use bevy_egui::egui;
use i18n::t;
use sim_core::brakes::{BrakeKind, BrakePosition, BrakeSpec, ControlValve, SlipProtection};
use sim_core::drive::{
    Circuit, CircuitKind, DieselEngine, DynamicBrake, Governor, HydrodynamicBrake, MAX_CIRCUITS,
    SeriesMotor, TractionSpec, Transmission,
};

fn drag(ui: &mut egui::Ui, value: &mut f64, speed: f64, range: std::ops::RangeInclusive<f64>) {
    ui.add(egui::DragValue::new(value).speed(speed).range(range));
}

/// Brake equipment.
pub fn brake_panel(ui: &mut egui::Ui, brake: &mut BrakeSpec, slip: &mut SlipProtection) {
    ui.label(egui::RichText::new(t!("group-brake")).strong());
    egui::Grid::new("brake").num_columns(2).show(ui, |ui| {
        row(ui, "brk-valve", |ui| {
            valve_combo(ui, &mut brake.valve);
        });
        row(ui, "brk-position", |ui| {
            position_combo(ui, &mut brake.position);
        });
        row(ui, "brk-friction", |ui| {
            kind_combo(ui, &mut brake.kind);
        });
        row(ui, "brk-weight", |ui| {
            drag(ui, &mut brake.brake_weight, 0.5, 0.0..=200.0);
        });
        row(ui, "brk-force", |ui| {
            ui.horizontal(|ui| {
                drag(ui, &mut brake.max_force, 500.0, 0.0..=1_000_000.0);
                if ui
                    .button(t!("action-suggest"))
                    .on_hover_text(t!("brk-force-suggest-hint"))
                    .clicked()
                {
                    brake.max_force =
                        BrakeSpec::from_brake_weight(brake.brake_weight, brake.kind.clone())
                            .max_force;
                }
            });
        });
        row(ui, "brk-cylinder", |ui| {
            drag(ui, &mut brake.max_cylinder, 0.05, 0.5..=6.0);
        });
        row(ui, "brk-cyl-reservoir", |ui| {
            drag(ui, &mut brake.cylinder_to_reservoir, 0.01, 0.05..=1.0);
        });
    });

    ui.separator();
    ui.label(egui::RichText::new(t!("group-additional-brakes")).strong());
    ui.checkbox(&mut brake.has_mg, t!("brk-mg"));
    if brake.has_mg {
        ui.horizontal(|ui| {
            ui.label(t!("label-force"));
            drag(ui, &mut brake.mg_force, 500.0, 0.0..=400_000.0);
            ui.label("N");
        });
    }
    ui.checkbox(&mut brake.has_direct, t!("brk-direct"));
    if brake.has_direct {
        ui.horizontal(|ui| {
            ui.label(t!("brk-cylinder"))
                .on_hover_text(t!("brk-direct-cylinder-hint"));
            drag(ui, &mut brake.direct_max_cylinder, 0.05, 0.0..=6.0);
        });
    }
    ui.horizontal(|ui| {
        ui.label(t!("brk-parking"));
        drag(ui, &mut brake.parking_force, 500.0, 0.0..=400_000.0);
        ui.label("N");
    });
    ui.checkbox(&mut brake.spring_parking, t!("brk-spring"))
        .on_hover_text(t!("brk-spring-hint"));
    ui.checkbox(&mut brake.pilot_controlled, t!("brk-pilot"))
        .on_hover_text(t!("brk-pilot-hint"));
    ui.checkbox(&mut brake.supplement_brake, t!("brk-supplement"))
        .on_hover_text(t!("brk-supplement-hint"));
    ui.checkbox(&mut brake.angleicher, t!("brk-angleicher"))
        .on_hover_text(t!("brk-angleicher-hint"));

    ui.separator();
    ui.label(egui::RichText::new(t!("group-air")).strong());
    egui::Grid::new("air").num_columns(2).show(ui, |ui| {
        row(ui, "air-aux", |ui| {
            drag(ui, &mut brake.aux_volume, 5.0, 10.0..=500.0);
        });
        row(ui, "air-pipe", |ui| {
            drag(ui, &mut brake.pipe_volume, 1.0, 1.0..=200.0);
        });
        row(ui, "air-main", |ui| {
            drag(ui, &mut brake.main_volume, 50.0, 0.0..=5_000.0);
        });
        row(ui, "air-compressor", |ui| {
            drag(ui, &mut brake.compressor_delivery, 50.0, 0.0..=6_000.0);
        });
        row(ui, "air-leakage", |ui| {
            drag(ui, &mut brake.leakage, 0.5, 0.0..=60.0);
        });
        row(ui, "brk-slip", |ui| {
            slip_combo(ui, slip);
        });
    });
}

/// Combo box over a fixed set of values; `label` supplies the text of each one.
fn combo<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    id: &str,
    value: &mut T,
    options: &[T],
    label: impl Fn(&T) -> String,
) {
    let selected = options
        .iter()
        .find(|v| *v == value)
        .map(&label)
        .unwrap_or_default();
    egui::ComboBox::from_id_salt(id)
        .selected_text(selected)
        .show_ui(ui, |ui| {
            for option in options {
                ui.selectable_value(value, *option, label(option));
            }
        });
}

/// Type designations of the equipment are names, not prose — they stay as they are.
fn valve_combo(ui: &mut egui::Ui, valve: &mut ControlValve) {
    let options = [
        ControlValve::KGp,
        ControlValve::KeGp,
        ControlValve::KeGpr,
        ControlValve::KeTm,
        ControlValve::KeL2a,
        ControlValve::KeL2d,
    ];
    combo(ui, "valve", valve, &options, |v| {
        match v {
            ControlValve::KGp => "K-GP",
            ControlValve::KeGp => "KE-GP",
            ControlValve::KeGpr => "KE-GPR",
            ControlValve::KeTm => "KE-Tm",
            ControlValve::KeL2a => "KE-L2a",
            ControlValve::KeL2d => "KE-L2d",
        }
        .into()
    });
}

fn position_combo(ui: &mut egui::Ui, position: &mut BrakePosition) {
    let options = [
        BrakePosition::G,
        BrakePosition::P,
        BrakePosition::R,
        BrakePosition::RMg,
    ];
    combo(ui, "position", position, &options, |p| {
        match p {
            BrakePosition::G => "G",
            BrakePosition::P => "P",
            BrakePosition::R => "R",
            BrakePosition::RMg => "R + Mg",
        }
        .into()
    });
}

fn friction_key(kind: &BrakeKind) -> &'static str {
    match kind {
        BrakeKind::Block => "friction-block",
        BrakeKind::Disc => "friction-disc",
        BrakeKind::CompositeK => "friction-k",
        BrakeKind::CompositeLl => "friction-ll",
        BrakeKind::Magnetic => "friction-magnetic",
        BrakeKind::Custom(_) => "friction-custom",
    }
}

fn kind_combo(ui: &mut egui::Ui, kind: &mut BrakeKind) {
    egui::ComboBox::from_id_salt("friction")
        .selected_text(t!(friction_key(kind)))
        .show_ui(ui, |ui| {
            for value in [
                BrakeKind::Block,
                BrakeKind::Disc,
                BrakeKind::CompositeK,
                BrakeKind::CompositeLl,
                BrakeKind::Magnetic,
            ] {
                if ui
                    .selectable_label(*kind == value, t!(friction_key(&value)))
                    .clicked()
                {
                    *kind = value;
                }
            }
            let is_custom = matches!(kind, BrakeKind::Custom(_));
            if ui
                .selectable_label(is_custom, t!("friction-custom"))
                .clicked()
                && !is_custom
            {
                *kind = BrakeKind::Custom(vec![(0.0, 0.35), (100.0, 0.25), (200.0, 0.18)]);
            }
        });
}

fn slip_combo(ui: &mut egui::Ui, slip: &mut SlipProtection) {
    let options = [
        SlipProtection::None,
        SlipProtection::SlipBrake,
        SlipProtection::TractionCutback,
        SlipProtection::CreepControl,
    ];
    combo(ui, "slip", slip, &options, |s| {
        t!(match s {
            SlipProtection::None => "slip-none",
            SlipProtection::SlipBrake => "slip-brake",
            SlipProtection::TractionCutback => "slip-cutback",
            SlipProtection::CreepControl => "slip-creep",
        })
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
    // Several tables can sit in one panel; the id keeps their buttons apart.
    if ui
        .push_id(id, |ui| ui.button(t!("action-add-point")))
        .inner
        .clicked()
    {
        let last = points.last().copied().unwrap_or((0.0, 0.0));
        points.push((last.0 + 20.0, last.1));
    }
}

/// Drive.
pub fn drive_panel(ui: &mut egui::Ui, traction: &mut Option<TractionSpec>) {
    ui.label(egui::RichText::new(t!("group-drive")).strong());
    type_combo(ui, traction);
    let Some(spec) = traction else {
        ui.small(t!("drive-unpowered-note"));
        return;
    };
    match spec {
        TractionSpec::Curve {
            force,
            v_max,
            brake,
            ramp_time,
        } => {
            ui.small(t!("curve-note"));
            egui::Grid::new("curve").num_columns(2).show(ui, |ui| {
                row(ui, "drv-vmax", |ui| drag(ui, v_max, 1.0, 0.0..=400.0));
                row(ui, "drv-ramp", |ui| drag(ui, ramp_time, 0.1, 0.1..=30.0));
            });
            ui.label(t!("table-tractive-effort"));
            table_editor(ui, "traction", " km/h", force);
            ui.label(t!("table-dynamic-brake"));
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
                row(ui, "tap-steps", |ui| {
                    ui.add(egui::DragValue::new(steps).range(1..=64));
                });
                row(ui, "tap-step-time", |ui| {
                    drag(ui, step_time, 0.05, 0.05..=5.0)
                });
                row(ui, "drv-start-force", |ui| {
                    drag(ui, max_force, 1000.0, 0.0..=800_000.0)
                });
                row(ui, "drv-power", |ui| {
                    drag(ui, max_power, 10_000.0, 0.0..=12_000_000.0)
                });
                row(ui, "drv-vmax", |ui| drag(ui, v_max, 1.0, 0.0..=400.0));
            });
            optional(
                ui,
                "section-series-motor",
                motor,
                series_motor_defaults,
                series_motor_editor,
            );
            optional(
                ui,
                "section-rheostatic-brake",
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
                row(ui, "drv-start-force", |ui| {
                    drag(ui, max_force, 1000.0, 0.0..=800_000.0)
                });
                row(ui, "drv-power", |ui| {
                    drag(ui, max_power, 10_000.0, 0.0..=12_000_000.0)
                });
                row(ui, "drv-vmax", |ui| drag(ui, v_max, 1.0, 0.0..=400.0));
                row(ui, "drv-pullout", |ui| {
                    drag(ui, v_pullout, 1.0, 0.0..=400.0)
                });
                row(ui, "drv-ramp", |ui| drag(ui, ramp_time, 0.1, 0.1..=30.0));
                row(ui, "drv-brake-force", |ui| {
                    drag(ui, brake_force, 1000.0, 0.0..=800_000.0)
                });
                row(ui, "drv-brake-power", |ui| {
                    drag(ui, brake_power, 10_000.0, 0.0..=12_000_000.0)
                });
                row(ui, "drv-brake-fade", |ui| {
                    drag(ui, brake_fade_kmh, 1.0, 0.0..=60.0)
                });
            });
            ui.checkbox(regenerative, t!("drv-regenerative"))
                .on_hover_text(t!("drv-regenerative-hint"));
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
                row(ui, "drv-start-force-diesel", |ui| {
                    drag(ui, max_force, 1000.0, 0.0..=800_000.0)
                });
                row(ui, "drv-power", |ui| {
                    drag(ui, max_power, 10_000.0, 0.0..=6_000_000.0)
                });
                row(ui, "drv-vmax", |ui| drag(ui, v_max, 1.0, 0.0..=250.0));
                row(ui, "drv-ramp", |ui| drag(ui, ramp_time, 0.1, 0.1..=30.0));
                row(ui, "drv-crank-time", |ui| {
                    drag(ui, start_time, 0.5, 0.5..=60.0)
                });
            });
            optional(
                ui,
                "section-engine-map",
                engine,
                engine_defaults,
                engine_editor,
            );
            optional(
                ui,
                "section-transmission",
                transmission,
                transmission_defaults,
                transmission_editor,
            );
            optional(
                ui,
                "section-retarder",
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
    key: &str,
    value: &mut Option<T>,
    defaults: impl FnOnce() -> T,
    body: impl FnOnce(&mut egui::Ui, &mut T),
) {
    ui.separator();
    let mut on = value.is_some();
    if ui.checkbox(&mut on, t!(key)).changed() {
        *value = on.then(defaults);
    }
    if let Some(inner) = value {
        body(ui, inner);
    }
}

fn traction_key(traction: &Option<TractionSpec>) -> &'static str {
    match traction {
        None => "traction-none",
        Some(TractionSpec::Curve { .. }) => "traction-curve",
        Some(TractionSpec::TapChanger { .. }) => "traction-tap",
        Some(TractionSpec::Converter { .. }) => "traction-converter",
        Some(TractionSpec::Diesel { .. }) => "traction-diesel",
    }
}

fn type_combo(ui: &mut egui::Ui, traction: &mut Option<TractionSpec>) {
    egui::ComboBox::from_id_salt("traction")
        .selected_text(t!(traction_key(traction)))
        .width(220.0)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(traction.is_none(), t!("traction-none"))
                .clicked()
            {
                *traction = None;
            }
            if ui
                .selectable_label(
                    matches!(traction, Some(TractionSpec::Curve { .. })),
                    t!("traction-curve"),
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
                    t!("traction-tap"),
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
                    t!("traction-converter"),
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
                    t!("traction-diesel"),
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
        row(ui, "mot-count", |ui| {
            ui.add(egui::DragValue::new(&mut m.count).range(1..=12));
        });
        row(ui, "mot-resistance", |ui| {
            drag(ui, &mut m.resistance, 0.005, 0.001..=2.0)
        });
        row(ui, "mot-machine-constant", |ui| {
            drag(ui, &mut m.flux_constant, 0.001, 0.0001..=1.0)
        });
        row(ui, "mot-saturation", |ui| {
            drag(ui, &mut m.saturation_current, 10.0, 10.0..=5_000.0)
        });
        row(ui, "mot-max-current", |ui| {
            drag(ui, &mut m.max_current, 10.0, 10.0..=5_000.0)
        });
        row(ui, "mot-max-voltage", |ui| {
            drag(ui, &mut m.max_voltage, 10.0, 50.0..=4_000.0)
        });
        row(ui, "mot-gear-ratio", |ui| {
            drag(ui, &mut m.gear_ratio, 0.01, 0.5..=10.0)
        });
        row(ui, "drv-wheel-diameter", |ui| {
            drag(ui, &mut m.wheel_diameter, 0.01, 0.3..=2.0)
        });
        row(ui, "mot-efficiency", |ui| {
            drag(ui, &mut m.efficiency, 0.01, 0.3..=1.0)
        });
    });
    ui.label(t!("mot-field-steps"));
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
    if ui.button(t!("action-add-stage")).clicked() {
        let last = m.field_steps.last().copied().unwrap_or(1.0);
        m.field_steps.push((last - 0.15).max(0.2));
    }
}

fn dynamic_brake_editor(ui: &mut egui::Ui, b: &mut DynamicBrake) {
    egui::Grid::new("dynbrake").num_columns(2).show(ui, |ui| {
        row(ui, "drv-brake-force", |ui| {
            drag(ui, &mut b.max_force, 1000.0, 0.0..=800_000.0)
        });
        row(ui, "drv-brake-power", |ui| {
            drag(ui, &mut b.max_power, 10_000.0, 0.0..=12_000_000.0)
        });
        row(ui, "drv-fade", |ui| {
            drag(ui, &mut b.fade_out_kmh, 1.0, 0.0..=60.0)
        });
        row(ui, "drv-ramp", |ui| {
            drag(ui, &mut b.ramp_time, 0.1, 0.1..=30.0)
        });
    });
    ui.checkbox(&mut b.regenerative, t!("drv-regenerative"));
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
        row(ui, "eng-idle", |ui| {
            drag(ui, &mut e.idle_rpm, 10.0, 100.0..=1_500.0)
        });
        row(ui, "eng-rated", |ui| {
            drag(ui, &mut e.rated_rpm, 10.0, 200.0..=4_000.0)
        });
        row(ui, "eng-overspeed", |ui| {
            drag(ui, &mut e.max_rpm, 10.0, 200.0..=4_500.0)
        });
        row(ui, "eng-inertia", |ui| {
            drag(ui, &mut e.inertia, 1.0, 0.5..=500.0)
        });
        row(ui, "eng-rack-time", |ui| {
            drag(ui, &mut e.response_time, 0.05, 0.05..=10.0)
        });
    });
    // Governor: speed-governed vehicles set an engine speed, fill-governed ones the rack.
    let speed_governed = matches!(e.governor, Governor::Speed { .. });
    ui.horizontal(|ui| {
        if ui
            .selectable_label(speed_governed, t!("gov-speed"))
            .on_hover_text(t!("gov-speed-hint"))
            .clicked()
        {
            e.governor = Governor::Speed { steps: 0 };
        }
        if ui
            .selectable_label(!speed_governed, t!("gov-fill"))
            .on_hover_text(t!("gov-fill-hint"))
            .clicked()
        {
            e.governor = Governor::Fill;
        }
    });
    if let Governor::Speed { steps } = &mut e.governor {
        ui.horizontal(|ui| {
            ui.label(t!("gov-notches"))
                .on_hover_text(t!("gov-notches-hint"));
            ui.add(egui::DragValue::new(steps).range(0..=32));
        });
    }
    ui.label(t!("table-torque"));
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
        row(ui, "trm-fill-steps", |ui| {
            ui.add(egui::DragValue::new(&mut t.fill_steps).range(0..=32));
        });
        row(ui, "trm-fill-time", |ui| {
            drag(ui, &mut t.fill_time, 0.05, 0.05..=10.0)
        });
        row(ui, "trm-hysteresis", |ui| {
            drag(ui, &mut t.hysteresis_kmh, 0.5, 0.0..=40.0)
        });
        row(ui, "trm-final-ratio", |ui| {
            drag(ui, &mut t.final_ratio, 0.01, 0.1..=10.0)
        });
        row(ui, "drv-wheel-diameter", |ui| {
            drag(ui, &mut t.wheel_diameter, 0.01, 0.3..=2.0)
        });
        row(ui, "trm-count", |ui| {
            ui.add(egui::DragValue::new(&mut t.count).range(1..=4));
        });
        row(ui, "trm-efficiency", |ui| {
            drag(ui, &mut t.efficiency, 0.01, 0.3..=1.0)
        });
    });

    ui.label(egui::RichText::new(t!("group-circuits")).strong());
    let mut remove = None;
    for (i, circuit) in t.circuits.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let converter = circuit.kind == CircuitKind::Converter;
            ui.label(format!("{}.", i + 1));
            if ui
                .selectable_label(converter, t!("circuit-converter"))
                .clicked()
            {
                circuit.kind = CircuitKind::Converter;
            }
            if ui
                .selectable_label(!converter, t!("circuit-coupling"))
                .clicked()
            {
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
                row(ui, "cir-ratio", |ui| {
                    drag(ui, &mut circuit.ratio, 0.01, 0.1..=10.0)
                });
                row(ui, "cir-stall", |ui| {
                    drag(ui, &mut circuit.stall_ratio, 0.05, 1.0..=6.0)
                });
                row(ui, "cir-coupling-point", |ui| {
                    drag(ui, &mut circuit.coupling_nu, 0.01, 0.1..=1.0)
                });
                row(ui, "cir-absorption", |ui| {
                    drag(ui, &mut circuit.absorption, 0.005, 0.0001..=10.0)
                });
                row(ui, "cir-shift-up", |ui| {
                    drag(ui, &mut circuit.shift_up_kmh, 1.0, 0.0..=250.0)
                });
            });
    }
    if let Some(i) = remove
        && t.circuits.len() > 1
    {
        t.circuits.remove(i);
    }
    if t.circuits.len() < MAX_CIRCUITS && ui.button(t!("action-add-circuit")).clicked() {
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
        row(ui, "ret-absorption", |ui| {
            drag(ui, &mut b.absorption, 0.005, 0.0001..=10.0)
        });
        row(ui, "ret-ratio", |ui| {
            drag(ui, &mut b.ratio, 0.05, 0.1..=12.0)
        });
        row(ui, "drv-wheel-diameter", |ui| {
            drag(ui, &mut b.wheel_diameter, 0.01, 0.3..=2.0)
        });
        row(ui, "ret-brake-force", |ui| {
            drag(ui, &mut b.max_force, 1000.0, 0.0..=400_000.0)
        });
        row(ui, "ret-brake-power", |ui| {
            drag(ui, &mut b.max_power, 10_000.0, 0.0..=6_000_000.0)
        });
        row(ui, "ret-fill-time", |ui| {
            drag(ui, &mut b.fill_time, 0.05, 0.05..=10.0)
        });
        row(ui, "drv-fade", |ui| {
            drag(ui, &mut b.fade_out_kmh, 1.0, 0.0..=60.0)
        });
    });
}
