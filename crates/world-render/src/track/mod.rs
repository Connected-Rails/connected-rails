//! The track as mesh (plan ch. 12): the ballast bed, the sleepers with their
//! fastenings, and the two rails — merged into chunks and culled by distance,
//! because a 50 mm rail clip is a fraction of a pixel at 200 m and the bed's
//! texture carries the look from there.
//!
//! **Every dimension comes from [`track_model::oberbau`]**, and every one of
//! them is the real one: 1435 mm gauge measured 14 mm under the running
//! surface, the rolled 60E1 section with its R 300 crown, rails inclined 1:40
//! by rotation, B 70 sleepers 214 mm deep at the seat and 175 mm between them
//! on the DB standard 60 cm spacing, and a ballast bed whose surface is level
//! with the sleeper tops — not under them, which is what leaves a track
//! looking like a ladder on a plate. Nothing here invents a millimetre.
//!
//! The one thing no geometry gives is what the steel *is*: a rail head is
//! mirror-polished where the wheels ride and rusts everywhere else, and no
//! shadow map resolves the shade a head casts on its own gauge face at that
//! scale. So the section carries a per-point `polish` and a gauge-flank flag
//! in its second uv set, and `rail.wgsl` paints from those rather than
//! guessing from a normal.
//!
//! **Multiplayer.** Nothing here is state — the whole track is a function of
//! the network and the track types, wobble included (see
//! [`mesh::hash01`]).

mod ballast;
mod mesh;
mod rail;
mod sleeper;

use bevy::asset::{Asset, embedded_asset};
use bevy::camera::visibility::VisibilityRange;
use bevy::image::{
    ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor,
};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialPlugin, ParallaxMappingMethod};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;
use glam::DVec3;
use track_model::{Oberbau, SleeperKind, TrackEdge, TrackNetwork, TrackType};
use world_coords::{EnuFrame, RenderOrigin};

use crate::{WorldAnchored, asset_path};
use mesh::to_render;

pub use track_model::GAUGE;

/// Registers the rail material — its shader is an embedded asset, so it has
/// to be announced with the app.
pub(crate) fn plugin(app: &mut App) {
    embedded_asset!(app, "rail.wgsl");
    app.add_plugins(MaterialPlugin::<RailMaterial>::default());
}

/// The rails' material: `rail.wgsl` lays the wheel-polished steel of the
/// running surface over the rust of the section before the PBR lighting runs.
///
/// Its own type, not the dressed [`crate::weather::WeatherMaterial`]: the
/// weather extension would be dropped when [`crate::weather::dress`] swaps
/// plain `StandardMaterial`s, and bare steel is the one surface that does not
/// need the rain look — the glaze is already the wet look.
pub type RailMaterial = ExtendedMaterial<StandardMaterial, RailHead>;

/// The extension proper — no bindings of its own: the shader works off the
/// section's own uv set, the world normal and the camera position.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone, Default)]
pub struct RailHead {}

impl MaterialExtension for RailHead {
    fn fragment_shader() -> ShaderRef {
        "embedded://world_render/track/rail.wgsl".into()
    }
}

/// Sample spacing along the edge for the bed \[m\]. Finer than the old 4 m:
/// the crest carries a slow wobble now, and a wobble sampled every four
/// metres is a zig-zag.
const SAMPLE_BED: f64 = 2.0;
/// The bed is chunked too, so a long edge does not hang one kilometre-sized
/// mesh on its anchor \[m\].
const BED_CHUNK: f64 = 192.0;
/// Detailed sleepers only pay off this close \[m\]; beyond, the bed's texture
/// carries the look — a 60 cm sleeper subtends under an arc-minute there.
const SLEEPER_CULL: f32 = 400.0;
/// Fastenings are 50 mm of steel and go long before the sleepers do \[m\].
const FASTENING_CULL: f32 = 160.0;
/// How deep the parallax on a ballast bed reaches \[m\] — one ballast stone,
/// which is what a fragment has to step down past to read as ballast rather
/// than as a photograph of it. Bevy wants it as a fraction of a texture
/// repeat, so it is divided by the type's own texture scale.
const BALLAST_RELIEF: f64 = 0.045;
/// Layers the depth map is split into. Bevy's default; the relief search adds
/// a binary refinement on top, and more layers on a surface this rough buys
/// nothing a mip level does not take away again.
const PARALLAX_LAYERS: f32 = 16.0;

/// Material set of one track type: the bed (ballast, or the slab's surface),
/// what the sleepers are skinned with, and the steel of the fastenings.
struct TypeMaterials {
    bed: Handle<StandardMaterial>,
    sleeper: Handle<StandardMaterial>,
    fastening: Handle<StandardMaterial>,
}

/// Builds meshes for all edges of the network and spawns them. Each type
/// section of an edge gets its own bed, sleeper and fastening chunks — the
/// type decides texture, sleeper shape and build height — and the rails run
/// over the whole edge in chunks of their own.
pub fn spawn_track(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    rail_materials: &mut Assets<RailMaterial>,
    assets: &AssetServer,
    net: &TrackNetwork,
    origin: &RenderOrigin,
) {
    let types = net.types();
    let per_type: Vec<TypeMaterials> = types
        .iter()
        .map(|ty| type_materials(commands, materials, assets, ty))
        .collect();
    let rail = rail_materials.add(RailMaterial {
        // The shader picks steel or rust per point of the section; these mid
        // values are only what a face gets that the extension leaves alone.
        base: StandardMaterial {
            base_color: Color::srgb(0.30, 0.26, 0.23),
            metallic: 0.6,
            perceptual_roughness: 0.55,
            ..default()
        },
        extension: RailHead {},
    });

    for edge in net.edges() {
        let frame = EnuFrame::at(edge.anchor);
        let (translation, rotation) = origin.frame_transform(&frame);
        let anchor = |offset: Vec3| {
            (
                Transform::from_translation(translation + rotation * offset)
                    .with_rotation(rotation),
                WorldAnchored::offset_in_frame(frame, offset),
            )
        };

        // Bed and sleepers belong to the formation — track on the builder's
        // own constructions (platforms, yards, bridges) has none, only the
        // rails stand; the bed there is the builder's to model.
        if edge.formation {
            for (s0, s1, index) in edge.track_type_runs() {
                let ty = types.get(index as usize);
                let mats = per_type.get(index as usize).unwrap_or(&per_type[0]);
                let oberbau = ty.map_or_else(Oberbau::default, |ty| ty.oberbau.clone());
                let scale = ty.map_or(1.5, texture_scale);

                for (a, b) in spans(s0, s1, BED_CHUNK) {
                    let bed = if oberbau.sleeper == SleeperKind::Slab {
                        ballast::build_slab(edge, &frame, a, b, &oberbau, scale)
                    } else {
                        ballast::build(edge, &frame, a, b, &oberbau, scale)
                    };
                    let offset = Vec3::from(to_render(ballast::mid_section(edge, &frame, a, b)));
                    let bed = recentre(bed, offset);
                    commands.spawn((
                        Mesh3d(meshes.add(bed)),
                        MeshMaterial3d(mats.bed.clone()),
                        anchor(offset),
                    ));
                }

                // Each chunk sits at its own centre, not at the edge anchor:
                // the visibility range measures to the entity translation,
                // and an anchor out of range must not take the sleepers under
                // the camera with it.
                let chunks = sleeper::build(edge, &frame, s0, s1, &oberbau);
                for (mesh, offset) in chunks.sleepers {
                    commands.spawn((
                        Mesh3d(meshes.add(mesh)),
                        MeshMaterial3d(mats.sleeper.clone()),
                        anchor(offset),
                        VisibilityRange::abrupt(0.0, SLEEPER_CULL),
                    ));
                }
                for (mesh, offset) in chunks.fastenings {
                    commands.spawn((
                        Mesh3d(meshes.add(mesh)),
                        MeshMaterial3d(mats.fastening.clone()),
                        anchor(offset),
                        VisibilityRange::abrupt(0.0, FASTENING_CULL),
                    ));
                }
            }
        }

        let (near, far) = rail::build(edge, &frame, rail_profile_of(net, edge));
        for (chunks, range) in [
            (near, VisibilityRange::abrupt(0.0, rail::DETAIL_RANGE)),
            (far, VisibilityRange::abrupt(rail::DETAIL_RANGE, f32::MAX)),
        ] {
            for (mesh, offset) in chunks {
                commands.spawn((
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(rail.clone()),
                    anchor(offset),
                    range.clone(),
                ));
            }
        }
    }
}

/// How many metres one repeat of the bed's texture covers, never zero.
fn texture_scale(ty: &TrackType) -> f64 {
    ty.texture_scale.max(0.05)
}

/// Splits `[s0, s1]` into runs of at most `chunk` metres, all the same length
/// so no run is left a stub.
fn spans(s0: f64, s1: f64, chunk: f64) -> Vec<(f64, f64)> {
    let count = (((s1 - s0) / chunk).ceil() as usize).max(1);
    (0..count)
        .map(|i| {
            (
                s0 + (s1 - s0) * i as f64 / count as f64,
                s0 + (s1 - s0) * (i + 1) as f64 / count as f64,
            )
        })
        .collect()
}

/// Moves a finished mesh into the frame of the point its entity will sit on.
/// [`mesh::MeshBuilder`] does this on the way out; the strip builder the bed
/// uses works in the edge's frame and is recentred afterwards.
fn recentre(mut mesh: Mesh, offset: Vec3) -> Mesh {
    if let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
    {
        for p in positions.iter_mut() {
            *p = [p[0] - offset.x, p[1] - offset.y, p[2] - offset.z];
        }
    }
    mesh
}

/// The edge's rail section — the type at `s = 0` decides, one edge keeps one
/// section (a rail does not change profile mid-edge; type changes are bed
/// work anyway).
fn rail_profile_of(net: &TrackNetwork, edge: &TrackEdge) -> track_model::RailProfile {
    edge.track_type_runs()
        .first()
        .and_then(|&(_, _, index)| net.types().get(index as usize))
        .map_or_else(Default::default, |ty| ty.oberbau.rail)
}

/// The three materials one track type is drawn with.
fn type_materials(
    commands: &mut Commands,
    materials: &mut Assets<StandardMaterial>,
    assets: &AssetServer,
    ty: &TrackType,
) -> TypeMaterials {
    let scale = texture_scale(ty);
    let bed_texture = ty
        .texture
        .as_ref()
        .map(|file| load_tiling(commands, assets, file));
    let mut linear = |file: &Option<String>| {
        file.as_ref()
            .map(|file| load_linear(commands, assets, file))
    };
    let (bed_normal, bed_depth, bed_occlusion) = (
        linear(&ty.normal_map),
        linear(&ty.depth_map),
        linear(&ty.occlusion_map),
    );
    let bed = materials.add(StandardMaterial {
        // A photographed bed is white-tinted; the type color would darken it
        // twice over. The color stays the untextured fallback and the
        // editor's section tint.
        base_color: if bed_texture.is_some() {
            Color::WHITE
        } else {
            Color::srgb(ty.color.0, ty.color.1, ty.color.2)
        },
        base_color_texture: bed_texture,
        normal_map_texture: bed_normal,
        // Ballast is stones, and a photograph of stones on a flat triangle is
        // a photograph. With a height map the fragments step through it and
        // the crib between two sleepers gets depth for the cost of a few
        // texture reads, which is the cheapest relief in the whole scene.
        depth_map: bed_depth,
        occlusion_texture: bed_occlusion,
        parallax_depth_scale: (BALLAST_RELIEF / scale) as f32,
        parallax_mapping_method: ParallaxMappingMethod::Relief { max_steps: 4 },
        max_parallax_layer_count: PARALLAX_LAYERS,
        perceptual_roughness: 0.95,
        ..default()
    });
    TypeMaterials {
        bed,
        sleeper: sleeper_material(commands, materials, assets, &ty.oberbau),
        // Fastenings: oiled steel that has been rained on for years. One
        // material for every type, so the chunks of a whole line batch.
        fastening: materials.add(StandardMaterial {
            base_color: Color::srgb(0.20, 0.19, 0.18),
            metallic: 0.75,
            perceptual_roughness: 0.55,
            ..default()
        }),
    }
}

/// Loads an image as endlessly tiling — clamped edges would smear the last
/// texel over kilometres — and sharp at grazing angles. The sampler goes on
/// the image once it has arrived ([`apply_tiling`]): a per-load
/// `ImageLoaderSettings` sampler breaks the weather material's bind group
/// (wgpu rejects the frame), while the same descriptor on the loaded asset is
/// what terrain, clouds and people already use.
fn load_tiling(commands: &mut Commands, assets: &AssetServer, file: &str) -> Handle<Image> {
    let handle = assets.load(asset_path(file));
    commands.spawn(TilingTexture(handle.clone()));
    handle
}

/// The same, for a map whose texels are **numbers, not colours**: a normal
/// map, a height map, an occlusion map. Those are authored linear, and the
/// image loader assumes sRGB unless told otherwise — decoded as a colour a
/// flat normal `(0.5, 0.5, 1.0)` comes out as `(0.21, 0.21, 1.0)`, which is
/// not "straight up" but a 40° tilt on every single texel. That is a whole
/// surface lit from the wrong direction, and it looks exactly like a texture
/// that is "somehow washed out".
fn load_linear(commands: &mut Commands, assets: &AssetServer, file: &str) -> Handle<Image> {
    let handle = assets
        .load_builder()
        .with_settings(|settings: &mut ImageLoaderSettings| settings.is_srgb = false)
        .load(asset_path(file));
    commands.spawn(TilingTexture(handle.clone()));
    handle
}

/// An image the track needs tiled — the sampler lands on it in [`apply_tiling`].
#[derive(Component)]
pub(crate) struct TilingTexture(Handle<Image>);

/// Marker: [`apply_tiling`] has dressed this image and is done with it.
#[derive(Component)]
pub(crate) struct Tiled;

/// Sets the tiling sampler on every arrived track texture, then stops watching it.
pub(crate) fn apply_tiling(
    mut todo: Query<(Entity, &TilingTexture), Without<Tiled>>,
    mut images: ResMut<Assets<Image>>,
    mut commands: Commands,
) {
    for (entity, tiling) in &mut todo {
        let Some(mut image) = images.get_mut(&tiling.0) else {
            continue;
        };
        if matches!(image.sampler, ImageSampler::Default) {
            image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                address_mode_u: ImageAddressMode::Repeat,
                address_mode_v: ImageAddressMode::Repeat,
                mag_filter: ImageFilterMode::Linear,
                min_filter: ImageFilterMode::Linear,
                mipmap_filter: ImageFilterMode::Linear,
                anisotropy_clamp: 8,
                ..default()
            });
        }
        commands.entity(entity).insert(Tiled);
    }
}

/// Material of the sleepers (or the slab surface): the mod's texture where it
/// names one, otherwise the colour of the material the sleeper is made of.
fn sleeper_material(
    commands: &mut Commands,
    materials: &mut Assets<StandardMaterial>,
    assets: &AssetServer,
    oberbau: &Oberbau,
) -> Handle<StandardMaterial> {
    let texture = oberbau
        .sleeper_texture
        .as_ref()
        .map(|file| load_tiling(commands, assets, file));
    let normal = oberbau
        .sleeper_normal_map
        .as_ref()
        .map(|file| load_linear(commands, assets, file));
    let (tint, roughness) = match oberbau.sleeper {
        // Cast concrete, weathered grey. Lighter than it looks in a
        // photograph of a sleeper: the picture was taken in the shade of the
        // rails and the sun here is not.
        SleeperKind::Concrete => (Color::srgb(0.66, 0.65, 0.62), 0.88),
        // Creosote-soaked hardwood.
        SleeperKind::Wood => (Color::srgb(0.28, 0.22, 0.17), 0.82),
        // In-situ concrete of the Feste Fahrbahn.
        SleeperKind::Slab => (Color::srgb(0.60, 0.60, 0.58), 0.75),
    };
    materials.add(StandardMaterial {
        base_color: if texture.is_some() {
            Color::WHITE
        } else {
            tint
        },
        base_color_texture: texture,
        normal_map_texture: normal,
        perceptual_roughness: roughness,
        ..default()
    })
}

/// Cross section of the track at `s`: centre, right and tangent of travel,
/// and up — all in `frame`, all cant-aware.
fn cross_section(e: &TrackEdge, frame: &EnuFrame, s: f64) -> (DVec3, DVec3, DVec3, DVec3) {
    let pose = e.eval(s);
    let center = frame.to_local(pose.pos);
    let tangent = frame.dir_to_local(pose.tangent);
    let up = frame.dir_to_local(pose.up);
    let right = tangent.cross(up).normalize();
    (center, right, tangent, up)
}

#[cfg(test)]
mod tests;
