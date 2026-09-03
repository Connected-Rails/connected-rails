//! Camera-local standing vegetation: field crops and the default terrain grass.
//!
//! The painted surface carries a field at any distance but one: close up a
//! crop is not a colour on the ground but things standing on it. A maize
//! field in August is two and a half metres above the ballast, rape flowers
//! a metre into the air, and a cut field is stubble, not paint. So every
//! field patch grows a crop on its own surface — **real plant models** where
//! the camera is close enough to make out a leaf, and **painted cards**
//! (quads standing on the patch mesh) under and between them, out to where
//! the paint alone is what a field is. The default meadow reuses the same
//! cell-based LOD system, clipped to the terrain's grass splat and kept out
//! of fields, roads, and water.
//!
//! Everything is a function of the surface mesh and the day: the patch's
//! vertex colours carry each field's tint and its own week of the crop year,
//! the phenology says what that week looks like, and every random decision
//! comes out of the triangle and the cell it falls in. Two machines grow the
//! same field the same way without a byte crossing the network.
//!
//! **The cell grid is what makes it affordable.** A patch is a whole tile of
//! one crop — up to a quarter of a square kilometre — and a stand at field
//! density over that is a hundred thousand plants, of which a camera on the
//! ground can see a few thousand. Spreading a fixed budget over the patch
//! would put a tuft every seven metres and call it a wheat field. So the
//! patch is cut into [`CELL`]-metre cells, and a cell is grown when the
//! camera comes near it and dropped when it leaves: the stand is at its real
//! density everywhere an eye is, and nowhere else.
//!
//! Three things hold the cost down on top of that:
//!
//! * **Three levels per cell**, each the one before it thinned. Inside
//!   [`CLOSE_END`] the crop stands at its own spacing, two crossed quads a
//!   card, with the real models among it; to [`NEAR_END`] under a quarter of
//!   the cards at twice the width; to [`PLANT_CULL`] a tenth at four and a
//!   half times, one quad each. Every card carries a rank and a level keeps
//!   the ones below its threshold, so nothing shuffles as a camera walks up
//!   to it.
//! * **A cap per cell.** Past [`MAX_CARDS`] the spacing stretches, which
//!   reads as a thinner stand long before it reads as a fault.
//! * **A triangle purse for the models.** A model with twice the triangles
//!   stands half as often; the painted cards carry the mass either way.
//!
//! **Multiplayer.** Nothing here is state.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::asset::{AssetId, AssetPath, LoadState, RenderAssetUsages, embedded_asset};
use bevy::camera::RenderTarget;
use bevy::camera::visibility::VisibilityRange;
use bevy::gltf::{Gltf, GltfMesh};
use bevy::image::Image;
use bevy::light::NotShadowCaster;
use bevy::mesh::{MeshVertexAttribute, MeshVertexBufferLayoutRef};
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline,
};
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    TextureDimension, TextureFormat, VertexFormat,
};
use bevy::shader::ShaderRef;
use content::TerrainTile;
use fields::CropClass;
use fields::phenology::{self, Stage};

use crate::{
    Season, TextureMips, WorldView,
    farmland::{FieldSurface, linear},
    sky::Sky,
    weather::{WeatherExt, WeatherMaterial, WeatherParams},
};

/// Physically lit close grass with wind-deformed geometry.
pub type GrassMaterial = ExtendedMaterial<StandardMaterial, GrassExt>;

// Bevy's standard colour and UV attributes are fixed to 32-bit floats. Grass
// carries bounded values, so a private packed layout cuts each vertex from 56
// to 36 bytes while positions and lighting normals remain full precision.
const ATTRIBUTE_GRASS_COLOR: MeshVertexAttribute =
    MeshVertexAttribute::new("Grass_Color", 988_540_920, VertexFormat::Unorm8x4);
const ATTRIBUTE_GRASS_DATA: MeshVertexAttribute =
    MeshVertexAttribute::new("Grass_Data", 988_540_921, VertexFormat::Unorm16x4);

#[derive(Clone, Copy, Debug, PartialEq, ShaderType)]
pub struct GrassParams {
    /// Close, near and far transition centres, followed by the hero-detail centre [m].
    pub bands: Vec4,
    /// Half-width of those four smooth transitions [m].
    pub fades: Vec4,
    /// x is one while grass is enabled. Kept as a vector for uniform alignment.
    pub options: Vec4,
}

impl Default for GrassParams {
    fn default() -> Self {
        Self {
            bands: Vec4::new(CLOSE_END, NEAR_END, PLANT_CULL, 22.0),
            fades: Vec4::new(12.0, 12.0, 12.0, 8.0),
            options: Vec4::X,
        }
    }
}

/// Live grass controls shared by streaming and the material. Keeping the same
/// values on both sides means lowering the range also releases the cell meshes
/// instead of merely hiding them in the shader.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct GrassRenderSettings {
    pub params: GrassParams,
}

impl GrassRenderSettings {
    pub fn new(enabled: bool, bands: Vec4, fades: Vec4) -> Self {
        Self {
            params: GrassParams {
                bands,
                fades,
                options: if enabled { Vec4::X } else { Vec4::ZERO },
            },
        }
    }

    fn enabled(self) -> bool {
        self.params.options.x > 0.5
    }
}

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone, Default)]
pub struct GrassExt {
    #[uniform(100)]
    pub weather: WeatherParams,
    #[uniform(101)]
    pub grass: GrassParams,
}

impl MaterialExtension for GrassExt {
    fn vertex_shader() -> ShaderRef {
        "embedded://world_render/grass.wgsl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "embedded://world_render/grass.wgsl".into()
    }

    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_NORMAL.at_shader_location(1),
            ATTRIBUTE_GRASS_COLOR.at_shader_location(2),
            ATTRIBUTE_GRASS_DATA.at_shader_location(3),
        ])?];
        for define in ["VERTEX_UVS_A", "VERTEX_UVS_B", "VERTEX_COLORS"] {
            descriptor.vertex.shader_defs.push(define.into());
            if let Some(fragment) = &mut descriptor.fragment {
                fragment.shader_defs.push(define.into());
            }
        }
        Ok(())
    }
}

pub(crate) fn plugin(app: &mut App) {
    embedded_asset!(app, "grass.wgsl");
    app.init_resource::<GrassRenderSettings>()
        .add_plugins(MaterialPlugin::<GrassMaterial>::default());
}

/// Where the thickest level of the stand hands over to the next [m]. Inside
/// this the crop is at its real spacing — a clump of wheat every 60 cm — and
/// that is the only distance at which an eye can tell.
pub const CLOSE_END: f32 = 30.0;
/// Where the crossed cards hand over to single wider ones [m].
pub const NEAR_END: f32 = 70.0;
/// Past this only the painted surface shows [m]. The rows painted on the
/// surface underneath are what a field is at that distance, and a card there
/// is one pixel of relief on a colour that is already right.
pub const PLANT_CULL: f32 = 170.0;
/// The side of one cell of the stand [m]. Small enough that a cell nearest
/// the camera — which is grown at the crop's own spacing, thousands of tufts
/// — is still one frame's work, large enough that a cell is one worthwhile
/// draw and the grid is a few hundred entries on the biggest patch.
const CELL: f32 = 32.0;
/// How far a cell may drift past its band's edge before it is regrown [m].
/// A cell sitting exactly on the hand-over must not rebuild every frame.
const BAND_SLACK: f32 = 12.0;
/// How close the camera must come before a patch looks at its cells at all
/// [m] — one distance test that skips the whole grid for the fields a line
/// is not near.
const MATERIALISE_AT: f32 = PLANT_CULL + 2.0 * CELL;
/// How far out a patch drops everything it grew [m]. The gap to
/// [`MATERIALISE_AT`] is the hysteresis that keeps a boundary patch from
/// building and dropping every frame.
const DEMATERIALISE_AT: f32 = MATERIALISE_AT + 80.0;
/// Most cards one cell grows, at the closest level. A cell of meadow at its
/// own spacing wants about eight thousand tufts; the cap is well over that,
/// so it is a stop rather than a budget. Only the handful of cells nearest
/// the camera ever grow this many — the coarser levels keep a fraction and
/// pay for a fraction.
const MAX_CARDS: usize = 12_000;
/// How many real plants one cell stands at most, before its model's triangle
/// cost — a heavy model thins itself out against the budget, a light one
/// against the count.
const MAX_HEROES: usize = 160;
/// The floor of that count for the heaviest model (the orchard tree): a cell
/// of orchard with one tree in it is not an orchard.
const MIN_HEROES: usize = 3;
/// The triangle purse one cell's real plants are drawn from.
const HERO_TRIANGLE_BUDGET: usize = 40_000;
/// How finely the stand's height is tracked before the cards are regrown.
/// Height is baked into the geometry, and it moves slowly enough that a
/// quarter metre is the step an eye catches.
const HEIGHT_BUCKET: f32 = 0.25;
/// Work units available for plant construction in one frame. A close terrain
/// cell consumes all three because it expands to roughly a million vertices;
/// treating it like a far cell was the source of visible streaming stalls.
const BUILD_BUDGET: usize = 3;

/// One draw for a card, 0 … 1 — deterministic on every machine of a run.
///
/// Not `fields::stats::vary`: that hashes with FNV-1a, and FNV-1a's high bits
/// — the ones a fraction is read from — carry over from one sequential seed
/// to the next. A card index running 0, 1, 2 … through a triangle then draws
/// a biased share, which is fine for a colour and not fine for the thinning
/// the levels of detail are built on: a band meant to keep 45 % of the stand
/// kept 38, and the coarse level was thinner than the maths that sized its
/// cards. One round of the splitmix64 finaliser is what that costs.
fn draw(seed: u64, salt: u64) -> f64 {
    let mut z = seed.wrapping_add(salt).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
}

/// What a crop stands as where the camera is close: the model file, how
/// thickly the real plants are set [per m²], and how much of the model
/// belongs under the ground.
#[derive(Clone, Copy)]
struct Hero {
    file: &'static str,
    /// Plants per square metre — a fraction, because a real plant costs what
    /// a wood's tree costs and the painted cards between them carry the mass.
    density: f32,
    /// The share of the model's own height that is buried. A sugar beet is a
    /// leaf rosette with a root under it, and the root belongs in the soil;
    /// the stand height stays the height of what shows.
    sink: f32,
}

const fn hero(file: &'static str, density: f32, sink: f32) -> Hero {
    Hero {
        file,
        density,
        sink,
    }
}

/// The real plant a crop stands as on a day. The models are one metre tall
/// with the origin at the foot (`tools/plants`), so the phenology's stand
/// height *is* the scale.
///
/// A cut field is the one place the stage changes the model and not just its
/// size: half-height wheat is not what a stubble field looks like, straw is.
fn model_of(crop: CropClass, stage: Stage) -> Option<Hero> {
    if stage == Stage::Stubble
        && matches!(
            crop,
            CropClass::WinterCereal
                | CropClass::SummerCereal
                | CropClass::Rapeseed
                | CropClass::Legume
                | CropClass::Maize
                | CropClass::Other
        )
    {
        return Some(hero("hay", 0.045, 0.0));
    }
    Some(match crop {
        CropClass::WinterCereal | CropClass::SummerCereal => hero("wheat", 0.030, 0.0),
        CropClass::Maize => hero("corn", 0.050, 0.0),
        CropClass::Rapeseed => hero("flowers", 0.020, 0.0),
        CropClass::SugarBeet => hero("turnip", 0.035, 0.30),
        CropClass::Potato | CropClass::Legume => hero("clover", 0.045, 0.0),
        CropClass::Grassland => hero("grass", 0.060, 0.0),
        CropClass::Vegetable => hero("lettuce", 0.045, 0.0),
        CropClass::Orchard => hero("tree", 0.002, 0.0),
        CropClass::Vineyard => hero("vines", 0.030, 0.0),
        CropClass::Fallow => hero("flowers", 0.012, 0.0),
        CropClass::Other => hero("grass", 0.030, 0.0),
    })
}

/// How thickly the *painted* cards stand under the real plants [per m²],
/// at the closest level. The cards are the mass of the stand; the models are
/// its shape.
///
/// Picked against [`card_width`] rather than on their own: what an eye
/// looking *through* a stand sees is the cards' width per square metre, and
/// **density times width is between one and one and three quarters** for
/// everything meant to close over — maize at the bottom of that, because its
/// own models are big enough to carry the near view and a wall of cards on
/// top of them is a wall.
/// That is the number that decides whether a field looks like a crop or like
/// tufts on bare soil, and it has to be that high because a card is a
/// vertical sheet: from a cab window three metres up you are looking *down*
/// into the stand, where a card presents almost nothing. Thinner where the
/// real thing is thin (a fallow, a vineyard's rows), thinnest of all under an
/// orchard, which is grass with trees on it.
fn density(crop: CropClass) -> f32 {
    match crop {
        CropClass::Maize => 0.95,
        CropClass::Potato | CropClass::SugarBeet => 3.70,
        CropClass::WinterCereal | CropClass::SummerCereal | CropClass::Legume => 4.70,
        CropClass::Rapeseed => 3.00,
        CropClass::Vegetable => 5.30,
        CropClass::Other => 8.40,
        CropClass::Vineyard => 0.50,
        CropClass::Grassland => 8.00,
        CropClass::Fallow => 4.20,
        CropClass::Orchard => 0.22,
    }
}

/// How wide one card is [m], for a stand `height` metres tall.
///
/// **A card is a clump, not a field.** The first version drew a wheat card
/// 95 cm wide, and since the tuft on it is the same picture whatever it is
/// stretched over, each of its blades came out 16 to 39 mm across where a
/// wheat leaf is 8 to 15. Everything read three times life size. The numbers
/// below are what one clump of the crop actually measures: a drill of wheat
/// a third of a metre, a maize plant's leaf span about a metre, a beet's
/// rosette a third, an orchard tree its crown.
///
/// **Width follows height** — not linearly, because a tuft of grass is broad
/// for its height and a maize plant is not, so it goes with the three-quarter
/// power and the crop's own number is what it comes to at a one-metre stand.
fn card_width(crop: CropClass, height: f32) -> f32 {
    let base = match crop {
        // A crown, not a clump.
        CropClass::Orchard => 1.10,
        // One length of trained cane.
        CropClass::Vineyard => 0.70,
        // One plant's leaf span.
        CropClass::Maize => 0.50,
        CropClass::Rapeseed => 0.40,
        // One haulm of peas or beans — leaflets, not culms.
        CropClass::Legume => 0.50,
        // One rosette — broad for its height, which is why it has a sheet
        // of its own rather than a maize leaf shrunk down.
        CropClass::SugarBeet | CropClass::Potato => 0.70,
        CropClass::Vegetable => 0.75,
        // One tussock.
        CropClass::Grassland | CropClass::Fallow => 0.45,
        // A hand's width of drill.
        _ => 0.34,
    };
    (base * height.clamp(0.04, 3.2).powf(0.75)).clamp(0.04, 3.2)
}

/// What the day decides for one patch, as the cells compare it: the day of
/// the year, the stage the patch's fields have reached, the height the cards
/// were grown to, and whether deep winter has taken the crop away. When the
/// day moves any of these the whole patch is grown again; everything else the
/// day does — the colour, the weather — rides in the material for free.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CropKey {
    day: u16,
    stage: Stage,
    /// The stand's height, in [`HEIGHT_BUCKET`]s.
    height: u16,
    snow: bool,
    /// Whether real plant models stood among the cards. A model that lands
    /// after its crop's patches have grown flips this, and they regrow.
    heroes: bool,
}

impl CropKey {
    /// The day's state for a patch whose fields average `week` (0 … 1, the
    /// surface's own `b` channel) into the crop year.
    fn of(crop: CropClass, month: u32, day: u32, week: f32, heroes: bool) -> Self {
        let growth = phenology::growth_offset(
            crop,
            phenology::day_of_year(month, day),
            (week * 2.0 - 1.0) * 7.0,
        );
        Self {
            day: phenology::day_of_year(month, day),
            stage: growth.stage,
            height: (growth.height / HEIGHT_BUCKET) as u16,
            snow: Season::on(month, day).snow > 0.5,
            heroes,
        }
    }
}

/// One plant: a foot point on the field's surface and the few numbers that
/// pose a card or a model on it.
#[derive(Debug, Clone, Copy)]
struct Card {
    /// Stable random identity, retained so the close grass mesh can grow
    /// several different blades from one sampled tuft without storing every
    /// blade in the CPU-side stand.
    seed: u64,
    /// Foot point, in the tile's own frame.
    pos: Vec3,
    /// The ground's normal under the foot — the card leans with the field.
    up: Vec3,
    /// Turn about the up axis [rad].
    yaw: f32,
    width: f32,
    height: f32,
    /// How far the top leans out of true vertical, as a share of the height.
    lean: f32,
    /// The field's tint, out of the surface's vertex colour: a card matches
    /// the paint of the field it stands on.
    tint: f32,
    /// Brightness, 0 … 1 — no two tufts catch quite the same light.
    light: f32,
    /// The card's place in the thinning, 0 … 255. Each [`Band`] keeps the
    /// cards below its own threshold, so the coarse levels are the fine one
    /// thinned rather than rearranged and no card moves as a camera walks up
    /// to it.
    rank: u8,
    /// A spot where a real plant model stands instead of a painted card. The
    /// models cost what a wood's trees cost, so they are a hash-thinned share
    /// of the stand; the cards fill the space between them.
    hero: bool,
    /// Which tuft of the card sheet the card is cut out of, mirrored by the
    /// top bit — eight silhouettes, so no two neighbours are the same shape.
    tuft: u8,
    /// Which of the model's variants stands here. A Poly Pizza pack is a
    /// scene of several plants, and one field standing seven different
    /// clumps is what keeps a stand from reading as a stamp.
    variant: u8,
}

/// One mesh part of a plant model: geometry in the model's own frame, the
/// tallest variant one metre tall, every variant standing on its own foot —
/// what [`normalise.mjs`](tools/plants) writes.
pub(crate) struct PlantPart {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// The model's own vertex colours, where it ships any.
    pub colors: Option<Vec<[f32; 4]>>,
    /// Texture coordinates, where the part is textured.
    pub uvs: Option<Vec<[f32; 2]>>,
    pub indices: Vec<u32>,
    /// Which of the model's materials draws it.
    pub material: usize,
}

/// One plant of a model's pack — a whole plant, not a piece of one.
pub(crate) struct PlantVariant {
    pub parts: Vec<PlantPart>,
    pub tris: usize,
}

/// One material of a plant model, and what the stand colour has to do to it.
pub(crate) struct PlantSkin {
    pub handle: Handle<StandardMaterial>,
    /// Whether the part carries a picture of its own.
    pub textured: bool,
    /// The material's colour relative to the model's own average, so the day
    /// can repaint the whole plant without flattening it: a corn cob stays
    /// yellower than its stalk, a beet's leaves darker than its crown.
    pub tint: [f32; 3],
}

/// One resolved plant model: its variants and the materials they draw with.
pub(crate) struct PlantModel {
    pub variants: Vec<PlantVariant>,
    pub skins: Vec<PlantSkin>,
    /// The average variant's triangles — what the hero count is budgeted on,
    /// because a card picks its variant uniformly.
    pub tris: usize,
}

/// The models of the standing crop, per crop: the glTF handles, the resolved
/// geometry once a file has loaded, and the weather-dressed materials built
/// from the glTF's own.
#[derive(Resource, Default)]
pub struct PlantModels {
    /// glTF handles per model file, loaded on first use.
    handles: HashMap<&'static str, Handle<Gltf>>,
    /// Resolved parts per glTF; `None` is a file that failed and stays a
    /// painted-card crop.
    resolved: HashMap<AssetId<Gltf>, Option<Arc<PlantModel>>>,
    /// The glTF materials, dressed for the weather and drawn from both
    /// sides, per loader material.
    dressed: HashMap<AssetId<StandardMaterial>, Handle<WeatherMaterial>>,
    /// Strong handles to the /std materials a resolve has asked for. A load
    /// nothing holds is cancelled at the frame's end — the first resolve
    /// would ask, drop, and ask again forever. Kept as a set: a model that
    /// takes a hundred frames to arrive is asked for by every patch in
    /// range, and a plain list would grow with every one of them.
    pending: HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
}

impl PlantModels {
    /// The model of a crop at a stage, loading it on first ask and resolving
    /// it once it has arrived. `None` until then — the patch grows as painted
    /// cards and is regrown when the model lands.
    #[allow(clippy::too_many_arguments)]
    fn model(
        &mut self,
        crop: CropClass,
        stage: Stage,
        assets: &AssetServer,
        gltfs: &Assets<Gltf>,
        gltf_meshes: &Assets<GltfMesh>,
        meshes: &Assets<Mesh>,
        standards: &Assets<StandardMaterial>,
        mips: &mut TextureMips,
    ) -> Option<Arc<PlantModel>> {
        let file = model_of(crop, stage)?.file;
        let handle = self
            .handles
            .entry(file)
            .or_insert_with(|| assets.load(format!("embedded://world_render/plants/{file}.glb")))
            .clone();
        if let Some(cached) = self.resolved.get(&handle.id()) {
            return cached.clone();
        }
        // Still loading, or failed for good.
        match assets.get_load_state(handle.id()) {
            Some(LoadState::Loaded) => {}
            Some(LoadState::Failed(_)) => {
                warn!("plants: {file}.glb failed to load — {crop:?} stays painted cards");
                self.resolved.insert(handle.id(), None);
                return None;
            }
            _ => return None,
        }
        let gltf = gltfs.get(&handle)?;
        let path = handle.path()?;
        // A resolve that comes back empty while the file is fine means a
        // part's material is still loading; the model is tried again next
        // frame and cached only once it is whole. The patch grows as painted
        // cards meanwhile, and the key's missing flag brings it back.
        let resolved = resolve(
            path,
            assets,
            gltf,
            gltf_meshes,
            meshes,
            standards,
            &mut self.pending,
            mips,
        );
        if let Some(resolved) = resolved.clone() {
            self.resolved.insert(handle.id(), Some(resolved));
        }
        resolved
    }

    /// The weather-dressed material of one loader material, made once.
    fn dressed(
        &mut self,
        skin: &PlantSkin,
        standards: &Assets<StandardMaterial>,
        materials: &mut Assets<WeatherMaterial>,
    ) -> Option<Handle<WeatherMaterial>> {
        if let Some(cached) = self.dressed.get(&skin.handle.id()) {
            return Some(cached.clone());
        }
        let mut base = standards.get(&skin.handle)?.clone();
        // A model's own picture stays; an untextured part is repainted by the
        // day's stand colour through its vertices, so it needs a white base
        // to multiply into.
        base.metallic = 0.0;
        if !skin.textured {
            base.base_color = Color::WHITE;
        }
        // Every leaf is seen from both sides. `double_sided` alone only flips
        // the normal for back faces — without turning the culling off with
        // it, half of every plant is simply not drawn.
        base.double_sided = true;
        base.cull_mode = None;
        let dressed = materials.add(WeatherMaterial {
            base,
            extension: WeatherExt::default(),
        });
        self.dressed.insert(skin.handle.id(), dressed.clone());
        Some(dressed)
    }
}

/// Flattens a loaded plant glTF: one variant per mesh, its primitives split
/// by material, and the materials themselves with the tint each owes the
/// stand colour. The normaliser has already baked the transforms in — every
/// variant stands on its own foot, the tallest of them a metre — so there is
/// no hierarchy to walk.
#[allow(clippy::too_many_arguments)]
fn resolve(
    path: &AssetPath,
    assets: &AssetServer,
    gltf: &Gltf,
    gltf_meshes: &Assets<GltfMesh>,
    meshes: &Assets<Mesh>,
    standards: &Assets<StandardMaterial>,
    pending: &mut HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
    mips: &mut TextureMips,
) -> Option<Arc<PlantModel>> {
    let mut variants = Vec::new();
    let mut skins: Vec<PlantSkin> = Vec::new();
    // Triangles and linear base colour per material, for the tint below.
    let mut weight: Vec<(usize, [f32; 3])> = Vec::new();
    for mesh_handle in &gltf.meshes {
        let Some(gltf_mesh) = gltf_meshes.get(mesh_handle) else {
            continue;
        };
        let mut parts = Vec::new();
        let mut tris = 0usize;
        for primitive in &gltf_mesh.primitives {
            let mesh = meshes.get(&primitive.mesh)?;
            let Ok(VertexAttributeValues::Float32x3(positions)) =
                mesh.try_attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                continue;
            };
            let normals = match mesh.try_attribute(Mesh::ATTRIBUTE_NORMAL) {
                Ok(VertexAttributeValues::Float32x3(v)) => Some(v.clone()),
                _ => None,
            };
            let colors = match mesh.try_attribute(Mesh::ATTRIBUTE_COLOR) {
                Ok(VertexAttributeValues::Float32x4(v)) => Some(v.clone()),
                _ => None,
            };
            let uvs = match mesh.try_attribute(Mesh::ATTRIBUTE_UV_0) {
                Ok(VertexAttributeValues::Float32x2(v)) => Some(v.clone()),
                _ => None,
            };
            let indices = match mesh.indices() {
                Some(Indices::U32(v)) => v.clone(),
                Some(Indices::U16(v)) => v.iter().map(|&i| i as u32).collect(),
                _ => (0..positions.len() as u32).collect(),
            };
            // The loader files a `StandardMaterial` next to every glTF
            // material, under the material's label plus `/std` — the same
            // way the trees reach theirs.
            let label = primitive
                .material
                .as_ref()
                .and_then(|m| m.path())
                .and_then(|p| p.label())
                .map(str::to_owned)
                .unwrap_or_else(|| bevy::gltf::GltfAssetLabel::DefaultMaterial.to_string());
            let material: Handle<StandardMaterial> =
                assets.load(path.clone_owned().with_label(format!("{label}/std")));
            // The part's material arrives with its own asset load — a model
            // is only whole once every one of them is here. Reading it earlier
            // would mistake a textured part for a plain one and paint the
            // plant white.
            // The handle is kept alive on the resource, or the server would
            // cancel a load nothing holds and the material would sit in
            // `Loading` forever.
            pending
                .entry(material.id())
                .or_insert_with(|| material.clone());
            let standard = standards.get(&material)?;
            if !matches!(
                assets.get_load_state(material.id()),
                Some(LoadState::Loaded)
            ) {
                return None;
            }
            let textured = standard.base_color_texture.is_some();
            // The loader's materials carry mip chains for the textures they
            // hold — without the chain a cut-out tuft thins out with
            // distance (the same lesson the trees taught).
            if textured {
                mips.enqueue_cutout(&material);
            }
            let at = match skins.iter().position(|s| s.handle.id() == material.id()) {
                Some(at) => at,
                None => {
                    let rgba = standard.base_color.to_linear();
                    skins.push(PlantSkin {
                        handle: material,
                        textured,
                        tint: [1.0; 3],
                    });
                    weight.push((0, [rgba.red, rgba.green, rgba.blue]));
                    skins.len() - 1
                }
            };
            let count = indices.len() / 3;
            tris += count;
            weight[at].0 += count;
            parts.push(PlantPart {
                positions: positions.clone(),
                normals: normals.unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; positions.len()]),
                colors,
                uvs,
                indices,
                material: at,
            });
        }
        if !parts.is_empty() {
            variants.push(PlantVariant { parts, tris });
        }
    }
    if variants.is_empty() {
        return None;
    }
    tint_skins(&mut skins, &weight);
    let tris = variants.iter().map(|v| v.tris).sum::<usize>() / variants.len();
    Some(Arc::new(PlantModel {
        variants,
        skins,
        tris: tris.max(1),
    }))
}

/// What each untextured material owes the stand colour.
///
/// The day repaints an untextured plant wholesale — a wheat model is
/// blue-green in May and gold in July, and that is the point of painting it
/// at all. Painting every part the *same* colour is what flattens it: a corn
/// plant loses its cob, a turnip its root. So each material keeps its colour
/// **relative to the model's own average**, and it is the average that
/// becomes the stand colour.
fn tint_skins(skins: &mut [PlantSkin], weight: &[(usize, [f32; 3])]) {
    let plain: Vec<usize> = (0..skins.len()).filter(|&i| !skins[i].textured).collect();
    let total: usize = plain.iter().map(|&i| weight[i].0).sum();
    if total == 0 {
        return;
    }
    let mut mean = [0.0f32; 3];
    for &i in &plain {
        let share = weight[i].0 as f32 / total as f32;
        for (c, mean) in mean.iter_mut().enumerate() {
            *mean += weight[i].1[c] * share;
        }
    }
    for &i in &plain {
        let own = weight[i].1;
        for (c, tint) in skins[i].tint.iter_mut().enumerate() {
            // A part twenty times brighter than the mean would blow out the
            // moment the stand goes gold; the band is what a low-poly model's
            // parts actually differ by.
            *tint = if mean[c] > 1e-4 {
                (own[c] / mean[c]).clamp(0.35, 2.8)
            } else {
                1.0
            };
        }
    }
}

/// A patch's cell of the standing crop: where it is, which level is up, and
/// the entities that draw it.
struct Cell {
    /// The cell's place on the patch's own [`CELL`]-metre grid.
    key: IVec2,
    /// The cell's box in the patch's frame — what the camera distance that
    /// picks its level is measured to.
    lo: Vec3,
    hi: Vec3,
    /// The level that is up, `None` for a cell that has grown nothing.
    band: Option<Band>,
    /// The meshes that are up, with their assets — a rebuild despawns the
    /// entities *and* drops the geometry, which nothing else owns.
    lods: Vec<(Entity, AssetId<Mesh>)>,
}

/// How near a cell is, and so how thickly it stands.
///
/// Each level keeps a **share** of the cell's cards and draws them that much
/// **wider**: what an eye sees looking through a stand is the cards' total
/// width per metre of depth, so halving the count and doubling the width is
/// most of the same field for half the vertices. No card ever moves — the
/// coarse levels are the fine one thinned, so nothing shuffles as a camera
/// walks up to it.
///
/// The product does **not** stay flat, and deliberately. A card is a vertical
/// sheet, so what it hides depends on the angle you look down at it: from a
/// cab window three metres up you are looking almost straight down into the
/// stand ten metres away and almost along it at sixty, and closing the near
/// ground takes several times the cards that closing the far ground does.
/// Spending that everywhere would be tens of megabytes of quads nobody can
/// see past the second row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Band {
    /// The stand at its own spacing, and the real plant models among it.
    /// Every card, its own width, two crossed quads.
    Close,
    /// Under half the cards at twice the width, still crossed, still with
    /// models — the band a cab window spends most of its time looking at.
    Near,
    /// A fifth of the cards at four and a half times the width, one quad
    /// each. A single quad shows half of what a crossed pair does from an
    /// average angle, so this is the level where the stand does thin — and
    /// the painted rows underneath are what carry a field at ninety metres
    /// anyway.
    Far,
}

impl Band {
    /// The share of the cell's cards this level keeps, and the factor on
    /// their width.
    fn stand(self) -> (f32, f32) {
        match self {
            Band::Close => (1.00, 1.00),
            Band::Near => (0.24, 2.10),
            Band::Far => (0.10, 4.60),
        }
    }

    /// Whether a card is two crossed quads here, and whether the real plant
    /// models stand among them — the same answer to both.
    fn crossed(self) -> bool {
        !matches!(self, Band::Far)
    }

    /// Where this level draws, as the visibility range measures it.
    fn range(self) -> (f32, f32) {
        match self {
            Band::Close => (0.0, CLOSE_END),
            Band::Near => (CLOSE_END, NEAR_END),
            Band::Far => (NEAR_END, PLANT_CULL),
        }
    }

    /// Whether this level keeps a card. The share is a threshold on the
    /// card's own draw, so the levels nest: everything the far level keeps
    /// the near one keeps too, and a card never moves as the camera walks up
    /// to it.
    fn keeps(self, card: &Card) -> bool {
        (card.rank as f32) < self.stand().0 * 256.0
    }

    /// The levels this cell has to carry: its own and every coarser one, so
    /// the hand-over happens at the distance the range says rather than
    /// wherever the cell's residency hysteresis let go.
    fn upwards(self) -> impl Iterator<Item = Band> {
        [Band::Close, Band::Near, Band::Far]
            .into_iter()
            .filter(move |band| *band >= self)
    }
}

/// What a survey of a patch's mesh found, once, before anything grows on it.
struct Survey {
    centre: Vec3,
    radius: f32,
    /// The fields' week of the crop year averaged over the patch, 0 … 1 — the
    /// `b` channel of the surface's own vertex colours, weighted by ground
    /// area. Measured here rather than after the first growth so a patch's
    /// very first key is already the right one and it does not grow twice.
    week: f32,
    cells: Vec<Cell>,
}

/// Measures a patch: its reach, its fields' average week, and the cells its
/// surface covers with the ground height in each.
fn survey(mesh: &Mesh) -> Option<Survey> {
    let positions = mesh
        .try_attribute(Mesh::ATTRIBUTE_POSITION)
        .ok()?
        .as_float3()?;
    let colors = match mesh.try_attribute(Mesh::ATTRIBUTE_COLOR).ok()? {
        VertexAttributeValues::Float32x4(colors) => colors,
        _ => return None,
    };
    let tris = Tris::of(mesh)?;
    let mut lo = Vec3::splat(f32::MAX);
    let mut hi = Vec3::splat(f32::MIN);
    let mut boxes: HashMap<IVec2, (f32, f32)> = HashMap::new();
    let mut week_sum = 0.0f64;
    let mut area_sum = 0.0f64;
    for at in 0..tris.len() {
        let [ia, ib, ic] = tris.at(at);
        if ia.max(ib).max(ic) >= positions.len() {
            continue;
        }
        let p = [
            Vec3::from(positions[ia]),
            Vec3::from(positions[ib]),
            Vec3::from(positions[ic]),
        ];
        let area = plan_area(p) as f64;
        if area > 0.0 {
            let week = (colors[ia][2] + colors[ib][2] + colors[ic][2]) / 3.0;
            week_sum += week as f64 * area;
            area_sum += area;
        }
        let t_lo = p[0].min(p[1]).min(p[2]);
        let t_hi = p[0].max(p[1]).max(p[2]);
        lo = lo.min(t_lo);
        hi = hi.max(t_hi);
        let (from, to) = (cell_of(t_lo.xz()), cell_of(t_hi.xz()));
        for x in from.x..=to.x {
            for y in from.y..=to.y {
                let e = boxes
                    .entry(IVec2::new(x, y))
                    .or_insert((f32::MAX, f32::MIN));
                e.0 = e.0.min(t_lo.y);
                e.1 = e.1.max(t_hi.y);
            }
        }
    }
    if lo.x > hi.x || boxes.is_empty() {
        return None;
    }
    let mut cells: Vec<Cell> = boxes
        .into_iter()
        .map(|(key, (lo_y, hi_y))| {
            let min = key.as_vec2() * CELL;
            Cell {
                key,
                lo: Vec3::new(min.x, lo_y, min.y),
                hi: Vec3::new(min.x + CELL, hi_y, min.y + CELL),
                band: None,
                lods: Vec::new(),
            }
        })
        .collect();
    // A stable order, so two machines walk the same patch the same way and a
    // build budget spends itself on the same cells.
    cells.sort_unstable_by_key(|cell| (cell.key.x, cell.key.y));
    Some(Survey {
        centre: (lo + hi) * 0.5,
        radius: hi.distance(lo) * 0.5,
        week: if area_sum > 0.0 {
            (week_sum / area_sum).clamp(0.0, 1.0) as f32
        } else {
            0.5
        },
        cells,
    })
}

/// The cell a point on the ground falls in.
fn cell_of(p: Vec2) -> IVec2 {
    (p / CELL).floor().as_ivec2()
}

/// Grows the cards of one cell of a field patch out of the patch's surface.
///
/// The mesh is the draped ground in the tile's own frame — already cut to
/// the tile, already cleared of the track corridor — and it carries each
/// field's tint and its own week of the crop year in its vertex colours.
/// Every triangle is clipped to the cell, cards are sampled over what is
/// left by area, and every random decision comes out of the triangle and the
/// cell, so the same cell always grows the same crop of cards: on every
/// machine of a multiplayer run, and on every frame the day is asked again.
///
/// Density is per square metre of *ground*, not of surface: a field's plants
/// are set out on the plan the tractor drives, and farmland is flat enough
/// that the two differ by less than the spacing does.
// A scatter takes what it scatters, where, when and how thickly; the count
// says nothing here.
#[allow(clippy::too_many_arguments)]
fn grow(
    mesh: &Mesh,
    crop: CropClass,
    month: u32,
    day: u32,
    key: IVec2,
    model: Option<&PlantModel>,
    stage: Stage,
    band: Band,
    terrain: Option<&TerrainGrass>,
) -> Vec<Card> {
    // Deep winter takes the crop away: the weather uniform has whitened the
    // paint, and green tufts over snow is the one thing worse than none.
    if Season::on(month, day).snow > 0.5 {
        return Vec::new();
    }
    let (Ok(positions), Ok(normals)) = (
        mesh.try_attribute(Mesh::ATTRIBUTE_POSITION),
        mesh.try_attribute(Mesh::ATTRIBUTE_NORMAL),
    ) else {
        return Vec::new();
    };
    let (Some(positions), Some(normals)) = (positions.as_float3(), normals.as_float3()) else {
        return Vec::new();
    };
    let Some(VertexAttributeValues::Float32x4(colors)) =
        mesh.try_attribute(Mesh::ATTRIBUTE_COLOR).ok()
    else {
        return Vec::new();
    };
    let Some(tris) = Tris::of(mesh) else {
        return Vec::new();
    };

    let min = key.as_vec2() * CELL;
    let max = min + CELL;
    // What of the patch falls in this cell, triangle by triangle. Clipped
    // once: the area decides the counts, and the counts are drawn out of the
    // same polygons.
    let mut pieces: Vec<(usize, f32, Vec<Vec2>)> = Vec::new();
    let mut area = 0.0f32;
    let mut scratch = Vec::new();
    let mut poly = Vec::new();
    let candidates: Box<dyn Iterator<Item = usize> + '_> = match terrain {
        Some(terrain) => Box::new(terrain.triangles(key).iter().copied()),
        None => Box::new(0..tris.len()),
    };
    for i in candidates {
        let [ia, ib, ic] = tris.at(i);
        if ia.max(ib).max(ic) >= positions.len() {
            continue;
        }
        let flat = [
            Vec3::from(positions[ia]).xz(),
            Vec3::from(positions[ib]).xz(),
            Vec3::from(positions[ic]).xz(),
        ];
        // Every cell of a patch walks every triangle of it, and a tile of
        // farmland has tens of thousands. The box test is what keeps that a
        // scan rather than a clip: only the handful that reach into this
        // cell are worth four planes of Sutherland-Hodgman.
        let (t_lo, t_hi) = (
            flat[0].min(flat[1]).min(flat[2]),
            flat[0].max(flat[1]).max(flat[2]),
        );
        if t_hi.x <= min.x || t_lo.x >= max.x || t_hi.y <= min.y || t_lo.y >= max.y {
            continue;
        }
        clip_to_cell(flat, min, max, &mut scratch, &mut poly);
        if poly.len() < 3 {
            continue;
        }
        let piece = polygon_area(&poly);
        if piece <= 1e-4 {
            continue;
        }
        area += piece;
        pieces.push((i, piece, std::mem::take(&mut poly)));
    }
    if pieces.is_empty() {
        return Vec::new();
    }

    // The factor that brings the cell to the cap — a plain ratio, not a
    // square root: spacing grows with the root of the count, the density
    // falls with the count.
    // The PBR scan closes the ground between plants. Four evenly distributed
    // batches per square metre. The base contributes 64 blades/m² and the
    // immediate curved layer another 88, without carrying millions of
    // alpha-tested quads into the far field.
    let wanted_density = if terrain.is_some() {
        4.0
    } else {
        density(crop)
    };
    let stretch = ((area * wanted_density) / MAX_CARDS as f32).max(1.0);
    let density = wanted_density / stretch;
    // A cell only ever draws its own level and the coarser ones, and every
    // coarser level is this one thinned — so a far cell has no use for the
    // cards its close level would have kept, and does not pay for them.
    // Terrain needs the complete deterministic sample in every resident
    // band: its close and near meshes choose different numbers of blades per
    // tuft, while the far card mesh applies its own coverage-preserving thin.
    // Field crops keep their established CPU-side thinning.
    let keep = if terrain.is_some() {
        256.0
    } else {
        band.stand().0 * 256.0
    };

    // The share of cards that stands as a real plant: the model's own
    // density over the cards', capped by the count the model's triangle cost
    // allows in one cell.
    let hero_share = match model {
        Some(model) => {
            let by_count =
                MAX_HEROES.min(MIN_HEROES.max(HERO_TRIANGLE_BUDGET / model.tris.max(1))) as f32;
            let wanted = model_of(crop, stage).map_or(0.0, |hero| hero.density)
                * if terrain.is_some() { 0.5 } else { 1.0 };
            let cards = area * density;
            if cards > 0.0 {
                (wanted / density).min(by_count / cards).min(1.0)
            } else {
                0.0
            }
        }
        None => 0.0,
    };
    let variants = model.map_or(1, |model| model.variants.len().max(1)) as f64;

    // A salt of the cell's own, so two cells of one field never grow the
    // same pattern of tufts and the grid never reads as a grid.
    let salt = 0x91A7u64
        ^ (key.x as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (key.y as i64 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    let today = phenology::day_of_year(month, day);
    let mut cards: Vec<Card> = Vec::new();
    for (i, piece, poly) in &pieces {
        let (i, piece, poly) = (*i, *piece, &poly[..]);
        let [ia, ib, ic] = tris.at(i);
        let (a, b, c) = (
            Vec3::from(positions[ia]),
            Vec3::from(positions[ib]),
            Vec3::from(positions[ic]),
        );
        // Whole cards for the whole area, and the fraction of one the area
        // earns — kept or not by the hash, so the count never needs a shared
        // random state.
        let want = piece * density;
        let mut count = want as usize;
        if (want - count as f32) as f64 > draw(i as u64, salt + 1) {
            count += 1;
        }
        for k in 0..count as u64 {
            let seed = i as u64 * 1_009 + k;
            let (r, u, v) = if terrain.is_some() {
                // Jittered strata give every part of a terrain triangle its
                // share of the stand. Pure random sampling left clusters and
                // holes; the previous additive sequence folded into long
                // diagonal bands. Hash-jittering a small square lattice has
                // neither failure mode.
                let side = (count as f64).sqrt().ceil().max(1.0);
                let x = (k as f64 % side + draw(seed, salt + 3)) / side;
                let y = ((k as f64 / side).floor() + draw(seed, salt + 4)) / side;
                (draw(seed, salt + 2), x, y)
            } else {
                (
                    draw(seed, salt + 2),
                    draw(seed, salt + 3),
                    draw(seed, salt + 4),
                )
            };
            let at = sample_polygon(poly, piece, r, u, v);
            if terrain.is_some_and(|terrain| terrain.covered(key, at)) {
                continue;
            }
            let Some(w) = barycentric(at, a.xz(), b.xz(), c.xz()) else {
                continue;
            };
            if terrain.is_some() {
                // Interpolated splat weight is the exact decision the ground
                // shader makes. Once grass is the meaningful surface here,
                // keep the complete low-discrepancy stand: stochastic
                // thinning turned smooth splat variation into conspicuous
                // bare islands. Roads, water and fields are already removed
                // by the geometric exclusion index above.
                let grass = (colors[ia][0] * w.x + colors[ib][0] * w.y + colors[ic][0] * w.z)
                    .clamp(0.0, 1.0);
                if grass < 0.12 {
                    continue;
                }
            }
            let pos = a * w.x + b * w.y + c * w.z;
            let up = (Vec3::from(normals[ia]) * w.x
                + Vec3::from(normals[ib]) * w.y
                + Vec3::from(normals[ic]) * w.z)
                .normalize_or_zero();
            let tint = if terrain.is_some() {
                0.5
            } else {
                colors[ia][0] * w.x + colors[ib][0] * w.y + colors[ic][0] * w.z
            };
            let week = if terrain.is_some() {
                0.5
            } else {
                (colors[ia][2] * w.x + colors[ib][2] * w.y + colors[ic][2] * w.z).clamp(0.0, 1.0)
            };

            // The field's own week decides what the day is here — two wheat
            // fields in one patch ripen a week apart, and the cards on them
            // do too. `pick` is the card's own draw from that same field.
            let pick = draw(seed, salt + 5) as f32;
            let mut growth = phenology::growth_offset(crop, today, (week * 2.0 - 1.0) * 7.0);
            if terrain.is_some() && growth.stage == Stage::Stubble {
                growth.stage = Stage::Growing;
                growth.cover = 1.0;
                growth.height = growth.height.max(0.12);
            }
            // Nothing stands on ploughed ground. Stubble keeps a third of
            // the tufts, short; a thin stand thins with its cover.
            let (scale, kept) = match growth.stage {
                Stage::Bare => (0.0, false),
                Stage::Stubble => (0.5, pick < 0.35),
                _ => (1.0, pick < growth.cover * 1.15),
            };
            if !kept {
                continue;
            }
            let rank = (draw(seed, salt + 8) * 256.0) as u8;
            if (rank as f32) >= keep {
                continue;
            }
            let mut height =
                (growth.height * (0.7 + 0.5 * draw(seed, salt + 9) as f32) * scale).max(0.03);
            if terrain.is_some() {
                // A default verge is a close lawn layer, not a field of
                // knee-high tussocks. A narrow range also makes the fade
                // into the detailed ground material read as detail loss
                // instead of vegetation suddenly losing volume.
                height = height.clamp(0.12, 0.26);
            }
            cards.push(Card {
                seed: seed ^ salt,
                pos,
                up: if up.length_squared() > 0.5 {
                    up
                } else {
                    Vec3::Y
                },
                yaw: (draw(seed, salt + 7) as f32 - 0.5) * std::f32::consts::TAU,
                width: card_width(crop, height)
                    * (0.8 + 0.4 * draw(seed, salt + 6) as f32)
                    * if terrain.is_some() { 1.1 } else { 1.0 },
                height,
                lean: (draw(seed, salt + 10) as f32 - 0.5) * 0.35,
                tint,
                light: draw(seed, salt + 21) as f32,
                rank,
                // Every so often a real plant stands where a card would: the
                // share is the model's density over the cards', so a thick
                // model thins itself out against the budget. Only where the
                // card's own field is at the patch's stage, because the model
                // was picked for that stage — a wheat plant has no business
                // in a field the combine has already been over.
                hero: growth.stage == stage && draw(seed, salt + 12) < hero_share as f64,
                tuft: (draw(seed, salt + 14) * (SHEET_TUFTS * 2) as f64) as u8,
                variant: (draw(seed, salt + 13) * variants) as u8,
            });
        }
    }
    cards
}

/// A triangle's footprint clipped to a cell, as a convex ring in the ground
/// plane. Sutherland–Hodgman against the cell's four edges — a triangle and
/// a rectangle are both convex, so what comes out is one ring of at most
/// seven points, and the cells of a patch partition its triangles exactly.
fn clip_to_cell(
    tri: [Vec2; 3],
    min: Vec2,
    max: Vec2,
    scratch: &mut Vec<Vec2>,
    out: &mut Vec<Vec2>,
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
        let inside = |p: Vec2| {
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

/// The area of a convex ring, by the shoelace over a fan from its first
/// corner.
fn polygon_area(poly: &[Vec2]) -> f32 {
    let mut area = 0.0;
    for i in 1..poly.len().saturating_sub(1) {
        area += (poly[i] - poly[0]).perp_dot(poly[i + 1] - poly[0]);
    }
    area.abs() * 0.5
}

/// A point in a convex ring of known `area`, uniform by area: `r` picks the
/// fan triangle, `u` and `v` the point in it.
fn sample_polygon(poly: &[Vec2], area: f32, r: f64, u: f64, v: f64) -> Vec2 {
    let target = r as f32 * area;
    let mut acc = 0.0;
    let mut at = 1usize;
    for i in 1..poly.len() - 1 {
        at = i;
        acc += (poly[i] - poly[0]).perp_dot(poly[i + 1] - poly[0]).abs() * 0.5;
        if acc >= target {
            break;
        }
    }
    let (a, b, c) = (poly[0], poly[at], poly[at + 1]);
    // A uniform point in the triangle, folded in at the far edge.
    let (mut u, mut v) = (u as f32, v as f32);
    if u + v > 1.0 {
        u = 1.0 - u;
        v = 1.0 - v;
    }
    a + (b - a) * u + (c - a) * v
}

/// Where a point in the ground plane sits on a triangle, as the weights that
/// rebuild everything the triangle's corners carry. `None` for a triangle
/// with no footprint — a wall, which farmland does not have.
fn barycentric(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> Option<Vec3> {
    let (v0, v1, v2) = (b - a, c - a, p - a);
    let den = v0.perp_dot(v1);
    if den.abs() < 1e-9 {
        return None;
    }
    let u = v2.perp_dot(v1) / den;
    let v = v0.perp_dot(v2) / den;
    // The sample came out of a polygon clipped from this very triangle, so
    // the weights are inside it but for the last bit of floating point.
    let w = Vec3::new(1.0 - u - v, u, v).clamp(Vec3::ZERO, Vec3::ONE);
    let sum = w.element_sum();
    (sum > 1e-6).then(|| w / sum)
}

/// The ground area a triangle covers, which is what a crop is set out on.
fn plan_area(p: [Vec3; 3]) -> f32 {
    (p[1].xz() - p[0].xz())
        .perp_dot(p[2].xz() - p[0].xz())
        .abs()
        * 0.5
}

/// The materials of the painted cards, one per crop, and the day they were
/// written for — the same shape as the farmland's own.
#[derive(Resource, Default)]
pub struct PlantMaterials {
    by_crop: HashMap<CropClass, Handle<WeatherMaterial>>,
    /// The default terrain meadow uses a full-width blade sheet instead of
    /// the compact crop tuft, so overlapping cards form one continuous mat.
    terrain: Option<Handle<WeatherMaterial>>,
    /// Opaque, physically lit blade geometry for the default meadow's close
    /// level. It deliberately has no cut-out texture: its silhouette is the
    /// mesh itself, which is what removes the crossed-billboard look.
    blades_by_crop: HashMap<CropClass, Handle<GrassMaterial>>,
    /// The cut-out sheets the cards are cut from, one per leaf shape, drawn
    /// on first use.
    sheets: HashMap<Leaf, Handle<Image>>,
    day: Option<u16>,
}

impl PlantMaterials {
    /// The material for a crop, made on first use.
    pub fn get(
        &mut self,
        crop: CropClass,
        assets: &mut Assets<WeatherMaterial>,
        images: &mut Assets<Image>,
        month: u32,
        day: u32,
    ) -> Handle<WeatherMaterial> {
        let leaf = leaf_of(crop);
        let sheet = self
            .sheets
            .entry(leaf)
            .or_insert_with(|| images.add(card_sheet(leaf)))
            .clone();
        self.by_crop
            .entry(crop)
            .or_insert_with(|| {
                let growth = phenology::growth(crop, month, day, 0);
                assets.add(WeatherMaterial {
                    base: StandardMaterial {
                        base_color: stand_colour(growth),
                        // The tuft the card is cut out of. Without it a card
                        // is a solid rectangle and a field is a hedge.
                        base_color_texture: Some(sheet),
                        alpha_mode: AlphaMode::Mask(CARD_CUTOFF),
                        // Two quads crossed: half of them face away from any
                        // camera, so the culling goes — but *not* into
                        // `double_sided`, which negates the normal on the far
                        // side. A card's normal already points at the sky
                        // (`card_mesh`); negated it points into the ground,
                        // and half of every stand goes black.
                        cull_mode: None,
                        perceptual_roughness: 0.85,
                        ..default()
                    },
                    extension: WeatherExt::default(),
                })
            })
            .clone()
    }

    /// The two-sided foliage material used by real blade geometry close to
    /// the camera. Diffuse transmission gives a sunlit leaf its bright back
    /// side without the cost and sorting problems of transparent blending.
    pub fn blades(
        &mut self,
        crop: CropClass,
        assets: &mut Assets<GrassMaterial>,
        month: u32,
        day: u32,
        grass: GrassParams,
    ) -> Handle<GrassMaterial> {
        self.blades_by_crop
            .entry(crop)
            .or_insert_with(|| {
                assets.add(GrassMaterial {
                    base: StandardMaterial {
                        base_color: stand_colour(phenology::growth(crop, month, day, 0)),
                        perceptual_roughness: 0.82,
                        reflectance: 0.22,
                        diffuse_transmission: 0.38,
                        thickness: 0.008,
                        double_sided: true,
                        cull_mode: None,
                        ..default()
                    },
                    extension: GrassExt { grass, ..default() },
                })
            })
            .clone()
    }

    pub fn terrain(
        &mut self,
        assets: &mut Assets<WeatherMaterial>,
        images: &mut Assets<Image>,
        month: u32,
        day: u32,
    ) -> Handle<WeatherMaterial> {
        if let Some(material) = &self.terrain {
            return material.clone();
        }
        let sheet = self
            .sheets
            .entry(Leaf::Meadow)
            .or_insert_with(|| images.add(card_sheet(Leaf::Meadow)))
            .clone();
        let material = assets.add(WeatherMaterial {
            base: StandardMaterial {
                base_color: stand_colour(phenology::growth(CropClass::Grassland, month, day, 0)),
                base_color_texture: Some(sheet),
                alpha_mode: AlphaMode::Mask(CARD_CUTOFF),
                cull_mode: None,
                perceptual_roughness: 0.85,
                ..default()
            },
            extension: WeatherExt::default(),
        });
        self.terrain = Some(material.clone());
        material
    }

    /// Writes the day's colour into every crop's material, if the day moved.
    /// The stage and the height are the meshes' business — a rebuild; the
    /// colour is the material's, and costs one write per crop.
    pub fn set_date(
        &mut self,
        assets: &mut Assets<WeatherMaterial>,
        blades: &mut Assets<GrassMaterial>,
        month: u32,
        day: u32,
    ) -> bool {
        let today = phenology::day_of_year(month, day);
        if self.day == Some(today) {
            return false;
        }
        self.day = Some(today);
        for (crop, handle) in &self.by_crop {
            if let Some(mut material) = assets.get_mut(handle) {
                material.base.base_color = stand_colour(phenology::growth(*crop, month, day, 0));
            }
        }
        for (crop, handle) in &self.blades_by_crop {
            if let Some(mut material) = blades.get_mut(handle) {
                material.base.base_color = stand_colour(phenology::growth(*crop, month, day, 0));
            }
        }
        if let Some(handle) = &self.terrain
            && let Some(mut material) = assets.get_mut(handle)
        {
            material.base.base_color =
                stand_colour(phenology::growth(CropClass::Grassland, month, day, 0));
        }
        true
    }

    pub fn is_empty(&self) -> bool {
        self.by_crop.is_empty() && self.blades_by_crop.is_empty() && self.terrain.is_none()
    }
}

/// A crop's colour on a day, as the material wants it: the phenology table is
/// sRGB, the material blends in linear.
fn stand_colour(growth: phenology::Growth) -> Color {
    Color::LinearRgba(LinearRgba::new(
        linear(growth.color[0]),
        linear(growth.color[1]),
        linear(growth.color[2]),
        1.0,
    ))
}

/// Turns the standing crop with the calendar, the way the painted surface
/// does: the same `Sky`, the same day of the year, the same one frame in a
/// thousand that is not a no-op.
pub fn follow_date(
    sky: Res<Sky>,
    mut materials: ResMut<PlantMaterials>,
    mut assets: ResMut<Assets<WeatherMaterial>>,
    mut blades: ResMut<Assets<GrassMaterial>>,
) {
    if materials.is_empty() {
        return;
    }
    materials.set_date(&mut assets, &mut blades, sky.month, sky.day);
}

/// The standing crop of one field patch: the cells it is cut into, what they
/// were grown for, and the reach the whole thing is gated on.
#[derive(Component, Default)]
pub struct FieldPlants {
    /// The cells the patch's surface covers, measured once.
    cells: Vec<Cell>,
    /// Whether that measurement has been taken — a patch whose mesh carries
    /// no colours is surveyed once and left alone.
    surveyed: bool,
    /// Centre and reach of the patch, in the surface's own frame.
    centre: Vec3,
    radius: f32,
    /// The fields' week averaged over the patch, as the surface's `b`
    /// channel carries it.
    week: f32,
    /// What the cells that are up were grown for.
    grown: Option<CropKey>,
}

/// The base terrain as a grass-bearing plant surface.
///
/// Two compact spatial indexes are built while the streamed [`TerrainTile`]
/// is still available: terrain triangles worth sampling in each plant cell,
/// and the field/road/water triangles where default grass must leave a hole.
/// A cell build therefore scans a few dozen triangles instead of the whole
/// 512 metre tile and needs not retain the complete content tile.
#[derive(Component, Default)]
pub struct TerrainGrass {
    triangles: HashMap<IVec2, Vec<usize>>,
    exclusions: HashMap<IVec2, Vec<[Vec2; 3]>>,
}

impl TerrainGrass {
    pub(crate) fn new(tile: &TerrainTile) -> Self {
        let mut grass = Self::default();
        for (triangle, indices) in tile.indices.as_chunks::<3>().0.iter().enumerate() {
            let [ia, ib, ic] = [
                indices[0] as usize,
                indices[1] as usize,
                indices[2] as usize,
            ];
            if ia.max(ib).max(ic) >= tile.positions.len() || ia.max(ib).max(ic) >= tile.splat.len()
            {
                continue;
            }
            // No meaningful grass weight means no tuft. This also drops the
            // vertical terrain skirt and the gravel formation up front.
            let weight = (tile.splat[ia][0] + tile.splat[ib][0] + tile.splat[ic][0]) / 3.0;
            if weight < 0.04 {
                continue;
            }
            let points = [
                Vec3::from(tile.positions[ia]).xz(),
                Vec3::from(tile.positions[ib]).xz(),
                Vec3::from(tile.positions[ic]).xz(),
            ];
            if triangle_area(points) <= 1e-4 {
                continue;
            }
            for key in triangle_cells(points) {
                grass.triangles.entry(key).or_default().push(triangle);
            }
        }
        for patch in &tile.fields {
            grass.add_exclusions(&patch.positions, &patch.indices);
        }
        for patch in &tile.roads {
            grass.add_exclusions(&patch.positions, &patch.indices);
        }
        for patch in &tile.waters {
            grass.add_exclusions(&patch.positions, &patch.indices);
        }
        grass
    }

    fn add_exclusions(&mut self, positions: &[[f32; 3]], indices: &[u32]) {
        for indices in indices.as_chunks::<3>().0 {
            let [ia, ib, ic] = [
                indices[0] as usize,
                indices[1] as usize,
                indices[2] as usize,
            ];
            if ia.max(ib).max(ic) >= positions.len() {
                continue;
            }
            let points = [
                Vec3::from(positions[ia]).xz(),
                Vec3::from(positions[ib]).xz(),
                Vec3::from(positions[ic]).xz(),
            ];
            if triangle_area(points) <= 1e-4 {
                continue;
            }
            for key in triangle_cells(points) {
                self.exclusions.entry(key).or_default().push(points);
            }
        }
    }

    fn triangles(&self, key: IVec2) -> &[usize] {
        self.triangles.get(&key).map(Vec::as_slice).unwrap_or(&[])
    }

    fn covered(&self, key: IVec2, point: Vec2) -> bool {
        self.exclusions.get(&key).is_some_and(|tris| {
            tris.iter()
                .any(|triangle| point_in_triangle(point, *triangle))
        })
    }
}

fn triangle_area(points: [Vec2; 3]) -> f32 {
    (points[1] - points[0])
        .perp_dot(points[2] - points[0])
        .abs()
        * 0.5
}

fn triangle_cells(points: [Vec2; 3]) -> impl Iterator<Item = IVec2> {
    let lo = cell_of(points[0].min(points[1]).min(points[2]));
    let hi = cell_of(points[0].max(points[1]).max(points[2]));
    (lo.x..=hi.x).flat_map(move |x| (lo.y..=hi.y).map(move |y| IVec2::new(x, y)))
}

fn point_in_triangle(point: Vec2, triangle: [Vec2; 3]) -> bool {
    let [a, b, c] = triangle;
    let ab = (b - a).perp_dot(point - a);
    let bc = (c - b).perp_dot(point - b);
    let ca = (a - c).perp_dot(point - c);
    let epsilon = 1e-4;
    (ab >= -epsilon && bc >= -epsilon && ca >= -epsilon)
        || (ab <= epsilon && bc <= epsilon && ca <= epsilon)
}

/// Takes one cell's meshes down: the entities go and their mesh assets with
/// them — the geometry is the cell's alone, and leaving it in the asset store
/// would pile a copy onto the GPU at every regrow.
fn drop_cell(commands: &mut Commands, cell: &mut Cell) {
    for (entity, mesh) in cell.lods.drain(..) {
        commands.entity(entity).try_despawn();
        // The mesh asset goes with the entity — but the despawn is queued
        // and the removal must not overtake it, or extraction reads an asset
        // that is already gone. Queue the removal behind the despawn, and
        // never remove the store itself: its event stream is what carries
        // every other mesh to the GPU.
        commands.queue(move |world: &mut World| {
            if let Some(mut meshes) = world.get_resource_mut::<Assets<Mesh>>() {
                meshes.remove(mesh);
            }
        });
    }
    cell.band = None;
}

/// Takes the whole patch down — out of range, or a new day.
fn clear(commands: &mut Commands, state: &mut FieldPlants) {
    for cell in &mut state.cells {
        drop_cell(commands, cell);
    }
    state.grown = None;
}

/// Which level a cell at `distance` should be at, given the one it is at.
///
/// The hand-overs themselves belong to the visibility ranges, which measure
/// them per frame and to the cell's own box. This only decides
/// what geometry has to be *resident*, and it is deliberately slack about it:
/// a cell sitting on a boundary must not rebuild every frame, and a near cell
/// that keeps its far level a few metres too long costs one draw.
fn band_for(distance: f32, current: Option<Band>) -> Option<Band> {
    // What the distance alone asks for.
    let plain = if distance < CLOSE_END {
        Some(Band::Close)
    } else if distance < NEAR_END {
        Some(Band::Near)
    } else if distance < PLANT_CULL {
        Some(Band::Far)
    } else {
        None
    };
    // A cell holds what it has as long as it is within a slack of the level
    // it is at — the *coarser* way only, because a cell that has grown its
    // close level already carries everything the coarser ones draw.
    let held = |band: Band| {
        let (start, end) = band.range();
        distance > start - BAND_SLACK && distance < end + BAND_SLACK
    };
    match current {
        Some(band) if held(band) => Some(band),
        _ => plain,
    }
}

/// Terrain meshes are prefetched one complete fade before their shader band
/// begins. The actual selection is per blade in `grass.wgsl`; these larger
/// bounds only decide which cell assets must be resident, never what is
/// visible. That distinction is what prevents the 32 m cell grid becoming a
/// visible LOD grid.
fn terrain_band_for(distance: f32, grass: GrassRenderSettings) -> Option<Band> {
    if !grass.enabled() {
        return None;
    }
    let bands = grass.params.bands;
    let fades = grass.params.fades;
    if distance < bands.x + fades.x {
        Some(Band::Close)
    } else if distance < bands.y + fades.y {
        Some(Band::Near)
    } else if distance < bands.z + fades.z {
        Some(Band::Far)
    } else {
        None
    }
}

/// How far a point is from a box, zero inside it.
fn box_distance(p: Vec3, lo: Vec3, hi: Vec3) -> f32 {
    (lo - p).max(p - hi).max(Vec3::ZERO).length()
}

/// Horizontal distance to a cell. Terrain grass is cached as a vertical
/// column around the camera: altitude changes only the shader LOD and never
/// tears down geometry that an immediate descent needs again.
fn planar_box_distance(p: Vec3, lo: Vec3, hi: Vec3) -> f32 {
    let delta = (lo.xz() - p.xz()).max(p.xz() - hi.xz()).max(Vec2::ZERO);
    delta.length()
}

/// Grows, regrows and drops the standing crop of every field patch and the
/// default grass on streamed terrain.
///
/// A patch measures itself once, then each of its cells grows when the camera
/// comes near it and drops again when it leaves. A new day regrows what it
/// changed. Both passes below are over every patch, and that is the point:
/// the budget has to be spent on the cells **nearest the camera**, not on
/// whichever patch the query happened to hand over first. Spent in query
/// order it fills a field five hundred metres away while the one under the
/// window stays bare — which is exactly what it did.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn update_field_plants(
    mut commands: Commands,
    assets: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    gltf_meshes: Res<Assets<GltfMesh>>,
    standards: Res<Assets<StandardMaterial>>,
    mut mips: ResMut<TextureMips>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<WeatherMaterial>>,
    mut grass_materials: ResMut<Assets<GrassMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut plants: ResMut<PlantMaterials>,
    mut models: ResMut<PlantModels>,
    grass: Res<GrassRenderSettings>,
    sky: Res<Sky>,
    mut surfaces: Query<(
        Entity,
        Option<&FieldSurface>,
        Option<&TerrainGrass>,
        &Mesh3d,
        &GlobalTransform,
        &mut FieldPlants,
    )>,
    // The camera that draws the *world*, not the first active one in it —
    // see `draws_the_world`.
    cameras: Query<(&Camera, &RenderTarget, Has<WorldView>, &GlobalTransform), With<Camera3d>>,
) {
    let Some((.., camera)) = cameras.iter().find(|(camera, target, world_view, _)| {
        crate::draws_the_world(camera, target, *world_view)
    }) else {
        return;
    };
    let eye = camera.translation();

    // The first pass measures, drops what the camera has left behind — which
    // costs nothing and so is not budgeted — and writes down what wants
    // growing, with the distance the queue is ordered on.
    let mut wanted: Vec<(f32, Entity, usize, Band)> = Vec::new();
    for (entity, field, terrain, mesh3d, at, mut state) in &mut surfaces {
        let terrain = field.is_none().then_some(terrain).flatten();
        let Some(crop) = field
            .map(|surface| surface.crop)
            .or_else(|| terrain.map(|_| CropClass::Grassland))
        else {
            continue;
        };
        // The patch's cells and its reach, measured once from its mesh:
        // everything after this is distance tests against boxes.
        if !state.surveyed {
            let Some(mesh) = meshes.get(&mesh3d.0) else {
                continue;
            };
            state.surveyed = true;
            if let Some(found) = survey(mesh) {
                state.centre = found.centre;
                state.radius = found.radius;
                state.week = if terrain.is_some() { 0.5 } else { found.week };
                state.cells = found.cells;
                if let Some(terrain) = terrain {
                    state
                        .cells
                        .retain(|cell| !terrain.triangles(cell.key).is_empty());
                }
            }
        }
        if state.cells.is_empty() {
            continue;
        }
        // The camera in the patch's own frame, so a cell's box can be
        // measured to without a transform each.
        let eye_local = at.affine().inverse().transform_point3(eye);
        let reach = if terrain.is_some() {
            eye_local.xz().distance(state.centre.xz()) - state.radius
        } else {
            eye_local.distance(state.centre) - state.radius
        };
        // Out of sight: the meshes go, the patch keeps its cells.
        if reach > DEMATERIALISE_AT {
            clear(&mut commands, &mut state);
            continue;
        }
        if reach > MATERIALISE_AT {
            continue;
        }

        // The crop's real plant, once its file has loaded. Until then the
        // patch grows as painted cards, and the missing flag in the key
        // brings it back here the frame the model lands. The stage picks the
        // model as well as the size — a cut field stands straw, not wheat.
        let mut key = CropKey::of(crop, sky.month, sky.day, state.week, false);
        // The crop calendar's grassland is cut three times a year. Default
        // verge and meadow terrain is not one synchronised managed field, so
        // it keeps the grass model and merely follows the seasonal height.
        if terrain.is_some() && key.stage == Stage::Stubble {
            key.stage = Stage::Growing;
        }
        key.heroes = terrain.is_none()
            && models
                .model(
                    crop,
                    key.stage,
                    &assets,
                    &gltfs,
                    &gltf_meshes,
                    &meshes,
                    &standards,
                    &mut mips,
                )
                .is_some();
        // Grown for the day already? The day's colour rode in with the
        // material; only stage, height, winter and a landed model rebuild.
        if state.grown != Some(key) {
            clear(&mut commands, &mut state);
            state.grown = Some(key);
        }

        for at in 0..state.cells.len() {
            let cell = &state.cells[at];
            let distance = if terrain.is_some() {
                planar_box_distance(eye_local, cell.lo, cell.hi)
            } else {
                box_distance(eye_local, cell.lo, cell.hi)
            };
            let want = if terrain.is_some() {
                terrain_band_for(distance, *grass)
            } else {
                band_for(distance, cell.band)
            };
            if want == cell.band {
                continue;
            }
            match want {
                Some(band) => wanted.push((distance, entity, at, band)),
                None => drop_cell(&mut commands, &mut state.cells[at]),
            }
        }
    }

    if wanted.is_empty() {
        return;
    }
    // Nearest first. Close terrain cells are over ten times heavier than the
    // coarser levels, so count work rather than cells: three close rebuilds in
    // one update caused a long main-thread spike whenever grass reappeared.
    wanted.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
    let mut work = 0;
    wanted.retain(|&(_, _, _, band)| {
        let cost = if band == Band::Close { 3 } else { 1 };
        if work + cost > BUILD_BUDGET {
            false
        } else {
            work += cost;
            true
        }
    });

    for (_, entity, at, band) in wanted {
        let Ok((entity, field, terrain, mesh3d, _, mut state)) = surfaces.get_mut(entity) else {
            continue;
        };
        let terrain = field.is_none().then_some(terrain).flatten();
        let Some(crop) = field
            .map(|surface| surface.crop)
            .or_else(|| terrain.map(|_| CropClass::Grassland))
        else {
            continue;
        };
        let Some(key) = state.grown else {
            continue;
        };
        drop_cell(&mut commands, &mut state.cells[at]);
        let model = if terrain.is_some() {
            None
        } else {
            models.model(
                crop,
                key.stage,
                &assets,
                &gltfs,
                &gltf_meshes,
                &meshes,
                &standards,
                &mut mips,
            )
        };
        let cards = {
            let Some(surface_mesh) = meshes.get(&mesh3d.0) else {
                continue;
            };
            grow(
                surface_mesh,
                crop,
                sky.month,
                sky.day,
                state.cells[at].key,
                model.as_deref(),
                key.stage,
                band,
                terrain,
            )
        };
        // The cell has had its turn either way: an empty one is a cell of
        // ploughed ground, and asking it again every frame would spend the
        // whole budget on the fields that grow nothing.
        state.cells[at].band = Some(band);
        if cards.is_empty() {
            continue;
        }

        // The painted cards' colour is the crop's stand colour, per day; the
        // real plants repaint themselves from it in their own vertices.
        let growth = phenology::growth(crop, sky.month, sky.day, 0);
        let stand = [
            linear(growth.color[0]),
            linear(growth.color[1]),
            linear(growth.color[2]),
        ];
        let material = if terrain.is_some() {
            plants.terrain(&mut materials, &mut images, sky.month, sky.day)
        } else {
            plants.get(crop, &mut materials, &mut images, sky.month, sky.day)
        };
        let blade_material = terrain
            .is_some()
            .then(|| plants.blades(crop, &mut grass_materials, sky.month, sky.day, grass.params));
        let sink = model_of(crop, key.stage).map_or(0.0, |hero| hero.sink);
        // One dressed material per part of the model. Both this and the
        // model above are cached lookups after the first cell of a crop.
        let skins: Vec<Option<Handle<WeatherMaterial>>> = model
            .as_ref()
            .map(|model| {
                model
                    .skins
                    .iter()
                    .map(|skin| models.dressed(skin, &standards, &mut materials))
                    .collect()
            })
            .unwrap_or_default();

        let mut spawned = Vec::new();
        commands.entity(entity).with_children(|parent| {
            if terrain.is_some() {
                // Three nested levels use the same actual tapered geometry,
                // only fewer blades. Their UVs carry the LOD class; the grass
                // shader selects them against the live camera per blade, not
                // against this cell entity.
                if let Some(blade_material) = &blade_material {
                    let mut combined = None;
                    for level in band.upwards() {
                        let detail = blade_mesh(&cards, level);
                        if detail.count_vertices() == 0 {
                            continue;
                        }
                        merge_grass_mesh(&mut combined, detail);

                        // Only the innermost level receives the segmented
                        // hero blades. They add curvature, varied leaf tips
                        // and another eighty-eight silhouettes per square metre at
                        // the camera, then disappear before the first normal
                        // LOD hand-over begins.
                        if level == Band::Close {
                            let hero_detail = curved_blade_mesh(&cards);
                            if hero_detail.count_vertices() > 0 {
                                merge_grass_mesh(&mut combined, hero_detail);
                            }
                        }
                    }
                    // All levels have identical vertex layouts and one material.
                    // Combining them preserves every blade while turning as many as
                    // four submissions per terrain cell into one draw call.
                    if let Some(combined) = combined {
                        let handle = meshes.add(combined);
                        let id = parent
                            .spawn((
                                Mesh3d(handle.clone()),
                                MeshMaterial3d(blade_material.clone()),
                                Transform::IDENTITY,
                                NotShadowCaster,
                            ))
                            .id();
                        spawned.push((id, handle.id()));
                    }
                }
            } else {
                // Field crops retain their specialised three-level stand.
                for level in band.upwards() {
                    let (start, end) = level.range();
                    let start = if level == band { 0.0 } else { start };
                    if level.crossed()
                        && let Some(model) = &model
                    {
                        for (skin, dressed) in skins.iter().enumerate() {
                            let Some(dressed) = dressed else {
                                continue;
                            };
                            let mesh = hero_mesh(model, skin, &cards, level, stand, sink);
                            if mesh.count_vertices() == 0 {
                                continue;
                            }
                            let handle = meshes.add(mesh);
                            spawned.push((
                                parent
                                    .spawn((
                                        Mesh3d(handle.clone()),
                                        MeshMaterial3d(dressed.clone()),
                                        Transform::IDENTITY,
                                        range(start, end),
                                    ))
                                    .id(),
                                handle.id(),
                            ));
                        }
                    }
                    let mesh = card_mesh(&cards, level, false);
                    if mesh.count_vertices() == 0 {
                        continue;
                    }
                    let handle = meshes.add(mesh);
                    spawned.push((
                        parent
                            .spawn((
                                Mesh3d(handle.clone()),
                                MeshMaterial3d(material.clone()),
                                Transform::IDENTITY,
                                range(start, end),
                                NotShadowCaster,
                            ))
                            .id(),
                        handle.id(),
                    ));
                }
            }
        });
        state.cells[at].lods = spawned;
    }
}

fn merge_grass_mesh(combined: &mut Option<Mesh>, mesh: Mesh) {
    if let Some(combined) = combined {
        // These meshes are all triangle lists with the same five attributes.
        // A mismatch is a programming error, not content that can vary at run time.
        combined
            .merge(&mesh)
            .expect("grass LOD meshes must have matching vertex layouts");
    } else {
        *combined = Some(mesh);
    }
}

/// The default meadow's close LOD as actual tapered, curved blade geometry.
///
/// Each scattered close sample expands into sixteen real tapered blades.
/// The closest level has a curved middle joint while the smaller distance
/// levels collapse to one pointed triangle. The wind shader pins every foot
/// and bends every tip. Distributing the feet across the sample's neighbourhood
/// makes one continuous sward instead of isolated radial tufts.
const GRASS_COLOR_RANGE: f32 = 1.5;

fn grass_color(value: [f32; 4]) -> [u8; 4] {
    let pack = |channel: f32| {
        ((channel.clamp(0.0, GRASS_COLOR_RANGE) / GRASS_COLOR_RANGE) * 255.0).round() as u8
    };
    [
        pack(value[0]),
        pack(value[1]),
        pack(value[2]),
        (value[3].clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}

fn grass_uv(lod: f32, random: f32) -> [u16; 2] {
    [
        ((lod / 3.0).clamp(0.0, 1.0) * 65_535.0).round() as u16,
        (random.clamp(0.0, 1.0) * 65_535.0).round() as u16,
    ]
}

fn leaf_uv(value: [f32; 2]) -> [u16; 2] {
    [
        (value[0].clamp(0.0, 1.0) * 65_535.0).round() as u16,
        (value[1].clamp(0.0, 1.0) * 65_535.0).round() as u16,
    ]
}

fn grass_data(uvs: Vec<[u16; 2]>, leaf_uvs: Vec<[u16; 2]>) -> VertexAttributeValues {
    debug_assert_eq!(uvs.len(), leaf_uvs.len());
    VertexAttributeValues::Unorm16x4(
        uvs.into_iter()
            .zip(leaf_uvs)
            .map(|([lod, random], [across, along])| [lod, random, across, along])
            .collect(),
    )
}

fn blade_mesh(cards: &[Card], band: Band) -> Mesh {
    let (blades_per_tuft, width_scale, spread_floor) = match band {
        Band::Close => (16u64, 0.72, 0.40),
        Band::Near => (7u64, 1.55, 0.38),
        Band::Far => (1u64, 2.35, 0.24),
    };
    let lod_code = match band {
        Band::Close => 0.0,
        Band::Near => 1.0,
        Band::Far => 2.0,
    };
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut colors = Vec::new();
    let mut uvs = Vec::new();
    let mut leaf_uvs = Vec::new();
    let mut indices = Vec::new();

    for card in cards.iter().filter(|card| !card.hero) {
        let up = if card.up.length_squared() > 0.5 {
            card.up.normalize()
        } else {
            Vec3::Y
        };
        let tangent = if up.y.abs() < 0.9 {
            up.cross(Vec3::Y).normalize()
        } else {
            up.cross(Vec3::X).normalize()
        };
        let bitangent = up.cross(tangent).normalize();

        for blade in 0..blades_per_tuft {
            let random = |salt: u64| draw(card.seed ^ blade.wrapping_mul(0x9E37), salt) as f32;
            let angle = card.yaw + blade as f32 * 2.399_963_1 + (random(41) - 0.5) * 0.7;
            let (sin, cos) = angle.sin_cos();
            let lateral = (tangent * cos + bitangent * sin).normalize();
            let facing = up.cross(lateral).normalize();

            // Blades start across the whole tussock instead of sharing one
            // pinched foot, which removes the evenly spaced "star" pattern.
            let foot_angle = card.yaw + blade as f32 * 2.399_963_1 + (random(42) - 0.5) * 0.35;
            let (foot_sin, foot_cos) = foot_angle.sin_cos();
            // The sampling point is only the deterministic centre of this
            // batch, not a visible tussock. Overlap neighbouring batches so
            // the close meadow becomes continuous instead of a field of
            // isolated stars.
            let spread = (card.width * 0.6).max(spread_floor);
            let radial_rank = (blade as f32 + 0.5 + random(43) * 0.2) / blades_per_tuft as f32;
            let foot_radius = radial_rank.min(1.0).sqrt() * spread;
            let foot =
                card.pos + (tangent * foot_cos + bitangent * foot_sin) * foot_radius + up * 0.004;

            let short_leaf = blade % 6 == 0;
            let height =
                card.height * (0.58 + random(44) * 0.52) * if short_leaf { 0.68 } else { 1.0 };
            let half_width = (0.0045 + height * 0.017)
                * (0.72 + random(45) * 0.56)
                * width_scale
                * if short_leaf { 1.45 } else { 1.0 };
            let curve = height * (0.045 + random(46) * 0.15 + card.lean * 0.32).clamp(-0.08, 0.22);
            let p0 = foot;
            let tip = foot + up * height + facing * curve;
            let width0 = lateral * half_width;

            // Mostly face the blade, but lift the authored normal enough for
            // skylight to keep the lower meadow readable. The two-sided PBR
            // material flips it correctly on the back face.
            let normal = (facing * 0.9 + up * 0.34).normalize();
            let hue = (random(47) - 0.5) * 0.2;
            let light = 0.88 + random(48) * 0.22;
            let shade = |value: f32, bend: f32| {
                let value = value * light;
                [value * (1.0 + hue), value, value * (1.0 - hue * 0.7), bend]
            };
            let base = positions.len() as u32;
            if band == Band::Close {
                let middle = foot + up * (height * 0.60) + facing * (curve * 0.30);
                let middle_width = lateral * (half_width * 0.52);
                positions.extend_from_slice(&[
                    (p0 - width0).to_array(),
                    (p0 + width0).to_array(),
                    (middle - middle_width).to_array(),
                    (middle + middle_width).to_array(),
                    tip.to_array(),
                ]);
                normals.extend_from_slice(&[[normal.x, normal.y, normal.z]; 5]);
                colors.extend_from_slice(
                    &[
                        shade(0.62, 0.0),
                        shade(0.62, 0.0),
                        shade(0.94, 0.58),
                        shade(0.94, 0.58),
                        shade(1.24, 1.0),
                    ]
                    .map(grass_color),
                );
                uvs.extend_from_slice(&[grass_uv(lod_code, random(49)); 5]);
                leaf_uvs.extend_from_slice(
                    &[
                        [0.0, 0.0],
                        [1.0, 0.0],
                        [0.24, 0.60],
                        [0.76, 0.60],
                        [0.5, 1.0],
                    ]
                    .map(leaf_uv),
                );
                indices.extend_from_slice(&[
                    base,
                    base + 1,
                    base + 3,
                    base,
                    base + 3,
                    base + 2,
                    base + 2,
                    base + 3,
                    base + 4,
                ]);
            } else {
                positions.extend_from_slice(&[
                    (p0 - width0).to_array(),
                    (p0 + width0).to_array(),
                    tip.to_array(),
                ]);
                normals.extend_from_slice(&[[normal.x, normal.y, normal.z]; 3]);
                colors.extend_from_slice(
                    &[shade(0.66, 0.0), shade(0.66, 0.0), shade(1.24, 1.0)].map(grass_color),
                );
                uvs.extend_from_slice(&[grass_uv(lod_code, random(49)); 3]);
                leaf_uvs.extend_from_slice(&[[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]].map(leaf_uv));
                indices.extend_from_slice(&[base, base + 1, base + 2]);
            }
        }
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(
        ATTRIBUTE_GRASS_COLOR,
        VertexAttributeValues::Unorm8x4(colors),
    );
    mesh.insert_attribute(ATTRIBUTE_GRASS_DATA, grass_data(uvs, leaf_uvs));
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Segmented hero blades for the camera's immediate surroundings.
///
/// The broad meadow still comes from [`blade_mesh`]. These thirty extra leaves
/// per sample are deliberately a little longer and carry a curved middle
/// joint, so the nearest grass has overlapping depth, bent tips and a more
/// organic wind silhouette without paying that geometry across the world.
fn curved_blade_mesh(cards: &[Card]) -> Mesh {
    // This layer exists only in the camera's immediate radial neighbourhood.
    // Thirty leaves per sample bring the local sward to roughly 184 individual
    // blades/m²; the coarse world never pays for them.
    const BLADES: u64 = 30;
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut colors = Vec::new();
    let mut uvs = Vec::new();
    let mut leaf_uvs = Vec::new();
    let mut indices = Vec::new();

    for card in cards.iter().filter(|card| !card.hero) {
        let up = if card.up.length_squared() > 0.5 {
            card.up.normalize()
        } else {
            Vec3::Y
        };
        let tangent = if up.y.abs() < 0.9 {
            up.cross(Vec3::Y).normalize()
        } else {
            up.cross(Vec3::X).normalize()
        };
        let bitangent = up.cross(tangent).normalize();

        for blade in 0..BLADES {
            let random = |salt: u64| {
                draw(
                    card.seed ^ 0xA24B_AED4_963E_E407 ^ blade.wrapping_mul(0x9E37),
                    salt,
                ) as f32
            };
            let angle = card.yaw + blade as f32 * 2.399_963_1 + (random(61) - 0.5) * 0.55;
            let (sin, cos) = angle.sin_cos();
            let lateral = (tangent * cos + bitangent * sin).normalize();
            let facing = up.cross(lateral).normalize();

            // A stratified disc overlaps neighbouring batches but never
            // pinches all leaves into the same visible star-shaped root.
            let foot_angle = angle + (random(62) - 0.5) * 0.7;
            let (foot_sin, foot_cos) = foot_angle.sin_cos();
            let radius = ((blade as f32 + 0.35 + random(63) * 0.3) / BLADES as f32).sqrt() * 0.34;
            let foot = card.pos + (tangent * foot_cos + bitangent * foot_sin) * radius + up * 0.006;

            let short = blade % 4 == 0;
            let height =
                (card.height * (0.72 + random(64) * 0.58) * if short { 0.64 } else { 1.0 })
                    .clamp(0.09, 0.34);
            let half_width = (0.004 + height * 0.014)
                * (0.76 + random(65) * 0.48)
                * if short { 1.25 } else { 1.0 };
            let curve = height * (0.07 + random(66) * 0.19 + card.lean.abs() * 0.22);
            let middle = foot + up * (height * 0.60) + facing * (curve * 0.30);
            let tip = foot + up * height + facing * curve;
            let root_width = lateral * half_width;
            let middle_width = lateral * (half_width * 0.52);

            let normal = (facing * 0.88 + up * 0.38).normalize();
            let hue = (random(67) - 0.5) * 0.24;
            let light = 0.90 + random(68) * 0.24;
            let shade = |value: f32, bend: f32| {
                let value = value * light;
                [value * (1.0 + hue), value, value * (1.0 - hue * 0.7), bend]
            };

            let base = positions.len() as u32;
            positions.extend_from_slice(&[
                (foot - root_width).to_array(),
                (foot + root_width).to_array(),
                (middle - middle_width).to_array(),
                (middle + middle_width).to_array(),
                tip.to_array(),
            ]);
            normals.extend_from_slice(&[[normal.x, normal.y, normal.z]; 5]);
            colors.extend_from_slice(
                &[
                    shade(0.58, 0.0),
                    shade(0.58, 0.0),
                    shade(0.92, 0.58),
                    shade(0.92, 0.58),
                    shade(1.28, 1.0),
                ]
                .map(grass_color),
            );
            // x = exact radial shader LOD (3 is immediate detail), y = one
            // stable random threshold shared by every vertex of this leaf.
            uvs.extend_from_slice(&[grass_uv(3.0, random(69)); 5]);
            leaf_uvs.extend_from_slice(
                &[
                    [0.0, 0.0],
                    [1.0, 0.0],
                    [0.24, 0.60],
                    [0.76, 0.60],
                    [0.5, 1.0],
                ]
                .map(leaf_uv),
            );
            indices.extend_from_slice(&[
                base,
                base + 1,
                base + 3,
                base,
                base + 3,
                base + 2,
                base + 2,
                base + 3,
                base + 4,
            ]);
        }
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(
        ATTRIBUTE_GRASS_COLOR,
        VertexAttributeValues::Unorm8x4(colors),
    );
    mesh.insert_attribute(ATTRIBUTE_GRASS_DATA, grass_data(uvs, leaf_uvs));
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// One card as quads: two crossed at a right angle for the near level, one
/// wider one for the far level, where half the cards are kept.
///
/// The near level leaves the hero cards out — a real plant stands there, and
/// a quad inside it is geometry nobody sees. The far level keeps them: the
/// models are gone by then.
///
/// The normal is one per quad and **mostly upright**: the ground's own normal
/// leant a little into the quad's facing. A card is a stand-in for a plant,
/// and a plant is lit by the sky it stands under — a purely horizontal
/// normal, which is what a quad's geometry says, takes almost no light at
/// all from a July sun overhead and turns a maize field black. The little
/// that is left of the facing keeps one side of a crossed pair darker than
/// the other, which is what stops a stand reading as flat.
///
/// One normal per quad rather than per corner: per corner lights the two
/// halves of a quad differently and splits every plant down the middle. And
/// the material is drawn from both sides *without* `double_sided`, so the far
/// side of a card keeps this normal instead of its negation — a card seen
/// from behind is the same plant in the same light, not a hole in the field.
fn card_mesh(cards: &[Card], band: Band, terrain: bool) -> Mesh {
    let (_, field_width) = band.stand();
    // Far terrain uses many modest overlapping tufts instead of a handful of
    // enormous crosses. Their projected coverage matches the middle blade
    // LOD, so the hand-over changes detail rather than turning grass off.
    let wider = if terrain { 4.2 } else { field_width };
    let crossed = terrain || band.crossed();
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut colors = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();

    for card in cards {
        let kept = if terrain { true } else { band.keeps(card) };
        if !kept {
            continue;
        }
        // A real plant stands here at the levels that draw them, and a quad
        // inside a model is geometry nobody sees. Past them the card is all
        // there is.
        if crossed && card.hero {
            continue;
        }
        let up = if card.up.length_squared() > 0.5 {
            card.up.normalize()
        } else {
            Vec3::Y
        };
        let (sin, cos) = card.yaw.sin_cos();
        let width = card.width * wider;
        // The tuft this card is cut out of, mirrored by the top bit.
        let tuft = (card.tuft as usize % SHEET_TUFTS) as f32 / SHEET_TUFTS as f32;
        let step = 1.0 / SHEET_TUFTS as f32;
        let (u0, u1) = if card.tuft as usize >= SHEET_TUFTS {
            (tuft + step, tuft)
        } else {
            (tuft, tuft + step)
        };
        // One quad across the working direction, one along it, both turned a
        // little by the card's own yaw — and the top of each leans a little
        // out of true, so a stand is never a picket fence.
        for (qi, axis) in [Vec3::new(cos, 0.0, sin), Vec3::new(-sin, 0.0, cos)]
            .into_iter()
            .enumerate()
        {
            let normal = (up * 0.7 + Vec3::new(-axis.z, 0.0, axis.x) * 0.72).normalize();
            let sway = axis * (card.lean * card.height * if qi == 0 { -1.0 } else { 1.0 });
            let foot_a = card.pos - axis * (width * 0.5);
            let foot_b = card.pos + axis * (width * 0.5);
            let top_a = foot_a + up * card.height + sway;
            let top_b = foot_b + up * card.height + sway;
            // The base lives in the stand's own shade and the head catches
            // the sky; both ride the field's tint and the tuft's own light,
            // so a card matches the painted field it stands on.
            // A stand is darker than the paint it grows out of, never
            // lighter: the head catches the sky and the foot lives in the
            // shade of everything above it, and no card casts a shadow to
            // say so.
            let dark = (0.36 + 0.20 * card.light) * (0.9 + 0.2 * card.tint);
            let bright = (0.82 + 0.22 * card.light) * (0.92 + 0.18 * card.tint);
            // A stand is never one green: some tufts run to yellow, some
            // stay blue, and the eye reads the spread as depth. Small —
            // the crop's colour is the phenology's, not the hash's.
            let hue = (card.light - 0.5) * 0.14;
            let shade = |v: f32| [v * (1.0 + hue), v, v * (1.0 - hue * 0.8), 1.0];
            let base = positions.len() as u32;
            positions.extend_from_slice(&[
                foot_a.to_array(),
                foot_b.to_array(),
                top_b.to_array(),
                top_a.to_array(),
            ]);
            normals.extend_from_slice(&[[normal.x, normal.y, normal.z]; 4]);
            uvs.extend_from_slice(&[[u0, 1.0], [u1, 1.0], [u1, 0.0], [u0, 0.0]]);
            colors.extend_from_slice(&[shade(dark), shade(dark), shade(bright), shade(bright)]);
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            // The far level draws one quad of the cross.
            if !crossed {
                break;
            }
        }
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// One material part's merged mesh, every hero card of a cell baked into it:
/// a transformed copy of the model's geometry per hero card, taking the
/// variant the card drew. The models stand one metre tall on their own feet,
/// so the card's height *is* the scale, and its yaw and lean pose it.
///
/// The day's colour rides in the vertex colour. An untextured part is
/// repainted — its material is white, and what the plant wears is the stand
/// colour times the part's own [tint](PlantSkin::tint) times the base-to-head
/// gradient, so a wheat model is blue-green in May and gold in July while a
/// corn cob stays yellower than the stalk it hangs on. A textured part keeps
/// its own picture and takes only the gradient and the tuft's light, the same
/// shading the painted cards get.
fn hero_mesh(
    model: &PlantModel,
    skin: usize,
    cards: &[Card],
    band: Band,
    stand: [f32; 3],
    sink: f32,
) -> Mesh {
    let textured = model.skins[skin].textured;
    let tint = model.skins[skin].tint;
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut colors = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();

    // The models thin with the level the cards do — the same threshold, so a
    // plant that stood at thirty metres is still standing at sixty.
    for card in cards.iter().filter(|c| c.hero && band.keeps(c)) {
        let variant = &model.variants[(card.variant as usize).min(model.variants.len() - 1)];
        let (sin, cos) = card.yaw.sin_cos();
        // The stand height is the height of what *shows*, so a model with a
        // root under the soil is grown past it and then sunk back.
        let scale = card.height.max(0.02) / (1.0 - sink).max(0.05);
        let foot = -sink * scale;
        // The lean shears the model along its own facing, so a plant tips
        // without its foot leaving the ground.
        let lean_axis = [cos, 0.0, sin];
        let rotate = |x: f32, z: f32| [x * cos - z * sin, x * sin + z * cos];
        for part in variant.parts.iter().filter(|p| p.material == skin) {
            let base = positions.len() as u32;
            for (i, p) in part.positions.iter().enumerate() {
                let [rx, rz] = rotate(p[0], p[2]);
                let shear = card.lean * p[1] * scale;
                positions.push([
                    card.pos[0] + rx * scale + lean_axis[0] * shear,
                    card.pos[1] + foot + p[1] * scale,
                    card.pos[2] + rz * scale + lean_axis[2] * shear,
                ]);
                let n = part.normals[i];
                let [nx, nz] = rotate(n[0], n[2]);
                normals.push([nx, n[1], nz]);
                // The base lives in the stand's shade, the head catches the
                // sky; both ride the field's tint and the tuft's own light.
                let gradient = 0.55 + 0.5 * p[1].clamp(0.0, 1.0);
                let shade = gradient * (0.9 + 0.2 * card.tint) * (0.85 + 0.3 * card.light);
                let own = part.colors.as_ref().map(|c| c[i]).unwrap_or([1.0; 4]);
                let rgb = if textured {
                    [own[0] * shade, own[1] * shade, own[2] * shade]
                } else {
                    [
                        stand[0] * tint[0] * shade,
                        stand[1] * tint[1] * shade,
                        stand[2] * tint[2] * shade,
                    ]
                };
                colors.push([rgb[0], rgb[1], rgb[2], own[3].clamp(0.0, 1.0)]);
                if let Some(uv) = &part.uvs {
                    uvs.push(uv[i]);
                }
            }
            for i in &part.indices {
                indices.push(base + i);
            }
        }
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    if !uvs.is_empty() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    }
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// How many tufts one card sheet holds. A card picks one and may mirror it,
/// so eight silhouettes stand in a field — enough that no two neighbours are
/// the same and few enough that the sheet is a quarter of a megabyte.
const SHEET_TUFTS: usize = 4;
/// Where a card's alpha is cut.
///
/// Below a half, deliberately: a blade tapers to less than a texel at its tip,
/// and at a half the tip dashes in and out of the cutoff from one mip level to
/// the next — a stand seen against the sky breaks into dotted lines. The mip
/// chain is built to hold coverage at the same figure, so the two agree.
const CARD_CUTOFF: f32 = 0.35;

/// One tuft's cell on the sheet [px] — **taller than it is wide**, because a
/// plant is. A square picture on a card twice as tall as it is broad
/// stretches every blade to twice its width, and that is what made a wheat
/// field read as pampas grass; the cell's own aspect is close to what
/// [`card_width`] gives the crops, so the picture arrives roughly unstretched.
const SHEET_TUFT_W: usize = 96;
const SHEET_TUFT_H: usize = 224;

/// The two shapes a card can be cut out of.
///
/// A wheat field and a maize field are not the same green rectangle at
/// different sizes: one is a thicket of stiff blades with ears on them, the
/// other a stand of broad leaves arching over. One sheet each is what tells
/// them apart at the distance where the real models have gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Leaf {
    /// Dense, full-width fine blades for the invariant default-terrain mat.
    Meadow,
    /// Thin, stiff, upright, a third of them carrying an ear — cereal,
    /// grass, whatever a meadow is made of.
    Blade,
    /// Broad and arching — maize, rape, the vine, anything that carries a
    /// long leaf up a stem.
    Broad,
    /// A few big leaves fanning out low — a beet's rosette, a potato haulm, a
    /// lettuce. On a maize sheet their leaves came out two centimetres across
    /// where the real thing is a hand's width.
    Rosette,
}

/// Which of them a crop is cut out of.
fn leaf_of(crop: CropClass) -> Leaf {
    match crop {
        CropClass::SugarBeet | CropClass::Potato | CropClass::Vegetable => Leaf::Rosette,
        CropClass::Maize
        | CropClass::Rapeseed
        | CropClass::Legume
        | CropClass::Orchard
        | CropClass::Vineyard => Leaf::Broad,
        _ => Leaf::Blade,
    }
}

/// A card sheet: [`SHEET_TUFTS`] tufts in a row, drawn once and shared by
/// every crop of the same [`Leaf`].
///
/// Without one a card is a **solid rectangle**, and a stand of solid
/// rectangles is a hedge — which is exactly what a maize field looked like.
/// The silhouette is the whole point of a card: what makes a field read as a
/// field, and not as a painted wall, is the light coming through it.
///
/// White, because the crop's own colour is the material's and the
/// base-to-head shading is the card's vertex colour; the sheet carries the
/// *shape* and nothing else. Drawn rather than shipped: a hundred lines of
/// blades beat a PNG in the repository that nobody can regenerate.
fn card_sheet(leaf: Leaf) -> Image {
    let (width, height) = (SHEET_TUFTS * SHEET_TUFT_W, SHEET_TUFT_H);
    let mut rgb = vec![0.0f32; width * height];
    let mut alpha = vec![0.0f32; width * height];
    // Count, how far they stand apart, how thick they start, how fast they
    // taper, how far they arch over, and whether they carry an ear. The
    // thicknesses are what decide the scale of the whole stand: at the card
    // widths `card_width` gives, a blade comes out 6 to 14 mm across and a
    // maize leaf 50 to 105 — which is what they measure in a field.
    let (count, spread, thick, taper, arch, ears) = match leaf {
        Leaf::Meadow => (58u64, 0.48, (0.65f32, 1.35f32), 0.78f32, 0.72f32, 0.0),
        Leaf::Blade => (26u64, 0.34, (0.8f32, 1.9f32), 0.8f32, 0.55f32, 0.35),
        Leaf::Broad => (13, 0.42, (2.4, 5.0), 1.3, 1.0, 0.0),
        Leaf::Rosette => (9, 0.44, (6.5, 12.0), 1.7, 1.7, 0.0),
    };
    for tuft in 0..SHEET_TUFTS {
        let salt = 0x5EAF + tuft as u64 * 0x9E37 + leaf as u64 * 0x2C1B;
        let left = (tuft * SHEET_TUFT_W) as f32;
        // The blades stay off the edges of their cell, so what a mip level
        // smears across the seam is empty rather than a neighbour's leaf.
        let centre = left + SHEET_TUFT_W as f32 * 0.5;
        let spread = SHEET_TUFT_W as f32 * spread;
        for blade in 0..count {
            let r = |n: u64| draw(blade * 31 + n, salt) as f32;
            let base_half = thick.0 + (thick.1 - thick.0) * r(4);
            // Nothing may reach the seam: a card samples one cell of the
            // sheet, so a leaf that grows over the edge is a leaf cut in half
            // on one card and sprouting out of nothing on the next.
            let room = SHEET_TUFT_W as f32 * 0.5 - 1.5 - base_half;
            let inside = |x: f32| centre + (x - centre).clamp(-room, room);
            let foot = inside(centre + (r(1) - 0.5) * 2.0 * spread);
            // A blade that starts at the edge of the tuft is a short one: a
            // tuft is thickest and tallest in the middle.
            let edge = 1.0 - ((foot - centre) / spread).abs() * 0.45;
            let length = (0.45 + 0.55 * r(2)) * edge * (SHEET_TUFT_H as f32 - 4.0);
            let bend = inside(foot + (r(3) - 0.5) * 2.0 * spread * arch) - foot;
            // Every third blade carries an ear: a cereal's head is what tells
            // a wheat field from a meadow at twenty metres.
            let ear = r(5) < ears;
            let tone = 0.74 + 0.26 * r(6);
            let rows = length.max(2.0) as usize;
            for row in 0..=rows {
                let t = row as f32 / rows as f32;
                let y = height as f32 - 2.0 - length * t;
                // The blade curves over rather than leaning: a stalk is stiff
                // at the foot and gives at the head.
                let x = foot + bend * t * t;
                let mut half = base_half * (1.0 - t).powf(taper);
                if ear && t > 0.62 {
                    // The head swells and then tapers into the awn.
                    let s = (t - 0.62) / 0.38;
                    half = half.max(base_half * 0.85 * (1.0 - (s * 2.0 - 1.0).abs()).max(0.12));
                }
                stamp(&mut rgb, &mut alpha, width, height, x, y, half, tone);
            }
        }
    }
    let mut data = Vec::with_capacity(width * height * 4);
    for i in 0..width * height {
        let a = alpha[i].clamp(0.0, 1.0);
        // The colour of a fully transparent texel is still averaged into the
        // mip chain, so it has to be a leaf's rather than black — a black
        // fringe round every blade is what that costs.
        let tone = if a > 1e-4 {
            (rgb[i] / a).clamp(0.0, 1.0)
        } else {
            0.88
        };
        let level = (tone * 255.0) as u8;
        data.extend_from_slice(&[level, level, level, (a * 255.0) as u8]);
    }
    let mut image = Image::new(
        Extent3d {
            width: width as u32,
            height: height as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    // The chain keeps the alpha's coverage: without it a stand thins out with
    // distance until the field is bare paint, which is the lesson the trees
    // taught and the reason `TextureMips` exists at all.
    crate::build_mip_chain(&mut image, Some(CARD_CUTOFF));
    image
}

/// Lays one row of a blade down, antialiased across its width.
#[allow(clippy::too_many_arguments)]
fn stamp(
    rgb: &mut [f32],
    alpha: &mut [f32],
    width: usize,
    height: usize,
    x: f32,
    y: f32,
    half: f32,
    tone: f32,
) {
    let row = y as isize;
    if row < 0 || row >= height as isize {
        return;
    }
    let from = (x - half - 1.0).floor() as isize;
    let to = (x + half + 1.0).ceil() as isize;
    for col in from..=to {
        if col < 0 || col >= width as isize {
            continue;
        }
        // Coverage of the pixel by the blade's half width, softened over the
        // outermost texel so the cut-out edge has something to resolve.
        let cover = (half + 0.5 - (col as f32 + 0.5 - x).abs()).clamp(0.0, 1.0);
        if cover <= 0.0 {
            continue;
        }
        let at = row as usize * width + col as usize;
        // Over, not add: two blades crossing are one leaf, not a bright one.
        let was = alpha[at];
        alpha[at] = was.max(cover);
        if alpha[at] > was || rgb[at] <= 0.0 {
            rgb[at] = tone * alpha[at];
        }
    }
}

/// The visibility band of one level, measured to the mesh's own bounds: a
/// cell's cards may stand five hundred metres from the tile's origin, and the
/// origin is what a range without `use_aabb` measures to.
fn range(start: f32, end: f32) -> VisibilityRange {
    const FADE: f32 = 12.0;
    range_with_fade(start, end, FADE)
}

fn range_with_fade(start: f32, end: f32, fade: f32) -> VisibilityRange {
    VisibilityRange {
        // The two neighbouring meshes overlap through complementary screen-
        // door dithering. A whole meadow no longer pops between real blades
        // and cards on one frame as the camera crosses a round number.
        start_margin: if start > 0.0 {
            (start - fade)..(start + fade)
        } else {
            0.0..0.0
        },
        end_margin: (end - fade)..(end + fade),
        use_aabb: true,
    }
}

/// A patch's triangles, whichever width its indices are.
///
/// Read where they lie rather than widened into a copy: every cell of a
/// patch walks the same list, and a tile of farmland is tens of thousands of
/// triangles that nobody needs a second time.
enum Tris<'a> {
    U16(&'a [u16]),
    U32(&'a [u32]),
}

impl<'a> Tris<'a> {
    fn of(mesh: &'a Mesh) -> Option<Self> {
        Some(match mesh.indices()? {
            Indices::U16(v) => Tris::U16(v),
            Indices::U32(v) => Tris::U32(v),
        })
    }

    fn len(&self) -> usize {
        match self {
            Tris::U16(v) => v.len() / 3,
            Tris::U32(v) => v.len() / 3,
        }
    }

    fn at(&self, i: usize) -> [usize; 3] {
        match self {
            Tris::U16(v) => [
                v[i * 3] as usize,
                v[i * 3 + 1] as usize,
                v[i * 3 + 2] as usize,
            ],
            Tris::U32(v) => [
                v[i * 3] as usize,
                v[i * 3 + 1] as usize,
                v[i * 3 + 2] as usize,
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The number of indices a mesh carries.
    fn index_count(mesh: &Mesh) -> usize {
        mesh.indices().map(Indices::len).unwrap_or(0)
    }

    #[test]
    fn overlay_holes_accept_both_windings_and_reject_the_outside() {
        let clockwise = [Vec2::ZERO, Vec2::Y, Vec2::ONE];
        let anticlockwise = [Vec2::ZERO, Vec2::ONE, Vec2::Y];
        for triangle in [clockwise, anticlockwise] {
            assert!(point_in_triangle(Vec2::new(0.25, 0.5), triangle));
            assert!(point_in_triangle(Vec2::new(0.0, 0.5), triangle));
            assert!(!point_in_triangle(Vec2::new(0.75, 0.25), triangle));
        }
    }

    /// One square of ground, `size` metres on a side, as a patch's mesh —
    /// two triangles, flat, with the vertex colours a field piece carries.
    fn patch(size: f32) -> Mesh {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![
                [0.0, 0.0, 0.0],
                [size, 0.0, 0.0],
                [size, 0.0, -size],
                [0.0, 0.0, -size],
            ],
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; 4]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; 4]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.5, 0.5, 0.5, 1.0]; 4]);
        mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
        mesh
    }

    /// Every cell of a patch, grown — what the renderer builds one cell at a
    /// time as a camera walks over it.
    fn grow_all(mesh: &Mesh, crop: CropClass, month: u32, day: u32) -> Vec<Card> {
        let survey = survey(mesh).expect("the patch surveys");
        let stage = phenology::growth_offset(
            crop,
            phenology::day_of_year(month, day),
            (survey.week * 2.0 - 1.0) * 7.0,
        )
        .stage;
        survey
            .cells
            .iter()
            .flat_map(|cell| {
                grow(
                    mesh,
                    crop,
                    month,
                    day,
                    cell.key,
                    None,
                    stage,
                    Band::Close,
                    None,
                )
            })
            .collect()
    }

    #[test]
    fn a_ripe_stand_grows_where_bare_ground_grows_nothing() {
        // Mid-July wheat stands, and its cards are as tall as the calendar
        // says; the same ground ploughed bare grows nothing at all.
        let mesh = patch(40.0);
        let ripe = grow_all(&mesh, CropClass::WinterCereal, 7, 15);
        assert!(!ripe.is_empty());
        for card in &ripe {
            assert!(card.height > 0.5, "{card:?}");
        }
        let bare = grow_all(&patch(40.0), CropClass::Maize, 4, 1);
        assert!(bare.is_empty(), "nothing stands on ploughed ground");
    }

    #[test]
    fn a_stand_is_at_field_density_however_big_the_field() {
        // The cap used to be spread over the whole patch, so a tile of wheat
        // grew a tuft every seven metres and called it a field. Cells are
        // what fixed that: a square kilometre stands as thickly as a garden.
        for size in [40.0f32, 200.0, 1_000.0] {
            let cards = grow_all(&patch(size), CropClass::WinterCereal, 7, 15);
            let per_m2 = cards.len() as f32 / (size * size);
            assert!(
                per_m2 > density(CropClass::WinterCereal) * 0.7,
                "{size} m: {per_m2} per m²",
            );
        }
    }

    #[test]
    fn a_cell_does_not_overrun_the_cap() {
        // One cell at the thickest crop's density is well under the cap, and
        // a cell that somehow wanted more would stretch its spacing instead.
        let mesh = patch(1_000.0);
        let survey = survey(&mesh).unwrap();
        for cell in &survey.cells {
            let cards = grow(
                &mesh,
                CropClass::WinterCereal,
                7,
                15,
                cell.key,
                None,
                Stage::Ripe,
                Band::Close,
                None,
            );
            assert!(
                cards.len() <= MAX_CARDS,
                "{} in {:?}",
                cards.len(),
                cell.key
            );
        }
    }

    #[test]
    fn every_card_stands_in_its_own_cell() {
        // The cells partition the patch's triangles exactly: no card falls
        // outside the cell that grew it, so none is grown twice and no seam
        // between two cells is bare.
        let mesh = patch(120.0);
        let survey = survey(&mesh).unwrap();
        for cell in &survey.cells {
            let min = cell.key.as_vec2() * CELL;
            for card in grow(
                &mesh,
                CropClass::Maize,
                8,
                1,
                cell.key,
                None,
                Stage::Flowering,
                Band::Close,
                None,
            ) {
                let at = card.pos.xz();
                assert!(
                    at.x >= min.x - 1e-3
                        && at.x <= min.x + CELL + 1e-3
                        && at.y >= min.y - 1e-3
                        && at.y <= min.y + CELL + 1e-3,
                    "{at:?} is not in {:?}",
                    cell.key,
                );
            }
        }
    }

    #[test]
    fn the_coarse_levels_are_the_fine_one_thinned() {
        let mesh = patch(40.0);
        let cards = grow_all(&mesh, CropClass::WinterCereal, 7, 15);
        // Nested, so no card ever moves as a camera walks up to it: what the
        // far level draws the near level draws, and what the near level
        // draws the close one does.
        for card in &cards {
            assert!(Band::Close.keeps(card), "the close level keeps everything");
            if Band::Far.keeps(card) {
                assert!(Band::Near.keeps(card), "{card:?} skips a level");
            }
        }
        // Two quads a card near, one far, and none where a model stands
        // except at the level that has no models.
        let heroes = cards.iter().filter(|c| c.hero).count();
        let quads = |band: Band| index_count(&card_mesh(&cards, band, false)) / 6;
        let kept = |band: Band| cards.iter().filter(|c| band.keeps(c)).count();
        assert_eq!(quads(Band::Close), (kept(Band::Close) - heroes) * 2);
        assert_eq!(quads(Band::Far), kept(Band::Far));
        // And the thinning is the draw's, not a hash quirk.
        for band in [Band::Near, Band::Far] {
            let share = kept(band) as f32 / cards.len() as f32;
            let wanted = band.stand().0;
            assert!(
                (share - wanted).abs() < 0.06,
                "{band:?} kept {share} of the stand, not {wanted}",
            );
        }
    }

    #[test]
    fn the_stand_thins_with_the_angle_it_is_seen_at() {
        // A card is a vertical sheet, so what it hides depends on how far
        // down you are looking at it. Closing the ground ten metres away
        // takes several times the cards that closing it a hundred away does,
        // and spending the near figure everywhere is tens of megabytes of
        // quads behind the second row. So the levels fall — but in order, and
        // never off a cliff.
        let closure = |band: Band| {
            let (keep, wider) = band.stand();
            keep * wider * if band.crossed() { 2.0 } else { 1.0 }
        };
        let (close, near, far) = (
            closure(Band::Close),
            closure(Band::Near),
            closure(Band::Far),
        );
        assert!(near < close && far < near, "{close} {near} {far}");
        assert!(
            (0.35..0.75).contains(&(near / close)),
            "near is {} of close",
            near / close,
        );
        assert!(
            (0.35..0.75).contains(&(far / near)),
            "far is {} of near",
            far / near,
        );
    }

    #[test]
    fn a_card_is_two_quads_with_a_base_to_head_gradient() {
        let cards = grow_all(&patch(40.0), CropClass::WinterCereal, 7, 15);
        let mesh = card_mesh(&cards, Band::Close, false);
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        let colors = match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
            Some(VertexAttributeValues::Float32x4(c)) => c,
            _ => panic!("colours are float quads"),
        };
        // Every card is four vertices, two dark at the foot and two light at
        // the head, and the indices stay inside the mesh.
        assert_eq!(positions.len() % 4, 0);
        for quad in colors.as_chunks::<4>().0 {
            assert!(quad[0][0] < quad[2][0], "base darker than head");
        }
        if let Indices::U32(indices) = mesh.indices().unwrap() {
            assert!(indices.iter().all(|&i| (i as usize) < positions.len()));
        }
    }

    /// A model of `variants` plants, each a triangle of its own height, in
    /// one material.
    fn model(variants: usize) -> PlantModel {
        PlantModel {
            variants: (0..variants)
                .map(|i| {
                    let h = 1.0 - i as f32 * 0.25;
                    PlantVariant {
                        parts: vec![PlantPart {
                            positions: vec![[0.0, 0.0, 0.0], [0.0, h, 0.0], [0.3, h, 0.0]],
                            normals: vec![[0.0, 1.0, 0.0]; 3],
                            colors: None,
                            uvs: None,
                            indices: vec![0, 1, 2],
                            material: 0,
                        }],
                        tris: 1,
                    }
                })
                .collect(),
            skins: vec![PlantSkin {
                handle: Handle::default(),
                textured: false,
                tint: [1.0; 3],
            }],
            tris: 1,
        }
    }

    fn card_at(height: f32) -> Card {
        Card {
            seed: 17,
            pos: Vec3::new(10.0, 0.0, -5.0),
            up: Vec3::Y,
            yaw: 0.0,
            width: 1.0,
            height,
            lean: 0.0,
            tint: 0.5,
            light: 0.5,
            rank: 0,
            hero: true,
            tuft: 0,
            variant: 0,
        }
    }

    #[test]
    fn a_hero_card_wears_the_real_plant() {
        // One hero card at 80 cm: the baked mesh is the model scaled, yawed
        // and repainted into the day's stand colour.
        let model = model(1);
        let mesh = hero_mesh(
            &model,
            0,
            &[card_at(0.8)],
            Band::Close,
            [0.2, 0.4, 0.1],
            0.0,
        );
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        assert_eq!(positions.len(), 3);
        // The foot at the card, the head at the card plus the stand height.
        assert_eq!(positions[0], [10.0, 0.0, -5.0]);
        assert_eq!(positions[1], [10.0, 0.8, -5.0]);
        // The vertex colour is the stand colour times the gradient: darker
        // at the foot, the stand colour at the head.
        let colors = match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
            Some(VertexAttributeValues::Float32x4(c)) => c,
            _ => panic!("colours are float quads"),
        };
        assert!(colors[0][1] < 0.4 * 0.8 + 1e-6, "foot darker than head");
        assert!(
            (colors[1][1] - 0.4 * 1.05).abs() < 0.2,
            "head near the stand colour",
        );
        // Not a hero: nothing is baked.
        let plain = Card {
            hero: false,
            ..card_at(0.8)
        };
        let empty = hero_mesh(&model, 0, &[plain], Band::Close, [0.2, 0.4, 0.1], 0.0);
        assert!(
            empty
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .unwrap()
                .as_float3()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn close_meadow_tufts_are_real_blade_geometry() {
        let card = Card {
            hero: false,
            ..card_at(0.34)
        };
        let mesh = blade_mesh(&[card], Band::Close);
        assert_eq!(mesh.count_vertices(), 16 * 5);
        assert_eq!(mesh.indices().unwrap().len(), 16 * 9);
        let data = match mesh.attribute(ATTRIBUTE_GRASS_DATA) {
            Some(VertexAttributeValues::Unorm16x4(data)) => data,
            _ => panic!("blade data carries radial LOD data"),
        };
        assert_eq!(data[0][0], 0, "close LOD is encoded per blade");
        assert_eq!(
            data[0][..2],
            data[1][..2],
            "one blade has one stable fade draw"
        );
    }

    #[test]
    fn immediate_meadow_detail_has_curved_segmented_blades() {
        let card = Card {
            hero: false,
            ..card_at(0.24)
        };
        let mesh = curved_blade_mesh(&[card]);
        assert_eq!(mesh.count_vertices(), 30 * 5);
        assert_eq!(mesh.indices().unwrap().len(), 30 * 9);
        let colors = match mesh.attribute(ATTRIBUTE_GRASS_COLOR) {
            Some(VertexAttributeValues::Unorm8x4(colors)) => colors,
            _ => panic!("blade colours carry their bend weights"),
        };
        assert_eq!(colors[0][3], 0, "roots stay pinned in the wind");
        assert_eq!(colors[4][3], 255, "tips take the full wind bend");
    }

    #[test]
    fn a_buried_model_shows_the_height_it_is_asked_for() {
        // A beet is a rosette with a root under it. Sinking the model must
        // not shorten what stands above the soil: the stand height is the
        // height of what shows, whatever is under it.
        let model = model(1);
        let mesh = hero_mesh(
            &model,
            0,
            &[card_at(0.5)],
            Band::Close,
            [0.2, 0.4, 0.1],
            0.3,
        );
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        assert!(positions[0][1] < -0.1, "the root is in the soil");
        assert!(
            (positions[1][1] - 0.5).abs() < 1e-5,
            "and the leaves reach the stand height: {:?}",
            positions[1],
        );
    }

    #[test]
    fn a_card_stands_the_variant_it_drew() {
        // A pack of several plants is several plants, not one clump of them:
        // two cards that drew different variants stand different models.
        let model = model(3);
        let tall = Card {
            variant: 0,
            ..card_at(1.0)
        };
        let short = Card {
            variant: 2,
            ..card_at(1.0)
        };
        let head = |card| {
            hero_mesh(&model, 0, &[card], Band::Close, [0.2, 0.4, 0.1], 0.0)
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .unwrap()
                .as_float3()
                .unwrap()[1][1]
        };
        assert!(head(tall) > head(short), "the variants differ in height");
        // A variant index past the end of a model that shipped fewer of them
        // than the card drew for falls back rather than panicking.
        let over = Card {
            variant: 200,
            ..card_at(1.0)
        };
        assert!(head(over) > 0.0);
    }

    #[test]
    fn a_models_parts_keep_their_colours_apart() {
        // The stand colour repaints an untextured plant, but not into one
        // flat colour: the corn model's cob is twice as bright as its stalk
        // and stays twice as bright, and it is the model's *average* that
        // becomes the day's stand colour.
        let plain = || PlantSkin {
            handle: Handle::default(),
            textured: false,
            tint: [1.0; 3],
        };
        let mut skins = vec![plain(), plain()];
        // The corn model as the normaliser writes it: a green stalk of 566
        // triangles and a yellow cob of 168.
        tint_skins(&mut skins, &[(566, [0.122; 3]), (168, [0.284; 3])]);
        let (stalk, cob) = (skins[0].tint[0], skins[1].tint[0]);
        assert!(
            (cob / stalk - 0.284 / 0.122).abs() < 1e-3,
            "the cob keeps its distance from the stalk: {cob} / {stalk}",
        );
        // And the parts average out to the stand colour itself, so a wheat
        // field is the colour the phenology says it is.
        let average = (566.0 * stalk + 168.0 * cob) / 734.0;
        assert!((average - 1.0).abs() < 1e-3, "{average}");
    }

    #[test]
    fn a_wild_part_is_reined_in() {
        // A part twenty times brighter than the model's average would blow
        // out the moment the stand goes gold.
        let mut skins = vec![
            PlantSkin {
                handle: Handle::default(),
                textured: false,
                tint: [1.0; 3],
            },
            PlantSkin {
                handle: Handle::default(),
                textured: false,
                tint: [1.0; 3],
            },
        ];
        tint_skins(&mut skins, &[(999, [0.01; 3]), (1, [1.0; 3])]);
        assert!(skins[1].tint[0] <= 2.8, "{:?}", skins[1].tint);
        assert!(skins[0].tint[0] >= 0.35, "{:?}", skins[0].tint);
    }

    #[test]
    fn a_textured_part_keeps_its_picture() {
        // Only the plain parts are repainted; a leaf sheet is what it is,
        // and it must not drag the average of the others about either.
        let mut skins = vec![
            PlantSkin {
                handle: Handle::default(),
                textured: true,
                tint: [1.0; 3],
            },
            PlantSkin {
                handle: Handle::default(),
                textured: false,
                tint: [1.0; 3],
            },
        ];
        tint_skins(&mut skins, &[(500, [0.9; 3]), (500, [0.2; 3])]);
        assert_eq!(skins[0].tint, [1.0; 3], "the textured part is left alone");
        assert_eq!(skins[1].tint, [1.0; 3], "and is the only plain one");
    }

    #[test]
    fn a_cell_takes_its_share_of_a_triangle() {
        // The clip is what makes the cells add up: a triangle over four
        // cells hands each of them exactly the part that falls in it.
        let tri = [
            Vec2::new(-10.0, -10.0),
            Vec2::new(30.0, -10.0),
            Vec2::new(-10.0, 30.0),
        ];
        let whole = polygon_area(&tri);
        let mut total = 0.0;
        let (mut scratch, mut out) = (Vec::new(), Vec::new());
        for x in -1..=1 {
            for y in -1..=1 {
                let min = Vec2::new(x as f32, y as f32) * 20.0;
                clip_to_cell(tri, min, min + 20.0, &mut scratch, &mut out);
                total += polygon_area(&out);
            }
        }
        assert!((total - whole).abs() < 1e-2, "{total} of {whole}");
        // A cell the triangle misses entirely gets nothing.
        clip_to_cell(
            tri,
            Vec2::new(100.0, 100.0),
            Vec2::new(120.0, 120.0),
            &mut scratch,
            &mut out,
        );
        assert!(out.is_empty());
    }

    #[test]
    fn a_sample_lands_where_its_weights_say() {
        // A point drawn out of a clipped polygon is read back onto the whole
        // triangle by its weights — that is how a card that grew in a cell
        // still knows the colour and the slope of the field under it.
        let (a, b, c) = (
            Vec2::new(0.0, 0.0),
            Vec2::new(30.0, 0.0),
            Vec2::new(0.0, 30.0),
        );
        let poly = vec![a, b, c];
        for i in 0..50u64 {
            let p = sample_polygon(
                &poly,
                polygon_area(&poly),
                draw(i, 1),
                draw(i, 2),
                draw(i, 3),
            );
            let w = barycentric(p, a, b, c).expect("a triangle with a footprint");
            assert!((w.element_sum() - 1.0).abs() < 1e-4, "{w:?}");
            let back = a * w.x + b * w.y + c * w.z;
            assert!(back.distance(p) < 1e-2, "{back:?} vs {p:?}");
        }
    }

    /// The draw the whole scatter rests on has to be flat, or a level that
    /// keeps 45 % of the stand keeps something else and the coarse cards are
    /// sized for a density that is not there. `vary`'s FNV-1a did not manage
    /// it for sequential seeds — 38 % where 45 was asked for.
    #[test]
    fn a_card_draws_from_a_flat_hat() {
        for salt in [0u64, 0x91A7, 0xC2B2_AE3D_27D4_EB4F, 7] {
            let mut eighths = [0usize; 8];
            let n = 40_000u64;
            for seed in 0..n {
                eighths[((draw(seed * 1_009, salt) * 8.0) as usize).min(7)] += 1;
            }
            for (i, count) in eighths.iter().enumerate() {
                let share = *count as f64 / n as f64;
                assert!(
                    (share - 0.125).abs() < 0.006,
                    "salt {salt}, eighth {i}: {share}",
                );
            }
        }
    }

    #[test]
    fn a_card_sheet_is_cut_out_and_not_a_rectangle() {
        // The whole point of the sheet: a card has to be a tuft with sky
        // between its blades. A solid one is the hedge the first stand was.
        for leaf in [Leaf::Meadow, Leaf::Blade, Leaf::Broad] {
            let sheet = card_sheet(leaf);
            let (width, height) = (SHEET_TUFTS * SHEET_TUFT_W, SHEET_TUFT_H);
            let data = sheet.data.as_ref().expect("the sheet is built on the CPU");
            // The mip chain is appended, so level zero is the first plane.
            let level0 = &data[..width * height * 4];
            let opaque = level0.chunks(4).filter(|t| t[3] > 200).count();
            let clear = level0.chunks(4).filter(|t| t[3] < 40).count();
            let cover = opaque as f32 / (width * height) as f32;
            assert!(
                (0.10..0.65).contains(&cover),
                "{leaf:?} covers {cover} of its sheet",
            );
            assert!(clear > opaque, "{leaf:?} is more sky than leaf");
            // A blade never reaches the edge of its cell: what a mip level
            // smears across the seam has to be empty, or one tuft's leaves
            // grow out of the next one's.
            for tuft in 0..SHEET_TUFTS {
                for edge in [tuft * SHEET_TUFT_W, (tuft + 1) * SHEET_TUFT_W - 1] {
                    for row in 0..height {
                        assert_eq!(
                            level0[(row * width + edge) * 4 + 3],
                            0,
                            "{leaf:?} touches the seam at {edge}",
                        );
                    }
                }
            }
            // And it stands on the bottom of its cell, not in mid-air.
            let foot: u32 = (0..width)
                .map(|col| level0[((height - 2) * width + col) * 4 + 3] as u32)
                .sum();
            assert!(foot > 0, "{leaf:?} floats");
        }
    }

    /// How wide one blade of a crop's sheet comes out on the ground [mm].
    ///
    /// The sheet is stretched over the card, so a blade's width on the ground
    /// is its share of the tuft cell times the card. That number is the whole
    /// argument about scale: the first stand drew wheat at 16 to 39 mm a
    /// blade, and a field of 30 mm wheat leaves reads as pampas grass at any
    /// distance you care to stand at.
    fn blade_millimetres(crop: CropClass, height: f32) -> (f32, f32) {
        let thick = match leaf_of(crop) {
            Leaf::Meadow => (0.65f32, 1.35f32),
            Leaf::Blade => (0.8f32, 1.9f32),
            Leaf::Broad => (2.4, 5.0),
            Leaf::Rosette => (6.5, 12.0),
        };
        let scale = card_width(crop, height) * 2.0 / SHEET_TUFT_W as f32 * 1000.0;
        (thick.0 * scale, thick.1 * scale)
    }

    #[test]
    fn a_crop_is_drawn_at_the_size_it_grows() {
        // Measured against what the plant is, at the day it stands tallest.
        // Millimetres across one blade or leaf, as a botanist would give them.
        for (crop, low, high) in [
            (CropClass::WinterCereal, 5.0f32, 18.0f32),
            (CropClass::SummerCereal, 5.0, 18.0),
            (CropClass::Legume, 15.0, 40.0),
            (CropClass::Grassland, 2.5, 10.0),
            (CropClass::Fallow, 3.0, 16.0),
            // A catch-all whose model is grass, so measured like grass.
            (CropClass::Other, 2.5, 12.0),
            (CropClass::Maize, 45.0, 115.0),
            (CropClass::Rapeseed, 22.0, 65.0),
            (CropClass::SugarBeet, 45.0, 130.0),
            (CropClass::Potato, 45.0, 130.0),
            (CropClass::Vegetable, 40.0, 120.0),
            (CropClass::Vineyard, 45.0, 125.0),
        ] {
            let peak = (1..=365u16)
                .map(|d| phenology::growth_on(crop, d, 0).height)
                .fold(0.0f32, f32::max);
            let (thin, thick) = blade_millimetres(crop, peak);
            assert!(
                thin >= low && thick <= high,
                "{crop:?} draws blades {thin:.0}-{thick:.0} mm, not {low:.0}-{high:.0}",
            );
        }
    }

    #[test]
    fn a_clump_is_the_size_of_a_clump() {
        // And the card itself is one clump of the crop, not a stretch of
        // field: a drill of wheat is a third of a metre, a maize plant's leaf
        // span about one, a beet's rosette half, an orchard tree its crown.
        for (crop, low, high) in [
            (CropClass::WinterCereal, 0.25f32, 0.45f32),
            (CropClass::Grassland, 0.12, 0.30),
            (CropClass::Maize, 0.70, 1.20),
            (CropClass::Rapeseed, 0.40, 0.70),
            (CropClass::SugarBeet, 0.30, 0.60),
            (CropClass::Vineyard, 0.80, 1.40),
            (CropClass::Orchard, 1.80, 3.00),
        ] {
            let peak = (1..=365u16)
                .map(|d| phenology::growth_on(crop, d, 0).height)
                .fold(0.0f32, f32::max);
            let width = card_width(crop, peak);
            assert!(
                (low..high).contains(&width),
                "{crop:?} stands {peak:.2} m and draws a {width:.2} m clump",
            );
        }
    }

    #[test]
    fn a_stand_closes_over_whatever_it_is_made_of() {
        // Density and width are picked together, never apart: what an eye
        // looking through a stand sees is the cards' width per square metre,
        // so a crop drawn in smaller clumps has to stand more of them. Every
        // crop meant to close over lands near the same product.
        for crop in [
            CropClass::WinterCereal,
            CropClass::Maize,
            CropClass::Rapeseed,
            CropClass::SugarBeet,
            CropClass::Potato,
            CropClass::Grassland,
            CropClass::Vegetable,
            CropClass::Legume,
            CropClass::Fallow,
            CropClass::Other,
        ] {
            let peak = (1..=365u16)
                .map(|d| phenology::growth_on(crop, d, 0).height)
                .fold(0.0f32, f32::max);
            let closure = density(crop) * card_width(crop, peak);
            assert!(
                (0.90..1.85).contains(&closure),
                "{crop:?} closes at {closure:.2}",
            );
        }
        // The two that are meant to be open stay open: rows with grass
        // between them, and trees standing in a meadow.
        for crop in [CropClass::Orchard, CropClass::Vineyard] {
            let peak = (1..=365u16)
                .map(|d| phenology::growth_on(crop, d, 0).height)
                .fold(0.0f32, f32::max);
            let closure = density(crop) * card_width(crop, peak);
            assert!(closure < 1.30, "{crop:?} closes at {closure:.2}");
        }
    }

    #[test]
    fn a_cell_on_a_boundary_does_not_rebuild_every_frame() {
        // Hysteresis: a cell holds the level it has grown as long as it is
        // within a slack of that level's own band.
        assert_eq!(band_for(10.0, None), Some(Band::Close));
        assert_eq!(band_for(CLOSE_END + 1.0, None), Some(Band::Near));
        assert_eq!(band_for(NEAR_END + 1.0, None), Some(Band::Far));
        assert_eq!(band_for(PLANT_CULL + 1.0, None), None);
        assert_eq!(
            band_for(CLOSE_END + 1.0, Some(Band::Close)),
            Some(Band::Close),
            "a close cell holds on past the hand-over",
        );
        assert_eq!(
            band_for(CLOSE_END - 1.0, Some(Band::Near)),
            Some(Band::Near),
            "and a near cell holds on inside it",
        );
        assert_eq!(
            band_for(CLOSE_END + BAND_SLACK + 1.0, Some(Band::Close)),
            Some(Band::Near),
            "but not for ever",
        );
        assert_eq!(
            band_for(NEAR_END + BAND_SLACK + 1.0, Some(Band::Near)),
            Some(Band::Far),
        );
        assert_eq!(
            band_for(PLANT_CULL + BAND_SLACK + 1.0, Some(Band::Far)),
            None
        );
    }

    #[test]
    fn a_cell_carries_every_level_it_can_be_seen_at() {
        // A cell draws its own level and every coarser one, so the hand-over
        // is the visibility range's and happens at one distance rather than
        // wherever the residency hysteresis let go — and the bands abut, so
        // there is no distance at which a cell draws nothing or twice.
        for band in [Band::Close, Band::Near, Band::Far] {
            let levels: Vec<Band> = band.upwards().collect();
            assert_eq!(levels[0], band, "{band:?} draws its own level first");
            let mut at = 0.0;
            for level in &levels {
                let (start, end) = level.range();
                let start = if *level == band { 0.0 } else { start };
                assert!((start - at).abs() < 1e-6, "{band:?} gaps at {at} m");
                at = end;
            }
            assert_eq!(at, PLANT_CULL, "{band:?} stops short of the cull");
        }
    }

    #[test]
    fn the_levels_hand_over_without_gaps() {
        const { assert!(CLOSE_END < NEAR_END) }
        const { assert!(NEAR_END < PLANT_CULL) }
        const { assert!(PLANT_CULL < MATERIALISE_AT) }
        const { assert!(MATERIALISE_AT < DEMATERIALISE_AT) }
        // The residency hysteresis has to be wider than nothing and narrower
        // than the narrowest band it sits in, or a cell rebuilds every frame.
        const { assert!(BAND_SLACK > 0.0 && BAND_SLACK * 2.0 < CLOSE_END) }
        // A patch is only looked at when a cell of it could be in range.
        const { assert!(MATERIALISE_AT > PLANT_CULL + CELL) }
    }
}
