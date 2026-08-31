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
use bevy::camera::RenderTarget;
use bevy::gltf::GltfAssetLabel;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{ConeAnchor, CylinderAnchor, MeshBuilder};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;
use content::{PersonInstance, SceneryInstance, TerrainTile, Tree};
use sim_core::interlock::{Aspect, DistantAspect, MainAspect, SignalKind, SignalModel};
use sim_core::train::lod_level;
use track_model::{Facing, TrackNetwork, TrackObject};
use world_coords::{EcefPos, EnuFrame, RenderOrigin};

pub mod clouds;
pub mod conductors;
pub mod farmland;
pub mod mist;
pub mod people;
pub mod plants;
pub mod precipitation;
pub mod roads;
pub mod scatter;
pub mod sky;
pub mod track;
pub mod water;
pub mod weather;
pub mod windscreen;

pub use conductors::{ConductorMark, ConductorMaterial, ConductorMaterials, spawn_conductors};
pub use farmland::{
    CropExt, CropParams, FieldDraw, FieldMaterial, FieldMaterials, FieldSurface, spawn_fields,
};
pub use people::{
    CYCLE_PACE, CYCLE_RATE, CharacterAssets, CharacterGraphs, Dressed, GAIT_FADE, Gait,
    PASSENGER_CULL, PERSON_CULL, Passengers, PeopleClock, Person, Stroller, WALKING_ABOVE,
    WalkwayHost, WalkwaysBound, bind_walkways, gait, move_strollers, person_bundle, play_gait,
    spawn_seated, spawn_strollers,
};
pub use plants::{FieldPlants, PlantMaterials, update_field_plants};
pub use roads::{RoadDraw, RoadMaterial, RoadMaterials, RoadSurfaceMark, spawn_roads};
pub use scatter::{
    OBJECT_CULL, PendingTrees, Scattered, SceneryIndex, TREE_CULL, TreeModels, Wood, WorldCatalog,
    cull_distant_woods, materialise_trees,
};
pub use track::{GAUGE, RailMaterial, spawn_track};
pub use water::{WaterMaterial, WaterMaterials, WaterSurface, spawn_waters};

/// Registers the splat shader and its material. Both programs add it after
/// `DefaultPlugins` — the embedded registry only exists once the asset plugin
/// has run.
pub struct WorldRenderPlugin;

impl Plugin for WorldRenderPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "terrain_splat.wgsl");
        embedded_asset!(app, "fields.wgsl");
        // The standing crop's near level: real plant models (Quaternius, CC0 —
        // see tools/plants and THIRD_PARTY_LICENSES.md).
        embedded_asset!(app, "plants/wheat.glb");
        embedded_asset!(app, "plants/corn.glb");
        embedded_asset!(app, "plants/lettuce.glb");
        embedded_asset!(app, "plants/grass.glb");
        embedded_asset!(app, "plants/clover.glb");
        embedded_asset!(app, "plants/turnip.glb");
        embedded_asset!(app, "plants/flowers.glb");
        embedded_asset!(app, "plants/hay.glb");
        embedded_asset!(app, "plants/vines.glb");
        embedded_asset!(app, "plants/tree.glb");
        app.add_plugins(MaterialPlugin::<TerrainMaterial>::default())
            .add_plugins(MaterialPlugin::<farmland::FieldMaterial>::default())
            .add_plugins(water::plugin)
            .add_plugins(conductors::plugin)
            .add_plugins(roads::plugin)
            .init_resource::<farmland::FieldMaterials>()
            .init_resource::<plants::PlantMaterials>()
            .init_resource::<plants::PlantModels>()
            .init_resource::<Daylight>()
            .init_resource::<TreeModels>()
            .init_resource::<people::CharacterGraphs>()
            .init_resource::<TextureMips>()
            .init_resource::<people::PeopleClock>()
            .add_plugins((
                sky::plugin,
                clouds::plugin,
                mist::plugin,
                precipitation::plugin,
                weather::plugin,
                windscreen::plugin,
                track::plugin,
            ))
            .add_systems(
                Update,
                (
                    switch_night_nodes,
                    materialise_trees,
                    scatter::cull_distant_woods,
                    farmland::follow_date,
                    // The standing crop follows the same calendar as the
                    // paint under it: the material turns with the day, and a
                    // moved stage regrows the cards on a budget.
                    plants::follow_date,
                    plants::update_field_plants,
                    // A walker dressed this frame gets its gait the same frame.
                    (people::dress_people, people::move_strollers, mip_textures).chain(),
                    people::bind_walkways,
                    // The track's tiling textures get their sampler once loaded.
                    track::apply_tiling,
                ),
            );
    }
}

/// How light it is outside: 0 = night … 1 = full daylight. The simulator writes
/// it from the sun's elevation; the editor leaves it at day.
#[derive(Resource)]
pub struct Daylight(pub f32);

impl Default for Daylight {
    fn default() -> Self {
        Self(1.0)
    }
}

/// Suffix that marks a glTF node as night furniture: lit windows in a house,
/// a glowing sign, the pool of light under a platform lamp.
///
/// A mod needs nothing but the node name — whatever is modelled there (an
/// emissive window pane is the usual answer) is switched like a signal's lamp
/// node, and a model without such a node simply never lights up. The same
/// convention as `_LOD<level>`, and it holds for every glTF the world is drawn
/// from: scenery objects, trees, signal parts, vehicles.
pub const NIGHT_SUFFIX: &str = "_NIGHT";

/// A node found by that suffix.
#[derive(Component)]
pub struct NightNode;

/// An entity put up once at startup that outlives every run — the cloud dome, the mist
/// volume and the offscreen pass that feeds them. The simulator tears its world down
/// when the player leaves a run for the title screen (`main::tear_down_run`); this is
/// what says "not me".
#[derive(Component)]
pub struct Persistent;

/// Below this much daylight the night nodes are on — the sun at the horizon,
/// the same dusk the headlights come up in.
const NIGHT_BELOW: f32 = 0.5;

/// Tags freshly spawned night nodes and switches all of them when dusk falls.
// ponytail: a hard switch at one threshold, not a fade — the glow lives in the
// mod's own emissive material, and fading it would mean patching every loaded
// glTF material per frame.
pub fn switch_night_nodes(
    mut commands: Commands,
    daylight: Res<Daylight>,
    fresh: Query<(Entity, &Name), Added<Name>>,
    mut nodes: Query<&mut Visibility, With<NightNode>>,
    mut was_lit: Local<Option<bool>>,
) {
    let lit = daylight.0 < NIGHT_BELOW;
    let shown = |on: bool| {
        if on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        }
    };
    // A glTF node does not have to carry `Visibility`; without one it could not
    // be switched at all (the same reason as for the lamp nodes).
    for (entity, name) in &fresh {
        if name.as_str().ends_with(NIGHT_SUFFIX) {
            commands.entity(entity).try_insert((NightNode, shown(lit)));
        }
    }
    if *was_lit != Some(lit) {
        *was_lit = Some(lit);
        for mut visibility in &mut nodes {
            *visibility = shown(lit);
        }
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
///
/// The frame is kept with the anchor: it is a pure function of it, and a
/// rebase that had to derive it again for every anchored entity — two Newton
/// iterations and a dozen trigonometric calls each — was the hitch the run
/// showed every four kilometres.
#[derive(Component)]
pub struct WorldAnchored {
    pub anchor: EcefPos,
    frame: EnuFrame,
    /// Fixed offset in the frame's own axes, applied on top of the frame
    /// transform. A sleeper chunk is placed at its own centre this way — the
    /// distance cull measures to the entity's translation, and a chunk at the
    /// edge anchor would vanish with the whole edge's anchor out of range.
    offset: Vec3,
}

impl WorldAnchored {
    pub fn at(anchor: EcefPos) -> Self {
        Self {
            anchor,
            frame: EnuFrame::at(anchor),
            offset: Vec3::ZERO,
        }
    }

    /// From a frame the caller already has — a tile built its mesh in it.
    pub fn in_frame(frame: EnuFrame) -> Self {
        Self {
            anchor: EcefPos(frame.origin),
            frame,
            offset: Vec3::ZERO,
        }
    }

    /// Like [`Self::in_frame`], but the object sits at `offset` within the
    /// frame (in mesh axes) — its mesh is built around that point.
    pub fn offset_in_frame(frame: EnuFrame, offset: Vec3) -> Self {
        Self {
            anchor: EcefPos(frame.origin),
            frame,
            offset,
        }
    }

    /// Translation and rotation under the given render origin.
    pub fn transform(&self, origin: &RenderOrigin) -> (Vec3, Quat) {
        let (translation, rotation) = origin.frame_transform(&self.frame);
        (translation + rotation * self.offset, rotation)
    }
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
    /// What the weather is doing to the ground — the same uniform the objects
    /// carry, written by `weather::update` (plan 14.1).
    #[uniform(106)]
    weather: weather::WeatherParams,
}

impl MaterialExtension for TerrainSplat {
    fn fragment_shader() -> ShaderRef {
        // The embedded path starts with the *crate* name.
        "embedded://world_render/terrain_splat.wgsl".into()
    }
}

/// What the date does to ground and foliage (plan ch. 14 "seasons v2").
///
/// The season falls out of the scenario's start date — the same date the sun
/// and moon are computed from (`world_coords::sun`). Both programs build their
/// ground textures and placeholder trees through it, so the editor shows the
/// module in the season it was set up for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Season {
    /// Snow cover: 0 = bare ground … 1 = deep winter.
    pub snow: f32,
    /// Autumn colour of the foliage: 0 = green … 1 = fully turned.
    pub autumn: f32,
}

impl Default for Season {
    /// Midsummer, like `sim_core::scenario::StartTime::default`.
    fn default() -> Self {
        Self::on(6, 21)
    }
}

impl Season {
    /// The season of a calendar date.
    // ponytail: two cosines over the day of the year instead of a climate
    // table — this is a central European lowland year, so a line in the Alps
    // or south of the equator gets the wrong month. A per-line climate entry
    // fixes that, not a finer curve here.
    pub fn on(month: u32, day: u32) -> Self {
        let day_of_year = (month.clamp(1, 12) - 1) as f32 * 30.44 + day as f32;
        let wave = |peak: f32| (std::f32::consts::TAU * (day_of_year - peak) / 365.25).cos();
        Self {
            // Snow from November to March, deepest around 20 January.
            snow: ((wave(20.0) - 0.35) / 0.5).clamp(0.0, 1.0),
            // The leaves turn through October.
            autumn: ((wave(288.0) - 0.8) / 0.15).clamp(0.0, 1.0),
        }
    }

    /// Anything green as the date paints it: turned in autumn, under snow in winter.
    pub fn green(&self, color: [f32; 3]) -> [f32; 3] {
        self.snowed(mix(color, STRAW, self.autumn), 1.0)
    }

    /// Bare ground under snow. `cover` says how much of it the snow holds —
    /// a rock face keeps showing through, gravel between the sleepers less so.
    pub fn snowed(&self, color: [f32; 3], cover: f32) -> [f32; 3] {
        mix(color, SNOW, self.snow * cover)
    }

    /// The model file an object shows in this season: the mod's winter or
    /// autumn variant where it ships one, otherwise the year-round model.
    /// Seasonal variants are optional — a mast needs none, a birch may bring
    /// three, and neither the line nor the editor has to know which.
    // ponytail: a variant either shows or it does not, at half a season —
    // cross-fading two glTFs means drawing every tree twice.
    pub fn model_of<'a>(&self, object: &'a TrackObject) -> &'a str {
        let variant = if self.snow > 0.5 {
            object.winter_model.as_deref()
        } else if self.autumn > 0.5 {
            object.autumn_model.as_deref()
        } else {
            None
        };
        variant.unwrap_or(&object.model)
    }
}

/// Fresh snow, and the straw the meadows turn to in autumn.
const SNOW: [f32; 3] = [0.86, 0.88, 0.93];
const STRAW: [f32; 3] = [0.45, 0.40, 0.19];

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

fn opaque(color: [f32; 3]) -> [f32; 4] {
    [color[0], color[1], color[2], 1.0]
}

/// The one terrain material, its ground textures generated at startup in the
/// colours of `season` — like the sound sources (ch. 13), the repository
/// carries no binary assets.
// ponytail: procedural noise textures instead of authored ones — photographed
// ground goes into a mod once terrain texturing is moddable content. The
// season is baked in at load: a run that drives from October into November
// keeps the ground it started on.
pub fn terrain_material(
    images: &mut Assets<Image>,
    materials: &mut Assets<TerrainMaterial>,
    season: Season,
    ground: GroundQuality,
) -> Handle<TerrainMaterial> {
    let [grass, rock, gravel] = ground_textures(season, ground);
    materials.add(TerrainMaterial {
        base: StandardMaterial {
            perceptual_roughness: 0.95,
            ..default()
        },
        extension: TerrainSplat {
            weather: weather::WeatherParams::default(),
            grass: images.add(grass),
            rock: images.add(rock),
            gravel: images.add(gravel),
        },
    })
}

/// How big the generated ground textures are and how far the sampler follows them into
/// the distance — a graphics setting of the simulator's, `(256, 4)` where nothing says
/// otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroundQuality {
    /// Edge length \[texels\].
    pub size: u32,
    /// Anisotropic samples, 1 … 16.
    pub anisotropy: u16,
}

impl Default for GroundQuality {
    fn default() -> Self {
        Self {
            size: GROUND_TEXTURE_SIZE,
            anisotropy: 4,
        }
    }
}

/// Grass, rock and gravel, in that order.
fn ground_textures(season: Season, ground: GroundQuality) -> [Image; 3] {
    [
        ground_texture(
            season.green([0.20, 0.32, 0.11]),
            season.green([0.41, 0.45, 0.18]),
            64,
            1,
            ground,
        ),
        ground_texture(
            season.snowed([0.35, 0.33, 0.31], 0.45),
            season.snowed([0.55, 0.53, 0.50], 0.45),
            48,
            2,
            ground,
        ),
        ground_texture(
            season.snowed([0.39, 0.35, 0.29], 0.7),
            season.snowed([0.57, 0.54, 0.49], 0.7),
            12,
            3,
            ground,
        ),
    ]
}

/// Generates the ground textures again and writes them into the handles the material
/// already holds, so the setting reaches terrain that is standing on screen rather than
/// only terrain built after it. `season` is the one the run was built with — the ground
/// keeps the month it started in, quality or no quality.
pub fn retexture_ground(
    images: &mut Assets<Image>,
    materials: &Assets<TerrainMaterial>,
    season: Season,
    ground: GroundQuality,
) {
    for (_, material) in materials.iter() {
        let splat = &material.extension;
        let made = ground_textures(season, ground);
        for (handle, image) in [&splat.grass, &splat.rock, &splat.gravel]
            .into_iter()
            .zip(made)
        {
            // The only way this fails is a handle whose asset has gone, and a material
            // that lost its texture is not something this can put right.
            let _ = images.insert(handle.id(), image);
        }
    }
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

/// Low-poly placeholder trees, coloured by vertex so one white material serves
/// both kinds — and by the season, so a winter run drives through snowy woods.
// ponytail: the broadleaf keeps its crown all year; bare winter branches are a
// second mesh, not a colour.
pub(crate) fn placeholder_trees(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    season: Season,
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
                // A conifer does not turn; it only carries the snow.
                opaque(season.snowed([0.10, 0.22, 0.09], 0.6)),
            )
            .transformed_by(Transform::from_xyz(0.0, 1.4, 0.0)),
        )
        .expect("same vertex layout");

    let mut broadleaf = trunk(2.6);
    broadleaf
        .merge(
            &colored(
                Sphere::new(2.3).mesh().uv(10, 7),
                opaque(season.green([0.17, 0.30, 0.10])),
            )
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
/// with its trees, scenery objects, people and walkers as children — they
/// stream in and out with the tile. The caller adds what it needs on top (the
/// simulator its view distance, the editor its own marker).
pub fn spawn_terrain_tile(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: &Handle<TerrainMaterial>,
    catalog: &WorldCatalog,
    tile: &TerrainTile,
    origin: &RenderOrigin,
) -> Entity {
    let mesh = terrain_mesh(tile);
    let anchored = WorldAnchored::at(tile.anchor);
    let (translation, rotation) = anchored.transform(origin);
    let mut entity = commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(material.clone()),
        Transform::from_translation(translation).with_rotation(rotation),
        anchored,
    ));
    scatter::spawn_scatter(
        &mut entity,
        tile.trees.clone(),
        &tile.objects,
        &tile.people,
        &tile.walkways,
        catalog,
    );
    entity.id()
}

/// Places the trees, objects and people of a standing tile anew — the editor
/// moved one, and the ground under them is the same. `old` is what the tile
/// carried so far (its [`Scattered`] children); the new set is spawned the
/// way [`spawn_terrain_tile`] did it, less the walkers: the editor, which is
/// who does this, shows no crowd and builds no ways.
pub fn respawn_scatter(
    commands: &mut Commands,
    tile: Entity,
    old: impl IntoIterator<Item = Entity>,
    trees: Vec<Tree>,
    objects: &[SceneryInstance],
    people: &[PersonInstance],
    catalog: &WorldCatalog,
) {
    if commands.get_entity(tile).is_err() {
        return;
    }
    // The wood carries the pending state and is one of the `Scattered` children,
    // so despawning them takes it with them.
    for child in old {
        commands.entity(child).try_despawn();
    }
    let Ok(mut entity) = commands.get_entity(tile) else {
        return;
    };
    scatter::spawn_scatter(&mut entity, trees, objects, people, &[], catalog);
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
                WorldAnchored::in_frame(*local.frame()),
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
            commands.entity(entity).try_remove::<Unmounted>();
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
                commands.entity(entity).try_remove::<Unmounted>();
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
            commands.entity(root).try_insert(LampsBound);
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
                commands.entity(entity).try_insert((
                    SignalLodNode {
                        signal: part.signal,
                        level,
                    },
                    Visibility::Inherited,
                ));
            }
            if let Some((index, _)) = motions.iter().find(|(_, m)| m.node == name.as_str()) {
                commands.entity(entity).try_insert((
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
                commands.entity(entity).try_insert((
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
            commands.entity(root).try_insert(LampsBound);
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

/// Edge length of the generated ground textures [texels] at `Quality::Medium`; one
/// repeat covers 32 m of terrain (the UV scale above).
const GROUND_TEXTURE_SIZE: u32 = 256;

/// One tileable ground texture: two octaves of value noise mix `base` towards
/// `accent`, `cell` sets the patch size in texels of the default size, so a patch stays
/// the same size on the ground when the texture is generated bigger or smaller.
fn ground_texture(
    base: [f32; 3],
    accent: [f32; 3],
    cell: u32,
    seed: u64,
    ground: GroundQuality,
) -> Image {
    let size = ground.size.max(16);
    let cell = (cell * size / GROUND_TEXTURE_SIZE).max(1);
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
    let (data, mip_level_count) = with_mipmaps(data, size, size, None);

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
        anisotropy_clamp: ground.anisotropy,
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

/// Anisotropic samples on a generated atlas. They are looked at obliquely — a
/// coat seen from the platform, a trunk seen from a passing cab — and 4 is
/// where the cost stops paying for itself at these texture sizes.
const TEXTURE_ANISOTROPY: u16 = 4;
/// Frames a texture is waited for before it is given up on — an asset whose
/// atlas never arrives is a broken file, not a slow disk.
const TEXTURE_PATIENCE: u32 = 600;

/// The generated atlases waiting for their mip chain, and the ones that have
/// it. The characters and the trees share it: both are built by a script that
/// writes plain PNG, which the loader takes as a single level, and both are
/// looked at from far enough away that one texel per pixel is a shimmer with
/// every step.
///
/// A material is queued, not an image: the trees' materials are labelled
/// sub-assets of a glTF and are not there the frame the model is read, so the
/// queue waits for the material first and for its textures second.
#[derive(Resource, Default)]
pub struct TextureMips {
    done: std::collections::HashSet<AssetId<Image>>,
    materials: Vec<PendingMaterial>,
    pending: Vec<PendingTexture>,
}

struct PendingMaterial {
    material: Handle<StandardMaterial>,
    /// Foliage: switch it from a hard mask to alpha-to-coverage once it is
    /// there. A leaf's edge is then anti-aliased against the sky instead of
    /// stepping from texel to texel, which is what a canopy is mostly made of.
    cutout: bool,
    frames: u32,
}

struct PendingTexture {
    image: Handle<Image>,
    /// The material that samples it, touched once the chain is built so its
    /// bind group is made anew with the new texture.
    material: Handle<StandardMaterial>,
    /// The alpha cutoff of a masked material — the mip chain needs it to keep
    /// the cut-out's coverage (see [`with_mipmaps`]).
    cutoff: Option<f32>,
    frames: u32,
}

impl TextureMips {
    /// Queues every texture of a material, once the material itself is there.
    pub fn enqueue(&mut self, material: &Handle<StandardMaterial>) {
        self.queue(material, false);
    }

    /// The same for a cut-out material — foliage, hair, a chain-link fence.
    /// Its mip chain keeps the coverage of its alpha, and its edges are
    /// resolved by the sample mask rather than by a hard test.
    pub fn enqueue_cutout(&mut self, material: &Handle<StandardMaterial>) {
        self.queue(material, true);
    }

    fn queue(&mut self, material: &Handle<StandardMaterial>, cutout: bool) {
        if self
            .materials
            .iter()
            .any(|p| p.material.id() == material.id())
        {
            return;
        }
        self.materials.push(PendingMaterial {
            material: material.clone(),
            cutout,
            frames: 0,
        });
    }

    fn enqueue_image(
        &mut self,
        image: &Handle<Image>,
        material: &Handle<StandardMaterial>,
        cutoff: Option<f32>,
    ) {
        if self.done.contains(&image.id())
            || self.pending.iter().any(|p| p.image.id() == image.id())
        {
            return;
        }
        self.pending.push(PendingTexture {
            image: image.clone(),
            material: material.clone(),
            cutoff,
            frames: 0,
        });
    }
}

/// Builds the mip chain of every queued atlas that has arrived. Cheap when
/// nothing is waiting, which is every frame but the ones after a tile or a
/// platform brought a model nobody had loaded yet.
pub fn mip_textures(
    mut textures: ResMut<TextureMips>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !textures.materials.is_empty() {
        for mut entry in std::mem::take(&mut textures.materials) {
            match materials.get(&entry.material) {
                Some(material) => {
                    let cutoff = match material.alpha_mode {
                        AlphaMode::Mask(cutoff) => Some(cutoff),
                        // Alpha-to-coverage uses the same 0.5 the mask does.
                        AlphaMode::AlphaToCoverage => Some(0.5),
                        _ => None,
                    };
                    let queued: Vec<Handle<Image>> = textures_of(material).cloned().collect();
                    for image in &queued {
                        textures.enqueue_image(image, &entry.material, cutoff);
                    }
                    if entry.cutout
                        && matches!(material.alpha_mode, AlphaMode::Mask(_))
                        && let Some(mut material) = materials.get_mut(&entry.material)
                    {
                        material.alpha_mode = AlphaMode::AlphaToCoverage;
                    }
                }
                None => {
                    entry.frames += 1;
                    if entry.frames < TEXTURE_PATIENCE {
                        textures.materials.push(entry);
                    } else {
                        debug!("material {:?} never arrived", entry.material.path());
                    }
                }
            }
        }
    }
    if textures.pending.is_empty() {
        return;
    }
    for mut entry in std::mem::take(&mut textures.pending) {
        let Some(mut image) = images.get_mut(&entry.image) else {
            entry.frames += 1;
            if entry.frames < TEXTURE_PATIENCE {
                textures.pending.push(entry);
            } else {
                debug!("texture {:?} never arrived", entry.image.path());
            }
            continue;
        };
        if build_mip_chain(&mut image, entry.cutoff) {
            // The material's bind group holds the old texture view; a touch
            // has it prepared anew with the one that carries the chain.
            materials.get_mut(&entry.material);
        }
        textures.done.insert(entry.image.id());
    }
}

/// Whether a camera is the one that draws the *world* into the window.
///
/// Not simply the first active camera: the simulator's cab displays and its
/// cloud panorama are `Camera2d`s of their own, and the route editor's
/// catalogue thumbnails are `Camera3d`s parked at the origin — all active,
/// all drawing into an image of their own. Anything that measures a distance
/// to one of those grows its geometry around the origin of the line instead
/// of around the player, which is what the standing crop did for its first
/// week and what wood culling would have done in the editor.
///
/// `Camera` requires `RenderTarget`, so a camera without one of its own is
/// already the primary window's.
pub fn draws_the_world(camera: &Camera, target: &RenderTarget) -> bool {
    camera.is_active && matches!(target, RenderTarget::Window(_))
}

/// Appends the mip chain to a plain RGBA8 image and sets a sampler that uses
/// it. `false` where the image is not one to touch: compressed, layered, or
/// already carrying a chain.
pub(crate) fn build_mip_chain(image: &mut Image, cutout: Option<f32>) -> bool {
    let descriptor = &image.texture_descriptor;
    let plain = descriptor.mip_level_count <= 1
        && descriptor.dimension == TextureDimension::D2
        && descriptor.size.depth_or_array_layers == 1
        && matches!(
            descriptor.format,
            TextureFormat::Rgba8UnormSrgb | TextureFormat::Rgba8Unorm
        );
    if !plain {
        return false;
    }
    let (width, height) = (descriptor.size.width, descriptor.size.height);
    let Some(data) = image.data.take() else {
        return false;
    };
    if data.len() != (width * height * 4) as usize {
        image.data = Some(data);
        return false;
    }
    let (data, levels) = with_mipmaps(data, width, height, cutout);
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = levels;
    if let Some(view) = image.texture_view_descriptor.as_mut() {
        view.mip_level_count = None;
    }
    // The glTF's own wrap modes are kept; the filtering is what changes.
    let (address_u, address_v) = match &image.sampler {
        ImageSampler::Descriptor(sampler) => (sampler.address_mode_u, sampler.address_mode_v),
        _ => (ImageAddressMode::Repeat, ImageAddressMode::Repeat),
    };
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: address_u,
        address_mode_v: address_v,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: TEXTURE_ANISOTROPY,
        ..default()
    });
    true
}

/// Every texture a material samples.
fn textures_of(material: &StandardMaterial) -> impl Iterator<Item = &Handle<Image>> {
    [
        &material.base_color_texture,
        &material.emissive_texture,
        &material.metallic_roughness_texture,
        &material.normal_map_texture,
        &material.occlusion_texture,
    ]
    .into_iter()
    .flatten()
}

/// Appends a box-filtered mip chain to a plain RGBA8 image of `width` ×
/// `height` texels — generated images have no loader to build one, the
/// characters' atlases ship without one, and without it a texture shimmers at
/// a distance. Levels halve each side down to 1 × 1 (a side that reaches 1
/// first stays there, as the GPU expects); an odd side folds its last texel
/// in twice rather than dropping it. Returns the data with the chain appended
/// and the level count.
///
/// `cutout` is the alpha cutoff of a masked material, and it changes the alpha
/// channel: **averaging alpha destroys a cut-out.** A leaf card is mostly
/// transparent, so every halving pulls its average alpha down, and by the
/// fourth or fifth level almost no texel still reaches the cutoff — the canopy
/// of a wood thins out and finally evaporates with distance while the opaque
/// trunks stay, which is what a forest at half a kilometre looked like before
/// this. Each level's alpha is therefore rescaled so that the share of texels
/// passing the cutoff stays what it was at full size (Castaño's method: bisect
/// a factor until the coverage matches). Pass `None` for an opaque image.
pub(crate) fn with_mipmaps(
    mut data: Vec<u8>,
    width: u32,
    height: u32,
    cutout: Option<f32>,
) -> (Vec<u8>, u32) {
    let target = cutout.map(|cutoff| (coverage(&data, cutoff, 1.0), cutoff));
    let mut levels = 1;
    let (mut prev_w, mut prev_h) = (width.max(1) as usize, height.max(1) as usize);
    // Every level is filtered from the level above it **as the filter made it**,
    // not as the rescale left it. Filtering the boosted alpha and boosting the
    // result again compounds: five levels of a fifth more each turn every texel
    // opaque, and the tree's billboard stops being a tree and becomes the
    // rectangle it is drawn on.
    let mut previous = data[..prev_w * prev_h * 4].to_vec();
    while prev_w > 1 || prev_h > 1 {
        let (next_w, next_h) = ((prev_w / 2).max(1), (prev_h / 2).max(1));
        let mut level = Vec::with_capacity(next_w * next_h * 4);
        for y in 0..next_h {
            let (y0, y1) = ((2 * y).min(prev_h - 1), (2 * y + 1).min(prev_h - 1));
            for x in 0..next_w {
                let (x0, x1) = ((2 * x).min(prev_w - 1), (2 * x + 1).min(prev_w - 1));
                for c in 0..4 {
                    let at = |px: usize, py: usize| u32::from(previous[(py * prev_w + px) * 4 + c]);
                    let sum = at(x0, y0) + at(x1, y0) + at(x0, y1) + at(x1, y1);
                    level.push((sum / 4) as u8);
                }
            }
        }
        previous = level.clone();
        if let Some((target, cutoff)) = target {
            rescale_alpha(&mut level, cutoff, target);
        }
        (prev_w, prev_h) = (next_w, next_h);
        data.extend_from_slice(&level);
        levels += 1;
    }
    (data, levels)
}

/// The share of texels that still pass the cutoff once their alpha has been
/// scaled — measured on the rounded byte the level will actually hold, not on
/// the exact product, because a factor that does not move the byte moves
/// nothing.
fn coverage(level: &[u8], cutoff: f32, scale: f32) -> f32 {
    if level.is_empty() {
        return 0.0;
    }
    let limit = (cutoff * 255.0).round();
    let (texels, _) = level.as_chunks::<4>();
    let passing = texels
        .iter()
        .filter(|texel| (f32::from(texel[3]) * scale).min(255.0).round() >= limit)
        .count();
    passing as f32 / texels.len().max(1) as f32
}

/// Scales a level's alpha so its coverage at `cutoff` comes back to `target`.
///
/// Coverage rises monotonically with the factor, so a bisection finds the
/// smallest one that reaches the target — but only after checking whether the
/// level drifted at all. A cut-out whose alpha is flatly 0 or 255 keeps its
/// coverage under *any* factor, and a bisection over a constant would run off
/// to whichever end it started from and wipe the image out.
///
/// A level that has blurred into one flat alpha cannot have a coverage between
/// nothing and everything, and the bisection then lands on everything. The
/// result is checked against simply leaving the level alone, and the nearer of
/// the two wins — a canopy turned into a solid rectangle is further from the
/// truth than one left thin.
fn rescale_alpha(level: &mut [u8], cutoff: f32, target: f32) {
    /// Coverage this close to the target counts as unchanged.
    const TOLERANCE: f32 = 0.02;
    /// Alpha is only ever brightened this far. One halving costs a cut-out
    /// perhaps a fifth of its coverage, so anything past a factor of three is a
    /// level that cannot hold the coverage at all — and forcing it there turns
    /// the sheet opaque.
    const MAX_SCALE: f32 = 3.0;

    if target <= 0.0 || level.is_empty() {
        return;
    }
    let have = coverage(level, cutoff, 1.0);
    if (have - target).abs() <= TOLERANCE {
        return;
    }
    let (mut lo, mut hi) = if have < target {
        (1.0, MAX_SCALE)
    } else {
        (0.0, 1.0)
    };
    // Where even the largest factor does not reach the target there is nothing
    // to bisect for: take it and leave the level as sparse as it turned out.
    if coverage(level, cutoff, hi) >= target {
        for _ in 0..14 {
            let mid = 0.5 * (lo + hi);
            if coverage(level, cutoff, mid) < target {
                lo = mid;
            } else {
                hi = mid;
            }
        }
    }
    // Only if it actually helps. A level that has blurred into one flat alpha —
    // the last few of a chain, where a texel covers a whole leaf — cannot hold a
    // coverage between nothing and everything, and the bisection then lands on
    // everything. Turning a canopy into a solid rectangle is further from the
    // truth than leaving it thin, so that scale is thrown away.
    let scale = hi;
    if (coverage(level, cutoff, scale) - target).abs() > (have - target).abs() {
        return;
    }
    for texel in level.as_chunks_mut::<4>().0 {
        texel[3] = (f32::from(texel[3]) * scale).min(255.0).round() as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::interlock::SignalPart;

    /// An offset anchored object keeps its offset across an origin rebase —
    /// the sleeper chunks hang at their own centre mid-edge, and the rebase's
    /// `resync_anchored` must not pull them back to the edge anchor.
    #[test]
    fn an_offset_survives_an_origin_rebase() {
        use glam::DVec3;
        let anchor = world_coords::geo::to_ecef_deg(52.0, 10.0, 100.0);
        let frame = EnuFrame::at(anchor);
        let offset = Vec3::new(0.0, 0.0, -38.1);
        let anchored = WorldAnchored::offset_in_frame(frame, offset);

        // At the frame's own anchor the offset is all there is.
        let origin = RenderOrigin::new(anchor);
        let (t, r) = anchored.transform(&origin);
        assert!((t - r * offset).length() < 1e-4, "offset lost: {t}");

        // Under a rebased origin the entity travels with its edge — and the
        // offset is applied anew in the frame's axes.
        let far = frame.to_ecef(DVec3::new(3_000.0, 500.0, 0.0));
        let (t2, _) = anchored.transform(&RenderOrigin::new(far));
        let (t_edge, _) = RenderOrigin::new(far).frame_transform(&frame);
        assert!(
            (t2 - (t_edge + r * offset)).length() < 0.1,
            "offset not reapplied after the rebase: {t2} vs {t_edge} + {}",
            r * offset
        );
    }

    /// A plain image gets its chain and a mipmapped sampler; one that has a
    /// chain, or is not RGBA8, is left alone.
    #[test]
    fn a_mip_chain_is_built_once_for_rgba8() {
        use bevy::asset::RenderAssetUsages;
        use bevy::render::render_resource::Extent3d;
        let mut image = Image::new(
            Extent3d {
                width: 4,
                height: 2,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            vec![200; 4 * 2 * 4],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        assert!(build_mip_chain(&mut image, None));
        assert_eq!(image.texture_descriptor.mip_level_count, 3);
        assert_eq!(image.data.as_ref().map(Vec::len), Some((8 + 2 + 1) * 4));
        match &image.sampler {
            ImageSampler::Descriptor(sampler) => {
                assert_eq!(sampler.anisotropy_clamp, TEXTURE_ANISOTROPY);
                assert_eq!(sampler.mipmap_filter, ImageFilterMode::Linear);
            }
            other => panic!("{other:?}"),
        }
        assert!(!build_mip_chain(&mut image, None), "not twice");
        let mut float = Image::new(
            Extent3d {
                width: 2,
                height: 2,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            vec![0; 2 * 2 * 8],
            TextureFormat::Rgba16Float,
            RenderAssetUsages::default(),
        );
        assert!(!build_mip_chain(&mut float, None));
        assert_eq!(float.texture_descriptor.mip_level_count, 1);
    }

    /// A cut-out keeps its coverage down the chain — a canopy of scattered
    /// leaves, which is what a foliage card is. Box-filtering alpha alone pulls
    /// the average under the cutoff level by level, and the wood then thins out
    /// with distance until only the opaque trunks are left. Rescaling holds the
    /// share of covered texels where it started.
    ///
    /// It must not run the other way either: a chain that boosts alpha it has
    /// already boosted turns opaque within a few levels, and the tree's
    /// billboard stops being a tree and becomes the rectangle it is drawn on.
    #[test]
    fn a_cut_out_keeps_its_coverage_down_the_chain() {
        const SIZE: usize = 128;
        let mut data = vec![0u8; SIZE * SIZE * 4];
        // Leaf-sized blobs on a lattice, jittered — about half the sheet.
        for row in 0..8 {
            for column in 0..8 {
                let cx = column as f32 * 16.0 + 8.0 + ((row * 5) % 7) as f32 - 3.0;
                let cy = row as f32 * 16.0 + 8.0 + ((column * 3) % 5) as f32 - 2.0;
                for y in 0..SIZE {
                    for x in 0..SIZE {
                        let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
                        if d > 6.0 {
                            continue;
                        }
                        let i = (y * SIZE + x) * 4;
                        data[i] = 80;
                        data[i + 1] = 160;
                        data[i + 2] = 60;
                        // A soft edge, as a photographed leaf has.
                        data[i + 3] = (255.0 * (1.0 - (d / 6.0).powi(3))).clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }
        let start = coverage(&data[..SIZE * SIZE * 4], 0.5, 1.0);
        assert!(
            (0.15..0.6).contains(&start),
            "the fixture covers a canopy's worth of the sheet ({start})"
        );

        let plain = with_mipmaps(data.clone(), SIZE as u32, SIZE as u32, None);
        let kept = with_mipmaps(data, SIZE as u32, SIZE as u32, Some(0.5));
        assert_eq!(plain.1, kept.1, "the same number of levels either way");

        let mut offset = SIZE * SIZE * 4;
        let mut side = SIZE / 2;
        let mut checked = 0;
        let mut thinned = false;
        while side >= 8 {
            let len = side * side * 4;
            let share = |image: &Vec<u8>| coverage(&image[offset..offset + len], 0.5, 1.0);
            let (before, after) = (share(&plain.0), share(&kept.0));
            thinned |= before < start * 0.8;
            assert!(
                after >= before,
                "level {side}: {after} is no worse than {before}"
            );
            // Nearer the original coverage than the plain chain, and never
            // filled up past it — a level that turns solid is a billboard that
            // reads as the rectangle it is drawn on.
            assert!(
                (after - start).abs() <= (before - start).abs(),
                "level {side}: {after} is nearer {start} than {before}"
            );
            assert!(
                after <= start + 0.12,
                "level {side} did not fill up ({after})"
            );
            offset += len;
            side /= 2;
            checked += 1;
        }
        assert!(checked >= 3, "several levels were compared");
        assert!(
            thinned,
            "the plain chain does thin out — otherwise this proves nothing"
        );

        // An image whose coverage the filter did not touch is left alone;
        // rescaling a constant would run the factor off to one end.
        let opaque = vec![255u8; 16 * 16 * 4];
        let (chain, _) = with_mipmaps(opaque, 16, 16, Some(0.5));
        assert!(chain.iter().all(|&v| v == 255), "nothing was scaled");
    }

    /// A 4 × 2 image halves to 2 × 1 and then 1 × 1 — three levels, the short
    /// side held at one; the chain is the box average of the level before.
    #[test]
    fn mip_chains_cover_non_square_images() {
        let mut data = Vec::new();
        for v in [0u8, 40, 80, 120, 160, 200, 240, 255] {
            data.extend_from_slice(&[v, v, v, 255]);
        }
        let (chain, levels) = with_mipmaps(data, 4, 2, None);
        assert_eq!(levels, 3);
        assert_eq!(chain.len(), (8 + 2 + 1) * 4);
        // Level 1, texel 0: the average of texels (0,0), (1,0), (0,1), (1,1).
        // The average of 0, 40, 160 and 200.
        assert_eq!(chain[8 * 4], 100);
        assert_eq!(chain[8 * 4 + 3], 255);
        // A square image keeps the old count: 256 → 9 levels.
        let (_, levels) = with_mipmaps(vec![0; 256 * 256 * 4], 256, 256, None);
        assert_eq!(levels, 9);
        // A single texel is its own chain.
        assert_eq!(with_mipmaps(vec![1, 2, 3, 4], 1, 1, None).1, 1);
    }

    /// The season is what the date makes of the ground and the leaves —
    /// green in summer, turned in October, white in January.
    #[test]
    fn the_season_follows_the_calendar() {
        let green = [0.20, 0.32, 0.11];

        let summer = Season::on(6, 21);
        assert_eq!((summer.snow, summer.autumn), (0.0, 0.0));
        assert_eq!(summer.green(green), green);

        let october = Season::on(10, 15);
        assert!(october.autumn > 0.5, "autumn {}", october.autumn);
        assert_eq!(october.snow, 0.0);
        // Turned: the meadow yellows — red gains more than green does.
        let turned = october.green(green);
        assert!(
            turned[0] - green[0] > turned[1] - green[1],
            "not yellowing: {turned:?}"
        );

        let january = Season::on(1, 20);
        assert_eq!(january.snow, 1.0);
        assert_eq!(january.green(green), SNOW);
        // Rock only holds part of it and keeps showing through.
        assert!(january.snowed(green, 0.45)[2] < SNOW[2]);

        assert_eq!(Season::default(), summer);
    }

    /// A mod may ship seasonal models, and may just as well not: whatever it
    /// leaves out falls back to the year-round one.
    #[test]
    fn seasonal_models_are_optional() {
        let birch = TrackObject {
            name: "Birke".into(),
            model: "x/birke.gltf".into(),
            autumn_model: Some("x/birke_herbst.gltf".into()),
            winter_model: Some("x/birke_winter.gltf".into()),
            ..plain("x/mast.gltf")
        };
        assert_eq!(Season::on(6, 21).model_of(&birch), "x/birke.gltf");
        assert_eq!(Season::on(10, 15).model_of(&birch), "x/birke_herbst.gltf");
        assert_eq!(Season::on(1, 20).model_of(&birch), "x/birke_winter.gltf");

        // A spruce with a winter model only keeps its own look in autumn.
        let spruce = TrackObject {
            winter_model: Some("x/fichte_winter.gltf".into()),
            ..plain("x/fichte.gltf")
        };
        assert_eq!(Season::on(10, 15).model_of(&spruce), "x/fichte.gltf");
        assert_eq!(Season::on(1, 20).model_of(&spruce), "x/fichte_winter.gltf");

        // A mast ships none and stands the same all year.
        let mast = plain("x/mast.gltf");
        for date in [(6, 21), (10, 15), (1, 20)] {
            assert_eq!(Season::on(date.0, date.1).model_of(&mast), "x/mast.gltf");
        }
    }

    /// Lit windows: a node named for the night is hidden by day, shown after
    /// dusk — and a node without the suffix is never touched.
    #[test]
    fn night_nodes_switch_at_dusk() {
        let mut app = App::new();
        app.init_resource::<Daylight>()
            .add_systems(Update, switch_night_nodes);
        let windows = app
            .world_mut()
            .spawn(Name::new(format!("fenster{NIGHT_SUFFIX}")))
            .id();
        let walls = app.world_mut().spawn(Name::new("mauer")).id();

        // Daylight: the windows are dark, the walls stay as they were modelled.
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(windows),
            Some(&Visibility::Hidden)
        );
        assert_eq!(app.world().get::<Visibility>(walls), None);

        // Dusk: they light up.
        app.world_mut().resource_mut::<Daylight>().0 = 0.2;
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(windows),
            Some(&Visibility::Inherited)
        );

        // A house that is built after dark is lit from its first frame.
        let late = app
            .world_mut()
            .spawn(Name::new(format!("laterne{NIGHT_SUFFIX}")))
            .id();
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(late),
            Some(&Visibility::Inherited)
        );

        // And back off at sunrise.
        app.world_mut().resource_mut::<Daylight>().0 = 1.0;
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(windows),
            Some(&Visibility::Hidden)
        );
    }

    fn plain(model: &str) -> TrackObject {
        TrackObject {
            name: "test".into(),
            model: model.into(),
            lateral_offset: 0.0,
            yaw_deg: 0.0,
            height: 0.0,
            autumn_model: None,
            winter_model: None,
            lod_distances: Vec::new(),
            tags: Vec::new(),
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
