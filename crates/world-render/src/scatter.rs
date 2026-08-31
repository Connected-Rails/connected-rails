//! What stands on a terrain tile: the trees and the scenery objects, streamed
//! in and out with it (plan ch. 14 "streamed instances", 4.3).
//!
//! A tree is **not** a scene. A glTF scene is instantiated per entity — its
//! whole node hierarchy, names, extras, animation players — and a baked wood is
//! ten thousand of them. Instead the model is read once out of the loaded
//! `Gltf` asset into a flat list of mesh parts, and every tree becomes one
//! entity per part that shares the part's mesh and material handles. Bevy
//! batches entities with the same mesh and material into instanced draws, so
//! a tile of three hundred firs is a handful of draw calls, not three hundred
//! hierarchies. Nodes named `_LOD0`, `_LOD1`, … (the same convention as the
//! vehicles and signals) become [`VisibilityRange`] bands. Where a species
//! names its own distances ([`TrackObject::lod_distances`]) those are used, and
//! that is the normal case for vegetation: a level is worth its triangles for
//! as long as the plant covers enough pixels, which depends on how big it is.
//! A forty metre fir hands over at eighty metres and is drawn to two and a
//! half kilometres; a two metre blackthorn hands over at twenty and is gone at
//! seven hundred. Objects that name nothing get [`LOD_BANDS`] and
//! [`TREE_CULL`].
//!
//! The model is only there once its glTF has loaded, which is some frames
//! after the tile. Until then the tile carries [`PendingTrees`], and
//! [`materialise_trees`] spawns the wood in one go when every species of the
//! tile has either loaded or failed — a wood that appears tree by tree as the
//! files come in is what pop-in looks like.
//!
//! Scenery objects — masts, huts, boards — are few per tile and may carry
//! lamps and moving parts one day, so they stay scene instances.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::asset::{AssetPath, LoadState};
use bevy::camera::RenderTarget;
use bevy::camera::visibility::VisibilityRange;
use bevy::gltf::{Gltf, GltfAssetLabel, GltfMesh, GltfNode};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::world_serialization::WorldAsset;
use content::{PersonInstance, SceneryInstance, Tree, Walkway};
use sim_core::train::lod_level;
use std::collections::BTreeMap;
use track_model::TrackObject;

use crate::people::{PERSON_CULL, Passengers, WalkwayHost, person_bundle, spawn_strollers};
use crate::{Season, asset_path};

/// Past this distance no tree is drawn [m] — what a species that names no
/// bands of its own is culled at.
pub const TREE_CULL: f32 = 2_500.0;
/// Where one level of detail hands over to the next [m] for an object without
/// its own table: `_LOD0` up to the first, `_LOD1` up to the second, and so
/// on. The last level a model ships runs on to [`TREE_CULL`].
const LOD_BANDS: [f32; 3] = [80.0, 260.0, 800.0];
/// Scenery objects are culled here [m] — a mast is thinner than a tree.
pub const OBJECT_CULL: f32 = 3_000.0;
/// Levels that start beyond this cast no shadow [m]. What it buys is the sun's
/// own visibility pass: every tree entity is looked at once per shadow view as
/// well as once for the camera, and a forested corridor is hundreds of
/// thousands of them.
///
/// It has to sit well beyond where a shadow is still *seen*, though. With a low
/// sun a wood throws its shadow hundreds of metres across open ground, and a
/// cutoff inside that range draws a line on the field: trees on one side of it
/// cast, trees behind it do not.
const SHADOW_CUTOFF: f32 = 900.0;

/// Render assets of everything a tile can carry: per tree species the glTF
/// of the mod object it names, per scenery object its scene, per passenger
/// character its glTF and scene, and the placeholders for names no installed
/// mod answers for.
#[derive(Clone)]
pub struct WorldCatalog {
    /// Indexed by [`Tree::object`]; `None` where the name resolved to no
    /// installed mod object.
    trees: Vec<Option<Species>>,
    /// Indexed by [`SceneryInstance::object`].
    objects: Vec<Option<Handle<WorldAsset>>>,
    /// Indexed by [`PersonInstance::character`].
    people: Passengers,
    /// Placeholder conifer and broadleaf, coloured by vertex so one white
    /// material serves both.
    placeholder_trees: [Handle<Mesh>; 2],
    placeholder_tree_material: Handle<StandardMaterial>,
    /// The magenta block an unknown object shows as — visible in the world
    /// instead of silently absent.
    placeholder_object: (Handle<Mesh>, Handle<StandardMaterial>),
}

impl WorldCatalog {
    /// Resolves the names against the installed mods' `objects/*.ron`,
    /// unknown names logged once and shown as placeholders; `people` are the
    /// crowd's characters, already resolved ([`Passengers::resolve`]).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tree_names: &[String],
        object_names: &[String],
        registry: &BTreeMap<String, TrackObject>,
        people: Passengers,
        assets: &AssetServer,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        season: Season,
    ) -> Self {
        let trees = tree_names
            .iter()
            .map(|name| match registry.get(name) {
                Some(object) => Some(Species {
                    model: assets.load(asset_path(season.model_of(object))),
                    bands: Arc::from(object.lod_distances.as_slice()),
                }),
                None => {
                    warn!("vegetation: unknown object {name:?} — placeholder shown");
                    None
                }
            })
            .collect();
        let objects = object_names
            .iter()
            .map(|name| match registry.get(name) {
                Some(object) => Some(assets.load(
                    GltfAssetLabel::Scene(0).from_asset(asset_path(season.model_of(object))),
                )),
                None => {
                    warn!("scenery: unknown object {name:?} — placeholder shown");
                    None
                }
            })
            .collect();
        let (placeholder_trees, placeholder_tree_material) =
            crate::placeholder_trees(meshes, materials, season);
        let placeholder_object = (
            meshes.add(Cuboid::new(0.8, 2.0, 0.8)),
            materials.add(StandardMaterial {
                base_color: Color::srgb(0.75, 0.30, 0.65),
                ..default()
            }),
        );
        Self {
            trees,
            objects,
            people,
            placeholder_trees,
            placeholder_tree_material,
            placeholder_object,
        }
    }
}

/// One species of the catalogue: the glTF its levels are read out of and the
/// distances they hand over at. The distances come with the mod object, so a
/// bush and a fir are drawn to the range each is worth.
#[derive(Clone)]
struct Species {
    model: Handle<Gltf>,
    /// Empty = the renderer's own [`LOD_BANDS`] and [`TREE_CULL`].
    bands: Arc<[f32]>,
}

impl WorldCatalog {
    /// The furthest any species among these trees is drawn [m]. A wood is kept
    /// alive by its longest-lived member: one fir among a hedge of blackthorn
    /// holds the whole tile's wood on out to the fir's own distance.
    fn cull_of(&self, trees: &[Tree]) -> f32 {
        let mut cull: f32 = 0.0;
        for tree in trees {
            let species = tree
                .object
                .and_then(|i| self.trees.get(i as usize))
                .and_then(|species| species.as_ref());
            let reach = species
                .and_then(|species| species.bands.last().copied())
                .unwrap_or(TREE_CULL);
            cull = cull.max(reach);
        }
        cull
    }
}

/// How far the furthest tree of a tile stands from the tile's own origin [m] —
/// the wood is measured from that origin, and a tree in the far corner has to
/// count as that much nearer.
fn tile_reach(trees: &[Tree]) -> f32 {
    trees
        .iter()
        .map(|tree| Vec3::from(tree.pos).length())
        .fold(0.0, f32::max)
}

/// Switches a tile's wood off once every tree in it is out of range, and on
/// again when it is not. Cheap: one distance per tile against a few hundred
/// thousand entities that would otherwise each be tested twice a frame.
pub fn cull_distant_woods(
    // The camera that draws the world — see `draws_the_world`. Measuring a
    // wood's distance to a cab display, a cloud panorama or one of the route
    // editor's catalogue thumbnails culls by the wrong thing entirely.
    cameras: Query<(&Camera, &RenderTarget, &GlobalTransform), With<Camera3d>>,
    mut woods: Query<(&GlobalTransform, &Wood, &mut Visibility)>,
    mut reported: Local<usize>,
) {
    let Some(eye) = cameras
        .iter()
        .find(|(camera, target, _)| crate::draws_the_world(camera, target))
        .map(|(.., at)| at.translation())
    else {
        return;
    };
    let mut hidden = 0usize;
    let mut total = 0usize;
    for (at, wood, mut visibility) in &mut woods {
        total += 1;
        let wanted = if at.translation().distance(eye) > wood.cull {
            hidden += 1;
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        // Only on a change: writing it every frame would mark the whole
        // hierarchy dirty and undo the saving.
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
    // Only when the count moves, so it is a line now and then rather than one a
    // frame. It is the number worth watching: it is the share of the tree
    // entities that never reach a bounds test.
    if hidden != *reported {
        *reported = hidden;
        debug!("scatter: {hidden} of {total} woods out of range");
    }
}

/// Marker on every scenery object spawned from a tile, carrying the index of
/// its placement in the line file — the editor's selection speaks in those.
#[derive(Component, Clone, Copy)]
pub struct SceneryIndex(pub u32);

/// Marker on everything a tile carries besides its ground — what
/// [`crate::respawn_scatter`] clears before it places the tile's trees and
/// objects anew.
#[derive(Component, Clone, Copy)]
pub struct Scattered;

/// The wood of one tile: the entity every one of its trees hangs under.
///
/// It exists for one reason. Terrain streams to the view distance — four to
/// seven kilometres — while no tree is drawn past two and a half, so more than
/// half the tiles that are resident carry trees that cannot appear. Each of
/// those trees is still four entities, and each of them is looked at once a
/// frame for the camera and once more for every shadow cascade, only to fail
/// its own [`VisibilityRange`]. Hiding the wood hides all of it at once:
/// `check_visibility` and the light's own pass both give up on an entity whose
/// `InheritedVisibility` is false before they touch its bounds.
///
/// The test is **distance, not the frustum**. A tile behind the camera still
/// throws its shadow into the picture with a low sun, and the shadow pass reads
/// the same inherited visibility — cull it by the frustum and the wood's shadow
/// goes with it. Distance is safe: past `cull` the trees are not drawn anyway.
#[derive(Component)]
pub struct Wood {
    /// The furthest any of this tile's species is drawn [m], plus the tile's
    /// own reach — the wood stays on while any single tree in it might show.
    cull: f32,
}

/// The trees of a tile whose models have not all loaded yet.
#[derive(Component)]
pub struct PendingTrees {
    trees: Vec<Tree>,
    catalog: WorldCatalog,
}

impl PendingTrees {
    pub fn new(trees: Vec<Tree>, catalog: &WorldCatalog) -> Self {
        Self {
            trees,
            catalog: catalog.clone(),
        }
    }
}

/// A tree model, flattened: every mesh part with its material and its pose
/// in the model, grouped by level of detail.
struct TreeModel {
    /// Sorted by level, finest first.
    levels: Vec<(u8, Vec<Part>)>,
}

struct Part {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    transform: Transform,
}

/// Tree models by glTF, read once out of the asset. `None` is a glTF that
/// failed to load or carries no mesh — its trees show the placeholder.
#[derive(Resource, Default)]
pub struct TreeModels {
    resolved: HashMap<AssetId<Gltf>, Option<Arc<TreeModel>>>,
}

/// Spawns the trees of every tile whose species have all arrived.
// A Bevy system takes its assets as parameters — the argument count says
// nothing here.
#[allow(clippy::too_many_arguments)]
pub fn materialise_trees(
    mut commands: Commands,
    pending: Query<(Entity, &PendingTrees)>,
    mut models: ResMut<TreeModels>,
    assets: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    nodes: Res<Assets<GltfNode>>,
    gltf_meshes: Res<Assets<GltfMesh>>,
    mut mips: ResMut<crate::TextureMips>,
) {
    for (wood, pending) in &pending {
        let species: Vec<&Handle<Gltf>> = pending
            .catalog
            .trees
            .iter()
            .flatten()
            .map(|species| &species.model)
            .collect();
        let ready = species.iter().all(|handle| {
            if models.resolved.contains_key(&handle.id()) {
                return true;
            }
            match assets.get_load_state(handle.id()) {
                Some(LoadState::Loaded) => {
                    let model = gltfs.get(*handle).and_then(|gltf| {
                        resolve(gltf, handle.path(), &nodes, &gltf_meshes, &assets)
                    });
                    // The sheets are plain PNG, which the loader takes as one
                    // level; without a chain a wood shimmers with every metre,
                    // and without the coverage the chain keeps, the canopy
                    // thins out with distance until only the trunks are left.
                    if let Some(model) = &model {
                        for (_, parts) in &model.levels {
                            for part in parts {
                                mips.enqueue_cutout(&part.material);
                            }
                        }
                    }
                    if model.is_none() {
                        warn!(
                            "vegetation: {:?} carries no mesh — placeholder shown",
                            handle.path()
                        );
                    }
                    models.resolved.insert(handle.id(), model.map(Arc::new));
                    true
                }
                Some(LoadState::Failed(_)) => {
                    models.resolved.insert(handle.id(), None);
                    true
                }
                _ => false,
            }
        });
        if !ready {
            continue;
        }
        // The tile may have been streamed out while its wood was loading.
        let Ok(mut wood) = commands.get_entity(wood) else {
            continue;
        };
        wood.remove::<PendingTrees>().with_children(|parent| {
            for tree in &pending.trees {
                spawn_tree(parent, tree, &pending.catalog, &models);
            }
        });
    }
}

fn spawn_tree(
    parent: &mut ChildSpawnerCommands,
    tree: &Tree,
    catalog: &WorldCatalog,
    models: &TreeModels,
) {
    let transform = Transform::from_translation(Vec3::from(tree.pos))
        .with_rotation(Quat::from_rotation_y(tree.rot))
        .with_scale(Vec3::splat(tree.scale));
    let species = tree
        .object
        .and_then(|i| catalog.trees.get(i as usize))
        .and_then(|species| species.as_ref());
    let model = species
        .and_then(|species| models.resolved.get(&species.model.id()))
        .and_then(|model| model.clone());
    let Some(model) = model else {
        // Placeholder: conifer or broadleaf, picked by position hash so a
        // wood mixes without carrying a species.
        let kind = (tree.pos[0].to_bits() ^ tree.pos[2].to_bits()) as usize & 1;
        parent.spawn((
            Mesh3d(catalog.placeholder_trees[kind].clone()),
            MeshMaterial3d(catalog.placeholder_tree_material.clone()),
            transform,
            VisibilityRange::abrupt(0.0, TREE_CULL),
            Scattered,
        ));
        return;
    };
    let count = model.levels.len();
    let bands = species.map_or(&[][..], |species| &species.bands);
    for (i, (_, parts)) in model.levels.iter().enumerate() {
        let (start, end) = band(i, count, bands);
        let range = crossfade(start, end);
        let casts_shadow = start < SHADOW_CUTOFF;
        for part in parts {
            let mut level = parent.spawn((
                Mesh3d(part.mesh.clone()),
                MeshMaterial3d(part.material.clone()),
                transform * part.transform,
                range.clone(),
                Scattered,
            ));
            if !casts_shadow {
                level.insert(NotShadowCaster);
            }
        }
    }
}

/// How wide the hand-over between two levels is, as a share of the distance it
/// happens at. Bevy dithers the two levels into each other across it, so a
/// wood does not change shape in one frame as the train rolls up to it.
///
/// **Zero, for now.** The dither is an ordered pattern of discarded fragments,
/// and foliage is already a cut-out: over a band a tenth of the hand-over
/// distance wide, a dense wood has hundreds of trees dithering at once and the
/// canopy breaks up into speckle. A tree changing level in one frame is a
/// smaller fault than a whole wood crawling. Worth revisiting with a narrower
/// band once the levels differ less than they do.
const CROSSFADE: f32 = 0.0;

/// The visibility band of a level, with its ends widened into a crossfade.
/// Bevy asks that a level's `end_margin` be the next level's `start_margin`,
/// and [`band`] hands over at exactly one distance, so both come out of the
/// same multiplication.
fn crossfade(start: f32, end: f32) -> VisibilityRange {
    let fade = |d: f32| (d * (1.0 - CROSSFADE))..(d * (1.0 + CROSSFADE));
    VisibilityRange {
        start_margin: if start <= 0.0 { 0.0..0.0 } else { fade(start) },
        end_margin: fade(end),
        use_aabb: false,
    }
}

/// From where to where level `i` of a model with `count` levels is drawn [m].
///
/// `bands` is the object's own table, last entry the cull distance; empty
/// falls back to [`LOD_BANDS`] and [`TREE_CULL`]. A model with fewer levels
/// than the table has entries runs its last one on to the cull distance, and
/// one with more shares the last band — neither is a reason to draw nothing.
fn band(i: usize, count: usize, bands: &[f32]) -> (f32, f32) {
    let (table, cull) = if bands.is_empty() {
        (&LOD_BANDS[..], TREE_CULL)
    } else {
        (&bands[..bands.len() - 1], bands[bands.len() - 1])
    };
    let at = |k: usize| match table.last() {
        Some(last) => table.get(k).copied().unwrap_or(*last).min(cull),
        None => cull,
    };
    let start = if i == 0 { 0.0 } else { at(i - 1) };
    let end = if i + 1 >= count { cull } else { at(i) };
    (start, end.max(start))
}

/// Flattens a loaded glTF into mesh parts: the scene's nodes walked from the
/// roots with their transforms accumulated, each primitive with the
/// `StandardMaterial` the loader made for its material.
fn resolve(
    gltf: &Gltf,
    path: Option<&AssetPath>,
    nodes: &Assets<GltfNode>,
    meshes: &Assets<GltfMesh>,
    assets: &AssetServer,
) -> Option<TreeModel> {
    let path = path?;
    // The `Gltf` asset lists every node but not which ones the scene starts
    // at; a node no other node has as a child is a root.
    let mut is_child: Vec<AssetId<GltfNode>> = Vec::new();
    for handle in &gltf.nodes {
        if let Some(node) = nodes.get(handle) {
            is_child.extend(node.children.iter().map(Handle::id));
        }
    }
    let mut levels: BTreeMap<u8, Vec<Part>> = BTreeMap::new();
    let mut stack: Vec<(Handle<GltfNode>, Transform, u8)> = gltf
        .nodes
        .iter()
        .filter(|h| !is_child.contains(&h.id()))
        .map(|h| (h.clone(), Transform::IDENTITY, 0))
        .collect();
    while let Some((handle, parent, inherited)) = stack.pop() {
        let Some(node) = nodes.get(&handle) else {
            continue;
        };
        let transform = parent * node.transform;
        // A level named on a node applies to everything below it.
        let level = lod_level(&node.name).unwrap_or(inherited);
        if let Some(mesh) = node.mesh.as_ref().and_then(|m| meshes.get(m)) {
            for primitive in &mesh.primitives {
                let label = primitive
                    .material
                    .as_ref()
                    .and_then(|m| m.path())
                    .and_then(|p| p.label())
                    .map(str::to_owned)
                    .unwrap_or_else(|| GltfAssetLabel::DefaultMaterial.to_string());
                // The loader files a `StandardMaterial` next to every glTF
                // material, under the material's label plus `/std`.
                let material = assets.load(path.clone_owned().with_label(format!("{label}/std")));
                levels.entry(level).or_default().push(Part {
                    mesh: primitive.mesh.clone(),
                    material,
                    transform,
                });
            }
        }
        for child in &node.children {
            stack.push((child.clone(), transform, level));
        }
    }
    if levels.is_empty() {
        return None;
    }
    Some(TreeModel {
        levels: levels.into_iter().collect(),
    })
}

/// Spawns the trees, objects, people and walkers of a tile under its entity.
/// The trees wait for their models ([`PendingTrees`]); the objects and the
/// people are scenes and wait on their own (a person is finished by
/// [`crate::people::dress_people`] once its hierarchy is there). A scenery
/// object carries the roster with it, for the walkways its model may bring
/// ([`crate::people::bind_walkways`]); the tile's own walkways get their
/// walkers here, in the tile's frame, moved by the clock from then on.
pub fn spawn_scatter(
    tile: &mut EntityCommands,
    trees: Vec<Tree>,
    objects: &[SceneryInstance],
    people: &[PersonInstance],
    walkways: &[Walkway],
    catalog: &WorldCatalog,
) {
    if !trees.is_empty() {
        // The wood is a child of the tile, and the trees are children of it.
        let cull = catalog.cull_of(&trees) + tile_reach(&trees);
        tile.with_children(|parent| {
            parent.spawn((
                Wood { cull },
                Transform::IDENTITY,
                Visibility::default(),
                PendingTrees::new(trees, catalog),
                Scattered,
            ));
        });
    }
    tile.with_children(|parent| {
        for object in objects {
            let transform = Transform::from_translation(Vec3::from(object.pos))
                .with_rotation(Quat::from_array(object.rotation));
            let scene = catalog
                .objects
                .get(object.object as usize)
                .and_then(|s| s.clone());
            match scene {
                Some(scene) => {
                    parent.spawn((
                        WorldAssetRoot(scene),
                        transform,
                        VisibilityRange::abrupt(0.0, OBJECT_CULL),
                        SceneryIndex(object.index),
                        WalkwayHost {
                            people: catalog.people.clone(),
                        },
                        Scattered,
                    ));
                }
                None => {
                    let (mesh, material) = catalog.placeholder_object.clone();
                    parent.spawn((
                        Mesh3d(mesh),
                        MeshMaterial3d(material),
                        transform * Transform::from_xyz(0.0, 1.0, 0.0),
                        SceneryIndex(object.index),
                        Scattered,
                    ));
                }
            }
        }
        // A person whose character no installed mod has is simply not there —
        // the catalog logged the name once; a magenta block on a platform
        // would be a worse answer than a gap in the crowd.
        for person in people {
            let Some(character) = catalog.people.get(person.character) else {
                continue;
            };
            parent.spawn((
                person_bundle(character, person.pose, person.phase, PERSON_CULL),
                Transform::from_translation(Vec3::from(person.pos))
                    .with_rotation(Quat::from_array(person.rotation)),
                Scattered,
            ));
        }
        // The walkers start where clock zero puts them; `move_strollers`
        // has them where the clock is before their scenes have even loaded.
        for walkway in walkways {
            spawn_strollers(
                parent,
                Arc::new(walkway.clone()),
                &catalog.people,
                0.0,
                Scattered,
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One level runs from the camera to the cull distance; three levels hand
    /// over at the bands and the last one runs to the cull distance.
    #[test]
    fn lod_bands_cover_the_range_without_gaps() {
        assert_eq!(band(0, 1, &[]), (0.0, TREE_CULL));
        let three: Vec<(f32, f32)> = (0..3).map(|i| band(i, 3, &[])).collect();
        assert_eq!(three[0].0, 0.0);
        assert_eq!(three[2].1, TREE_CULL);
        for pair in three.windows(2) {
            assert_eq!(pair[0].1, pair[1].0, "levels butt together");
        }
    }

    /// A species' own table wins, its last entry being the cull distance. A
    /// model with fewer levels than the table has bands runs its last level to
    /// the cull distance rather than stopping early, and one with more levels
    /// than bands keeps drawing rather than collapsing to zero width.
    #[test]
    fn a_species_names_its_own_bands() {
        let table = [20.0, 70.0, 200.0, 700.0];
        let four: Vec<(f32, f32)> = (0..4).map(|i| band(i, 4, &table)).collect();
        assert_eq!(four[0], (0.0, 20.0));
        assert_eq!(four[3], (200.0, 700.0));

        let two: Vec<(f32, f32)> = (0..2).map(|i| band(i, 2, &table)).collect();
        assert_eq!(two, vec![(0.0, 20.0), (20.0, 700.0)]);

        let five: Vec<(f32, f32)> = (0..5).map(|i| band(i, 5, &table)).collect();
        assert_eq!(five[4], (200.0, 700.0));
        for pair in five.windows(2) {
            assert!(pair[0].1 <= pair[1].1, "bands never run backwards");
        }

        // A single-entry table is a cull distance and nothing else.
        assert_eq!(band(0, 1, &[400.0]), (0.0, 400.0));
    }
}
