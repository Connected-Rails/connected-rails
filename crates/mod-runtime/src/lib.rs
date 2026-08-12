//! Mod runtime: discovery of mods, declarative content, Lua behaviour hooks (plan ch. 19).
//!
//! **Data and behaviour are separate.** Vehicles, lines, scenarios and signal types are RON
//! and are validated while loading — that covers the bulk of every mod. Lua only runs where
//! real behaviour is needed: tap changer logic, AFB, the choice of a signal aspect.
//!
//! A mod is a directory below `mods/`:
//!
//! ```text
//! mods/<id>/mod.ron          manifest (id, name, version, author, depends)
//!          /vehicles/*.ron   VehicleSpec
//!          /lines/*.ron      LineSource
//!          /scenarios/*.ron  Scenario
//!          /signals/*.ron    SignalType (state machine, optional script hook)
//!          /scripts/*.lua    behaviour scripts
//!          /assets/…         models, textures, sounds (asset source `mods://`)
//! ```
//!
//! Everything is addressed as `"<mod>:<file stem>"`, so two mods may use the same file
//! names. Nothing here is fatal: a broken file produces a warning, the rest still loads.

pub mod script;

use content::route::LineSource;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use sim_core::interlock::{Interlock, SignalType};
use sim_core::scenario::Scenario;
use sim_core::train::VehicleSpec;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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
}

fn enabled_by_default() -> bool {
    true
}

/// Everything the loaded mods contribute.
#[derive(Debug, Default)]
pub struct Mods {
    pub manifests: Vec<ModManifest>,
    pub vehicles: BTreeMap<String, VehicleSpec>,
    pub lines: BTreeMap<String, LineSource>,
    pub scenarios: BTreeMap<String, Scenario>,
    pub signal_types: BTreeMap<String, SignalType>,
    /// Lua sources, keyed `"<mod>:<file stem>"`.
    pub scripts: BTreeMap<String, String>,
    /// Everything that went wrong — displayed, never fatal (plan 19.3).
    pub warnings: Vec<String>,
}

impl Mods {
    /// Reads all mods below `root`. A missing directory is not an error.
    pub fn load(root: impl AsRef<Path>) -> Self {
        let mut mods = Mods::default();
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
                Ok(man) if man.enabled => pending.push((dir, man)),
                Ok(_) => {}
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
        mods
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
            &dir.join("scenarios"),
            id,
            &mut self.scenarios,
            &mut self.warnings,
        );
        read_ron(
            &dir.join("signals"),
            id,
            &mut self.signal_types,
            &mut self.warnings,
        );
        for path in files(&dir.join("scripts"), "lua") {
            insert(path, id, &mut self.scripts, &mut self.warnings, |t| {
                Ok(t.to_string())
            });
        }
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
        assert!(mods.scripts.contains_key("example:afb"));
    }

    #[test]
    fn vehicle_from_a_mod_names_its_script() {
        let mods = example_mods();
        let loco = &mods.vehicles["example:br101_afb"];
        assert_eq!(loco.script.as_deref(), Some("example:afb"));
        // The physical data stay declarative — no script involved.
        assert!(loco.mass_empty > 80_000.0);
        assert!(loco.traction.is_some());
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

    #[test]
    fn missing_mods_directory_is_not_an_error() {
        let mods = Mods::load("does/not/exist");
        assert!(mods.manifests.is_empty());
        assert!(mods.warnings.is_empty());
    }
}
