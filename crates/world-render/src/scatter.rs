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
//! vehicles and signals) become [`VisibilityRange`] bands, and every tree is
//! culled past [`TREE_CULL`] — a tree at three kilometres is a pixel.
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
use bevy::camera::visibility::VisibilityRange;
use bevy::gltf::{Gltf, GltfAssetLabel, GltfMesh, GltfNode};
use bevy::prelude::*;
use bevy::world_serialization::WorldAsset;
use content::{SceneryInstance, Tree};
use sim_core::train::lod_level;
use std::collections::BTreeMap;
use track_model::TrackObject;

use crate::{Season, asset_path};

/// Past this distance no tree is drawn [m].
pub const TREE_CULL: f32 = 2_500.0;
/// Where one level of detail hands over to the next [m]: `_LOD0` up to the
/// first, `_LOD1` up to the second, and so on. The last level a model ships
/// runs on to [`TREE_CULL`].
// ponytail: fixed bands; a per-object table in `objects/*.ron` when a mod
// wants them — the vehicles have one, the trees have not needed it yet.
const LOD_BANDS: [f32; 3] = [200.0, 700.0, 1_500.0];
/// Scenery objects are culled here [m] — a mast is thinner than a tree.
pub const OBJECT_CULL: f32 = 3_000.0;

/// Render assets of everything a tile can carry: per tree species the glTF
/// of the mod object it names, per scenery object its scene, and the
/// placeholders for names no installed mod answers for.
#[derive(Clone)]
pub struct WorldCatalog {
    /// Indexed by [`Tree::object`]; `None` where the name resolved to no
    /// installed mod object.
    trees: Vec<Option<Handle<Gltf>>>,
    /// Indexed by [`SceneryInstance::object`].
    objects: Vec<Option<Handle<WorldAsset>>>,
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
    /// unknown names logged once and shown as placeholders.
    pub fn new(
        tree_names: &[String],
        object_names: &[String],
        registry: &BTreeMap<String, TrackObject>,
        assets: &AssetServer,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        season: Season,
    ) -> Self {
        let trees = tree_names
            .iter()
            .map(|name| match registry.get(name) {
                Some(object) => Some(assets.load(asset_path(season.model_of(object)))),
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
            placeholder_trees,
            placeholder_tree_material,
            placeholder_object,
        }
    }
}

/// Marker on every scenery object spawned from a tile, carrying the index of
/// its placement in the line file — the editor's selection speaks in those.
#[derive(Component, Clone, Copy)]
pub struct SceneryIndex(pub u32);

/// Marker on everything a tile carries besides its ground — what
/// [`crate::respawn_scatter`] clears before it places the tile's trees and
/// objects anew.
#[derive(Component)]
pub struct Scattered;

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
pub fn materialise_trees(
    mut commands: Commands,
    pending: Query<(Entity, &PendingTrees)>,
    mut models: ResMut<TreeModels>,
    assets: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    nodes: Res<Assets<GltfNode>>,
    gltf_meshes: Res<Assets<GltfMesh>>,
) {
    for (tile, pending) in &pending {
        let species: Vec<&Handle<Gltf>> = pending.catalog.trees.iter().flatten().collect();
        let ready = species.iter().all(|handle| {
            if models.resolved.contains_key(&handle.id()) {
                return true;
            }
            match assets.get_load_state(handle.id()) {
                Some(LoadState::Loaded) => {
                    let model = gltfs.get(*handle).and_then(|gltf| {
                        resolve(gltf, handle.path(), &nodes, &gltf_meshes, &assets)
                    });
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
        let Ok(mut tile) = commands.get_entity(tile) else {
            continue;
        };
        tile.remove::<PendingTrees>().with_children(|parent| {
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
    let model = tree
        .object
        .and_then(|i| catalog.trees.get(i as usize))
        .and_then(|handle| handle.as_ref())
        .and_then(|handle| models.resolved.get(&handle.id()))
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
    for (i, (_, parts)) in model.levels.iter().enumerate() {
        let range = VisibilityRange::abrupt(band_start(i), band_end(i, count));
        for part in parts {
            parent.spawn((
                Mesh3d(part.mesh.clone()),
                MeshMaterial3d(part.material.clone()),
                transform * part.transform,
                range.clone(),
                Scattered,
            ));
        }
    }
}

/// Where level `i` of a model begins [m].
fn band_start(i: usize) -> f32 {
    if i == 0 {
        0.0
    } else {
        LOD_BANDS[(i - 1).min(LOD_BANDS.len() - 1)]
    }
}

/// Where level `i` of a model with `count` levels ends [m].
fn band_end(i: usize, count: usize) -> f32 {
    if i + 1 >= count {
        TREE_CULL
    } else {
        LOD_BANDS[i.min(LOD_BANDS.len() - 1)]
    }
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

/// Spawns the trees and objects of a tile under its entity. The trees wait
/// for their models ([`PendingTrees`]); the objects are scenes and wait on
/// their own.
pub fn spawn_scatter(
    tile: &mut EntityCommands,
    trees: Vec<Tree>,
    objects: &[SceneryInstance],
    catalog: &WorldCatalog,
) {
    if !trees.is_empty() {
        tile.insert(PendingTrees::new(trees, catalog));
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
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One level runs from the camera to the cull distance; three levels hand
    /// over at the bands and the last one runs to the cull distance.
    #[test]
    fn lod_bands_cover_the_range_without_gaps() {
        assert_eq!((band_start(0), band_end(0, 1)), (0.0, TREE_CULL));
        let three: Vec<(f32, f32)> = (0..3).map(|i| (band_start(i), band_end(i, 3))).collect();
        assert_eq!(three[0].0, 0.0);
        assert_eq!(three[2].1, TREE_CULL);
        for pair in three.windows(2) {
            assert_eq!(pair[0].1, pair[1].0, "levels butt together");
        }
    }
}
