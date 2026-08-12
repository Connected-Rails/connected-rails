//! Testtrecke „Musterbahn" — kurze Strecke mit Signalen, PZB-Magneten und LZB-Abschnitt.
//!
//! Dient Tests, dem Editor als Beispiel und der App als Startszene.

use crate::route::{
    DeviceSource, EdgeSource, EdgeStart, GeoPoint, LineSource, NodeSource, SectionSource,
    SignalSource,
};
use sim_core::interlock::{SignalKind, SignalSystem};
use sim_core::safety::de::{LzbTelegram, MagnetPayload};
use track_model::{DeviceKind, Facing, Segment};

/// Startpunkt der Musterbahn (Niedersachsen, UTM-Zone 32).
pub const START: GeoPoint = GeoPoint {
    lat: 52.0,
    lon: 10.0,
    height: 100.0,
};

/// Baut die Beispielstrecke: 3 km Gerade, 1 km Bogen, 3 km Steigung.
///
/// Signalisierung: Vorsignal bei km 1,0 und Hauptsignal bei km 2,0 (Blockende),
/// dazu die drei PZB-Magnete. Ab dem dritten Abschnitt liegt Linienleiter (LZB).
pub fn musterbahn() -> LineSource {
    let magnet = |p: &MagnetPayload| ron::to_string(p).unwrap();

    LineSource {
        name: "Musterbahn".into(),
        geoid_offset: 46.0,
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
            },
            EdgeSource {
                from: 2,
                to: 3,
                start: EdgeStart::Continue { edge: 1 },
                segments: vec![Segment::straight(3000.0)],
                grade: vec![(0.0, 0.0), (500.0, 8.0), (2500.0, 0.0)],
                cant: vec![],
                speed: vec![(0.0, 160.0)],
            },
        ],
        devices: vec![
            // 0: Vorsignal bei km 1,0
            DeviceSource {
                kind: DeviceKind::Signal,
                edge: 0,
                s: 1000.0,
                facing: Facing::Forward,
                lateral_offset: 3.5,
                payload: String::new(),
            },
            // 1: 1000-Hz-Magnet am Vorsignal
            DeviceSource {
                kind: DeviceKind::Magnet,
                edge: 0,
                s: 1000.0,
                facing: Facing::Forward,
                lateral_offset: 0.0,
                payload: magnet(&MagnetPayload::hz1000(0)),
            },
            // 2: Hauptsignal bei km 2,0
            DeviceSource {
                kind: DeviceKind::Signal,
                edge: 0,
                s: 2000.0,
                facing: Facing::Forward,
                lateral_offset: 3.5,
                payload: String::new(),
            },
            // 3: 500-Hz-Magnet 250 m davor
            DeviceSource {
                kind: DeviceKind::Magnet,
                edge: 0,
                s: 1750.0,
                facing: Facing::Forward,
                lateral_offset: 0.0,
                payload: magnet(&MagnetPayload::hz500(1)),
            },
            // 4: 2000-Hz-Magnet am Hauptsignal
            DeviceSource {
                kind: DeviceKind::Magnet,
                edge: 0,
                s: 2000.0,
                facing: Facing::Forward,
                lateral_offset: 0.0,
                payload: magnet(&MagnetPayload::hz2000(1)),
            },
            // 5: Beginn des Linienleiters (LZB) im dritten Abschnitt
            DeviceSource {
                kind: DeviceKind::LineConductor,
                edge: 2,
                s: 0.0,
                facing: Facing::Forward,
                lateral_offset: 0.0,
                payload: ron::to_string(&LzbTelegram {
                    permitted_speed: 160.0,
                    target_speed: 0.0,
                    target_distance: 3000.0,
                    end_of_authority: false,
                    length: 3000.0,
                })
                .unwrap(),
            },
            // 6: Bahnsteig am Ende
            DeviceSource {
                kind: DeviceKind::Platform,
                edge: 2,
                s: 2600.0,
                facing: Facing::Both,
                lateral_offset: 5.0,
                payload: "(name:\"Musterstadt\",length:210.0)".into(),
            },
        ],
        sections: vec![
            SectionSource { edges: vec![0] },
            SectionSource { edges: vec![1, 2] },
        ],
        signals: vec![
            SignalSource {
                kind: SignalKind::Distant,
                system: SignalSystem::HV,
                device: 0,
                next: Some(1),
                guarded: vec![],
                requires_route: false,
                diverging_speed: None,
            },
            SignalSource {
                kind: SignalKind::Main,
                system: SignalSystem::HV,
                device: 2,
                next: None,
                guarded: vec![1],
                requires_route: false,
                diverging_speed: None,
            },
        ],
        routes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::LineSource;

    #[test]
    fn musterbahn_compiles_and_is_continuous() {
        let line = musterbahn();
        let compiled = line.compile().expect("übersetzbar");
        assert_eq!(compiled.net.edges().len(), 3);
        assert_eq!(compiled.net.devices().len(), 7);
        assert_eq!(compiled.interlock.signals.len(), 2);

        // Kanten schließen geometrisch aneinander an.
        for i in 0..2 {
            let end = compiled.net.edges()[i].end_pose().pos;
            let start = compiled.net.edges()[i + 1].eval(0.0).pos;
            assert!(
                end.distance(start) < 0.01,
                "Lücke zwischen Kante {i} und {}: {} m",
                i + 1,
                end.distance(start)
            );
        }

        // Gesamtlänge ~ 7 km.
        let len: f64 = compiled.net.edges().iter().map(|e| e.length()).sum();
        assert!((len - 7000.0).abs() < 1.0, "{len}");
    }

    #[test]
    fn ron_roundtrip() {
        let line = musterbahn();
        let text = line.to_ron();
        let back = LineSource::from_ron(&text).expect("RON lesbar");
        assert_eq!(back, line);
        // Auch nach dem Umweg über RON übersetzbar.
        assert!(back.compile().is_ok());
    }

    #[test]
    fn grade_profile_climbs_16_m() {
        let compiled = musterbahn().compile().unwrap();
        let edge = &compiled.net.edges()[2];
        let h0 = world_coords::geo::from_ecef(edge.eval(0.0).pos).2;
        let h1 = world_coords::geo::from_ecef(edge.eval(3000.0).pos).2;
        // 2000 m à 8 ‰ = 16 m.
        assert!((h1 - h0 - 16.0).abs() < 0.05, "{}", h1 - h0);
    }
}
