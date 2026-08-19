//! Key figures of the data panel: the numbers a vehicle is checked against.
//!
//! Braked weight percentage, axle load and the tractive effort curve are all
//! derived — they are what a data sheet is read for, and what tells a modder
//! that a figure entered above is off by a decimal place.
//!
//! Everything here is read off `sim-core`'s own functions. A figure recomputed
//! in the editor is a figure that drifts away from the one the simulator drives
//! on, and then the editor is worse than no editor at all.

use crate::ui::row;
use bevy_egui::egui;
use editor_ui::{NBSP, Series, colors, space};
use i18n::t;
use sim_core::G;
use sim_core::brakes::BrakePosition;
use sim_core::physics::adhesion_coefficient;
use sim_core::train::{RailCondition, VehicleSpec};

/// Highest axle load line class D carries [t]. Above it the vehicle is not
/// forbidden, it is restricted to the lines that take it — which is a decision
/// to make in the editor, not something to find out on the road.
const MAX_AXLE_LOAD_T: f64 = 22.5;

/// Samples of a curve that is searched rather than drawn.
///
/// ponytail: a linear scan and a linear step between the two samples that
/// bracket the crossing, no root finder. Over 250 km/h that is a step of
/// 1.25 km/h on curves that are piecewise linear anyway, and the figure is read
/// to the km/h. Upgrade path is a bisection in the bracketing interval.
const SAMPLES: usize = 200;

/// One colour per traction chain of a dual-mode vehicle. All in the accent
/// family: what pushes the vehicle is blue, what holds it back is not.
const MODE_COLORS: [egui::Color32; 3] = [colors::ACCENT, colors::TEXT_STRONG, colors::ACCENT_BG];

/// The brake positions a vehicle can anscribe a braked weight of its own for.
const POSITIONS: [(BrakePosition, &str); 3] = [
    (BrakePosition::G, "key-brake-percentage-g"),
    (BrakePosition::P, "key-brake-percentage-p"),
    (BrakePosition::R, "key-brake-percentage-r"),
];

pub fn panel(ui: &mut egui::Ui, spec: &VehicleSpec) {
    let empty = spec.mass_empty;
    let laden = spec.mass_laden();
    let over_axle_load = axle_load_exceeded(spec, laden);
    let limit = adhesion_limit(spec, empty);
    let effort = starting_effort(spec);
    let slips = limit > 0.0 && effort > limit;

    editor_ui::form_grid("metrics").show(ui, |ui| {
        row(ui, "key-mass", |ui| {
            value(
                ui,
                pair(empty / 1000.0, laden / 1000.0, 1, "t"),
                colors::TEXT,
            );
        });
        // Without an axle count `axle_load_t` falls back to the reference load
        // of the friction curves — a figure about the brake, not about this
        // vehicle, and shown here it would read as one.
        if spec.axles > 0 {
            row(ui, "key-axle-load", |ui| {
                value(
                    ui,
                    pair(spec.axle_load_t(empty), spec.axle_load_t(laden), 1, "t"),
                    if over_axle_load {
                        colors::WARN
                    } else {
                        colors::TEXT
                    },
                );
            });
        }
        row(ui, "key-brake-percentage", |ui| {
            value(
                ui,
                pair(
                    brake_percentage(spec, empty, None),
                    brake_percentage(spec, laden, None),
                    0,
                    "%",
                ),
                colors::TEXT,
            );
        });
        // One row per position only where the vehicle anscribes one. A vehicle
        // that does not would repeat the line above three times.
        for (position, key) in POSITIONS {
            if spec.brake.brake_weight_override(position).is_none() {
                continue;
            }
            row(ui, key, |ui| {
                value(
                    ui,
                    pair(
                        brake_percentage(spec, empty, Some(position)),
                        brake_percentage(spec, laden, Some(position)),
                        0,
                        "%",
                    ),
                    colors::TEXT,
                );
            });
        }
        if limit > 0.0 {
            row(ui, "key-adhesive-mass", |ui| {
                value(
                    ui,
                    pair(
                        adhesive_mass(spec, empty) / 1000.0,
                        adhesive_mass(spec, laden) / 1000.0,
                        1,
                        "t",
                    ),
                    colors::TEXT,
                );
            });
            row(ui, "key-adhesion-limit", |ui| {
                value(
                    ui,
                    pair(
                        limit / 1000.0,
                        adhesion_limit(spec, laden) / 1000.0,
                        0,
                        "kN",
                    ),
                    colors::TEXT,
                );
            });
        }
        if spec.powered() {
            row(ui, "key-starting-effort", |ui| {
                value(
                    ui,
                    one(effort / 1000.0, 0, "kN"),
                    if slips { colors::WARN } else { colors::TEXT },
                );
            });
            if empty > 0.0 {
                let power = peak_power(spec) / 1000.0;
                row(ui, "key-power-weight", |ui| {
                    value(
                        ui,
                        pair(
                            power / (empty / 1000.0),
                            power / (laden / 1000.0),
                            1,
                            "kW/t",
                        ),
                        colors::TEXT,
                    );
                });
            }
            row(ui, "key-balancing-speed", |ui| {
                match balancing_speed(spec) {
                    Some(kmh) => value(ui, one(kmh, 0, "km/h"), colors::TEXT),
                    // Not a defect: the drive runs into its own v max before the
                    // running resistance ever catches up with it.
                    None => value(ui, t!("key-above-v-max"), colors::TEXT_SECONDARY),
                }
            });
        }
    });

    if over_axle_load {
        note(
            ui,
            t!(
                "key-axle-load-warn",
                load = i18n::decimal(spec.axle_load_t(laden), 1),
                limit = i18n::decimal(MAX_AXLE_LOAD_T, 1)
            ),
        );
    }
    if slips {
        note(
            ui,
            t!(
                "key-slip-warn",
                force = i18n::decimal(effort / 1000.0, 0),
                limit = i18n::decimal(limit / 1000.0, 0)
            ),
        );
    }

    plot(ui, spec, limit);
}

/// The tractive effort diagram — the view a vehicle is judged from.
///
/// Everything over the same speed axis: what the vehicle pulls in each mode,
/// what its dynamic brake takes back, what the rail carries, and the running
/// resistance whose crossing with the effort is the balancing speed.
fn plot(ui: &mut egui::Ui, spec: &VehicleSpec, limit: f64) {
    let top = plot_top(spec);
    let modes = spec.modes();
    let mut series = Vec::new();
    for (i, &mode) in modes.iter().enumerate() {
        series.push(Series::sampled(
            t!(mode.key()),
            MODE_COLORS[i % MODE_COLORS.len()],
            top,
            |kmh| spec.available_force(mode, kmh / 3.6),
        ));
    }
    if spec.has_dynamic_brake() {
        // Mirrored rather than drawn below zero. On a shared positive axis the
        // brake is read straight against the effort it has to take back, and
        // the axis keeps twice the resolution for every other curve.
        series.push(Series::sampled(
            t!("plot-dynamic-brake"),
            colors::ERROR,
            top,
            |kmh| {
                modes
                    .iter()
                    .map(|&m| spec.available_brake_force(m, kmh / 3.6))
                    .fold(0.0, f64::max)
            },
        ));
    }
    series.push(Series::sampled(
        t!("plot-resistance"),
        colors::WARN,
        top,
        |kmh| spec.resistance(kmh / 3.6),
    ));
    if spec.powered() && limit > 0.0 {
        series.push(Series {
            label: t!("plot-adhesion-limit"),
            color: colors::TEXT_SECONDARY,
            points: vec![(0.0, limit), (top, limit)],
        });
    }
    // A vehicle without a drive has no tractive effort diagram — what is left
    // is the resistance, and it is titled as what it is.
    editor_ui::subheading(
        ui,
        t!(if spec.powered() {
            "plot-tractive-effort"
        } else {
            "res-plot"
        }),
    );
    editor_ui::multi_plot(ui, "km/h", "N", &series);
}

// --- The figures -----------------------------------------------------------

/// Braked weight percentage at a total mass of `mass_kg`, in `position` where
/// the vehicle anscribes a braked weight of its own.
///
/// [`VehicleSpec::brake_percentage`] is this for the empty vehicle in no
/// particular position; the load and the changeover handle are what the editor
/// adds on top.
fn brake_percentage(spec: &VehicleSpec, mass_kg: f64, position: Option<BrakePosition>) -> f64 {
    if mass_kg <= 0.0 {
        return 0.0;
    }
    let weight = match position {
        Some(position) => spec.brake.brake_weight_at_position(position) * spec.load_share(mass_kg),
        None => spec.brake_weight_at(mass_kg),
    };
    weight / (mass_kg / 1000.0) * 100.0
}

/// Mass on the driven axles [kg] at a total mass of `mass_kg` — what
/// `Vehicle::adhesive_mass` reads off the running gear, before there is a
/// vehicle to read it off.
fn adhesive_mass(spec: &VehicleSpec, mass_kg: f64) -> f64 {
    let share: f64 = spec
        .running_gear()
        .iter()
        .filter(|axle| axle.driven)
        .map(|axle| axle.load_share)
        .sum();
    mass_kg * share
}

/// Tractive effort the adhesion carries at standstill [N]: μ·m·g on dry rail
/// without sand, off the same Curtius/Kniffler curve the physics runs on.
///
/// Standstill is where that curve is at its best and where the vehicle is
/// judged: an effort entered above this one spins the wheels on every start.
fn adhesion_limit(spec: &VehicleSpec, mass_kg: f64) -> f64 {
    adhesion_coefficient(0.0, RailCondition::Dry, false) * adhesive_mass(spec, mass_kg) * G
}

/// Tractive effort at standstill [N] — of the strongest mode, since the mode
/// selector runs one at a time.
fn starting_effort(spec: &VehicleSpec) -> f64 {
    spec.modes()
        .iter()
        .map(|&mode| spec.available_force(mode, 0.0))
        .fold(0.0, f64::max)
}

/// Does the axle load exceed what line class D carries?
fn axle_load_exceeded(spec: &VehicleSpec, mass_kg: f64) -> bool {
    spec.axles > 0 && spec.axle_load_t(mass_kg) > MAX_AXLE_LOAD_T
}

/// Highest power the tractive effort curve reaches [W], `max F·v`.
///
/// ponytail: sampled off the curve instead of read out of the drive. The rated
/// power sits in five variants of `TractionSpec` and comes out of the motor
/// data in a sixth, while `F·v` is the same number in all of them — and it is
/// the power at the wheel, which is the one a power-to-weight ratio is quoted
/// with. Upgrade path is a `max_power()` on `TractionSpec` the day anything
/// else needs one.
fn peak_power(spec: &VehicleSpec) -> f64 {
    let top = plot_top(spec) / 3.6;
    let mut best = 0.0_f64;
    for mode in spec.modes() {
        for i in 0..=SAMPLES {
            let v = top * i as f64 / SAMPLES as f64;
            best = best.max(spec.available_force(mode, v) * v);
        }
    }
    best
}

/// Speed [km/h] at which the tractive effort has fallen to the running
/// resistance — what the vehicle settles at on the level, on its own.
///
/// `None` where the drive runs into its own v max first: then the vehicle is
/// limited by its top speed, not by its resistance, and there is no crossing to
/// name.
fn balancing_speed(spec: &VehicleSpec) -> Option<f64> {
    let top = spec.drive_v_max();
    let surplus = |kmh: f64| {
        let v = kmh / 3.6;
        spec.modes()
            .iter()
            .map(|&mode| spec.available_force(mode, v))
            .fold(0.0, f64::max)
            - spec.resistance(v)
    };
    let mut previous = (0.0, surplus(0.0));
    if previous.1 <= 0.0 {
        return None;
    }
    for i in 1..=SAMPLES {
        let kmh = top * i as f64 / SAMPLES as f64;
        let now = surplus(kmh);
        if now <= 0.0 {
            let (before, was) = previous;
            return Some(before + (kmh - before) * was / (was - now));
        }
        previous = (kmh, now);
    }
    None
}

/// Top of the speed axis [km/h]: as fast as the vehicle may run and as fast as
/// it can pull, whichever is higher, so neither curve is cut off. A vehicle
/// that states neither still gets an axis to look at.
fn plot_top(spec: &VehicleSpec) -> f64 {
    let top = spec.v_max.max(spec.drive_v_max());
    if top > 0.0 { top } else { 160.0 }
}

// --- Presentation ----------------------------------------------------------

/// One figure and its unit, with the decimal mark of the current language.
fn one(value: f64, decimals: usize, unit: &str) -> String {
    format!("{}{NBSP}{unit}", i18n::decimal(value, decimals))
}

/// A figure as a data sheet gives it: empty, and after the arrow the laden
/// value where the load moves it. One row for both — a second row per figure
/// would double a table that is read by running an eye down it. Where the load
/// leaves the figure where it was, it is written once.
fn pair(empty: f64, laden: f64, decimals: usize, unit: &str) -> String {
    if i18n::decimal(empty, decimals) == i18n::decimal(laden, decimals) {
        return one(empty, decimals, unit);
    }
    format!(
        "{} → {}",
        i18n::decimal(empty, decimals),
        one(laden, decimals, unit)
    )
}

/// A derived figure. `TEXT`, not the `TEXT_SECONDARY` of the label next to it:
/// the number is what the row is read for and has to outrank its own name.
fn value(ui: &mut egui::Ui, text: String, color: egui::Color32) {
    ui.label(egui::RichText::new(text).color(color));
}

/// A figure that is out of bounds says so in words. A number quietly turning
/// yellow is a colour, not a finding.
fn note(ui: &mut egui::Ui, text: String) {
    ui.add_space(space::XS);
    ui.label(egui::RichText::new(text).small().color(colors::WARN));
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::drive::{DriveSpec, TractionSpec};
    use sim_core::train::Davis;

    /// A locomotive with a flat tractive effort curve and a resistance of
    /// 180 N per m/s — every figure below is then arithmetic that can be
    /// checked by hand.
    fn loco(force_n: f64) -> VehicleSpec {
        VehicleSpec {
            mass_empty: 84_000.0,
            axles: 4,
            adhesive_mass_fraction: 1.0,
            v_max: 250.0,
            davis: Davis {
                a: 0.0,
                b: 180.0,
                c: 0.0,
            },
            drives: vec![DriveSpec::new(TractionSpec::Curve {
                force: vec![(0.0, force_n), (250.0, force_n)],
                v_max: 250.0,
                brake: Vec::new(),
                ramp_time: 1.0,
            })],
            ..VehicleSpec::default()
        }
    }

    /// The one figure that says a locomotive will spin its wheels away every
    /// time it starts — and the only place the editor ever says so.
    #[test]
    fn a_starting_effort_over_the_adhesion_is_recognised() {
        let spec = loco(200_000.0);
        assert_eq!(adhesive_mass(&spec, spec.mass_empty), 84_000.0);
        // The coefficient is sim-core's own, not a second opinion about it.
        let mu = adhesion_coefficient(0.0, RailCondition::Dry, false);
        let limit = adhesion_limit(&spec, spec.mass_empty);
        assert!((limit - mu * 84_000.0 * G).abs() < 1e-6);
        assert!(starting_effort(&spec) < limit, "200 kN stay on the rail");
        assert!(
            starting_effort(&loco(400_000.0)) > limit,
            "400 kN off 84 t is a wheelspin"
        );
    }

    #[test]
    fn the_axle_load_warning_turns_at_the_line_class_limit() {
        let mut spec = loco(200_000.0);
        spec.mass_empty = 4.0 * MAX_AXLE_LOAD_T * 1000.0;
        assert_eq!(spec.axle_load_t(spec.mass_empty), MAX_AXLE_LOAD_T);
        assert!(
            !axle_load_exceeded(&spec, spec.mass_empty),
            "class D carries 22.5 t"
        );
        spec.mass_empty += 400.0;
        assert!(
            axle_load_exceeded(&spec, spec.mass_empty),
            "22.6 t does not"
        );
        // Without an axle count there is no axle load to warn about.
        spec.axles = 0;
        assert!(!axle_load_exceeded(&spec, spec.mass_empty));
    }

    #[test]
    fn the_balancing_speed_is_where_effort_meets_resistance() {
        // 10 kN against 180 N per m/s: 55.6 m/s, 200 km/h.
        let kmh = balancing_speed(&loco(10_000.0)).expect("crosses below v max");
        assert!((kmh - 200.0).abs() < 1.0, "{kmh}");
        // 100 kN would balance at 2 000 km/h — the vehicle runs into its own
        // v max long before, and the row says so instead of naming a speed.
        assert!(balancing_speed(&loco(100_000.0)).is_none());
    }

    /// The percentage of the empty vehicle is `sim-core`'s, the load and the
    /// changeover handle are what this file adds.
    #[test]
    fn the_braked_weight_percentage_follows_the_load_and_the_position() {
        let mut spec = loco(200_000.0);
        spec.brake.brake_weight = 84.0;
        assert_eq!(
            brake_percentage(&spec, spec.mass_empty, None),
            spec.brake_percentage()
        );
        assert_eq!(brake_percentage(&spec, spec.mass_empty, None), 100.0);
        spec.brake.brake_weight_g = Some(63.0);
        assert_eq!(
            brake_percentage(&spec, spec.mass_empty, Some(BrakePosition::G)),
            75.0
        );
        // A vehicle with no mass has no percentage, and no division by zero.
        spec.mass_empty = 0.0;
        assert_eq!(brake_percentage(&spec, 0.0, None), 0.0);
    }

    /// Empty and laden in one cell — and one figure where the vehicle carries
    /// nothing, rather than the same number written twice.
    #[test]
    fn a_figure_that_the_load_does_not_move_is_written_once() {
        i18n::set_language("en");
        assert_eq!(pair(40.0, 40.0, 1, "t"), "40.0\u{A0}t");
        assert_eq!(pair(40.0, 45.0, 1, "t"), "40.0 → 45.0\u{A0}t");
        // Rounded to the same figure is the same figure on screen.
        assert_eq!(pair(100.4, 100.2, 0, "%"), "100\u{A0}%");
    }

    /// Every label the grid draws, and the two lines that carry a figure.
    const KEYS: [&str; 14] = [
        "key-mass",
        "key-axle-load",
        "key-brake-percentage",
        "key-brake-percentage-g",
        "key-brake-percentage-p",
        "key-brake-percentage-r",
        "key-adhesive-mass",
        "key-adhesion-limit",
        "key-starting-effort",
        "key-power-weight",
        "key-balancing-speed",
        "key-above-v-max",
        "plot-tractive-effort",
        "res-plot",
    ];

    const VALUE_KEYS: [(&str, [&str; 2]); 2] = [
        ("key-axle-load-warn", ["load", "limit"]),
        ("key-slip-warn", ["force", "limit"]),
    ];

    #[test]
    fn every_key_the_panel_draws_exists() {
        for key in KEYS {
            assert!(i18n::maybe(key).is_some(), "{key}");
        }
        for label in [
            "plot-resistance",
            "plot-dynamic-brake",
            "plot-adhesion-limit",
        ] {
            assert!(i18n::maybe(label).is_some(), "{label}");
        }
    }

    /// A message with placeholders does not resolve until every one of them has
    /// its argument, so asking for the figures covers both: a key the locales
    /// do not have comes back as its own name, and a mistyped placeholder
    /// leaves a warning that names no number.
    #[test]
    fn the_warnings_name_their_figures() {
        for (key, placeholders) in VALUE_KEYS {
            let mut args = i18n::Args::new();
            for (i, placeholder) in placeholders.iter().enumerate() {
                args.insert(
                    std::borrow::Cow::Borrowed(placeholder),
                    i18n::FluentValue::from(format!("4{i}")),
                );
            }
            let line = i18n::lookup_args(key, &args);
            for i in 0..placeholders.len() {
                assert!(line.contains(&format!("4{i}")), "{key}: {line}");
            }
        }
    }
}
