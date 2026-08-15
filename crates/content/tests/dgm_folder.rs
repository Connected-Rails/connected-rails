//! Acceptance test: DGM1 tiles from a directory, loaded lazily (plan ch. 14/15).

use content::import::dgm::TerrainSource;
use std::path::PathBuf;

/// Creates a directory with four 1 km tiles following the states' sheet layout.
/// Grid spacing 50 m instead of 1 m so that the test runs in milliseconds — for the
/// indexing and the cache that is the same case.
fn fixture(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("trainsim-dgm-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("subfolder")).unwrap();

    let cell = 50.0;
    for (kx, ky) in [(600u32, 5760u32), (601, 5760), (600, 5761), (601, 5761)] {
        let mut text = String::new();
        for iy in 0..=(1000.0 / cell) as u32 {
            for ix in 0..=(1000.0 / cell) as u32 {
                let x = kx as f64 * 1000.0 + ix as f64 * cell;
                let y = ky as f64 * 1000.0 + iy as f64 * cell;
                // The height identifies the tile — so it is checkable which one was loaded.
                let z = 100.0 + (kx - 600) as f64 * 10.0 + (ky - 5760) as f64 * 100.0;
                text.push_str(&format!("{x} {y} {z}\n"));
            }
        }
        // Two tiles live in a subfolder (recursive search).
        let sub = if ky == 5761 { "subfolder" } else { "" };
        let path = dir.join(sub).join(format!("dgm1_32_{kx}_{ky}_1_ni.xyz"));
        std::fs::write(path, text).unwrap();
    }
    // A file that is not a DGM — must be ignored.
    std::fs::write(dir.join("readme.md"), "no grid").unwrap();
    dir
}

#[test]
fn directory_is_indexed_recursively() {
    let dir = fixture("index");
    let source = TerrainSource::from_dir(&dir, 32).expect("directory readable");
    assert_eq!(source.tile_count(), 4, "four tiles, .md ignored");
    assert_eq!(source.load_count(), 0, "nothing is loaded while indexing");

    let (x0, y0, x1, y1) = source.bounds().unwrap();
    assert_eq!((x0, y0), (600_000.0, 5_760_000.0));
    assert_eq!((x1, y1), (602_000.0, 5_762_000.0));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn tiles_are_loaded_only_on_demand() {
    let dir = fixture("lazy");
    let mut source = TerrainSource::from_dir(&dir, 32).expect("directory readable");

    // Query in the south-west tile.
    assert_eq!(source.height_at_utm(600_500.0, 5_760_500.0), Some(100.0));
    assert_eq!(source.load_count(), 1, "exactly one tile loaded");

    // A second query in the same tile comes from the cache.
    assert_eq!(source.height_at_utm(600_600.0, 5_760_600.0), Some(100.0));
    assert_eq!(source.load_count(), 1);

    // Neighbouring tiles supply their own heights.
    assert_eq!(source.height_at_utm(601_500.0, 5_760_500.0), Some(110.0));
    assert_eq!(source.height_at_utm(600_500.0, 5_761_500.0), Some(200.0));
    assert_eq!(source.height_at_utm(601_500.0, 5_761_500.0), Some(210.0));
    assert_eq!(source.load_count(), 4);

    // Outside the area there is no height — and no load attempt.
    assert_eq!(source.height_at_utm(700_000.0, 5_760_000.0), None);
    assert_eq!(source.load_count(), 4);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn cache_stays_bounded() {
    let dir = fixture("cache");
    let mut source = TerrainSource::from_dir(&dir, 32).expect("directory readable");
    source.cache_limit = 2;

    // Round robin through all four tiles: the cache holds only two, so it reloads.
    let points = [
        (600_500.0, 5_760_500.0, 100.0),
        (601_500.0, 5_760_500.0, 110.0),
        (600_500.0, 5_761_500.0, 200.0),
        (601_500.0, 5_761_500.0, 210.0),
    ];
    for _ in 0..3 {
        for (x, y, z) in points {
            assert_eq!(source.height_at_utm(x, y), Some(z));
        }
    }
    assert!(
        source.load_count() > 4,
        "a small cache has to reload: {}",
        source.load_count()
    );
    // But the values stay correct — that is the whole point.
    assert_eq!(source.height_at_utm(601_500.0, 5_761_500.0), Some(210.0));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn terrain_from_the_directory() {
    use content::terrain::{TerrainOptions, build};
    use track_model::{EdgeId, NodeKind, Segment, TrackEdge, TrackNetwork};
    use world_coords::geo;

    let dir = fixture("terrain");
    let mut sources = [TerrainSource::from_dir(&dir, 32).expect("directory readable")];

    // Lay a track right across the tile area.
    let (lat, lon) = geo::from_utm(600_200.0, 5_760_500.0, 32);
    let mut net = TrackNetwork::new();
    let a = net.add_node(NodeKind::Buffer);
    let b = net.add_node(NodeKind::Buffer);
    net.add_edge(TrackEdge::new(
        EdgeId(0),
        a,
        b,
        geo::to_ecef(lat, lon, 150.0),
        0.0,
        vec![Segment::straight(1500.0)],
    ));

    let options = TerrainOptions {
        radius: 300.0,
        ..Default::default()
    };
    let (tiles, stats) = build(&net, &mut sources, &options);

    assert!(stats.tiles > 0);
    assert!(stats.triangles > 1000);
    // At its ends the corridor extends beyond the sheet layout; there the fallback
    // height applies. The bulk of it must come from the DGM though.
    let covered = 1.0 - stats.missing as f64 / stats.vertices as f64;
    assert!(covered > 0.6, "only {:.0} % from the DGM", covered * 100.0);
    assert!(
        stats.tile_loads <= sources[0].tile_count(),
        "each DGM tile loaded at most once per cache pass"
    );
    // All mesh data is finite and lies close to the anchor.
    for tile in &tiles {
        assert!(
            tile.positions
                .iter()
                .all(|p| p.iter().all(|v| v.is_finite()))
        );
        let max = tile
            .positions
            .iter()
            .map(|p| p[0].abs().max(p[2].abs()))
            .fold(0.0f32, f32::max);
        assert!(max <= options.tile_size as f32, "{max} m from the anchor");
    }
    std::fs::remove_dir_all(dir).ok();
}
