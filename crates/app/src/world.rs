//! Building the simulation out of the loaded mods: line, trains, scenario.
//!
//! Split out of `setup` because the dedicated server (`net::run_dedicated`) needs exactly
//! this and nothing that follows it — no terrain, no scenery, no window. Client and server
//! must land on the same world, so both go through here.

use crate::{arg, spawn_train};
use ai_driver::{AiDriver, ScheduledStop, Timetable, TimetableKind};
use bevy::prelude::*;
use content::vehicles::br101;
use content::{LineSource, musterbahn, re_4711, to_musterstadt};
use mod_runtime::ModRuntime;
use sim_core::Sim;
use track_model::{EdgeId, TrackPosition};

/// Everything the run is built out of, before anything is drawn.
pub struct World {
    pub sim: Sim,
    /// Train the player drives in single player; in multiplayer the server reassigns it.
    pub player: usize,
    pub drivers: Vec<(usize, AiDriver)>,
    pub line: LineSource,
}

/// Fingerprint of the world, so client and server can tell they built the same one.
/// Line name and the consists are what a mismatch shows up in first.
pub fn fingerprint(line: &str, sim: &Sim) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |x: u64| {
        h ^= x;
        h = h.wrapping_mul(0x100_0000_01b3);
    };
    for byte in line.as_bytes() {
        mix(u64::from(*byte));
    }
    mix(sim.trains.len() as u64);
    for train in &sim.trains {
        mix(train.vehicles.len() as u64);
        // Variant and load decide which model and which mass a vehicle has. The
        // server builds its world from the scenario and a client from its menu, so
        // the two can disagree here — and a disagreement over liveries and loads is
        // to be refused at join, not discovered by two players describing different
        // trains to each other. Nothing replicates them: they belong to the consist,
        // and the consist is the server's (CLAUDE.md, ch. 20).
        for vehicle in &train.vehicles {
            mix(vehicle.variant.map_or(u64::MAX, |v| v as u64));
            mix(vehicle.load_index.map_or(u64::MAX, |l| l as u64));
        }
    }
    h
}

/// Builds line, trains and scenario. `selection` comes from the menu; CLI flags win over
/// it, so the documented command line invocations stay non-interactive.
pub fn build(mods: &mut ModRuntime, selection: &crate::menu::Selection) -> World {
    let scenario_id = arg("--scenario").or_else(|| selection.scenario_id.clone());
    let line_ref = arg("--line")
        .or_else(|| selection.line_ref.clone())
        .or_else(|| {
            scenario_id
                .as_ref()
                .and_then(|id| mods.mods.scenarios.get(id))
                .and_then(|s| s.line.clone())
        });
    let resolved = line_ref.and_then(|id| match mods.mods.resolve_line(&id) {
        Ok(composed) => {
            for note in &composed.notes {
                info!("{id}: {note}");
            }
            Some(composed)
        }
        Err(e) => {
            warn!("line {id}: {e} — using the example line");
            None
        }
    });
    let modded = resolved.is_some();
    let module_offsets = resolved
        .as_ref()
        .map(|c| c.offsets.clone())
        .unwrap_or_default();
    let line_source = resolved.map(|c| c.line).unwrap_or_else(musterbahn);
    let mut line = line_source.compile().expect("line compiles");
    for warning in mods
        .mods
        .apply_signal_types(&line_source, &mut line.interlock)
    {
        warn!("{}: {warning}", line_source.name);
    }
    // Track types: specs behind the names, and the superstructure speed cap
    // merged into the one profile AI, LZB, HUD and scoring read.
    for warning in mods.mods.apply_track_types(&mut line.net) {
        warn!("{}: {warning}", line_source.name);
    }
    let mut sim = Sim::new(line.net, line.interlock, 2024);

    // Vehicle from menu selection or CLI flag.
    let loco = arg("--loco")
        .or_else(|| selection.loco_id.clone())
        .and_then(|id| match mods.mods.vehicles.get(&id) {
            Some(spec) => Some(spec.clone()),
            None => {
                warn!("vehicle {id} not found — using the BR 101");
                None
            }
        })
        .unwrap_or_else(br101);

    let player = spawn_train(&mut sim, TrackPosition::new(EdgeId(0), 200.0, 1), 5, loco);
    // The livery picked in the menu. It rides on the vehicle, not on a render
    // component, so it is part of the consist the fingerprint below compares.
    sim.trains[player].vehicles[0].variant = selection.variant;

    // Second train, timetable and scenario belong to the example line — a modded line
    // brings its own scenario or none at all.
    let mut drivers = Vec::new();
    if !modded {
        let ai_train = spawn_train(
            &mut sim,
            TrackPosition::new(EdgeId(1), 400.0, 1),
            3,
            br101(),
        );
        drivers.push((
            ai_train,
            AiDriver::new(Timetable {
                number: "RB 20".into(),
                category: "RB".into(),
                kind: TimetableKind::Scenario,
                module: None,
                stops: vec![ScheduledStop {
                    name: "Musterstadt".into(),
                    edge: EdgeId(2),
                    s: 2600.0,
                    arrival: 300.0,
                    departure: 360.0,
                    platform: "1".into(),
                    module: None,
                }],
            }),
        ));

        // Load the scenario with timetable and scoring (plan 11.4).
        let mut scenario = to_musterstadt();
        scenario.player_train = player;
        sim.set_scenario(scenario, re_4711());
    }

    // `--scenario <mod>:<name>` runs a scenario out of a mod. A `timetable/*.ron` the
    // scenario references adds stop scoring; without one only the scenario points count.
    if let Some(id) = scenario_id {
        match mods.mods.scenarios.get(&id) {
            Some(scenario) => {
                let mut scenario = scenario.clone();
                scenario.player_train = player;
                for warning in mod_runtime::qualify_scenario(&mut scenario, &module_offsets) {
                    warn!("scenario {id}: {warning}");
                }
                let timetable = scenario
                    .timetable
                    .as_deref()
                    .and_then(|name| {
                        let timetable = mods.mods.timetables.get(name).cloned();
                        if timetable.is_none() {
                            warn!("scenario {id}: timetable {name:?} not found");
                        }
                        timetable
                    })
                    .map(|mut timetable| {
                        for warning in
                            mod_runtime::qualify_timetable(&mut timetable, &module_offsets)
                        {
                            warn!("scenario {id}: {warning}");
                        }
                        timetable
                    })
                    .unwrap_or_else(|| sim_core::timetable::Timetable {
                        number: scenario.name.clone(),
                        ..default()
                    });
                sim.set_scenario(scenario, timetable);
            }
            None => warn!("scenario {id} not found"),
        }
    }

    // Line and scenario hooks: `on_load` now, `on_frame` every frame (plan 19.7).
    mods.begin(&mut sim, &line_source);
    World {
        sim,
        player,
        drivers,
        line: line_source,
    }
}
