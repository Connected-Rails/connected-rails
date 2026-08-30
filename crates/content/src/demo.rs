//! Test line "Musterbahn" — short line with signals, PZB magnets and an LZB section.
//!
//! Serves as a test fixture, as an example for the editor and as the app's start scene.

use crate::route::{
    DeviceSource, EdgeSource, EdgeStart, GeoPoint, LineSource, NodeSource, SectionSource,
    SignalSource, TreeSource, WaterPoint, WaterSource, YardSource,
};
use sim_core::interlock::BlockMarkerPayload;
use sim_core::interlock::{SignalKind, SignalSystem};
use sim_core::safety::de::{LzbSection, MagnetPayload};
use sim_core::yard::YardKind;
use track_model::{DeviceKind, Facing, Segment};

/// Start point of the Musterbahn (Lower Saxony, UTM zone 32).
pub const START: GeoPoint = GeoPoint {
    lat: 52.0,
    lon: 10.0,
    height: 100.0,
};

/// A stand of the `trees` mod: the species a wood of this kind is mixed from,
/// named as often as they are meant to occur. [`crate::terrain::fill_polygon`]
/// draws one entry per tree, so a species listed twice is twice as likely, and
/// the three individuals each species ships (`_a`, `_b`, `_c`) keep a stand of
/// one species from being a stand of one shape.
///
/// The same mixtures the route editor offers under the `stand-*` tags of the
/// mod — this is the demo line naming them by hand, not a second catalogue.
fn stand(members: &[&str]) -> Vec<String> {
    let mut objects = Vec::with_capacity(members.len() * 3);
    for member in members {
        for variant in ['a', 'b', 'c'] {
            objects.push(format!("trees:{member}_{variant}"));
        }
    }
    objects
}

/// Vegetation of the example line: a mixed wood on the northern slope, a
/// spruce stand to the south, scrub along the line, and two solitary trees near
/// the start. All of it baked into single trees exactly as the editor's forest
/// brush does it, so streaming, instancing and per-tree editing stay exercised
/// — and all of it out of the `trees` mod, which is where the levels of detail
/// and the seasonal models come from. Without that mod installed every one of
/// them falls back to the renderer's placeholder, which is the point of the
/// fallback.
fn demo_trees() -> Vec<TreeSource> {
    let mut trees = vec![
        // A solitary oak beside the line, and a birch across from it.
        TreeSource {
            object: "trees:stieleiche_c".into(),
            lat: 52.0006,
            lon: 10.004,
            yaw_deg: 0.0,
            scale: 1.15,
        },
        TreeSource {
            object: "trees:sandbirke_b".into(),
            lat: 51.9994,
            lon: 10.007,
            yaw_deg: 120.0,
            scale: 1.0,
        },
    ];
    let mixed = stand(&[
        "rotbuche",
        "rotbuche",
        "stieleiche",
        "hainbuche",
        "bergahorn",
        "winterlinde",
        "sandbirke",
        "vogelkirsche",
        "fichte",
        "laerche",
        "hasel",
        "eberesche",
    ]);
    let conifer = stand(&[
        "fichte",
        "fichte",
        "fichte",
        "kiefer",
        "kiefer",
        "weisstanne",
        "douglasie",
        "laerche",
        "sandbirke",
    ]);
    // What grows on a railway embankment: pioneers and thorn scrub.
    let scrub = stand(&[
        "robinie",
        "salweide",
        "holunder",
        "schlehe",
        "weissdorn",
        "eberesche",
        "sandbirke",
        "zitterpappel",
    ]);
    for (polygon, objects, area, seed) in [
        (
            vec![
                (52.001, 10.005),
                (52.001, 10.030),
                (52.005, 10.028),
                (52.004, 10.006),
            ],
            &mixed,
            220.0,
            1,
        ),
        (
            vec![
                (51.995, 10.010),
                (51.998, 10.012),
                (51.998, 10.022),
                (51.994, 10.020),
            ],
            &conifer,
            160.0,
            2,
        ),
        // Two bands along the first straight, clear of the embankment the
        // terrain pulls up to rail height (`TREE_TRACK_CLEARANCE`).
        (
            vec![
                (52.00060, 10.0020),
                (52.00060, 10.0380),
                (52.00090, 10.0380),
                (52.00090, 10.0020),
            ],
            &scrub,
            90.0,
            3,
        ),
        (
            vec![
                (51.99910, 10.0040),
                (51.99910, 10.0360),
                (51.99940, 10.0360),
                (51.99940, 10.0040),
            ],
            &scrub,
            90.0,
            4,
        ),
    ] {
        // The polygons keep off the track on their own — no clearance filter.
        trees.extend(crate::terrain::fill_polygon(
            &polygon,
            objects,
            area,
            seed,
            32,
            |_, _| true,
        ));
    }
    trees
}

/// Builds the example line: 3 km straight, 1 km curve, 3 km climb.
///
/// Signalling: distant signal at km 1.0 and main signal at km 2.0 (end of block),
/// plus the three PZB magnets. From the third section on there is a line cable (LZB).
pub fn musterbahn() -> LineSource {
    let magnet = |p: &MagnetPayload| ron::to_string(p).unwrap();

    LineSource {
        name: "Musterbahn".into(),
        geoid_offset: 46.0,
        electrification: track_model::PowerSystem::Ac15kv.id().to_string(),
        nodes: vec![
            NodeSource::Buffer,
            NodeSource::Joint,
            NodeSource::Joint,
            NodeSource::Buffer,
        ],
        edges: vec![
            EdgeSource {
                from: 0,
                to: 1,
                start: EdgeStart::Geo {
                    point: START,
                    heading_deg: 90.0,
                },
                segments: vec![Segment::straight(3000.0)],
                grade: vec![],
                cant: vec![],
                speed: vec![(0.0, 160.0)],
                track_type: vec![],
                electrification: Vec::new(),
                formation: true,
            },
            EdgeSource {
                from: 1,
                to: 2,
                start: EdgeStart::Continue { edge: 0 },
                segments: vec![
                    Segment::transition(200.0, 0.0, 1.0 / 1200.0),
                    Segment::arc(600.0, 1200.0),
                    Segment::transition(200.0, 1.0 / 1200.0, 0.0),
                ],
                grade: vec![],
                cant: vec![(0.0, 0.0), (200.0, 80.0), (800.0, 0.0)],
                speed: vec![(0.0, 130.0)],
                track_type: vec![],
                electrification: Vec::new(),
                formation: true,
            },
            EdgeSource {
                from: 2,
                to: 3,
                start: EdgeStart::Continue { edge: 1 },
                segments: vec![Segment::straight(3000.0)],
                grade: vec![(0.0, 0.0), (500.0, 8.0), (2500.0, 0.0)],
                cant: vec![],
                speed: vec![(0.0, 160.0)],
                track_type: vec![],
                electrification: Vec::new(),
                formation: true,
            },
        ],
        devices: vec![
            // 0: distant signal at km 1.0
            DeviceSource {
                kind: DeviceKind::Signal,
                edge: 0,
                s: 1000.0,
                facing: Facing::Forward,
                lateral_offset: 3.5,
                payload: String::new(),
            },
            // 1: 1000 Hz magnet at the distant signal
            DeviceSource {
                kind: DeviceKind::Magnet,
                edge: 0,
                s: 1000.0,
                facing: Facing::Forward,
                lateral_offset: 0.0,
                payload: magnet(&MagnetPayload::hz1000(0)),
            },
            // 2: main signal at km 2.0
            DeviceSource {
                kind: DeviceKind::Signal,
                edge: 0,
                s: 2000.0,
                facing: Facing::Forward,
                lateral_offset: 3.5,
                payload: String::new(),
            },
            // 3: 500 Hz magnet 250 m ahead of it
            DeviceSource {
                kind: DeviceKind::Magnet,
                edge: 0,
                s: 1750.0,
                facing: Facing::Forward,
                lateral_offset: 0.0,
                payload: magnet(&MagnetPayload::hz500(1)),
            },
            // 4: 2000 Hz magnet at the main signal
            DeviceSource {
                kind: DeviceKind::Magnet,
                edge: 0,
                s: 2000.0,
                facing: Facing::Forward,
                lateral_offset: 0.0,
                payload: magnet(&MagnetPayload::hz2000(1)),
            },
            // 5: start of the line cable (LZB) behind the main signal, over the last 4 km
            DeviceSource {
                kind: DeviceKind::LineConductor,
                edge: 1,
                s: 0.0,
                facing: Facing::Forward,
                lateral_offset: 0.0,
                payload: ron::to_string(&LzbSection {
                    length: 4000.0,
                    cir_elke: false,
                    end: false,
                })
                .unwrap(),
            },
            // 6: platform at the end
            DeviceSource {
                kind: DeviceKind::Platform,
                edge: 2,
                s: 2600.0,
                facing: Facing::Both,
                lateral_offset: 5.0,
                payload: "(name:\"Musterstadt\",length:210.0)".into(),
            },
            // 7/8: LZB block markers — the block division of the line under the LZB. They are
            // what makes the section run in full block mode; without them the main signals
            // would stay the only boundaries.
            DeviceSource {
                kind: DeviceKind::BlockMarker,
                edge: 1,
                s: 0.0,
                facing: Facing::Forward,
                lateral_offset: 0.0,
                payload: ron::to_string(&BlockMarkerPayload { section: 1 }).unwrap(),
            },
            DeviceSource {
                kind: DeviceKind::BlockMarker,
                edge: 2,
                s: 0.0,
                facing: Facing::Forward,
                lateral_offset: 0.0,
                payload: ron::to_string(&BlockMarkerPayload { section: 2 }).unwrap(),
            },
        ],
        objects: vec![],
        // Where trains appear and disappear (plan ch. 11). The Musterbahn is a plain
        // line with a buffer stop at each end, so it has two portals and no siding —
        // the example mod's line carries the stabling road.
        yards: vec![
            YardSource {
                name: "Portal West".into(),
                kind: YardKind::Portal,
                edge: 0,
                s: 300.0,
                facing: Facing::Forward,
                length: 300.0,
            },
            YardSource {
                name: "Portal Ost".into(),
                kind: YardKind::Portal,
                edge: 2,
                s: 2700.0,
                facing: Facing::Backward,
                length: 300.0,
            },
        ],
        trees: demo_trees(),
        // A lake south of the first straight — the stand-in for the water
        // import, so the demo shows a body of water without an extract. Its
        // surface is laid over the terrain when the tiles are built, like the
        // woods are filled into trees.
        waters: vec![WaterSource {
            name: "Mustersee".into(),
            polygon: vec![
                WaterPoint {
                    lat: 51.9981,
                    lon: 10.0040,
                },
                WaterPoint {
                    lat: 51.9981,
                    lon: 10.0100,
                },
                WaterPoint {
                    lat: 51.9992,
                    lon: 10.0102,
                },
                WaterPoint {
                    lat: 51.9992,
                    lon: 10.0038,
                },
            ],
            // An island in the middle — the multipolygon import's work, so
            // the demo shows a lake with an island in it.
            holes: vec![vec![
                WaterPoint {
                    lat: 51.99850,
                    lon: 10.0058,
                },
                WaterPoint {
                    lat: 51.99848,
                    lon: 10.0082,
                },
                WaterPoint {
                    lat: 51.99880,
                    lon: 10.0080,
                },
                WaterPoint {
                    lat: 51.99882,
                    lon: 10.0060,
                },
            ]],
            tags: vec!["water".into()],
        }],
        markers: vec![],
        terrain: vec![],
        heights: vec![],
        sections: vec![
            SectionSource { edges: vec![0] },
            SectionSource { edges: vec![1] },
            SectionSource { edges: vec![2] },
        ],
        areas: Vec::new(),
        signals: vec![
            SignalSource {
                kind: SignalKind::Distant,
                system: SignalSystem::HV,
                device: 0,
                next: Some(1),
                guarded: vec![],
                requires_route: false,
                diverging_speed: None,
                signal_type: None,
                model: None,
            },
            SignalSource {
                kind: SignalKind::Main,
                system: SignalSystem::HV,
                device: 2,
                next: None,
                guarded: vec![1, 2],
                requires_route: false,
                diverging_speed: None,
                signal_type: None,
                model: None,
            },
        ],
        routes: vec![],
        boundaries: Vec::new(),
        script: None,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::LineSource;

    #[test]
    fn musterbahn_compiles_and_is_continuous() {
        let line = musterbahn();
        let compiled = line.compile().expect("compiles");
        assert_eq!(compiled.net.edges().len(), 3);
        assert_eq!(compiled.net.devices().len(), 9);
        assert_eq!(compiled.interlock.signals.len(), 2);

        // Edges join up geometrically.
        for i in 0..2 {
            let end = compiled.net.edges()[i].end_pose().pos;
            let start = compiled.net.edges()[i + 1].eval(0.0).pos;
            assert!(
                end.distance(start) < 0.01,
                "gap between edge {i} and {}: {} m",
                i + 1,
                end.distance(start)
            );
        }

        // Total length ~ 7 km.
        let len: f64 = compiled.net.edges().iter().map(|e| e.length()).sum();
        assert!((len - 7000.0).abs() < 1.0, "{len}");
    }

    #[test]
    fn ron_roundtrip() {
        let line = musterbahn();
        let text = line.to_ron();
        let back = LineSource::from_ron(&text).expect("RON readable");
        assert_eq!(back, line);
        // Still compiles after the detour through RON.
        assert!(back.compile().is_ok());
    }

    #[test]
    fn grade_profile_climbs_16_m() {
        let compiled = musterbahn().compile().unwrap();
        let edge = &compiled.net.edges()[2];
        let h0 = world_coords::geo::from_ecef(edge.eval(0.0).pos).2;
        let h1 = world_coords::geo::from_ecef(edge.eval(3000.0).pos).2;
        // 2000 m at 8 ‰ = 16 m.
        assert!((h1 - h0 - 16.0).abs() < 0.05, "{}", h1 - h0);
    }
}
