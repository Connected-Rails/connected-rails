//! The inference backend against a real ONNX file.
//!
//! Everything else about this crate can be tested with a hand-written
//! detector; this is the part that cannot. `fixtures/tiny-obb.onnx` is 253
//! bytes of oriented head whose every number is the mean brightness of the
//! window times a constant (see `fixtures/make_tiny_obb.py`), which is enough
//! to hold the whole path to account at once: a byte becomes a float the way
//! the spec says, the tensor is laid out the way the model expects, tract runs
//! it, and the output is read back in the right order.

#![cfg(feature = "onnx")]

use std::path::PathBuf;
use vision::model::{ClassSpec, Head, InputSpec, ModelSpec};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tiny-obb.onnx")
}

/// A model description that fits the fixture: eight pixels square, an oriented
/// head, two classes, one of which is placed.
fn spec() -> ModelSpec {
    ModelSpec {
        id: "tiny".into(),
        name: "Tiny OBB".into(),
        file: "tiny-obb.onnx".into(),
        input: InputSpec {
            width: 8,
            height: 8,
            ..Default::default()
        },
        head: Head::Oriented {
            confidence: 0.25,
            iou: 0.45,
        },
        classes: vec![
            ClassSpec::placed("car", "car", (4.4, 1.8)),
            ClassSpec::ignored("nothing"),
        ],
        ground_sample: 0.3,
        overlap: 0.2,
        note: String::new(),
    }
}

fn window(value: u8) -> Vec<u8> {
    vec![value; 8 * 8 * 3]
}

#[test]
fn a_bright_window_comes_back_with_the_models_own_numbers() {
    let spec = spec();
    let mut detector =
        vision::load_detector(&spec, &fixture()).expect("the fixture is a model tract can run");
    let found = detector.detect(&window(255), 8, 8).expect("it runs");
    assert_eq!(found.len(), 1, "one anchor, one detection");
    let d = found[0];
    assert_eq!(d.class, 0, "the class with the higher score wins");
    assert!((d.score - 0.9).abs() < 1e-4, "{}", d.score);
    assert!((d.cx - 4.0).abs() < 1e-4, "{}", d.cx);
    assert!((d.cy - 2.0).abs() < 1e-4, "{}", d.cy);
    assert!((d.w - 6.0).abs() < 1e-4, "{}", d.w);
    assert!((d.h - 3.0).abs() < 1e-4, "{}", d.h);
    assert!((d.angle - 0.5).abs() < 1e-4, "{}", d.angle);
}

#[test]
fn the_window_actually_reaches_the_model() {
    // Half brightness has to halve every number that comes back. If the
    // pixels never arrived — a wrong layout, a forgotten scaling — this is
    // where it shows, and nowhere else.
    let spec = spec();
    let mut detector = vision::load_detector(&spec, &fixture()).unwrap();
    let found = detector.detect(&window(128), 8, 8).expect("it runs");
    assert_eq!(found.len(), 1);
    let expected = 0.9 * 128.0 / 255.0;
    assert!(
        (found[0].score - expected).abs() < 1e-3,
        "{} vs {expected}",
        found[0].score
    );
}

#[test]
fn a_dark_window_finds_nothing() {
    // 0.9 × 60/255 = 0.21, under the head's own threshold — the floor is
    // applied inside the backend, before a box is even built.
    let spec = spec();
    let mut detector = vision::load_detector(&spec, &fixture()).unwrap();
    assert!(detector.detect(&window(60), 8, 8).unwrap().is_empty());
}

#[test]
fn a_window_of_the_wrong_size_is_refused_rather_than_run() {
    let spec = spec();
    let mut detector = vision::load_detector(&spec, &fixture()).unwrap();
    let error = detector.detect(&window(255), 16, 16).unwrap_err();
    assert!(error.contains("8x8"), "{error}");
}

#[test]
fn a_file_that_is_not_a_model_says_so_with_its_name() {
    let spec = spec();
    let missing = PathBuf::from("/tmp/there-is-no-such-model.onnx");
    let error = match vision::load_detector(&spec, &missing) {
        Ok(_) => panic!("a model that is not there cannot load"),
        Err(error) => error,
    };
    assert!(error.contains("there-is-no-such-model"), "{error}");
}

/// A whole run over a region, with the imagery mocked out: the fixture finds
/// its one car in every window, and what comes back has to be on the map,
/// clear of the track, and in metres.
#[test]
fn a_run_over_a_corridor_puts_the_finds_on_the_ground() {
    use imagery::DecodedTile;
    use vision::region::{Region, Shape};

    let mut spec = spec();
    // Sixteen pixels of ground per window at the model's own resolution keeps
    // the walk to a handful of windows.
    spec.ground_sample = 0.6;
    let mut detector = vision::load_detector(&spec, &fixture()).unwrap();
    let mut sheet = vision::Sheet::new(19, 256, 64, |id| {
        Some(DecodedTile {
            tile: id,
            width: 256,
            height: 256,
            // Bright, so the fixture reports its car in every window.
            pixels: vec![255; 256 * 256 * 4],
        })
    });
    let track = vec![vec![(51.0, 7.0), (51.0, 7.002)]];
    let region = Region::new(Shape::Corridor { radius: 50.0 }, &track, 7.0, 32);

    let outcome = vision::run(&mut sheet, detector.as_mut(), &spec, &region, &mut |_| true)
        .expect("the run comes to an end");
    assert!(
        !outcome.found.is_empty(),
        "the fixture finds a car in every window"
    );
    assert_eq!(outcome.blank, 0);
    for car in &outcome.found {
        assert_eq!(car.place, "car");
        let distance = region.track_distance(car.lat, car.lon);
        assert!(
            distance >= 7.0,
            "a car came within the clearance: {distance}"
        );
        assert!(distance <= 50.0, "a car came from outside the corridor");
        // 6 px of a 0.6 m/px window is 3.6 m — a car, and inside what the
        // class calls plausible.
        assert!((car.length - 3.6).abs() < 0.6, "{}", car.length);
    }
}
