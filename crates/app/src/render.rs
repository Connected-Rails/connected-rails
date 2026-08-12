//! Procedural track rendering and floating-origin synchronisation (plan ch. 4, 12).

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use glam::DVec3;
use track_model::{EdgeId, TrackNetwork};
use world_coords::{EcefPos, EnuFrame, RenderOrigin};

/// Reference point of the rendering as a Bevy resource.
#[derive(Resource)]
pub struct Origin(pub RenderOrigin);

/// An object whose geometry lies in the ENU frame of a fixed world point
/// (track, scenery). On an origin rebase only the transform is set anew.
#[derive(Component)]
pub struct WorldAnchored {
    pub anchor: EcefPos,
}

/// A terrain tile — with its own view distance so that distant tiles are not drawn.
#[derive(Component)]
pub struct TerrainChunk {
    /// Circumscribed radius of the tile [m].
    pub radius: f32,
    pub lod: u8,
}

/// A vehicle in train `train`, vehicle index `vehicle`.
#[derive(Component)]
pub struct VehicleView {
    pub train: usize,
    pub vehicle: usize,
}

/// Track gauge [m].
const GAUGE: f64 = 1.435;
/// Half width of the ballast bed [m].
const BALLAST_HALF: f64 = 2.6;
/// Sample spacing along the edge [m].
const SAMPLE: f64 = 4.0;

/// Builds meshes for all edges of the network and spawns them.
pub fn spawn_track(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    net: &TrackNetwork,
    origin: &RenderOrigin,
) {
    let ballast_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.32, 0.30, 0.28),
        perceptual_roughness: 1.0,
        ..default()
    });
    let rail_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.45, 0.45, 0.48),
        metallic: 0.8,
        perceptual_roughness: 0.35,
        ..default()
    });

    for edge in net.edges() {
        let anchor = edge.anchor;
        let frame = EnuFrame::at(anchor);
        let (translation, rotation) = origin.frame_transform(&frame);

        let (ballast, rails) = build_edge_meshes(net, edge.id, &frame);
        for (mesh, material) in [
            (ballast, ballast_material.clone()),
            (rails, rail_material.clone()),
        ] {
            commands.spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(material),
                Transform::from_translation(translation).with_rotation(rotation),
                WorldAnchored { anchor },
            ));
        }
    }
}

/// Creates ballast bed and rails of an edge in the ENU frame `frame`.
fn build_edge_meshes(net: &TrackNetwork, edge: EdgeId, frame: &EnuFrame) -> (Mesh, Mesh) {
    let e = net.edge(edge);
    let steps = ((e.length() / SAMPLE).ceil() as usize).max(1);

    let mut ballast = RibbonBuilder::default();
    let mut rails = RibbonBuilder::default();

    for i in 0..=steps {
        let s = e.length() * i as f64 / steps as f64;
        let pose = e.eval(s);
        let center = frame.to_local(pose.pos);
        let tangent = frame.dir_to_local(pose.tangent);
        let up = frame.dir_to_local(pose.up);
        let right = tangent.cross(up).normalize();

        // Ballast bed, 30 cm below the rail head.
        let bed = center - up * 0.3;
        ballast.push_pair(bed - right * BALLAST_HALF, bed + right * BALLAST_HALF);

        // Two rails as narrow ribbons.
        let half = GAUGE / 2.0;
        rails.push_quad(
            center - right * (half + 0.04),
            center - right * (half - 0.04),
            center + right * (half - 0.04),
            center + right * (half + 0.04),
        );
    }

    (ballast.build(), rails.build_pairs())
}

/// Collects a ribbon from point pairs and builds a triangle mesh from it.
#[derive(Default)]
struct RibbonBuilder {
    positions: Vec<[f32; 3]>,
    /// Points per cross section (2 for one ribbon, 4 for two rails).
    stride: usize,
}

impl RibbonBuilder {
    fn push_pair(&mut self, left: DVec3, right: DVec3) {
        self.stride = 2;
        self.positions.push(to_render(left));
        self.positions.push(to_render(right));
    }

    fn push_quad(&mut self, a: DVec3, b: DVec3, c: DVec3, d: DVec3) {
        self.stride = 4;
        for p in [a, b, c, d] {
            self.positions.push(to_render(p));
        }
    }

    fn build(self) -> Mesh {
        self.build_with(&[(0, 1)])
    }

    /// Two separate ribbons (left and right rail).
    fn build_pairs(self) -> Mesh {
        self.build_with(&[(0, 1), (2, 3)])
    }

    fn build_with(self, bands: &[(usize, usize)]) -> Mesh {
        let stride = self.stride.max(1);
        let rows = self.positions.len() / stride;
        let mut indices = Vec::new();
        for row in 0..rows.saturating_sub(1) {
            for (l, r) in bands.iter().copied() {
                let a = (row * stride + l) as u32;
                let b = (row * stride + r) as u32;
                let c = ((row + 1) * stride + l) as u32;
                let d = ((row + 1) * stride + r) as u32;
                indices.extend_from_slice(&[a, c, b, b, c, d]);
            }
        }
        let normals = vec![[0.0f32, 1.0, 0.0]; self.positions.len()];
        let uvs: Vec<[f32; 2]> = (0..self.positions.len())
            .map(|i| [(i % stride) as f32, (i / stride) as f32 * 0.5])
            .collect();

        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
        mesh.insert_indices(Indices::U32(indices));
        mesh.compute_normals();
        mesh
    }
}

/// ENU (x = east, y = north, z = up) → render axes (x = east, y = up, z = −north).
fn to_render(p: DVec3) -> [f32; 3] {
    [p.x as f32, p.z as f32, -p.y as f32]
}

/// One material per LOD level — that way it is visible in debug where the resolution
/// changes, and the levels can be coloured separately.
pub fn terrain_materials(
    materials: &mut Assets<StandardMaterial>,
) -> Vec<Handle<StandardMaterial>> {
    [
        Color::srgb(0.36, 0.45, 0.26),
        Color::srgb(0.37, 0.46, 0.27),
        Color::srgb(0.38, 0.47, 0.28),
        Color::srgb(0.39, 0.48, 0.29),
    ]
    .into_iter()
    .map(|base_color| {
        materials.add(StandardMaterial {
            base_color,
            perceptual_roughness: 0.95,
            ..default()
        })
    })
    .collect()
}

/// Spawns a single terrain tile from [`content::terrain`] (streaming, plan 4.3).
pub fn spawn_terrain_tile(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &[Handle<StandardMaterial>],
    tile: &content::TerrainTile,
    origin: &RenderOrigin,
) -> Entity {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    let uvs: Vec<[f32; 2]> = tile
        .positions
        .iter()
        .map(|p| [p[0] / 32.0, p[2] / 32.0])
        .collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, tile.positions.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(tile.indices.clone()));
    mesh.compute_normals();

    let frame = EnuFrame::at(tile.anchor);
    let (translation, rotation) = origin.frame_transform(&frame);
    commands
        .spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials[(tile.lod as usize).min(materials.len() - 1)].clone()),
            Transform::from_translation(translation).with_rotation(rotation),
            WorldAnchored {
                anchor: tile.anchor,
            },
            TerrainChunk {
                radius: tile.radius,
                lod: tile.lod,
            },
        ))
        .id()
}

/// Sets the transforms of all world-anchored objects anew — after an origin rebase.
pub fn resync_anchored(origin: &RenderOrigin, query: &mut Query<(&WorldAnchored, &mut Transform)>) {
    for (anchored, mut transform) in query.iter_mut() {
        let frame = EnuFrame::at(anchored.anchor);
        let (translation, rotation) = origin.frame_transform(&frame);
        transform.translation = translation;
        transform.rotation = rotation;
    }
}
