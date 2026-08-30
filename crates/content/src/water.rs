//! Water as ground to look at: OSM outline in, surface mesh out.
//!
//! A [`crate::route::WaterSource`] is a polygon on a map — the waterline of a
//! lake, a pond, a reservoir, a stretch of river between its banks. What the
//! run needs is a surface that sits where the water sits: flat in a lake
//! basin, falling along a river, hidden where the ground rises above it.
//!
//! Where that is comes from the elevation data. The DGM is sampled around the
//! outline — the shoreline, where the ground meets the water — and a robust
//! low percentile of those samples is the polygon's level. The surface then
//! follows the ground itself, but never sinks below the level:
//!
//! * **The DGM models the water surface** (the usual case for German DGM1):
//!   the surface rides a hand's width above it, and a river follows its fall.
//! * **The DGM models the bed** (deep lakes, some deliveries): the surface is
//!   raised to the shoreline level, so the lake stays visible instead of
//!   drowning under its own ground.
//!
//! Everything is computed as a pure function of position, so the pieces two
//! neighbouring tiles cut out of one polygon meet without a seam, and an
//! embankment carrying the line across a valley holds the water back exactly
//! as a dam does — the shaped ground there is higher, and covers the surface.
//!
//! The surfaces are cut to the terrain tiles and handed out with them, like
//! the fields and the trees: a lake of forty hectares streams as one patch per
//! tile it touches. Nothing here knows what the water looks like; the
//! renderer's shader makes the waves out of the wind and the weather (plan
//! 14.1), which the scenario already knows — so a module carries no water
//! look, and two clients of a multiplayer run agree on it without a byte
//! crossing the network.
//
// ponytail: one level per polygon, from the shoreline alone. A river with a
// gauge, a reservoir with a rule curve, a seasonal flood — all want a second
// number, and none of it is in the OSM extract. When the first line needs it,
// the level moves into the line file next to the polygon, and the editor gets
// a field to correct it by hand.

use crate::route::{LineSource, WaterSource};
use crate::terrain::{Sampler, TerrainOptions, TileKey};
use glam::{DVec2, DVec3, Vec3};
use std::collections::HashMap;
use std::sync::Arc;

use crate::import::dgm::TerrainSource;
use world_coords::{EnuFrame, geo};

/// How far the surface rides above the height it settled at [m]. Enough that
/// the depth buffer never has to choose between water and ground, little
/// enough that the waterline stays inside the outline the map drew.
pub const LIFT: f64 = 0.12;

/// Shore sample below which the polygon's level sits, as a fraction of the
/// samples taken. Not the minimum — a single DGM artefact at one corner would
/// pull a whole lake down — but low enough that a river, whose shoreline
/// climbs from one end to the other, is clamped only at its lowest reach.
const LEVEL_PERCENTILE: f64 = 0.05;

/// The column assumed where the elevation data has no word [m]. A module
/// without a DGM would otherwise draw every body as flat as the ground it
/// lies on — a dull green sheet — where "a few metres of water under it" is
/// the honest guess.
pub(crate) const NOMINAL_DEPTH: f64 = 2.5;

/// Target edge length of a water surface's mesh [m]. The waves are the
/// shader's business; the geometry only has to follow the fall of a river,
/// and the coarsest grid that does that is the cheapest.
const TARGET_EDGE: f64 = 16.0;

/// Most a single patch is subdivided. Three levels turn one triangle into 64 —
/// a tile-wide river at [`TARGET_EDGE`] needs no more.
const MAX_LEVELS: u32 = 3;

/// The bodies of water of a line, indexed by the terrain tiles they touch.
#[derive(Debug, Clone, Default)]
pub struct Waters {
    /// Per tile, the bodies whose bounding box reaches it.
    by_tile: HashMap<TileKey, Vec<usize>>,
    waters: Vec<Water>,
    /// Whether [`Self::prepare`] has run — the shoreline levels are sampled
    /// once, when the builder takes the waters in.
    prepared: bool,
}

/// One body of water, ready to be cut up.
#[derive(Debug, Clone)]
struct Water {
    /// The waterline [m UTM].
    ring: Vec<DVec2>,
    /// Islands — ground the surface goes around [m UTM].
    holes: Vec<Vec<DVec2>>,
    /// Middle of the bounding box — the origin of the wave phases, so one
    /// body's ripples line up across every tile it crosses.
    centre: DVec2,
    /// Ellipsoidal height the surface never sinks below [m]; `None` until
    /// [`Waters::prepare`] has sampled the shoreline.
    level: Option<f64>,
    /// Index in [`LineSource::waters`] — what the editor selects.
    index: u32,
}

impl Waters {
    pub fn from_line(line: &LineSource, zone: u8, tile_size: f64) -> Self {
        Self::from_parts(&line.waters, zone, tile_size)
    }

    pub fn from_parts(sources: &[WaterSource], zone: u8, tile_size: f64) -> Self {
        let mut out = Waters::default();
        for (index, source) in sources.iter().enumerate() {
            let utm = |points: &[crate::route::WaterPoint]| {
                points
                    .iter()
                    .map(|p| {
                        let (e, n) = geo::to_utm(p.lat.to_radians(), p.lon.to_radians(), zone);
                        DVec2::new(e, n)
                    })
                    .collect::<Vec<_>>()
            };
            let ring = utm(&source.polygon);
            if ring.len() < 3 {
                continue;
            }
            // The holes (islands) lie inside the outline, so the outline's
            // bounding box covers them and the tile index needs no second pass.
            let holes: Vec<Vec<DVec2>> = source
                .holes
                .iter()
                .map(|h| utm(h))
                .filter(|h| h.len() >= 3)
                .collect();
            let at = out.waters.len();
            let (lo, hi) = fields::geometry::bounds(&ring);
            let centre = (lo + hi) / 2.0;
            let (kx0, ky0) = key(lo, tile_size);
            let (kx1, ky1) = key(hi, tile_size);
            for ky in ky0..=ky1 {
                for kx in kx0..=kx1 {
                    out.by_tile.entry((kx, ky)).or_default().push(at);
                }
            }
            out.waters.push(Water {
                ring,
                holes,
                centre,
                level: None,
                index: index as u32,
            });
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.waters.is_empty()
    }

    pub fn len(&self) -> usize {
        self.waters.len()
    }

    /// A cheap fingerprint of the line's water list — the editor compares it
    /// to decide whether the levels it has already sampled still belong to
    /// the polygons on the line. Counts say how many, the mix says where:
    /// a corner dragged by hand changes the mix, and the levels are sampled
    /// again.
    pub fn fingerprint(&self) -> (u64, usize, usize) {
        let mut mix: u64 = 0x517C_C1D5;
        let stir = |mix: &mut u64, p: &DVec2| {
            *mix = (*mix ^ p.x.to_bits()).wrapping_mul(0x9E37_79B9_7F4A_7C15);
            *mix = (*mix ^ p.y.to_bits()).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        };
        for water in &self.waters {
            for p in &water.ring {
                stir(&mut mix, p);
            }
            for hole in &water.holes {
                for p in hole {
                    stir(&mut mix, p);
                }
            }
        }
        (
            mix,
            self.waters.len(),
            self.waters.iter().map(|w| w.ring.len()).sum(),
        )
    }

    /// Samples the shoreline of every body against the elevation data and
    /// fixes the level it never sinks below. Runs once — the builder calls it
    /// when it takes the waters in, and an already prepared set is left alone.
    pub(crate) fn prepare(
        &mut self,
        sources: &[Arc<TerrainSource>],
        zone: u8,
        geoid_offset: f64,
        fallback_height: f64,
    ) {
        if self.prepared {
            return;
        }
        self.prepared = true;
        let fallback = fallback_height + geoid_offset;
        // One sampler for the whole pass; the hot-sheet cache makes the
        // samples of one shoreline, and of neighbouring ones, cheap.
        let mut sampler = Sampler::new(sources.iter().map(Arc::as_ref), zone);
        for water in &mut self.waters {
            let mut heights: Vec<f64> = Vec::with_capacity(water.ring.len());
            for p in &water.ring {
                let (lat, lon) = geo::from_utm(p.x, p.y, zone);
                if let Some(h) = sampler.height(*p, lat, lon) {
                    heights.push(h);
                }
            }
            water.level = Some(if heights.is_empty() {
                // No elevation data at the waterline: the ground everywhere is
                // the fallback, and the water settles on it.
                fallback
            } else {
                heights.sort_by(f64::total_cmp);
                let below = ((heights.len() as f64 - 1.0) * LEVEL_PERCENTILE) as usize;
                heights[below] + geoid_offset
            });
        }
    }

    /// Whether any body reaches this tile — the cheap question the tile
    /// builder asks before doing any of the work below.
    pub(crate) fn touches(&self, k: TileKey) -> bool {
        self.by_tile.contains_key(&k)
    }
}

/// One body's surface on one tile, in the tile's own frame.
#[derive(Debug, Clone, PartialEq)]
pub struct WaterPatch {
    /// Render axes (x = east, y = up, z = −north), relative to the tile anchor.
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// `u` east and `v` north, in metres from the body's own centre — the
    /// wave phases run in them, so the ripples of one body line up across
    /// the tile boundaries it crosses.
    pub uvs: Vec<[f32; 2]>,
    /// Per-vertex data: `r` carries the depth of the water column under the
    /// vertex [m] — what the shader makes its shore-to-deep colours of.
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
    /// The bodies that went into this patch, in line order — what a click on
    /// it selects, and what the editor highlights.
    pub sources: Vec<u32>,
}

impl WaterPatch {
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }
}

/// The surfaces of one tile.
#[allow(clippy::too_many_arguments)]
pub(crate) fn patches(
    k: TileKey,
    sampler: &mut Sampler<'_>,
    frame: &EnuFrame,
    options: &TerrainOptions,
    tile_size: f64,
    waters: &Waters,
) -> Vec<WaterPatch> {
    let Some(indices) = waters.by_tile.get(&k) else {
        return Vec::new();
    };
    let zone = options.zone;
    let geoid = options.geoid_offset;
    let fallback = options.fallback_height + geoid;
    let min = DVec2::new(k.0 as f64 * tile_size, k.1 as f64 * tile_size);
    // The tile itself: a body is cut to it exactly, and the neighbouring tile
    // cuts the other half the same way, so the two meet without a seam.
    let rect = vec![
        min,
        DVec2::new(min.x + tile_size, min.y),
        min + DVec2::splat(tile_size),
        DVec2::new(min.x, min.y + tile_size),
    ];

    let mut patch = WaterPatch {
        positions: Vec::new(),
        normals: Vec::new(),
        uvs: Vec::new(),
        colors: Vec::new(),
        indices: Vec::new(),
        sources: Vec::new(),
    };
    for &at in indices {
        let water = &waters.waters[at];
        let Some(level) = water.level else {
            continue;
        };
        // Surface and column depth as pure functions of position: both tiles
        // at a seam evaluate them to the same numbers.
        let surface = |p: DVec2, sampler: &mut Sampler<'_>| -> (f64, f64) {
            let (lat, lon) = geo::from_utm(p.x, p.y, zone);
            let bottom = sampler
                .height(p, lat, lon)
                .map(|h| h + geoid)
                .unwrap_or(fallback - NOMINAL_DEPTH);
            (bottom.max(level) + LIFT, bottom)
        };
        // The outline and the islands are each cut to the tile, and an outer
        // piece takes the island pieces it holds as its holes.
        let hole_pieces: Vec<Vec<DVec2>> = water
            .holes
            .iter()
            .flat_map(|hole| fields::geometry::clip(hole, &rect, fields::geometry::Op::Intersect))
            .collect();
        for piece in fields::geometry::clip(&water.ring, &rect, fields::geometry::Op::Intersect) {
            let holes: Vec<Vec<DVec2>> = hole_pieces
                .iter()
                .filter(|hole| {
                    hole.first()
                        .is_some_and(|p| crate::terrain::point_in_polygon(*p, &piece))
                })
                .cloned()
                .collect();
            add_piece(
                &mut patch, &piece, &holes, water, level, zone, surface, sampler, frame,
            );
        }
    }

    if patch.indices.is_empty() {
        Vec::new()
    } else {
        vec![patch]
    }
}

/// Triangulates one piece of a body — its islands with it — refines the mesh
/// to [`TARGET_EDGE`] and lays it on the water.
#[allow(clippy::too_many_arguments)]
fn add_piece(
    patch: &mut WaterPatch,
    ring: &[DVec2],
    holes: &[Vec<DVec2>],
    water: &Water,
    level: f64,
    zone: u8,
    surface: impl Fn(DVec2, &mut Sampler<'_>) -> (f64, f64),
    sampler: &mut Sampler<'_>,
    frame: &EnuFrame,
) {
    let (mut points, tris) = triangulate_with_holes(ring, holes);
    if tris.is_empty() {
        return;
    }
    let mut tris = tris;
    refine(&mut points, &mut tris, levels(ring));

    let centre = water.centre;
    let base = patch.positions.len() as u32;
    if !patch.sources.contains(&water.index) {
        patch.sources.push(water.index);
    }

    for p in &points {
        let (height, bottom) = surface(*p, sampler);
        let depth = (level - bottom).max(0.0);
        let (lat, lon) = geo::from_utm(p.x, p.y, zone);
        let world = geo::to_ecef(lat, lon, height);
        patch.positions.push(to_render(frame.to_local(world)));
        patch.normals.push(normal_at(*p, &surface, sampler));
        let offset = *p - centre;
        patch.uvs.push([offset.x as f32, offset.y as f32]);
        // r: the water column in metres. g, b, a spare.
        patch.colors.push([depth as f32, 0.0, 0.0, 1.0]);
    }
    for [a, b, c] in tris {
        patch
            .indices
            .extend_from_slice(&[base + a, base + b, base + c]);
    }
}

/// Triangulates a ring with holes: each island is **bridged** into the
/// outline — the rightmost island point is joined to the waterline with a
/// keyhole the eye cannot see — and the one ring that leaves is ear-clipped
/// as usual.
///
/// A hole the bridging cannot place (a degenerate ring, a ray that leaves
/// without crossing) is dropped rather than fatal: the water then covers the
/// island, which the terrain under it was going to show anyway.
fn triangulate_with_holes(outer: &[DVec2], holes: &[Vec<DVec2>]) -> (Vec<DVec2>, Vec<[u32; 3]>) {
    let mut points: Vec<DVec2> = outer.to_vec();
    // The ring as indices into `points`, so a bridge can split an edge
    // without the ear clipping ever knowing it happened.
    let mut ring: Vec<u32> = (0..outer.len() as u32).collect();
    if signed_area(outer) < 0.0 {
        ring.reverse();
    }

    // Rightmost islands first: a bridge leaves to the right, so the islands
    // it could cross are gone before it is laid.
    let mut order: Vec<&Vec<DVec2>> = holes.iter().collect();
    order.sort_by(|a, b| {
        let right = |h: &Vec<DVec2>| h.iter().fold(f64::MIN, |m, p| m.max(p.x));
        right(b).total_cmp(&right(a))
    });
    for hole in order {
        if hole.len() < 3 {
            continue;
        }
        // Islands are walked clockwise, so the water keeps to the left of
        // the boundary all the way round.
        let mut hole = hole.clone();
        if signed_area(&hole) > 0.0 {
            hole.reverse();
        }
        // The island's rightmost point — the shortest bridge the geometry
        // guarantees.
        let Some(m) = hole
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.x.total_cmp(&b.1.x).then(b.1.y.total_cmp(&a.1.y)))
            .map(|(i, _)| i)
        else {
            continue;
        };
        let origin = hole[m];

        // Closest crossing of the ray due east with the ring so far.
        let mut best: Option<(f64, usize)> = None;
        for e in 0..ring.len() {
            let a = points[ring[e] as usize];
            let b = points[ring[(e + 1) % ring.len()] as usize];
            let (ay, by) = (a.y - origin.y, b.y - origin.y);
            if (ay > 0.0) == (by > 0.0) || ay == by {
                continue;
            }
            let t = ay / (ay - by);
            let d = a.x + (b.x - a.x) * t - origin.x;
            if d > 1e-9 && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, e));
            }
        }
        let Some((_, e)) = best else {
            continue;
        };
        // The crossing is made a corner of its own, and the ring is spliced:
        // up to the crossing, out to the island, round it, back through the
        // crossing, and on along the far side of the split edge.
        let a = points[ring[e] as usize];
        let b = points[ring[(e + 1) % ring.len()] as usize];
        let (ay, by) = (a.y - origin.y, b.y - origin.y);
        let t = ay / (ay - by);
        let corner = points.len() as u32;
        points.push(a + (b - a) * t);
        ring.insert(e + 1, corner);
        let mut merged = Vec::with_capacity(ring.len() + hole.len() + 1);
        merged.extend_from_slice(&ring[..=e + 1]);
        for k in 0..hole.len() {
            let index = points.len() as u32;
            points.push(hole[(m + k) % hole.len()]);
            merged.push(index);
        }
        // Back through the crossing — pushed once more, and the far part of
        // the split edge continues after it.
        merged.push(corner);
        merged.extend_from_slice(&ring[e + 2..]);
        ring = merged;
    }

    // The ear clipping reads values, so it is handed the ring's own points.
    // A ring with bridges is weakly simple — every crossing appears twice —
    // and the shared ear clipper counts a point *on* an ear's edge as inside
    // it, so a bridge's twin would veto its own ear and no triangle would
    // ever fall. The local clipper lets coincident points pass.
    let tris = ear_clip(
        &(0..ring.len())
            .map(|i| points[ring[i] as usize])
            .collect::<Vec<_>>(),
    )
    .into_iter()
    .map(|t| {
        [
            ring[t[0] as usize],
            ring[t[1] as usize],
            ring[t[2] as usize],
        ]
    })
    .collect();
    (points, tris)
}

/// Ear clipping for a ring that may touch itself — the waterline with its
/// bridges. The same walk `fields::geometry::triangulate` does, with one
/// difference: a vertex that coincides with one of the ear's own corners
/// (its twin across a bridge, always) does not count as swallowed.
fn ear_clip(ring: &[DVec2]) -> Vec<[u32; 3]> {
    let n = ring.len();
    if n < 3 {
        return Vec::new();
    }
    let same = |p: DVec2, q: DVec2| (p - q).length_squared() < 1e-12;
    let ccw = signed_area(ring) > 0.0;
    let mut remaining: Vec<usize> = if ccw {
        (0..n).collect()
    } else {
        (0..n).rev().collect()
    };
    let mut out = Vec::with_capacity(n.saturating_sub(2));
    let mut guard = 0;
    while remaining.len() > 3 {
        guard += 1;
        if guard > n * n + 16 {
            // A ring that will not clip — the bridges have crossed. What is
            // triangulated so far is kept, and the lake shows the gap as
            // water with a seam, not as no lake at all.
            break;
        }
        let count = remaining.len();
        let mut clipped = false;
        for k in 0..count {
            let (i, j, l) = (
                remaining[(k + count - 1) % count],
                remaining[k],
                remaining[(k + 1) % count],
            );
            let (a, b, c) = (ring[i], ring[j], ring[l]);
            if (b - a).perp_dot(c - a) <= 0.0 {
                continue;
            }
            let swallowed = remaining.iter().any(|&m| {
                let p = ring[m];
                m != i
                    && m != j
                    && m != l
                    && !same(p, a)
                    && !same(p, b)
                    && !same(p, c)
                    && in_triangle(p, a, b, c)
            });
            if swallowed {
                continue;
            }
            out.push([i as u32, j as u32, l as u32]);
            remaining.remove(k);
            clipped = true;
            break;
        }
        if !clipped {
            break;
        }
    }
    if remaining.len() == 3 {
        out.push([
            remaining[0] as u32,
            remaining[1] as u32,
            remaining[2] as u32,
        ]);
    }
    out
}

/// Whether `p` lies in the triangle `a b c`, boundary included — the same
/// test the shared clipper makes.
fn in_triangle(p: DVec2, a: DVec2, b: DVec2, c: DVec2) -> bool {
    let d1 = (b - a).perp_dot(p - a);
    let d2 = (c - b).perp_dot(p - b);
    let d3 = (a - c).perp_dot(p - c);
    (d1 >= 0.0 && d2 >= 0.0 && d3 >= 0.0) || (d1 <= 0.0 && d2 <= 0.0 && d3 <= 0.0)
}

/// Signed area of a ring, positive when it runs counter-clockwise.
fn signed_area(ring: &[DVec2]) -> f64 {
    let mut total = 0.0;
    let mut j = ring.len().saturating_sub(1);
    for i in 0..ring.len() {
        total += (ring[j].x - ring[i].x) * (ring[j].y + ring[i].y);
        j = i;
    }
    total / 2.0
}

/// The surface's normal under a point, from the surface's own gradient — the
/// same finite difference on both sides of a tile seam, so the shading does
/// not crease where the mesh is cut.
fn normal_at(
    p: DVec2,
    surface: &impl Fn(DVec2, &mut Sampler<'_>) -> (f64, f64),
    sampler: &mut Sampler<'_>,
) -> [f32; 3] {
    const D: f64 = 2.0;
    let at = |q: DVec2, sampler: &mut Sampler<'_>| surface(q, sampler).0;
    let dx = at(p + DVec2::new(D, 0.0), sampler) - at(p - DVec2::new(D, 0.0), sampler);
    let dy = at(p + DVec2::new(0.0, D), sampler) - at(p - DVec2::new(0.0, D), sampler);
    // Render axes: +x east, +y up, +z south — so north is −z.
    let n = Vec3::new(-(dx / (2.0 * D)) as f32, 1.0, (dy / (2.0 * D)) as f32).normalize_or_zero();
    let n = if n == Vec3::ZERO { Vec3::Y } else { n };
    [n.x, n.y, n.z]
}

/// How many times a piece is subdivided, from how big it is.
fn levels(ring: &[DVec2]) -> u32 {
    let (lo, hi) = fields::geometry::bounds(ring);
    let size = (hi - lo).max_element();
    let mut levels = 0;
    let mut edge = size;
    while edge > TARGET_EDGE && levels < MAX_LEVELS {
        edge /= 2.0;
        levels += 1;
    }
    levels
}

/// Splits every triangle into four, `levels` times over. Uniform rather than
/// by edge length: a mesh subdivided the same everywhere cannot crack, and
/// water is smooth enough that the wasted vertices on its short axis are a
/// few hundred bytes. The same subdivision the farmland drapes its fields
/// with, so the two stay in step.
fn refine(points: &mut Vec<DVec2>, tris: &mut Vec<[u32; 3]>, levels: u32) {
    for _ in 0..levels {
        let mut midpoints: HashMap<(u32, u32), u32> = HashMap::new();
        let mut split = Vec::with_capacity(tris.len() * 4);
        for &[a, b, c] in tris.iter() {
            let mut mid = |i: u32, j: u32, points: &mut Vec<DVec2>| -> u32 {
                let key = if i < j { (i, j) } else { (j, i) };
                *midpoints.entry(key).or_insert_with(|| {
                    let at = points.len() as u32;
                    points.push((points[i as usize] + points[j as usize]) / 2.0);
                    at
                })
            };
            let ab = mid(a, b, points);
            let bc = mid(b, c, points);
            let ca = mid(c, a, points);
            split.extend_from_slice(&[[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]]);
        }
        *tris = split;
    }
}

/// ENU (east, north, up) to render axes (east, up, −north).
fn to_render(v: DVec3) -> [f32; 3] {
    [v.x as f32, v.z as f32, -v.y as f32]
}

/// The tile a UTM point falls in.
fn key(p: DVec2, tile_size: f64) -> TileKey {
    (
        (p.x / tile_size).floor() as i64,
        (p.y / tile_size).floor() as i64,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::WaterPoint;

    /// A square body of `size` metres, with its south-west corner at the given
    /// UTM point in zone 32.
    fn source(e: f64, n: f64, size: f64, name: &str) -> WaterSource {
        let corner = |dx: f64, dy: f64| {
            let (lat, lon) = geo::from_utm(e + dx, n + dy, 32);
            WaterPoint {
                lat: lat.to_degrees(),
                lon: lon.to_degrees(),
            }
        };
        WaterSource {
            name: name.into(),
            polygon: vec![
                corner(0.0, 0.0),
                corner(size, 0.0),
                corner(size, size),
                corner(0.0, size),
            ],
            holes: Vec::new(),
            tags: Vec::new(),
        }
    }

    /// An elevation source over the given UTM square, in the
    /// `easting northing height` format the DGM reader takes. `height` decides
    /// the ground at each sample.
    fn sheet(
        e: f64,
        n: f64,
        size: f64,
        cell: f64,
        height: impl Fn(f64, f64) -> f64,
    ) -> TerrainSource {
        let mut text = String::new();
        let steps = (size / cell) as usize;
        for iy in 0..=steps {
            for ix in 0..=steps {
                let (x, y) = (e + ix as f64 * cell, n + iy as f64 * cell);
                text.push_str(&format!("{x:.1} {y:.1} {:.1}\n", height(x, y)));
            }
        }
        TerrainSource::from_tile(
            crate::import::dgm::HeightTile::parse_xyz(&text, 32).expect("parses"),
        )
    }

    /// The square the sheets cover.
    const SHEET_AT: (f64, f64) = (599_000.0, 5_759_000.0);

    /// Prepares the waters of `sources` against one sheet shaped by `height`.
    fn prepared_with(sources: &[WaterSource], height: impl Fn(f64, f64) -> f64) -> Waters {
        let sheet = Arc::new(sheet(SHEET_AT.0, SHEET_AT.1, 3_000.0, 2.0, height));
        let mut waters = Waters::from_parts(sources, 32, 512.0);
        waters.prepare(std::slice::from_ref(&sheet), 32, 46.0, 100.0);
        waters
    }

    /// Builds the patch of one tile over the sheet `height` shapes.
    fn patch_of(
        sources: &[WaterSource],
        tile: TileKey,
        height: impl Fn(f64, f64) -> f64,
    ) -> Vec<WaterPatch> {
        let tile_size = 512.0;
        let min = DVec2::new(tile.0 as f64 * tile_size, tile.1 as f64 * tile_size);
        let centre = min + DVec2::splat(tile_size / 2.0);
        let (clat, clon) = geo::from_utm(centre.x, centre.y, 32);
        let frame = EnuFrame::at(geo::to_ecef(clat, clon, 0.0));
        let options = TerrainOptions::default();
        let sheet = sheet(SHEET_AT.0, SHEET_AT.1, 3_000.0, 2.0, &height);
        let mut sampler = Sampler::new(std::iter::once(&sheet), 32);
        let waters = prepared_with(sources, height);
        patches(tile, &mut sampler, &frame, &options, tile_size, &waters)
    }

    #[test]
    fn a_body_lands_on_the_tiles_it_covers() {
        let waters =
            Waters::from_parts(&[source(599_900.0, 5_759_900.0, 1_500.0, "See")], 32, 512.0);
        assert_eq!(waters.len(), 1);
        assert!(waters.touches((1171, 11249)), "{:?}", waters.by_tile.keys());
        assert!(!waters.touches((0, 0)));
    }

    #[test]
    fn a_polygon_with_two_corners_is_no_water() {
        let mut bad = source(599_900.0, 5_759_900.0, 100.0, "See");
        bad.polygon.truncate(2);
        assert!(Waters::from_parts(&[bad], 32, 512.0).is_empty());
    }

    /// A lake with an island in the middle: the surface leaves the island
    /// out — no triangle covers it — and the mesh area comes out as the
    /// lake's less the island's.
    #[test]
    fn an_island_is_a_hole_in_the_surface() {
        let tile = (1171, 11249);
        let min = DVec2::new(tile.0 as f64 * 512.0, tile.1 as f64 * 512.0);
        let mut body = source(min.x + 100.0, min.y + 100.0, 400.0, "See");
        // The island: 100 m square in the lake's middle.
        let utm = |dx: f64, dy: f64| {
            let (lat, lon) = geo::from_utm(min.x + 250.0 + dx, min.y + 250.0 + dy, 32);
            crate::route::WaterPoint {
                lat: lat.to_degrees(),
                lon: lon.to_degrees(),
            }
        };
        body.holes = vec![vec![
            utm(-50.0, -50.0),
            utm(50.0, -50.0),
            utm(50.0, 50.0),
            utm(-50.0, 50.0),
        ]];
        let patches = patch_of(&[body], tile, |_, _| 100.0);
        assert_eq!(patches.len(), 1);
        let patch = &patches[0];

        // The island's centre sits at (−50, −50) from the body's centre —
        // the UV space the mesh's own triangles live in, since the UVs are
        // an affine image of the positions. No triangle may cover it.
        let island_centre = DVec2::new(-50.0, -50.0);
        let covered = patch.indices.as_chunks::<3>().0.iter().any(|t| {
            let tri: Vec<DVec2> = t
                .iter()
                .map(|&i| {
                    DVec2::new(
                        patch.uvs[i as usize][0] as f64,
                        patch.uvs[i as usize][1] as f64,
                    )
                })
                .collect();
            crate::terrain::point_in_polygon(island_centre, &tri)
        });
        assert!(!covered, "a triangle covers the island");

        // And the mesh area is the lake less the island, within the
        // refinement's slack.
        let area: f64 = patch
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| {
                let p = |i: u32| {
                    let v = patch.positions[i as usize];
                    DVec3::new(v[0] as f64, v[1] as f64, v[2] as f64)
                };
                (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])).length() / 2.0
            })
            .sum();
        let expected = 400.0 * 400.0 - 100.0 * 100.0;
        assert!((area - expected).abs() < 400.0, "{area} vs {expected}");
    }

    /// Flat ground at the shoreline: the surface rides `LIFT` above it, and
    /// the column has no depth to speak of.
    #[test]
    fn a_lake_settles_on_flat_ground() {
        let tile = (1171, 11249);
        let min = DVec2::new(tile.0 as f64 * 512.0, tile.1 as f64 * 512.0);
        let patches = patch_of(
            &[source(min.x + 100.0, min.y + 100.0, 200.0, "See")],
            tile,
            |_, _| 100.0,
        );
        assert_eq!(patches.len(), 1);
        let patch = &patches[0];
        assert_eq!(patch.sources, vec![0]);
        assert!(patch.triangles() > 0);
        assert_eq!(patch.positions.len(), patch.normals.len());
        assert_eq!(patch.positions.len(), patch.uvs.len());
        assert_eq!(patch.positions.len(), patch.colors.len());
        // Flat water: every normal points up.
        for n in &patch.normals {
            assert!((n[1] - 1.0).abs() < 1e-4, "{n:?}");
        }
        // The surface sits `LIFT` above the ground, ellipsoidal: 100 NHN
        // + 46 geoid + lift.
        let y = patch.positions[0][1];
        assert!((y - (146.0 + LIFT as f32)).abs() < 0.01, "{y}");
        // Flat ground at the level: no column to colour.
        assert!(
            patch.colors.iter().all(|c| c[0] < 1e-4),
            "{:?}",
            patch.colors[0]
        );
        // Every index addresses a vertex that exists.
        let count = patch.positions.len() as u32;
        assert!(patch.indices.iter().all(|i| *i < count));
    }

    /// A DGM that models the bed: the surface is raised to the shoreline, so
    /// the lake stays visible, and the column carries the depth the shader
    /// makes its colours of.
    #[test]
    fn a_deep_bed_is_raised_to_the_shoreline() {
        let tile = (1171, 11249);
        let min = DVec2::new(tile.0 as f64 * 512.0, tile.1 as f64 * 512.0);
        // Ground at 100 m NHN around the waterline; 4 m of bed below it
        // inside. The bed begins 20 m inside the waterline, so the ring
        // itself samples the shore, not the bed.
        let body = source(min.x + 100.0, min.y + 100.0, 200.0, "See");
        let (x0, y0) = (min.x + 120.0, min.y + 120.0);
        let bed = |e: f64, n: f64| {
            if (x0..x0 + 160.0).contains(&e) && (y0..y0 + 160.0).contains(&n) {
                96.0
            } else {
                100.0
            }
        };
        let patches = patch_of(&[body], tile, bed);
        let patch = &patches[0];
        // The level comes out of the shoreline at 100 m, so the surface
        // stands there, a lift above it, even where the bed lies 4 m lower.
        let y = patch.positions[0][1];
        assert!((y - (146.0 + LIFT as f32)).abs() < 0.01, "{y}");
        // And the column reads the water between shoreline level and bed.
        let deepest = patch.colors.iter().fold(0.0f32, |m, c| m.max(c[0]));
        assert!((deepest - 4.0).abs() < 0.05, "{deepest}");
    }

    #[test]
    fn a_body_is_cut_at_the_tile_boundary() {
        let tile = (1171, 11249);
        let min = DVec2::new(tile.0 as f64 * 512.0, tile.1 as f64 * 512.0);
        // Straddling the eastern seam, 60 m of it in this tile and 140 in the
        // next — off centre, so a bug that halved it would still show.
        let body = source(min.x + 452.0, min.y + 100.0, 200.0, "See");
        let here = patch_of(std::slice::from_ref(&body), tile, |_, _| 100.0);
        let next = patch_of(&[body], (tile.0 + 1, tile.1), |_, _| 100.0);
        assert_eq!(here.len(), 1);
        assert_eq!(next.len(), 1);
        let area = |patch: &WaterPatch| {
            patch
                .indices
                .as_chunks::<3>()
                .0
                .iter()
                .map(|t| {
                    let p = |i: u32| {
                        let v = patch.positions[i as usize];
                        DVec3::new(v[0] as f64, v[1] as f64, v[2] as f64)
                    };
                    (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])).length() / 2.0
                })
                .sum::<f64>()
        };
        // Nothing is lost and nothing is drawn twice: the pieces add up to
        // the body. (Within a tenth of a per cent — UTM's scale factor is
        // not 1.)
        let total = area(&here[0]) + area(&next[0]);
        assert!((total - 40_000.0).abs() < 40.0, "{total}");
        assert!(
            (area(&here[0]) - 12_000.0).abs() < 60.0,
            "{}",
            area(&here[0])
        );
    }

    /// The wave phases run from the body's own centre, not the tile's, so the
    /// two halves of a cut body share one coordinate system.
    #[test]
    fn the_waves_line_up_across_a_tile_boundary() {
        let tile = (1171, 11249);
        let min = DVec2::new(tile.0 as f64 * 512.0, tile.1 as f64 * 512.0);
        let body = source(min.x + 452.0, min.y + 100.0, 200.0, "See");
        let here = patch_of(std::slice::from_ref(&body), tile, |_, _| 100.0);
        let next = patch_of(&[body], (tile.0 + 1, tile.1), |_, _| 100.0);
        // The body's centre sits 40 m east of the seam, so both pieces end
        // at u = −40 — the seam measured from the body, not from the tile.
        let seam_u = -40.0;
        let u_range = |patch: &WaterPatch| {
            patch.uvs.iter().fold((f32::MAX, f32::MIN), |(lo, hi), uv| {
                (lo.min(uv[0]), hi.max(uv[0]))
            })
        };
        let (lo_here, hi_here) = u_range(&here[0]);
        let (lo_next, hi_next) = u_range(&next[0]);
        assert!((hi_here - seam_u).abs() < 2.0, "{hi_here}");
        assert!((lo_next - seam_u).abs() < 2.0, "{lo_next}");
        // Between them they cover the whole body, once.
        assert!((lo_here - -100.0).abs() < 2.0, "{lo_here}");
        assert!((hi_next - 100.0).abs() < 2.0, "{hi_next}");
    }

    /// The whole chain: a line with a body of water, taken through the
    /// terrain builder — the tile that covers the water carries its surface.
    #[test]
    fn the_builder_hands_the_surface_out_with_the_tile() {
        use crate::terrain::{TerrainBuilder, TerrainStats, Vegetation};
        use track_model::{EdgeId, NodeKind, Segment, TrackEdge, TrackNetwork};

        let tile_size = 512.0;
        // A body of water 200–500 m south of the line, on the flat sheet.
        let (e0, n0) = geo::to_utm(52.0f64.to_radians(), 10.0f64.to_radians(), 32);
        let corner = |dx: f64, dy: f64| {
            let (lat, lon) = geo::from_utm(e0 + dx, n0 + dy, 32);
            WaterPoint {
                lat: lat.to_degrees(),
                lon: lon.to_degrees(),
            }
        };
        let body = WaterSource {
            name: "See".into(),
            polygon: vec![
                corner(200.0, -500.0),
                corner(600.0, -500.0),
                corner(600.0, -200.0),
                corner(200.0, -200.0),
            ],
            holes: Vec::new(),
            tags: Vec::new(),
        };

        let mut net = TrackNetwork::new();
        let a = net.add_node(NodeKind::Buffer);
        let b = net.add_node(NodeKind::Buffer);
        net.add_edge(TrackEdge::new(
            EdgeId(0),
            a,
            b,
            geo::to_ecef_deg(52.0, 10.0, 146.0),
            0.0,
            vec![Segment::straight(1000.0)],
        ));

        let sheet = sheet(e0 - 2_000.0, n0 - 2_000.0, 4_000.0, 2.0, |_, _| 100.0);
        let options = TerrainOptions {
            tile_size,
            ..Default::default()
        };
        let builder = TerrainBuilder::new(&net, vec![sheet], options)
            .with_vegetation(Vegetation::default())
            .with_waters(Waters::from_parts(&[body], 32, tile_size));

        // The tile over the water, from the builder's own corridor keys.
        let key = key(DVec2::new(e0 + 400.0, n0 - 350.0), tile_size);
        let mut stats = TerrainStats::default();
        let tile = builder.build_key(key, &mut stats).expect("in the corridor");
        assert_eq!(tile.waters.len(), 1, "the tile carries the surface");
        let patch = &tile.waters[0];
        assert!(patch.triangles() > 0);
        assert_eq!(patch.sources, vec![0]);
        // A tile off the water carries none.
        let dry = builder
            .build_key((key.0, key.1 + 3), &mut stats)
            .expect("in the corridor");
        assert!(dry.waters.is_empty());
    }
}
