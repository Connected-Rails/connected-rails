//! The scatter pass and the indirect blade draws.
//!
//! Per frame: a compute dispatch over the patch grid around the camera fills
//! three instance lists and their indirect draw arguments, and one phase item
//! in the opaque pass issues the three draws. The vertex and fragment work is
//! `blades.wgsl`; the pipeline for it is Bevy's own mesh pipeline with the
//! shaders and the third bind group swapped, so every view-level detail —
//! MSAA, HDR, the shadow filter, the fog, the atmosphere — is whatever the
//! camera has, without a second copy of the logic here.

use bevy::core_pipeline::Core3dSystems;
use bevy::core_pipeline::core_3d::{Opaque3d, Opaque3dBatchSetKey, Opaque3dBinKey};
use bevy::core_pipeline::schedule::Core3d;
use bevy::ecs::query::ROQueryItem;
use bevy::ecs::system::SystemParamItem;
use bevy::ecs::system::lifetimeless::SRes;
use bevy::math::primitives::ViewFrustum;
use bevy::mesh::{
    MeshVertexBufferLayout, MeshVertexBufferLayoutRef, MeshVertexBufferLayouts, PrimitiveTopology,
    VertexBufferLayout,
};
use bevy::pbr::{
    MeshPipeline, MeshPipelineKey, MeshPipelineSystems, SetMeshViewBindGroup,
    SetMeshViewBindingArrayBindGroup, ViewKeyCache,
};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::mesh::allocator::MeshSlabs;
use bevy::render::render_phase::{
    AddRenderCommand, BinnedRenderPhaseType, DrawFunctions, InputUniformIndex, PhaseItem,
    RenderCommand, RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewBinnedRenderPhases,
};
use bevy::render::render_resource::binding_types::{
    storage_buffer_read_only_sized, storage_buffer_sized, texture_2d, uniform_buffer,
};
use bevy::render::render_resource::{
    BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, Buffer,
    BufferBinding, BufferDescriptor, BufferUsages, CachedComputePipelineId, CachedPipelineState,
    CachedRenderPipelineId, ComputePassDescriptor, ComputePipelineDescriptor, IndexFormat,
    PipelineCache, RawBufferVec, ShaderStages, ShaderType, SpecializedMeshPipeline,
    TextureSampleType, UniformBuffer, VertexStepMode,
};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery};
use bevy::render::sync_world::MainEntity;
use bevy::render::view::ExtractedView;
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};

use super::ground::{self, GroundCache, INVALID_HEIGHT, TEXELS};
use super::{GrassEnvironment, GrassRenderer, GrassView};
use crate::weather::WeatherParams;

/// Side of one patch of the scatter grid \[m\] — one compute workgroup.
pub(super) const PATCH: f32 = 4.0;
/// Threads in a scatter workgroup; `scatter.wgsl` says the same.
const WORKGROUP: u32 = 64;
/// Segments along a blade per level of detail: eleven, seven and three
/// vertices.
const SEGMENTS: [u32; 3] = [5, 3, 1];
/// Most blades a level may hold in one frame. Sized for the authored density
/// out to the widest range with a wide field of view, plus air; the counters
/// are clamped to it on overflow, so a flying camera loses blades rather than
/// the frame.
const CAPACITY: [u32; 3] = [262_144, 393_216, 524_288];
/// Bytes of one packed blade — `Blade` in the shaders.
const BLADE_BYTES: u64 = 32;
/// Bytes between two levels' indirect arguments.
const INDIRECT_STRIDE: u64 = 32;
/// Blades per square metre at the camera, before the quality scale.
const DENSITY_AT_CAMERA: f32 = 450.0;
/// Distance at which the density has halved \[m\].
const DENSITY_FALLOFF: f32 = 10.0;
/// Where the fine and the middle level hand over \[m\].
const LOD_ENDS: [f32; 2] = [18.0, 60.0];
/// Width of the fade-out at the range \[m\].
const RANGE_FADE: f32 = 30.0;

/// Everything the shaders read besides the instances. Matches `GrassUniform`
/// in `scatter.wgsl` and `blades.wgsl` field for field.
#[derive(ShaderType, Clone, Copy, Default)]
struct GrassUniform {
    frustum: [Vec4; 6],
    camera: Vec4,
    ground: Vec4,
    grid: Vec4,
    density: Vec4,
    lods: Vec4,
    look: Vec4,
    season: Vec4,
    capacity: UVec4,
    weather: WeatherParams,
}

#[derive(ShaderType, Clone, Copy)]
struct LodInfo {
    info: UVec4,
}

#[derive(Resource)]
pub(super) struct GrassBuffers {
    uniform: UniformBuffer<GrassUniform>,
    lods: Vec<UniformBuffer<LodInfo>>,
    instances: Buffer,
    indirect: Buffer,
    indices: RawBufferVec<u32>,
    index_starts: [u32; 3],
    compute_bind_group: Option<BindGroup>,
    draw_bind_groups: Vec<BindGroup>,
    patches_per_side: u32,
    /// Everything for this frame is in place; the draw command checks it.
    ready: bool,
}

#[derive(Resource)]
struct GrassPipelines {
    compute_layout: BindGroupLayoutDescriptor,
    draw_layout: BindGroupLayoutDescriptor,
    scatter: CachedComputePipelineId,
    finish: CachedComputePipelineId,
    blades: Handle<Shader>,
    mesh_pipeline: MeshPipeline,
    /// A vertex layout with no attributes: the blades have no vertex buffer.
    empty_layout: MeshVertexBufferLayoutRef,
    draw: HashMap<MeshPipelineKey, CachedRenderPipelineId>,
}

impl GrassPipelines {
    /// The blade pipeline for a view: Bevy's mesh pipeline for the view's key
    /// with our shaders and bind group in place of the mesh's.
    fn draw_pipeline(
        &mut self,
        cache: &PipelineCache,
        view_key: MeshPipelineKey,
    ) -> Option<CachedRenderPipelineId> {
        let key = view_key
            | MeshPipelineKey::from_primitive_topology_and_strip_index(
                PrimitiveTopology::TriangleList,
                None,
            );
        if let Some(id) = self.draw.get(&key) {
            return Some(*id);
        }
        let mut descriptor = self
            .mesh_pipeline
            .specialize(key, &self.empty_layout)
            .ok()?;
        descriptor.label = Some("grass_blades".into());
        descriptor.vertex.shader = self.blades.clone();
        descriptor.vertex.buffers.clear();
        if let Some(fragment) = &mut descriptor.fragment {
            fragment.shader = self.blades.clone();
        }
        // [view, view binding arrays, mesh] — the mesh group becomes ours.
        descriptor.layout.truncate(2);
        descriptor.layout.push(self.draw_layout.clone());
        // A blade is seen from both sides.
        descriptor.primitive.cull_mode = None;
        let id = cache.queue_render_pipeline(descriptor);
        self.draw.insert(key, id);
        Some(id)
    }
}

pub(super) fn plugin(app: &mut App) {
    ground::plugin(app);
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app
        .init_resource::<GrassEnvironment>()
        .add_render_command::<Opaque3d, DrawGrass>()
        .add_systems(RenderStartup, init.after(MeshPipelineSystems))
        .add_systems(
            Render,
            (
                queue.in_set(RenderSystems::QueueMeshes),
                prepare
                    .in_set(RenderSystems::PrepareResources)
                    .after(ground::prepare),
            ),
        )
        .add_systems(
            Core3d,
            scatter_pass
                .before(Core3dSystems::Prepass)
                .after(ground::pass),
        );
}

fn init(
    mut commands: Commands,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    asset_server: Res<AssetServer>,
    pipeline_cache: Res<PipelineCache>,
    mesh_pipeline: Res<MeshPipeline>,
) {
    let compute_layout = BindGroupLayoutDescriptor::new(
        "grass_scatter",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                uniform_buffer::<GrassUniform>(false),
                texture_2d(TextureSampleType::Float { filterable: false }),
                storage_buffer_sized(false, None),
                storage_buffer_sized(false, None),
            ),
        ),
    );
    let draw_layout = BindGroupLayoutDescriptor::new(
        "grass_blades",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::VERTEX_FRAGMENT,
            (
                uniform_buffer::<GrassUniform>(false),
                storage_buffer_read_only_sized(false, None),
                uniform_buffer::<LodInfo>(false),
            ),
        ),
    );
    let scatter_shader = asset_server.load("embedded://world_render/grass/scatter.wgsl");
    let scatter = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("grass_scatter".into()),
        layout: vec![compute_layout.clone()],
        shader: scatter_shader.clone(),
        entry_point: Some("scatter".into()),
        ..default()
    });
    let finish = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("grass_finish".into()),
        layout: vec![compute_layout.clone()],
        shader: scatter_shader,
        entry_point: Some("finish".into()),
        ..default()
    });

    // One index list for all three levels, back to back. A blade of N
    // segments has 2N + 1 vertices: two per row and the tip.
    let mut indices = RawBufferVec::new(BufferUsages::INDEX);
    let mut index_starts = [0u32; 3];
    for (lod, &segments) in SEGMENTS.iter().enumerate() {
        index_starts[lod] = indices.len() as u32;
        for row in 0..segments {
            let base = 2 * row;
            if row + 1 < segments {
                for index in [base, base + 1, base + 2, base + 1, base + 3, base + 2] {
                    indices.push(index);
                }
            } else {
                for index in [base, base + 1, 2 * segments] {
                    indices.push(index);
                }
            }
        }
    }
    indices.write_buffer(&device, &queue);

    let instances = device.create_buffer(&BufferDescriptor {
        label: Some("grass_instances"),
        size: u64::from(CAPACITY.iter().sum::<u32>()) * BLADE_BYTES,
        usage: BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    let indirect = device.create_buffer(&BufferDescriptor {
        label: Some("grass_indirect"),
        size: 3 * INDIRECT_STRIDE,
        usage: BufferUsages::STORAGE | BufferUsages::INDIRECT | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let lods = (0..3)
        .map(|lod| {
            let mut buffer = UniformBuffer::from(LodInfo {
                info: UVec4::new(SEGMENTS[lod], lod as u32, 0, 0),
            });
            buffer.write_buffer(&device, &queue);
            buffer
        })
        .collect();

    let mut layouts = MeshVertexBufferLayouts::default();
    let empty_layout = layouts.insert(MeshVertexBufferLayout::new(
        Vec::new(),
        VertexBufferLayout {
            array_stride: 0,
            step_mode: VertexStepMode::Vertex,
            attributes: Vec::new(),
        },
    ));

    commands.insert_resource(GrassPipelines {
        compute_layout,
        draw_layout,
        scatter,
        finish,
        blades: asset_server.load("embedded://world_render/grass/blades.wgsl"),
        mesh_pipeline: mesh_pipeline.clone(),
        empty_layout,
        draw: HashMap::default(),
    });
    commands.insert_resource(GrassBuffers {
        uniform: UniformBuffer::from(GrassUniform::default()),
        lods,
        instances,
        indirect,
        indices,
        index_starts,
        compute_bind_group: None,
        draw_bind_groups: Vec::new(),
        patches_per_side: 0,
        ready: false,
    });
}

/// Indirect draw arguments with nothing drawn: the scatter pass counts the
/// instances up from here.
fn reset_indirect(index_starts: &[u32; 3]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(3 * INDIRECT_STRIDE as usize);
    for (lod, &segments) in SEGMENTS.iter().enumerate() {
        let args = [(2 * segments - 1) * 3, 0, index_starts[lod], 0, 0, 0, 0, 0];
        for value in args {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

/// Writes the frame's uniform, clears the counters and, once, builds the
/// bind groups.
#[allow(clippy::too_many_arguments)]
fn prepare(
    mut buffers: ResMut<GrassBuffers>,
    pipelines: Res<GrassPipelines>,
    pipeline_cache: Res<PipelineCache>,
    ground: Res<GroundCache>,
    environment: Res<GrassEnvironment>,
    views: Query<&ExtractedView, With<GrassView>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    buffers.ready = false;
    // Deep winter takes the meadow away: green blades over snow is the one
    // thing worse than none.
    if !environment.settings.enabled || environment.season.snow > 0.5 || !ground.valid {
        return;
    }
    let Some(view) = views.iter().next() else {
        return;
    };
    let compiled = |id| {
        matches!(
            pipeline_cache.get_compute_pipeline_state(id),
            CachedPipelineState::Ok(_)
        )
    };
    if !compiled(pipelines.scatter) || !compiled(pipelines.finish) {
        return;
    }

    let eye = view.world_from_view.translation();
    let clip_from_world = view
        .clip_from_world
        .unwrap_or_else(|| view.clip_from_view * view.world_from_view.to_matrix().inverse());
    let frustum = ViewFrustum::from_clip_from_world(&clip_from_world);
    let mut planes = [Vec4::ZERO; 6];
    for (plane, half_space) in planes.iter_mut().zip(frustum.half_spaces.iter()) {
        *plane = half_space.normal_d();
    }
    // The far plane is the range; an infinite projection has none anyway.
    planes[5] = Vec4::new(0.0, 0.0, 0.0, 1.0);

    let settings = environment.settings;
    let patches_per_side = ((2.0 * settings.range / PATCH).ceil() as u32 + 2).next_multiple_of(2);
    let origin = (eye.xz() / PATCH).floor() - Vec2::splat(patches_per_side as f32 / 2.0);
    let density = DENSITY_AT_CAMERA * settings.density;
    let slots = ((density * PATCH * PATCH).ceil() as u32)
        .next_multiple_of(WORKGROUP)
        .max(WORKGROUP);

    buffers.uniform.set(GrassUniform {
        frustum: planes,
        camera: eye.extend(0.0),
        ground: Vec4::new(
            ground.centre.x,
            ground.centre.z,
            ground.half_extent,
            2.0 * ground.half_extent / TEXELS as f32,
        ),
        grid: Vec4::new(PATCH, patches_per_side as f32, origin.x, origin.y),
        density: Vec4::new(density, DENSITY_FALLOFF, settings.range, slots as f32),
        lods: Vec4::new(LOD_ENDS[0], LOD_ENDS[1], RANGE_FADE, 1.0),
        look: Vec4::new(environment.height, 0.0, 0.0, 0.0),
        season: Vec4::new(
            environment.season.snow,
            environment.season.autumn,
            INVALID_HEIGHT,
            0.0,
        ),
        capacity: UVec4::new(CAPACITY[0], CAPACITY[1], CAPACITY[2], 0),
        weather: environment.weather,
    });
    buffers.uniform.write_buffer(&device, &queue);
    queue.write_buffer(&buffers.indirect, 0, &reset_indirect(&buffers.index_starts));

    if buffers.compute_bind_group.is_none() {
        let Some(uniform) = buffers.uniform.binding() else {
            return;
        };
        let layout = pipeline_cache.get_bind_group_layout(&pipelines.compute_layout);
        let compute = device.create_bind_group(
            "grass_scatter",
            &layout,
            &BindGroupEntries::sequential((
                uniform.clone(),
                &ground.color_view,
                buffers.instances.as_entire_binding(),
                buffers.indirect.as_entire_binding(),
            )),
        );
        let layout = pipeline_cache.get_bind_group_layout(&pipelines.draw_layout);
        let mut offset = 0u64;
        let mut draws = Vec::with_capacity(3);
        for (lod, &capacity) in CAPACITY.iter().enumerate() {
            let size = u64::from(capacity) * BLADE_BYTES;
            let Some(info) = buffers.lods[lod].binding() else {
                return;
            };
            draws.push(device.create_bind_group(
                "grass_blades",
                &layout,
                &BindGroupEntries::sequential((
                    uniform.clone(),
                    BufferBinding {
                        buffer: &buffers.instances,
                        offset,
                        size: Some(size.try_into().expect("a level holds blades")),
                    },
                    info,
                )),
            ));
            offset += size;
        }
        buffers.compute_bind_group = Some(compute);
        buffers.draw_bind_groups = draws;
    }
    buffers.patches_per_side = patches_per_side;
    buffers.ready = true;
}

/// Puts the meadow into the world view's opaque phase.
// A Bevy system takes its world access as parameters; the count says nothing here.
#[allow(clippy::too_many_arguments)]
fn queue(
    draw_functions: Res<DrawFunctions<Opaque3d>>,
    mut pipelines: ResMut<GrassPipelines>,
    pipeline_cache: Res<PipelineCache>,
    mut phases: ResMut<ViewBinnedRenderPhases<Opaque3d>>,
    view_keys: Res<ViewKeyCache>,
    views: Query<&ExtractedView, With<GrassView>>,
    renderer: Query<(Entity, &MainEntity), With<GrassRenderer>>,
    environment: Res<GrassEnvironment>,
) {
    let draw_function = draw_functions.read().id::<DrawGrass>();
    for view in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        for (entity, main_entity) in &renderer {
            // The phase is retained: out with last frame's item, in with this
            // frame's, so a changed view key or a switched-off meadow takes
            // effect at once.
            phase.remove(*main_entity);
            if !environment.settings.enabled {
                continue;
            }
            let Some(&view_key) = view_keys.get(&view.retained_view_entity) else {
                continue;
            };
            let Some(pipeline) = pipelines.draw_pipeline(&pipeline_cache, view_key) else {
                continue;
            };
            phase.add(
                Opaque3dBatchSetKey {
                    draw_function,
                    pipeline,
                    material_bind_group_index: None,
                    lightmap_slab: None,
                    slabs: MeshSlabs::default(),
                },
                Opaque3dBinKey {
                    asset_id: AssetId::<Mesh>::invalid().untyped(),
                },
                (entity, *main_entity),
                InputUniformIndex::default(),
                BinnedRenderPhaseType::NonMesh,
            );
        }
    }
}

/// The compute dispatch: one workgroup per patch, then the clamp.
fn scatter_pass(
    view: ViewQuery<(), With<GrassView>>,
    buffers: Res<GrassBuffers>,
    pipelines: Res<GrassPipelines>,
    pipeline_cache: Res<PipelineCache>,
    mut ctx: RenderContext,
) {
    let _ = view;
    if !buffers.ready {
        return;
    }
    let (Some(scatter), Some(finish), Some(bind_group)) = (
        pipeline_cache.get_compute_pipeline(pipelines.scatter),
        pipeline_cache.get_compute_pipeline(pipelines.finish),
        buffers.compute_bind_group.as_ref(),
    ) else {
        return;
    };
    let mut pass = ctx
        .command_encoder()
        .begin_compute_pass(&ComputePassDescriptor {
            label: Some("grass_scatter"),
            timestamp_writes: None,
        });
    pass.set_bind_group(0, bind_group, &[]);
    pass.set_pipeline(scatter);
    pass.dispatch_workgroups(buffers.patches_per_side, buffers.patches_per_side, 1);
    pass.set_pipeline(finish);
    pass.dispatch_workgroups(1, 1, 1);
}

type DrawGrass = (
    SetItemPipeline,
    SetMeshViewBindGroup<0>,
    SetMeshViewBindingArrayBindGroup<1>,
    DrawBlades,
);

/// Three indirect draws, one per level of detail.
struct DrawBlades;

impl<P: PhaseItem> RenderCommand<P> for DrawBlades {
    type Param = SRes<GrassBuffers>;
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _entity: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        buffers: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let buffers = buffers.into_inner();
        if !buffers.ready || buffers.draw_bind_groups.len() < 3 {
            return RenderCommandResult::Skip;
        }
        let Some(indices) = buffers.indices.buffer() else {
            return RenderCommandResult::Skip;
        };
        pass.set_index_buffer(indices.slice(..), IndexFormat::Uint32);
        for (lod, bind_group) in buffers.draw_bind_groups.iter().enumerate() {
            pass.set_bind_group(2, bind_group, &[]);
            pass.draw_indexed_indirect(&buffers.indirect, lod as u64 * INDIRECT_STRIDE);
        }
        RenderCommandResult::Success
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blade_has_two_vertices_a_row_and_a_tip() {
        // The index list of a level covers 2N + 1 vertices in 2N − 1
        // triangles, and every level's list starts where the last ended.
        let mut count = 0;
        for &segments in &SEGMENTS {
            let triangles = 2 * segments - 1;
            count += triangles * 3;
        }
        let bytes = reset_indirect(&[0, 27, 42]);
        assert_eq!(bytes.len(), 3 * INDIRECT_STRIDE as usize);
        let words: Vec<u32> = bytes
            .chunks(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        assert_eq!(words[0], 27, "the fine level draws nine triangles");
        assert_eq!(words[1], 0, "nothing is drawn before the scatter counts");
        assert_eq!(words[8], 15);
        assert_eq!(
            words[10], 27,
            "the middle level's indices follow the fine one's"
        );
        assert_eq!(words[16], 3);
        assert_eq!(count, 27 + 15 + 3);
    }

    #[test]
    fn the_instance_buffer_holds_every_level() {
        let total: u32 = CAPACITY.iter().sum();
        assert!(u64::from(total) * BLADE_BYTES < 64 << 20, "under 64 MB");
        for capacity in CAPACITY {
            // Each level's binding starts on a 256-byte boundary.
            assert_eq!((u64::from(capacity) * BLADE_BYTES) % 256, 0);
        }
    }
}
