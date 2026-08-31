//! The material of the overhead line conductors — see `conductors.wgsl` for
//! what it draws and why a wire cannot simply be geometry of its own size.
//!
//! One mesh and one material per tile. A conductor carries nothing that varies
//! from line to line — every wire in the country is the same grey
//! aluminium-steel rope — so there is no per-line material: the wire's own
//! width travels in the mesh, and everything else is the same handful of
//! numbers for the whole world.
//!
//! Conductors cast **no shadow** and take **no prepass**. The shadow map's
//! texel is metres wide at the distance a power line is seen at, and a wire in
//! it is a crawling dotted line across the field below — worse than no shadow,
//! and it costs a second pass over every wire on the tile. The prepass is out
//! because the vertex stage moves the vertices: a depth buffer written from the
//! unmoved centre line would disagree with what is drawn.
//!
//! **Multiplayer.** Nothing here is state: the mesh is a function of the line
//! and the elevation data, and two clients build the same wires.

use bevy::asset::embedded_asset;
use bevy::camera::visibility::VisibilityRange;
use bevy::light::NotShadowCaster;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
use content::ConductorPatch;

/// The least a wire is ever drawn \[px\].
///
/// Below one pixel a line is not thin, it is *dotted* — the rasteriser hits
/// some pixel centres and misses others, and the pattern crawls as the camera
/// moves. One and a half leaves enough of a margin that the chord profile still
/// has somewhere to fall off, so the wire keeps a soft edge instead of turning
/// into a hard two-pixel bar.
const MIN_PIXELS: f32 = 1.5;

/// How much of the shading is sky rather than sun.
///
/// High, and on purpose: a conductor is a matt dark rope seen against the sky
/// far more often than against the ground, and a wire that goes black whenever
/// the sun is behind it reads as a crack in the world rather than as a cable.
const AMBIENT_SHARE: f32 = 0.72;

/// Past this the wires are not drawn at all \[m\].
///
/// The shader has already thinned them to almost nothing out here — a 380 kV
/// conductor at three kilometres covers about three hundredths of a pixel — so
/// what this saves is the drawing, not the look. The masts stay: they are the
/// landmark, and they are still four pixels of lattice at four kilometres.
const CULL: f32 = 3_000.0;

pub(crate) fn plugin(app: &mut App) {
    embedded_asset!(app, "conductors.wgsl");
    app.add_plugins(MaterialPlugin::<ConductorMaterial>::default())
        .init_resource::<ConductorMaterials>();
}

/// What the shader is told about the wire it is drawing.
#[derive(ShaderType, Debug, Clone, Copy)]
pub struct ConductorParams {
    /// x = the least the wire may be drawn \[px\], y = the ambient share of the
    /// shading, z and w free.
    pub params: Vec4,
    /// The metal; `a` scales the coverage of the whole line.
    pub color: Vec4,
}

impl Default for ConductorParams {
    fn default() -> Self {
        Self {
            params: Vec4::new(MIN_PIXELS, AMBIENT_SHARE, 0.0, 0.0),
            // Weathered aluminium-steel: dark, and darker than the galvanised
            // mast it hangs on, which is what makes the wire read as a wire
            // against a bright sky.
            color: Vec4::new(0.14, 0.145, 0.15, 1.0),
        }
    }
}

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone, Default)]
pub struct ConductorMaterial {
    #[uniform(0)]
    pub params: ConductorParams,
}

impl Material for ConductorMaterial {
    fn vertex_shader() -> ShaderRef {
        "embedded://world_render/conductors.wgsl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "embedded://world_render/conductors.wgsl".into()
    }

    /// Blended, because coverage is the whole mechanism: a distant wire is a
    /// fraction of a pixel of metal, and the only honest way to draw a fraction
    /// of a pixel is to be that fraction transparent. A mask would put it back
    /// where it started — on or off, and dotted.
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn enable_shadows() -> bool {
        false
    }

    /// The vertex stage moves the vertices, so a prepass would write depth for
    /// a line that is not where the depth says it is.
    fn enable_prepass() -> bool {
        false
    }

    /// The band turns to face the camera, so which way round its two triangles
    /// wind depends on where the camera stands. Culled, half of every line
    /// disappears.
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// The one conductor material, made on first use and shared by every tile.
#[derive(Resource, Default)]
pub struct ConductorMaterials(Option<Handle<ConductorMaterial>>);

impl ConductorMaterials {
    pub fn get(&mut self, materials: &mut Assets<ConductorMaterial>) -> Handle<ConductorMaterial> {
        self.0
            .get_or_insert_with(|| {
                materials.add(ConductorMaterial {
                    params: ConductorParams::default(),
                })
            })
            .clone()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_none()
    }
}

/// Marks a tile's conductors — a child of the tile, so the wires stream in and
/// out with the ground the masts stand on.
#[derive(Component, Debug, Clone, Copy)]
pub struct ConductorMark;

/// Hangs a tile's conductors under the tile.
pub fn spawn_conductors(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ConductorMaterial>,
    shared: &mut ConductorMaterials,
    tile: Entity,
    patches: &[ConductorPatch],
) {
    if patches.is_empty() {
        return;
    }
    let Ok(mut entity) = commands.get_entity(tile) else {
        return;
    };
    let material = shared.get(materials);
    entity.with_children(|parent| {
        for patch in patches {
            parent.spawn((
                Mesh3d(meshes.add(mesh(patch))),
                MeshMaterial3d(material.clone()),
                // The patch is already in the tile's own frame.
                Transform::IDENTITY,
                VisibilityRange::abrupt(0.0, CULL),
                NotShadowCaster,
                ConductorMark,
            ));
        }
    });
}

/// A tile's conductors as a Bevy mesh, in the tile's own frame.
///
/// The positions are the wires' centre lines; what the shader needs beyond them
/// rides in the tangent (the wire's direction) and the first UV (which side of
/// the centre line this vertex is, and how thick the wire really is).
pub fn mesh(patch: &ConductorPatch) -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::mesh::{Indices, PrimitiveTopology};

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, patch.positions.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, patch.across.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, patch.tangents.clone());
    mesh.insert_indices(Indices::U32(patch.indices.clone()));
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mesh carries the three attributes the shader reads, in the formats
    /// it declares them as. Bevy's mesh pipeline numbers them position 0,
    /// UV_0 2 and tangent 4 — `conductors.wgsl` names those locations, and a
    /// format that disagreed here would be a pipeline error at the first frame
    /// rather than a compile error.
    #[test]
    fn a_patch_becomes_a_mesh_the_shader_can_read() {
        let patch = ConductorPatch {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            tangents: vec![[1.0, 0.0, 0.0, 1.0]; 3],
            across: vec![[-1.0, 0.055], [-1.0, 0.055], [1.0, 0.055]],
            indices: vec![0, 1, 2],
        };
        let mesh = mesh(&patch);
        use bevy::mesh::VertexAttributeValues;
        assert_eq!(mesh.count_vertices(), 3);
        assert!(matches!(
            mesh.attribute(Mesh::ATTRIBUTE_POSITION),
            Some(VertexAttributeValues::Float32x3(_))
        ));
        assert!(matches!(
            mesh.attribute(Mesh::ATTRIBUTE_UV_0),
            Some(VertexAttributeValues::Float32x2(_))
        ));
        assert!(matches!(
            mesh.attribute(Mesh::ATTRIBUTE_TANGENT),
            Some(VertexAttributeValues::Float32x4(_))
        ));
        // No normal: the wire's direction is what the shader needs, and it
        // rides in the tangent. A normal here would only cost bandwidth.
        assert!(mesh.attribute(Mesh::ATTRIBUTE_NORMAL).is_none());
    }

    /// The wire's own half-width travels per vertex, so one material draws a
    /// 380 kV bundle and a 20 kV single conductor at their own sizes.
    #[test]
    fn the_true_width_rides_in_the_mesh() {
        let patch = ConductorPatch {
            positions: vec![[0.0, 0.0, 0.0]],
            tangents: vec![[1.0, 0.0, 0.0, 1.0]],
            across: vec![[1.0, 0.055]],
            indices: vec![],
        };
        let mesh = mesh(&patch);
        let Some(bevy::mesh::VertexAttributeValues::Float32x2(uv)) =
            mesh.attribute(Mesh::ATTRIBUTE_UV_0)
        else {
            panic!("uv");
        };
        assert!((uv[0][1] - 0.055).abs() < 1e-6, "the half-width");
    }
}
