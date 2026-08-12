//! Terrain meshes from the DGM (plan ch. 14).
//!
//! Only the corridor around the line is generated, split into tiles and with a
//! distance-dependent resolution. Everything is computed in UTM — that is the system of
//! the DGM, so each support point costs exactly one projection instead of three.
//!
//! The number that matters: one square kilometre of DGM1 has two million triangles.
//! Hence
//!
//! * **tiles** (512 m by default) → frustum culling and view distance limit per tile,
//! * **LOD by track distance** → 4 m at the track, 32 m at the edge of the corridor,
//! * **skirts** at the tile borders → no cracks between different levels,
//! * **cutting/embankment**: close to the track the terrain is pulled up to the rail
//!   height, otherwise the alignment would sit inside the hill.

use crate::import::dgm::TerrainSource;
use glam::DVec2;
use std::collections::HashMap;
use track_model::TrackNetwork;
use world_coords::{EcefPos, EnuFrame, geo};

/// Settings for the terrain generation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerrainOptions {
    /// UTM zone used for the computation (that of the DGM).
    pub zone: u8,
    /// Geoid undulation of the line [m] (NHN → ellipsoidal).
    pub geoid_offset: f64,
    /// Edge length of a terrain tile [m].
    pub tile_size: f64,
    /// How far beside the track terrain is generated [m].
    pub radius: f64,
    /// Finest grid spacing [m] (applies inside the corridor).
    pub base_step: f64,
    /// Up to here the finest level applies [m].
    pub corridor: f64,
    /// Up to here the terrain follows the rail height exactly [m].
    pub flatten: f64,
    /// Up to here rail and terrain height are blended [m].
    pub blend: f64,
    /// Height of the skirt at the tile borders [m].
    pub skirt: f64,
    /// Height where no DGM is available [m] (NHN).
    pub fallback_height: f64,
    /// Sampling distance along the track centreline [m].
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

/// A finished terrain tile — raw mesh data, so that `content` works without Bevy.
#[derive(Debug, Clone)]
pub struct TerrainTile {
    /// Origin of the local ENU frame (tile centre) in world coordinates.
    pub anchor: EcefPos,
    /// Positions in render axes (x = east, y = up, z = −north), relative to the anchor.
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// Grid spacing used [m].
    pub step: f64,
    /// LOD level (0 = finest).
    pub lod: u8,
    /// Bounding radius around the anchor [m] — for view distance and culling.
    pub radius: f32,
}

impl TerrainTile {
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }
}

/// Key figures of a terrain build.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TerrainStats {
    pub tiles: usize,
    pub vertices: usize,
    pub triangles: usize,
    /// Support points without a DGM value (fallback height used).
    pub missing: usize,
    /// How often a DGM tile was read from disk.
    pub tile_loads: usize,
}

impl TerrainStats {
    /// Rough size of the mesh data in memory [bytes].
    pub fn memory(&self) -> usize {
        self.vertices * 12 + self.triangles * 12
    }
}

/// The support points of the track centreline in UTM.
struct Centerline {
    points: Vec<DVec2>,
    /// Ellipsoidal height of the top of rail [m].
    heights: Vec<f64>,
    /// Accelerated neighbourhood index.
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

    /// Nearest centreline point in the neighbourhood: `(distance, height)`.
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

    /// Distance to the centreline without the neighbourhood index (checks only).
    #[cfg(test)]
    fn distance_scan(&self, p: DVec2) -> f64 {
        self.points
            .iter()
            .map(|q| (*q - p).length())
            .fold(f64::INFINITY, f64::min)
    }

    /// Distance from the centreline to the **area** of a tile (0 if the track runs
    /// through it).
    ///
    /// The distance to the tile centre is no good for this: a 512 m tile the track runs
    /// through still has its centre hundreds of metres to the side — it would
    /// otherwise get the coarse level.
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

/// Builds the terrain around all tracks of the network.
///
/// `source` may be `None` — then flat terrain at `fallback_height` is created, which
/// is enough for test scenes and lines without a DGM.
pub fn build(
    net: &TrackNetwork,
    source: Option<&mut TerrainSource>,
    options: &TerrainOptions,
) -> (Vec<TerrainTile>, TerrainStats) {
    let centerline = Centerline::build(net, options);
    if centerline.points.is_empty() {
        return (Vec::new(), TerrainStats::default());
    }

    // Lay a tile grid over the corridor. Sorted order, so that the same line state
    // always yields the same tiles in the same sequence.
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
        // Tiles that do not touch the corridor are dropped entirely.
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

/// Grid spacing and LOD level by distance to the track centreline.
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

/// Builds a single tile.
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

    // Anchor at the tile centre, so that the local f32 coordinates stay small.
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

            // Cutting/embankment: exactly rail height at the track, then blend.
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

    // Regular triangulation.
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

    // Skirt: the border is extended downwards so that no cracks become visible at
    // LOD boundaries.
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

/// Attaches a vertical skirt to the tile border.
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
    // Border points once clockwise.
    let border: Vec<usize> = (0..row) // south edge
        .chain((1..row).map(|iy| iy * row + n)) // east edge
        .chain((0..n).rev().map(|ix| n * row + ix)) // north edge
        .chain((0..n).rev().map(|iy| iy * row)) // west edge
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

/// ENU (x = east, y = north, z = up) → render axes.
fn to_render(p: glam::DVec3) -> [f32; 3] {
    [p.x as f32, p.z as f32, -p.y as f32]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::dgm::{HeightTile, TerrainSource};
    use track_model::{EdgeId, NodeKind, Segment, TrackEdge, TrackNetwork};

    /// Straight 1 km test line at 52° N, 10° E.
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

    /// DGM with a 25 m grid over the test area: a slope rising towards the north.
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
    fn terrain_is_generated_only_in_the_corridor() {
        let net = test_net();
        let mut source = test_source();
        let (tiles, stats) = build(&net, Some(&mut source), &options());

        assert!(stats.tiles > 0);
        assert_eq!(tiles.len(), stats.tiles);
        // All tiles lie within the radius around the line.
        let centerline = Centerline::build(&net, &options());
        for tile in &tiles {
            let (lat, lon, _) = geo::from_ecef(tile.anchor);
            let (e, n) = geo::to_utm(lat, lon, 32);
            let d = centerline.distance_scan(DVec2::new(e, n));
            assert!(
                d <= options().radius + options().tile_size,
                "tile {d:.0} m off the line"
            );
        }
    }

    #[test]
    fn lod_gets_coarser_with_distance() {
        let net = test_net();
        let mut source = test_source();
        let (tiles, _) = build(&net, Some(&mut source), &options());

        let fine = tiles.iter().filter(|t| t.lod == 0).count();
        let coarse = tiles.iter().filter(|t| t.lod > 0).count();
        assert!(fine > 0 && coarse > 0, "fine {fine}, coarse {coarse}");

        // Fine tiles have the base step, coarse ones a multiple of it.
        for tile in &tiles {
            let expected = options().base_step * 2f64.powi(tile.lod as i32);
            assert_eq!(tile.step, expected, "LOD {}", tile.lod);
        }
    }

    #[test]
    fn triangle_count_stays_manageable() {
        let net = test_net();
        let mut source = test_source();
        let (_, stats) = build(&net, Some(&mut source), &options());

        // For comparison: the same corridor at full DGM1 resolution.
        let area = 1000.0 * 2.0 * options().radius; // m²
        let full_detail = area as usize * 2;
        assert!(
            stats.triangles * 5 < full_detail,
            "{} triangles versus {} at a 1 m grid",
            stats.triangles,
            full_detail
        );
        // And still enough for visible terrain.
        assert!(stats.triangles > 10_000);
        assert!(
            stats.memory() < 40 * 1024 * 1024,
            "{} bytes",
            stats.memory()
        );

        // The real lever is the grading: with each LOD level the triangle count of an
        // equally sized tile is quartered.
        let (tiles, _) = build(&net, Some(&mut test_source()), &options());
        let per_lod = |lod: u8| tiles.iter().find(|t| t.lod == lod).map(|t| t.triangles());
        if let (Some(fine), Some(coarse)) = (per_lod(0), per_lod(1)) {
            let ratio = fine as f64 / coarse as f64;
            assert!((3.0..5.0).contains(&ratio), "ratio LOD0:LOD1 = {ratio}");
        }
    }

    #[test]
    fn terrain_follows_the_rail_height_at_the_track() {
        let net = test_net();
        let options = options();
        let centerline = Centerline::build(&net, &options);
        let (tiles, _) = build(&net, Some(&mut test_source()), &options);

        // The test data must differ, otherwise the test checks nothing.
        let p = centerline.points[10];
        let (_, rail) = centerline.nearest(p).unwrap();
        let ground = test_source().height_at_utm(p.x, p.y).unwrap() + options.geoid_offset;
        assert!(
            (ground - rail).abs() > 0.5,
            "terrain and rail height must differ: {ground} vs {rail}"
        );

        // Convert every terrain point back and compare it with the expected value:
        // rail height at the track, DGM height far away.
        let mut source = test_source();
        let mut checked_rail = 0;
        let mut checked_far = 0;
        for tile in &tiles {
            let frame = EnuFrame::at(tile.anchor);
            // Only check the grid points — the skirt hangs below on purpose.
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
                        "at the track (d = {d:.1} m): {height:.2} instead of {rail:.2}"
                    );
                    checked_rail += 1;
                } else if d > options.blend * 2.0 && d < options.blend * 3.0 {
                    let ground = source.height_at_utm(e, n).unwrap() + options.geoid_offset;
                    assert!(
                        (height - ground).abs() < 0.5,
                        "off the track (d = {d:.1} m): {height:.2} instead of {ground:.2}"
                    );
                    checked_far += 1;
                }
            }
        }
        assert!(checked_rail > 10, "too few points checked at the track");
        assert!(checked_far > 10, "too few points checked off the track");
    }

    #[test]
    fn without_a_dgm_the_terrain_is_flat() {
        let net = test_net();
        let (tiles, stats) = build(&net, None, &options());
        assert!(!tiles.is_empty());
        assert!(stats.missing > 0, "missing heights are counted");
        assert_eq!(stats.tile_loads, 0);
    }

    #[test]
    fn tiles_have_skirts() {
        let net = test_net();
        let mut source = test_source();
        let (tiles, _) = build(&net, Some(&mut source), &options());
        let tile = &tiles[0];
        let n = (tile.step.recip() * 512.0).round() as usize;
        let grid_vertices = (n + 1) * (n + 1);
        assert!(
            tile.positions.len() > grid_vertices,
            "skirt points are missing: {} vs {}",
            tile.positions.len(),
            grid_vertices
        );
        // The lowest point lies below the grid.
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
