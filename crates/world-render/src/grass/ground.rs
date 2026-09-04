//! The ground cache: what the meadow stands on, as one top-down texture.
//!
//! Every [`GroundSurface`](super::GroundSurface) around the camera is drawn
//! orthographically into a square of [`TEXELS`]² texels covering the grass
//! range plus a margin. Red is the height in render space, green the grass
//! weight — the terrain's own splat share, zero on a field, a road or a lake,
//! and the sentinel [`INVALID_HEIGHT`] where nothing was drawn at all. The
//! scatter pass reads its blades' feet off this, so a blade stands exactly
//! on the drawn ground, and stands nowhere the ground is covered.
//!
//! It is drawn again only when it has to be: the camera has left the margin,
//! a tile has streamed in or out, the origin was rebased (which moves every
//! surface at once), or a mesh was not on the GPU yet last time. In a normal
//! frame this pass does nothing.

use std::hash::{Hash, Hasher};

use bevy::core_pipeline::Core3dSystems;
use bevy::core_pipeline::schedule::Core3d;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::prelude::*;
use bevy::render::mesh::allocator::MeshAllocator;
use bevy::render::mesh::{RenderMesh, RenderMeshBufferInfo};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{storage_buffer_read_only, uniform_buffer};
use bevy::render::render_resource::{
    BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
    CachedRenderPipelineId, ColorTargetState, ColorWrites, CompareFunction, DepthStencilState,
    Extent3d, FragmentState, LoadOp, Operations, PipelineCache, PrimitiveState,
    RenderPassColorAttachment, RenderPassDepthStencilAttachment, RenderPassDescriptor,
    RenderPipelineDescriptor, ShaderStages, ShaderType, SpecializedMeshPipeline,
    SpecializedMeshPipelineError, SpecializedMeshPipelines, StorageBuffer, StoreOp,
    TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureView,
    TextureViewDescriptor, UniformBuffer, VertexState,
};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery};
use bevy::render::view::ExtractedView;
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};

use super::{ExtractedGroundSurface, GrassEnvironment, GrassView};

/// Texels on a side of the cache. At the widest range (400 m + margin) that
/// is still under a metre a texel, on a terrain whose grid is four.
pub(super) const TEXELS: u32 = 1024;
/// How far past the grass range the cache reaches, and so how far the camera
/// may travel before it is drawn again \[m\].
const MARGIN: f32 = 64.0;
/// Half the height band the depth test resolves, around the camera \[m\].
const HEIGHT_RANGE: f32 = 1200.0;
/// The height written where nothing was drawn. Far below any terrain.
pub(super) const INVALID_HEIGHT: f32 = -1.0e9;

#[derive(ShaderType, Clone, Copy)]
struct GroundUniform {
    /// xz = centre, y = reference height, w = half side \[m\].
    centre: Vec4,
    /// x = [`HEIGHT_RANGE`], y = [`INVALID_HEIGHT`].
    range: Vec4,
}

#[derive(ShaderType, Clone, Copy)]
struct GroundDraw {
    world_from_local: Mat4,
    /// x = 1 for a surface that excludes grass.
    flags: UVec4,
}

struct QueuedDraw {
    mesh: AssetId<Mesh>,
    pipeline: CachedRenderPipelineId,
}

/// The cache itself and the bookkeeping that decides when to draw it again.
#[derive(Resource)]
pub(super) struct GroundCache {
    pub color_view: TextureView,
    depth_view: TextureView,
    /// Where the cache was drawn from: xz centre, y reference height.
    pub centre: Vec3,
    pub half_extent: f32,
    /// Drawn at least once, so the scatter pass may read it.
    pub valid: bool,
    /// Has to be drawn this frame.
    dirty: bool,
    /// A surface's GPU mesh was missing last time; draw again when it lands.
    incomplete: bool,
    surfaces: u64,
    uniform: UniformBuffer<GroundUniform>,
    draws: StorageBuffer<Vec<GroundDraw>>,
    bind_group: Option<BindGroup>,
    queued: Vec<QueuedDraw>,
}

#[derive(Resource)]
pub(super) struct GroundPipeline {
    shader: Handle<Shader>,
    layout: BindGroupLayoutDescriptor,
}

impl SpecializedMeshPipeline for GroundPipeline {
    type Key = ();

    fn specialize(
        &self,
        _key: (),
        layout: &MeshVertexBufferLayoutRef,
    ) -> Result<RenderPipelineDescriptor, SpecializedMeshPipelineError> {
        let vertex = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(1),
        ])?;
        Ok(RenderPipelineDescriptor {
            label: Some("grass_ground".into()),
            layout: vec![self.layout.clone()],
            vertex: VertexState {
                shader: self.shader.clone(),
                buffers: vec![vertex],
                ..default()
            },
            fragment: Some(FragmentState {
                shader: self.shader.clone(),
                targets: vec![Some(ColorTargetState {
                    format: TextureFormat::Rg32Float,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            primitive: PrimitiveState {
                cull_mode: None,
                ..default()
            },
            // Highest surface wins: the depth is the height.
            depth_stencil: Some(DepthStencilState {
                format: TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(CompareFunction::Greater),
                stencil: default(),
                bias: default(),
            }),
            ..default()
        })
    }
}

pub(super) fn plugin(app: &mut App) {
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app
        .init_resource::<SpecializedMeshPipelines<GroundPipeline>>()
        .add_systems(RenderStartup, init)
        .add_systems(Render, prepare.in_set(RenderSystems::PrepareResources))
        .add_systems(Core3d, pass.before(Core3dSystems::Prepass));
}

fn init(mut commands: Commands, device: Res<RenderDevice>, asset_server: Res<AssetServer>) {
    let size = Extent3d {
        width: TEXELS,
        height: TEXELS,
        depth_or_array_layers: 1,
    };
    let color = device.create_texture(&TextureDescriptor {
        label: Some("grass_ground_color"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rg32Float,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let depth = device.create_texture(&TextureDescriptor {
        label: Some("grass_ground_depth"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Depth32Float,
        usage: TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    commands.insert_resource(GroundCache {
        color_view: color.create_view(&TextureViewDescriptor::default()),
        depth_view: depth.create_view(&TextureViewDescriptor::default()),
        centre: Vec3::ZERO,
        half_extent: 0.0,
        valid: false,
        dirty: false,
        incomplete: false,
        surfaces: 0,
        uniform: UniformBuffer::from(GroundUniform {
            centre: Vec4::ZERO,
            range: Vec4::ZERO,
        }),
        draws: StorageBuffer::default(),
        bind_group: None,
        queued: Vec::new(),
    });
    commands.insert_resource(GroundPipeline {
        shader: asset_server.load("embedded://world_render/grass/ground.wgsl"),
        layout: BindGroupLayoutDescriptor::new(
            "grass_ground",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::VERTEX,
                (
                    uniform_buffer::<GroundUniform>(false),
                    storage_buffer_read_only::<Vec<GroundDraw>>(false),
                ),
            ),
        ),
    });
}

/// Decides whether the cache is drawn this frame and, if so, lays out the
/// draws: one per surface whose mesh is on the GPU.
#[allow(clippy::too_many_arguments)]
pub(super) fn prepare(
    mut cache: ResMut<GroundCache>,
    pipeline: Res<GroundPipeline>,
    mut pipelines: ResMut<SpecializedMeshPipelines<GroundPipeline>>,
    pipeline_cache: Res<PipelineCache>,
    environment: Res<GrassEnvironment>,
    views: Query<&ExtractedView, With<GrassView>>,
    surfaces: Query<&ExtractedGroundSurface>,
    meshes: Res<RenderAssets<RenderMesh>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    cache.queued.clear();
    cache.dirty = false;
    if !environment.settings.enabled {
        return;
    }
    let Some(view) = views.iter().next() else {
        return;
    };
    let eye = view.world_from_view.translation();
    let half_extent = environment.settings.range + MARGIN;

    // A fingerprint of the surfaces and where they stand. A tile streaming
    // in or out changes it, and so does an origin rebase, which moves them
    // all at once.
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut count = 0usize;
    for surface in &surfaces {
        surface.mesh.hash(&mut hasher);
        for value in surface.world_from_local.w_axis.to_array() {
            value.to_bits().hash(&mut hasher);
        }
        count += 1;
    }
    count.hash(&mut hasher);
    let surfaces_now = hasher.finish();

    let moved = (eye.xz() - cache.centre.xz()).length() > MARGIN * 0.75
        || (eye.y - cache.centre.y).abs() > HEIGHT_RANGE * 0.5;
    let wanted = !cache.valid
        || moved
        || surfaces_now != cache.surfaces
        || half_extent != cache.half_extent
        || cache.incomplete;
    if !wanted {
        return;
    }
    cache.centre = eye;
    cache.half_extent = half_extent;
    cache.surfaces = surfaces_now;

    let mut draws = Vec::new();
    let mut incomplete = false;
    for surface in &surfaces {
        let Some(mesh) = meshes.get(surface.mesh) else {
            // Streamed in this frame; the GPU copy follows next frame.
            incomplete = true;
            continue;
        };
        // A mesh without colours cannot say where its grass is; it is left
        // out for good rather than asked again every frame.
        let Ok(id) = pipelines.specialize(&pipeline_cache, &pipeline, (), &mesh.layout) else {
            continue;
        };
        draws.push(GroundDraw {
            world_from_local: surface.world_from_local,
            flags: UVec4::new(u32::from(surface.excluded), 0, 0, 0),
        });
        cache.queued.push(QueuedDraw {
            mesh: surface.mesh,
            pipeline: id,
        });
    }
    cache.incomplete = incomplete;
    if draws.is_empty() {
        cache.bind_group = None;
        // Nothing to stand on yet: the cache is drawn (clear) all the same, so
        // the scatter pass reads the sentinel rather than stale ground.
        cache.dirty = true;
        return;
    }

    cache.uniform.set(GroundUniform {
        centre: Vec4::new(eye.x, eye.y, eye.z, half_extent),
        range: Vec4::new(HEIGHT_RANGE, INVALID_HEIGHT, 0.0, 0.0),
    });
    cache.uniform.write_buffer(&device, &queue);
    cache.draws.set(draws);
    cache.draws.write_buffer(&device, &queue);
    let layout = pipeline_cache.get_bind_group_layout(&pipeline.layout);
    let (Some(uniform), Some(draws)) = (cache.uniform.binding(), cache.draws.binding()) else {
        return;
    };
    let bind_group = device.create_bind_group(
        "grass_ground",
        &layout,
        &BindGroupEntries::sequential((uniform, draws)),
    );
    cache.bind_group = Some(bind_group);
    cache.dirty = true;
}

/// Draws the cache when [`prepare`] asked for it.
pub(super) fn pass(
    view: ViewQuery<(), With<GrassView>>,
    mut cache: ResMut<GroundCache>,
    pipeline_cache: Res<PipelineCache>,
    meshes: Res<RenderAssets<RenderMesh>>,
    allocator: Res<MeshAllocator>,
    mut ctx: RenderContext,
) {
    let _ = view;
    if !cache.dirty {
        return;
    }
    let mut all_ready = true;
    {
        let color_attachment = RenderPassColorAttachment {
            view: &cache.color_view,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(wgpu::Color {
                    r: f64::from(INVALID_HEIGHT),
                    g: 0.0,
                    b: 0.0,
                    a: 0.0,
                }),
                store: StoreOp::Store,
            },
        };
        let depth_attachment = RenderPassDepthStencilAttachment {
            view: &cache.depth_view,
            depth_ops: Some(Operations {
                load: LoadOp::Clear(0.0),
                store: StoreOp::Store,
            }),
            stencil_ops: None,
        };
        let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("grass_ground_pass"),
            color_attachments: &[Some(color_attachment)],
            depth_stencil_attachment: Some(depth_attachment),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if let Some(bind_group) = &cache.bind_group {
            pass.set_bind_group(0, bind_group, &[]);
            for (k, draw) in cache.queued.iter().enumerate() {
                let Some(pipeline) = pipeline_cache.get_render_pipeline(draw.pipeline) else {
                    // Still compiling: drawn again next frame.
                    all_ready = false;
                    continue;
                };
                let (Some(mesh), Some(vertices)) = (
                    meshes.get(draw.mesh),
                    allocator.mesh_vertex_slice(&draw.mesh),
                ) else {
                    all_ready = false;
                    continue;
                };
                pass.set_render_pipeline(pipeline);
                pass.set_vertex_buffer(0, vertices.buffer.slice(..));
                let instance = k as u32..k as u32 + 1;
                match &mesh.buffer_info {
                    RenderMeshBufferInfo::Indexed {
                        count,
                        index_format,
                    } => {
                        let Some(indices) = allocator.mesh_index_slice(&draw.mesh) else {
                            all_ready = false;
                            continue;
                        };
                        pass.set_index_buffer(indices.buffer.slice(..), *index_format);
                        pass.draw_indexed(
                            indices.range.start..indices.range.start + count,
                            vertices.range.start as i32,
                            instance,
                        );
                    }
                    RenderMeshBufferInfo::NonIndexed => {
                        pass.draw(vertices.range.clone(), instance);
                    }
                }
            }
        }
    }
    cache.valid = true;
    if !all_ready {
        cache.incomplete = true;
    }
}
