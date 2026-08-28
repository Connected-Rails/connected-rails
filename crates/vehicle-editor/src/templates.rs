//! Reference vehicles as starting points for a new file.
//!
//! `content::vehicles` already carries worked examples of every traction
//! model; New from template hands one of them over instead of an empty spec.

use bevy_egui::egui;
use content::vehicles;
use i18n::t;
use sim_core::train::VehicleSpec;

/// One line of the menu: type designation, tooltip key, reference vehicle.
///
/// The designation stays a literal — "BR 101" and "Eaos" are names, not prose
/// (see CLAUDE.md). It is also the name the new vehicle starts under, see
/// [`spec`].
type Template = (&'static str, &'static str, fn() -> VehicleSpec);

/// Grouped by what the user is building: something that pulls, or something
/// that gets pulled. Every traction model of `content::vehicles` appears once.
const GROUPS: &[(&str, &[Template])] = &[
    (
        "tpl-group-powered",
        &[
            ("BR 101", "tpl-br101-hint", vehicles::br101),
            ("BR 110", "tpl-br110-hint", vehicles::br110),
            ("BR 218", "tpl-br218-hint", vehicles::br218),
            ("BR 232", "tpl-br232-hint", vehicles::br232),
            ("BR 52", "tpl-br52-hint", vehicles::br52),
            ("BR 648", "tpl-railcar-hint", vehicles::railcar),
        ],
    ),
    (
        "tpl-group-hauled",
        &[
            ("Bnrz", "tpl-coach-hint", vehicles::passenger_coach),
            ("Eaos", "tpl-eaos-hint", vehicles::freight_wagon),
            (
                "Eaos (K-GP)",
                "tpl-eaos-k-hint",
                vehicles::freight_wagon_k_valve,
            ),
        ],
    ),
];

/// The vehicle a template starts the user off with.
///
/// The data is handed over as it is; what belongs to the prototype is stripped
/// (`templates_bring_nothing_of_the_prototype_along`): the BR 101 wears the
/// example mod's glTF, cab, displays and sound table, and a template that
/// drags a foreign model path along is a broken preview on the first frame.
/// None of the reference vehicles carries a script or a diagram, and their
/// data sheet entries are empty. The name does belong to the prototype too —
/// saved unchanged it would put a second "BR 101" into the vehicle browser.
/// It keeps the designation instead of falling back to the blank spec's name
/// because the status line names the template through this field, and the
/// suffix is what tells the user which field is theirs to fill in.
fn spec(&(name, _, make): &Template) -> VehicleSpec {
    VehicleSpec {
        name: t!("tpl-name", base = name),
        model: None,
        sounds: Vec::new(),
        ..make()
    }
}

/// Draws the template list. Returns the chosen vehicle, `None` while nothing
/// is picked.
///
/// The block diagram needs nothing here: a spec without one gets it
/// synthesised in the same frame (`ui::vehicle_editor_ui`), and no reference
/// vehicle brings one along.
pub fn menu(ui: &mut egui::Ui) -> Option<VehicleSpec> {
    let mut chosen = None;
    for (index, (group, templates)) in GROUPS.iter().enumerate() {
        if index > 0 {
            ui.separator();
        }
        ui.label(editor_ui::section_title(t!(group)));
        for template in *templates {
            // The tooltip is the point of the list: "BR 218" alone says
            // nothing about which drive, brake and train protection come with
            // it, and that is what the choice turns on.
            if ui
                .button(template.0)
                .on_hover_text(t!(template.1))
                .clicked()
            {
                chosen = Some(spec(template));
            }
        }
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::blocks::{Registry, Severity, bake, from_spec};
    use sim_core::brakes::{BrakeKind, BrakeSpec};

    fn templates() -> impl Iterator<Item = &'static Template> {
        GROUPS.iter().flat_map(|(_, templates)| templates.iter())
    }

    /// The editor turns every vehicle into a block diagram when it opens it and
    /// bakes the diagram back over the spec. A template the palette cannot
    /// express would lose its drive, brake or train protection at the first
    /// save, quietly.
    #[test]
    fn every_template_survives_the_block_diagram() {
        let reg = Registry::builtin();
        for template in templates() {
            let spec = spec(template);
            let graph = from_spec(&spec, &reg);
            let mut baked = spec.clone();
            // Cleared first, so the comparison shows what the diagram carries
            // rather than what was in the spec anyway.
            baked.drives.clear();
            baked.brake = BrakeSpec::from_brake_weight(1.0, BrakeKind::Block);
            baked.safety = Default::default();
            let issues = bake(&graph, &reg, &mut baked);
            let errors: Vec<_> = issues
                .iter()
                .filter(|i| i.severity == Severity::Error)
                .collect();
            assert!(errors.is_empty(), "{}: {errors:?}", template.0);
            assert_eq!(baked.drives, spec.drives, "{}", template.0);
            assert_eq!(baked.brake, spec.brake, "{}", template.0);
            assert_eq!(baked.safety, spec.safety, "{}", template.0);
        }
    }

    /// Mass and length divide in the resistance and the consist list; a
    /// template starting at zero would hand the user a vehicle that cannot run.
    #[test]
    fn every_template_has_mass_and_length() {
        for template in templates() {
            let spec = spec(template);
            assert!(spec.mass_empty > 0.0, "{}", template.0);
            assert!(spec.length > 0.0, "{}", template.0);
            assert!(spec.name.contains(template.0), "{}", spec.name);
        }
    }

    /// The reference vehicles are generic enough to be handed over as they are.
    /// Should one of them gain a model or a sound table of its own, [`spec`] has
    /// to strip it — a template that drags a foreign glTF path along is a
    /// broken preview on the first frame.
    #[test]
    fn templates_bring_nothing_of_the_prototype_along() {
        for template in templates() {
            let spec = spec(template);
            assert!(spec.model.is_none(), "{}", template.0);
            assert!(spec.script.is_none(), "{}", template.0);
            assert!(spec.sounds.is_empty(), "{}", template.0);
            assert!(spec.variants.is_empty(), "{}", template.0);
            assert!(spec.meta.is_empty(), "{}", template.0);
        }
    }

    /// A key missing from one language fails `cargo test -p i18n`; a key
    /// missing from both fails only here, by putting `tpl-br101-hint` on screen
    /// instead of a tooltip.
    #[test]
    fn every_key_of_the_menu_is_translated() {
        for (group, _) in GROUPS {
            assert!(i18n::maybe(group).is_some(), "{group}");
        }
        for template in templates() {
            assert!(i18n::maybe(template.1).is_some(), "{}", template.1);
        }
        // Not `maybe`: a message with a placeholder has none without its
        // argument, so the name key is asked for the way [`spec`] asks for it.
        let name = t!("tpl-name", base = "BR 101");
        assert!(name.contains("BR 101"), "{name}");
    }
}
