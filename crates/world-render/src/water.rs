//! Drawing the water (plan ch. 14): the line's lakes and rivers as surfaces
//! with a shader of their own.
//!
//! One material for every body of water there is. What differs between a
//! mill pond and the Rhine is the wind over it, the rain on it, and how deep
//! the bed lies below the surface — and all three arrive without a byte of
//! per-water data: the depth is baked into the mesh's vertex colours by
//! `content::water`, the weather rides the same uniform every other surface
//! reads (`crate::weather`), and the waves are the shader's own business.
//!
//! **Multiplayer.** Nothing here is state. The uniform is a function of
//! [`Sky`](crate::sky::Sky), which is a function of the scenario clock, and
//! the mesh is a function of the line and the elevation data.

use crate::weather;
use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;
use content::WaterPatch;

/// The material a water surface is drawn with.
pub type WaterMaterial = ExtendedMaterial<StandardMaterial, WaterExt>;

/// The water's extension — one uniform, one shader.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone, Default)]
pub struct WaterExt {
    /// What the weather is doing — the same uniform the terrain and the
    /// objects carry, written by `weather::update` (plan 14.1). The wind makes
    /// the waves, the rain rings them, the cloud shadow lies on the surface.
    #[uniform(100)]
    pub weather: weather::WeatherParams,
}

impl MaterialExtension for WaterExt {
    fn fragment_shader() -> ShaderRef {
        "embedded://world_render/water.wgsl".into()
    }

    // The ground shows through where the water is shallow, so a shore reads
    // as a shore instead of as a hard edge where two opaque planes meet.
    fn alpha_mode() -> Option<AlphaMode> {
        Some(AlphaMode::Blend)
    }

    // A surface that blends has no business in the depth prepass — what it
    // hides behind it is meant to stay visible — and a horizontal plane casts
    // no shadow worth the fill rate.
    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }
}

/// Registers the water material. Part of
/// [`WorldRenderPlugin`](crate::WorldRenderPlugin) — both programs draw the
/// same water.
pub(crate) fn plugin(app: &mut App) {
    embedded_asset!(app, "water.wgsl");
    app.add_plugins(MaterialPlugin::<WaterMaterial>::default())
        .init_resource::<WaterMaterials>();
}

/// The one water material, made on first use and shared by every tile —
/// a lake and a river differ in their meshes, never in their material.
#[derive(Resource, Default)]
pub struct WaterMaterials {
    handle: Option<Handle<WaterMaterial>>,
}

impl WaterMaterials {
    pub fn get(&mut self, materials: &mut Assets<WaterMaterial>) -> Handle<WaterMaterial> {
        self.handle
            .get_or_insert_with(|| {
                materials.add(WaterMaterial {
                    base: StandardMaterial {
                        // The shader writes the colour from the water column; this
                        // is only what it starts from.
                        base_color: Color::srgb(0.07, 0.12, 0.11),
                        perceptual_roughness: 0.08,
                        ..default()
                    },
                    extension: WaterExt {
                        weather: weather::WeatherParams::default(),
                    },
                })
            })
            .clone()
    }
}

/// Marks a water surface — one tile's worth of a body, a child of the tile it
/// lies on, so it streams in and out with the ground under it. `sources` are
/// the line's water bodies that went into it, in line order — what a click on
/// it would select.
#[derive(Component, Debug, Clone)]
pub struct WaterSurface {
    pub sources: Vec<u32>,
}

/// Hangs a tile's water under the tile.
///
/// Children rather than entities of their own: the terrain streaming despawns
/// a tile with everything below it, so a surface never outlives the ground it
/// was cut for, and nothing has to be tracked twice — the same arrangement
/// the farmland hangs by.
pub fn spawn_waters(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut WaterMaterials,
    assets: &mut Assets<WaterMaterial>,
    tile: Entity,
    patches: &[WaterPatch],
) {
    if patches.is_empty() {
        return;
    }
    let Ok(mut entity) = commands.get_entity(tile) else {
        return;
    };
    let material = materials.get(assets);
    entity.with_children(|parent| {
        for patch in patches {
            parent.spawn((
                Mesh3d(meshes.add(mesh(patch))),
                MeshMaterial3d(material.clone()),
                // The patch is already in the tile's own frame.
                Transform::IDENTITY,
                WaterSurface {
                    sources: patch.sources.clone(),
                },
            ));
        }
    });
}

/// A water surface as a Bevy mesh, in the tile's own frame.
pub fn mesh(patch: &WaterPatch) -> Mesh {
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

    /// The water carries its depth to the shader in the vertex colour, and
    /// its mesh keeps the attribute set the shader reads.
    #[test]
    fn a_patch_is_a_coloured_surface() {
        let patch = WaterPatch {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            normals: vec![[0.0, 1.0, 0.0]; 3],
            uvs: vec![[0.0, 0.0]; 3],
            colors: vec![[1.5, 0.0, 0.0, 1.0]; 3],
            indices: vec![0, 1, 2],
            sources: vec![0],
        };
        let mesh = mesh(&patch);
        assert_eq!(mesh.count_vertices(), 3);
        let Some(bevy::mesh::VertexAttributeValues::Float32x4(colors)) =
            mesh.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("colors");
        };
        assert!((colors[0][0] - 1.5).abs() < 1e-6, "{:?}", colors[0]);
    }
}
