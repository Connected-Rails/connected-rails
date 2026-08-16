//! Procedural track rendering and floating-origin synchronisation (plan ch. 4, 12).

use bevy::asset::RenderAssetUsages;
use bevy::gltf::GltfAssetLabel;
use bevy::image::{
    ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor,
};
use bevy::mesh::{ConeAnchor, CylinderAnchor, MeshBuilder};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;
use bevy::world_serialization::WorldAsset;
use glam::DVec3;
use track_model::{TrackEdge, TrackNetwork};
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
#[derive(Component, Clone, Copy)]
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

/// Builds meshes for all edges of the network and spawns them. The ballast
/// bed takes its material from the edge's track type — color, and the type's
/// texture where one is named (`mods://…`, tiled along the track); the type
/// sections of one edge become separate meshes at the type boundaries.
pub fn spawn_track(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    assets: &AssetServer,
    net: &TrackNetwork,
    origin: &RenderOrigin,
) {
    let ballast_materials: Vec<Handle<StandardMaterial>> = net
        .types()
        .iter()
        .map(|ty| {
            let texture = ty.texture.as_ref().map(|file| {
                assets
                    .load_builder()
                    .with_settings(|settings: &mut ImageLoaderSettings| {
                        // The ballast tiles along the track — clamped edges
                        // would smear the last texel over kilometres.
                        settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                            address_mode_u: ImageAddressMode::Repeat,
                            address_mode_v: ImageAddressMode::Repeat,
                            ..default()
                        });
                    })
                    .load(crate::models::asset_path(file))
            });
            materials.add(StandardMaterial {
                base_color: Color::srgb(ty.color.0, ty.color.1, ty.color.2),
                base_color_texture: texture,
                perceptual_roughness: 1.0,
                ..default()
            })
        })
        .collect();
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

        let mut parts: Vec<(Mesh, Handle<StandardMaterial>)> = edge
            .track_type_runs()
            .into_iter()
            .map(|(s0, s1, index)| {
                let material = ballast_materials
                    .get(index as usize)
                    .unwrap_or(&ballast_materials[0]);
                (build_ballast(edge, &frame, s0, s1), material.clone())
            })
            .collect();
        parts.push((build_rails(edge, &frame), rail_material.clone()));
        for (mesh, material) in parts {
            commands.spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(material),
                Transform::from_translation(translation).with_rotation(rotation),
                WorldAnchored { anchor },
            ));
        }
    }
}

/// Cross section of the track at `s`: centre, right and up in `frame`.
fn cross_section(e: &TrackEdge, frame: &EnuFrame, s: f64) -> (DVec3, DVec3, DVec3) {
    let pose = e.eval(s);
    let center = frame.to_local(pose.pos);
    let tangent = frame.dir_to_local(pose.tangent);
    let up = frame.dir_to_local(pose.up);
    let right = tangent.cross(up).normalize();
    (center, right, up)
}

/// Ballast bed of the edge between `s0` and `s1`, 30 cm below the rail head.
fn build_ballast(e: &TrackEdge, frame: &EnuFrame, s0: f64, s1: f64) -> Mesh {
    let steps = (((s1 - s0) / SAMPLE).ceil() as usize).max(1);
    let mut ballast = RibbonBuilder {
        // The texture continues across a type boundary instead of restarting.
        uv_row_offset: (s0 / SAMPLE) as f32,
        ..default()
    };
    for i in 0..=steps {
        let s = s0 + (s1 - s0) * i as f64 / steps as f64;
        let (center, right, up) = cross_section(e, frame, s);
        let bed = center - up * 0.3;
        ballast.push_pair(bed - right * BALLAST_HALF, bed + right * BALLAST_HALF);
    }
    ballast.build()
}

/// Two rails as narrow ribbons over the whole edge.
fn build_rails(e: &TrackEdge, frame: &EnuFrame) -> Mesh {
    let steps = ((e.length() / SAMPLE).ceil() as usize).max(1);
    let mut rails = RibbonBuilder::default();
    for i in 0..=steps {
        let s = e.length() * i as f64 / steps as f64;
        let (center, right, _) = cross_section(e, frame, s);
        let half = GAUGE / 2.0;
        rails.push_quad(
            center - right * (half + 0.04),
            center - right * (half - 0.04),
            center + right * (half - 0.04),
            center + right * (half + 0.04),
        );
    }
    rails.build_pairs()
}

/// Collects a ribbon from point pairs and builds a triangle mesh from it.
#[derive(Default)]
struct RibbonBuilder {
    positions: Vec<[f32; 3]>,
    /// Points per cross section (2 for one ribbon, 4 for two rails).
    stride: usize,
    /// Added to the UV row index — a mesh that starts mid-edge keeps the
    /// texture phase of the whole edge.
    uv_row_offset: f32,
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
            .map(|i| {
                [
                    (i % stride) as f32,
                    ((i / stride) as f32 + self.uv_row_offset) * 0.5,
                ]
            })
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

/// Terrain material: the standard PBR path plus texture splatting (plan ch. 14).
pub type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainSplat>;

/// Splat extension — three generated ground textures, blended in
/// `terrain_splat.wgsl` by the vertex-color weights `content::terrain` bakes
/// into every tile (r = grass, g = rock, b = gravel).
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct TerrainSplat {
    #[texture(100)]
    #[sampler(101)]
    grass: Handle<Image>,
    #[texture(102)]
    #[sampler(103)]
    rock: Handle<Image>,
    #[texture(104)]
    #[sampler(105)]
    gravel: Handle<Image>,
}

impl MaterialExtension for TerrainSplat {
    fn fragment_shader() -> ShaderRef {
        // The embedded path starts with the *bin target* name, not the package name.
        "embedded://train_sim/terrain_splat.wgsl".into()
    }
}

/// The one terrain material, its ground textures generated at startup —
/// like the sound sources (ch. 13), the repository carries no binary assets.
// ponytail: procedural noise textures instead of authored ones — photographed
// ground goes into a mod once terrain texturing is moddable content.
pub fn terrain_material(
    images: &mut Assets<Image>,
    materials: &mut Assets<TerrainMaterial>,
) -> Handle<TerrainMaterial> {
    materials.add(TerrainMaterial {
        base: StandardMaterial {
            perceptual_roughness: 0.95,
            ..default()
        },
        extension: TerrainSplat {
            grass: images.add(ground_texture(
                [0.20, 0.32, 0.11],
                [0.41, 0.45, 0.18],
                64,
                1,
            )),
            rock: images.add(ground_texture(
                [0.35, 0.33, 0.31],
                [0.55, 0.53, 0.50],
                48,
                2,
            )),
            gravel: images.add(ground_texture(
                [0.39, 0.35, 0.29],
                [0.57, 0.54, 0.49],
                12,
                3,
            )),
        },
    })
}

/// Edge length of the generated ground textures [texels]; one repeat covers
/// 32 m of terrain (the UV scale below).
const GROUND_TEXTURE_SIZE: u32 = 256;

/// One tileable ground texture: two octaves of value noise mix `base` towards
/// `accent`, `cell` sets the patch size in texels.
fn ground_texture(base: [f32; 3], accent: [f32; 3], cell: u32, seed: u64) -> Image {
    let size = GROUND_TEXTURE_SIZE;
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let t = tileable_noise(x, y, size, cell, seed);
            for c in 0..3 {
                let v = base[c] * (1.0 - t) + accent[c] * t;
                data.push((v.clamp(0.0, 1.0) * 255.0) as u8);
            }
            data.push(255);
        }
    }
    let (data, mip_level_count) = with_mipmaps(data, size);

    // `Image::new` checks the data length against mip level 0 — build uninit
    // and attach the full mip chain by hand.
    let mut image = Image::new_uninit(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = mip_level_count;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 4,
        ..default()
    });
    image
}

/// Value noise that wraps at the texture border, so tiling shows no seam.
fn tileable_noise(x: u32, y: u32, size: u32, cell: u32, seed: u64) -> f32 {
    let mut sum = 0.0;
    let mut amp = 2.0 / 3.0;
    let mut cell = cell.clamp(2, size);
    for octave in 0..2u64 {
        let period = (size / cell).max(1) as u64;
        let (gx, gy) = (x / cell, y / cell);
        let fx = (x % cell) as f32 / cell as f32;
        let fy = (y % cell) as f32 / cell as f32;
        let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
        let h = |dx: u32, dy: u32| {
            hash01(
                ((gx + dx) as u64) % period,
                ((gy + dy) as u64) % period,
                seed.wrapping_add(octave),
            )
        };
        let top = h(0, 0) * (1.0 - sx) + h(1, 0) * sx;
        let bottom = h(0, 1) * (1.0 - sx) + h(1, 1) * sx;
        sum += (top * (1.0 - sy) + bottom * sy) * amp;
        amp /= 2.0;
        cell = (cell / 4).max(2);
    }
    sum
}

/// SplitMix64 finaliser → [0, 1).
fn hash01(x: u64, y: u64, seed: u64) -> f32 {
    let mut z = seed ^ x.wrapping_mul(0x8CB9_2BA7_2F3D_8DD7) ^ y.rotate_left(32);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    ((z >> 40) as f32) / (1u64 << 24) as f32
}

/// Appends a box-filtered mip chain — generated images have no loader to build
/// one, and without it the ground shimmers at a distance.
fn with_mipmaps(mut data: Vec<u8>, size: u32) -> (Vec<u8>, u32) {
    let mut levels = 1;
    let mut prev_size = size as usize;
    let mut prev_start = 0usize;
    while prev_size > 1 {
        let next = prev_size / 2;
        let mut level = Vec::with_capacity(next * next * 4);
        for y in 0..next {
            for x in 0..next {
                for c in 0..4 {
                    let at =
                        |px: usize, py: usize| data[prev_start + (py * prev_size + px) * 4 + c];
                    let sum = at(2 * x, 2 * y) as u32
                        + at(2 * x + 1, 2 * y) as u32
                        + at(2 * x, 2 * y + 1) as u32
                        + at(2 * x + 1, 2 * y + 1) as u32;
                    level.push((sum / 4) as u8);
                }
            }
        }
        prev_start = data.len();
        prev_size = next;
        data.extend_from_slice(&level);
        levels += 1;
    }
    (data, levels)
}

/// Render assets of the line's vegetation: per catalog entry the glTF scene of
/// the mod object it names, the generated placeholder trees for everything
/// else. Every tree of one entry shares mesh and material handles, so Bevy
/// batches them into instanced draws (plan ch. 14 "streamed instances").
#[derive(Clone)]
pub struct TreeCatalog {
    /// Indexed by [`content::Tree::object`]; `None` where the name resolved to
    /// no installed mod object.
    scenes: Vec<Option<Handle<WorldAsset>>>,
    /// Placeholder conifer and broadleaf, coloured by vertex so one white
    /// material serves both.
    placeholder: [Handle<Mesh>; 2],
    placeholder_material: Handle<StandardMaterial>,
}

/// Resolves the vegetation catalog: each object name against the installed
/// mods' `objects/*.ron`, unknown names logged once and shown as placeholders.
pub fn tree_catalog(
    names: &[String],
    registry: &std::collections::BTreeMap<String, track_model::TrackObject>,
    assets: &AssetServer,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> TreeCatalog {
    let scenes = names
        .iter()
        .map(|name| match registry.get(name) {
            Some(object) => Some(
                assets
                    .load(
                        GltfAssetLabel::Scene(0)
                            .from_asset(crate::models::asset_path(&object.model)),
                    )
                    .clone(),
            ),
            None => {
                warn!("vegetation: unknown object {name:?} — placeholder shown");
                None
            }
        })
        .collect();
    let (placeholder, placeholder_material) = placeholder_trees(meshes, materials);
    TreeCatalog {
        scenes,
        placeholder,
        placeholder_material,
    }
}

/// Low-poly placeholder trees, coloured by vertex so one white material serves both kinds.
fn placeholder_trees(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> ([Handle<Mesh>; 2], Handle<StandardMaterial>) {
    let material = materials.add(StandardMaterial {
        perceptual_roughness: 0.9,
        ..default()
    });
    let trunk = |height: f32| {
        colored(
            Cylinder::new(0.18, height)
                .mesh()
                .resolution(6)
                .anchor(CylinderAnchor::Bottom)
                .build(),
            [0.23, 0.17, 0.12, 1.0],
        )
    };

    let mut conifer = trunk(1.4);
    conifer
        .merge(
            &colored(
                Cone::new(1.8, 5.6)
                    .mesh()
                    .resolution(8)
                    .anchor(ConeAnchor::Base)
                    .build(),
                [0.10, 0.22, 0.09, 1.0],
            )
            .transformed_by(Transform::from_xyz(0.0, 1.4, 0.0)),
        )
        .expect("same vertex layout");

    let mut broadleaf = trunk(2.6);
    broadleaf
        .merge(
            &colored(Sphere::new(2.3).mesh().uv(10, 7), [0.17, 0.30, 0.10, 1.0])
                .transformed_by(Transform::from_xyz(0.0, 4.2, 0.0)),
        )
        .expect("same vertex layout");

    ([meshes.add(conifer), meshes.add(broadleaf)], material)
}

/// Paints the whole mesh in one vertex color.
fn colored(mut mesh: Mesh, color: [f32; 4]) -> Mesh {
    let count = mesh.count_vertices();
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![color; count]);
    mesh
}

/// Spawns a single terrain tile from [`content::terrain`] (streaming, plan 4.3)
/// with its trees as children — they stream in and out with the tile.
pub fn spawn_terrain_tile(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: &Handle<TerrainMaterial>,
    trees: &TreeCatalog,
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
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, tile.splat.clone());
    mesh.insert_indices(Indices::U32(tile.indices.clone()));
    mesh.compute_normals();

    let frame = EnuFrame::at(tile.anchor);
    let (translation, rotation) = origin.frame_transform(&frame);
    commands
        .spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(translation).with_rotation(rotation),
            WorldAnchored {
                anchor: tile.anchor,
            },
            TerrainChunk {
                radius: tile.radius,
                lod: tile.lod,
            },
        ))
        .with_children(|parent| {
            for tree in &tile.trees {
                let transform = Transform::from_translation(Vec3::from(tree.pos))
                    .with_rotation(Quat::from_rotation_y(tree.rot))
                    .with_scale(Vec3::splat(tree.scale));
                let scene = tree
                    .object
                    .and_then(|i| trees.scenes.get(i as usize))
                    .and_then(|s| s.clone());
                match scene {
                    Some(scene) => {
                        parent.spawn((WorldAssetRoot(scene), transform));
                    }
                    None => {
                        // Placeholder: conifer or broadleaf, picked by position
                        // hash so a wood mixes without carrying a species.
                        let kind = (tree.pos[0].to_bits() ^ tree.pos[2].to_bits()) as usize & 1;
                        parent.spawn((
                            Mesh3d(trees.placeholder[kind].clone()),
                            MeshMaterial3d(trees.placeholder_material.clone()),
                            transform,
                        ));
                    }
                }
            }
        })
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
