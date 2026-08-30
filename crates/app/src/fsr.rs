//! AMD FidelityFX Super Resolution 3 — temporal upscaling of the cab camera.
//!
//! The world draws at a fraction of the window's resolution and the upscaler
//! reconstructs the full picture from that, from the depth and from the motion
//! vectors of the prepass — the same three inputs Bevy's own DLSS integration
//! feeds on, and the same slot in the pipeline: early in the post-process chain,
//! before bloom and tonemapping run at full size. FSR does its own anti-aliasing,
//! so it is worked together with the `anti_aliasing` setting in `settings.rs`.
//!
//! Bevy ships no FSR of its own; this rides on `wgpu-ffx`, a pure-Rust FSR 3 that
//! compiles to the same wgpu device the renderer already holds. No SDK, no
//! download, every GPU.
//!
//! The one thing Bevy's pipeline does not hand over ready-made is *sized* input:
//! with `MainPassResolutionOverride` the scene lands in the corner of a
//! full-resolution target, and the FSR shaders sample their inputs by UV. A small
//! compute pass (`fsr_crop.wgsl`) copies the corner into textures of exactly the
//! render resolution each frame — colour, depth and motion vectors in one
//! dispatch — and FSR reads those.

use std::borrow::Cow;
use std::ops::Deref;
use std::sync::Mutex;

use bevy::camera::{
    Camera3d, CameraMainTextureUsages, Hdr, MainPassResolutionOverride, Projection,
};
use bevy::core_pipeline::prepass::{DepthPrepass, MotionVectorPrepass, ViewPrepassTextures};
use bevy::core_pipeline::schedule::{Core3d, Core3dSystems};
use bevy::diagnostic::FrameCount;
use bevy::math::{UVec2, Vec2, Vec4Swizzles};
use bevy::prelude::*;
use bevy::render::camera::{MipBias, TemporalJitter};
use bevy::render::render_resource::TextureUsages;
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery};
use bevy::render::sync_world::RenderEntity;
use bevy::render::view::{ExtractedView, ViewTarget, prepare_view_targets};
use bevy::render::{ExtractSchedule, MainWorld, Render, RenderApp, RenderSystems};
use wgpu_ffx::{
    FsrContext, FsrContextFlags, FsrContextInfo, FsrDispatchFlags, FsrDispatchInfo, FsrView,
    get_jitter_offset, get_jitter_phase_count,
};

use crate::settings::Quality;

/// Adds FSR support to the renderer. Nothing here touches the picture unless a
/// camera carries the [`Fsr`] component — `settings::apply_upscaling` puts that on
/// the cab camera when the setting asks for it.
pub struct FsrPlugin;

impl Plugin for FsrPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Fsr>();
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .add_systems(ExtractSchedule, extract_fsr)
            .add_systems(
                Render,
                // Before the view targets are sized, so the resolution override the
                // context carries is honoured by the very frame it appears on.
                prepare_fsr
                    .in_set(RenderSystems::PrepareViews)
                    .before(prepare_view_targets),
            )
            // The same early slot in the post-process chain Bevy's DLSS node sits
            // in: after the main pass, before bloom and tonemapping run at full
            // resolution on the reconstructed picture.
            .add_systems(
                Core3d,
                fsr_super_resolution.in_set(Core3dSystems::EarlyPostProcess),
            );
    }

    fn finish(&self, app: &mut App) {
        // The render device only exists once `RenderPlugin` has finished, which is
        // why this is `finish` and not `build` — `DlssPlugin` does the same.
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        let device = render_app
            .world()
            .resource::<RenderDevice>()
            .wgpu_device()
            .clone();
        let (crop_pipeline, crop_layout) = crop_pass(&device);
        let context = FsrContext::new(FsrContextInfo {
            // The scene arrives linear and before tonemapping; the depth is
            // Bevy's reversed Z; the motion vectors carry this frame's jitter,
            // which the upscaler cancels out again.
            flags: FsrContextFlags::HIGH_DYNAMIC_RANGE
                | FsrContextFlags::DEPTH_INVERTED
                | FsrContextFlags::MOTION_VECTORS_JITTER_CANCELLATION,
            device: device.clone(),
        });
        render_app.insert_resource(FsrSdk {
            device,
            context,
            crop_pipeline,
            crop_layout,
        });
        // Main world: the settings page and `apply_upscaling` read this.
        app.insert_resource(FsrSupported);
    }
}

/// FSR's compiled pipelines and the crop pass, built once against the renderer's
/// own device. Its absence — no render app, as on a dedicated server — is what
/// makes the technique unavailable.
#[derive(Resource)]
pub struct FsrSdk {
    device: wgpu::Device,
    context: FsrContext,
    crop_pipeline: wgpu::ComputePipeline,
    crop_layout: wgpu::BindGroupLayout,
}

/// Set once the renderer is up and FSR's pipelines are compiled. The settings
/// page offers the FSR step only while it exists.
#[derive(Resource, Clone, Copy, Debug)]
pub struct FsrSupported;

/// Puts temporal upscaling on a camera: render at a fraction of the viewport,
/// reconstruct the rest. Requires the depth and motion-vector prepasses, which are
/// what the reconstruction reads — Bevy adds them on its own, and
/// `apply_upscaling` takes them off again when the setting goes back to off.
#[derive(Component, Reflect, Clone, Copy)]
#[reflect(Component)]
#[require(TemporalJitter, MipBias, DepthPrepass, MotionVectorPrepass, Hdr)]
pub struct Fsr {
    /// The fraction of the picture that is actually drawn — [`Quality::Low`]
    /// renders half the edge in each axis, [`Quality::High`] two thirds.
    pub quality: Quality,
}

/// Camera facts the upscaler wants that live outside the render world: the
/// perspective it reconstructs against, and the frame time the accumulation
/// decays by. Extracted alongside [`Fsr`].
#[derive(Component, Clone, Copy)]
pub struct FsrFrame {
    fov_y: f32,
    near: f32,
    far: f32,
    delta_ms: f32,
}

/// The per-camera FSR state on the render side: the upscaler's own accumulation
/// textures, and the exact-size copies of colour, depth and motion vectors that
/// the crop pass fills in each frame.
#[derive(Component)]
pub struct FsrRenderContext {
    view: Mutex<FsrView>,
    quality: Quality,
    render_size: UVec2,
    upscale_size: UVec2,
    color: wgpu::Texture,
    color_view: wgpu::TextureView,
    depth: wgpu::Texture,
    depth_view: wgpu::TextureView,
    motion_vectors: wgpu::Texture,
    motion_vectors_view: wgpu::TextureView,
    dilated_depth: wgpu::Texture,
    dilated_motion_vectors: wgpu::Texture,
    reconstructed_previous_depth: wgpu::Buffer,
}

impl FsrRenderContext {
    fn new(
        sdk: &FsrSdk,
        queue: &RenderQueue,
        render: UVec2,
        upscale: UVec2,
        quality: Quality,
    ) -> Self {
        let texture = |format: wgpu::TextureFormat, label: &'static str| {
            let texture = sdk.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: render.x,
                    height: render.y,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: TextureUsages::STORAGE_BINDING | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            (texture, view)
        };
        let (color, color_view) = texture(wgpu::TextureFormat::Rgba16Float, "fsr_color");
        let (depth, depth_view) = texture(wgpu::TextureFormat::R32Float, "fsr_depth");
        let (motion_vectors, motion_vectors_view) =
            texture(wgpu::TextureFormat::Rg16Float, "fsr_motion_vectors");
        let (dilated_depth, _) = texture(wgpu::TextureFormat::R32Float, "fsr_dilated_depth");
        let (dilated_motion_vectors, _) =
            texture(wgpu::TextureFormat::Rg16Float, "fsr_dilated_motion_vectors");
        // `render_width × render_height × 4` bytes, what the FSR 3 api asks for.
        let reconstructed_previous_depth = sdk.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fsr_reconstructed_previous_depth"),
            size: u64::from(render.x * render.y) * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        FsrRenderContext {
            view: Mutex::new(
                sdk.context
                    .create_view(queue, render.to_array(), upscale.to_array()),
            ),
            quality,
            render_size: render,
            upscale_size: upscale,
            color,
            color_view,
            depth,
            depth_view,
            motion_vectors,
            motion_vectors_view,
            dilated_depth,
            dilated_motion_vectors,
            reconstructed_previous_depth,
        }
    }
}

/// Render resolution for an upscale resolution: the fraction of the picture that
/// is actually drawn. Half the edge in each axis on `Low` — a quarter of the
/// pixels — and a good deal more on the sharper steps.
fn render_size(upscale: UVec2, quality: Quality) -> UVec2 {
    let ratio = match quality {
        Quality::Low => 2.0,
        Quality::Medium => 1.7,
        Quality::High => 1.5,
    };
    UVec2::new(
        (upscale.x as f32 / ratio).round().max(2.0) as u32,
        (upscale.y as f32 / ratio).round().max(2.0) as u32,
    )
}

/// Copies cameras carrying [`Fsr`] into the render world, together with the
/// perspective and frame time the dispatch wants. When the component is gone —
/// the setting turned off, the run ended — their render-side state goes with it.
fn extract_fsr(
    mut commands: Commands,
    mut main_world: ResMut<MainWorld>,
    cleanup: Query<Has<Fsr>>,
) {
    let delta_ms = main_world.resource::<bevy::time::Time>().delta_secs_f64();
    let delta_ms = (delta_ms * 1_000.0) as f32;
    let mut cameras_3d = main_world
        .query_filtered::<(RenderEntity, &Camera, &Projection, Option<&mut Fsr>), With<Hdr>>();
    for (entity, camera, projection, fsr) in cameras_3d.iter_mut(&mut main_world) {
        let driving = camera.is_active && matches!(projection, Projection::Perspective(_));
        if let Some(fsr) = fsr.filter(|_| driving) {
            // Orthographic projections would pass the upscaler a zero field of
            // view; the cab camera is a perspective one, and nothing else asks.
            let Projection::Perspective(perspective) = projection else {
                continue;
            };
            let Ok(mut render_entity) = commands.get_entity(entity) else {
                continue;
            };
            render_entity.insert((
                *fsr,
                FsrFrame {
                    fov_y: perspective.fov,
                    near: perspective.near,
                    far: perspective.far,
                    delta_ms,
                },
            ));
        } else if cleanup.get(entity) == Ok(true)
            && let Ok(mut render_entity) = commands.get_entity(entity)
        {
            render_entity.remove::<(Fsr, FsrFrame, FsrRenderContext, MainPassResolutionOverride)>();
        }
    }
}

/// Sizes the render to the chosen quality, jitters the projection and keeps the
/// render-side state in step with the camera's. Everything the dispatch later
/// needs — and the resolution override that shrinks the main pass — hangs off the
/// context created here.
#[allow(clippy::type_complexity)]
fn prepare_fsr(
    mut query: Query<(
        Entity,
        &ExtractedView,
        &Fsr,
        &mut Camera3d,
        &mut CameraMainTextureUsages,
        &mut TemporalJitter,
        &mut MipBias,
        Option<&mut FsrRenderContext>,
    )>,
    sdk: Option<Res<FsrSdk>>,
    queue: Res<RenderQueue>,
    frame_count: Res<FrameCount>,
    mut commands: Commands,
) {
    let Some(sdk) = sdk else {
        return;
    };
    for (
        entity,
        view,
        fsr,
        mut camera_3d,
        mut main_texture_usages,
        mut jitter,
        mut mip_bias,
        mut context,
    ) in &mut query
    {
        // The upscaled picture is written back by compute, and the prepass depth
        // is read as a plain texture rather than an attachment.
        main_texture_usages.0 |= TextureUsages::STORAGE_BINDING;
        let depth_usages =
            TextureUsages::from(camera_3d.depth_texture_usages) | TextureUsages::TEXTURE_BINDING;
        camera_3d.depth_texture_usages = depth_usages.into();

        let upscale = view.viewport.zw();
        // A viewport smaller than one workgroup has nothing to upscale, and the
        // upscaler refuses to allocate textures that small.
        if upscale.x < 8 || upscale.y < 8 {
            continue;
        }
        let render = render_size(upscale, fsr.quality);

        // Halton jitter, in pixels of the *render* resolution: the same value goes
        // into the projection through `TemporalJitter` and into the dispatch, so
        // the upscaler knows exactly how the frame was shaken.
        let phases = get_jitter_phase_count(
            i32::try_from(render.x).unwrap_or(1),
            i32::try_from(upscale.x).unwrap_or(1),
        );
        jitter.offset = Vec2::from(get_jitter_offset(frame_count.0 as i32, phases));
        // Textures authored for the full resolution are now sampled a step down;
        // the mip chain follows, or every surface reads blurred.
        mip_bias.0 = f32::log2(render.x as f32 / upscale.x as f32);

        let current = context.as_deref_mut();
        if !matches!(current, Some(ctx)
            if ctx.quality == fsr.quality
                && ctx.render_size == render
                && ctx.upscale_size == upscale)
        {
            // A new context is a new history: quality steps and window resizes
            // reset the accumulation, which is the honest thing to do anyway.
            commands.entity(entity).insert((
                FsrRenderContext::new(&sdk, &queue, render, upscale, fsr.quality),
                MainPassResolutionOverride(render),
            ));
        }
    }
}

/// The upscaling itself: the crop pass copies the render-resolution corner of the
/// frame into exact-size textures, and FSR reconstructs the full picture from
/// them into the other half of the main target. Everything after this pass —
/// bloom, tonemapping, the HUD — runs at full resolution on that picture.
fn fsr_super_resolution(
    view: ViewQuery<(
        &Fsr,
        &FsrRenderContext,
        &MainPassResolutionOverride,
        &TemporalJitter,
        &ViewTarget,
        &ViewPrepassTextures,
        &FsrFrame,
    )>,
    sdk: Res<FsrSdk>,
    mut ctx: RenderContext,
) {
    let (_fsr, context, _, jitter, view_target, prepass, frame) = view.into_inner();
    let (Some(depth), Some(motion_vectors)) = (&prepass.depth, &prepass.motion_vectors) else {
        return;
    };
    let view_target = view_target.post_process_write();
    let render = context.render_size;

    let crop = ctx
        .render_device()
        .wgpu_device()
        .create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fsr_crop_bind_group"),
            layout: &sdk.crop_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view_target.source),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&depth.texture.default_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(
                        &motion_vectors.texture.default_view,
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&context.color_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&context.depth_view),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&context.motion_vectors_view),
                },
            ],
        });
    {
        let mut pass = ctx
            .command_encoder()
            .begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("fsr_crop"),
                ..default()
            });
        pass.set_pipeline(&sdk.crop_pipeline);
        pass.set_bind_group(0, &crop, &[]);
        pass.dispatch_workgroups(render.x.div_ceil(8), render.y.div_ceil(8), 1);
    }

    // The api takes the textures by value; they are refcounted handles, so this
    // clones a handful of them a frame and nothing more.
    let dispatch = FsrDispatchInfo {
        color: context.color.clone(),
        depth: context.depth.clone(),
        motion_vectors: context.motion_vectors.clone(),
        exposure: None,
        reactive_mask: None,
        transparency_and_composition: None,
        dilated_depth: context.dilated_depth.clone(),
        dilated_motion_vectors: context.dilated_motion_vectors.clone(),
        reconstructed_previous_depth: context.reconstructed_previous_depth.clone(),
        // `output` is Bevy's wrapped texture; the field wants the wgpu one beneath it.
        output: view_target.destination_texture.deref().clone(),
        jitter_offset: jitter.offset.to_array(),
        // Bevy's motion vectors are normalised; scaling by the negative render
        // size turns them into the pixel units, current- to previous-frame, that
        // the upscaler reads — the same convention DLSS is fed with.
        motion_vector_scale: [-(render.x as f32), -(render.y as f32)],
        render_size: render.to_array(),
        upscale_size: context.upscale_size.to_array(),
        // A gentle RCAS pass over the reconstructed picture; upscaling wants some
        // sharpening to read as crisp as the native picture does.
        enable_sharpening: true,
        sharpness: 0.2,
        // The api refuses deltas below a millisecond — a hitch is a hitch, not a
        // faster frame.
        frame_time_delta: frame.delta_ms.max(1.0),
        pre_exposure: 1.0,
        reset_history: false,
        // Inverted depth: the api wants the plane distances in depth-space, so the
        // far plane (device depth 0) comes first — near and far are swapped.
        camera_near: frame.far,
        camera_far: frame.near,
        camera_fov_y: frame.fov_y,
        // World units are metres.
        view_space_to_meters_factor: 1.0,
        flags: FsrDispatchFlags::empty(),
    };
    // `dispatch` walks the accumulation forward, so the state is behind a lock —
    // the same reason Bevy's DLSS node holds one.
    let mut fsr_view = context.view.lock().expect("poisoned FSR view");
    if let Err(error) = sdk
        .context
        .dispatch(&mut fsr_view, ctx.command_encoder(), &dispatch)
    {
        warn!("FSR: {error}");
    }
}

/// The crop pass' pipeline: one compute dispatch that copies colour, depth and
/// motion vectors out of the corner of the full-size targets into the exact-size
/// textures FSR reads.
fn crop_pass(device: &wgpu::Device) -> (wgpu::ComputePipeline, wgpu::BindGroupLayout) {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("fsr_crop"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("fsr_crop.wgsl"))),
    });
    let texture = |binding, filterable| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    };
    let storage = |binding, format| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::StorageTexture {
            access: wgpu::StorageTextureAccess::WriteOnly,
            format,
            view_dimension: wgpu::TextureViewDimension::D2,
        },
        count: None,
    };
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("fsr_crop_layout"),
        entries: &[
            texture(0, true),
            texture(1, false),
            texture(2, true),
            storage(3, wgpu::TextureFormat::Rgba16Float),
            storage(4, wgpu::TextureFormat::R32Float),
            storage(5, wgpu::TextureFormat::Rg16Float),
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("fsr_crop_pipeline_layout"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("fsr_crop_pipeline"),
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some("crop"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    });
    (pipeline, layout)
}
