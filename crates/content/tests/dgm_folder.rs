//! Abnahme: DGM1-Kacheln aus einem Verzeichnis, verzögert geladen (Plan Kap. 14/15).

use content::import::dgm::TerrainSource;
use std::path::PathBuf;

/// Legt ein Verzeichnis mit vier 1-km-Kacheln nach dem Blattschnitt der Länder an.
/// Rasterweite 50 m statt 1 m, damit der Test in Millisekunden läuft — für die
/// Indizierung und den Cache ist das derselbe Fall.
fn fixture(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("trainsim-dgm-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("unterordner")).unwrap();

    let cell = 50.0;
    for (kx, ky) in [(600u32, 5760u32), (601, 5760), (600, 5761), (601, 5761)] {
        let mut text = String::new();
        for iy in 0..=(1000.0 / cell) as u32 {
            for ix in 0..=(1000.0 / cell) as u32 {
                let x = kx as f64 * 1000.0 + ix as f64 * cell;
                let y = ky as f64 * 1000.0 + iy as f64 * cell;
                // Höhe hängt eindeutig an der Kachel — so ist prüfbar, welche geladen wurde.
                let z = 100.0 + (kx - 600) as f64 * 10.0 + (ky - 5760) as f64 * 100.0;
                text.push_str(&format!("{x} {y} {z}\n"));
            }
        }
        // Zwei Kacheln liegen in einem Unterordner (rekursive Suche).
        let sub = if ky == 5761 { "unterordner" } else { "" };
        let path = dir.join(sub).join(format!("dgm1_32_{kx}_{ky}_1_ni.xyz"));
        std::fs::write(path, text).unwrap();
    }
    // Eine Datei, die kein DGM ist — muss ignoriert werden.
    std::fs::write(dir.join("liesmich.md"), "kein Raster").unwrap();
    dir
}

#[test]
fn verzeichnis_wird_rekursiv_indiziert() {
    let dir = fixture("index");
    let source = TerrainSource::from_dir(&dir, 32).expect("Verzeichnis lesbar");
    assert_eq!(source.tile_count(), 4, "vier Kacheln, .md ignoriert");
    assert_eq!(
        source.load_count(),
        0,
        "beim Indizieren wird nichts geladen"
    );

    let (x0, y0, x1, y1) = source.bounds().unwrap();
    assert_eq!((x0, y0), (600_000.0, 5_760_000.0));
    assert_eq!((x1, y1), (602_000.0, 5_762_000.0));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn kacheln_werden_erst_bei_bedarf_geladen() {
    let dir = fixture("lazy");
    let mut source = TerrainSource::from_dir(&dir, 32).expect("Verzeichnis lesbar");

    // Abfrage in der Südwestkachel.
    assert_eq!(source.height_at_utm(600_500.0, 5_760_500.0), Some(100.0));
    assert_eq!(source.load_count(), 1, "genau eine Kachel geladen");

    // Zweite Abfrage in derselben Kachel kommt aus dem Cache.
    assert_eq!(source.height_at_utm(600_600.0, 5_760_600.0), Some(100.0));
    assert_eq!(source.load_count(), 1);

    // Nachbarkacheln liefern ihre eigenen Höhen.
    assert_eq!(source.height_at_utm(601_500.0, 5_760_500.0), Some(110.0));
    assert_eq!(source.height_at_utm(600_500.0, 5_761_500.0), Some(200.0));
    assert_eq!(source.height_at_utm(601_500.0, 5_761_500.0), Some(210.0));
    assert_eq!(source.load_count(), 4);

    // Außerhalb des Gebiets gibt es keine Höhe — und keinen Ladeversuch.
    assert_eq!(source.height_at_utm(700_000.0, 5_760_000.0), None);
    assert_eq!(source.load_count(), 4);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn cache_bleibt_begrenzt() {
    let dir = fixture("cache");
    let mut source = TerrainSource::from_dir(&dir, 32).expect("Verzeichnis lesbar");
    source.cache_limit = 2;

    // Reihum durch alle vier Kacheln: der Cache hält nur zwei, also wird nachgeladen.
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
        "kleiner Cache muss nachladen: {}",
        source.load_count()
    );
    // Aber die Werte bleiben korrekt — genau darum geht es.
    assert_eq!(source.height_at_utm(601_500.0, 5_761_500.0), Some(210.0));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn gelaende_aus_dem_verzeichnis() {
    use content::terrain::{TerrainOptions, build};
    use track_model::{EdgeId, NodeKind, Segment, TrackEdge, TrackNetwork};
    use world_coords::geo;

    let dir = fixture("terrain");
    let mut source = TerrainSource::from_dir(&dir, 32).expect("Verzeichnis lesbar");

    // Gleis quer durch das Kachelgebiet legen.
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
    let (tiles, stats) = build(&net, Some(&mut source), &options);

    assert!(stats.tiles > 0);
    assert!(stats.triangles > 1000);
    // Der Korridor ragt an den Enden über den Blattschnitt hinaus; dort greift die
    // Ersatzhöhe. Der überwiegende Teil muss aber aus dem DGM kommen.
    let covered = 1.0 - stats.missing as f64 / stats.vertices as f64;
    assert!(covered > 0.6, "nur {:.0} % aus dem DGM", covered * 100.0);
    assert!(
        stats.tile_loads <= source.tile_count(),
        "jede DGM-Kachel höchstens einmal je Cachedurchlauf geladen"
    );
    // Alle Meshdaten sind endlich und liegen nahe am Anker.
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
        assert!(max <= options.tile_size as f32, "{max} m vom Anker");
    }
    std::fs::remove_dir_all(dir).ok();
}
