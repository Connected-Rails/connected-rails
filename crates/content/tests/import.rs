//! Abnahme des Streckenimports (Plan Kap. 15): OSM + DGM → befahrbare Strecke.

use content::import::dgm::{HeightTile, TerrainSource};
use content::import::{ImportOptions, import_line};
use content::vehicles::{br101, passenger_coach, vehicle};
use sim_core::Sim;
use sim_core::safety::SafetySystems;
use sim_core::train::Train;
use track_model::{EdgeId, TrackPosition};
use world_coords::geo;

/// Baut ein Overpass-JSON aus einer synthetischen Trasse: Gerade – Rechtsbogen
/// (R = 2000 m) – Gerade, aufgeteilt auf zwei Wege (der zweite verkehrt herum, wie es in
/// OSM häufig vorkommt).
fn overpass_json() -> String {
    let (lat0, lon0) = (52.0f64, 10.0f64);
    let start = geo::to_ecef_deg(lat0, lon0, 0.0);
    let frame = world_coords::EnuFrame::at(start);

    // Punkte im lokalen ENU (Ost/Nord), 25 m Abstand.
    // 1 km Gerade, 1,5 km Bogen R = 2000 m, 0,5 km Gerade — eine Strecke, die nicht
    // mitten im Bogen abbricht, so wie ein sinnvoller Overpass-Ausschnitt.
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

    // Weg 1: Knoten 1..60. Weg 2: Knoten 120..60 (umgekehrte Richtung).
    let way1: Vec<String> = (1..=60).map(|i| i.to_string()).collect();
    let way2: Vec<String> = (60..=120).rev().map(|i| i.to_string()).collect();

    format!(
        r#"{{"version":0.6,"elements":[{nodes}
        {{"type":"way","id":1001,"nodes":[{}],"tags":{{"railway":"rail","maxspeed":"120","name":"Teststrecke"}}}},
        {{"type":"way","id":1002,"nodes":[{}],"tags":{{"railway":"rail","maxspeed":"100","name":"Teststrecke"}}}},
        {{"type":"way","id":1003,"nodes":[1,2],"tags":{{"railway":"platform"}}}},
        {{"type":"relation","id":5,"members":[]}}
        ]}}"#,
        way1.join(","),
        way2.join(",")
    )
}

/// DGM über dem Testgebiet: 100 m NHN, ab 1,5 km mit 10 ‰ steigend.
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
    TerrainSource::from_tile(HeightTile::parse_xyz(&text, 32).expect("Raster lesbar"))
}

#[test]
fn osm_wege_werden_verkettet() {
    let railway = content::import::osm::parse(&overpass_json()).expect("JSON lesbar");
    assert_eq!(railway.ways.len(), 2, "nur railway=rail übernehmen");
    assert_eq!(railway.nodes.len(), 120);
    assert_eq!(railway.name().as_deref(), Some("Teststrecke"));

    let chain = railway.chain(None).expect("Kette");
    // Beide Wege ergeben zusammen 120 Knoten, der Verbindungsknoten zählt einmal.
    assert_eq!(chain.len(), 120);
    // Der zweite Weg wurde umgedreht: ohne Drehung gäbe es an der Nahtstelle einen
    // Sprung über die halbe Strecke. Alle Schritte bleiben beim Knotenabstand.
    let max_step = chain
        .windows(2)
        .map(|w| {
            let a = world_coords::geo::to_ecef_deg(w[0].lat, w[0].lon, 0.0);
            let b = world_coords::geo::to_ecef_deg(w[1].lat, w[1].lon, 0.0);
            a.distance(b)
        })
        .fold(0.0f64, f64::max);
    assert!(max_step < 40.0, "Sprung in der Kette: {max_step:.0} m");
}

#[test]
fn import_erzeugt_befahrbare_strecke() {
    let options = ImportOptions {
        name: "Teststrecke".into(),
        ..Default::default()
    };
    let (line, report) =
        import_line(&overpass_json(), Some(&mut height_grid()), &options).expect("Import gelingt");

    // ~3 km Trasse, in Kanten à 2 km aufgeteilt.
    assert!(
        (report.length - 2975.0).abs() < 100.0,
        "{} m",
        report.length
    );
    assert_eq!(report.edges, 2);
    // Die Abweichung misst den Abstand zur **OSM-Linie**, nicht zur Wirklichkeit. Radius
    // und Drehwinkel werden exakt getroffen (siehe unten); die Restabweichung entsteht,
    // weil sich Anfang und Ende eines Bogens aus einer Punktfolge nur auf etwa zehn Meter
    // genau bestimmen lassen — dieselbe Größenordnung, in der OSM selbst liegt.
    assert!(
        report.max_deviation < 20.0,
        "Trassierungsfehler {:.2} m",
        report.max_deviation
    );

    // Entwurfselemente statt Punktrauschen: Gerade – Übergang – Bogen – Übergang.
    assert_eq!(report.arcs, 1, "ein Bogen erwartet");
    assert!(
        (3..=6).contains(&report.elements),
        "{} Elemente sind zu viele oder zu wenige",
        report.elements
    );
    let radius = report.min_radius.expect("Radius rekonstruiert");
    assert!(
        (radius - 2000.0).abs() < 300.0,
        "Radius {radius:.0} m statt ~2000 m"
    );
    assert!(report.height_coverage > 0.95, "{}", report.height_coverage);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    // Geschwindigkeit aus dem maxspeed-Tag.
    assert_eq!(line.edges[0].speed[0].1, 120.0);

    // Übersetzen und tatsächlich befahren.
    let compiled = line.compile().expect("übersetzbar");
    let mut sim = Sim::new(compiled.net, compiled.interlock, 1);
    let head = TrackPosition::new(EdgeId(0), 50.0, 1);
    let mut vehicles = vec![vehicle(br101(), head, SafetySystems::None)];
    for _ in 0..3 {
        vehicles.push(vehicle(passenger_coach(), head, SafetySystems::None));
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
    // Bis über die Kantengrenze fahren — am Streckenende steht ein Prellbock.
    for i in 0..30_000 {
        sim.controls[t].sifa = (i / 200) % 2 == 0;
        sim.step(Sim::DT);
        if sim.runtime[t].odometer > 2200.0 {
            break;
        }
    }
    assert!(
        sim.runtime[t].odometer > 2200.0,
        "Zug ist nur {:.0} m gefahren",
        sim.runtime[t].odometer
    );
    assert!(!sim.runtime[t].blocked, "Zug ist unterwegs aufgelaufen");
    assert_eq!(
        sim.trains[t].vehicles[0].pos.edge,
        EdgeId(1),
        "Kantenwechsel"
    );
}

#[test]
fn dgm_hoehen_landen_im_neigungsprofil() {
    let (line, report) = import_line(
        &overpass_json(),
        Some(&mut height_grid()),
        &ImportOptions::default(),
    )
    .expect("Import gelingt");
    assert!(report.height_coverage > 0.95);

    let compiled = line.compile().unwrap();
    // Höhe entlang der Strecke abgreifen — die Kantengrenzen liegen an Elementgrenzen,
    // deshalb wird über das Netz gelaufen statt eine feste Kante zu adressieren.
    let height_at = |distance: f64| {
        let mut pos = TrackPosition::new(EdgeId(0), 0.0, 1);
        let mut scratch = Vec::new();
        pos.advance(&compiled.net, distance, &mut scratch).unwrap();
        geo::from_ecef(pos.pose(&compiled.net).pos).2
    };

    // Erster Kilometer eben, danach die 10-‰-Steigung des Testgeländes.
    let h0 = height_at(100.0);
    let h1 = height_at(1000.0);
    let h2 = height_at(2500.0);
    assert!((h1 - h0).abs() < 1.5, "erster Kilometer eben: {}", h1 - h0);
    assert!(h2 > h1 + 5.0, "Steigung fehlt: {h1} → {h2}");
}

#[test]
fn import_ohne_dgm_warnt() {
    let (_, report) =
        import_line(&overpass_json(), None, &ImportOptions::default()).expect("Import gelingt");
    assert!(report.warnings.iter().any(|w| w.contains("DGM")));
    assert_eq!(report.height_coverage, 0.0);
}

#[test]
fn fehlerfaelle_sind_verstaendlich() {
    let err = import_line(r#"{"elements":[]}"#, None, &ImportOptions::default()).unwrap_err();
    assert!(err.to_string().contains("railway=rail"), "{err}");

    let err = import_line("kein json", None, &ImportOptions::default()).unwrap_err();
    assert!(err.to_string().contains("Overpass-JSON"), "{err}");
}
