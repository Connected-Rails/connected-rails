//! File-wide check report of the data panel.
//!
//! The block diagram reports its own bake findings; this looks at everything
//! else — figures that contradict each other, bindings that point at nothing,
//! fields a vehicle of this kind cannot sensibly leave empty.
//!
//! [`check`] is the whole check and knows nothing about egui, so the tests below
//! read findings rather than pixels; [`panel`] only paints what it returns.

use bevy_egui::egui;
use editor_ui::colors;
use i18n::t;
use sim_core::brakes::LoadBraking;
use sim_core::doors::DoorSystem;
use sim_core::physics::adhesion_coefficient;
use sim_core::train::{RailCondition, VehicleSpec, lod_level};

/// How badly a finding is meant.
///
/// The order is the drawing order: what stops the vehicle from working comes
/// first, what is merely legal-but-pointless comes last. A `Note` must never be
/// something the editor itself suggests — that is what makes the report worth
/// reading (`Part::function` is documented free-form text, so an unknown name is
/// a note and never an error).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// The vehicle does not work: a figure the physics divides by is missing, a
    /// binding points at nothing.
    Error,
    /// The vehicle works but does something other than its data sheet says.
    Warning,
    /// Legal, and without effect.
    Note,
}

/// One finding: what is wrong, how badly, and the sentence to show for it.
///
/// `key` is kept next to the rendered `text` so a test can tell "the message is
/// missing from the `.ftl`" from "the message says something odd" — `t!` hands
/// back the key itself for an unknown one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub level: Level,
    pub key: &'static str,
    pub text: String,
}

impl Finding {
    fn new(level: Level, key: &'static str) -> Self {
        Self {
            level,
            key,
            text: t!(key),
        }
    }

    /// A finding whose message carries placeholders, rendered by the caller.
    fn with(level: Level, key: &'static str, text: String) -> Self {
        Self { level, key, text }
    }
}

/// Everything the report has to say about `spec`, worst first.
///
/// `nodes` are the node names of the loaded glTF model. **Empty means no model
/// is loaded**, not "a model without nodes" — every binding check is skipped
/// then, because otherwise the report would shout at every file opened without
/// its model.
///
/// ponytail: one empty list for both cases, so a glTF that really carries no
/// named node is the one file the binding checks stay quiet about. Take an
/// `Option<&[String]>` here as soon as the caller can tell the two apart — the
/// data panel already knows, it just has nowhere to say so.
pub fn check(spec: &VehicleSpec, nodes: &[String]) -> Vec<Finding> {
    let mut out = Vec::new();
    figures(spec, &mut out);
    brake(spec, &mut out);
    drive(spec, &mut out);
    model(spec, nodes, &mut out);
    // Stable, so the order within a level stays the order the checks run in —
    // the report does not reshuffle itself while the user edits.
    out.sort_by_key(|f| f.level);
    out
}

/// The figures every vehicle needs, whatever it is.
fn figures(spec: &VehicleSpec, out: &mut Vec<Finding>) {
    if spec.length <= 0.0 {
        out.push(Finding::new(Level::Error, "check-length"));
    }
    if spec.mass_empty <= 0.0 {
        out.push(Finding::new(Level::Error, "check-mass"));
    }
    if spec.gauge <= 0.0 {
        out.push(Finding::new(Level::Error, "check-gauge"));
    }
    if spec.axles == 0 {
        out.push(Finding::new(Level::Warning, "check-axles"));
    }
    // A coach carries about 0.05, a powered vehicle about 0.25; outside 0 … 0.5
    // the vehicle either gains inertia out of nowhere or loses it.
    if !(0.0..=0.5).contains(&spec.rotating_mass_factor) {
        out.push(Finding::with(
            Level::Warning,
            "check-rotating-mass",
            t!(
                "check-rotating-mass",
                value = format!("{:.2}", spec.rotating_mass_factor)
            ),
        ));
    }
    // The payload the loads are capped against.
    for load in &spec.loads {
        if spec.max_payload > 0.0 && load.mass > spec.max_payload {
            out.push(Finding::with(
                Level::Warning,
                "check-load-over-payload",
                t!(
                    "check-load-over-payload",
                    load = load.name.as_str(),
                    mass = format!("{:.1}", load.mass / 1000.0),
                    max = format!("{:.1}", spec.max_payload / 1000.0)
                ),
            ));
        }
    }
    // Only a powered vehicle can release its own doors; a hauled coach takes the
    // door control of the vehicle at the head of the train and rightly states
    // none of its own.
    if spec.passenger_doors && spec.doors == DoorSystem::None && spec.powered() {
        out.push(Finding::new(Level::Warning, "check-doors"));
    }
}

/// Brake figures — the braked weight percentage is what a brake sheet is read in,
/// so it is the figure a decimal place shows up in.
fn brake(spec: &VehicleSpec, out: &mut Vec<Finding>) {
    if spec.brake.brake_weight <= 0.0 {
        out.push(Finding::new(Level::Error, "check-no-brake-weight"));
    }
    if spec.brake.max_force <= 0.0 {
        out.push(Finding::new(Level::Error, "check-no-brake-force"));
    }
    let percentage = spec.brake_percentage();
    // Band of what a European brake sheet ever shows: a block-braked freight
    // wagon in G sits near 55 %, a disc-braked coach in R near 150 %. Outside
    // 30 … 250 % a figure is a unit or a decimal place out, not a design.
    if spec.brake.brake_weight > 0.0
        && spec.mass_empty > 0.0
        && !(30.0..=250.0).contains(&percentage)
    {
        out.push(Finding::with(
            Level::Warning,
            "check-brake-percentage",
            t!(
                "check-brake-percentage",
                value = format!("{percentage:.0}"),
                weight = format!("{:.1}", spec.brake.brake_weight)
            ),
        ));
    }
    // Without load-proportional braking the braked weight stays where it is while
    // the mass grows, so the deceleration falls off with every tonne loaded. Half
    // the tare mass is where that starts to matter: the 5 t of passengers in a
    // coach move the figure by a tenth, the 57 t of a freight wagon by two thirds.
    let laden = spec.mass_laden();
    if spec.max_payload > spec.mass_empty * 0.5
        && spec.brake.load_braking == LoadBraking::None
        && laden > 0.0
    {
        let laden_percentage = spec.brake_weight_at(laden) / (laden / 1000.0) * 100.0;
        out.push(Finding::with(
            Level::Warning,
            "check-load-braking",
            t!(
                "check-load-braking",
                value = format!("{laden_percentage:.0}"),
                payload = format!("{:.1}", spec.max_payload / 1000.0)
            ),
        ));
    }
}

/// Drive, adhesion and the top speed the two of them have to agree on.
fn drive(spec: &VehicleSpec, out: &mut Vec<Finding>) {
    let gear = spec.running_gear();
    let driven: f64 = gear.iter().filter(|a| a.driven).map(|a| a.load_share).sum();
    // An empty running gear means the vehicle states no axles at all, which
    // `figures` already reports — one missing figure, one finding.
    if spec.powered() && !gear.is_empty() && driven <= 0.0 {
        out.push(Finding::new(Level::Error, "check-drive-no-adhesion"));
    }
    if !spec.powered() && spec.adhesive_mass_fraction > 0.0 {
        out.push(Finding::new(Level::Warning, "check-adhesion-no-drive"));
    }

    let drive_v_max = spec.drive_v_max();
    if spec.v_max <= 0.0 {
        out.push(Finding::new(Level::Warning, "check-no-v-max"));
    } else if drive_v_max > spec.v_max {
        // Nothing in the physics caps the vehicle at `v_max` — it is the running
        // gear limit the speedometer, the AFB and the AI driver plan with, so a
        // drive that pulls past it simply overspeeds the vehicle.
        out.push(Finding::with(
            Level::Warning,
            "check-drive-over-v-max",
            t!(
                "check-drive-over-v-max",
                drive = format!("{drive_v_max:.0}"),
                vehicle = format!("{:.0}", spec.v_max)
            ),
        ));
    }

    // Starting tractive effort against the rail: Curtius/Kniffler at standstill on
    // a dry rail, plus what the creep control gets on top. No sand — a vehicle
    // that only starts with the sander running is one the report should mention.
    if spec.powered() && driven > 0.0 && spec.mass_empty > 0.0 {
        let start = spec
            .modes()
            .iter()
            .map(|&mode| spec.available_force(mode, 0.0))
            .fold(0.0, f64::max);
        let limit = adhesion_coefficient(0.0, RailCondition::Dry, false)
            * spec.slip_protection.adhesion_bonus()
            * spec.mass_empty
            * driven
            * sim_core::G;
        // ponytail: a tenth of headroom instead of a tolerance per drive type. A
        // real locomotive is specified right at its adhesion limit, so an exact
        // comparison would flag every well-built vehicle. Narrow it once the
        // report can tell a designed limit from a typo — which needs the data
        // sheet's own starting effort, and no field carries it.
        if start > limit * 1.1 {
            out.push(Finding::with(
                Level::Warning,
                "check-tractive-effort",
                t!(
                    "check-tractive-effort",
                    force = format!("{:.0}", start / 1000.0),
                    limit = format!("{:.0}", limit / 1000.0)
                ),
            ));
        }
    }
}

/// Model file, node bindings and levels of detail.
fn model(spec: &VehicleSpec, nodes: &[String], out: &mut Vec<Finding>) {
    let Some(model) = &spec.model else {
        return;
    };
    if model.file.is_empty() {
        out.push(Finding::new(Level::Error, "check-model-no-file"));
    }
    lods(spec, nodes, out);
    // No model loaded — every name below would look wrong for want of anything
    // to compare it against.
    if nodes.is_empty() {
        return;
    }
    let has = |node: &str| nodes.iter().any(|n| n == node);

    for part in &model.parts {
        if !has(&part.node) {
            out.push(Finding::with(
                Level::Error,
                "check-part-node",
                t!("check-part-node", node = part.node.as_str()),
            ));
        }
        // `function` is documented free-form (MODS.md): the app maps the names it
        // knows and a mod may invent its own, so an unknown one is legal and
        // simply without effect — a note, never an error. The editor's own name
        // fallback suggests `wheel` for `wheel_*` nodes, which the simulator does
        // not evaluate; a report that called that an error would be ignored.
        if let Some(reason) = sim_core::cab::part_function_error(&part.function) {
            out.push(Finding::with(
                Level::Note,
                "check-part-function",
                t!(
                    "check-part-function",
                    node = part.node.as_str(),
                    reason = t!(reason)
                ),
            ));
        }
    }
    for control in model.cab.iter().flat_map(|cab| &cab.controls) {
        if !has(&control.node) {
            out.push(Finding::with(
                Level::Error,
                "check-control-node",
                t!("check-control-node", node = control.node.as_str()),
            ));
        }
    }
    for display in &model.displays {
        if !has(&display.node) {
            out.push(Finding::with(
                Level::Error,
                "check-display-node",
                t!(
                    "check-display-node",
                    name = display.name.as_str(),
                    node = display.node.as_str()
                ),
            ));
        }
    }
    for load in &spec.loads {
        let Some(node) = &load.node else { continue };
        if !has(node) {
            out.push(Finding::with(
                Level::Error,
                "check-load-node",
                t!(
                    "check-load-node",
                    load = load.name.as_str(),
                    node = node.as_str()
                ),
            ));
        }
    }
    // One node, one mover: `app::models` walks the scene once and takes the first
    // control or part that matches the node's name, so every further binding on
    // it is dropped without a word. Displays and loads are left out — they are
    // driven by systems of their own and do not fight over the transform.
    let mut bound: Vec<&str> = model
        .cab
        .iter()
        .flat_map(|cab| &cab.controls)
        .map(|c| c.node.as_str())
        .chain(model.parts.iter().map(|p| p.node.as_str()))
        .collect();
    bound.sort_unstable();
    let mut duplicates: Vec<&str> = bound
        .windows(2)
        .filter(|w| w[0] == w[1])
        .map(|w| w[0])
        .collect();
    duplicates.dedup();
    for node in duplicates {
        out.push(Finding::with(
            Level::Warning,
            "check-node-twice",
            t!("check-node-twice", node = node),
        ));
    }
}

/// Levels of detail: one entry per level, distances rising, and a node for each.
fn lods(spec: &VehicleSpec, nodes: &[String], out: &mut Vec<Finding>) {
    let Some(model) = &spec.model else {
        return;
    };
    let mut seen: Vec<u8> = Vec::new();
    let mut previous: Option<f64> = None;
    for lod in &model.lods {
        if seen.contains(&lod.level) {
            out.push(Finding::with(
                Level::Error,
                "check-lod-duplicate",
                t!("check-lod-duplicate", level = lod.level),
            ));
        }
        seen.push(lod.level);
        if previous.is_some_and(|d| lod.distance <= d) {
            out.push(Finding::with(
                Level::Error,
                "check-lod-order",
                t!("check-lod-order", level = lod.level),
            ));
        }
        previous = Some(lod.distance);
    }
    if nodes.is_empty() {
        return;
    }
    let levels: Vec<u8> = nodes.iter().filter_map(|n| lod_level(n)).collect();
    for lod in &model.lods {
        if !levels.contains(&lod.level) {
            out.push(Finding::with(
                Level::Warning,
                "check-lod-no-nodes",
                t!("check-lod-no-nodes", level = lod.level),
            ));
        }
    }
    // The other way round: nodes carry levels the vehicle lists none of, so every
    // level draws at once and the model is there twice over.
    if model.lods.is_empty() && !levels.is_empty() {
        out.push(Finding::new(Level::Warning, "check-lod-missing"));
    }
}

/// The report under the "Checks" heading. The heading itself is drawn by `ui.rs`.
pub fn panel(ui: &mut egui::Ui, findings: &[Finding]) {
    if findings.is_empty() {
        ui.small(t!("check-ok"));
        return;
    }
    for finding in findings {
        let color = match finding.level {
            Level::Error => colors::ERROR,
            Level::Warning => colors::WARN,
            Level::Note => colors::TEXT_SECONDARY,
        };
        ui.add(egui::Label::new(egui::RichText::new(&finding.text).small().color(color)).wrap());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::cab::{CabSpec, DisplaySpec};
    use sim_core::train::{LoadSpec, Lod, Motion, Part, VehicleModel};

    fn errors(findings: &[Finding]) -> Vec<&str> {
        findings
            .iter()
            .filter(|f| f.level == Level::Error)
            .map(|f| f.key)
            .collect()
    }

    fn keys(findings: &[Finding]) -> Vec<&str> {
        findings.iter().map(|f| f.key).collect()
    }

    fn part(node: &str, function: &str) -> Part {
        Part {
            node: node.into(),
            function: function.into(),
            motion: Motion::Visibility,
        }
    }

    /// The most important test in the file: a report that complains about every
    /// healthy vehicle is a report nobody reads again.
    #[test]
    fn the_reference_vehicles_produce_no_error() {
        let vehicles = [
            content::vehicles::br101(),
            content::vehicles::br110(),
            content::vehicles::br218(),
            content::vehicles::br232(),
            content::vehicles::br52(),
            content::vehicles::passenger_coach(),
            content::vehicles::freight_wagon(),
            content::vehicles::freight_wagon_k_valve(),
            content::vehicles::railcar(),
        ];
        for spec in vehicles {
            let findings = check(&spec, &[]);
            assert!(
                errors(&findings).is_empty(),
                "{}: {:?}",
                spec.name,
                errors(&findings)
            );
            // They say nothing at all, in fact — and that is the bar. A warning
            // on a vehicle straight out of the template menu would train the user
            // to scroll past the whole section.
            assert!(findings.is_empty(), "{}: {:?}", spec.name, keys(&findings));
        }
    }

    /// "New" starts here, and so does every file a mod writes by hand from the
    /// defaults.
    #[test]
    fn a_blank_vehicle_says_nothing() {
        assert!(check(&VehicleSpec::default(), &[]).is_empty());
    }

    #[test]
    fn a_binding_is_checked_against_the_model_only_where_one_is_loaded() {
        let mut spec = content::vehicles::br101();
        spec.model = Some(VehicleModel {
            file: "example/assets/br101.gltf".into(),
            parts: vec![part("pantograph_rear", "pantograph")],
            ..VehicleModel::default()
        });
        // No model loaded: the file is opened, not judged.
        assert!(check(&spec, &[]).is_empty());
        // Loaded and the node is there: still nothing to say.
        let nodes = vec!["body".to_string(), "pantograph_rear".to_string()];
        assert!(check(&spec, &nodes).is_empty());
        // Loaded and the node is gone: the part would silently never move.
        let nodes = vec!["body".to_string()];
        assert_eq!(errors(&check(&spec, &nodes)), ["check-part-node"]);
    }

    #[test]
    fn an_unknown_part_function_is_a_note_and_a_missing_node_an_error() {
        let mut spec = content::vehicles::br101();
        spec.model = Some(VehicleModel {
            file: "example/assets/br101.gltf".into(),
            parts: vec![
                // What the editor itself suggests for a `wheel_*` node — legal,
                // and evaluated by nothing.
                part("wheel_1", "wheel"),
                part("gauge_typo", "gauge:tippfehler"),
                part("pantograph_rear", "pantograph"),
            ],
            ..VehicleModel::default()
        });
        let nodes = vec![
            "wheel_1".to_string(),
            "gauge_typo".to_string(),
            "pantograph_rear".to_string(),
        ];
        let findings = check(&spec, &nodes);
        assert!(errors(&findings).is_empty(), "{:?}", keys(&findings));
        assert_eq!(
            findings.iter().filter(|f| f.level == Level::Note).count(),
            2,
            "{:?}",
            findings
        );
        // The reason travels with the finding, so the two notes read differently.
        let notes: Vec<&str> = findings.iter().map(|f| f.text.as_str()).collect();
        assert_ne!(notes[0], notes[1]);
    }

    #[test]
    fn every_kind_of_binding_is_checked() {
        let mut spec = content::vehicles::br101();
        spec.loads = vec![LoadSpec {
            name: "Kohle".into(),
            mass: 1_000.0,
            node: Some("coal".into()),
        }];
        spec.model = Some(VehicleModel {
            file: "example/assets/br101.gltf".into(),
            parts: vec![part("gone_part", "pantograph")],
            cab: Some(CabSpec {
                controls: vec![sim_core::cab::CabControlSpec {
                    node: "gone_control".into(),
                    input: sim_core::cab::CabControl::Throttle,
                    motion: Motion::Visibility,
                }],
                ..CabSpec::default()
            }),
            displays: vec![DisplaySpec {
                name: "mfa".into(),
                node: "gone_display".into(),
                width: 256,
                height: 160,
                widgets: vec![],
                html: None,
            }],
            ..VehicleModel::default()
        });
        let findings = check(&spec, &["body".to_string()]);
        let mut found = errors(&findings);
        found.sort_unstable();
        assert_eq!(
            found,
            [
                "check-control-node",
                "check-display-node",
                "check-load-node",
                "check-part-node",
            ]
        );
    }

    #[test]
    fn a_node_bound_twice_is_reported_once() {
        let mut spec = content::vehicles::br101();
        spec.model = Some(VehicleModel {
            file: "example/assets/br101.gltf".into(),
            parts: vec![
                part("lamp", "lamp:sanding"),
                part("lamp", "lamp:main_switch"),
            ],
            ..VehicleModel::default()
        });
        let findings = check(&spec, &["lamp".to_string()]);
        assert_eq!(keys(&findings), ["check-node-twice"]);
    }

    #[test]
    fn levels_of_detail_have_to_be_one_per_level_and_rise() {
        let mut spec = content::vehicles::br101();
        spec.model = Some(VehicleModel {
            file: "example/assets/br101.gltf".into(),
            lods: vec![
                Lod {
                    level: 0,
                    distance: 150.0,
                },
                Lod {
                    level: 0,
                    distance: 100.0,
                },
                Lod {
                    level: 2,
                    distance: 400.0,
                },
            ],
            ..VehicleModel::default()
        });
        let nodes = vec!["body_LOD0".to_string()];
        let findings = check(&spec, &nodes);
        let mut found = keys(&findings);
        found.sort_unstable();
        assert_eq!(
            found,
            [
                "check-lod-duplicate",
                "check-lod-no-nodes",
                "check-lod-order",
            ]
        );
        // A model built to the convention, listed properly, says nothing.
        spec.model.as_mut().unwrap().lods = vec![
            Lod {
                level: 0,
                distance: 150.0,
            },
            Lod {
                level: 1,
                distance: 400.0,
            },
        ];
        let nodes = vec!["body_LOD0".to_string(), "body_LOD1".to_string()];
        assert!(check(&spec, &nodes).is_empty());
        // …and a model that brings levels the vehicle lists none of draws them
        // all at once.
        spec.model.as_mut().unwrap().lods.clear();
        assert_eq!(keys(&check(&spec, &nodes)), ["check-lod-missing"]);
    }

    #[test]
    fn contradicting_figures_are_found() {
        let mut spec = content::vehicles::br101();
        spec.v_max = 140.0; // the converter pulls to 220
        spec.axles = 0;
        let findings = check(&spec, &[]);
        let mut found = keys(&findings);
        found.sort_unstable();
        assert_eq!(found, ["check-axles", "check-drive-over-v-max"]);

        // A drive whose weight rests on no driven axle carries its force nowhere.
        let mut spec = content::vehicles::br101();
        spec.adhesive_mass_fraction = 0.0;
        assert_eq!(errors(&check(&spec, &[])), ["check-drive-no-adhesion"]);

        // …and the other way round: a coach that claims driven axles.
        let mut spec = content::vehicles::passenger_coach();
        spec.adhesive_mass_fraction = 1.0;
        assert_eq!(keys(&check(&spec, &[])), ["check-adhesion-no-drive"]);

        // Missing compulsory figures.
        let mut spec = content::vehicles::passenger_coach();
        spec.length = 0.0;
        spec.mass_empty = 0.0;
        spec.gauge = 0.0;
        let findings = check(&spec, &[]);
        let found = errors(&findings);
        assert!(found.contains(&"check-length"), "{found:?}");
        assert!(found.contains(&"check-mass"), "{found:?}");
        assert!(found.contains(&"check-gauge"), "{found:?}");
    }

    #[test]
    fn a_wagon_that_doubles_its_weight_without_load_braking_is_reported() {
        let mut spec = content::vehicles::freight_wagon();
        spec.brake.load_braking = LoadBraking::None;
        assert!(keys(&check(&spec, &[])).contains(&"check-load-braking"));
        // A load heavier than the payload is carried only in part.
        spec.loads = vec![LoadSpec {
            name: "Schotter".into(),
            mass: spec.max_payload + 10_000.0,
            node: None,
        }];
        assert!(keys(&check(&spec, &[])).contains(&"check-load-over-payload"));
    }

    #[test]
    fn a_drive_that_out_pulls_the_rail_is_reported() {
        let mut spec = content::vehicles::br101();
        // Twice what the loco was built for — nothing but wheelspin.
        if let sim_core::drive::TractionSpec::Converter { max_force, .. } =
            &mut spec.drives[0].traction
        {
            *max_force = 600_000.0;
        }
        assert!(keys(&check(&spec, &[])).contains(&"check-tractive-effort"));
    }

    /// `t!` hands an unknown key back unchanged, so a rendered message that still
    /// reads like its key is one that never made it into the `.ftl` files.
    #[test]
    fn every_message_exists_in_both_languages() {
        let mut spec = content::vehicles::br101();
        spec.length = 0.0;
        spec.mass_empty = 0.0;
        spec.gauge = 0.0;
        spec.axles = 0;
        spec.rotating_mass_factor = 2.0;
        spec.v_max = 0.0;
        spec.brake.brake_weight = 0.0;
        spec.brake.max_force = 0.0;
        spec.passenger_doors = true;
        spec.doors = DoorSystem::None;
        spec.loads = vec![LoadSpec {
            name: "Kohle".into(),
            mass: 1.0,
            node: Some("gone".into()),
        }];
        spec.max_payload = 0.5;
        spec.model = Some(VehicleModel {
            file: String::new(),
            seats: Vec::new(),
            lods: vec![
                Lod {
                    level: 0,
                    distance: 400.0,
                },
                Lod {
                    level: 0,
                    distance: 150.0,
                },
            ],
            parts: vec![part("gone", "wheel"), part("gone", "pantograph")],
            cab: Some(CabSpec {
                controls: vec![sim_core::cab::CabControlSpec {
                    node: "gone".into(),
                    input: sim_core::cab::CabControl::Throttle,
                    motion: Motion::Visibility,
                }],
                ..CabSpec::default()
            }),
            displays: vec![DisplaySpec {
                name: "mfa".into(),
                node: "gone".into(),
                width: 256,
                height: 160,
                widgets: vec![],
                html: None,
            }],
        });
        // The two figure checks the wreck above cannot trip at the same time as
        // the rest, plus the one that needs a healthy drive.
        let extra = {
            let mut over = content::vehicles::br101();
            over.v_max = 140.0;
            let mut brake = content::vehicles::freight_wagon();
            brake.brake.brake_weight = 500.0;
            let mut load = content::vehicles::freight_wagon();
            load.brake.load_braking = LoadBraking::None;
            let mut effort = content::vehicles::br101();
            effort.adhesive_mass_fraction = 0.05;
            let mut coach = content::vehicles::passenger_coach();
            coach.adhesive_mass_fraction = 1.0;
            let mut lod = content::vehicles::br101();
            lod.model = Some(VehicleModel {
                file: "a.gltf".into(),
                ..VehicleModel::default()
            });
            let mut idle = content::vehicles::br101();
            idle.adhesive_mass_fraction = 0.0;
            [over, brake, load, effort, coach, lod, idle]
        };

        for language in ["en", "de"] {
            i18n::set_language(language);
            let mut findings = check(&spec, &["body_LOD3".to_string()]);
            findings.extend(check(&extra[0], &[]));
            findings.extend(check(&extra[1], &[]));
            findings.extend(check(&extra[2], &[]));
            findings.extend(check(&extra[3], &[]));
            findings.extend(check(&extra[4], &[]));
            findings.extend(check(&extra[5], &["body_LOD0".to_string()]));
            findings.extend(check(&extra[6], &[]));
            let seen: Vec<&str> = findings.iter().map(|f| f.key).collect();
            for key in ALL_KEYS {
                assert!(seen.contains(key), "{key} is never reported ({language})");
            }
            for finding in &findings {
                assert_ne!(
                    finding.text, finding.key,
                    "{} is missing from {language}",
                    finding.key
                );
                assert!(
                    !finding.text.contains('{'),
                    "{}: unfilled placeholder in {language}: {}",
                    finding.key,
                    finding.text
                );
            }
        }
        i18n::set_language("en");
    }

    /// Every key the report can produce — the list the test above walks so a new
    /// check without a translation cannot slip through.
    const ALL_KEYS: &[&str] = &[
        "check-length",
        "check-mass",
        "check-gauge",
        "check-axles",
        "check-rotating-mass",
        "check-load-over-payload",
        "check-doors",
        "check-no-brake-weight",
        "check-no-brake-force",
        "check-brake-percentage",
        "check-load-braking",
        "check-drive-no-adhesion",
        "check-adhesion-no-drive",
        "check-no-v-max",
        "check-drive-over-v-max",
        "check-tractive-effort",
        "check-model-no-file",
        "check-part-node",
        "check-part-function",
        "check-control-node",
        "check-display-node",
        "check-load-node",
        "check-node-twice",
        "check-lod-duplicate",
        "check-lod-order",
        "check-lod-no-nodes",
        "check-lod-missing",
    ];
}
