//! How the world is drawn (plan ch. 12, 14, 15): the track with its ballast and
//! rails, terrain tiles with their splat material, the line's trees, its scenery
//! objects and its signal assemblies.
//!
//! This lives in its own crate because two programs draw the same world: the
//! simulator, and the route editor, which shows the module it is editing. Same
//! mesh, same shader, same generated ground textures, same glTF at the same
//! pose — a builder judges a wood or a signal box in the editor and gets what
//! the run shows.

use bevy::asset::io::AssetSourceBuilder;
use bevy::asset::io::file::FileAssetReader;
use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::gltf::GltfAssetLabel;
use bevy::image::ImageLoaderSettings;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{ConeAnchor, CylinderAnchor, MeshBuilder};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;
use bevy::world_serialization::WorldAsset;
use content::{LineSource, TerrainBuilder, TerrainTile};
use glam::{DQuat, DVec3};
use sim_core::interlock::{Aspect, DistantAspect, MainAspect, SignalKind, SignalModel};
use sim_core::train::lod_level;
use std::collections::BTreeMap;
use track_model::{Facing, TrackEdge, TrackNetwork, TrackObject};
use world_coords::{EcefPos, EnuFrame, RenderOrigin, geo};

/// Registers the splat shader and its material. Both programs add it after
/// `DefaultPlugins` — the embedded registry only exists once the asset plugin
/// has run.
pub struct WorldRenderPlugin;

impl Plugin for WorldRenderPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "terrain_splat.wgsl");
        app.add_plugins(MaterialPlugin::<TerrainMaterial>::default());
    }
}

/// Asset source of the mods: `mods://<mod>/assets/…`. Both programs register it
/// with [`mod_asset_source`] **before** the asset plugin.
pub const MOD_SOURCE: &str = "mods";

/// Full asset path of a model file stated relative to the `mods/` directory.
pub fn asset_path(file: &str) -> String {
    format!("{MOD_SOURCE}://{file}")
}

/// The `mods/` directory next to the program — the same one the mod runtime reads.
pub fn mod_asset_source() -> AssetSourceBuilder {
    let root = std::env::current_dir().unwrap_or_default().join("mods");
    AssetSourceBuilder::new(move || Box::new(FileAssetReader::new(root.clone())))
}

/// An object whose geometry lies in the ENU frame of a fixed world point
/// (track, scenery, terrain). On an origin rebase only the transform is set anew.
#[derive(Component)]
pub struct WorldAnchored {
    pub anchor: EcefPos,
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
        // The embedded path starts with the *crate* name.
        "embedded://world_render/terrain_splat.wgsl".into()
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

/// Mesh of one terrain tile: the builder's grid, its splat weights as vertex
/// colors, and UVs at one texture repeat per 32 m of ground.
pub fn terrain_mesh(tile: &content::TerrainTile) -> Mesh {
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
    mesh
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
    registry: &BTreeMap<String, TrackObject>,
    assets: &AssetServer,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> TreeCatalog {
    let scenes = names
        .iter()
        .map(|name| match registry.get(name) {
            Some(object) => Some(
                assets
                    .load(GltfAssetLabel::Scene(0).from_asset(asset_path(&object.model)))
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
/// with its trees as children — they stream in and out with the tile. The
/// caller adds what it needs on top (the simulator its view distance, the
/// editor its own marker).
pub fn spawn_terrain_tile(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: &Handle<TerrainMaterial>,
    trees: &TreeCatalog,
    tile: &TerrainTile,
    origin: &RenderOrigin,
) -> Entity {
    let mesh = terrain_mesh(tile);
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

/// Spawns every scenery object of the line. An unknown object kind gets a
/// placeholder block — visible in the world instead of silently absent.
/// `terrain` answers the ground height for objects that snap to the terrain.
/// The same objects in the editor and in the run — that is the point.
#[allow(clippy::too_many_arguments)]
pub fn spawn_objects(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    assets: &AssetServer,
    line: &LineSource,
    net: &TrackNetwork,
    origin: &RenderOrigin,
    registry: &BTreeMap<String, TrackObject>,
    mut terrain: Option<&mut TerrainBuilder>,
) {
    let placeholder_mesh = meshes.add(Cuboid::new(0.8, 2.0, 0.8));
    let placeholder_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.75, 0.30, 0.65),
        ..default()
    });

    for (i, placement) in line.objects.iter().enumerate() {
        // Compile refused dangling indices; a guard keeps a stale file harmless.
        let Some(edge) = net.edges().get(placement.edge as usize) else {
            continue;
        };
        let pose = edge.eval(placement.s.clamp(0.0, edge.length()));
        // Positive offset = right of increasing arc length.
        let right = pose.tangent.cross(pose.up).normalize();
        let base = EcefPos(pose.pos.0 + right * placement.lateral_offset);
        // `snap_to_terrain` needs the ground; without a terrain builder — the
        // editor's first frame — the placement keeps the rail plane.
        let ground = terrain
            .as_mut()
            .filter(|_| placement.snap_to_terrain)
            .map(|t| t.surface_height(base));
        let anchor = if let Some(ground) = ground {
            let (lat, lon, _) = geo::from_ecef(base);
            geo::to_ecef(lat, lon, ground + placement.height)
        } else {
            EcefPos(base.0 + pose.up * placement.height)
        };
        // Yaw is clockwise seen from above; 0 = front along increasing s.
        let dir = DQuat::from_axis_angle(pose.up, -placement.yaw_deg.to_radians()) * pose.tangent;

        let local = RenderOrigin::new(anchor);
        let rotation = local.look_rotation(dir, local.frame().up);
        let (translation, frame_rotation) = origin.frame_transform(local.frame());
        let root = commands
            .spawn((
                Transform::from_translation(translation).with_rotation(frame_rotation),
                Visibility::default(),
                WorldAnchored { anchor },
            ))
            .id();
        let view = commands
            .spawn((
                Transform::from_rotation(rotation),
                Visibility::default(),
                ChildOf(root),
            ))
            .id();

        match registry.get(&placement.object) {
            Some(object) => {
                let scene =
                    assets.load(GltfAssetLabel::Scene(0).from_asset(asset_path(&object.model)));
                commands.spawn((WorldAssetRoot(scene), Transform::default(), ChildOf(view)));
            }
            None => {
                warn!(
                    "object {i}: unknown object {:?} — placeholder shown",
                    placement.object
                );
                commands.spawn((
                    Mesh3d(placeholder_mesh.clone()),
                    MeshMaterial3d(placeholder_material.clone()),
                    Transform::from_xyz(0.0, 1.0, 0.0),
                    ChildOf(view),
                ));
            }
        }
    }
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
                    .load(asset_path(file))
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
                // Left, right, then along the track: that order faces upwards,
                // which is where the normals point and where the camera is
                // (pinned by a test) — the other way round the ribbon is a
                // backface and the ballast is not drawn at all.
                indices.extend_from_slice(&[a, b, c, b, d, c]);
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

/// 3D models of the line's signals, resolved at setup — index = signal index.
#[derive(Resource, Default)]
pub struct SignalModels(pub Vec<Option<SignalModel>>);

/// Root of one spawned part's glTF scene.
#[derive(Component)]
pub struct SignalPartRoot {
    pub signal: usize,
    pub part: usize,
}

/// The part still waits to be hung onto its mount node; hidden until then.
#[derive(Component)]
pub struct Unmounted {
    pub parent: u32,
    pub node: String,
}

/// The part's lamp, motion and LOD nodes have been bound.
#[derive(Component)]
pub struct LampsBound;

/// A lamp node: visible while `lamp` is in the signal's current lamp image.
#[derive(Component)]
pub struct SignalLamp {
    pub signal: usize,
    pub lamp: String,
}

/// A moving node (semaphore arm): travels towards 1 while its string is in the
/// signal's lamp image, back to 0 without it.
#[derive(Component)]
pub struct MotionNode {
    pub signal: usize,
    /// Index into `SignalModel::motions`.
    pub motion: usize,
    /// Transform as it comes out of the file — the motion is applied on top.
    pub base: Transform,
    /// Current travel 0 … 1.
    pub value: f32,
}

/// A node of one level of detail (`<name>_LOD<level>`).
#[derive(Component)]
pub struct SignalLodNode {
    pub signal: usize,
    pub level: u8,
}

/// Placeholder light whose material follows the aspect.
#[derive(Component)]
pub struct PlaceholderHead {
    pub signal: usize,
}

/// Materials of the placeholder light, one per shown colour.
#[derive(Resource)]
pub struct AspectMaterials {
    off: Handle<StandardMaterial>,
    red: Handle<StandardMaterial>,
    green: Handle<StandardMaterial>,
    yellow: Handle<StandardMaterial>,
    white: Handle<StandardMaterial>,
}

impl AspectMaterials {
    /// Material of an aspect — the editor sets it once, the simulator follows
    /// the interlocking with it.
    pub fn of(&self, aspect: &Aspect) -> Handle<StandardMaterial> {
        self.handle(aspect)
    }

    pub fn new(materials: &mut Assets<StandardMaterial>) -> Self {
        let lamp = |materials: &mut Assets<StandardMaterial>, colour: Color, lit: bool| {
            materials.add(StandardMaterial {
                base_color: colour,
                emissive: if lit {
                    colour.to_linear() * 4.0
                } else {
                    LinearRgba::BLACK
                },
                perceptual_roughness: 0.4,
                ..default()
            })
        };
        Self {
            off: lamp(materials, Color::srgb(0.12, 0.12, 0.12), false),
            red: lamp(materials, Color::srgb(0.9, 0.1, 0.1), true),
            green: lamp(materials, Color::srgb(0.1, 0.85, 0.3), true),
            yellow: lamp(materials, Color::srgb(0.95, 0.75, 0.1), true),
            white: lamp(materials, Color::srgb(0.95, 0.95, 0.9), true),
        }
    }

    /// Placeholder colour of an aspect: the main aspect first, a pure distant
    /// signal shows what it announces.
    fn handle(&self, aspect: &sim_core::interlock::Aspect) -> Handle<StandardMaterial> {
        match aspect.main {
            Some(MainAspect::Stop) => self.red.clone(),
            Some(MainAspect::Proceed) => self.green.clone(),
            Some(MainAspect::ProceedSlow) => self.yellow.clone(),
            Some(MainAspect::Substitute) => self.white.clone(),
            Some(MainAspect::DarkLight) => self.off.clone(),
            None => match aspect.distant {
                Some(DistantAspect::ExpectProceed) => self.green.clone(),
                Some(DistantAspect::ExpectStop) | Some(DistantAspect::ExpectSlow) => {
                    self.yellow.clone()
                }
                None => self.off.clone(),
            },
        }
    }
}

/// What one signal shows: where it stands, what it is, and the model it wears.
/// The simulator fills it from the interlocking, the route editor from the line
/// file — the spawning is the same either way.
pub struct SignalView<'a> {
    /// Trackside device the signal sits on.
    pub device: track_model::DeviceId,
    pub kind: SignalKind,
    /// Aspect the placeholder light shows.
    pub aspect: Aspect,
    /// Resolved 3D model; `None` gets the placeholder mast.
    pub model: Option<&'a SignalModel>,
}

/// Spawns every signal of the line: its resolved model, or the placeholder.
pub fn spawn_signals(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    assets: &AssetServer,
    net: &TrackNetwork,
    signals: &[SignalView],
    origin: &RenderOrigin,
) -> AspectMaterials {
    let aspect_materials = AspectMaterials::new(materials);
    let mast_mesh = meshes.add(Cuboid::new(0.15, 4.0, 0.15));
    let head_mesh = meshes.add(Sphere::new(0.25));
    // A track lock has no mast — it lies on the rail.
    let shoe_mesh = meshes.add(Cuboid::new(0.6, 0.25, 1.2));
    let mast_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.55, 0.56, 0.58),
        perceptual_roughness: 0.7,
        ..default()
    });

    for (i, signal) in signals.iter().enumerate() {
        let device = net.device(signal.device);
        let pose = net.edge(device.edge).eval(device.s);
        // The signal faces the trains its device applies to; `Both` picks one side.
        let dir = match device.facing {
            Facing::Backward => -pose.tangent,
            _ => pose.tangent,
        };
        // Positive offset = left of the direction of travel (device.rs).
        let left = pose.up.cross(dir).normalize();
        let anchor = EcefPos(pose.pos.0 + left * device.lateral_offset);

        // The geometry lives in the ENU frame of the signal itself: on an origin
        // rebase `resync_anchored` resets the frame transform, the local rotation
        // (mast plumb, front towards the approaching driver, +Z in the model)
        // survives as the child's transform.
        let local = RenderOrigin::new(anchor);
        let rotation = local.look_rotation(dir, local.frame().up);
        let (translation, frame_rotation) = origin.frame_transform(local.frame());

        let root = commands
            .spawn((
                Transform::from_translation(translation).with_rotation(frame_rotation),
                Visibility::default(),
                WorldAnchored { anchor },
            ))
            .id();
        let view = commands
            .spawn((
                Transform::from_rotation(rotation),
                Visibility::default(),
                ChildOf(root),
            ))
            .id();

        match signal.model {
            Some(model) => {
                for (p, part) in model.parts.iter().enumerate() {
                    let scene =
                        assets.load(GltfAssetLabel::Scene(0).from_asset(asset_path(&part.file)));
                    let mut entity = commands.spawn((
                        WorldAssetRoot(scene),
                        Transform::default(),
                        SignalPartRoot { signal: i, part: p },
                        ChildOf(view),
                    ));
                    match &part.mount {
                        // A cyclic mount chain (hand-written file) can never
                        // resolve — the part stands at the signal foot instead.
                        Some(_) if mounts_cyclically(model, p) => {
                            warn!("signal {i}: part {p} mounts in a cycle — placed at the root");
                            entity.insert(Visibility::default());
                        }
                        // Hidden until it hangs on its mount node — a mounted part
                        // must not flash at the signal foot while the parent loads.
                        Some((parent, node)) => {
                            entity.insert((
                                Visibility::Hidden,
                                Unmounted {
                                    parent: *parent,
                                    node: node.clone(),
                                },
                            ));
                        }
                        None => {
                            entity.insert(Visibility::default());
                        }
                    }
                }
            }
            // A track lock without a model: the shoe itself, in the colour of
            // its aspect (stop = laid on). A mast would be the wrong picture,
            // and the shape is what a mod replaces with its own model.
            None if signal.kind == SignalKind::TrackLock => {
                commands.spawn((
                    Mesh3d(shoe_mesh.clone()),
                    MeshMaterial3d(aspect_materials.handle(&signal.aspect)),
                    Transform::from_xyz(0.0, 0.2, 0.0),
                    PlaceholderHead { signal: i },
                    ChildOf(view),
                ));
            }
            None => {
                commands.spawn((
                    Mesh3d(mast_mesh.clone()),
                    MeshMaterial3d(mast_material.clone()),
                    Transform::from_xyz(0.0, 2.0, 0.0),
                    ChildOf(view),
                ));
                commands.spawn((
                    Mesh3d(head_mesh.clone()),
                    MeshMaterial3d(aspect_materials.handle(&signal.aspect)),
                    Transform::from_xyz(0.0, 4.3, 0.0),
                    PlaceholderHead { signal: i },
                    ChildOf(view),
                ));
            }
        }
    }

    aspect_materials
}

/// Does `part`'s mount chain loop back on itself? More hops than parts is a cycle.
fn mounts_cyclically(model: &SignalModel, part: usize) -> bool {
    let mut current = part;
    for _ in 0..model.parts.len() {
        match model.parts.get(current).and_then(|p| p.mount.as_ref()) {
            Some((next, _)) => current = *next as usize,
            None => return false,
        }
    }
    true
}

/// Hangs waiting parts onto their mount nodes once the parent part's scene exists.
pub fn mount_parts(
    mut commands: Commands,
    unmounted: Query<(Entity, &SignalPartRoot, &Unmounted)>,
    parts: Query<(Entity, &SignalPartRoot)>,
    children: Query<&Children>,
    named: Query<&Name>,
) {
    for (entity, part, mount) in unmounted.iter() {
        let Some((parent_root, _)) = parts
            .iter()
            .find(|(_, p)| p.signal == part.signal && p.part == mount.parent as usize)
        else {
            // Dangling part index — the editor validates, a hand-written file may not.
            warn!(
                "signal {}: part {} mounts on missing part {}",
                part.signal, part.part, mount.parent
            );
            commands.entity(entity).remove::<Unmounted>();
            continue;
        };
        // Walk the parent's subtree; the scene is only there a few frames after spawn.
        // Parts already mounted inside it belong to other files and are skipped.
        let mut stack = vec![parent_root];
        let mut target = None;
        let mut loaded = false;
        while let Some(e) = stack.pop() {
            if e != parent_root && parts.contains(e) {
                continue;
            }
            if let Ok(kids) = children.get(e) {
                stack.extend(kids.iter());
            }
            if let Ok(name) = named.get(e) {
                loaded = true;
                if name.as_str() == mount.node {
                    target = Some(e);
                    break;
                }
            }
        }
        match target {
            Some(node) => {
                commands
                    .entity(entity)
                    .insert((ChildOf(node), Visibility::Inherited))
                    .remove::<Unmounted>();
                info!(
                    "signal {}: part {} mounted on {:?} of part {}",
                    part.signal, part.part, mount.node, mount.parent
                );
            }
            None if loaded => {
                // The parent scene is there and the node is not in it: permanent.
                warn!(
                    "signal {}: mount node {:?} not found in part {}",
                    part.signal, mount.node, mount.parent
                );
                commands.entity(entity).remove::<Unmounted>();
            }
            None => {}
        }
    }
}

/// Binds the lamp, motion and LOD nodes of a part once its scene has been spawned.
pub fn bind_lamps(
    mut commands: Commands,
    models: Res<SignalModels>,
    roots: Query<(Entity, &SignalPartRoot), Without<LampsBound>>,
    all_parts: Query<(), With<SignalPartRoot>>,
    children: Query<&Children>,
    named: Query<(&Name, &Transform)>,
) {
    for (root, part) in roots.iter() {
        let Some(model) = models.0.get(part.signal).and_then(|m| m.as_ref()) else {
            continue;
        };
        let lamps: Vec<_> = model
            .lamps
            .iter()
            .filter(|l| l.part as usize == part.part)
            .collect();
        let motions: Vec<_> = model
            .motions
            .iter()
            .enumerate()
            .filter(|(_, m)| m.part as usize == part.part)
            .collect();
        if lamps.is_empty() && motions.is_empty() && model.lods.is_empty() {
            commands.entity(root).insert(LampsBound);
            continue;
        }
        let mut stack = vec![root];
        let mut found = false;
        let (mut lamps_bound, mut motions_bound) = (0, 0);
        while let Some(entity) = stack.pop() {
            // A part mounted inside this one binds its own lamps.
            if entity != root && all_parts.contains(entity) {
                continue;
            }
            if let Ok(kids) = children.get(entity) {
                stack.extend(kids.iter());
            }
            let Ok((name, transform)) = named.get(entity) else {
                continue;
            };
            found = true;
            // The LOD table spans all parts; nodes without the suffix are
            // every level's furniture and stay as they are.
            if !model.lods.is_empty()
                && let Some(level) = lod_level(name.as_str())
            {
                commands.entity(entity).insert((
                    SignalLodNode {
                        signal: part.signal,
                        level,
                    },
                    Visibility::Inherited,
                ));
            }
            if let Some((index, _)) = motions.iter().find(|(_, m)| m.node == name.as_str()) {
                commands.entity(entity).insert((
                    MotionNode {
                        signal: part.signal,
                        motion: *index,
                        base: *transform,
                        value: 0.0,
                    },
                    Visibility::Inherited,
                ));
                motions_bound += 1;
            }
            if let Some(binding) = lamps.iter().find(|l| l.node == name.as_str()) {
                // Dark until the first update — and a glTF node does not have to
                // carry `Visibility`, without one it could not be switched.
                commands.entity(entity).insert((
                    SignalLamp {
                        signal: part.signal,
                        lamp: binding.lamp.clone(),
                    },
                    Visibility::Hidden,
                ));
                lamps_bound += 1;
            }
        }
        if found {
            commands.entity(root).insert(LampsBound);
            info!(
                "signal {}: part {}: {lamps_bound} of {} lamp nodes, {motions_bound} of {} motion nodes bound",
                part.signal,
                part.part,
                lamps.len(),
                motions.len()
            );
        }
    }
}

/// Edge length of the generated ground textures [texels]; one repeat covers
/// 32 m of terrain (the UV scale above).
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

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::interlock::SignalPart;

    /// The ballast bed is seen from above — from the cab as much as from the
    /// editor's map. A ribbon wound the other way round is a backface and the
    /// track is simply not there.
    #[test]
    fn the_track_ribbon_faces_upwards() {
        let mut ribbon = RibbonBuilder::default();
        for i in 0..3 {
            let along = DVec3::new(i as f64 * 4.0, 0.0, 0.0);
            // left of travel = north, right = south (ENU: x east, y north).
            ribbon.push_pair(
                along + DVec3::new(0.0, 2.6, 0.0),
                along - DVec3::new(0.0, 2.6, 0.0),
            );
        }
        let mesh = ribbon.build();
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions");
        };
        let Some(bevy::mesh::Indices::U32(indices)) = mesh.indices() else {
            panic!("indices");
        };
        for triangle in indices.chunks(3) {
            let p = |i: u32| Vec3::from(positions[i as usize]);
            let (a, b, c) = (p(triangle[0]), p(triangle[1]), p(triangle[2]));
            let normal = (b - a).cross(c - a);
            assert!(normal.y > 0.0, "triangle faces down: {normal:?}");
        }
    }

    #[test]
    fn a_mount_cycle_is_detected() {
        let part = |mount| SignalPart {
            file: "a.gltf".into(),
            mount,
        };
        let chain = SignalModel {
            parts: vec![
                part(None),
                part(Some((0, "mp".into()))),
                part(Some((1, "mp".into()))),
            ],
            ..Default::default()
        };
        assert!(!mounts_cyclically(&chain, 2));
        let cycle = SignalModel {
            parts: vec![part(Some((1, "mp".into()))), part(Some((0, "mp".into())))],
            ..Default::default()
        };
        assert!(mounts_cyclically(&cycle, 0));
    }
}
