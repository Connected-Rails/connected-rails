//! Acceptance test of the overhead lines: a line file with `power_lines:` in
//! it comes out of the terrain build as masts standing on the ground and
//! conductors hanging between them.
//!
//! The unit tests in `content::power` check the pieces — the preset table, the
//! conductor positions, the sag. This one checks that the pieces are actually
//! wired to the pipeline, which is the part that silently does nothing when a
//! builder step is forgotten: the masts have to reach
//! [`content::terrain::Vegetation`] and the conductors have to reach the tiles.

use content::TerrainBuilder;
use content::power::PowerLines;
use content::route::LineSource;
use content::terrain::{TerrainOptions, Vegetation};

/// The example module, which carries a Bahnstromleitung along the track and a
/// 380 kV line crossing it.
fn modul_west() -> LineSource {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../mods/example/lines/modul_west.ron"
    );
    let text = std::fs::read_to_string(path).expect("the example module is in the repository");
    ron::from_str(&text).expect("the example module parses")
}

#[test]
fn the_example_module_carries_two_overhead_lines() {
    let line = modul_west();
    assert_eq!(line.power_lines.len(), 2, "Bahnstrom and 380 kV");

    let bahn = &line.power_lines[0];
    assert_eq!(bahn.arms.len(), 1, "one crossarm");
    assert_eq!(bahn.arms[0].conductors, 4, "two two-pole circuits");
    assert!(bahn.points.first().expect("a first mast").tension);
    assert!(bahn.points.last().expect("a last mast").tension);

    let grid = &line.power_lines[1];
    assert_eq!(grid.arms.len(), 2, "the Donaumast's two crossarms");
    assert_eq!(grid.arms.iter().map(|a| a.conductors).sum::<u8>(), 6);
}

/// The masts travel with the vegetation, so every mast of every line is an
/// instance the tile pipeline can place — and it names an object the `pylons`
/// mod actually ships.
#[test]
fn every_mast_becomes_an_instance() {
    let line = modul_west();
    let masts = content::power::masts(&line.power_lines);
    let expected: usize = line.power_lines.iter().map(|l| l.points.len()).sum();
    assert_eq!(masts.len(), expected);

    let objects = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods/pylons/objects");
    for mast in &masts {
        let stem = mast
            .object
            .strip_prefix("pylons:")
            .expect("mod-qualified object");
        let path = std::path::Path::new(objects).join(format!("{stem}.ron"));
        assert!(
            path.exists(),
            "{}: {} is missing",
            mast.object,
            path.display()
        );
    }
}

/// The whole way through: line file → terrain builder → tiles with masts
/// standing on them and conductors crossing them.
///
/// No elevation data, so the ground is the flat fallback — which is exactly
/// what a test scene has, and what the mast feet then sit on.
#[test]
fn the_tiles_carry_the_masts_and_the_conductors() {
    let line = modul_west();
    let compiled = line.compile().expect("the example module compiles");
    let options = TerrainOptions {
        zone: 32,
        geoid_offset: line.geoid_offset,
        ..Default::default()
    };
    let builder = TerrainBuilder::new(&compiled.net, Vec::new(), options)
        .with_vegetation(Vegetation::from_line(&line, options.zone))
        .with_power_lines(PowerLines::from_line(
            &line,
            options.zone,
            options.tile_size,
        ));

    let mut masts = 0usize;
    let mut conductor_triangles = 0usize;
    let mut tiles_with_wire = 0usize;
    let mut stats = content::terrain::TerrainStats::default();
    for k in builder.corridor_keys() {
        let Some(tile) = builder.build_key(k, &mut stats) else {
            continue;
        };
        // A mast is a tree as far as the tile is concerned; the ones that name
        // a `pylons:` object are ours.
        masts += tile
            .trees
            .iter()
            .filter(|t| {
                t.object
                    .and_then(|i| builder.tree_objects().get(i as usize))
                    .is_some_and(|name| name.starts_with("pylons:"))
            })
            .count();
        if !tile.conductors.is_empty() {
            tiles_with_wire += 1;
        }
        conductor_triangles += tile.conductors.iter().map(|p| p.triangles()).sum::<usize>();
    }

    let expected: usize = line.power_lines.iter().map(|l| l.points.len()).sum();
    assert!(
        masts > 0 && masts <= expected,
        "{masts} masts on the tiles, {expected} in the file"
    );
    assert!(conductor_triangles > 0, "the conductors were strung");
    assert!(
        tiles_with_wire > 1,
        "a line crosses more than one tile: {tiles_with_wire}"
    );
}

/// A module without overhead lines builds exactly as it did before: no masts,
/// no conductors, and nothing that costs anything.
#[test]
fn a_module_without_overhead_lines_carries_none() {
    let mut line = modul_west();
    line.power_lines.clear();
    let compiled = line.compile().expect("compiles");
    let options = TerrainOptions {
        zone: 32,
        geoid_offset: line.geoid_offset,
        ..Default::default()
    };
    let builder = TerrainBuilder::new(&compiled.net, Vec::new(), options).with_power_lines(
        PowerLines::from_line(&line, options.zone, options.tile_size),
    );
    let mut stats = content::terrain::TerrainStats::default();
    for k in builder.corridor_keys() {
        if let Some(tile) = builder.build_key(k, &mut stats) {
            assert!(tile.conductors.is_empty());
        }
    }
}
