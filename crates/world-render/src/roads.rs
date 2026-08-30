//! Drawing the roads: the line's carriageways as draped surfaces with a
//! shader of their own.
//!
//! One material per surface kind, not per road. What differs between two
//! asphalt roads is their width and their markings — and both of those ride
//! in the mesh's vertex colours and UVs, the way the fields' crops do. So a
//! tile costs one draw per surface kind on it, and the whole line costs two.
//!
//! **Multiplayer.** Nothing here is state: the uniform is a function of the
//! weather, the mesh is a function of the line and the elevation data.

use crate::weather;
use bevy::asset::embedded_asset;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;
use content::RoadPatch;

/// The material a carriageway is drawn with.
pub type RoadMaterial = ExtendedMaterial<StandardMaterial, RoadExt>;

/// What `roads.wgsl` needs to know about the surface it is drawing.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct RoadExt {
    #[uniform(100)]
    pub weather: weather::WeatherParams,
    /// The carriageway's own look — asphalt or concrete — and its normal
    /// map. Program assets, not the module's: nothing about a road varies
    /// from module to module but its geometry and its markings.
    #[texture(101)]
    #[sampler(102)]
    pub texture: Handle<Image>,
    #[texture(103)]
    #[sampler(104)]
    pub normal_map: Handle<Image>,
}

impl MaterialExtension for RoadExt {
    fn fragment_shader() -> ShaderRef {
        "embedded://world_render/roads.wgsl".into()
    }
}

/// Registers the road material. Part of
/// [`WorldRenderPlugin`](crate::WorldRenderPlugin) — both programs draw the
/// same roads.
pub(crate) fn plugin(app: &mut App) {
    embedded_asset!(app, "roads.wgsl");
    embedded_asset!(app, "roads/asphalt.jpg");
    embedded_asset!(app, "roads/asphalt_nor.jpg");
    embedded_asset!(app, "roads/concrete.jpg");
    embedded_asset!(app, "roads/concrete_nor.jpg");
    app.add_plugins(MaterialPlugin::<RoadMaterial>::default())
        .init_resource::<RoadMaterials>();
}

/// The two road materials, made on first use and shared by every tile.
#[derive(Resource, Default)]
pub struct RoadMaterials {
    by_surface: std::collections::HashMap<content::route::RoadSurface, Handle<RoadMaterial>>,
}

impl RoadMaterials {
    /// The material for a surface, made on first use. The textures are the
    /// program's own — CC0 (ambientCG), like the train ground's.
    pub fn get(
        &mut self,
        surface: content::route::RoadSurface,
        materials: &mut Assets<RoadMaterial>,
        server: &AssetServer,
    ) -> Handle<RoadMaterial> {
        self.by_surface
            .entry(surface)
            .or_insert_with(|| {
                let (color, normal) = match surface {
                    content::route::RoadSurface::Asphalt => (
                        "embedded://world_render/roads/asphalt.jpg",
                        "embedded://world_render/roads/asphalt_nor.jpg",
                    ),
                    content::route::RoadSurface::Concrete => (
                        "embedded://world_render/roads/concrete.jpg",
                        "embedded://world_render/roads/concrete_nor.jpg",
                    ),
                };
                materials.add(RoadMaterial {
                    base: StandardMaterial {
                        base_color: Color::WHITE,
                        perceptual_roughness: 0.88,
                        metallic: 0.0,
                        ..default()
                    },
                    extension: RoadExt {
                        weather: weather::WeatherParams::default(),
                        texture: server.load(color),
                        normal_map: server.load(normal),
                    },
                })
            })
            .clone()
    }

    pub fn is_empty(&self) -> bool {
        self.by_surface.is_empty()
    }
}

/// Marks a road surface — one surface kind's worth on one tile, a child of
/// the tile it lies on, so it streams in and out with the ground under it.
/// `sources` are the line's roads that went into it, in line order — what a
/// click on it would select.
#[derive(Component, Debug, Clone)]
pub struct RoadSurfaceMark {
    pub surface: content::route::RoadSurface,
    pub sources: Vec<u32>,
}

/// What [`spawn_roads`] needs besides the patches. Bundled because both
/// programs pass the same things from the same resources.
pub struct RoadDraw<'a> {
    pub materials: &'a mut RoadMaterials,
    pub assets: &'a mut Assets<RoadMaterial>,
    pub server: &'a AssetServer,
}

/// Hangs a tile's roads under the tile.
///
/// Children rather than entities of their own: the terrain streaming despawns
/// a tile with everything below it, so a carriageway never outlives the
/// ground it was draped on, and nothing has to be tracked twice — the same
/// arrangement the fields and the waters hang by.
pub fn spawn_roads(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    draw: &mut RoadDraw,
    tile: Entity,
    patches: &[RoadPatch],
) {
    if patches.is_empty() {
        return;
    }
    let Ok(mut entity) = commands.get_entity(tile) else {
        return;
    };
    let RoadDraw {
        materials,
        assets,
        server,
    } = draw;
    entity.with_children(|parent| {
        for patch in patches {
            let material = materials.get(patch.surface, assets, server);
            parent.spawn((
                Mesh3d(meshes.add(mesh(patch))),
                MeshMaterial3d(material),
                // The patch is already in the tile's own frame.
                Transform::IDENTITY,
                RoadSurfaceMark {
                    surface: patch.surface,
                    sources: patch.sources.clone(),
                },
            ));
        }
    });
}

/// A road's surface as a Bevy mesh, in the tile's own frame.
pub fn mesh(patch: &RoadPatch) -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::mesh::{Indices, PrimitiveTopology};

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, patch.positions.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, patch.normals.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, patch.uvs.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, patch.colors.clone());
    mesh.insert_indices(Indices::U32(patch.indices.clone()));
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mesh carries the attribute set the shader reads, and the vertex
    /// colour is where the markings live.
    #[test]
    fn a_patch_is_a_marked_surface() {
        let patch = RoadPatch {
            surface: content::route::RoadSurface::Asphalt,
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            normals: vec![[0.0, 1.0, 0.0]; 3],
            uvs: vec![[0.5, 0.0]; 3],
            colors: vec![[1.0, 1.0, 3.0, 1.0]; 3],
            indices: vec![0, 1, 2],
            sources: vec![0],
        };
        let mesh = mesh(&patch);
        assert_eq!(mesh.count_vertices(), 3);
        for attribute in [Mesh::ATTRIBUTE_UV_0, Mesh::ATTRIBUTE_COLOR] {
            assert!(mesh.attribute(attribute).is_some(), "{attribute:?}");
        }
        let Some(bevy::mesh::VertexAttributeValues::Float32x4(colors)) =
            mesh.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("colors");
        };
        assert!((colors[0][2] - 3.0).abs() < 1e-6, "the half-width");
    }
}
