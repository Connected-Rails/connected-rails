//! The weights that ship, against the descriptions that ship with them.
//!
//! `ai.ron`'s entries say how big the input is, what head the model has and how
//! many classes it has — and every one of those is a number that can disagree
//! with the file without anything complaining. A wrong input size decodes into
//! boxes in the wrong place; a wrong class count reads the score of one class
//! as the box of another. Both are silent. This runs the two shipped models
//! once each and checks that what comes out is the shape the registry claims.
//!
//! It is skipped where the weights are not there. That is not laziness: they
//! are Git LFS objects, a clone without `git lfs pull` has the pointer files
//! instead, and a test that failed then would be reporting on the clone rather
//! than on the code. What it must never do is pass quietly on a pointer file,
//! so the size is checked before anything else.

#![cfg(feature = "onnx")]

use std::path::{Path, PathBuf};
use vision::model::{Head, VisionConfig};

/// Where `models/` is, from this crate.
fn models() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The weights, or `None` with a word about why — a missing file and a Git LFS
/// pointer left unfetched are different things and deserve different words.
fn weights(path: &Path) -> Option<PathBuf> {
    let Ok(size) = std::fs::metadata(path).map(|m| m.len()) else {
        eprintln!("skipped: {} is not there", path.display());
        return None;
    };
    // An LFS pointer is a few hundred bytes of text; the smallest real model
    // here is twelve megabytes.
    if size < 1_000_000 {
        eprintln!(
            "skipped: {} is {size} bytes — a Git LFS pointer, not the weights. \
             Run `git lfs pull`.",
            path.display()
        );
        return None;
    }
    Some(path.to_path_buf())
}

/// A flat grey window of the size the model asks for.
fn window(width: u32, height: u32) -> Vec<u8> {
    vec![128u8; (width * height * 3) as usize]
}

/// The car detector: one tensor, four rows of box, fifteen classes and an
/// angle. If the export ever came out with a different class list, the
/// registry's fifteen would silently read the wrong rows.
#[test]
fn the_shipped_car_detector_runs_and_fits_its_entry() {
    let config = VisionConfig::default();
    let spec = config.model_by_id("dota-obb").expect("the registry has it");
    let Some(path) = weights(&spec.path(&models())) else {
        return;
    };
    assert!(matches!(spec.head, Head::Oriented { .. }));
    assert_eq!(spec.classes.len(), 15);
    let mut detector = vision::load_detector(spec, &path).expect("loads");
    let found = detector
        .detect(
            &window(spec.input.width, spec.input.height),
            spec.input.width,
            spec.input.height,
        )
        .expect("a grey window decodes");
    // Grey is not a car park, so what matters is that it decoded at all: a
    // mismatched class count is an error out of `decode`, not an empty list.
    assert!(
        found.iter().all(|d| d.class < 15),
        "a class outside the registry's list"
    );
}

/// The tree detector: two tensors and an anchor grid that has to come out at
/// exactly the length the file's own output says. This is the test that would
/// fail if the model were ever re-exported at another input size and the
/// registry not moved with it.
#[test]
fn the_shipped_tree_detector_runs_and_fits_its_grid() {
    let config = VisionConfig::default();
    let spec = config
        .model_by_id("deepforest")
        .expect("the registry has it");
    let Some(path) = weights(&spec.path(&models())) else {
        return;
    };
    assert!(matches!(spec.head, Head::Retina { .. }));
    assert_eq!(spec.classes.len(), 1);
    assert_eq!(
        (spec.input.width, spec.input.height),
        (768, 768),
        "the grid is rebuilt from this, and the file was exported for it"
    );
    let mut detector = vision::load_detector(spec, &path).expect("loads");
    let found = detector
        .detect(
            &window(spec.input.width, spec.input.height),
            spec.input.width,
            spec.input.height,
        )
        .expect("the anchor grid matches the model's own output");
    // A flat grey field has no crowns in it. Anything found here at the
    // registry's own threshold would mean the decoding is producing boxes out
    // of noise, which is worse than finding nothing.
    assert!(
        found.is_empty(),
        "{} crowns in a flat grey window",
        found.len()
    );
}

/// Both entries name a file under `models/`, which is the directory that
/// ships. A path that pointed somewhere else would work on the machine it was
/// written on and nowhere else.
#[test]
fn the_shipped_entries_point_into_the_shipped_directory() {
    let config = VisionConfig::default();
    for id in ["dota-obb", "deepforest"] {
        let spec = config.model_by_id(id).expect(id);
        assert_eq!(
            spec.file.parent().and_then(|p| p.to_str()),
            Some("models"),
            "{id} points at {}",
            spec.file.display()
        );
        assert!(
            !spec.note.is_empty(),
            "{id} says nothing about where it is from"
        );
    }
}
