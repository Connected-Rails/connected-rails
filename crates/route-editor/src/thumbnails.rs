//! Model previews for the content drawer: every mod model rendered once into a
//! small texture, so the catalogue shows the thing rather than a symbol for its
//! kind.
//!
//! One preview at a time. A model is a glTF that has to load before anything can
//! be framed, so a job runs over several frames anyway, and rendering them one
//! after another keeps it to a single camera and a single render layer instead
//! of one of each per entry. The drawer asks for what it is about to draw, so
//! nothing is rendered for a catalogue nobody opened.
//!
//! The picture is **read back** off the render target into an ordinary image,
//! and that is what egui draws. A render target only holds its contents while
//! an active camera is pointed at it — take the camera away and the texture
//! comes back cleared, which is a preview that vanishes the moment it is done.

use bevy::app::Propagate;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{RenderTarget, primitives::Aabb};
use bevy::image::Image;
use bevy::prelude::*;
use bevy::render::gpu_readback::{Readback, ReadbackComplete};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::world_serialization::WorldAsset;
use bevy_egui::egui;
use std::collections::{HashMap, VecDeque};

/// Edge length of a preview [px]. Twice what the card draws, so the image
/// survives a scaled display without going soft.
const SIZE: u32 = 128;
/// The layer the preview scene lives on. The map is on the default layer 0, and
/// neither camera may see the other's world — the preview would otherwise have
/// the terrain behind it, and the map a mast floating at its origin.
const LAYER: usize = 1;
/// Frames the model is rendered for once it is loaded and framed, before the
/// picture is read back.
///
/// Not a safety margin but the actual wait: the asset being read and its
/// entities existing say nothing about its meshes and textures being on the
/// GPU, and a readback taken before they are returns the clear colour and
/// nothing else. Four frames were not enough for a signal mast.
const SETTLE: u32 = 30;
/// Half the vertical field of view of Bevy's default perspective projection.
const HALF_FOV: f32 = std::f32::consts::FRAC_PI_8;
/// Frames a job waits for the model's meshes to appear before it gives up.
/// `is_loaded_with_dependencies` goes true when the glTF is *read*; the scene
/// entities it describes are spawned a frame or more later, and until they
/// exist there is nothing to frame the camera on.
const GIVE_UP: u32 = 600;
/// Previews kept. A mod library is a few hundred models at a quarter of a
/// megabyte each; past this the oldest are dropped and rendered again if they
/// come back into view.
const KEPT: usize = 128;

/// The previews, and the queue of what still has to be rendered.
#[derive(Resource, Default)]
pub struct Thumbnails {
    /// Model path (`"<mod>/assets/x.gltf"`) to the texture egui draws it from.
    ready: HashMap<String, egui::TextureId>,
    /// In the order they were first shown — what gets dropped first.
    order: VecDeque<String>,
    /// Asked for, not started. A model already queued is not queued twice.
    queued: VecDeque<String>,
    /// Models that produced nothing — a missing file, or a glTF with no mesh
    /// in it. Asking again would only put them back in the queue for ever.
    failed: std::collections::HashSet<String>,
    /// Started and not finished. `active` is empty while the readback runs,
    /// so without this the drawer asks again in that gap and a second scene
    /// goes up on the same render layer as the first.
    running: std::collections::HashSet<String>,
    /// The one preview being rendered.
    active: Option<Job>,
}

/// A preview being rendered.
struct Job {
    model: String,
    scene: Handle<WorldAsset>,
    /// The render target. Only ever a staging area — what egui gets is the
    /// ordinary image the readback writes.
    target: Handle<Image>,
    /// The model and its light.
    scenery: Entity,
    camera: Entity,
    /// Frames since the model's bounds were found; `None` until they are.
    settled: Option<u32>,
    /// Frames the job has run — the give-up counter.
    age: u32,
}

impl Thumbnails {
    /// The preview of `model`, and a request for it if there is none yet.
    ///
    /// Called from the drawer while it draws, so asking is what schedules the
    /// work — a catalogue that is never opened renders nothing.
    pub fn get(&mut self, model: &str) -> Option<egui::TextureId> {
        if let Some(texture) = self.ready.get(model) {
            return Some(*texture);
        }
        if self.failed.contains(model) {
            return None;
        }
        if !self.running.contains(model) && !self.queued.iter().any(|m| m == model) {
            self.queued.push_back(model.to_string());
        }
        None
    }
}

/// Renders the queued previews, one at a time.
pub fn render(
    mut commands: Commands,
    mut thumbnails: ResMut<Thumbnails>,
    mut images: ResMut<Assets<Image>>,
    assets: Res<AssetServer>,
    children: Query<&Children>,
    parts: Query<(&GlobalTransform, &Aabb)>,
    mut cameras: Query<&mut Transform, With<Camera3d>>,
) {
    if let Some(job) = thumbnails.active.take() {
        match advance(job, &mut commands, &assets, &children, &parts, &mut cameras) {
            Ok(job) => {
                thumbnails.active = Some(job);
                return;
            }
            Err(Outcome::Failed(model)) => {
                warn!("no preview for {model}: nothing to render");
                thumbnails.running.remove(&model);
                thumbnails.failed.insert(model);
            }
            // Handed to the readback; its observer puts the picture in `ready`.
            Err(Outcome::Reading) => return,
        }
    }
    let Some(model) = thumbnails.queued.pop_front() else {
        return;
    };
    thumbnails.running.insert(model.clone());
    thumbnails.active = Some(start_job(&model, &mut commands, &mut images, &assets));
}

/// Sets up the scene for one preview: the model, a light and a camera pointed
/// at where the model will be once it has loaded.
fn start_job(
    model: &str,
    commands: &mut Commands,
    images: &mut Assets<Image>,
    assets: &AssetServer,
) -> Job {
    let mut image = Image::new_fill(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0; 4],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    // `COPY_SRC` so the finished picture can be read back off it.
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let handle = images.add(image);

    let layer = RenderLayers::layer(LAYER);
    let scenery = commands
        .spawn((Transform::default(), Visibility::default(), layer.clone()))
        .id();
    let scene: Handle<WorldAsset> =
        assets.load(GltfAssetLabel::Scene(0).from_asset(world_render::asset_path(model)));
    commands.spawn((
        WorldAssetRoot(scene.clone()),
        Transform::default(),
        // Without it the glTF's own children inherit no visibility and are
        // never rasterised — the camera renders, the texture arrives, and the
        // model is simply not in it.
        Visibility::default(),
        // The glTF spawns its own children, and a render layer does not reach
        // them by itself — without this the model is drawn into the map.
        Propagate(layer.clone()),
        ChildOf(scenery),
    ));
    // Lit from over the viewer's shoulder, plus enough ambient that the far
    // side is not a silhouette: a catalogue picture, not a scene.
    commands.spawn((
        DirectionalLight {
            illuminance: 6_000.0,
            ..default()
        },
        Transform::default().looking_to(Vec3::new(-0.5, -0.8, -0.6), Vec3::Y),
        layer.clone(),
        ChildOf(scenery),
    ));
    let camera = commands
        .spawn((
            Camera3d::default(),
            Camera {
                // Before the map's camera, and on nothing but its own texture.
                order: -1,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            AmbientLight {
                brightness: 900.0,
                ..default()
            },
            RenderTarget::Image(handle.clone().into()),
            Transform::from_xyz(0.0, 0.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
            layer,
        ))
        .id();

    Job {
        model: model.to_string(),
        scene,
        target: handle,
        scenery,
        camera,
        settled: None,
        age: 0,
    }
}

/// How a job ended.
enum Outcome {
    /// The picture is rendered and the readback is running; its observer
    /// finishes the job.
    Reading,
    /// Nothing was ever there to draw.
    Failed(String),
}

/// One frame of a running job. `Err` when it is over, either way.
fn advance(
    mut job: Job,
    commands: &mut Commands,
    assets: &AssetServer,
    children: &Query<&Children>,
    parts: &Query<(&GlobalTransform, &Aabb)>,
    cameras: &mut Query<&mut Transform, With<Camera3d>>,
) -> Result<Job, Outcome> {
    job.age += 1;
    let settled = match job.settled {
        Some(settled) => settled,
        None => {
            let failed = assets
                .get_load_state(&job.scene)
                .is_some_and(|state| state.is_failed());
            // Both conditions, because neither implies the other: the glTF
            // being read does not mean its entities exist, and the entities
            // existing does not mean the file is through with its textures.
            let ready = assets.is_loaded_with_dependencies(&job.scene);
            let framed = bounds(job.scenery, children, parts).filter(|_| ready);
            let Some((center, radius)) = framed else {
                if failed || job.age > GIVE_UP {
                    commands.entity(job.scenery).despawn();
                    commands.entity(job.camera).despawn();
                    return Err(Outcome::Failed(job.model));
                }
                return Ok(job);
            };
            if let Ok(mut transform) = cameras.get_mut(job.camera) {
                // The margin covers what the three-quarter view turns towards
                // the camera; the bound is measured axis-aligned.
                let distance = (radius / HALF_FOV.tan()).max(0.1) * 1.3;
                // Three-quarter view from slightly above — the angle a
                // catalogue photograph is taken from.
                let direction = Vec3::new(0.85, 0.5, 1.0).normalize();
                *transform = Transform::from_translation(center + direction * distance)
                    .looking_at(center, Vec3::Y);
            }
            job.settled = Some(0);
            0
        }
    };
    if settled < SETTLE {
        job.settled = Some(settled + 1);
        return Ok(job);
    }
    // Rendered. Read the target back into an ordinary image: the target only
    // holds its contents while an active camera points at it, so a preview
    // left on one vanishes the moment its job is over.
    let model = job.model.clone();
    // The scene and its camera outlive the readback: a render target whose
    // camera has gone comes back cleared, and a readback started in the same
    // frame is not finished yet — the picture would arrive empty, sometimes.
    let (scenery, camera) = (job.scenery, job.camera);
    commands
        .spawn(Readback::texture(job.target.clone()))
        .observe(
            move |readback: On<ReadbackComplete>,
                  mut commands: Commands,
                  mut images: ResMut<Assets<Image>>,
                  mut textures: ResMut<bevy_egui::EguiUserTextures>,
                  mut thumbnails: ResMut<Thumbnails>| {
                commands.entity(readback.entity).despawn();
                commands.entity(scenery).despawn();
                commands.entity(camera).despawn();
                let picture = Image::new(
                    Extent3d {
                        width: SIZE,
                        height: SIZE,
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    readback.data.clone(),
                    TextureFormat::Rgba8UnormSrgb,
                    RenderAssetUsages::default(),
                );
                let handle = images.add(picture);
                let texture = textures.add_image(bevy_egui::EguiTextureHandle::Strong(handle));
                thumbnails.running.remove(&model);
                thumbnails.ready.insert(model.clone(), texture);
                thumbnails.order.push_back(model.clone());
                // Drop the oldest rather than grow without bound; the texture
                // goes with the image handle it was made from.
                while thumbnails.order.len() > KEPT {
                    if let Some(old) = thumbnails.order.pop_front() {
                        thumbnails.ready.remove(&old);
                    }
                }
            },
        );
    Err(Outcome::Reading)
}

/// Centre and radius of everything drawn below `root`, in world space.
fn bounds(
    root: Entity,
    children: &Query<&Children>,
    parts: &Query<(&GlobalTransform, &Aabb)>,
) -> Option<(Vec3, f32)> {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for entity in std::iter::once(root).chain(children.iter_descendants(root)) {
        let Ok((transform, aabb)) = parts.get(entity) else {
            continue;
        };
        // The box is the mesh's own; its corners have to go through the
        // entity's transform before they mean anything here.
        let affine = transform.affine();
        for corner in 0..8u32 {
            let sign = |bit: u32| if corner & (1 << bit) == 0 { -1.0 } else { 1.0 };
            let local = Vec3::from(aabb.center)
                + Vec3::from(aabb.half_extents) * Vec3::new(sign(0), sign(1), sign(2));
            let world = affine.transform_point3(local);
            min = min.min(world);
            max = max.max(world);
        }
    }
    (min.x <= max.x).then(|| {
        // Half the longest edge, not half the diagonal: a signal is three
        // metres tall and half a metre wide, and framing its diagonal leaves
        // the picture mostly empty either side of it.
        let center = (min + max) * 0.5;
        (center, (max - min).max_element() * 0.5)
    })
}
