//! Geländemeshes aus dem DGM (Plan Kap. 14).
//!
//! Erzeugt wird nur der Korridor um die Strecke, gekachelt und mit
//! entfernungsabhängiger Auflösung. Gerechnet wird durchgehend in UTM — das ist das
//! System des DGM, dadurch kostet jeder Stützpunkt genau eine Projektion statt drei.
//!
//! Die Kennzahl, um die es geht: ein Quadratkilometer DGM1 hat zwei Millionen Dreiecke.
//! Deshalb
//!
//! * **Kacheln** (Vorgabe 512 m) → Frustum-Culling und Sichtweitenbegrenzung je Kachel,
//! * **LOD nach Gleisabstand** → 4 m am Gleis, 32 m am Rand des Korridors,
//! * **Schürzen** an den Kachelrändern → keine Risse zwischen verschiedenen Stufen,
//! * **Einschnitt/Damm**: nahe am Gleis wird das Gelände auf die Schienenhöhe gezogen,
//!   sonst läge die Trasse im Hügel.

use crate::import::dgm::TerrainSource;
use glam::DVec2;
use std::collections::HashMap;
use track_model::TrackNetwork;
use world_coords::{EcefPos, EnuFrame, geo};

/// Einstellungen der Geländeerzeugung.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerrainOptions {
    /// UTM-Zone, in der gerechnet wird (die des DGM).
    pub zone: u8,
    /// Geoid-Undulation der Strecke [m] (NHN → ellipsoidisch).
    pub geoid_offset: f64,
    /// Kantenlänge einer Geländekachel [m].
    pub tile_size: f64,
    /// Wie weit neben dem Gleis Gelände entsteht [m].
    pub radius: f64,
    /// Feinste Rasterweite [m] (gilt im Korridor).
    pub base_step: f64,
    /// Bis hierhin gilt die feinste Stufe [m].
    pub corridor: f64,
    /// Bis hierhin folgt das Gelände exakt der Schienenhöhe [m].
    pub flatten: f64,
    /// Bis hierhin wird zwischen Schienen- und Geländehöhe überblendet [m].
    pub blend: f64,
    /// Höhe der Schürze an den Kachelrändern [m].
    pub skirt: f64,
    /// Höhe, wo kein DGM vorliegt [m] (NHN).
    pub fallback_height: f64,
    /// Abtastabstand der Gleisachse [m].
    pub centerline_step: f64,
}

impl Default for TerrainOptions {
    fn default() -> Self {
        Self {
            zone: 32,
            geoid_offset: 46.0,
            tile_size: 512.0,
            radius: 1_200.0,
            base_step: 4.0,
            corridor: 96.0,
            flatten: 10.0,
            blend: 45.0,
            skirt: 8.0,
            fallback_height: 100.0,
            centerline_step: 25.0,
        }
    }
}

/// Eine fertige Geländekachel — rohe Meshdaten, damit `content` ohne Bevy auskommt.
#[derive(Debug, Clone)]
pub struct TerrainTile {
    /// Ursprung des lokalen ENU-Frames (Kachelmitte) in Weltkoordinaten.
    pub anchor: EcefPos,
    /// Positionen in Renderachsen (x = Ost, y = oben, z = −Nord), relativ zum Anker.
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// Verwendete Rasterweite [m].
    pub step: f64,
    /// LOD-Stufe (0 = feinste).
    pub lod: u8,
    /// Umkreisradius um den Anker [m] — für Sichtweite und Culling.
    pub radius: f32,
}

impl TerrainTile {
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }
}

/// Kennzahlen eines Geländeaufbaus.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TerrainStats {
    pub tiles: usize,
    pub vertices: usize,
    pub triangles: usize,
    /// Stützpunkte ohne DGM-Wert (Ersatzhöhe verwendet).
    pub missing: usize,
    /// Wie oft eine DGM-Kachel von der Platte gelesen wurde.
    pub tile_loads: usize,
}

impl TerrainStats {
    /// Grobe Größe der Meshdaten im Speicher [Byte].
    pub fn memory(&self) -> usize {
        self.vertices * 12 + self.triangles * 12
    }
}

/// Ein Stützpunkt der Gleisachse in UTM.
struct Centerline {
    points: Vec<DVec2>,
    /// Ellipsoidische Höhe der Schienenoberkante [m].
    heights: Vec<f64>,
    /// Beschleunigter Nachbarschaftsindex.
    grid: HashMap<(i64, i64), Vec<usize>>,
    cell: f64,
}

impl Centerline {
    fn build(net: &TrackNetwork, options: &TerrainOptions) -> Self {
        let mut points = Vec::new();
        let mut heights = Vec::new();
        for edge in net.edges() {
            let steps = (edge.length() / options.centerline_step).ceil().max(1.0) as usize;
            for i in 0..=steps {
                let s = edge.length() * i as f64 / steps as f64;
                let pose = edge.eval(s);
                let (lat, lon, h) = geo::from_ecef(pose.pos);
                let (e, n) = geo::to_utm(lat, lon, options.zone);
                points.push(DVec2::new(e, n));
                heights.push(h);
            }
        }

        let cell = options.blend.max(50.0);
        let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        for (i, p) in points.iter().enumerate() {
            grid.entry(key(*p, cell)).or_default().push(i);
        }
        Self {
            points,
            heights,
            grid,
            cell,
        }
    }

    /// Nächster Achspunkt in der Nachbarschaft: `(Abstand, Höhe)`.
    fn nearest(&self, p: DVec2) -> Option<(f64, f64)> {
        let (kx, ky) = key(p, self.cell);
        let mut best: Option<(f64, f64)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                let Some(bucket) = self.grid.get(&(kx + dx, ky + dy)) else {
                    continue;
                };
                for &i in bucket {
                    let d = (self.points[i] - p).length();
                    if best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, self.heights[i]));
                    }
                }
            }
        }
        best
    }

    /// Abstand zur Achse ohne Nachbarschaftsindex (nur für Prüfungen).
    #[cfg(test)]
    fn distance_scan(&self, p: DVec2) -> f64 {
        self.points
            .iter()
            .map(|q| (*q - p).length())
            .fold(f64::INFINITY, f64::min)
    }

    /// Abstand der Achse zur **Fläche** einer Kachel (0, wenn das Gleis hindurchführt).
    ///
    /// Der Abstand zum Kachelmittelpunkt taugt dafür nicht: eine 512-m-Kachel, durch die
    /// das Gleis läuft, hat ihren Mittelpunkt trotzdem hunderte Meter daneben — sie
    /// bekäme sonst die grobe Stufe.
    fn distance_to_rect(&self, min: DVec2, size: f64) -> f64 {
        let max = min + DVec2::splat(size);
        self.points
            .iter()
            .map(|q| {
                let clamped = DVec2::new(q.x.clamp(min.x, max.x), q.y.clamp(min.y, max.y));
                (*q - clamped).length()
            })
            .fold(f64::INFINITY, f64::min)
    }
}

fn key(p: DVec2, cell: f64) -> (i64, i64) {
    ((p.x / cell).floor() as i64, (p.y / cell).floor() as i64)
}

/// Baut das Gelände um alle Gleise des Netzes.
///
/// `source` darf `None` sein — dann entsteht ebenes Gelände auf `fallback_height`,
/// was für Testszenen und Strecken ohne DGM genügt.
pub fn build(
    net: &TrackNetwork,
    source: Option<&mut TerrainSource>,
    options: &TerrainOptions,
) -> (Vec<TerrainTile>, TerrainStats) {
    let centerline = Centerline::build(net, options);
    if centerline.points.is_empty() {
        return (Vec::new(), TerrainStats::default());
    }

    // Kachelraster über den Korridor legen. Reihenfolge sortiert, damit derselbe
    // Streckenzustand immer dieselben Kacheln in derselben Folge ergibt.
    let mut key_set: std::collections::HashSet<(i64, i64)> = std::collections::HashSet::new();
    let reach = (options.radius / options.tile_size).ceil() as i64;
    for p in &centerline.points {
        let (kx, ky) = key(*p, options.tile_size);
        for dx in -reach..=reach {
            for dy in -reach..=reach {
                key_set.insert((kx + dx, ky + dy));
            }
        }
    }
    let mut keys: Vec<(i64, i64)> = key_set.into_iter().collect();
    keys.sort_unstable();

    let mut source = source;
    let mut tiles = Vec::new();
    let mut stats = TerrainStats::default();

    for k in keys {
        let min = DVec2::new(
            k.0 as f64 * options.tile_size,
            k.1 as f64 * options.tile_size,
        );
        let distance = centerline.distance_to_rect(min, options.tile_size);
        // Kacheln, die den Korridor nicht berühren, entfallen ganz.
        if distance > options.radius {
            continue;
        }
        let (step, lod) = level_of_detail(distance, options);
        let tile = build_tile(
            min,
            step,
            lod,
            &centerline,
            source.as_deref_mut(),
            options,
            &mut stats,
        );
        stats.tiles += 1;
        stats.vertices += tile.positions.len();
        stats.triangles += tile.triangles();
        tiles.push(tile);
    }

    if let Some(s) = source {
        stats.tile_loads = s.load_count();
    }
    (tiles, stats)
}

/// Rasterweite und LOD-Stufe nach Abstand zur Gleisachse.
fn level_of_detail(distance: f64, options: &TerrainOptions) -> (f64, u8) {
    let base = options.base_step;
    if distance <= options.corridor {
        (base, 0)
    } else if distance <= options.corridor * 4.0 {
        (base * 2.0, 1)
    } else if distance <= options.corridor * 8.0 {
        (base * 4.0, 2)
    } else {
        (base * 8.0, 3)
    }
}

/// Baut eine einzelne Kachel.
fn build_tile(
    min: DVec2,
    step: f64,
    lod: u8,
    centerline: &Centerline,
    mut source: Option<&mut TerrainSource>,
    options: &TerrainOptions,
    stats: &mut TerrainStats,
) -> TerrainTile {
    let n = (options.tile_size / step).round().max(1.0) as usize;
    let center = min + DVec2::splat(options.tile_size / 2.0);

    // Anker in der Kachelmitte, damit die lokalen f32-Koordinaten klein bleiben.
    let (clat, clon) = geo::from_utm(center.x, center.y, options.zone);
    let anchor = geo::to_ecef(clat, clon, 0.0);
    let frame = EnuFrame::at(anchor);

    let mut positions = Vec::with_capacity((n + 1) * (n + 1));
    let mut heights = Vec::with_capacity((n + 1) * (n + 1));

    for iy in 0..=n {
        for ix in 0..=n {
            let p = min + DVec2::new(ix as f64 * step, iy as f64 * step);
            let ground = source
                .as_deref_mut()
                .and_then(|s| s.height_at_utm(p.x, p.y))
                .map(|h| h + options.geoid_offset)
                .unwrap_or_else(|| {
                    stats.missing += 1;
                    options.fallback_height + options.geoid_offset
                });

            // Einschnitt/Damm: am Gleis exakt Schienenhöhe, dann überblenden.
            let height = match centerline.nearest(p) {
                Some((d, rail)) if d <= options.flatten => rail,
                Some((d, rail)) if d <= options.blend => {
                    let t = (d - options.flatten) / (options.blend - options.flatten);
                    rail * (1.0 - t) + ground * t
                }
                _ => ground,
            };
            heights.push(height);

            let (lat, lon) = geo::from_utm(p.x, p.y, options.zone);
            let world = geo::to_ecef(lat, lon, height);
            positions.push(to_render(frame.to_local(world)));
        }
    }

    // Reguläre Triangulierung.
    let row = n + 1;
    let mut indices = Vec::with_capacity(n * n * 6);
    for iy in 0..n {
        for ix in 0..n {
            let a = (iy * row + ix) as u32;
            let b = a + 1;
            let c = a + row as u32;
            let d = c + 1;
            indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }

    // Schürze: der Rand wird nach unten verlängert, damit an LOD-Grenzen keine
    // Risse sichtbar werden.
    add_skirt(
        &mut positions,
        &mut indices,
        &heights,
        min,
        step,
        n,
        &frame,
        options,
    );

    let radius = (options.tile_size * 0.75) as f32;
    TerrainTile {
        anchor,
        positions,
        indices,
        step,
        lod,
        radius,
    }
}

/// Hängt eine senkrechte Schürze an den Kachelrand.
#[allow(clippy::too_many_arguments)]
fn add_skirt(
    positions: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
    heights: &[f64],
    min: DVec2,
    step: f64,
    n: usize,
    frame: &EnuFrame,
    options: &TerrainOptions,
) {
    let row = n + 1;
    // Randpunkte einmal im Uhrzeigersinn.
    let border: Vec<usize> = (0..row) // Südrand
        .chain((1..row).map(|iy| iy * row + n)) // Ostrand
        .chain((0..n).rev().map(|ix| n * row + ix)) // Nordrand
        .chain((0..n).rev().map(|iy| iy * row)) // Westrand
        .collect();

    let first_skirt = positions.len() as u32;
    for &index in &border {
        let ix = index % row;
        let iy = index / row;
        let p = min + DVec2::new(ix as f64 * step, iy as f64 * step);
        let (lat, lon) = geo::from_utm(p.x, p.y, options.zone);
        let world = geo::to_ecef(lat, lon, heights[index] - options.skirt);
        positions.push(to_render(frame.to_local(world)));
    }

    for i in 0..border.len() {
        let j = (i + 1) % border.len();
        let top_a = border[i] as u32;
        let top_b = border[j] as u32;
        let bot_a = first_skirt + i as u32;
        let bot_b = first_skirt + j as u32;
        indices.extend_from_slice(&[top_a, bot_a, top_b, top_b, bot_a, bot_b]);
    }
}

/// ENU (x = Ost, y = Nord, z = oben) → Renderachsen.
fn to_render(p: glam::DVec3) -> [f32; 3] {
    [p.x as f32, p.z as f32, -p.y as f32]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::dgm::{HeightTile, TerrainSource};
    use track_model::{EdgeId, NodeKind, Segment, TrackEdge, TrackNetwork};

    /// Gerade Teststrecke von 1 km bei 52° N, 10° O.
    fn test_net() -> TrackNetwork {
        let mut net = TrackNetwork::new();
        let a = net.add_node(NodeKind::Buffer);
        let b = net.add_node(NodeKind::Buffer);
        net.add_edge(TrackEdge::new(
            EdgeId(0),
            a,
            b,
            geo::to_ecef_deg(52.0, 10.0, 100.0),
            0.0,
            vec![Segment::straight(1000.0)],
        ));
        net
    }

    /// DGM mit 25 m Raster über dem Testgebiet: Hang, der nach Norden ansteigt.
    fn test_source() -> TerrainSource {
        let (e0, n0) = geo::to_utm(52.0f64.to_radians(), 10.0f64.to_radians(), 32);
        let mut text = String::new();
        for iy in -60..60 {
            for ix in -20..80 {
                let x = (e0 / 25.0).round() * 25.0 + ix as f64 * 25.0;
                let y = (n0 / 25.0).round() * 25.0 + iy as f64 * 25.0;
                let z = 100.0 + (y - n0) * 0.02;
                text.push_str(&format!("{x} {y} {z}\n"));
            }
        }
        TerrainSource::from_tile(HeightTile::parse_xyz(&text, 32).unwrap())
    }

    fn options() -> TerrainOptions {
        TerrainOptions {
            radius: 400.0,
            ..Default::default()
        }
    }

    #[test]
    fn gelaende_entsteht_nur_im_korridor() {
        let net = test_net();
        let mut source = test_source();
        let (tiles, stats) = build(&net, Some(&mut source), &options());

        assert!(stats.tiles > 0);
        assert_eq!(tiles.len(), stats.tiles);
        // Alle Kacheln liegen im Umkreis der Strecke.
        let centerline = Centerline::build(&net, &options());
        for tile in &tiles {
            let (lat, lon, _) = geo::from_ecef(tile.anchor);
            let (e, n) = geo::to_utm(lat, lon, 32);
            let d = centerline.distance_scan(DVec2::new(e, n));
            assert!(
                d <= options().radius + options().tile_size,
                "Kachel {d:.0} m abseits"
            );
        }
    }

    #[test]
    fn lod_wird_mit_dem_abstand_grober() {
        let net = test_net();
        let mut source = test_source();
        let (tiles, _) = build(&net, Some(&mut source), &options());

        let fein = tiles.iter().filter(|t| t.lod == 0).count();
        let grob = tiles.iter().filter(|t| t.lod > 0).count();
        assert!(fein > 0 && grob > 0, "fein {fein}, grob {grob}");

        // Feine Kacheln haben die Grundschrittweite, grobe ein Vielfaches davon.
        for tile in &tiles {
            let expected = options().base_step * 2f64.powi(tile.lod as i32);
            assert_eq!(tile.step, expected, "LOD {}", tile.lod);
        }
    }

    #[test]
    fn dreieckszahl_bleibt_beherrschbar() {
        let net = test_net();
        let mut source = test_source();
        let (_, stats) = build(&net, Some(&mut source), &options());

        // Zum Vergleich: derselbe Korridor in voller DGM1-Auflösung.
        let area = 1000.0 * 2.0 * options().radius; // m²
        let full_detail = area as usize * 2;
        assert!(
            stats.triangles * 5 < full_detail,
            "{} Dreiecke gegenüber {} bei 1 m Raster",
            stats.triangles,
            full_detail
        );
        // Und trotzdem genug für ein sichtbares Gelände.
        assert!(stats.triangles > 10_000);
        assert!(stats.memory() < 40 * 1024 * 1024, "{} Byte", stats.memory());

        // Der eigentliche Hebel ist die Staffelung: je LOD-Stufe viertelt sich die
        // Dreieckszahl einer gleich großen Kachel.
        let (tiles, _) = build(&net, Some(&mut test_source()), &options());
        let per_lod = |lod: u8| tiles.iter().find(|t| t.lod == lod).map(|t| t.triangles());
        if let (Some(fein), Some(grob)) = (per_lod(0), per_lod(1)) {
            let ratio = fein as f64 / grob as f64;
            assert!(
                (3.0..5.0).contains(&ratio),
                "Verhältnis LOD0:LOD1 = {ratio}"
            );
        }
    }

    #[test]
    fn gelaende_folgt_am_gleis_der_schienenhoehe() {
        let net = test_net();
        let options = options();
        let centerline = Centerline::build(&net, &options);
        let (tiles, _) = build(&net, Some(&mut test_source()), &options);

        // Testdaten müssen sich unterscheiden, sonst prüft der Test nichts.
        let p = centerline.points[10];
        let (_, rail) = centerline.nearest(p).unwrap();
        let ground = test_source().height_at_utm(p.x, p.y).unwrap() + options.geoid_offset;
        assert!(
            (ground - rail).abs() > 0.5,
            "Gelände und Schienenhöhe müssen auseinanderliegen: {ground} vs {rail}"
        );

        // Jeden Geländepunkt zurückrechnen und mit dem Erwartungswert vergleichen:
        // am Gleis Schienenhöhe, weit weg DGM-Höhe.
        let mut source = test_source();
        let mut checked_rail = 0;
        let mut checked_far = 0;
        for tile in &tiles {
            let frame = EnuFrame::at(tile.anchor);
            // Nur die Rasterpunkte prüfen — die Schürze hängt absichtlich darunter.
            let n = (options.tile_size / tile.step).round() as usize;
            let grid_vertices = (n + 1) * (n + 1);
            for pos in &tile.positions[..grid_vertices] {
                let local = glam::DVec3::new(pos[0] as f64, -pos[2] as f64, pos[1] as f64);
                let world = frame.to_ecef(local);
                let (lat, lon, height) = geo::from_ecef(world);
                let (e, n) = geo::to_utm(lat, lon, options.zone);
                let Some((d, rail)) = centerline.nearest(DVec2::new(e, n)) else {
                    continue;
                };
                if d < options.flatten * 0.5 && height > 0.0 {
                    assert!(
                        (height - rail).abs() < 0.5,
                        "am Gleis (d = {d:.1} m): {height:.2} statt {rail:.2}"
                    );
                    checked_rail += 1;
                } else if d > options.blend * 2.0 && d < options.blend * 3.0 {
                    let ground = source.height_at_utm(e, n).unwrap() + options.geoid_offset;
                    assert!(
                        (height - ground).abs() < 0.5,
                        "abseits (d = {d:.1} m): {height:.2} statt {ground:.2}"
                    );
                    checked_far += 1;
                }
            }
        }
        assert!(checked_rail > 10, "zu wenige Punkte am Gleis geprüft");
        assert!(checked_far > 10, "zu wenige Punkte abseits geprüft");
    }

    #[test]
    fn ohne_dgm_entsteht_ebenes_gelaende() {
        let net = test_net();
        let (tiles, stats) = build(&net, None, &options());
        assert!(!tiles.is_empty());
        assert!(stats.missing > 0, "fehlende Höhen werden gezählt");
        assert_eq!(stats.tile_loads, 0);
    }

    #[test]
    fn kacheln_haben_schuerzen() {
        let net = test_net();
        let mut source = test_source();
        let (tiles, _) = build(&net, Some(&mut source), &options());
        let tile = &tiles[0];
        let n = (tile.step.recip() * 512.0).round() as usize;
        let grid_vertices = (n + 1) * (n + 1);
        assert!(
            tile.positions.len() > grid_vertices,
            "Schürzenpunkte fehlen: {} vs {}",
            tile.positions.len(),
            grid_vertices
        );
        // Der tiefste Punkt liegt unter dem Raster.
        let min_y = tile
            .positions
            .iter()
            .map(|p| p[1])
            .fold(f32::INFINITY, f32::min);
        let grid_min = tile.positions[..grid_vertices]
            .iter()
            .map(|p| p[1])
            .fold(f32::INFINITY, f32::min);
        assert!(min_y < grid_min - 1.0, "{min_y} vs {grid_min}");
    }
}
