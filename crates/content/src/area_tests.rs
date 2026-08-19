//! Acceptance of the marked track areas: what an area sets, what it leaves alone, and
//! what happens to a marking when the track under it is cut or removed.

use crate::route::*;
use track_model::EdgeId;

/// A straight 3 km line with nothing stated per edge — everything the areas do is
/// visible against the defaults.
fn line() -> LineSource {
    LineSource {
        name: "areas".into(),
        geoid_offset: 46.0,
        electrification: track_model::PowerSystem::Ac15kv.id().to_string(),
        nodes: vec![NodeSource::Buffer, NodeSource::Buffer],
        edges: vec![EdgeSource {
            from: 0,
            to: 1,
            start: EdgeStart::Geo {
                point: GeoPoint {
                    lat: 52.0,
                    lon: 10.0,
                    height: 100.0,
                },
                heading_deg: 90.0,
            },
            segments: vec![track_model::Segment::straight(3000.0)],
            grade: vec![],
            cant: vec![],
            speed: vec![],
            track_type: vec![],
            electrification: vec![],
        }],
        devices: vec![],
        objects: vec![],
        trees: vec![],
        markers: vec![],
        terrain: vec![],
        heights: vec![],
        sections: vec![],
        areas: vec![],
        signals: vec![],
        routes: vec![],
        boundaries: vec![],
        script: None,
        ..Default::default()
    }
}

fn area(from: f64, to: f64) -> TrackAreaSource {
    TrackAreaSource {
        name: "area".into(),
        spans: vec![AreaSpan::new(0, from, to)],
        ..TrackAreaSource::default()
    }
}

#[test]
fn an_area_sets_what_it_states_and_leaves_the_rest_alone() {
    let mut line = line();
    line.areas.push(TrackAreaSource {
        speed: Some(40.0),
        cant: Some(90.0),
        ..area(1000.0, 2000.0)
    });
    let net = line.compile().expect("compiles").net;
    let edge = &net.edges()[0];
    // Outside it, the defaults.
    assert_eq!(edge.speed.at(500.0), DEFAULT_SPEED);
    assert_eq!(edge.cant.at(500.0), 0.0);
    // Inside it, what the area says.
    assert_eq!(edge.speed.at(1500.0), 40.0);
    assert_eq!(edge.cant.at(1500.0), 90.0);
    // And past its end, the defaults again.
    assert_eq!(edge.speed.at(2500.0), DEFAULT_SPEED);
    assert_eq!(edge.cant.at(2500.0), 0.0);
    // What it does not state, it does not touch: the wire is still the one the line has.
    assert_eq!(
        net.electrification_at(EdgeId(0), 1500.0),
        Some(track_model::PowerSystem::Ac15kv)
    );
}

#[test]
fn an_area_overrides_the_edge_profile_and_gives_it_back_afterwards() {
    let mut line = line();
    line.edges[0].speed = vec![(0.0, 120.0), (2000.0, 80.0)];
    line.areas.push(TrackAreaSource {
        speed: Some(40.0),
        ..area(1000.0, 2500.0)
    });
    let net = line.compile().expect("compiles").net;
    let edge = &net.edges()[0];
    assert_eq!(edge.speed.at(500.0), 120.0);
    assert_eq!(edge.speed.at(1500.0), 40.0);
    assert_eq!(edge.speed.at(2200.0), 40.0);
    // Past the area the edge profile applies again — including the step the area was
    // laid over.
    assert_eq!(edge.speed.at(2600.0), 80.0);
}

#[test]
fn a_later_area_is_drawn_on_top_of_an_earlier_one() {
    let mut line = line();
    line.areas.push(TrackAreaSource {
        speed: Some(100.0),
        ..area(500.0, 2500.0)
    });
    line.areas.push(TrackAreaSource {
        speed: Some(40.0),
        ..area(1000.0, 1500.0)
    });
    let net = line.compile().expect("compiles").net;
    let edge = &net.edges()[0];
    assert_eq!(edge.speed.at(700.0), 100.0);
    assert_eq!(edge.speed.at(1200.0), 40.0);
    assert_eq!(edge.speed.at(2000.0), 100.0);
}

#[test]
fn an_area_can_switch_the_wire_and_the_superstructure() {
    let mut line = line();
    line.areas.push(TrackAreaSource {
        electrification: Some("none".into()),
        track_type: Some("example:nebenbahn".into()),
        ..area(1000.0, 2000.0)
    });
    let net = line.compile().expect("compiles").net;
    assert_eq!(
        net.electrification_at(EdgeId(0), 500.0),
        Some(track_model::PowerSystem::Ac15kv)
    );
    assert_eq!(net.electrification_at(EdgeId(0), 1500.0), None);
    assert_eq!(
        net.electrification_at(EdgeId(0), 2500.0),
        Some(track_model::PowerSystem::Ac15kv)
    );
    // The type table interned the name, and only the marked stretch uses it.
    let inside = net.edges()[0].track_type.at(1500.0);
    let outside = net.edges()[0].track_type.at(500.0);
    assert_ne!(inside, outside);
    assert_eq!(outside, 0, "outside the area, the default type");
    assert_eq!(net.types()[inside as usize].name, "example:nebenbahn");
}

#[test]
fn an_area_that_states_nothing_changes_nothing() {
    let mut line = line();
    line.areas.push(area(1000.0, 2000.0));
    assert!(!line.areas[0].sets_anything());
    assert_eq!(line.areas[0].length(), 1000.0);
    let plain = LineSource {
        areas: vec![],
        ..line.clone()
    };
    let with_area = line.compile().expect("compiles").net;
    let without = plain.compile().expect("compiles").net;
    for s in [0.0, 500.0, 1500.0, 2500.0] {
        assert_eq!(
            with_area.edges()[0].speed.at(s),
            without.edges()[0].speed.at(s)
        );
        assert_eq!(
            with_area.edges()[0].cant.at(s),
            without.edges()[0].cant.at(s)
        );
    }
}

#[test]
fn a_span_follows_the_track_it_is_marked_on_when_that_track_is_split() {
    let mut line = line();
    line.areas.push(TrackAreaSource {
        speed: Some(40.0),
        // One span wholly beyond the cut, one straddling it.
        spans: vec![
            AreaSpan::new(0, 2000.0, 2500.0),
            AreaSpan::new(0, 900.0, 1100.0),
        ],
        ..TrackAreaSource::default()
    });
    line.split_edge(0, 1000.0).expect("splits");
    let net = line.compile().expect("compiles").net;
    // The marking on the map has not moved.
    assert_eq!(net.edges()[0].speed.at(950.0), 40.0);
    assert_eq!(net.edges()[1].speed.at(50.0), 40.0);
    assert_eq!(net.edges()[1].speed.at(200.0), DEFAULT_SPEED);
    assert_eq!(net.edges()[1].speed.at(1200.0), 40.0);
}

#[test]
fn removing_a_track_takes_its_spans_with_it() {
    let mut line = line();
    line.nodes.push(NodeSource::Buffer);
    line.edges.push(EdgeSource {
        from: 1,
        to: 2,
        start: EdgeStart::Continue { edge: 0 },
        segments: vec![track_model::Segment::straight(1000.0)],
        grade: vec![],
        cant: vec![],
        speed: vec![],
        track_type: vec![],
        electrification: vec![],
    });
    line.areas.push(TrackAreaSource {
        speed: Some(40.0),
        spans: vec![AreaSpan::new(0, 100.0, 200.0), AreaSpan::new(1, 0.0, 500.0)],
        ..TrackAreaSource::default()
    });
    line.remove_edge(0);
    // The span on the removed track is gone; the other one moved down with its edge.
    assert_eq!(line.areas[0].spans.len(), 1);
    assert_eq!(line.areas[0].spans[0].edge, 0);
    let net = line.compile().expect("compiles").net;
    assert_eq!(net.edges()[0].speed.at(200.0), 40.0);
}

#[test]
fn a_span_is_stored_the_way_round_it_was_marked() {
    // Marking backwards along the track is the same marking.
    let back = AreaSpan::new(3, 800.0, 200.0);
    assert_eq!(back.from, 200.0);
    assert_eq!(back.to, 800.0);
    assert_eq!(back.length(), 600.0);
    assert!(back.covers(3, 500.0));
    assert!(!back.covers(3, 900.0));
    assert!(!back.covers(2, 500.0));
    // The upper end is open, so two spans meeting at a metre do not both claim it.
    assert!(!back.covers(3, 800.0));
}

#[test]
fn the_rule_check_finds_a_marking_that_does_not_reach_the_line() {
    use std::collections::BTreeMap;
    let types = BTreeMap::new();
    let objects = BTreeMap::new();

    // A marking with no properties yet: useful while working, worth saying out loud.
    let mut line = line();
    line.areas.push(area(100.0, 200.0));
    let issues = line.check(&types, &objects);
    assert!(issues.contains(&RuleIssue::AreaWithoutEffect { area: 0 }));

    // Give it one and the finding goes away.
    line.areas[0].speed = Some(40.0);
    assert!(line.check(&types, &objects).is_empty());

    // A stretch beyond the end of its track marks nothing.
    line.areas[0].spans = vec![AreaSpan::new(0, 4000.0, 4500.0)];
    assert!(
        line.check(&types, &objects)
            .contains(&RuleIssue::AreaOffTrack { area: 0 })
    );

    // A stretch on a track that does not exist, likewise.
    line.areas[0].spans = vec![AreaSpan::new(7, 0.0, 100.0)];
    assert!(
        line.check(&types, &objects)
            .contains(&RuleIssue::AreaOffTrack { area: 0 })
    );

    // A type name no installed mod answers is visible before the run.
    line.areas[0].spans = vec![AreaSpan::new(0, 100.0, 200.0)];
    line.areas[0].track_type = Some("nomod:nothing".into());
    assert!(
        line.check(&types, &objects)
            .contains(&RuleIssue::AreaUnknownTrackType { area: 0 })
    );
}
