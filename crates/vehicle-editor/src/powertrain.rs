//! Editor panels for brake equipment and drive (plan ch. 7, 8).
//!
//! Everything the simulation needs is a field of the data sheet, so everything here is a
//! form: control valve, friction pairing, reservoir volumes on one side, motor, engine map
//! and torque converters on the other. Units sit on the fields themselves; the tooltip
//! (`<key>-hint`) explains where the number comes from.

use crate::ui::row;
use bevy_egui::egui;
use editor_ui::{colors, field, form_grid, form_label, space, subheading};
use i18n::t;
use sim_core::brakes::{
    BrakeKind, BrakePosition, BrakeSpec, ControlValve, LoadBraking, SlipProtection,
};
use sim_core::drive::{
    Circuit, CircuitKind, DieselEngine, DynamicBrake, Governor, HydrodynamicBrake, MAX_CIRCUITS,
    SeriesMotor, TractionSpec, Transmission,
};

/// Brake equipment.
pub fn brake_panel(
    ui: &mut egui::Ui,
    brake: &mut BrakeSpec,
    slip: &mut SlipProtection,
    v_max: f64,
    axle_load_t: f64,
) {
    form_grid("brake").show(ui, |ui| {
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
            field(ui, &mut brake.brake_weight, 0.5, 0.0..=200.0, "t");
        });
        row(ui, "brk-load", |ui| {
            load_combo(ui, &mut brake.load_braking);
        });
        // The weighing valve reads tare mass and payload off the data sheet; only the
        // changeover has figures of its own, and they are on the wagon.
        if let LoadBraking::Changeover {
            empty_share,
            changeover_mass_t,
        } = &mut brake.load_braking
        {
            row(ui, "brk-load-empty", |ui| {
                field(ui, empty_share, 0.01, 0.05..=1.0, "");
            });
            row(ui, "brk-load-mass", |ui| {
                field(ui, changeover_mass_t, 1.0, 0.0..=200.0, "t");
            });
        }
        row(ui, "brk-force", |ui| {
            field(ui, &mut brake.max_force, 500.0, 0.0..=1_000_000.0, "N");
            let suggestion =
                BrakeSpec::from_brake_weight(brake.brake_weight, brake.kind.clone()).max_force;
            if ui
                .button(t!("action-suggest"))
                .on_hover_text(t!(
                    "brk-force-suggest-hint",
                    value = editor_ui::group_digits(suggestion)
                ))
                .clicked()
            {
                brake.max_force = suggestion;
            }
        });
        row(ui, "brk-cylinder", |ui| {
            field(ui, &mut brake.max_cylinder, 0.05, 0.5..=6.0, "bar");
        });
        row(ui, "brk-cyl-reservoir", |ui| {
            field(ui, &mut brake.cylinder_to_reservoir, 0.01, 0.05..=1.0, "");
        });
        row(ui, "brk-slip", |ui| {
            slip_combo(ui, slip);
        });
    });

    // A closed curve per pairing; the combo names it but shows nothing of how
    // it falls off. Drawn at this vehicle's own axle load, which picks the curve
    // out of the family. `Custom` already plots its own points just below.
    if !matches!(brake.kind, BrakeKind::Custom(_)) {
        subheading(ui, t!("brk-friction-plot"));
        editor_ui::sparkline_fn(ui, v_max, "km/h", "", |kmh| {
            brake.kind.friction_factor_at(kmh, axle_load_t)
        });
    }

    if let BrakeKind::Custom(points) = &mut brake.kind {
        // Selectable in the combo since forever, but nothing ever drew its
        // points — the curve was set once and then out of reach, and a
        // hand-written one was invisible.
        table_editor(
            ui,
            "friction",
            t!("brk-friction-points"),
            "km/h",
            "",
            0.01,
            0.0..=1.0,
            points,
        );
    }

    subheading(ui, t!("group-additional-brakes"));
    ui.checkbox(&mut brake.has_mg, t!("brk-mg"));
    if brake.has_mg {
        form_grid("mg").show(ui, |ui| {
            form_label(ui, t!("label-force"));
            field(ui, &mut brake.mg_force, 500.0, 0.0..=400_000.0, "N");
            ui.end_row();
        });
    }
    ui.checkbox(&mut brake.has_direct, t!("brk-direct"));
    if brake.has_direct {
        form_grid("direct").show(ui, |ui| {
            form_label(ui, t!("brk-cylinder")).on_hover_text(t!("brk-direct-cylinder-hint"));
            field(ui, &mut brake.direct_max_cylinder, 0.05, 0.0..=6.0, "bar");
            ui.end_row();
        });
    }
    // How the parking brake is held on, then how hard — the same order as the
    // magnetic and direct brakes above. Below the force it sat in the run of
    // four independent checkboxes and read as one of them.
    ui.checkbox(&mut brake.spring_parking, t!("brk-spring"))
        .on_hover_text(t!("brk-spring-hint"));
    form_grid("parking").show(ui, |ui| {
        form_label(ui, t!("brk-parking"));
        field(ui, &mut brake.parking_force, 500.0, 0.0..=400_000.0, "N");
        ui.end_row();
    });
    ui.checkbox(&mut brake.pilot_controlled, t!("brk-pilot"))
        .on_hover_text(t!("brk-pilot-hint"));
    ui.checkbox(&mut brake.supplement_brake, t!("brk-supplement"))
        .on_hover_text(t!("brk-supplement-hint"));
    ui.checkbox(&mut brake.angleicher, t!("brk-angleicher"))
        .on_hover_text(t!("brk-angleicher-hint"));

    subheading(ui, t!("group-air"));
    form_grid("air").show(ui, |ui| {
        row(ui, "air-aux", |ui| {
            field(ui, &mut brake.aux_volume, 5.0, 10.0..=500.0, "l");
        });
        row(ui, "air-pipe", |ui| {
            field(ui, &mut brake.pipe_volume, 1.0, 1.0..=200.0, "l");
        });
        row(ui, "air-main", |ui| {
            field(ui, &mut brake.main_volume, 50.0, 0.0..=5_000.0, "l");
        });
        row(ui, "air-compressor", |ui| {
            field(
                ui,
                &mut brake.compressor_delivery,
                50.0,
                0.0..=6_000.0,
                "l/min",
            );
        });
        row(ui, "air-leakage", |ui| {
            field(ui, &mut brake.leakage, 0.5, 0.0..=60.0, "l/min");
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

fn load_key(load: &LoadBraking) -> &'static str {
    match load {
        LoadBraking::None => "load-none",
        LoadBraking::Weighing => "load-weighing",
        LoadBraking::Changeover { .. } => "load-changeover",
    }
}

fn load_combo(ui: &mut egui::Ui, load: &mut LoadBraking) {
    egui::ComboBox::from_id_salt("load-braking")
        .selected_text(t!(load_key(load)))
        .show_ui(ui, |ui| {
            for value in [LoadBraking::None, LoadBraking::Weighing] {
                if ui
                    .selectable_label(*load == value, t!(load_key(&value)))
                    .clicked()
                {
                    *load = value;
                }
            }
            let is_changeover = matches!(load, LoadBraking::Changeover { .. });
            if ui
                .selectable_label(is_changeover, t!("load-changeover"))
                .clicked()
                && !is_changeover
            {
                // The anscription of a standard UIC freight wagon: 22 t of 55 t, over at 40 t.
                *load = LoadBraking::Changeover {
                    empty_share: 0.4,
                    changeover_mass_t: 40.0,
                };
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

/// Titled `(x, y)` table behind a clickable sparkline — the shared curve
/// editor, parameterised for this panel's units and ranges.
#[allow(clippy::too_many_arguments)]
fn table_editor(
    ui: &mut egui::Ui,
    id: &str,
    title: String,
    x_unit: &'static str,
    y_unit: &'static str,
    // Forces run to a million in whole newtons, a friction coefficient to one
    // in hundredths — the y column cannot share one step and one range.
    y_speed: f64,
    y_range: std::ops::RangeInclusive<f64>,
    points: &mut Vec<(f64, f64)>,
) {
    subheading(ui, title.clone());
    editor_ui::curve_editor(
        ui,
        &editor_ui::CurveSpec {
            id: egui::Id::new(("powertrain-curve", id)),
            title,
            x_unit,
            y_unit,
            x_speed: 1.0,
            y_speed,
            x_range: 0.0..=10_000.0,
            y_range,
        },
        points,
    );
}

/// Drive.
pub fn drive_panel(ui: &mut egui::Ui, traction: &mut Option<TractionSpec>) {
    form_grid("drive-type").show(ui, |ui| {
        row(ui, "drv-type", |ui| {
            type_combo(ui, traction);
        });
    });
    let Some(spec) = traction else {
        ui.label(
            egui::RichText::new(t!("drive-unpowered-note"))
                .small()
                .color(colors::TEXT_SECONDARY),
        );
        return;
    };
    ui.add_space(space::XS);
    // The curve type stores its points and plots them itself; the other three
    // derive theirs from a handful of limits, and nothing on screen showed the
    // shape those limits add up to.
    let plot_effort = !matches!(spec, TractionSpec::Curve { .. });
    match spec {
        TractionSpec::Curve {
            force,
            v_max,
            brake,
            ramp_time,
        } => {
            ui.label(
                egui::RichText::new(t!("curve-note"))
                    .small()
                    .color(colors::TEXT_SECONDARY),
            );
            form_grid("curve").show(ui, |ui| {
                row(ui, "drv-vmax", |ui| {
                    field(ui, v_max, 1.0, 0.0..=400.0, "km/h");
                });
                row(ui, "drv-ramp", |ui| {
                    field(ui, ramp_time, 0.1, 0.1..=30.0, "s");
                });
            });
            table_editor(
                ui,
                "traction",
                t!("table-tractive-effort"),
                "km/h",
                "N",
                100.0,
                0.0..=1_000_000.0,
                force,
            );
            table_editor(
                ui,
                "brake",
                t!("table-dynamic-brake"),
                "km/h",
                "N",
                100.0,
                0.0..=1_000_000.0,
                brake,
            );
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
            form_grid("tap").show(ui, |ui| {
                row(ui, "tap-steps", |ui| {
                    field(ui, steps, 1.0, 1.0..=64.0, "");
                });
                row(ui, "tap-step-time", |ui| {
                    field(ui, step_time, 0.05, 0.05..=5.0, "s");
                });
                row(ui, "drv-start-force", |ui| {
                    field(ui, max_force, 1000.0, 0.0..=800_000.0, "N");
                });
                row(ui, "drv-power", |ui| {
                    field(ui, max_power, 10_000.0, 0.0..=12_000_000.0, "W");
                });
                row(ui, "drv-vmax", |ui| {
                    field(ui, v_max, 1.0, 0.0..=400.0, "km/h");
                });
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
            form_grid("conv").show(ui, |ui| {
                row(ui, "drv-start-force", |ui| {
                    field(ui, max_force, 1000.0, 0.0..=800_000.0, "N");
                });
                row(ui, "drv-power", |ui| {
                    field(ui, max_power, 10_000.0, 0.0..=12_000_000.0, "W");
                });
                row(ui, "drv-vmax", |ui| {
                    field(ui, v_max, 1.0, 0.0..=400.0, "km/h");
                });
                row(ui, "drv-pullout", |ui| {
                    field(ui, v_pullout, 1.0, 0.0..=400.0, "km/h");
                });
                row(ui, "drv-ramp", |ui| {
                    field(ui, ramp_time, 0.1, 0.1..=30.0, "s");
                });
                row(ui, "drv-brake-force", |ui| {
                    field(ui, brake_force, 1000.0, 0.0..=800_000.0, "N");
                });
                row(ui, "drv-brake-power", |ui| {
                    field(ui, brake_power, 10_000.0, 0.0..=12_000_000.0, "W");
                });
                row(ui, "drv-brake-fade", |ui| {
                    field(ui, brake_fade_kmh, 1.0, 0.0..=60.0, "km/h");
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
            form_grid("diesel").show(ui, |ui| {
                row(ui, "drv-start-force-diesel", |ui| {
                    field(ui, max_force, 1000.0, 0.0..=800_000.0, "N");
                });
                row(ui, "drv-power", |ui| {
                    field(ui, max_power, 10_000.0, 0.0..=6_000_000.0, "W");
                });
                row(ui, "drv-vmax", |ui| {
                    field(ui, v_max, 1.0, 0.0..=250.0, "km/h");
                });
                row(ui, "drv-ramp", |ui| {
                    field(ui, ramp_time, 0.1, 0.1..=30.0, "s");
                });
                row(ui, "drv-crank-time", |ui| {
                    field(ui, start_time, 0.5, 0.5..=60.0, "s");
                });
            });
            optional(
                ui,
                "section-engine-map",
                engine,
                engine_defaults,
                engine_editor,
            );
            // The transmission is fitted against the plot below, and the suggestion that
            // starts the fit needs the engine map and the two data sheet figures.
            let (start_force, top_speed, engine_data) = (*max_force, *v_max, engine.clone());
            optional(
                ui,
                "section-transmission",
                transmission,
                transmission_defaults,
                |ui, t| transmission_editor(ui, t, engine_data.as_ref(), start_force, top_speed),
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

    if plot_effort {
        subheading(ui, t!("drv-force-plot"));
        let top = spec.v_max();
        editor_ui::sparkline_fn(ui, top, "km/h", "N", |kmh| spec.available_force(kmh / 3.6));
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
    ui.add_space(space::S);
    let mut on = value.is_some();
    if ui
        .checkbox(&mut on, editor_ui::section_title(t!(key)))
        .changed()
    {
        *value = on.then(defaults);
    }
    if let Some(inner) = value {
        ui.indent(key.to_owned(), |ui| body(ui, inner));
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
    form_grid("motor").show(ui, |ui| {
        row(ui, "mot-count", |ui| {
            field(ui, &mut m.count, 1.0, 1.0..=12.0, "");
        });
        row(ui, "mot-resistance", |ui| {
            field(ui, &mut m.resistance, 0.005, 0.001..=2.0, "Ω");
        });
        row(ui, "mot-machine-constant", |ui| {
            field(ui, &mut m.flux_constant, 0.001, 0.0001..=1.0, "V·s/A");
        });
        row(ui, "mot-saturation", |ui| {
            field(ui, &mut m.saturation_current, 10.0, 10.0..=5_000.0, "A");
        });
        row(ui, "mot-max-current", |ui| {
            field(ui, &mut m.max_current, 10.0, 10.0..=5_000.0, "A");
        });
        row(ui, "mot-max-voltage", |ui| {
            field(ui, &mut m.max_voltage, 10.0, 50.0..=4_000.0, "V");
        });
        row(ui, "mot-gear-ratio", |ui| {
            field(ui, &mut m.gear_ratio, 0.01, 0.5..=10.0, "");
        });
        row(ui, "drv-wheel-diameter", |ui| {
            field(ui, &mut m.wheel_diameter, 0.01, 0.3..=2.0, "m");
        });
        row(ui, "mot-efficiency", |ui| {
            field(ui, &mut m.efficiency, 0.01, 0.3..=1.0, "");
        });
    });
    subheading(ui, t!("mot-field-steps"));
    let mut remove = None;
    for (i, field) in m.field_steps.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            editor_ui::field(ui, field, 0.01, 0.2..=1.0, "");
            if ui.small_button("×").clicked() {
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
    form_grid("dynbrake").show(ui, |ui| {
        row(ui, "drv-brake-force", |ui| {
            field(ui, &mut b.max_force, 1000.0, 0.0..=800_000.0, "N");
        });
        row(ui, "drv-brake-power", |ui| {
            field(ui, &mut b.max_power, 10_000.0, 0.0..=12_000_000.0, "W");
        });
        row(ui, "drv-fade", |ui| {
            field(ui, &mut b.fade_out_kmh, 1.0, 0.0..=60.0, "km/h");
        });
        row(ui, "drv-ramp", |ui| {
            field(ui, &mut b.ramp_time, 0.1, 0.1..=30.0, "s");
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
        governor: Governor::Speed {
            steps: 0,
            droop: 0.04,
        },
        inertia: 60.0,
        response_time: 1.0,
    }
}

fn engine_editor(ui: &mut egui::Ui, e: &mut DieselEngine) {
    form_grid("engine").show(ui, |ui| {
        row(ui, "eng-idle", |ui| {
            field(ui, &mut e.idle_rpm, 10.0, 100.0..=1_500.0, "/min");
        });
        row(ui, "eng-rated", |ui| {
            field(ui, &mut e.rated_rpm, 10.0, 200.0..=4_000.0, "/min");
        });
        row(ui, "eng-overspeed", |ui| {
            field(ui, &mut e.max_rpm, 10.0, 200.0..=4_500.0, "/min");
        });
        row(ui, "eng-inertia", |ui| {
            field(ui, &mut e.inertia, 1.0, 0.5..=500.0, "kg·m²");
        });
        row(ui, "eng-rack-time", |ui| {
            field(ui, &mut e.response_time, 0.05, 0.05..=10.0, "s");
        });
    });
    // Governor: speed-governed vehicles set an engine speed, fill-governed ones
    // the rack. A labelled row like every other choice — as a bare pair of
    // toggles it was the one control in the panel that named its options but
    // not what they were options for.
    let speed_governed = matches!(e.governor, Governor::Speed { .. });
    form_grid("governor").show(ui, |ui| {
        row(ui, "eng-governor", |ui| {
            egui::ComboBox::from_id_salt("governor")
                .selected_text(t!(if speed_governed {
                    "gov-speed"
                } else {
                    "gov-fill"
                }))
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(speed_governed, t!("gov-speed"))
                        .on_hover_text(t!("gov-speed-hint"))
                        .clicked()
                    {
                        e.governor = Governor::Speed {
                            steps: 0,
                            droop: 0.04,
                        };
                    }
                    if ui
                        .selectable_label(!speed_governed, t!("gov-fill"))
                        .on_hover_text(t!("gov-fill-hint"))
                        .clicked()
                    {
                        e.governor = Governor::Fill;
                    }
                });
        });
        if let Governor::Speed { steps, droop } = &mut e.governor {
            row(ui, "gov-notches", |ui| {
                field(ui, steps, 1.0, 0.0..=32.0, "");
            });
            row(ui, "gov-droop", |ui| {
                field(ui, droop, 0.005, 0.0..=0.2, "");
            });
        }
    });
    table_editor(
        ui,
        "torque",
        t!("table-torque"),
        "/min",
        "N·m",
        100.0,
        0.0..=1_000_000.0,
        &mut e.torque_curve,
    );
}

fn transmission_defaults() -> Transmission {
    Transmission {
        circuits: vec![Circuit {
            kind: CircuitKind::Converter,
            ratio: 3.9,
            stall_ratio: 2.4,
            coupling_nu: 0.85,
            absorption: 0.53,
            absorption_slope: 0.15,
            shift_up_kmh: 70.0,
            shift_primary_kmh: 20.0,
        }],
        fill_steps: 0,
        fill_time: 1.2,
        drain_time: 0.7,
        hysteresis_kmh: 10.0,
        final_ratio: 1.0,
        wheel_diameter: 1.0,
        count: 1,
        efficiency: 0.95,
    }
}

fn transmission_editor(
    ui: &mut egui::Ui,
    t: &mut Transmission,
    engine: Option<&DieselEngine>,
    start_force: f64,
    v_max: f64,
) {
    // The converter figures cannot be read back out of a tractive effort curve, so fitting
    // against the plot is the only way — and this button is where the fit starts.
    if let Some(engine) = engine
        && ui
            .button(t!("action-suggest"))
            .on_hover_text(t!("trm-suggest-hint"))
            .clicked()
    {
        *t = t.suggest(engine, start_force, v_max);
    }
    form_grid("gearbox").show(ui, |ui| {
        row(ui, "trm-fill-steps", |ui| {
            field(ui, &mut t.fill_steps, 1.0, 0.0..=32.0, "");
        });
        row(ui, "trm-fill-time", |ui| {
            field(ui, &mut t.fill_time, 0.05, 0.05..=10.0, "s");
        });
        row(ui, "trm-drain-time", |ui| {
            field(ui, &mut t.drain_time, 0.05, 0.0..=10.0, "s");
        });
        row(ui, "trm-hysteresis", |ui| {
            field(ui, &mut t.hysteresis_kmh, 0.5, 0.0..=40.0, "km/h");
        });
        row(ui, "trm-final-ratio", |ui| {
            field(ui, &mut t.final_ratio, 0.01, 0.1..=10.0, "");
        });
        row(ui, "drv-wheel-diameter", |ui| {
            field(ui, &mut t.wheel_diameter, 0.01, 0.3..=2.0, "m");
        });
        row(ui, "trm-count", |ui| {
            field(ui, &mut t.count, 1.0, 1.0..=4.0, "");
        });
        row(ui, "trm-efficiency", |ui| {
            field(ui, &mut t.efficiency, 0.01, 0.3..=1.0, "");
        });
    });

    subheading(ui, t!("group-circuits"));
    let mut remove = None;
    for (i, circuit) in t.circuits.iter_mut().enumerate() {
        editor_ui::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                let converter = circuit.kind == CircuitKind::Converter;
                ui.label(editor_ui::section_title(format!("{}.", i + 1)));
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
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("×").clicked() {
                        remove = Some(i);
                    }
                });
            });
            form_grid(&format!("circuit-{i}")).show(ui, |ui| {
                row(ui, "cir-ratio", |ui| {
                    field(ui, &mut circuit.ratio, 0.01, 0.1..=10.0, "");
                });
                row(ui, "cir-stall", |ui| {
                    field(ui, &mut circuit.stall_ratio, 0.05, 1.0..=6.0, "");
                });
                row(ui, "cir-coupling-point", |ui| {
                    field(ui, &mut circuit.coupling_nu, 0.01, 0.1..=1.0, "");
                });
                row(ui, "cir-absorption", |ui| {
                    field(ui, &mut circuit.absorption, 0.005, 0.0001..=10.0, "");
                });
                row(ui, "cir-absorption-slope", |ui| {
                    field(ui, &mut circuit.absorption_slope, 0.01, -0.5..=1.0, "");
                });
                row(ui, "cir-shift-up", |ui| {
                    field(ui, &mut circuit.shift_up_kmh, 1.0, 0.0..=250.0, "km/h");
                });
                row(ui, "cir-shift-primary", |ui| {
                    field(ui, &mut circuit.shift_primary_kmh, 1.0, 0.0..=100.0, "km/h");
                });
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
    form_grid("retarder").show(ui, |ui| {
        row(ui, "ret-absorption", |ui| {
            field(ui, &mut b.absorption, 0.005, 0.0001..=10.0, "");
        });
        row(ui, "ret-ratio", |ui| {
            field(ui, &mut b.ratio, 0.05, 0.1..=12.0, "");
        });
        row(ui, "drv-wheel-diameter", |ui| {
            field(ui, &mut b.wheel_diameter, 0.01, 0.3..=2.0, "m");
        });
        row(ui, "ret-brake-force", |ui| {
            field(ui, &mut b.max_force, 1000.0, 0.0..=400_000.0, "N");
        });
        row(ui, "ret-brake-power", |ui| {
            field(ui, &mut b.max_power, 10_000.0, 0.0..=6_000_000.0, "W");
        });
        row(ui, "ret-fill-time", |ui| {
            field(ui, &mut b.fill_time, 0.05, 0.05..=10.0, "s");
        });
        row(ui, "drv-fade", |ui| {
            field(ui, &mut b.fade_out_kmh, 1.0, 0.0..=60.0, "km/h");
        });
    });
}
