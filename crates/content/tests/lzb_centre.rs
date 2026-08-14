//! LZB centre: the movement authority follows the block division of the line data
//! (plan ch. 9.4) — headless.

use content::musterbahn;
use content::vehicles::br101;
use sim_core::Sim;
use sim_core::safety::SafetySystems;
use sim_core::safety::de::{LzbBlockMode, LzbSection, lzb};
use sim_core::train::{Train, Vehicle};
use track_model::{DeviceKind, EdgeId, TrackPosition};

/// A line conductor section as the Musterbahn carries it.
fn section() -> LzbSection {
    LzbSection {
        length: 4000.0,
        cir_elke: false,
        end: false,
    }
}

/// Head of the train at the start of the LZB area (start of edge 1).
fn in_the_lzb_area() -> TrackPosition {
    TrackPosition::new(EdgeId(1), 0.0, 1)
}

#[test]
fn the_authority_runs_to_the_first_occupied_block() {
    let mut line = musterbahn().compile().expect("line compiles");
    let free = lzb::authority(&line.net, &line.interlock, in_the_lzb_area(), &section());
    assert!(
        free.target_distance > 3900.0,
        "nothing in the way: the authority runs on to the end of the line, {} m",
        free.target_distance
    );

    // The block behind the marker at the start of edge 2 is occupied — 1000 m ahead.
    line.interlock.sections[2].occupied = true;
    let blocked = lzb::authority(&line.net, &line.interlock, in_the_lzb_area(), &section());
    assert_eq!(blocked.target_speed, 0.0);
    assert!(
        (blocked.target_distance - 1000.0).abs() < 1.0,
        "authority ends at the block marker, not at a signal: {} m",
        blocked.target_distance
    );
}

/// The block mode is not a setting: with LZB block markers in the line the LZB divides the
/// line itself, without them the main signals stay the only boundaries — and binding.
#[test]
fn the_block_mode_falls_out_of_the_line_data() {
    let with_markers = musterbahn().compile().expect("line compiles");
    let full = lzb::authority(
        &with_markers.net,
        &with_markers.interlock,
        in_the_lzb_area(),
        &section(),
    );
    assert_eq!(full.block_mode, LzbBlockMode::Full);

    let mut source = musterbahn();
    source.devices.retain(|d| d.kind != DeviceKind::BlockMarker);
    let without = source.compile().expect("line compiles");
    let partial = lzb::authority(
        &without.net,
        &without.interlock,
        in_the_lzb_area(),
        &section(),
    );
    assert_eq!(partial.block_mode, LzbBlockMode::Partial);
}

/// A speed restriction of the line is a target like any other: the curve onto the 130 km/h
/// curve is what the LZB supervises, long before the train reaches the board.
#[test]
fn a_speed_restriction_ahead_becomes_the_target() {
    let line = musterbahn().compile().expect("line compiles");
    // 500 m ahead of the curve, which is good for 130 km/h.
    let head = TrackPosition::new(EdgeId(0), 2500.0, 1);
    let t = lzb::authority(&line.net, &line.interlock, head, &section());
    assert_eq!(t.permitted_speed, 160.0);
    assert_eq!(t.target_speed, 130.0);
    assert!(
        (t.target_distance - 500.0).abs() < 1.0,
        "target distance {} m",
        t.target_distance
    );
}

/// The whole chain in the running simulation: line conductor passed → centre → telegram →
/// on-board LZB. Without it the line data would be built but never read.
#[test]
fn a_train_running_into_the_area_is_guided_by_the_centre() {
    let line = musterbahn().compile().expect("line compiles");
    let mut sim = Sim::new(line.net, line.interlock, 1);
    let head = TrackPosition::new(EdgeId(0), 2900.0, 1);
    let train = Train::assemble(vec![Vehicle::new(br101(), head)], head, &sim.net);
    let t = sim.add_train(train);
    for v in &mut sim.trains[t].vehicles {
        v.v = 160.0 / 3.6;
    }

    // 200 m to the start of the line conductor, then take over.
    for _ in 0..1000 {
        sim.step(Sim::DT);
    }
    sim.controls[t].lzb_takeover = true;
    for _ in 0..20 {
        sim.step(Sim::DT);
    }
    sim.controls[t].lzb_takeover = false;
    sim.step(Sim::DT);

    let SafetySystems::De(de) = sim.trains[t].vehicles[0].safety else {
        panic!("BR 101 carries the German package");
    };
    let lzb = de.lzb.expect("BR 101 is fitted with LZB");
    assert!(lzb.is_guiding(), "the centre's telegram was taken over");
    assert_eq!(lzb.block_mode(), LzbBlockMode::Full, "the line has markers");
    assert!(
        !lzb.signals_binding(),
        "full block mode: the LZB alone gives the authority"
    );
    // v-target is the line speed of the curve the train is running in, not a figure written
    // into the line conductor.
    assert_eq!(sim.runtime[t].protection.speed_limit, Some(130.0));
}

/// Without a block division of its own the authority ends at the main signal at stop.
#[test]
fn a_signal_at_stop_ends_the_authority() {
    let mut source = musterbahn();
    source.devices.retain(|d| d.kind != DeviceKind::BlockMarker);
    let mut line = source.compile().expect("line compiles");
    // The section the main signal at km 2.0 guards is occupied, so it goes to stop.
    line.interlock.sections[1].occupied = true;
    line.interlock.update(&mut line.net);

    let head = TrackPosition::new(EdgeId(0), 0.0, 1);
    let t = lzb::authority(&line.net, &line.interlock, head, &section());
    assert_eq!(t.target_speed, 0.0);
    assert!(
        (t.target_distance - 2000.0).abs() < 1.0,
        "authority ends at the main signal: {} m",
        t.target_distance
    );
}
