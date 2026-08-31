//! Fields as ground to look at: outline in, draped mesh out.
//!
//! A [`crate::route::FieldSource`] is a polygon on a map. What the run needs is
//! a surface that lies on the terrain, follows every hollow in it, and carries
//! the two things that make a field read as a field — the crop's colour on the
//! day, and the direction it was worked in (plan ch. 7).
//!
//! The surfaces are cut to the terrain tiles and handed out with them, like the
//! trees and the scenery: a field of forty hectares crosses four tiles and is
//! streamed as four patches, so nothing is drawn that the camera is nowhere
//! near. Within a tile the patches are grouped by crop, so a tile costs one
//! draw per crop on it rather than one per field.
//!
//! Nothing here knows the date. The patch carries the crop and the field's
//! seed; `fields::phenology` turns those and the scenario clock into the colour
//! and the growth stage at draw time, so the same tile shows April and July
//! without being built twice — and so two clients of a multiplayer run agree on
//! what a field looks like without a byte crossing the network.
//
// The surface lies on the ground, and the crop's height rides in its colour
// and row contrast — but the standing crop is geometry now: world_render::plants
// grows the plant cards out of this very mesh (its triangles are the ground the
// cards stand on, its vertex colours the field's tint and its week of the crop
// year). This is still the ground it stands on, and the paint the cards stand
// over.

use crate::route::{FieldSource, LineSource};
use crate::terrain::{HeightGrid, TileKey};
use fields::CropClass;
use glam::{DVec2, DVec3, Vec3};
use std::collections::HashMap;
use world_coords::{EnuFrame, geo};

/// How far a field's surface is lifted off the terrain [m]. Enough that the
/// depth buffer never has to choose between the two, little enough that the
/// edge of a field does not stand proud of the grass beside it.
pub const LIFT: f64 = 0.05;

/// Least a piece of a field may be and still be a field [m²].
///
/// Taking one parcel out of the one beside it leaves hairlines where the two
/// share a boundary — they were digitised from either side and agree to a few
/// centimetres, not exactly. Those slivers are numerical residue, not fields.
const MIN_PIECE: f64 = 25.0;

/// How much two parcels have to share before one is taken out of the other
/// [m²]. Adjacent parcels *touch*, which is not overlapping: of the 135
/// fields on the example line, 49 pairs share a boundary and only 5 share any
/// ground at all. Cutting on a touch would be work for nothing, and worse —
/// the clip is least sure of itself where two rings run along each other.
const MIN_SHARED: f64 = 1.0;

/// The fields of a line, indexed by the terrain tiles they touch.
#[derive(Debug, Clone, Default)]
pub struct Fields {
    /// Per tile, the fields whose bounding box reaches it.
    by_tile: HashMap<TileKey, Vec<usize>>,
    fields: Vec<Field>,
    /// How many of the line's fields had ground taken off them because an
    /// earlier one already had it, and how much [m²]. Register parcels
    /// overlap more often than they should; a large number here means the
    /// import is worth a look rather than the renderer.
    overlaps: usize,
    overlap_area: f64,
}

/// One field, ready to be cut up.
#[derive(Debug, Clone)]
struct Field {
    /// The outline [m UTM].
    ring: Vec<DVec2>,
    crop: CropClass,
    /// Working direction against grid east [rad].
    direction: f64,
    seed: u64,
    /// Index in [`LineSource::fields`] — what the editor selects.
    index: u32,
}

impl Fields {
    pub fn from_line(line: &LineSource, zone: u8, tile_size: f64) -> Self {
        Self::from_parts(&line.fields, zone, tile_size)
    }

    pub fn from_parts(sources: &[FieldSource], zone: u8, tile_size: f64) -> Self {
        let mut out = Fields::default();
        for (index, source) in sources.iter().enumerate() {
            // A crop id no installed table knows is drawn as bare ground rather
            // than dropped — the field is there, and the rule check says which
            // id to correct.
            let crop = CropClass::from_id(&source.crop).unwrap_or(CropClass::Other);
            let ring: Vec<DVec2> = source
                .polygon
                .iter()
                .map(|p| {
                    let (e, n) = geo::to_utm(p.lat.to_radians(), p.lon.to_radians(), zone);
                    DVec2::new(e, n)
                })
                .collect();
            if ring.len() < 3 {
                continue;
            }
            // Two parcels may not stand on the same ground. Where they do —
            // and in a register they do, in slivers along boundaries that
            // were digitised twice — the one that came first keeps it, and
            // what is left of the later one is what gets drawn. Otherwise the
            // two surfaces sit at the same height and flicker against each
            // other, and the crop grows twice over the strip they share.
            //
            // Whoever came first: the order in the line file, so the answer
            // is the same on every machine of a multiplayer run and does not
            // move when a tile is streamed in again.
            let mut pieces = vec![ring];
            let mut taken = 0.0;
            for rival in out.rivals(&pieces[0], tile_size) {
                let other = out.fields[rival].ring.clone();
                let mut left = Vec::new();
                for piece in &pieces {
                    let shared: f64 =
                        fields::geometry::clip(piece, &other, fields::geometry::Op::Intersect)
                            .iter()
                            .map(|ring| fields::geometry::area(ring).abs())
                            .sum();
                    // Adjacent is not overlapping.
                    if shared <= MIN_SHARED {
                        left.push(piece.clone());
                        continue;
                    }
                    taken += shared;
                    left.extend(
                        fields::geometry::clip(piece, &other, fields::geometry::Op::Difference)
                            .into_iter()
                            .filter(|ring| {
                                ring.len() >= 3 && fields::geometry::area(ring).abs() > MIN_PIECE
                            }),
                    );
                }
                pieces = left;
                if pieces.is_empty() {
                    break;
                }
            }
            if taken > 0.0 {
                out.overlaps += 1;
                out.overlap_area += taken;
            }
            for piece in pieces {
                let at = out.fields.len();
                let (lo, hi) = fields::geometry::bounds(&piece);
                let (kx0, ky0) = key(lo, tile_size);
                let (kx1, ky1) = key(hi, tile_size);
                for ky in ky0..=ky1 {
                    for kx in kx0..=kx1 {
                        out.by_tile.entry((kx, ky)).or_default().push(at);
                    }
                }
                out.fields.push(Field {
                    ring: piece,
                    crop,
                    direction: source.direction_deg.to_radians(),
                    seed: source.seed,
                    index: index as u32,
                });
            }
        }
        out
    }

    /// The fields already placed whose tiles a ring reaches — the only ones
    /// it could be standing on. `by_tile` holds what has been placed so far,
    /// so this is exactly "everything that came before, near here".
    fn rivals(&self, ring: &[DVec2], tile_size: f64) -> Vec<usize> {
        let (lo, hi) = fields::geometry::bounds(ring);
        let (kx0, ky0) = key(lo, tile_size);
        let (kx1, ky1) = key(hi, tile_size);
        let mut out = Vec::new();
        for ky in ky0..=ky1 {
            for kx in kx0..=kx1 {
                let Some(here) = self.by_tile.get(&(kx, ky)) else {
                    continue;
                };
                for &at in here {
                    // Boxes first: a tile holds fields that never come near
                    // this one, and a clip is a great deal more work than
                    // four comparisons.
                    let (o_lo, o_hi) = fields::geometry::bounds(&self.fields[at].ring);
                    if o_hi.x < lo.x || hi.x < o_lo.x || o_hi.y < lo.y || hi.y < o_lo.y {
                        continue;
                    }
                    out.push(at);
                }
            }
        }
        // In order and once each: the tiles of a big field list it many times.
        out.sort_unstable();
        out.dedup();
        out
    }

    /// How many fields lost ground to one that came before them, and how much
    /// [m²] — what the editor's checks report and the line's log counts.
    pub fn overlaps(&self) -> (usize, f64) {
        (self.overlaps, self.overlap_area)
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    pub fn len(&self) -> usize {
        self.fields.len()
    }

    /// Whether any field reaches this tile — the cheap question the tile
    /// builder asks before doing any of the work below.
    pub fn touches(&self, k: TileKey) -> bool {
        self.by_tile.contains_key(&k)
    }
}

/// One crop's surface on one tile, in the tile's own frame.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldPatch {
    pub crop: CropClass,
    /// Render axes (x = east, y = up, z = −north), relative to the tile anchor.
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// `u` across the working direction, `v` along it, both in metres from the
    /// field's own centre. The shader makes furrows and tramlines of them, so
    /// the rows of one field line up across the tile boundaries it crosses.
    pub uvs: Vec<[f32; 2]>,
    /// Per-vertex tint, from the field's seed: two wheat fields side by side
    /// are never quite the same green. `g` carries the row phase, `b` the
    /// field's own week of the year (the standing crop's phenology reads it
    /// back), `a` is opaque.
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
    /// The fields that went into this patch, in line order — what a click on
    /// it selects, and what the editor highlights.
    pub sources: Vec<u32>,
}

impl FieldPatch {
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }
}

/// The surfaces of one tile, one per crop found on it.
pub(crate) fn patches(
    k: TileKey,
    grid: &HeightGrid,
    frame: &EnuFrame,
    zone: u8,
    tile_size: f64,
    fields: &Fields,
) -> Vec<FieldPatch> {
    let Some(indices) = fields.by_tile.get(&k) else {
        return Vec::new();
    };
    let min = DVec2::new(k.0 as f64 * tile_size, k.1 as f64 * tile_size);
    // The tile itself: a field is cut to it exactly, and the neighbouring tile
    // cuts the other half the same way, so the two meet without a seam.
    let rect = vec![
        min,
        DVec2::new(min.x + tile_size, min.y),
        min + DVec2::splat(tile_size),
        DVec2::new(min.x, min.y + tile_size),
    ];

    let mut by_crop: HashMap<CropClass, FieldPatch> = HashMap::new();
    for &at in indices {
        let field = &fields.fields[at];
        for piece in fields::geometry::clip(&field.ring, &rect, fields::geometry::Op::Intersect) {
            let patch = by_crop.entry(field.crop).or_insert_with(|| FieldPatch {
                crop: field.crop,
                positions: Vec::new(),
                normals: Vec::new(),
                uvs: Vec::new(),
                colors: Vec::new(),
                indices: Vec::new(),
                sources: Vec::new(),
            });
            if !patch.sources.contains(&field.index) {
                patch.sources.push(field.index);
            }
            add_piece(patch, &piece, field, grid, frame, zone);
        }
    }

    let mut out: Vec<FieldPatch> = by_crop
        .into_values()
        .filter(|p| !p.indices.is_empty())
        .collect();
    // A stable order, so the same tile always builds the same entities.
    out.sort_by_key(|p| p.crop);
    out
}

/// Triangulates one piece of a field, cuts it on the tile's own height grid
/// and drapes it there.
fn add_piece(
    patch: &mut FieldPatch,
    ring: &[DVec2],
    field: &Field,
    grid: &HeightGrid,
    frame: &EnuFrame,
    zone: u8,
) {
    let points = ring.to_vec();
    let tris = fields::geometry::triangulate(&points);
    if tris.is_empty() {
        return;
    }
    // Cut on the ground's own grid, so the surface follows the DGM instead of
    // spanning it.
    let (points, tris) = on_grid(&points, &tris, grid);

    // The rows run along the working direction, measured from the field's own
    // centre rather than the piece's — so the furrows of a field cut across
    // four tiles still line up.
    let centre = fields::geometry::centroid(&field.ring);
    let (sin, cos) = field.direction.sin_cos();
    let along = DVec2::new(cos, sin);
    let across = DVec2::new(-sin, cos);

    // A tint and a row phase per field, steady from its seed.
    let tint = fields::stats::vary(field.seed, 0xC0107) as f32;
    let phase = fields::stats::vary(field.seed, 0x9042E) as f32;
    // The field's own place in the crop year, as a half of the ±7-day spread
    // the seed gives: the standing crop (world_render::plants) reads it back
    // and knows this field is a week behind its neighbour without ever having
    // seen the seed.
    let year = (fields::phenology::offset_of(field.seed) / 7.0 + 1.0) as f32 * 0.5;
    let base = patch.positions.len() as u32;

    for p in &points {
        // The mesh, not the bilinear surface: a field is drawn over the
        // triangles the terrain draws, and between them the two disagree by
        // the cell's twist — a decimetre of ground on a rolling field.
        let height = grid.mesh_at(*p) + LIFT;
        let (lat, lon) = geo::from_utm(p.x, p.y, zone);
        let world = geo::to_ecef(lat, lon, height);
        patch.positions.push(to_render(frame.to_local(world)));
        patch.normals.push(normal_at(*p, grid, height));
        let offset = *p - centre;
        patch
            .uvs
            .push([offset.dot(across) as f32, offset.dot(along) as f32]);
        // r, g: the tint and the row phase. b is the field's own week of the
        // year, 0 … 1 — the standing crop's phenology reads it back. a is
        // opaque.
        patch.colors.push([tint, phase, year, 1.0]);
    }
    for [a, b, c] in tris {
        patch
            .indices
            .extend_from_slice(&[base + a, base + b, base + c]);
    }
}

/// Cuts a field's triangles on the terrain's own height grid.
///
/// A field is a polygon and the ground is a grid, and a triangle that spans
/// several grid cells is a flat plane where the ground is not: draped by its
/// corners, it cuts through everything between them. What this did before was
/// subdivide uniformly and give up after four levels — an imported field the
/// size of a tile came out with **32 m triangles over a 4 m grid**, which is
/// a field lying across the hills rather than on them.
///
/// Cutting it *on* the grid instead costs about two triangles a cell and puts
/// every vertex on a grid line, where the drape is exact. Two neighbouring
/// cells cut the same triangle edge at the same point, so nothing cracks and
/// no corner is left hanging between them.
///
/// The cell is the terrain's own, and the fan starts at its **south-east**
/// corner — which is the diagonal `build_tile` splits its cells on. A field
/// cut that way does not merely come close to the ground: over every cell it
/// covers whole it *is* the ground, triangle for triangle, and the five
/// centimetres of [`LIFT`] are the same five centimetres everywhere.
fn on_grid(points: &[DVec2], tris: &[[u32; 3]], grid: &HeightGrid) -> (Vec<DVec2>, Vec<[u32; 3]>) {
    let cut = grid.step();
    let origin = grid.min();
    let cell_of = |p: DVec2| ((p - origin) / cut).floor();

    let mut out_points: Vec<DVec2> = Vec::new();
    let mut at: HashMap<(i64, i64), u32> = HashMap::new();
    let mut out_tris: Vec<[u32; 3]> = Vec::new();
    let (mut poly, mut half, mut scratch) = (Vec::new(), Vec::new(), Vec::new());

    for &[a, b, c] in tris {
        let tri = [points[a as usize], points[b as usize], points[c as usize]];
        let lo = tri[0].min(tri[1]).min(tri[2]);
        let hi = tri[0].max(tri[1]).max(tri[2]);
        let (from, to) = (cell_of(lo), cell_of(hi));
        for y in from.y as i64..=to.y as i64 {
            for x in from.x as i64..=to.x as i64 {
                let min = origin + DVec2::new(x as f64, y as f64) * cut;
                clip_to_cell(tri, min, min + cut, &mut scratch, &mut poly);
                if poly.len() < 3 {
                    continue;
                }
                // `build_tile` splits every cell on its south-east to
                // north-west diagonal, so the field is split there too. That
                // is the step from *near* the ground to *on* it: each half
                // lies inside one terrain triangle, so a plane through its
                // corners is that triangle's own plane.
                let diagonal = min.x + min.y + cut;
                for lower in [true, false] {
                    half.clear();
                    half.extend_from_slice(&poly);
                    clip_half(&mut scratch, &mut half, |p| {
                        let side = diagonal - (p.x + p.y);
                        if lower { side } else { -side }
                    });
                    if half.len() < 3 {
                        continue;
                    }
                    // Vertices are shared by the millimetre, so the cuts of
                    // two triangles that met along an edge still meet along
                    // it.
                    let mut intern = |p: DVec2| -> u32 {
                        let key = ((p.x * 1e3).round() as i64, (p.y * 1e3).round() as i64);
                        *at.entry(key).or_insert_with(|| {
                            out_points.push(p);
                            out_points.len() as u32 - 1
                        })
                    };
                    let fan: Vec<u32> = half.iter().map(|p| intern(*p)).collect();
                    for i in 1..fan.len() - 1 {
                        // A sliver the weld collapsed draws nothing and costs
                        // a vertex fetch.
                        if fan[0] == fan[i] || fan[i] == fan[i + 1] || fan[0] == fan[i + 1] {
                            continue;
                        }
                        out_tris.push([fan[0], fan[i], fan[i + 1]]);
                    }
                }
            }
        }
    }
    (out_points, out_tris)
}

/// Clips a convex ring to the half-plane where `side` is not negative.
///
/// The cell's own four edges are done by [`clip_to_cell`], which can snap the
/// crossing exactly onto the plane because it knows which coordinate it is.
/// This one is for the cell's diagonal, which is internal to the cell — both
/// halves take their crossing from the same ring, so they agree with each
/// other and nothing outside the cell depends on it.
fn clip_half(scratch: &mut Vec<DVec2>, out: &mut Vec<DVec2>, side: impl Fn(DVec2) -> f64) {
    if out.len() < 3 {
        out.clear();
        return;
    }
    std::mem::swap(scratch, out);
    out.clear();
    for i in 0..scratch.len() {
        let (a, b) = (scratch[i], scratch[(i + 1) % scratch.len()]);
        let (sa, sb) = (side(a), side(b));
        if sa >= 0.0 {
            out.push(a);
        }
        if (sa >= 0.0) != (sb >= 0.0) {
            out.push(a + (b - a) * (sa / (sa - sb)));
        }
    }
    if out.len() < 3 {
        out.clear();
    }
}

/// A triangle clipped to an axis-aligned cell, as a convex ring.
///
/// Sutherland–Hodgman against the cell's four edges: a triangle and a
/// rectangle are both convex, so what comes out is one ring of at most seven
/// points, and the cells of the grid partition the triangle exactly.
fn clip_to_cell(
    tri: [DVec2; 3],
    min: DVec2,
    max: DVec2,
    scratch: &mut Vec<DVec2>,
    out: &mut Vec<DVec2>,
) {
    out.clear();
    out.extend_from_slice(&tri);
    for (axis, limit, above) in [
        (0usize, min.x, true),
        (0, max.x, false),
        (1, min.y, true),
        (1, max.y, false),
    ] {
        if out.len() < 3 {
            out.clear();
            return;
        }
        std::mem::swap(scratch, out);
        out.clear();
        let inside = |p: DVec2| {
            if above {
                p[axis] >= limit
            } else {
                p[axis] <= limit
            }
        };
        for i in 0..scratch.len() {
            let (a, b) = (scratch[i], scratch[(i + 1) % scratch.len()]);
            let (ia, ib) = (inside(a), inside(b));
            if ia {
                out.push(a);
            }
            // The two ends straddle the edge, so the crossing is on it — and
            // the denominator cannot vanish, because an edge parallel to the
            // limit has both ends on the same side of it.
            if ia != ib {
                let mut crossing = a + (b - a) * ((limit - a[axis]) / (b[axis] - a[axis]));
                // Snapped onto the plane rather than left a bit off it: the
                // cell next door computes the same crossing from the other
                // side, and the two have to be the same point exactly.
                crossing[axis] = limit;
                out.push(crossing);
            }
        }
    }
    if out.len() < 3 {
        out.clear();
    }
}

/// The ground's normal under a point, from the height grid's own gradient.
fn normal_at(p: DVec2, grid: &HeightGrid, _height: f64) -> [f32; 3] {
    // A metre either side: finer than the grid, so the difference is the
    // bilinear slope of the cell the point is in.
    const D: f64 = 1.0;
    let dx = grid.at(p + DVec2::new(D, 0.0)) - grid.at(p - DVec2::new(D, 0.0));
    let dy = grid.at(p + DVec2::new(0.0, D)) - grid.at(p - DVec2::new(0.0, D));
    // Render axes: +x east, +y up, +z south — so north is −z.
    let n = Vec3::new(-(dx / (2.0 * D)) as f32, 1.0, (dy / (2.0 * D)) as f32).normalize_or_zero();
    let n = if n == Vec3::ZERO { Vec3::Y } else { n };
    [n.x, n.y, n.z]
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
    use crate::route::{FieldPoint, FieldSource};

    /// A square field of `size` metres, with its south-west corner at the given
    /// UTM point in zone 32.
    fn source(e: f64, n: f64, size: f64, crop: &str) -> FieldSource {
        let corner = |dx: f64, dy: f64| {
            let (lat, lon) = geo::from_utm(e + dx, n + dy, 32);
            FieldPoint {
                lat: lat.to_degrees(),
                lon: lon.to_degrees(),
            }
        };
        FieldSource {
            polygon: vec![
                corner(0.0, 0.0),
                corner(size, 0.0),
                corner(size, size),
                corner(0.0, size),
            ],
            crop: crop.into(),
            code: String::new(),
            label: String::new(),
            level: String::new(),
            direction_deg: 0.0,
            source: String::new(),
            year: None,
            seed: 42,
            tags: Vec::new(),
        }
    }

    /// A field from an explicit ring, in metres east/north of a fixed point
    /// on the test line — for the cases where the shape is the point.
    fn source_ring(ring: &[(f64, f64)], crop: &str) -> FieldSource {
        let (e0, n0) = (440_000.0, 5_715_000.0);
        FieldSource {
            polygon: ring
                .iter()
                .map(|(e, n)| {
                    let (lat, lon) = geo::from_utm(e0 + e, n0 + n, 32);
                    FieldPoint {
                        lat: lat.to_degrees(),
                        lon: lon.to_degrees(),
                    }
                })
                .collect(),
            crop: crop.into(),
            ..source(0.0, 0.0, 1.0, crop)
        }
    }

    /// A flat height grid for one tile.
    fn flat(min: DVec2, step: f64, n: usize, height: f32) -> Vec<f32> {
        let _ = (min, step);
        vec![height; (n + 1) * (n + 1)]
    }

    #[test]
    fn a_field_lands_on_the_tiles_it_covers() {
        // 3 km across, so it spans several 512 m tiles.
        let fields = Fields::from_parts(
            &[source(440_000.0, 5_715_000.0, 3_000.0, "maize")],
            32,
            512.0,
        );
        assert_eq!(fields.len(), 1);
        assert!(fields.touches((859, 11162)), "{:?}", fields.by_tile.keys());
        // And not on a tile a long way off.
        assert!(!fields.touches((0, 0)));
    }

    #[test]
    fn an_unknown_crop_is_drawn_as_bare_ground_not_dropped() {
        let fields = Fields::from_parts(
            &[source(440_000.0, 5_715_000.0, 100.0, "quinoa")],
            32,
            512.0,
        );
        assert_eq!(fields.len(), 1);
        assert_eq!(fields.fields[0].crop, CropClass::Other);
    }

    #[test]
    fn a_polygon_with_two_corners_is_no_field() {
        let mut bad = source(440_000.0, 5_715_000.0, 100.0, "maize");
        bad.polygon.truncate(2);
        assert!(Fields::from_parts(&[bad], 32, 512.0).is_empty());
    }

    /// Builds the patches of one tile over flat ground.
    fn patches_of(sources: &[FieldSource], tile: TileKey) -> Vec<FieldPatch> {
        let tile_size = 512.0;
        let step = 8.0;
        let n = (tile_size / step) as usize;
        let min = DVec2::new(tile.0 as f64 * tile_size, tile.1 as f64 * tile_size);
        let heights = flat(min, step, n, 100.0);
        let grid = HeightGrid::new(min, &heights, step, n);
        let centre = min + DVec2::splat(tile_size / 2.0);
        let (clat, clon) = geo::from_utm(centre.x, centre.y, 32);
        let frame = EnuFrame::at(geo::to_ecef(clat, clon, 0.0));
        let fields = Fields::from_parts(sources, 32, tile_size);
        patches(tile, &grid, &frame, 32, tile_size, &fields)
    }

    #[test]
    fn a_field_becomes_a_draped_surface() {
        // One tile's worth: the tile at 440 000 / 5 715 000 in 512 m squares.
        let tile = (859, 11162);
        let min = DVec2::new(tile.0 as f64 * 512.0, tile.1 as f64 * 512.0);
        let patches = patches_of(
            &[source(min.x + 100.0, min.y + 100.0, 200.0, "maize")],
            tile,
        );
        assert_eq!(patches.len(), 1);
        let patch = &patches[0];
        assert_eq!(patch.crop, CropClass::Maize);
        assert_eq!(patch.sources, vec![0]);
        assert!(patch.triangles() > 0);
        assert_eq!(patch.positions.len(), patch.normals.len());
        assert_eq!(patch.positions.len(), patch.uvs.len());
        assert_eq!(patch.positions.len(), patch.colors.len());
        // Flat ground: every normal points up.
        for n in &patch.normals {
            assert!((n[1] - 1.0).abs() < 1e-5, "{n:?}");
        }
        // Every index addresses a vertex that exists.
        let count = patch.positions.len() as u32;
        assert!(patch.indices.iter().all(|i| *i < count));
    }

    #[test]
    fn two_crops_on_a_tile_are_two_patches() {
        let tile = (859, 11162);
        let min = DVec2::new(tile.0 as f64 * 512.0, tile.1 as f64 * 512.0);
        let patches = patches_of(
            &[
                source(min.x + 50.0, min.y + 50.0, 150.0, "maize"),
                source(min.x + 250.0, min.y + 50.0, 150.0, "winter-cereal"),
            ],
            tile,
        );
        assert_eq!(patches.len(), 2);
        // Sorted by crop, so the same tile always builds the same entities.
        assert!(patches[0].crop < patches[1].crop);
    }

    /// Total surface area of a patch [m²], from its own triangles.
    fn patch_area(patch: &FieldPatch) -> f64 {
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
            .sum()
    }

    #[test]
    fn a_field_is_cut_at_the_tile_boundary() {
        let tile = (859, 11162);
        let min = DVec2::new(tile.0 as f64 * 512.0, tile.1 as f64 * 512.0);
        // Straddling the eastern seam, 60 m of it in this tile and 140 in the
        // next — off centre, so a bug that halved it would still show.
        let field = source(min.x + 452.0, min.y + 100.0, 200.0, "maize");
        let here = patches_of(std::slice::from_ref(&field), tile);
        let next = patches_of(&[field], (tile.0 + 1, tile.1));
        assert_eq!(here.len(), 1);
        assert_eq!(next.len(), 1);
        // Nothing is lost and nothing is drawn twice: the pieces add up to the
        // field. (Within a tenth of a per cent — UTM's scale factor is not 1.)
        let total = patch_area(&here[0]) + patch_area(&next[0]);
        assert!((total - 40_000.0).abs() < 40.0, "{total}");
        // And the split is where the seam is, not somewhere else.
        assert!(
            (patch_area(&here[0]) - 12_000.0).abs() < 60.0,
            "{}",
            patch_area(&here[0])
        );
    }

    #[test]
    fn the_rows_line_up_across_a_tile_boundary() {
        let tile = (859, 11162);
        let min = DVec2::new(tile.0 as f64 * 512.0, tile.1 as f64 * 512.0);
        let field = source(min.x + 452.0, min.y + 100.0, 200.0, "maize");
        let here = patches_of(std::slice::from_ref(&field), tile);
        let next = patches_of(&[field], (tile.0 + 1, tile.1));
        // The UVs are measured from the field's own centre, not the tile's, so
        // the two halves share one coordinate system and the furrows of one
        // meet the furrows of the other. The field runs east-west and its
        // centre is 40 m east of the seam, so both pieces end at v = -40.
        let v = |patch: &FieldPatch| {
            patch.uvs.iter().fold((f32::MAX, f32::MIN), |(lo, hi), uv| {
                (lo.min(uv[1]), hi.max(uv[1]))
            })
        };
        let (lo_here, hi_here) = v(&here[0]);
        let (lo_next, hi_next) = v(&next[0]);
        assert!((hi_here - -40.0).abs() < 2.0, "{hi_here}");
        assert!((lo_next - -40.0).abs() < 2.0, "{lo_next}");
        // Between them they cover the whole field, once.
        assert!((lo_here - -100.0).abs() < 2.0, "{lo_here}");
        assert!((hi_next - 100.0).abs() < 2.0, "{hi_next}");
    }

    #[test]
    fn a_field_is_cut_on_the_ground_s_own_grid() {
        // Every triangle of a field has to lie inside one cell of the height
        // grid it is draped on, or it spans ground it never sampled. The old
        // uniform refinement gave up after four levels and left 32 m
        // triangles on a 4 m grid; this is the property that says it cannot.
        let heights = vec![0.0f32; 129 * 129];
        let grid = HeightGrid::new(DVec2::ZERO, &heights, 4.0, 128);
        let cut = grid.step();
        let ring = vec![
            DVec2::new(3.0, 5.0),
            DVec2::new(311.0, 17.0),
            DVec2::new(280.0, 297.0),
            DVec2::new(41.0, 233.0),
        ];
        let tris = fields::geometry::triangulate(&ring);
        let (points, cut_tris) = on_grid(&ring, &tris, &grid);
        assert!(!cut_tris.is_empty());
        for [a, b, c] in &cut_tris {
            let t = [
                points[*a as usize],
                points[*b as usize],
                points[*c as usize],
            ];
            let lo = t[0].min(t[1]).min(t[2]);
            let hi = t[0].max(t[1]).max(t[2]);
            // Inside one cell — the one its middle falls in.
            let cell = (((t[0] + t[1] + t[2]) / 3.0) / cut).floor();
            let (from, to) = (cell * cut, (cell + 1.0) * cut);
            assert!(
                lo.x >= from.x - 1e-9
                    && lo.y >= from.y - 1e-9
                    && hi.x <= to.x + 1e-9
                    && hi.y <= to.y + 1e-9,
                "{t:?} spans more than the {cut} m cell at {from:?}",
            );
        }
        // And the cut keeps the field's area: nothing is dropped, nothing is
        // covered twice.
        let whole: f64 = tris
            .iter()
            .map(|[a, b, c]| {
                let (a, b, c) = (ring[*a as usize], ring[*b as usize], ring[*c as usize]);
                (b - a).perp_dot(c - a).abs() / 2.0
            })
            .sum();
        let pieces: f64 = cut_tris
            .iter()
            .map(|[a, b, c]| {
                let (a, b, c) = (
                    points[*a as usize],
                    points[*b as usize],
                    points[*c as usize],
                );
                (b - a).perp_dot(c - a).abs() / 2.0
            })
            .sum();
        assert!((pieces - whole).abs() < whole * 1e-6, "{pieces} of {whole}");
    }

    /// A tile of ground with a hill on it, `step` metres to a grid point.
    fn hilly(step: f64, n: usize, amplitude: f64, wavelength: f64) -> Vec<f32> {
        (0..=n)
            .flat_map(|y| {
                (0..=n).map(move |x| {
                    let (u, v) = (x as f64 * step, y as f64 * step);
                    let t = std::f64::consts::TAU / wavelength;
                    (amplitude * (u * t).sin() * (v * t).cos()) as f32
                })
            })
            .collect()
    }

    #[test]
    fn two_fields_never_stand_on_the_same_ground() {
        // Two parcels overlapping by a strip — a register's own boundaries,
        // digitised twice and disagreeing by a few metres. The later one
        // gives the strip up, so no ground carries two surfaces at the same
        // height and no crop grows twice over it.
        let overlapping = [
            source_ring(
                &[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)],
                "maize",
            ),
            source_ring(
                &[(80.0, 0.0), (200.0, 0.0), (200.0, 100.0), (80.0, 100.0)],
                "winter-cereal",
            ),
        ];
        let fields = Fields::from_parts(&overlapping, 32, 512.0);
        let (count, taken) = fields.overlaps();
        assert_eq!(count, 1, "one field gave ground up");
        assert!((taken - 2_000.0).abs() < 50.0, "{taken} m² given up");
        // The first keeps all of its ten thousand square metres; the second
        // is down to the ten thousand it had to itself.
        let total: f64 = fields
            .fields
            .iter()
            .map(|f| fields::geometry::area(&f.ring).abs())
            .sum();
        assert!((total - 20_000.0).abs() < 100.0, "{total} m² of field");
        // And no two of them share any ground at all.
        for (i, a) in fields.fields.iter().enumerate() {
            for b in &fields.fields[i + 1..] {
                let shared: f64 =
                    fields::geometry::clip(&a.ring, &b.ring, fields::geometry::Op::Intersect)
                        .iter()
                        .map(|r| fields::geometry::area(r).abs())
                        .sum();
                assert!(shared <= MIN_SHARED, "{shared} m² shared");
            }
        }
    }

    #[test]
    fn parcels_that_only_touch_are_left_alone() {
        // Neighbours sharing a boundary are not overlapping, and cutting one
        // out of the other would be work for nothing — and a clip along two
        // coincident edges is where a clipper is least sure of itself.
        let touching = [
            source_ring(
                &[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)],
                "maize",
            ),
            source_ring(
                &[(100.0, 0.0), (200.0, 0.0), (200.0, 100.0), (100.0, 100.0)],
                "winter-cereal",
            ),
        ];
        let fields = Fields::from_parts(&touching, 32, 512.0);
        assert_eq!(
            fields.overlaps().0,
            0,
            "a shared boundary is not an overlap"
        );
        assert_eq!(fields.len(), 2);
        for f in &fields.fields {
            assert!(
                (fields::geometry::area(&f.ring).abs() - 10_000.0).abs() < 1.0,
                "a field lost ground to its neighbour",
            );
        }
    }

    #[test]
    fn a_field_cut_in_two_stays_one_field() {
        // Taking a strip out of the middle leaves two rings, and both are
        // still the same parcel: the same crop, the same seed, the same index
        // for the editor to select.
        let split = [
            source_ring(
                &[(40.0, 0.0), (60.0, 0.0), (60.0, 100.0), (40.0, 100.0)],
                "maize",
            ),
            source_ring(
                &[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)],
                "winter-cereal",
            ),
        ];
        let fields = Fields::from_parts(&split, 32, 512.0);
        let pieces: Vec<&Field> = fields.fields.iter().filter(|f| f.index == 1).collect();
        assert_eq!(pieces.len(), 2, "the strip cut it in two");
        assert!(pieces.iter().all(|f| f.crop == CropClass::WinterCereal));
        assert!(pieces.iter().all(|f| f.seed == pieces[0].seed));
        let left: f64 = pieces
            .iter()
            .map(|f| fields::geometry::area(&f.ring).abs())
            .sum();
        assert!((left - 8_000.0).abs() < 100.0, "{left} m² left");
    }

    #[test]
    fn a_field_lies_on_the_ground_and_not_across_it() {
        // The property the whole cut exists for. A field draped by its
        // corners is a plane where the ground is a shape: over a hill five
        // metres high it hangs metres clear in the middle and digs in at the
        // edges. Cut on the terrain's own cells *and* on the diagonal it
        // splits them by, every triangle of the field lies inside one
        // triangle of the ground — so it does not come close to it, it is it.
        let step = 8.0;
        let heights = hilly(step, 64, 5.0, 96.0);
        let grid = HeightGrid::new(DVec2::ZERO, &heights, step, 64);
        let ring = vec![
            DVec2::new(20.0, 20.0),
            DVec2::new(430.0, 30.0),
            DVec2::new(420.0, 400.0),
            DVec2::new(35.0, 380.0),
        ];
        let (points, tris) = on_grid(&ring, &fields::geometry::triangulate(&ring), &grid);

        let mut worst: f64 = 0.0;
        for [a, b, c] in &tris {
            let (pa, pb, pc) = (
                points[*a as usize],
                points[*b as usize],
                points[*c as usize],
            );
            let (ha, hb, hc) = (grid.mesh_at(pa), grid.mesh_at(pb), grid.mesh_at(pc));
            // A handful of points inside the triangle, by barycentric weight.
            for (u, v) in [(0.2, 0.2), (0.6, 0.2), (0.2, 0.6), (1.0 / 3.0, 1.0 / 3.0)] {
                let w = 1.0 - u - v;
                let p = pa * w + pb * u + pc * v;
                worst = worst.max((ha * w + hb * u + hc * v - grid.mesh_at(p)).abs());
            }
        }
        assert!(
            worst < 1e-6,
            "a field triangle stands {worst:.3} m off the ground it is drawn on",
        );
        // And the hill is really there, so the bound above means something:
        // a mesh that spanned it would be metres out.
        let relief = heights.iter().fold(0.0f32, |m, h| m.max(h.abs()));
        assert!(relief > 4.0, "{relief}");
    }

    #[test]
    fn the_cut_leaves_no_corner_hanging() {
        // Two triangles that met along an edge have to still meet along it
        // after the cut, or the field cracks open along the seam. They do
        // because both cut that edge at the grid lines it crosses, and the
        // vertices are shared by the millimetre.
        let heights = vec![0.0f32; 65 * 65];
        let grid = HeightGrid::new(DVec2::ZERO, &heights, 8.0, 64);
        let ring = vec![
            DVec2::new(1.0, 1.0),
            DVec2::new(97.0, 3.0),
            DVec2::new(95.0, 89.0),
            DVec2::new(5.0, 91.0),
        ];
        let (points, tris) = on_grid(&ring, &fields::geometry::triangulate(&ring), &grid);
        // Every edge is either on the outline or shared by exactly two
        // triangles — a hanging corner shows up as an edge used once inside.
        let mut edges: HashMap<(u32, u32), usize> = HashMap::new();
        for [a, b, c] in &tris {
            for (i, j) in [(*a, *b), (*b, *c), (*c, *a)] {
                *edges
                    .entry(if i < j { (i, j) } else { (j, i) })
                    .or_default() += 1;
            }
        }
        let outline: Vec<(u32, u32)> = edges
            .iter()
            .filter(|(_, n)| **n == 1)
            .map(|(e, _)| *e)
            .collect();
        assert!(edges.values().all(|n| *n <= 2), "an edge used three times");
        // The outline is a closed ring: every vertex on it is used twice.
        let mut ends: HashMap<u32, usize> = HashMap::new();
        for (a, b) in &outline {
            *ends.entry(*a).or_default() += 1;
            *ends.entry(*b).or_default() += 1;
        }
        assert!(
            ends.values().all(|n| *n == 2),
            "the boundary is not a closed ring — {} points",
            points.len(),
        );
    }

    #[test]
    fn the_vertex_colour_carries_the_field_s_week() {
        // Channel b is the field's place in the crop year, 0 … 1, from its
        // seed. The standing crop maps it back onto ±7 days; the round trip
        // has to land on the growth the seed itself gives.
        let patches = patches_of(
            &[source(440_000.0, 5_715_000.0, 100.0, "maize")],
            (859, 11162),
        );
        let b = patches[0].colors[0][2];
        assert!((0.0..=1.0).contains(&b), "{b}");
        let expected = (fields::phenology::offset_of(42) / 7.0 + 1.0) * 0.5;
        assert!((b - expected as f32).abs() < 1e-6);
    }

    #[test]
    fn a_slope_tilts_the_normals() {
        let tile = (859, 11162);
        let tile_size = 512.0;
        let step = 8.0;
        let n = (tile_size / step) as usize;
        let min = DVec2::new(tile.0 as f64 * tile_size, tile.1 as f64 * tile_size);
        // Ground rising one metre per metre eastward.
        let heights: Vec<f32> = (0..=n)
            .flat_map(|_| (0..=n).map(|ix| 100.0 + ix as f32 * step as f32))
            .collect();
        let grid = HeightGrid::new(min, &heights, step, n);
        let centre = min + DVec2::splat(tile_size / 2.0);
        let (clat, clon) = geo::from_utm(centre.x, centre.y, 32);
        let frame = EnuFrame::at(geo::to_ecef(clat, clon, 0.0));
        let source = source(min.x + 100.0, min.y + 100.0, 200.0, "maize");
        let fields = Fields::from_parts(&[source], 32, tile_size);
        let patches = patches(tile, &grid, &frame, 32, tile_size, &fields);
        // The surface leans west, away from the rise.
        assert!(
            patches[0].normals.iter().all(|n| n[0] < -0.5),
            "{:?}",
            patches[0].normals[0]
        );
    }
}
