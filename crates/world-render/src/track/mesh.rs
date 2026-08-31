//! The mesh plumbing the three parts of the track share: a vertex sink that
//! takes quads and boxes in world (ENU) coordinates, a strip builder for the
//! cross-section surfaces, and the deterministic wobble that keeps a bed from
//! reading as an extrusion.
//!
//! Every builder finishes around an origin of its own and hands that origin
//! back in render axes: a chunk entity has to sit where its geometry is, or a
//! distance cull measures to the wrong place (see
//! [`chunks_are_hung_on_their_own_centre`](super::tests)).

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use glam::DVec3;

/// ENU (x = east, y = north, z = up) → render axes (x = east, y = up, z = −north).
pub(super) fn to_render(p: DVec3) -> [f32; 3] {
    [p.x as f32, p.z as f32, -p.y as f32]
}

/// Collects triangles with their normals and up to two sets of texture
/// coordinates. `uv1` carries what a surface is rather than where it is — the
/// rails put the wheels' polish in it — and is only written into the mesh
/// when something asked for it.
pub(super) struct MeshBuilder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    uv1: Option<Vec<[f32; 2]>>,
    indices: Vec<u32>,
}

impl MeshBuilder {
    pub(super) fn new(with_uv1: bool) -> Self {
        Self {
            positions: Vec::new(),
            normals: Vec::new(),
            uvs: Vec::new(),
            uv1: with_uv1.then(Vec::new),
            indices: Vec::new(),
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    /// One quad, wound so its face points along the normal the ring gives —
    /// mesh and shading cannot disagree, because the normal is computed from
    /// the four corners rather than passed in beside them.
    pub(super) fn quad(&mut self, ring: [DVec3; 4], uv: [[f32; 2]; 4]) {
        let normal = (ring[1] - ring[0]).cross(ring[2] - ring[1]).normalize();
        self.quad_with_normals(ring, uv, [to_render(normal); 4], [[0.0; 2]; 4]);
    }

    /// A quad whose normals (and second uv set) are given per corner — for
    /// the rails, where the section knows its own normals better than four
    /// corner points do.
    pub(super) fn quad_with_normals(
        &mut self,
        ring: [DVec3; 4],
        uv: [[f32; 2]; 4],
        normals: [[f32; 3]; 4],
        uv1: [[f32; 2]; 4],
    ) {
        let base = self.positions.len() as u32;
        for i in 0..4 {
            self.positions.push(to_render(ring[i]));
            self.normals.push(normals[i]);
            self.uvs.push(uv[i]);
            if let Some(second) = &mut self.uv1 {
                second.push(uv1[i]);
            }
        }
        // Both triangles over the 0–2 diagonal — a pattern over 1–3 winds one
        // of them back into the face.
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 3, base]);
    }

    /// A fan around `centre` closing the ring `points` — the cap on a cut
    /// rail or the end of a sleeper, so neither is looked into. `outward`
    /// decides which way the cap faces; the ring may be given either way
    /// round and is wound to match.
    ///
    /// The cap carries real texture coordinates like every other face: a
    /// mesh with a degenerate uv island cannot be given tangents at all, and
    /// a mesh without tangents silently drops its normal map.
    pub(super) fn fan(
        &mut self,
        centre: (DVec3, [f32; 2]),
        points: &[(DVec3, [f32; 2])],
        outward: DVec3,
    ) {
        let normal = to_render(outward.normalize());
        let base = self.positions.len() as u32;
        self.push_vertex(centre.0, normal, centre.1);
        for (p, uv) in points {
            self.push_vertex(*p, normal, *uv);
        }
        for i in 0..points.len() {
            let next = (i + 1) % points.len();
            let (a, b) = (base + 1 + i as u32, base + 1 + next as u32);
            let winding = (points[i].0 - centre.0).cross(points[next].0 - centre.0);
            let (a, b) = if winding.dot(outward) > 0.0 {
                (a, b)
            } else {
                (b, a)
            };
            self.indices.extend_from_slice(&[base, a, b]);
        }
    }

    fn push_vertex(&mut self, p: DVec3, normal: [f32; 3], uv: [f32; 2]) {
        self.positions.push(to_render(p));
        self.normals.push(normal);
        self.uvs.push(uv);
        if let Some(second) = &mut self.uv1 {
            second.push([0.0; 2]);
        }
    }

    /// One triangle, wound as given, with the normal it is handed — for the
    /// two odd ends of a strip that is otherwise all quads.
    pub(super) fn triangle(&mut self, ring: [DVec3; 3], uv: [[f32; 2]; 3], normal: [f32; 3]) {
        let base = self.positions.len() as u32;
        for i in 0..3 {
            self.push_vertex(ring[i], normal, uv[i]);
        }
        self.indices.extend_from_slice(&[base, base + 1, base + 2]);
    }

    /// A box given by its centre and three half-axes — the guide plates,
    /// clips and baseplates of a rail fastening are all this shape, and
    /// nothing else in the track needs a general solid. The axes have to be
    /// **right-handed** (`x × y = z`), or every face is wound inside out.
    ///
    /// Each face takes the whole texture: a fastening is bare steel and gets
    /// its look from the material, not from a map, but the coordinates have
    /// to be non-degenerate all the same so the mesh can carry tangents.
    pub(super) fn cuboid(&mut self, centre: DVec3, axes: [DVec3; 3]) {
        let [x, y, z] = axes;
        let corner = |sx: f64, sy: f64, sz: f64| centre + x * sx + y * sy + z * sz;
        let uvs = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        // Six faces, each wound counter-clockwise seen from outside.
        let faces = [
            // +x, −x
            [
                corner(1.0, -1.0, -1.0),
                corner(1.0, 1.0, -1.0),
                corner(1.0, 1.0, 1.0),
                corner(1.0, -1.0, 1.0),
            ],
            [
                corner(-1.0, -1.0, 1.0),
                corner(-1.0, 1.0, 1.0),
                corner(-1.0, 1.0, -1.0),
                corner(-1.0, -1.0, -1.0),
            ],
            // +y, −y
            [
                corner(-1.0, 1.0, -1.0),
                corner(1.0, 1.0, -1.0),
                corner(1.0, 1.0, 1.0),
                corner(-1.0, 1.0, 1.0),
            ],
            [
                corner(-1.0, -1.0, 1.0),
                corner(1.0, -1.0, 1.0),
                corner(1.0, -1.0, -1.0),
                corner(-1.0, -1.0, -1.0),
            ],
            // +z, −z
            [
                corner(-1.0, -1.0, 1.0),
                corner(-1.0, 1.0, 1.0),
                corner(1.0, 1.0, 1.0),
                corner(1.0, -1.0, 1.0),
            ],
            [
                corner(1.0, -1.0, -1.0),
                corner(1.0, 1.0, -1.0),
                corner(-1.0, 1.0, -1.0),
                corner(-1.0, -1.0, -1.0),
            ],
        ];
        for face in faces {
            self.quad(face, uvs);
        }
    }

    /// Finishes the chunk around `origin` (ENU in the edge's frame): every
    /// vertex moves into that frame and the render-space offset comes back
    /// with the mesh, because that is where the entity has to sit.
    pub(super) fn build(mut self, origin: DVec3) -> (Mesh, Vec3) {
        let o = to_render(origin);
        for p in &mut self.positions {
            *p = [p[0] - o[0], p[1] - o[1], p[2] - o[2]];
        }
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs);
        if let Some(second) = self.uv1 {
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, second);
        }
        mesh.insert_indices(Indices::U32(self.indices));
        // The caller gets the offset back so the entity can be hung there.
        (with_tangents(mesh), Vec3::from(o))
    }
}

/// Adds tangents where the mesh can carry them. A normal map without them is
/// silently ignored by the PBR shader, which is the quiet way a bed loses its
/// relief; a degenerate uv layout (a fan's cap, all at one texel) cannot have
/// them and is left alone rather than made an error.
pub(super) fn with_tangents(mut mesh: Mesh) -> Mesh {
    if mesh.generate_tangents().is_err() {
        debug!("track mesh has no usable uv layout for tangents");
    }
    mesh
}

/// One cross-section row: columns left to right, each with its texture
/// coordinates.
type Row = (Vec<[f32; 3]>, Vec<[f32; 2]>);

/// A strip meshed from cross-section rows: each row is the same number of
/// columns, and consecutive columns — bridged left to right — become quads
/// facing upwards, where the camera is (pinned by a test).
#[derive(Default)]
pub(super) struct SectionBuilder {
    rows: Vec<Row>,
}

impl SectionBuilder {
    pub(super) fn push_row(&mut self, positions: Vec<[f32; 3]>, uvs: Vec<[f32; 2]>) {
        self.rows.push((positions, uvs));
    }

    pub(super) fn build(self) -> Mesh {
        let stride = self.rows.first().map_or(0, |(p, _)| p.len()).max(1);
        let mut positions = Vec::with_capacity(self.rows.len() * stride);
        let mut uvs = Vec::with_capacity(positions.capacity());
        let mut indices = Vec::new();
        for (row, (row_positions, row_uvs)) in self.rows.iter().enumerate() {
            for (pos, uv) in row_positions.iter().zip(row_uvs) {
                positions.push(*pos);
                uvs.push(*uv);
            }
            if row + 1 < self.rows.len() {
                for col in 0..stride.saturating_sub(1) {
                    let a = (row * stride + col) as u32;
                    let b = (row * stride + col + 1) as u32;
                    let c = ((row + 1) * stride + col) as u32;
                    let d = ((row + 1) * stride + col + 1) as u32;
                    // Left, right, then along the track: that order faces
                    // upwards — the other way round the strip is a backface
                    // and the bed is not drawn at all.
                    indices.extend_from_slice(&[a, b, c, b, d, c]);
                }
            }
        }
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
        mesh.insert_indices(Indices::U32(indices));
        mesh.compute_normals();
        with_tangents(mesh)
    }
}

/// SplitMix64 finaliser → \[0, 1). The track's own randomness: a bed that is
/// flat to the millimetre and sleepers laid to the millimetre are what makes
/// track read as CAD, so both are wobbled — but from a hash of *where* they
/// are, never from a random number generator. Two machines in the same
/// session have to build the same track, and a chunk rebuilt when the camera
/// comes back has to be the chunk that was there before.
pub(super) fn hash01(a: u64, b: u64) -> f64 {
    let mut z = a
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(b.rotate_left(31));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    f64::from((z >> 40) as u32) / f64::from(1u32 << 24)
}

/// Smooth value noise along one axis \[m\] with the given wavelength — the
/// long, soft undulation a tamped ballast bed has, not per-vertex hash.
pub(super) fn wobble(s: f64, wavelength: f64, seed: u64) -> f64 {
    let x = s / wavelength;
    let cell = x.floor();
    let t = x - cell;
    let smooth = t * t * (3.0 - 2.0 * t);
    let a = hash01(cell as i64 as u64, seed) * 2.0 - 1.0;
    let b = hash01(cell as i64 as u64 + 1, seed) * 2.0 - 1.0;
    a * (1.0 - smooth) + b * smooth
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hash is a function of its inputs and nothing else — same track on
    /// every machine in a session, same chunk after it is rebuilt.
    #[test]
    fn the_wobble_is_a_function_of_where_it_is() {
        for s in [0.0, 3.25, 191.5, -40.0] {
            assert_eq!(wobble(s, 7.0, 1), wobble(s, 7.0, 1));
        }
        assert_ne!(wobble(0.0, 7.0, 1), wobble(0.0, 7.0, 2));
        // Bounded and continuous: a bed that jumps 20 cm between two rows is
        // not a bed.
        let mut previous = wobble(0.0, 7.0, 1);
        for i in 1..2000 {
            let value = wobble(i as f64 * 0.25, 7.0, 1);
            assert!((-1.0..=1.0).contains(&value), "{value}");
            assert!((value - previous).abs() < 0.2, "jump at {i}");
            previous = value;
        }
    }
}
