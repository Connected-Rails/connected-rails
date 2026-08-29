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
// ponytail: the surface lies on the ground, and the crop's height is colour and
// row contrast rather than geometry. A maize field in August really does stand
// two and a half metres above the track, and standing crop wants its own pass —
// a shell over the surface with sides, or instanced cards. This is the ground
// it would stand on.

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

/// Target edge length of a field's mesh [m]. The terrain's own grid is four
/// metres at its finest, so a field that follows it to eight is following the
/// shape of the ground and not the shape of its own triangulation.
const TARGET_EDGE: f64 = 8.0;

/// Most a single patch is subdivided. Four levels turn one triangle into 256,
/// which is where a field the size of a tile lands at [`TARGET_EDGE`].
const MAX_LEVELS: u32 = 4;

/// The fields of a line, indexed by the terrain tiles they touch.
#[derive(Debug, Clone, Default)]
pub struct Fields {
    /// Per tile, the fields whose bounding box reaches it.
    by_tile: HashMap<TileKey, Vec<usize>>,
    fields: Vec<Field>,
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
            let at = out.fields.len();
            let (lo, hi) = fields::geometry::bounds(&ring);
            let (kx0, ky0) = key(lo, tile_size);
            let (kx1, ky1) = key(hi, tile_size);
            for ky in ky0..=ky1 {
                for kx in kx0..=kx1 {
                    out.by_tile.entry((kx, ky)).or_default().push(at);
                }
            }
            out.fields.push(Field {
                ring,
                crop,
                direction: source.direction_deg.to_radians(),
                seed: source.seed,
                index: index as u32,
            });
        }
        out
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
    /// are never quite the same green. `a` carries the seed as a fraction, so
    /// the shader can vary the row phase per field too.
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

/// Triangulates one piece of a field, refines it to [`TARGET_EDGE`] and drapes
/// it on the tile's own height grid.
fn add_piece(
    patch: &mut FieldPatch,
    ring: &[DVec2],
    field: &Field,
    grid: &HeightGrid,
    frame: &EnuFrame,
    zone: u8,
) {
    let mut points = ring.to_vec();
    let mut tris = fields::geometry::triangulate(&points);
    if tris.is_empty() {
        return;
    }
    refine(&mut points, &mut tris, levels(ring));

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
    let base = patch.positions.len() as u32;

    for p in &points {
        let height = grid.at(*p) + LIFT;
        let (lat, lon) = geo::from_utm(p.x, p.y, zone);
        let world = geo::to_ecef(lat, lon, height);
        patch.positions.push(to_render(frame.to_local(world)));
        patch.normals.push(normal_at(*p, grid, height));
        let offset = *p - centre;
        patch
            .uvs
            .push([offset.dot(across) as f32, offset.dot(along) as f32]);
        // r, g: the tint and the row phase. b is spare, a is opaque.
        patch.colors.push([tint, phase, 0.0, 1.0]);
    }
    for [a, b, c] in tris {
        patch
            .indices
            .extend_from_slice(&[base + a, base + b, base + c]);
    }
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
/// by edge length: a mesh subdivided the same everywhere cannot crack, and a
/// field is flat enough that the wasted vertices on its short axis are a few
/// hundred bytes.
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
    fn refinement_follows_the_ground() {
        // A field 300 m across gets subdivided; a 5 m one does not.
        let big = vec![
            DVec2::ZERO,
            DVec2::new(300.0, 0.0),
            DVec2::new(300.0, 300.0),
        ];
        let small = vec![DVec2::ZERO, DVec2::new(5.0, 0.0), DVec2::new(5.0, 5.0)];
        assert!(levels(&big) > 0);
        assert_eq!(levels(&small), 0);

        let mut points = big.clone();
        let mut tris = fields::geometry::triangulate(&points);
        let before = tris.len();
        refine(&mut points, &mut tris, 2);
        assert_eq!(tris.len(), before * 16);
        // Subdivision is conforming: no vertex is repeated, so no crack.
        let mut sorted: Vec<(u64, u64)> = points
            .iter()
            .map(|p| ((p.x * 1000.0) as u64, (p.y * 1000.0) as u64))
            .collect();
        sorted.sort_unstable();
        let count = sorted.len();
        sorted.dedup();
        assert_eq!(sorted.len(), count, "a midpoint was made twice");
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
