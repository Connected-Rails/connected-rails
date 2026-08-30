//! The standing crop: plants on the fields (the field plan's deferred pass).
//!
//! The painted surface carries a field at any distance but one: close up a
//! crop is not a colour on the ground but things standing on it. A maize
//! field in August is two and a half metres above the ballast, rape flowers
//! a metre into the air, and a cut field is stubble, not paint. So every
//! field patch grows a crop of **plant cards** on its own surface — quads
//! standing on the mesh the patch is drawn from, two crossed where the
//! camera is close, one where it is not, and nothing at all where the paint
//! carries the field alone.
//!
//! The cards are **grown, not modelled**: what a crop looks like on a day is
//! a function of the crop, the day and the field's own seed, and the surface
//! mesh already carries the last two of those in its vertex colours. No
//! binary assets, no attribution, and nothing that can disagree with the
//! paint underneath — the cards take the stand's colour, its height and its
//! stage from the phenology, the field's tint from the surface, and the
//! weather from the same uniform every other outdoor material takes.
//!
//! The cost is held down three ways:
//!
//! * **A cap per patch.** A whole tile of one crop would grow half a million
//!   cards at field density; past [`MAX_CARDS`] the spacing stretches, which
//!   reads as a thinner stand long before it reads as a fault.
//! * **Two levels.** To [`LOD0_END`] every card is two crossed quads; out to
//!   [`PLANT_CULL`] every third card is one quad; past that the painted
//!   surface is what a field is. A card is a few pixels at each hand-over,
//!   so the switches are lost in the stand.
//! * **Distance.** The meshes are grown when the camera comes within
//!   [`MATERIALISE_AT`] of a patch and dropped again past [`DEMATERIALISE`],
//!   two patches to a frame. A resident line of fields holds card geometry
//!   only where a camera could see it.
//!
//! **Multiplayer.** Nothing here is state. The cards are a function of the
//! patch mesh and the scenario clock, which every client of a run already
//! agrees on, so two machines grow the same field the same way without a
//! byte crossing the network.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::asset::{AssetId, AssetPath, LoadState, RenderAssetUsages};
use bevy::camera::visibility::VisibilityRange;
use bevy::gltf::{Gltf, GltfMesh};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use fields::CropClass;
use fields::phenology::{self, Stage};
use fields::stats::vary;

use crate::{
    Season, TextureMips,
    farmland::{FieldSurface, linear},
    sky::Sky,
    weather::{WeatherExt, WeatherMaterial},
};

/// Where two crossed cards hand over to one [m]. A metre of crop at ninety
/// metres is a dozen pixels; one quad draws the same green rectangle both
/// did, for half the triangles.
pub const LOD0_END: f32 = 90.0;
/// Past this only the painted surface shows [m]. A single quad is a few
/// pixels at four hundred metres, and the rows painted on the surface
/// underneath are what a field is at that distance.
pub const PLANT_CULL: f32 = 420.0;
/// How close the camera must come before a patch's cards are grown [m].
const MATERIALISE_AT: f32 = 480.0;
/// How far out they are dropped again [m]. The gap to [`MATERIALISE_AT`] is
/// the hysteresis that keeps a boundary patch from building and dropping
/// every frame.
const DEMATERIALISE_AT: f32 = 560.0;
/// Most *painted* cards one patch grows. The real models are capped by their
/// own triangle budget, not by this; the paint between the cards keeps the
/// stand closed wherever neither reaches.
const MAX_CARDS: usize = 6_000;
/// How many real plants one patch stands at most, before its model's
/// triangle cost — a heavy model thins itself out against the budget, a
/// light one against the count. The paint and the cards keep the stand
/// closed between them; what the heroes add is the shape the eye catches at
/// twenty metres, and a few hundred of those per patch is what that takes.
const MAX_HEROES: usize = 400;
/// The floor of that count for the heaviest model (the orchard tree): an
/// orchard is rows of trees, and rows need more than a handful.
const MIN_HEROES: usize = 40;
/// The triangle purse the count is drawn from: a model with twice the
/// triangles stands half as often within it.
const HERO_TRIANGLE_BUDGET: usize = 120_000;
/// How finely the stand's height is tracked before the cards are regrown.
/// Height is baked into the geometry, and it moves slowly enough that a
/// quarter metre is the step an eye catches.
const HEIGHT_BUCKET: f32 = 0.25;
/// How many patches may grow their cards in one frame. A patch's cards are
/// a few thousand vertices; two to a frame keeps the day changing without
/// the frame noticing.
const BUILD_BUDGET: usize = 2;

/// The real plant a crop stands as, and how thickly. The models are one
/// metre tall with the origin at the foot (`tools/plants`), so the phenology's
/// stand height *is* the scale, and the field's tint rides in the vertex
/// colour on top.
fn model_of(crop: CropClass) -> Option<(&'static str, f32)> {
    // The second number is the model's own density, in plants per square
    // metre — a fraction, because a real plant costs what a wood's tree
    // costs, and the painted rows between them carry the mass.
    let (file, density) = match crop {
        CropClass::WinterCereal | CropClass::SummerCereal => ("wheat", 0.030),
        CropClass::Maize => ("corn", 0.050),
        CropClass::Rapeseed => ("flowers", 0.015),
        CropClass::SugarBeet => ("turnip", 0.030),
        CropClass::Potato | CropClass::Legume => ("clover", 0.040),
        CropClass::Grassland => ("grass", 0.060),
        CropClass::Vegetable => ("lettuce", 0.040),
        CropClass::Orchard => ("tree", 0.002),
        CropClass::Vineyard => ("vines", 0.010),
        CropClass::Fallow => ("flowers", 0.006),
        CropClass::Other => ("grass", 0.030),
    };
    Some((file, density))
}

/// How thickly the *painted* cards stand under the real plants [per m²]. The
/// cards are the mass of the stand; the models are its shape.
fn density(crop: CropClass) -> f32 {
    match crop {
        CropClass::Maize | CropClass::Potato | CropClass::SugarBeet => 0.5,
        CropClass::WinterCereal | CropClass::SummerCereal | CropClass::Legume => 0.45,
        CropClass::Rapeseed | CropClass::Vegetable | CropClass::Other => 0.4,
        CropClass::Vineyard => 0.3,
        CropClass::Grassland => 0.25,
        CropClass::Fallow => 0.2,
        CropClass::Orchard => 0.08,
    }
}

/// How wide one card is [m]. A cereal tuft is broader than its drill, a maize
/// plant a narrow fan, an orchard tree its crown.
fn card_width(crop: CropClass) -> f32 {
    match crop {
        CropClass::Orchard => 2.6,
        CropClass::Vineyard => 1.2,
        CropClass::Maize => 0.85,
        CropClass::Grassland | CropClass::Fallow => 1.2,
        _ => 1.0,
    }
}

/// What the day decides for one patch, as the meshes compare it: the day of
/// the year, the stage the patch's fields have reached, the height the cards
/// were grown to, and whether deep winter has taken the crop away. When the
/// day moves any of these the cards are grown again; everything else the day
/// does — the colour, the weather — rides in the material for free.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CropKey {
    day: u16,
    stage: u8,
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
            stage: growth.stage as u8,
            height: (growth.height / HEIGHT_BUCKET) as u16,
            snow: Season::on(month, day).snow > 0.5,
            heroes,
        }
    }
}

/// One plant: a foot point on the field's surface and the few numbers that
/// pose a card on it.
#[derive(Debug, Clone, Copy)]
struct Card {
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
    /// Kept by the sparse level: every third, by the same draw that made the
    /// scatter, so the far level is the near one thinned rather than
    /// rearranged.
    sparse: bool,
    /// A spot where a real plant model stands instead of a painted card. The
    /// models cost what a wood's trees cost, so they are a hash-thinned share
    /// of the stand; the cards fill the space between them.
    hero: bool,
}

/// One mesh part of a plant model: world-space geometry, one metre tall, foot
/// at the origin — what [`normalise.mjs`](tools/plants) writes. Parts split
/// by material, so a corn plant's green stalk and yellow cob can keep their
/// own colours.
pub(crate) struct PlantPart {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// The model's own vertex colours, where it ships any.
    pub colors: Option<Vec<[f32; 4]>>,
    /// Texture coordinates, where the part is textured.
    pub uvs: Option<Vec<[f32; 2]>>,
    pub indices: Vec<u32>,
    /// The part's material, as the glTF loader built it.
    pub material: Handle<StandardMaterial>,
}

/// One resolved plant model.
pub(crate) struct PlantModel {
    pub parts: Vec<PlantPart>,
    pub tris: usize,
}

impl PlantModel {
    /// Triangles of the whole model — the hero budget is paid in these.
    fn tris(&self) -> usize {
        self.tris
    }
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
    /// The glTF materials, dressed for the weather and made double-sided,
    /// per loader material.
    dressed: HashMap<AssetId<StandardMaterial>, Handle<WeatherMaterial>>,
    /// Strong handles to the /std materials a resolve has asked for. A load
    /// nothing holds is cancelled at the frame's end — the first resolve
    /// would ask, drop, and ask again forever.
    pending: Vec<Handle<StandardMaterial>>,
}

impl PlantModels {
    /// The model of a crop, loading it on first ask and resolving it once it
    /// has arrived. `None` until then — the patch grows as painted cards and
    /// is regrown when the model lands.
    #[allow(clippy::too_many_arguments)]
    fn model(
        &mut self,
        crop: CropClass,
        assets: &AssetServer,
        gltfs: &Assets<Gltf>,
        gltf_meshes: &Assets<GltfMesh>,
        meshes: &Assets<Mesh>,
        standards: &Assets<StandardMaterial>,
        mips: &mut TextureMips,
    ) -> Option<Arc<PlantModel>> {
        let (file, _) = model_of(crop)?;
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
        handle: &Handle<StandardMaterial>,
        standards: &Assets<StandardMaterial>,
        materials: &mut Assets<WeatherMaterial>,
        textured: bool,
    ) -> Option<Handle<WeatherMaterial>> {
        if let Some(cached) = self.dressed.get(&handle.id()) {
            return Some(cached.clone());
        }
        let mut base = standards.get(handle)?.clone();
        // A model's own colours stay; an untextured part is repainted by the
        // day's stand colour through its vertices, so it needs a white base
        // to multiply into. Everything is drawn double-sided — half of a
        // crossed plant faces away from any camera.
        base.double_sided = true;
        base.metallic = 0.0;
        if !textured {
            base.base_color = Color::WHITE;
        }
        let dressed = materials.add(WeatherMaterial {
            base,
            extension: WeatherExt::default(),
        });
        self.dressed.insert(handle.id(), dressed.clone());
        Some(dressed)
    }
}

/// Flattens a loaded plant glTF: every primitive's geometry with its
/// material. The normaliser has already baked the transforms — one node, one
/// metre, foot at the origin — so there is no hierarchy to walk.
#[allow(clippy::too_many_arguments)]
fn resolve(
    path: &AssetPath,
    assets: &AssetServer,
    gltf: &Gltf,
    gltf_meshes: &Assets<GltfMesh>,
    meshes: &Assets<Mesh>,
    standards: &Assets<StandardMaterial>,
    pending: &mut Vec<Handle<StandardMaterial>>,
    mips: &mut TextureMips,
) -> Option<Arc<PlantModel>> {
    let mut parts = Vec::new();
    let mut tris = 0usize;
    for mesh_handle in &gltf.meshes {
        let Some(gltf_mesh) = gltf_meshes.get(mesh_handle) else {
            continue;
        };
        for primitive in &gltf_mesh.primitives {
            let mesh = meshes.get(&primitive.mesh)?;
            let Ok(positions) = mesh.try_attribute(Mesh::ATTRIBUTE_POSITION) else {
                continue;
            };
            let VertexAttributeValues::Float32x3(positions) = positions else {
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
            pending.push(material.clone());
            if !matches!(
                assets.get_load_state(material.id()),
                Some(LoadState::Loaded)
            ) {
                return None;
            }
            let textured = standards
                .get(&material)
                .is_some_and(|m| m.base_color_texture.is_some());
            // The loader's materials carry mip chains for the textures they
            // hold — without the chain a cut-out tuft thins out with
            // distance (the same lesson the trees taught).
            if textured {
                mips.enqueue_cutout(&material);
            }
            tris += indices.len() / 3;
            parts.push(PlantPart {
                positions: positions.clone(),
                normals: normals.unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; positions.len()]),
                colors,
                uvs,
                indices,
                material,
            });
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(Arc::new(PlantModel { parts, tris }))
}

/// What growing a patch produced: what its fields average, and the cards
/// themselves. The patch's reach travels with the distance gate separately.
struct Grown {
    /// The fields' week averaged over the patch, 0 … 1 — what a regrow
    /// compares against.
    week: f32,
    cards: Vec<Card>,
}

/// Grows the cards of one field patch out of its own surface mesh.
///
/// The mesh is the draped ground in the tile's own frame — already cut to
/// the tile, already cleared of the track corridor — and it carries each
/// field's tint and its own week of the crop year in its vertex colours.
/// Cards sample the triangles by area, and every random decision comes out
/// of the triangle index, so the same patch always grows the same crop of
/// cards: on every machine of a multiplayer run, and on every frame the day
/// is asked again.
fn grow(
    mesh: &Mesh,
    crop: CropClass,
    month: u32,
    day: u32,
    model: Option<&PlantModel>,
) -> Option<Grown> {
    let positions = mesh
        .try_attribute(Mesh::ATTRIBUTE_POSITION)
        .ok()?
        .as_float3()?;
    let normals = mesh
        .try_attribute(Mesh::ATTRIBUTE_NORMAL)
        .ok()?
        .as_float3()?;
    let colors = match mesh.try_attribute(Mesh::ATTRIBUTE_COLOR).ok()? {
        VertexAttributeValues::Float32x4(colors) => colors,
        _ => return None,
    };
    let indices = mesh_indices(mesh)?;

    // Deep winter takes the crop away: the weather uniform has whitened the
    // paint, and green tufts over snow is the one thing worse than none.
    if Season::on(month, day).snow > 0.5 {
        return Some(Grown {
            week: 0.5,
            cards: Vec::new(),
        });
    }

    // The patch's own area decides whether the spacing has to stretch, so
    // the triangles are summed once and sampled in a second pass. A whole
    // tile of one crop would grow half a million cards at the drilled crops'
    // density; stretched to the cap it grows what a wood of a hundred trees
    // carries, and the paint between the cards keeps the stand closed.
    let tris: &[[u32; 3]] = indices.as_chunks::<3>().0;
    let mut area = 0.0f64;
    for tri in tris {
        area += tri_area(
            Vec3::from(positions[tri[0] as usize]),
            Vec3::from(positions[tri[1] as usize]),
            Vec3::from(positions[tri[2] as usize]),
        ) as f64;
    }
    // The factor that brings the whole patch to the cap — a plain ratio, not
    // a square root: spacing grows with the root of the count, the density
    // falls with the count.
    let stretch = ((area * density(crop) as f64) / MAX_CARDS as f64).max(1.0);
    let density = density(crop) / stretch as f32;

    // The share of cards that stands as a real plant: the model's own
    // density over the cards', capped by the count the model's triangle
    // cost allows.
    let hero_share = match model {
        Some(model) => {
            let by_count =
                (MAX_HEROES.min(MIN_HEROES.max(HERO_TRIANGLE_BUDGET / model.tris().max(1))) as f64)
                    .max(1.0);
            let (_, wanted) = model_of(crop)?;
            let cards = area * density as f64;
            ((wanted as f64 / density as f64)
                .min(by_count / cards)
                .min(1.0)) as f32
        }
        None => 0.0,
    };

    let today = phenology::day_of_year(month, day);
    let mut cards: Vec<Card> = Vec::with_capacity(MAX_CARDS);
    let mut week_sum = 0.0f64;
    for (i, tri) in tris.iter().enumerate() {
        let (ia, ib, ic) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
        let (a, b, c) = (
            Vec3::from(positions[ia]),
            Vec3::from(positions[ib]),
            Vec3::from(positions[ic]),
        );
        let piece = tri_area(a, b, c);
        if piece <= 1e-9 {
            continue;
        }
        // Whole cards for the whole area, and the fraction of one the area
        // earns — kept or not by the hash, so the count never needs a shared
        // random state.
        let want = piece as f64 * density as f64;
        let mut count = want as usize;
        if want - count as f64 > vary(i as u64, 0x91A7 + 1) {
            count += 1;
        }
        for k in 0..count as u64 {
            // A uniform point in the triangle, folded in at the edges.
            let mut u = vary(i as u64 * 73 + k * 11, 0x91A7 + 2);
            let mut v = vary(i as u64 * 31 + k * 7, 0x91A7 + 3);
            if u + v > 1.0 {
                u = 1.0 - u;
                v = 1.0 - v;
            }
            let (w, u, v) = ((1.0 - u - v) as f32, u as f32, v as f32);
            let pos = a * w + b * u + c * v;
            let up = (Vec3::from(normals[ia]) * w
                + Vec3::from(normals[ib]) * u
                + Vec3::from(normals[ic]) * v)
                .normalize_or_zero();
            let tint = colors[ia][0] * w + colors[ib][0] * u + colors[ic][0] * v;
            let week = (colors[ia][2] * w + colors[ib][2] * u + colors[ic][2] * v).clamp(0.0, 1.0);
            week_sum += week as f64 * piece as f64;

            // The field's own week decides what the day is here — two wheat
            // fields in one patch ripen a week apart, and the cards on them
            // do too. `pick` is the card's own draw from that same field.
            let pick = vary(i as u64 * 13 + k * 17 + 3, 0x91A7 + 5) as f32;
            let growth = phenology::growth_offset(crop, today, (week * 2.0 - 1.0) * 7.0);
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
            cards.push(Card {
                pos,
                up: if up.length_squared() > 0.5 {
                    up
                } else {
                    Vec3::Y
                },
                yaw: (vary(i as u64 * 3 + k * 13, 0x91A7 + 7) as f32 - 0.5) * 1.1,
                width: card_width(crop) * (0.75 + 0.5 * vary(i as u64 + k, 0x91A7 + 6) as f32),
                height: (growth.height
                    * (0.7 + 0.5 * vary(i as u64 + k * 11, 0x91A7 + 9) as f32)
                    * scale)
                    .max(0.03),
                lean: (vary(i as u64 + k * 7, 0x91A7 + 10) as f32 - 0.5) * 0.35,
                tint,
                light: vary(i as u64 + k * 13, 0x91A7 + 21) as f32,
                sparse: vary(i as u64 * 17 + k * 3, 0x91A7 + 8) < 1.0 / 3.0,
                // Every so often a real plant stands where a card would: the
                // share is the model's density over the cards', so a thick
                // model thins itself out against the budget.
                hero: vary(i as u64 + k * 19, 0x91A7 + 12) < hero_share as f64,
            });
        }
    }
    let week = if area > 0.0 {
        (week_sum / (area * 1.0)).clamp(0.0, 1.0) as f32
    } else {
        0.5
    };
    Some(Grown { week, cards })
}

/// The materials of the standing crop, one per crop, and the day they were
/// written for — the same shape as the farmland's own.
#[derive(Resource, Default)]
pub struct PlantMaterials {
    by_crop: HashMap<CropClass, Handle<WeatherMaterial>>,
    day: Option<u16>,
}

impl PlantMaterials {
    /// The material for a crop, made on first use.
    pub fn get(
        &mut self,
        crop: CropClass,
        assets: &mut Assets<WeatherMaterial>,
        month: u32,
        day: u32,
    ) -> Handle<WeatherMaterial> {
        self.by_crop
            .entry(crop)
            .or_insert_with(|| {
                let growth = phenology::growth(crop, month, day, 0);
                assets.add(WeatherMaterial {
                    base: StandardMaterial {
                        base_color: stand_colour(growth),
                        // Two quads crossed: half of them face away from any
                        // camera, and culled they would draw as a dashed
                        // line.
                        double_sided: true,
                        perceptual_roughness: 0.85,
                        ..default()
                    },
                    extension: WeatherExt::default(),
                })
            })
            .clone()
    }

    /// Writes the day's colour into every crop's material, if the day moved.
    /// The stage and the height are the meshes' business — a rebuild; the
    /// colour is the material's, and costs one write per crop.
    pub fn set_date(&mut self, assets: &mut Assets<WeatherMaterial>, month: u32, day: u32) -> bool {
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
        true
    }

    pub fn is_empty(&self) -> bool {
        self.by_crop.is_empty()
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
) {
    if materials.is_empty() {
        return;
    }
    materials.set_date(&mut assets, sky.month, sky.day);
}

/// The standing crop of one field patch: which level meshes are up, and the
/// crop year they were grown for.
#[derive(Component, Default)]
pub struct FieldPlants {
    /// The level meshes that are up, nearest level first, with their mesh
    /// assets — a rebuild despawns the entities *and* drops the geometry,
    /// which nothing else owns.
    lods: Vec<(Entity, AssetId<Mesh>)>,
    /// What the meshes that are up were grown for.
    grown: Option<CropKey>,
    /// Centre and reach of the patch, in the surface's own frame — measured
    /// from the mesh once and kept.
    bounds: Option<(Vec3, f32)>,
    /// The fields' week averaged over the patch, as the surface's `b`
    /// channel carries it.
    week: f32,
}

/// Takes a patch's standing meshes down: the entities go and their mesh
/// assets with them — the geometry is the patch's alone, and leaving it in
/// the asset store would pile a copy onto the GPU at every regrow.
fn drop_lods(commands: &mut Commands, state: &mut FieldPlants) {
    for (entity, mesh) in state.lods.drain(..) {
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
    state.grown = None;
}

/// Grows, regrows and drops the standing crop of every field patch.
///
/// A patch grows its cards when the camera comes within [`MATERIALISE_AT`]
/// of its bounds, and drops them again past [`DEMATERIALISE_AT`] — a
/// resident line of fields is hundreds of patches, and only a handful of
/// them are ever near enough for a card to be more than a pixel. A new day
/// regrows what it changed, budgeted like the first growth: the painted
/// surface underneath is right the whole time, so a card that is an hour
/// late is invisible.
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
    mut plants: ResMut<PlantMaterials>,
    mut models: ResMut<PlantModels>,
    sky: Res<Sky>,
    mut fields: Query<(
        Entity,
        &FieldSurface,
        &Mesh3d,
        &GlobalTransform,
        &mut FieldPlants,
    )>,
    cameras: Query<(&Camera, &GlobalTransform)>,
) {
    let Some((_, at)) = cameras.iter().find(|(camera, _)| camera.is_active) else {
        return;
    };
    let eye = at.translation();
    let mut builds = 0usize;

    for (entity, surface, mesh, at, mut state) in &mut fields {
        // The patch's reach, measured once from its mesh: everything after
        // this is one distance test against it.
        if state.bounds.is_none() {
            state.bounds = bounds_of(meshes.get(&mesh.0));
        }
        let Some((centre, radius)) = state.bounds else {
            continue;
        };
        let distance = at.transform_point(centre).distance(eye) - radius;

        // Out of sight: the meshes go, the patch keeps its place.
        if distance > DEMATERIALISE_AT {
            drop_lods(&mut commands, &mut state);
            continue;
        }
        if distance > MATERIALISE_AT {
            continue;
        }

        // The crop's real plant, once its file has loaded. Until then the
        // patch grows as painted cards, and the missing flag in the key
        // brings it back here the frame the model lands.
        let model = models.model(
            surface.crop,
            &assets,
            &gltfs,
            &gltf_meshes,
            &meshes,
            &standards,
            &mut mips,
        );

        // Grown for the day already? The day's colour rode in with the
        // material; only stage, height, winter and a landed model rebuild.
        let key = CropKey::of(
            surface.crop,
            sky.month,
            sky.day,
            state.week,
            model.is_some(),
        );
        if state.grown == Some(key) || builds >= BUILD_BUDGET {
            continue;
        }
        builds += 1;
        drop_lods(&mut commands, &mut state);
        let Some(surface_mesh) = meshes.get(&mesh.0) else {
            continue;
        };
        state.grown = Some(key);
        let Some(grown) = grow(
            surface_mesh,
            surface.crop,
            sky.month,
            sky.day,
            model.as_deref(),
        ) else {
            continue;
        };
        state.week = grown.week;
        if grown.cards.is_empty() {
            continue;
        }
        // The painted cards' colour is the crop's stand colour, per day; the
        // real plants repaint themselves from it in their own vertices.
        let growth = phenology::growth(surface.crop, sky.month, sky.day, 0);
        let stand = [
            linear(growth.color[0]),
            linear(growth.color[1]),
            linear(growth.color[2]),
        ];
        let material = plants.get(surface.crop, &mut materials, sky.month, sky.day);
        // The hero meshes, one per material part, each with its own dressed
        // material; then the painted filler beneath them and the sparse
        // level beyond.
        let heroes = model.as_ref().map(|model| {
            model
                .parts
                .iter()
                .enumerate()
                .map(|(at, part)| {
                    (
                        at,
                        hero_mesh(part, &grown.cards, stand),
                        models.dressed(
                            &part.material,
                            &standards,
                            &mut materials,
                            part.uvs.is_some(),
                        ),
                    )
                })
                .collect::<Vec<_>>()
        });
        let mut spawned = Vec::new();
        commands.entity(entity).with_children(|parent| {
            if let Some(heroes) = &heroes {
                for (_, hero_mesh, material) in heroes {
                    let Some(material) = material else {
                        continue;
                    };
                    let handle = meshes.add(hero_mesh.clone());
                    spawned.push((
                        parent
                            .spawn((
                                Mesh3d(handle.clone()),
                                MeshMaterial3d(material.clone()),
                                Transform::IDENTITY,
                                range(0.0, LOD0_END),
                                NotShadowCaster,
                            ))
                            .id(),
                        handle.id(),
                    ));
                }
            }
            let filler = meshes.add(card_mesh(&grown.cards, false));
            spawned.push((
                parent
                    .spawn((
                        Mesh3d(filler.clone()),
                        MeshMaterial3d(material.clone()),
                        Transform::IDENTITY,
                        range(0.0, LOD0_END),
                        NotShadowCaster,
                    ))
                    .id(),
                filler.id(),
            ));
            let sparse = meshes.add(card_mesh(&grown.cards, true));
            spawned.push((
                parent
                    .spawn((
                        Mesh3d(sparse.clone()),
                        MeshMaterial3d(material),
                        Transform::IDENTITY,
                        range(LOD0_END, PLANT_CULL),
                        NotShadowCaster,
                    ))
                    .id(),
                sparse.id(),
            ));
        });
        state.lods.extend(spawned);
    }
}

/// One card as quads: two crossed at a right angle for the close level, one
/// for the sparse one.
///
/// The normal is one per quad and horizontal — the same per-blade normal the
/// trees use. A normal per *corner* lights the two halves of a quad
/// differently and splits every plant down the middle, and one pointing up
/// would lose the front-versus-back shading altogether. Double-sided drawing
/// negates it on the far side, so a card darkens when the sun gets behind it
/// the way a tree does.
fn card_mesh(cards: &[Card], sparse: bool) -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();

    for card in cards {
        if sparse && !card.sparse {
            continue;
        }
        let up = if card.up.length_squared() > 0.5 {
            card.up.normalize()
        } else {
            Vec3::Y
        };
        let (sin, cos) = card.yaw.sin_cos();
        // One quad across the working direction, one along it, both turned a
        // little by the card's own yaw — and the top of each leans a little
        // out of true, so a stand is never a picket fence.
        for (qi, axis) in [Vec3::new(cos, 0.0, sin), Vec3::new(-sin, 0.0, cos)]
            .into_iter()
            .enumerate()
        {
            let normal = Vec3::new(-axis.z, 0.0, axis.x);
            let sway = axis * (card.lean * card.height * if qi == 0 { -1.0 } else { 1.0 });
            let foot_a = card.pos - axis * (card.width * 0.5);
            let foot_b = card.pos + axis * (card.width * 0.5);
            let top_a = foot_a + up * card.height + sway;
            let top_b = foot_b + up * card.height + sway;
            // The base lives in the stand's own shade and the head catches
            // the sky; both ride the field's tint and the tuft's own light,
            // so a card matches the painted field it stands on.
            let dark = (0.55 + 0.18 * card.light) * (0.9 + 0.2 * card.tint);
            let bright = (1.0 + 0.16 * card.light) * (0.92 + 0.18 * card.tint);
            let base = positions.len() as u32;
            positions.extend_from_slice(&[
                foot_a.to_array(),
                foot_b.to_array(),
                top_b.to_array(),
                top_a.to_array(),
            ]);
            normals.extend_from_slice(&[[normal.x, normal.y, normal.z]; 4]);
            colors.extend_from_slice(&[
                [dark, dark, dark, 1.0],
                [dark, dark, dark, 1.0],
                [bright, bright, bright, 1.0],
                [bright, bright, bright, 1.0],
            ]);
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            // The sparse level draws one quad of the cross.
            if sparse {
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
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// One part's merged mesh, every hero card baked into it: a transformed
/// copy of the model's geometry per hero card. The model is one metre tall
/// with its foot at the origin, so the card's height *is* the scale, and
/// the card's yaw and lean pose it.
///
/// The day's colour rides in the vertex colour. An untextured part is
/// repainted wholesale — its material is white, and the stand colour
/// multiplied by the base-to-head gradient is what the plant wears, so a
/// wheat model is blue-green in May and gold in July. A textured part keeps
/// its own picture and takes only the gradient and the tuft's light, the
/// same shading the painted cards get.
fn hero_mesh(part: &PlantPart, cards: &[Card], stand: [f32; 3]) -> Mesh {
    let textured = part.uvs.is_some();
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut colors = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();

    for card in cards.iter().filter(|c| c.hero) {
        let (sin, cos) = card.yaw.sin_cos();
        let scale = card.height.max(0.02);
        // The lean shears the model along its own facing, so a plant tips
        // without its foot leaving the ground.
        let lean_axis = [cos, 0.0, sin];
        let rotate = |x: f32, z: f32| [x * cos - z * sin, x * sin + z * cos];
        let base = positions.len() as u32;
        for (i, p) in part.positions.iter().enumerate() {
            let [rx, rz] = rotate(p[0], p[2]);
            let shear = card.lean * p[1] * scale;
            positions.push([
                card.pos[0] + rx * scale + lean_axis[0] * shear,
                card.pos[1] + p[1] * scale,
                card.pos[2] + rz * scale + lean_axis[2] * shear,
            ]);
            let n = part.normals[i];
            let [nx, nz] = rotate(n[0], n[2]);
            normals.push([nx, n[1], nz]);
            // The base lives in the stand's shade, the head catches the sky;
            // both ride the field's tint and the tuft's own light.
            let gradient = 0.55 + 0.5 * p[1].clamp(0.0, 1.0);
            let shade = gradient * (0.9 + 0.2 * card.tint) * (0.85 + 0.3 * card.light);
            let own = part.colors.as_ref().map(|c| c[i]).unwrap_or([1.0; 4]);
            let rgb = if textured {
                [own[0] * shade, own[1] * shade, own[2] * shade]
            } else {
                [stand[0] * shade, stand[1] * shade, stand[2] * shade]
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

/// The visibility band of one level, measured to the mesh's own bounds: a
/// patch's cards may stand three hundred metres from the tile's origin, and
/// the origin is what a range without `use_aabb` measures to.
fn range(start: f32, end: f32) -> VisibilityRange {
    VisibilityRange {
        start_margin: start..start,
        end_margin: end..end,
        use_aabb: true,
    }
}

/// The patch's centre and reach, for the distance gate.
fn bounds_of(mesh: Option<&Mesh>) -> Option<(Vec3, f32)> {
    positions_bounds(
        mesh?
            .try_attribute(Mesh::ATTRIBUTE_POSITION)
            .ok()?
            .as_float3()?,
    )
}

/// The centre and reach of a set of points.
fn positions_bounds(positions: &[[f32; 3]]) -> Option<(Vec3, f32)> {
    let mut lo = Vec3::splat(f32::MAX);
    let mut hi = Vec3::splat(f32::MIN);
    for p in positions {
        lo = lo.min(Vec3::from(*p));
        hi = hi.max(Vec3::from(*p));
    }
    if lo.x >= hi.x {
        return None;
    }
    Some(((lo + hi) * 0.5, hi.distance(lo) * 0.5))
}

/// The patch's indices, widened to `u32`.
fn mesh_indices(mesh: &Mesh) -> Option<Vec<u32>> {
    Some(match mesh.indices()? {
        Indices::U16(v) => v.iter().map(|&i| i as u32).collect(),
        Indices::U32(v) => v.clone(),
    })
}

/// The area of a triangle, the cheap way.
fn tri_area(a: Vec3, b: Vec3, c: Vec3) -> f32 {
    (b - a).cross(c - a).length() * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The number of indices a mesh carries.
    fn index_count(mesh: &Mesh) -> usize {
        mesh.indices().map(Indices::len).unwrap_or(0)
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

    #[test]
    fn a_ripe_stand_grows_where_bare_ground_grows_nothing() {
        // Mid-July wheat stands, and its cards are as tall as the calendar
        // says; the same ground ploughed bare grows nothing at all.
        let mesh = patch(40.0);
        let ripe = grow(&mesh, CropClass::WinterCereal, 7, 15, None).unwrap();
        assert!(!ripe.cards.is_empty());
        for card in &ripe.cards {
            assert!(card.height > 0.5, "{card:?}");
        }
        let bare = grow(&patch(40.0), CropClass::Maize, 4, 1, None).unwrap();
        assert!(bare.cards.is_empty(), "nothing stands on ploughed ground");
    }

    #[test]
    fn a_big_field_does_not_overrun_the_cap() {
        // A square kilometre of one crop would grow half a million cards at
        // field density; the spacing stretches instead.
        let grown = grow(&patch(1_000.0), CropClass::WinterCereal, 7, 15, None).unwrap();
        assert!(grown.cards.len() <= MAX_CARDS, "{}", grown.cards.len());
        assert!(grown.cards.len() > MAX_CARDS / 2, "{}", grown.cards.len());
    }

    #[test]
    fn the_sparse_level_thins_the_near_one() {
        let mesh = patch(40.0);
        let grown = grow(&mesh, CropClass::WinterCereal, 7, 15, None).unwrap();
        let near = card_mesh(&grown.cards, false);
        let far = card_mesh(&grown.cards, true);
        // Every card the sparse level keeps is one the full level drew: a
        // third of the cards, one quad of the cross each.
        let kept = grown.cards.iter().filter(|c| c.sparse).count();
        assert_eq!(index_count(&far) / 6, kept, "one quad per kept card");
        assert_eq!(
            index_count(&near) / 12,
            grown.cards.len(),
            "two quads a card"
        );
        // And the thinning is the draw's, not a hash quirk: about a third.
        assert!(
            kept * 3 < grown.cards.len() * 2,
            "{kept} of {}",
            grown.cards.len()
        );
        assert!(
            kept * 3 > grown.cards.len(),
            "{} of {}",
            kept,
            grown.cards.len()
        );
    }

    #[test]
    fn a_card_is_two_quads_with_a_base_to_head_gradient() {
        let grown = grow(&patch(40.0), CropClass::WinterCereal, 7, 15, None).unwrap();
        let mesh = card_mesh(&grown.cards, false);
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
        if let bevy::render::mesh::Indices::U32(indices) = mesh.indices().unwrap() {
            assert!(indices.iter().all(|&i| (i as usize) < positions.len()));
        }
    }

    #[test]
    fn a_hero_card_wears_the_real_plant() {
        // One triangle as a model, one hero card at 80 cm: the baked mesh is
        // the model scaled, yawed and repainted into the day's stand colour.
        let model = PlantModel {
            parts: vec![PlantPart {
                positions: vec![[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.3, 1.0, 0.0]],
                normals: vec![[0.0, 1.0, 0.0]; 3],
                colors: None,
                uvs: None,
                indices: vec![0, 1, 2],
                material: Handle::default(),
            }],
            tris: 1,
        };
        let card = Card {
            pos: Vec3::new(10.0, 0.0, -5.0),
            up: Vec3::Y,
            yaw: 0.0,
            width: 1.0,
            height: 0.8,
            lean: 0.0,
            tint: 0.5,
            light: 0.5,
            sparse: true,
            hero: true,
        };
        let mesh = hero_mesh(&model.parts[0], &[card], [0.2, 0.4, 0.1]);
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
            "head near the stand colour"
        );
        // Not a hero: nothing is baked.
        let plain = Card {
            hero: false,
            ..card
        };
        let empty = hero_mesh(&model.parts[0], &[plain], [0.2, 0.4, 0.1]);
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
    fn the_levels_hand_over_without_gaps() {
        const { assert!(LOD0_END < PLANT_CULL) }
        const { assert!(PLANT_CULL < MATERIALISE_AT) }
        const { assert!(MATERIALISE_AT < DEMATERIALISE_AT) }
    }
}
