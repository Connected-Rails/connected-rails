//! Mod runtime: discovery of mods, declarative content, Lua behaviour hooks (plan ch. 19).
//!
//! **Data and behaviour are separate.** Vehicles, lines, scenarios and signal types are RON
//! and are validated while loading — that covers the bulk of every mod. Lua only runs where
//! real behaviour is needed: tap changer logic, AFB, the choice of a signal aspect.
//!
//! A mod is a directory below `mods/`:
//!
//! ```text
//! mods/<id>/mod.ron           manifest (id, name, version, author, depends)
//!          /vehicles/*.ron    VehicleSpec
//!          /blocks/*.ron      block presets for the vehicle editor (sim_core::blocks)
//!          /lines/*.ron       LineSource — a line, or a module with `boundaries`
//!          /compositions/*.ron Composition — modules merged into one line
//!          /scenarios/*.ron   Scenario
//!          /timetable/*.ron   Timetable (referenced by a scenario for stop scoring)
//!          /signals/*.ron     SignalType (state machine, optional script hook)
//!          /signal_models/*.ron SignalModel (glTF parts on mount points, lamp bindings)
//!          /track_types/*.ron TrackType (texture, roughness, superstructure speed, LZB)
//!          /objects/*.ron     TrackObject (3D object with its default pose relative to the track)
//!          /scripts/*.lua     behaviour scripts
//!          /assets/…          models, textures, sounds (asset source `mods://`)
//! ```
//!
//! Everything is addressed as `"<mod>:<file stem>"`, so two mods may use the same file
//! names. Nothing here is fatal: a broken file produces a warning, the rest still loads.

pub mod display;
pub mod script;

use content::Composition;
use content::compose::{Composed, ModuleOffsets};
use content::route::LineSource;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use sim_core::interlock::{Interlock, SignalModel, SignalType};
use sim_core::scenario::{Action, Scenario, Trigger};
use sim_core::timetable::Timetable;
use sim_core::train::VehicleSpec;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use track_model::{TrackNetwork, TrackObject, TrackType};

pub use script::{ModRuntime, Scripts};

/// `mod.ron` of a mod.
#[derive(Debug, Clone, Deserialize)]
pub struct ModManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    /// Mod ids that have to be loaded first.
    #[serde(default)]
    pub depends: Vec<String>,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    /// The mod's directory — not in the file, filled in while reading.
    #[serde(skip)]
    pub dir: PathBuf,
}

fn enabled_by_default() -> bool {
    true
}

impl ModManifest {
    /// Writes `enabled` back into `mod.ron`. Only that one field is touched, so comments
    /// and formatting of the file survive.
    pub fn set_enabled(&mut self, enabled: bool) -> std::io::Result<()> {
        let path = self.dir.join("mod.ron");
        let text = std::fs::read_to_string(&path)?;
        let value = if enabled { "true" } else { "false" };
        let patched = match field_span(&text, "enabled") {
            Some(span) => format!("{}{value}{}", &text[..span.start], &text[span.end..]),
            // No such field yet: the manifest is a RON struct, so it opens with `(`.
            None => match text.find('(') {
                Some(i) => format!("{}\n    enabled: {value},{}", &text[..=i], &text[i + 1..]),
                None => return Err(std::io::ErrorKind::InvalidData.into()),
            },
        };
        std::fs::write(&path, patched)?;
        self.enabled = enabled;
        Ok(())
    }
}

/// Byte range of the value of a `<name>: <value>` line in a RON file. Line-based on
/// purpose: a `description` that mentions the field name must not be hit.
fn field_span(text: &str, name: &str) -> Option<std::ops::Range<usize>> {
    let prefix = format!("{name}:");
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix(prefix.as_str()) {
            let value = rest.trim_start();
            let start =
                offset + (line.len() - trimmed.len()) + prefix.len() + (rest.len() - value.len());
            let end = start + value.find([',', ')', '\r', '\n']).unwrap_or(value.len());
            return Some(start..end);
        }
        offset += line.len();
    }
    None
}

/// Everything the loaded mods contribute.
#[derive(Debug, Default)]
pub struct Mods {
    pub manifests: Vec<ModManifest>,
    pub vehicles: BTreeMap<String, VehicleSpec>,
    /// Block palette: the built-in blocks plus every mod's presets (`blocks/*.ron`).
    pub blocks: sim_core::blocks::Registry,
    pub lines: BTreeMap<String, LineSource>,
    pub compositions: BTreeMap<String, Composition>,
    pub scenarios: BTreeMap<String, Scenario>,
    pub timetables: BTreeMap<String, Timetable>,
    pub signal_types: BTreeMap<String, SignalType>,
    pub signal_models: BTreeMap<String, SignalModel>,
    pub track_types: BTreeMap<String, TrackType>,
    pub objects: BTreeMap<String, TrackObject>,
    /// Lua sources, keyed `"<mod>:<file stem>"`.
    pub scripts: BTreeMap<String, String>,
    /// Everything that went wrong — displayed, never fatal (plan 19.3).
    pub warnings: Vec<String>,
}

impl Mods {
    /// Reads all mods below `root`. A missing directory is not an error.
    pub fn load(root: impl AsRef<Path>) -> Self {
        let mut mods = Mods {
            blocks: sim_core::blocks::Registry::builtin(),
            ..Default::default()
        };
        let Ok(entries) = std::fs::read_dir(root.as_ref()) else {
            return mods;
        };

        let mut pending: Vec<(PathBuf, ModManifest)> = Vec::new();
        for dir in entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
            let file = dir.join("mod.ron");
            if !file.exists() {
                continue;
            }
            match std::fs::read_to_string(&file)
                .map_err(|e| e.to_string())
                .and_then(|t| ron::from_str::<ModManifest>(&t).map_err(|e| e.to_string()))
            {
                Ok(mut man) => {
                    man.dir = dir.clone();
                    if man.enabled {
                        pending.push((dir, man));
                    } else {
                        // Listed, not read — the mod manager has to be able to switch it
                        // back on.
                        mods.manifests.push(man);
                    }
                }
                Err(e) => mods.warnings.push(format!("{}: {e}", file.display())),
            }
        }
        // Alphabetical, so the load order does not depend on the file system.
        pending.sort_by(|a, b| a.1.id.cmp(&b.1.id));

        // Dependencies first; whatever stays unresolvable (missing mod or cycle) is loaded
        // last with a warning instead of being dropped.
        let mut loaded: Vec<String> = Vec::new();
        while !pending.is_empty() {
            let index = pending
                .iter()
                .position(|(_, m)| m.depends.iter().all(|d| loaded.contains(d)));
            let (dir, man) = pending.remove(index.unwrap_or(0));
            if index.is_none() {
                mods.warnings.push(format!(
                    "mod {}: dependency {:?} missing or circular — loaded anyway",
                    man.id, man.depends
                ));
            }
            mods.read_mod(&dir, &man);
            loaded.push(man.id.clone());
            mods.manifests.push(man);
        }
        // Stable order for the mod manager; the load order above is already done with.
        mods.manifests.sort_by(|a, b| a.id.cmp(&b.id));

        // Bake block graphs with the complete palette — after every mod's presets are in.
        let mut warnings = Vec::new();
        for (key, spec) in mods.vehicles.iter_mut() {
            // A file written before the multi-drive split carries a single `traction`.
            spec.normalise();
            let Some(graph) = spec.graph.clone() else {
                continue;
            };
            for issue in sim_core::blocks::bake(&graph, &mods.blocks, spec) {
                if issue.severity == sim_core::blocks::Severity::Error {
                    warnings.push(format!("{key}: block graph: {}", issue.key));
                }
            }
        }
        mods.warnings.append(&mut warnings);
        mods
    }

    /// Dependencies of `id` that are not present as an enabled mod (plan 19.7,
    /// dependency check of the mod manager).
    pub fn missing_depends(&self, id: &str) -> Vec<String> {
        let Some(man) = self.manifests.iter().find(|m| m.id == id) else {
            return Vec::new();
        };
        man.depends
            .iter()
            .filter(|d| !self.manifests.iter().any(|m| m.enabled && m.id == ***d))
            .cloned()
            .collect()
    }

    fn read_mod(&mut self, dir: &Path, man: &ModManifest) {
        let id = &man.id;
        read_ron(
            &dir.join("vehicles"),
            id,
            &mut self.vehicles,
            &mut self.warnings,
        );
        read_ron(&dir.join("lines"), id, &mut self.lines, &mut self.warnings);
        read_ron(
            &dir.join("compositions"),
            id,
            &mut self.compositions,
            &mut self.warnings,
        );
        read_ron(
            &dir.join("scenarios"),
            id,
            &mut self.scenarios,
            &mut self.warnings,
        );
        read_ron(
            &dir.join("timetable"),
            id,
            &mut self.timetables,
            &mut self.warnings,
        );
        read_ron(
            &dir.join("signals"),
            id,
            &mut self.signal_types,
            &mut self.warnings,
        );
        read_ron(
            &dir.join("signal_models"),
            id,
            &mut self.signal_models,
            &mut self.warnings,
        );
        read_ron(
            &dir.join("track_types"),
            id,
            &mut self.track_types,
            &mut self.warnings,
        );
        read_ron(
            &dir.join("objects"),
            id,
            &mut self.objects,
            &mut self.warnings,
        );
        for path in files(&dir.join("scripts"), "lua") {
            insert(path, id, &mut self.scripts, &mut self.warnings, |t| {
                Ok(t.to_string())
            });
        }
        for path in files(&dir.join("blocks"), "ron") {
            let result = std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|t| sim_core::blocks::parse_mod_block(&t).map_err(|e| e.to_string()))
                .and_then(|def| self.blocks.add_mod_block(id, def));
            if let Err(e) = result {
                self.warnings.push(format!("{}: {e}", path.display()));
            }
        }
    }

    /// Resolves a `--line`-style reference: a plain line as it stands, a composition
    /// merged into one line. The notes are worth logging — per-module index offsets,
    /// boundaries that stayed open. A plain line counts as a module of itself with
    /// zero offsets, so module-qualified content works against it too.
    pub fn resolve_line(&self, name: &str) -> Result<Composed, String> {
        if let Some(line) = self.lines.get(name) {
            let mut offsets = BTreeMap::new();
            offsets.insert(name.to_string(), ModuleOffsets::default());
            return Ok(Composed {
                line: line.clone(),
                offsets,
                notes: Vec::new(),
            });
        }
        match self.compositions.get(name) {
            Some(composition) => composition.compose(&self.lines),
            None => Err("not found".into()),
        }
    }

    /// Turns a mod-qualified path (`"<mod>:heights/<line>"`) into a real one
    /// below that mod's directory. `None` if the mod is not installed.
    ///
    /// For files that are read directly rather than through Bevy's asset
    /// system — a module's DGM cut-out is the only one so far.
    pub fn resolve_path(&self, qualified: &str) -> Option<PathBuf> {
        let (id, rest) = qualified.split_once(':')?;
        let man = self.manifests.iter().find(|m| m.id == id)?;
        Some(man.dir.join(rest))
    }

    /// Resolves the signal type names of a line and hangs the types into the interlocking.
    ///
    /// Signals are compiled in source order, so `line.signals[i]` belongs to
    /// `interlock.signals[i]`.
    pub fn apply_signal_types(&self, line: &LineSource, interlock: &mut Interlock) -> Vec<String> {
        let mut warnings = Vec::new();
        for (i, source) in line.signals.iter().enumerate() {
            let Some(name) = source.signal_type.as_deref() else {
                continue;
            };
            let Some(ty) = self.signal_types.get(name) else {
                warnings.push(format!("signal {i}: unknown signal type {name:?}"));
                continue;
            };
            let index = match interlock.types.iter().position(|t| t == ty) {
                Some(index) => index as u32,
                None => interlock.add_type(ty.clone()),
            };
            if let Some(signal) = interlock.signals.get_mut(i) {
                signal.type_index = Some(index);
                signal.system = ty.system;
            }
        }
        warnings
    }

    /// Resolves the track type names of a compiled network against the loaded
    /// `track_types/*.ron` and caps every edge's speed profile with the
    /// superstructure limit (see `TrackNetwork::apply_track_types`).
    pub fn apply_track_types(&self, net: &mut TrackNetwork) -> Vec<String> {
        net.apply_track_types(|name| self.track_types.get(name).cloned())
    }

    /// Name of the 3D model of signal `index`: the placement's override wins over the
    /// signal type's default. `None` — the signal has no model and stays a placeholder.
    pub fn signal_model_name<'a>(&'a self, line: &'a LineSource, index: usize) -> Option<&'a str> {
        let source = line.signals.get(index)?;
        source.model.as_deref().or_else(|| {
            self.signal_types
                .get(source.signal_type.as_deref()?)?
                .model
                .as_deref()
        })
    }
}

/// Resolves the module-qualified indices of a scenario against the composed line's
/// offsets, in place. `module` on the scenario is the default, `module` on an event
/// overrides it; the fields are cleared afterwards, so applying this twice cannot shift
/// twice. An unknown module name leaves the event untouched and comes back as a warning.
pub fn qualify_scenario(
    scenario: &mut Scenario,
    offsets: &BTreeMap<String, ModuleOffsets>,
) -> Vec<String> {
    let mut warnings = Vec::new();
    let default = scenario.module.take();
    for event in &mut scenario.events {
        let Some(module) = event.module.take().or_else(|| default.clone()) else {
            continue;
        };
        let Some(off) = offsets.get(&module) else {
            warnings.push(format!("event {}: unknown module {module}", event.name));
            continue;
        };
        shift_trigger(&mut event.trigger, off);
        for action in &mut event.actions {
            shift_action(action, off);
        }
    }
    warnings
}

/// The timetable counterpart of [`qualify_scenario`]: shifts each stop's `edge` by its
/// module's offset. Same rules — per-stop `module` beats the timetable's, the fields
/// are cleared, unknown modules warn and leave the stop alone.
pub fn qualify_timetable(
    timetable: &mut Timetable,
    offsets: &BTreeMap<String, ModuleOffsets>,
) -> Vec<String> {
    let mut warnings = Vec::new();
    let default = timetable.module.take();
    for stop in &mut timetable.stops {
        let Some(module) = stop.module.take().or_else(|| default.clone()) else {
            continue;
        };
        let Some(off) = offsets.get(&module) else {
            warnings.push(format!("stop {}: unknown module {module}", stop.name));
            continue;
        };
        stop.edge.0 += off.edges;
    }
    warnings
}

fn shift_trigger(trigger: &mut Trigger, off: &ModuleOffsets) {
    match trigger {
        Trigger::TrainPast { edge, .. } | Trigger::TrainStopped { edge, .. } => {
            edge.0 += off.edges;
        }
        Trigger::SignalStop { signal, .. } => signal.0 += off.signals,
        Trigger::All(list) | Trigger::Any(list) => {
            for t in list {
                shift_trigger(t, off);
            }
        }
        _ => {}
    }
}

fn shift_action(action: &mut Action, off: &ModuleOffsets) {
    match action {
        Action::SetSwitch { node, .. } => node.0 += off.nodes,
        Action::RequestRoute(route) | Action::ReleaseRoute(route) => route.0 += off.routes,
        _ => {}
    }
}

/// All files with the given extension in a directory, sorted. A missing directory is empty.
fn files(dir: &Path, extension: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == extension))
        .collect();
    files.sort();
    files
}

/// Reads every `*.ron` of a directory into `into`, keyed `"<mod>:<file stem>"`.
fn read_ron<T: DeserializeOwned>(
    dir: &Path,
    mod_id: &str,
    into: &mut BTreeMap<String, T>,
    warnings: &mut Vec<String>,
) {
    for path in files(dir, "ron") {
        insert(path, mod_id, into, warnings, |text| {
            ron::from_str(text).map_err(|e: ron::error::SpannedError| e.to_string())
        });
    }
}

fn insert<T>(
    path: PathBuf,
    mod_id: &str,
    into: &mut BTreeMap<String, T>,
    warnings: &mut Vec<String>,
    parse: impl Fn(&str) -> Result<T, String>,
) {
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return;
    };
    let key = format!("{mod_id}:{stem}");
    match std::fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|text| parse(&text))
    {
        Ok(value) => {
            if into.insert(key.clone(), value).is_some() {
                warnings.push(format!("{key} defined twice — the later one wins"));
            }
        }
        Err(e) => warnings.push(format!("{}: {e}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The example mod shipped with the repository.
    fn example_mods() -> Mods {
        Mods::load(concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods"))
    }

    #[test]
    fn example_mod_loads_without_warnings() {
        let mods = example_mods();
        assert!(mods.warnings.is_empty(), "warnings: {:?}", mods.warnings);
        assert!(mods.manifests.iter().any(|m| m.id == "example"));
        assert!(mods.vehicles.contains_key("example:br101_afb"));
        assert!(mods.signal_types.contains_key("example:ks_main"));
        assert!(mods.objects.contains_key("example:mast"));
        // The example line places that object; the check knows the registry.
        let line = &mods.lines["example:beispielstrecke"];
        assert_eq!(line.objects.len(), 2);
        assert!(
            line.check(&mods.track_types, &mods.objects).is_empty(),
            "{:?}",
            line.check(&mods.track_types, &mods.objects)
        );
        assert!(mods.scripts.contains_key("example:afb"));
        // The scenario references its timetable, and the timetable is loaded.
        let scenario = &mods.scenarios["example:probefahrt"];
        let name = scenario.timetable.as_deref().expect("timetable reference");
        let timetable = &mods.timetables[name];
        assert!(!timetable.stops.is_empty());
        assert!(mods.compositions.contains_key("example:gesamtstrecke"));
    }

    /// The example line's track types resolve against `track_types/*.ron`:
    /// the specs replace the placeholders and the superstructure limit caps
    /// the speed profile.
    #[test]
    fn track_types_resolve_and_cap() {
        let mods = example_mods();
        assert!(mods.track_types.contains_key("example:hauptbahn"));
        let line = &mods.lines["example:beispielstrecke"];
        let mut net = line.compile().expect("compiles").net;
        let warnings = mods.apply_track_types(&mut net);
        assert!(warnings.is_empty(), "{warnings:?}");

        let edge = track_model::EdgeId(0);
        assert_eq!(net.track_type_at(edge, 1000.0).roughness, 1.0);
        assert_eq!(net.track_type_at(edge, 3500.0).roughness, 1.4);
        // Altbau allows the 120 km/h the line runs — the profile is unchanged.
        assert_eq!(net.edges()[0].speed.at(3500.0), 120.0);

        // An unknown name warns and keeps default properties.
        let mut line = line.clone();
        line.edges[0].track_type = vec![(0.0, "example:fehlt".into())];
        let mut net = line.compile().unwrap().net;
        let warnings = mods.apply_track_types(&mut net);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("example:fehlt"));
    }

    /// A signal's 3D model: the placement's override wins over the type's default,
    /// and the model file itself is loaded from `signal_models/`.
    #[test]
    fn signal_models_resolve_placement_over_type_default() {
        let mods = example_mods();
        let model = &mods.signal_models["example:ks_mast"];
        assert_eq!(model.parts.len(), 3);
        assert_eq!(model.parts[1].mount, Some((0, "mp_schirm".into())));
        assert!(model.lamps.iter().any(|l| l.lamp == "zs3_4" && l.part == 2));

        // The example line's signal takes the type's default model …
        let line = &mods.lines["example:beispielstrecke"];
        assert_eq!(mods.signal_model_name(line, 0), Some("example:ks_mast"));
        // … and an override on the placement wins over it.
        let mut line = line.clone();
        line.signals[0].model = Some("example:other".into());
        assert_eq!(mods.signal_model_name(&line, 0), Some("example:other"));
        // Out of range or no type: no model.
        assert_eq!(mods.signal_model_name(&line, 99), None);
    }

    /// The example composition merges its two modules into one line, connected by
    /// nothing but the shared boundary coordinates.
    #[test]
    fn a_composition_merges_modules_into_one_line() {
        let mods = example_mods();
        let composed = mods
            .resolve_line("example:gesamtstrecke")
            .expect("composes");
        let line = &composed.line;
        assert_eq!(line.edges.len(), 2);
        // The seam is fused: the second module's edge continues at the first one's end.
        assert_eq!(line.edges[1].from, line.edges[0].to);
        assert!(composed.notes.iter().any(|n| n.contains("edges +1")));
        // The second module's magnet follows its shifted signal index.
        assert!(line.devices[3].payload.contains("signal:Some(1)"));
        // The composition's signal link crosses the boundary.
        assert_eq!(line.signals[0].next, Some(1));
        let compiled = line.compile().expect("composed line compiles");
        assert_eq!(compiled.interlock.signals.len(), 2);
    }

    /// Module-qualified indices in scenario and timetable resolve against the
    /// composition's offsets — no offset arithmetic in the content files.
    #[test]
    fn module_qualified_references_resolve_against_the_composition() {
        let mods = example_mods();
        let composed = mods
            .resolve_line("example:gesamtstrecke")
            .expect("composes");

        let mut scenario = mods.scenarios["example:modulfahrt"].clone();
        assert!(qualify_scenario(&mut scenario, &composed.offsets).is_empty());
        match &scenario.events[0].trigger {
            sim_core::scenario::Trigger::TrainStopped { edge, .. } => assert_eq!(edge.0, 1),
            other => panic!("unexpected trigger {other:?}"),
        }

        let mut timetable = mods.timetables["example:modulfahrt"].clone();
        assert!(qualify_timetable(&mut timetable, &composed.offsets).is_empty());
        assert_eq!(timetable.stops[0].edge.0, 1);
        // Applying it twice cannot shift twice — the module fields are consumed.
        qualify_timetable(&mut timetable, &composed.offsets);
        assert_eq!(timetable.stops[0].edge.0, 1);
    }

    #[test]
    fn vehicle_from_a_mod_names_its_script() {
        let mods = example_mods();
        let loco = &mods.vehicles["example:br101_afb"];
        assert_eq!(loco.script.as_deref(), Some("example:afb"));
        // The physical data stay declarative — no script involved.
        assert!(loco.mass_empty > 80_000.0);
        assert!(loco.powered());
        // Train protection and door control are equipment of the vehicle, from the RON.
        assert!(
            matches!(
                loco.safety,
                sim_core::safety::SafetyEquipment::De {
                    pzb: Some(_),
                    lzb: true,
                    sifa: Some(_),
                    ..
                }
            ),
            "{:?}",
            loco.safety
        );
        assert_eq!(loco.doors, sim_core::doors::DoorSystem::Tb0);
        assert!(!loco.safety.build().indicators().is_empty());
    }

    /// The sound table comes out of the vehicle file like everything else — a loop that is
    /// modulated, a loop held by conditions, and one entry with a trigger.
    #[test]
    fn a_modded_vehicle_brings_its_sound_table() {
        use sim_core::sound::{Quantity, SoundState, Trigger};

        let mods = example_mods();
        let loco = &mods.vehicles["example:br101_afb"];
        let entry = |name: &str| {
            loco.sounds
                .iter()
                .find(|s| s.name == name)
                .unwrap_or_else(|| panic!("{name} missing"))
        };

        // Rolling noise is three crossfaded layers; at 120 km/h the middle one carries it
        // and the top one is fading in, and neither is stretched far from its own pitch.
        let fast = SoundState {
            speed: 120.0,
            ..SoundState::default()
        };
        let rolling: Vec<_> = ["rolling-low", "rolling-mid", "rolling-high"]
            .map(entry)
            .into_iter()
            .collect();
        assert!(rolling.iter().all(|layer| layer.is_loop()));
        assert_eq!(
            rolling[0].level(&fast).0,
            0.0,
            "the low band is out by then"
        );
        let audible: f64 = rolling.iter().map(|layer| layer.level(&fast).0).sum();
        assert!(audible > 0.0);
        for layer in &rolling {
            assert!(
                (0.8..=1.35).contains(&layer.level(&fast).1),
                "{} is resampled too far",
                layer.name
            );
        }

        // Squeal is a condition on a loop: braking slowly yes, rolling fast no.
        let squeal = entry("brake-squeal");
        let braking = SoundState {
            speed: 8.0,
            brake_effort: 60.0,
            ..SoundState::default()
        };
        assert!(squeal.level(&braking).0 > 0.0);
        assert_eq!(squeal.level(&fast).0, 0.0);

        // The joint is the only entry that is an event.
        let joint = entry("rail-joint");
        assert_eq!(
            joint.trigger,
            Trigger::Every {
                quantity: Quantity::Distance,
                interval: 30.0
            }
        );
    }

    /// The whole chain: line from a mod → compile → signal type → aspect from the table.
    #[test]
    fn a_modded_line_gets_its_aspect_from_the_rule_table() {
        use sim_core::Sim;
        use sim_core::interlock::MainAspect;
        use track_model::EdgeId;

        let mods = example_mods();
        let line = &mods.lines["example:beispielstrecke"];
        let mut compiled = line.compile().expect("line compiles");
        assert!(
            mods.apply_signal_types(line, &mut compiled.interlock)
                .is_empty()
        );
        assert_eq!(compiled.interlock.signals[0].type_index, Some(0));

        let mut sim = Sim::new(compiled.net, compiled.interlock, 1);
        sim.step(Sim::DT);
        assert_eq!(
            sim.interlock.signals[0].aspect.main,
            Some(MainAspect::Proceed)
        );
        assert_eq!(sim.interlock.signals[0].lamps, ["green"]);

        sim.interlock.update_occupancy(&[EdgeId(0)]);
        sim.interlock.update(&mut sim.net);
        assert_eq!(sim.interlock.signals[0].aspect.main, Some(MainAspect::Stop));
        assert_eq!(sim.interlock.signals[0].lamps, ["red"]);
    }

    /// What the table cannot express, the hook can: Zs1 after three minutes at stop.
    #[test]
    fn a_signal_script_can_override_the_rule_table() {
        use sim_core::Sim;
        use sim_core::interlock::MainAspect;
        use track_model::EdgeId;

        let mut runtime = ModRuntime::load(concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods"));
        let line = runtime.mods.lines["example:beispielstrecke"].clone();
        let mut compiled = line.compile().expect("line compiles");
        let ty = runtime.mods.signal_types["example:ks_main_zs1"].clone();
        let index = compiled.interlock.add_type(ty);
        compiled.interlock.signals[0].type_index = Some(index);

        let mut sim = Sim::new(compiled.net, compiled.interlock, 1);
        sim.interlock.update_occupancy(&[EdgeId(0)]);
        sim.interlock.update(&mut sim.net);
        runtime.post_step(&mut sim, 0.0);
        assert_eq!(sim.interlock.signals[0].aspect.main, Some(MainAspect::Stop));

        sim.time = 200.0;
        sim.interlock.update(&mut sim.net);
        runtime.post_step(&mut sim, 0.0);
        assert_eq!(
            sim.interlock.signals[0].aspect.main,
            Some(MainAspect::Substitute)
        );
        assert_eq!(sim.interlock.signals[0].aspect.speed, Some(40.0));
        assert!(runtime.log().is_empty(), "log: {:?}", runtime.log());
    }

    /// A mod's `blocks/*.ron` presets join the palette under the `<mod>:` prefix, and a
    /// broken preset warns instead of failing the load.
    #[test]
    fn mod_block_presets_join_the_registry() {
        let root = std::env::temp_dir().join("trainsim-mods-blockdefs");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("m").join("blocks")).unwrap();
        std::fs::write(root.join("m").join("mod.ron"), "(id: \"m\", name: \"M\")").unwrap();
        std::fs::write(
            root.join("m").join("blocks").join("l620.ron"),
            "(id: \"l620\", name: \"Voith L 620 reU2\", base: \"hydro-transmission\", \
             params: { \"final_ratio\": Number(2.73) })",
        )
        .unwrap();
        std::fs::write(
            root.join("m").join("blocks").join("broken.ron"),
            "(id: \"x\", name: \"X\", base: \"warp-drive\")",
        )
        .unwrap();
        let mods = Mods::load(&root);
        let preset = mods.blocks.get("m:l620").expect("preset registered");
        assert_eq!(preset.name, "Voith L 620 reU2");
        assert_eq!(mods.blocks.base_kind("m:l620"), Some("hydro-transmission"));
        assert_eq!(
            mods.blocks.default_of("m:l620", "final_ratio"),
            Some(sim_core::blocks::ParamValue::Number(2.73))
        );
        assert_eq!(mods.warnings.len(), 1, "{:?}", mods.warnings);
        assert!(
            mods.warnings[0].contains("warp-drive"),
            "{:?}",
            mods.warnings
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_mods_directory_is_not_an_error() {
        let mods = Mods::load("does/not/exist");
        assert!(mods.manifests.is_empty());
        assert!(mods.warnings.is_empty());
    }

    /// Writes two mods into a scratch directory: `a` is off, `b` depends on `a`.
    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("trainsim-mods-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        for (id, text) in [
            (
                "a",
                "(\n    id: \"a\",\n    // a comment the mod manager must not eat\n    \
                 name: \"A\",\n    enabled: false,\n)\n",
            ),
            (
                "b",
                "(\n    id: \"b\",\n    name: \"B\",\n    depends: [\"a\"],\n)\n",
            ),
        ] {
            std::fs::create_dir_all(root.join(id)).unwrap();
            std::fs::write(root.join(id).join("mod.ron"), text).unwrap();
        }
        root
    }

    /// The mod manager needs to see switched-off mods, otherwise it cannot switch them on.
    #[test]
    fn disabled_mods_are_listed_but_not_read() {
        let root = scratch("listed");
        let mods = Mods::load(&root);
        let ids: Vec<&str> = mods.manifests.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["a", "b"]);
        assert!(!mods.manifests[0].enabled);
        // `b` depends on the switched-off `a` — that is what the dependency check reports.
        assert_eq!(mods.missing_depends("b"), ["a"]);
        assert!(mods.missing_depends("a").is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Switching writes back into `mod.ron` — and touches nothing else in the file.
    #[test]
    fn enabling_a_mod_patches_only_that_field() {
        let root = scratch("toggle");
        let mut mods = Mods::load(&root);
        mods.manifests[0].set_enabled(true).unwrap();

        let text = std::fs::read_to_string(root.join("a").join("mod.ron")).unwrap();
        assert!(text.contains("enabled: true,"), "{text}");
        assert!(
            text.contains("a comment the mod manager must not eat"),
            "{text}"
        );

        // `b` has no `enabled` field at all — switching it off has to add one.
        mods.manifests[1].set_enabled(false).unwrap();
        let reloaded = Mods::load(&root);
        assert!(reloaded.manifests[0].enabled);
        assert!(!reloaded.manifests[1].enabled);
        assert!(reloaded.warnings.is_empty(), "{:?}", reloaded.warnings);
        let _ = std::fs::remove_dir_all(&root);
    }
}
