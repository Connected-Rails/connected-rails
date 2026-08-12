//! Abnahme der Trassierung an einer regelkonform entworfenen Strecke (Plan Kap. 15).

use content::import::{ImportOptions, import_line};
use world_coords::{EnuFrame, geo};

/// Baut Overpass-JSON aus einem Entwurf: Gerade – Übergangsbogen – Kreisbogen –
/// Übergangsbogen – Gerade, so wie eine echte Strecke trassiert ist.
fn design_line(radius: f64, transition: f64, arc: f64, noise: f64) -> String {
    let start = geo::to_ecef_deg(52.0, 10.0, 0.0);
    let frame = EnuFrame::at(start);
    let step = 20.0;

    let mut seed = 4242u64;
    let mut rand = move || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((seed >> 33) as f64 / (1u64 << 31) as f64) - 1.0
    };

    let plan = [
        (800.0, 0.0, 0.0),
        (transition, 0.0, 1.0 / radius),
        (arc, 1.0 / radius, 1.0 / radius),
        (transition, 1.0 / radius, 0.0),
        (800.0, 0.0, 0.0),
    ];
    let mut nodes = String::new();
    let mut ids = Vec::new();
    let (mut x, mut y, mut heading) = (0.0f64, 0.0f64, 0.0f64);
    let mut id = 1;
    for (length, k0, k1) in plan {
        let count = (length / step).round() as usize;
        for i in 0..count {
            let k = k0 + (k1 - k0) * i as f64 / count as f64;
            heading += k * step;
            x += heading.cos() * step;
            y += heading.sin() * step;
            let local = glam::DVec3::new(x + rand() * noise, y + rand() * noise, 0.0);
            let (lat, lon, _) = geo::from_ecef(frame.to_ecef_curved(local));
            nodes.push_str(&format!(
                r#"{{"type":"node","id":{id},"lat":{},"lon":{}}},"#,
                lat.to_degrees(),
                lon.to_degrees()
            ));
            ids.push(id.to_string());
            id += 1;
        }
    }
    format!(
        r#"{{"elements":[{nodes}
        {{"type":"way","id":1,"nodes":[{}],"tags":{{"railway":"rail","maxspeed":"160"}}}}]}}"#,
        ids.join(",")
    )
}

#[test]
fn regelkonforme_trasse_wird_genau_rekonstruiert() {
    let json = design_line(1500.0, 180.0, 700.0, 0.0);
    let (line, report) = import_line(&json, None, &ImportOptions::default()).expect("Import");

    // Radius exakt getroffen, Überhöhung nach Regelwerk (11,8·v²/R − Fehlbetrag).
    assert_eq!(report.min_radius, Some(1500.0), "Radius");
    assert!(
        (report.max_cant - 140.0).abs() < 6.0,
        "Überhöhung {} mm",
        report.max_cant
    );
    assert_eq!(report.arcs, 1);
    assert!(
        report.max_deviation < 6.0,
        "Abweichung {:.1} m",
        report.max_deviation
    );
    assert!(report.warnings.iter().all(|w| !w.contains("Trassierung")));

    // Die Überhöhung steht in der Streckendatei und wird beim Übersetzen zur Gleislage.
    let compiled = line.compile().expect("übersetzbar");
    let max_cant = compiled
        .net
        .edges()
        .iter()
        .flat_map(|e| e.cant.steps().iter().map(|(_, c)| *c))
        .fold(0.0f64, f64::max);
    assert!((max_cant - report.max_cant).abs() < 1e-6);

    // Im Bogen ist das Gleis tatsächlich verwunden: „oben" kippt zur Kurveninnenseite.
    let edge = compiled
        .net
        .edges()
        .iter()
        .find(|e| e.cant.steps().iter().any(|(_, c)| *c > 100.0))
        .expect("Kante mit Überhöhung");
    let s = edge
        .cant
        .steps()
        .iter()
        .find(|(_, c)| *c > 100.0)
        .map(|(s, _)| *s)
        .unwrap();
    let pose = edge.eval(s + 50.0);
    let plumb = world_coords::EnuFrame::at(pose.pos).up;
    let tilt = pose.up.dot(plumb).acos().to_degrees();
    assert!(
        tilt > 3.0 && tilt < 8.0,
        "Verwindung {tilt:.1}° passt nicht zu {} mm",
        report.max_cant
    );
}

#[test]
fn verrauschte_quelle_liefert_denselben_radius() {
    // ±2 m Rauschen — die Größenordnung von OSM aus Luftbildern.
    let json = design_line(1500.0, 180.0, 700.0, 2.0);
    let (_, report) = import_line(&json, None, &ImportOptions::default()).expect("Import");
    let radius = report.min_radius.expect("Radius");
    assert!(
        (radius - 1500.0).abs() <= 150.0,
        "Radius {radius:.0} m statt 1500 m"
    );
    assert!(report.max_cant > 100.0, "Überhöhung {} mm", report.max_cant);
}
