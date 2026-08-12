//! Acceptance test of the line import (plan ch. 15): OSM + DGM → drivable line.

use content::import::dgm::{HeightTile, TerrainSource};
use content::import::{ImportOptions, import_line};
use content::vehicles::{br101, passenger_coach};
use sim_core::Sim;
use sim_core::safety::SafetyEquipment;
use sim_core::train::{Train, Vehicle, VehicleSpec};
use track_model::{EdgeId, TrackPosition};
use world_coords::geo;

/// Builds an Overpass JSON from a synthetic alignment: straight – right-hand curve
/// (R = 2000 m) – straight, split across two ways (the second one reversed, as often
/// happens in OSM).
fn overpass_json() -> String {
    let (lat0, lon0) = (52.0f64, 10.0f64);
    let start = geo::to_ecef_deg(lat0, lon0, 0.0);
    let frame = world_coords::EnuFrame::at(start);

    // Points in local ENU (east/north), 25 m apart.
    // 1 km straight, 1.5 km curve R = 2000 m, 0.5 km straight — a line that does not
    // break off in the middle of a curve, like a sensible Overpass extract.
    let mut local = Vec::new();
    let mut pos = glam::DVec2::ZERO;
    let mut heading = 0.0f64;
    for i in 0..120 {
        let k: f64 = if (40..100).contains(&i) {
            -1.0 / 2000.0
        } else {
            0.0
        };
        heading += k * 25.0;
        pos += glam::DVec2::new(heading.cos(), heading.sin()) * 25.0;
        local.push(pos);
    }

    let to_geo = |p: glam::DVec2| {
        let ecef = frame.to_ecef_curved(glam::DVec3::new(p.x, p.y, 0.0));
        let (lat, lon, _) = geo::from_ecef(ecef);
        (lat.to_degrees(), lon.to_degrees())
    };

    let mut nodes = String::new();
    for (i, p) in local.iter().enumerate() {
        let (lat, lon) = to_geo(*p);
        nodes.push_str(&format!(
            r#"{{"type":"node","id":{},"lat":{lat},"lon":{lon}}},"#,
            i + 1
        ));
    }

    // Way 1: nodes 1..60. Way 2: nodes 120..60 (reversed direction).
    let way1: Vec<String> = (1..=60).map(|i| i.to_string()).collect();
    let way2: Vec<String> = (60..=120).rev().map(|i| i.to_string()).collect();

    format!(
        r#"{{"version":0.6,"elements":[{nodes}
        {{"type":"way","id":1001,"nodes":[{}],"tags":{{"railway":"rail","maxspeed":"120","name":"Test line"}}}},
        {{"type":"way","id":1002,"nodes":[{}],"tags":{{"railway":"rail","maxspeed":"100","name":"Test line"}}}},
        {{"type":"way","id":1003,"nodes":[1,2],"tags":{{"railway":"platform"}}}},
        {{"type":"relation","id":5,"members":[]}}
        ]}}"#,
        way1.join(","),
        way2.join(",")
    )
}

/// DGM over the test area: 100 m NHN, climbing at 10 ‰ from 1.5 km on.
fn height_grid() -> TerrainSource {
    let (e0, n0) = geo::to_utm(52.0f64.to_radians(), 10.0f64.to_radians(), 32);
    let mut text = String::new();
    for iy in -80..80 {
        for ix in -20..160 {
            let x = (e0 / 25.0).round() * 25.0 + ix as f64 * 25.0;
            let y = (n0 / 25.0).round() * 25.0 + iy as f64 * 25.0;
            let along = x - e0;
            let z = if along < 1500.0 {
                100.0
            } else {
                100.0 + (along - 1500.0) * 0.01
            };
            text.push_str(&format!("{x} {y} {z}\n"));
        }
    }
    TerrainSource::from_tile(HeightTile::parse_xyz(&text, 32).expect("grid readable"))
}

#[test]
fn osm_ways_are_chained() {
    let railway = content::import::osm::parse(&overpass_json()).expect("JSON readable");
    assert_eq!(railway.ways.len(), 2, "only take railway=rail");
    assert_eq!(railway.nodes.len(), 120);
    assert_eq!(railway.name().as_deref(), Some("Test line"));

    let chain = railway.chain(None).expect("chain");
    // Both ways together give 120 nodes, the connecting node counts once.
    assert_eq!(chain.len(), 120);
    // The second way was reversed: without reversing there would be a jump across half
    // the line at the seam. All steps stay at the node spacing.
    let max_step = chain
        .windows(2)
        .map(|w| {
            let a = world_coords::geo::to_ecef_deg(w[0].lat, w[0].lon, 0.0);
            let b = world_coords::geo::to_ecef_deg(w[1].lat, w[1].lon, 0.0);
            a.distance(b)
        })
        .fold(0.0f64, f64::max);
    assert!(max_step < 40.0, "jump in the chain: {max_step:.0} m");
}

#[test]
fn import_produces_a_drivable_line() {
    let options = ImportOptions {
        name: "Test line".into(),
        ..Default::default()
    };
    let (line, report) =
        import_line(&overpass_json(), Some(&mut height_grid()), &options).expect("import succeeds");

    // ~3 km of alignment, split into edges of 2 km.
    assert!(
        (report.length - 2975.0).abs() < 100.0,
        "{} m",
        report.length
    );
    assert_eq!(report.edges, 2);
    // The deviation measures the distance to the **OSM line**, not to reality. Radius
    // and turn angle are hit exactly (see below); the remaining deviation arises because
    // the start and end of a curve can only be determined to about ten metres from a
    // sequence of points — the same order of magnitude that OSM itself lies in.
    assert!(
        report.max_deviation < 20.0,
        "alignment error {:.2} m",
        report.max_deviation
    );

    // Design elements instead of point noise: straight – transition – curve – transition.
    assert_eq!(report.arcs, 1, "one curve expected");
    assert!(
        (3..=6).contains(&report.elements),
        "{} elements are too many or too few",
        report.elements
    );
    let radius = report.min_radius.expect("radius reconstructed");
    assert!(
        (radius - 2000.0).abs() < 300.0,
        "radius {radius:.0} m instead of ~2000 m"
    );
    assert!(report.height_coverage > 0.95, "{}", report.height_coverage);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    // Speed from the maxspeed tag.
    assert_eq!(line.edges[0].speed[0].1, 120.0);

    // Compile it and actually drive on it.
    let compiled = line.compile().expect("compiles");
    let mut sim = Sim::new(compiled.net, compiled.interlock, 1);
    let head = TrackPosition::new(EdgeId(0), 50.0, 1);
    // The imported line has no train protection equipment — so the loco runs without it.
    let mut vehicles = vec![Vehicle::new(
        VehicleSpec {
            safety: SafetyEquipment::None,
            ..br101()
        },
        head,
    )];
    for _ in 0..3 {
        vehicles.push(Vehicle::new(passenger_coach(), head));
    }
    let train = Train::assemble(vehicles, head, &sim.net);
    let t = sim.add_train(train);
    for v in &mut sim.trains[t].vehicles {
        if v.is_powered() {
            v.traction.battery = true;
            v.traction.pantograph_command = true;
            v.traction.main_switch_command = true;
            v.traction.pantograph = 1.0;
        }
    }
    sim.controls[t].reverser = 1;
    sim.controls[t].throttle = 0.8;
    // Drive past the edge boundary — there is a buffer stop at the end of the line.
    for i in 0..30_000 {
        sim.controls[t].sifa = (i / 200) % 2 == 0;
        sim.step(Sim::DT);
        if sim.runtime[t].odometer > 2200.0 {
            break;
        }
    }
    assert!(
        sim.runtime[t].odometer > 2200.0,
        "train only travelled {:.0} m",
        sim.runtime[t].odometer
    );
    assert!(!sim.runtime[t].blocked, "train got stuck on the way");
    assert_eq!(sim.trains[t].vehicles[0].pos.edge, EdgeId(1), "edge change");
}

#[test]
fn dgm_heights_end_up_in_the_gradient_profile() {
    let (line, report) = import_line(
        &overpass_json(),
        Some(&mut height_grid()),
        &ImportOptions::default(),
    )
    .expect("import succeeds");
    assert!(report.height_coverage > 0.95);

    let compiled = line.compile().unwrap();
    // Sample the height along the line — the edge boundaries lie at element boundaries,
    // so the network is walked instead of addressing a fixed edge.
    let height_at = |distance: f64| {
        let mut pos = TrackPosition::new(EdgeId(0), 0.0, 1);
        let mut scratch = Vec::new();
        pos.advance(&compiled.net, distance, &mut scratch).unwrap();
        geo::from_ecef(pos.pose(&compiled.net).pos).2
    };

    // First kilometre level, then the 10 ‰ climb of the test terrain.
    let h0 = height_at(100.0);
    let h1 = height_at(1000.0);
    let h2 = height_at(2500.0);
    assert!((h1 - h0).abs() < 1.5, "first kilometre level: {}", h1 - h0);
    assert!(h2 > h1 + 5.0, "climb missing: {h1} → {h2}");
}

#[test]
fn import_without_a_dgm_warns() {
    let (_, report) =
        import_line(&overpass_json(), None, &ImportOptions::default()).expect("import succeeds");
    assert!(report.warnings.iter().any(|w| w.contains("DGM")));
    assert_eq!(report.height_coverage, 0.0);
}

#[test]
fn error_cases_are_understandable() {
    let err = import_line(r#"{"elements":[]}"#, None, &ImportOptions::default()).unwrap_err();
    assert!(err.to_string().contains("railway=rail"), "{err}");

    let err = import_line("not json", None, &ImportOptions::default()).unwrap_err();
    assert!(err.to_string().contains("Overpass JSON"), "{err}");
}
