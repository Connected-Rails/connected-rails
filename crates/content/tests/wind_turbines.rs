//! Acceptance test of the wind turbines: a line file with `wind_turbines:` in
//! it reads back whole, and the entries reach the pipeline that stands them
//! up — as scenery, each one naming a model the `wind` mod actually ships.
//!
//! The unit tests in `content::wind` check the pieces — the classes, the
//! register match, the scale. This one checks that the pieces are wired to the
//! pipeline, which is the part that silently does nothing when a builder step
//! is forgotten: a turbine has to reach [`content::terrain::Scenery`] the way a
//! placed hut does, and the object it names has to exist on disk.

use content::route::{LineSource, WindTurbineSource};
use content::terrain::Scenery;

/// One machine of each generation, as an import writes them.
fn turbines() -> Vec<WindTurbineSource> {
    vec![
        // A 1990s machine on a lattice tower.
        content::wind::source_from(
            52.0,
            10.0,
            65.0,
            44.0,
            "Fuhrländer FL 600".into(),
            String::new(),
            false,
        ),
        // The 2000s workhorse.
        content::wind::source_from(
            52.001,
            10.0,
            95.0,
            82.0,
            "REpower MM82".into(),
            String::new(),
            false,
        ),
        // What is being built now, and the one drop-shaped nacelle.
        content::wind::source_from(
            52.002,
            10.0,
            149.0,
            138.0,
            "Enercon E-138 EP3".into(),
            String::new(),
            false,
        ),
    ]
}

/// A line file written with turbines reads back with all of them, and the
/// numbers survive the round trip — a module is the only place they are kept.
#[test]
fn the_turbines_survive_the_line_file() {
    let line = LineSource {
        wind_turbines: turbines(),
        ..LineSource::default()
    };
    let text = ron::ser::to_string_pretty(&line, ron::ser::PrettyConfig::default())
        .expect("a line serialises");
    let read: LineSource = ron::from_str(&text).expect("and reads back");

    assert_eq!(read.wind_turbines.len(), 3);
    let mm82 = &read.wind_turbines[1];
    assert_eq!(mm82.model, "REpower MM82");
    assert_eq!(mm82.hub_height, 95.0);
    assert_eq!(mm82.rotor_diameter, 82.0);
    assert_eq!(mm82.tags, vec!["wea-80"]);
    assert_eq!(mm82.object, "wind:wea_80_standard");
    // The nacelle looks into the prevailing wind, and every machine of one
    // import looks the same way.
    assert!(
        read.wind_turbines
            .iter()
            .all(|t| t.yaw_deg == content::wind::PREVAILING_BEARING)
    );
}

/// Every turbine of a line becomes a scenery object on the terrain, and every
/// object it names is a file the `wind` mod ships.
#[test]
fn every_turbine_is_scenery_the_mod_can_show() {
    let line = LineSource {
        wind_turbines: turbines(),
        ..LineSource::default()
    };
    // Any track will do: the turbines are geo-positioned and never ask it.
    let net = content::musterbahn()
        .compile()
        .expect("the example line compiles")
        .net;
    let scenery = Scenery::from_line(&line, &net, 32);
    assert_eq!(
        scenery.objects(),
        [
            "wind:wea_50_gitter",
            "wind:wea_80_standard",
            "wind:wea_150_enercon"
        ]
    );

    let objects = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods/wind/objects");
    for name in scenery.objects() {
        let stem = name.strip_prefix("wind:").expect("mod-qualified object");
        let path = std::path::Path::new(objects).join(format!("{stem}.ron"));
        assert!(
            path.exists(),
            "{name} has no object file at {}",
            path.display()
        );
        let text = std::fs::read_to_string(&path).expect("readable");
        let object: track_model::TrackObject = ron::from_str(&text).expect("parses");
        assert_eq!(object.lod_distances.len(), 4, "{name}: four levels");
        let model = std::path::Path::new(objects)
            .join("..")
            .join("..")
            .join(&object.model);
        assert!(
            model.exists(),
            "{name}: model {} is missing",
            model.display()
        );
    }
}

/// Every model the mod ships carries the two nodes the game moves, and the
/// rotor says how big and how fast it is.
#[test]
fn every_model_has_its_moving_parts() {
    let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods/wind/assets");
    let mut models = 0;
    for entry in std::fs::read_dir(assets).expect("the wind mod is in the repository") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("gltf") {
            continue;
        }
        models += 1;
        let text = std::fs::read_to_string(&path).expect("readable");
        let gltf: serde_json::Value = serde_json::from_str(&text).expect("json");
        let nodes = gltf["nodes"].as_array().expect("nodes");
        let named = |name: &str| nodes.iter().find(|n| n["name"] == name);
        assert!(
            named("nacelle").is_some(),
            "{}: no nacelle node",
            path.display()
        );
        let rotor = named("rotor").unwrap_or_else(|| panic!("{}: no rotor node", path.display()));
        let extras = &rotor["extras"];
        assert!(extras["rotor_diameter"].as_f64().unwrap_or(0.0) > 10.0);
        assert!(extras["rated_rpm"].as_f64().unwrap_or(0.0) > 5.0);
        assert!(extras["hub_height"].as_f64().unwrap_or(0.0) > 30.0);
        for level in 0..4 {
            assert!(
                named(&format!("rotor_LOD{level}")).is_some(),
                "{}: rotor level {level}",
                path.display()
            );
            assert!(
                named(&format!("turm_LOD{level}")).is_some(),
                "{}: tower level {level}",
                path.display()
            );
        }
    }
    assert_eq!(models, 10, "four classes, ten builds");
}
