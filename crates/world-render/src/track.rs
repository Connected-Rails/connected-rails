//! The track as mesh (plan ch. 12): the ballast bed as a trapezoid strip,
//! the two rails extruded from their real rolled section, and the sleepers —
//! merged into chunk meshes and distance-culled, because a 60 cm sleeper is
//! less than an arc-minute wide at 400 m and the textured bed carries the
//! look from there.
//!
//! All dimensions come from the track type's [`Oberbau`] and are the real
//! ones: 1435 mm gauge measured 14 mm under the rail top, rails inclined
//! 1:40 toward the gauge, sleepers on the DB standard spacing of 60 cm
//! (1667 per km).

use bevy::asset::{Asset, RenderAssetUsages, embedded_asset};
use bevy::camera::visibility::VisibilityRange;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;
use glam::DVec3;
use track_model::{Oberbau, RailProfile, SleeperKind, TrackEdge, TrackNetwork};
use world_coords::{EnuFrame, RenderOrigin};

use crate::{WorldAnchored, asset_path};

/// Registers the rail material — its shader is an embedded asset, so it has
/// to be announced with the app.
pub(crate) fn plugin(app: &mut App) {
    embedded_asset!(app, "rail_head.wgsl");
    app.add_plugins(MaterialPlugin::<RailMaterial>::default());
}

/// The rails' material: `rail_head.wgsl` lays the wheel-polished steel of the
/// running surface over the rust of the section before the PBR lighting runs.
///
/// Its own type, not the dressed [`crate::weather::WeatherMaterial`]: the
/// weather extension would be dropped when [`crate::weather::dress`] swaps
/// plain `StandardMaterial`s, and bare steel is the one surface that does not
/// need the rain look — the glaze is already the wet look.
pub type RailMaterial = ExtendedMaterial<StandardMaterial, RailHead>;

/// The extension proper — no bindings, the shader works off the world normal.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone, Default)]
pub struct RailHead {}

impl MaterialExtension for RailHead {
    fn fragment_shader() -> ShaderRef {
        "embedded://world_render/rail_head.wgsl".into()
    }
}

/// Track gauge [m]: 1435 mm, measured between the inner head faces 14 mm
/// below the rail top.
pub const GAUGE: f64 = 1.435;
/// Depth under the rail top where the gauge is measured [m].
const GAUGE_MEASURE: f64 = 0.014;
/// Sample spacing along the edge [m].
const SAMPLE: f64 = 4.0;
/// Rail pad between rail foot and sleeper top [m] — the Zwischenlage the
/// fastening adds. The sleeper top (and on a slab, its surface) sits this
/// far below the rail foot.
const PAD: f64 = 0.01;
/// Rails stand inclined 1:40 toward the gauge — the head leans in.
const RAIL_CANT: f64 = 1.0 / 40.0;
/// One bed texture repeat spans this many metres, along and across.
const BED_REPEAT: f64 = 4.0;
/// Sleepers merged into one mesh — the culling granularity of the near band.
const SLEEPERS_PER_CHUNK: usize = 128;
/// Detailed sleepers only pay off this close [m]; beyond, the bed's texture
/// carries the look — a 60 cm sleeper subtends under an arc-minute there.
const SLEEPER_CULL: f32 = 400.0;

/// Material pair of one track type: the bed (ballast, or the slab's surface)
/// and what the sleepers are skinned with.
struct TypeMaterials {
    bed: Handle<StandardMaterial>,
    sleeper: Handle<StandardMaterial>,
}

/// Builds meshes for all edges of the network and spawns them. Each type
/// section of an edge gets its own bed and sleeper chunks — the type decides
/// texture, sleeper shape and build height — and the rails run over the whole
/// edge as one extrusion.
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
        .map(|ty| {
            let bed_texture = ty
                .texture
                .as_ref()
                .map(|file| load_tiling(commands, assets, file));
            let bed_normal = ty
                .normal_map
                .as_ref()
                .map(|file| load_tiling(commands, assets, file));
            let bed = materials.add(StandardMaterial {
                // A photographed bed is white-tinted; the type color would
                // darken it twice over. The color stays the untextured
                // fallback and the editor's section tint.
                base_color: if bed_texture.is_some() {
                    Color::WHITE
                } else {
                    Color::srgb(ty.color.0, ty.color.1, ty.color.2)
                },
                base_color_texture: bed_texture,
                normal_map_texture: bed_normal,
                perceptual_roughness: 0.95,
                ..default()
            });
            let sleeper = sleeper_material(commands, materials, assets, &ty.oberbau);
            TypeMaterials { bed, sleeper }
        })
        .collect();
    let rail = rail_materials.add(RailMaterial {
        // The shader picks steel or rust per face normal; these mid values
        // are only what it falls back to on a face that straddles the two.
        base: StandardMaterial {
            base_color: Color::srgb(0.55, 0.50, 0.45),
            metallic: 0.6,
            perceptual_roughness: 0.55,
            ..default()
        },
        extension: RailHead {},
    });

    for edge in net.edges() {
        let frame = EnuFrame::at(edge.anchor);
        let (translation, rotation) = origin.frame_transform(&frame);

        for (s0, s1, index) in edge.track_type_runs() {
            let ty = types.get(index as usize);
            let mats = per_type.get(index as usize).unwrap_or(&per_type[0]);
            let oberbau = ty.map_or_else(Oberbau::default, |ty| ty.oberbau.clone());

            let bed = match oberbau.sleeper {
                SleeperKind::Slab => build_slab(edge, &frame, s0, s1, &oberbau),
                _ => build_ballast(edge, &frame, s0, s1, &oberbau),
            };
            commands.spawn((
                Mesh3d(meshes.add(bed)),
                MeshMaterial3d(mats.bed.clone()),
                Transform::from_translation(translation).with_rotation(rotation),
                WorldAnchored::in_frame(frame),
            ));
            if oberbau.sleeper != SleeperKind::Slab {
                for mesh in build_sleepers(edge, &frame, s0, s1, &oberbau) {
                    commands.spawn((
                        Mesh3d(meshes.add(mesh)),
                        MeshMaterial3d(mats.sleeper.clone()),
                        Transform::from_translation(translation).with_rotation(rotation),
                        WorldAnchored::in_frame(frame),
                        VisibilityRange::abrupt(0.0, SLEEPER_CULL),
                    ));
                }
            }
        }

        commands.spawn((
            Mesh3d(meshes.add(build_rails(edge, &frame, rail_profile_of(net, edge)))),
            MeshMaterial3d(rail.clone()),
            Transform::from_translation(translation).with_rotation(rotation),
            WorldAnchored::in_frame(frame),
        ));
    }
}

/// The edge's rail section — the type at `s = 0` decides, one edge keeps one
/// section (a rail does not change profile mid-edge; type changes are bed
/// work anyway).
fn rail_profile_of(net: &TrackNetwork, edge: &TrackEdge) -> RailProfile {
    edge.track_type_runs()
        .first()
        .and_then(|&(_, _, index)| net.types().get(index as usize))
        .map_or(RailProfile::default(), |ty| ty.oberbau.rail)
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
        .map(|file| load_tiling(commands, assets, file));
    let (tint, roughness) = match oberbau.sleeper {
        // Cast concrete, weathered grey.
        SleeperKind::Concrete => (Color::srgb(0.62, 0.62, 0.60), 0.85),
        // Creosote-soaked hardwood.
        SleeperKind::Wood => (Color::srgb(0.35, 0.28, 0.22), 0.8),
        // In-situ concrete of the Feste Fahrbahn.
        SleeperKind::Slab => (Color::srgb(0.55, 0.55, 0.53), 0.7),
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

/// How far below the rail top the sleeper top (or the slab surface) sits [m].
fn sleeper_top(ob: &Oberbau) -> f64 {
    ob.rail.section().0 + PAD
}

/// The bed's cross-section in one row: bottom-left, top-left, top-right,
/// bottom-right — laterals along `right`, heights under the rail top.
fn bed_section(ob: &Oberbau) -> ([f64; 4], [f64; 4]) {
    let top_y = -sleeper_top(ob) - ob.sleeper_height;
    let top_half = ob.sleeper_length / 2.0 + ob.ballast_overhang;
    // The sides fall 1:1 down to the Planum, but never narrower than the
    // sleeper itself.
    let bottom_half = (top_half - ob.ballast_depth).max(ob.sleeper_length / 2.0);
    (
        [-bottom_half, -top_half, top_half, bottom_half],
        [
            top_y - ob.ballast_depth,
            top_y,
            top_y,
            top_y - ob.ballast_depth,
        ],
    )
}

/// The ballast bed between `s0` and `s1`: a trapezoid strip, top width
/// sleeper + twice the shoulder (RL 853: 4.0 m over a 2.6 m sleeper), sides
/// falling 1:1. The texture repeats every [`BED_REPEAT`] metres so it tiles
/// along the edge without visible seams.
fn build_ballast(e: &TrackEdge, frame: &EnuFrame, s0: f64, s1: f64, ob: &Oberbau) -> Mesh {
    let (laterals, heights) = bed_section(ob);
    let steps = (((s1 - s0) / SAMPLE).ceil() as usize).max(1);
    let mut strip = SectionBuilder::default();
    for i in 0..=steps {
        let s = s0 + (s1 - s0) * i as f64 / steps as f64;
        let (center, right, _, up) = cross_section(e, frame, s);
        strip.push_row(
            laterals
                .iter()
                .zip(heights)
                .map(|(&l, y)| to_render(center + right * l + up * y))
                .collect(),
            laterals
                .iter()
                .map(|&l| [(l / BED_REPEAT) as f32, (s / BED_REPEAT) as f32])
                .collect(),
        );
    }
    strip.build()
}

/// The Feste Fahrbahn between `s0` and `s1`: a slab of the type's width and
/// thickness, its surface just under the rail fastenings, sides straight down
/// into the formation.
fn build_slab(e: &TrackEdge, frame: &EnuFrame, s0: f64, s1: f64, ob: &Oberbau) -> Mesh {
    let top_y = -sleeper_top(ob);
    let bottom_y = top_y - ob.sleeper_height;
    let half = ob.sleeper_length / 2.0;
    let laterals = [-half, -half, half, half];

    let steps = (((s1 - s0) / SAMPLE).ceil() as usize).max(1);
    let mut strip = SectionBuilder::default();
    for i in 0..=steps {
        let s = s0 + (s1 - s0) * i as f64 / steps as f64;
        let (center, right, _, up) = cross_section(e, frame, s);
        let heights = [bottom_y, top_y, top_y, bottom_y];
        strip.push_row(
            laterals
                .iter()
                .zip(heights)
                .map(|(&l, y)| to_render(center + right * l + up * y))
                .collect(),
            laterals
                .iter()
                .map(|&l| [(l / BED_REPEAT) as f32, (s / BED_REPEAT) as f32])
                .collect(),
        );
    }
    strip.build()
}

/// The closed section of one rail: points `(across, down)` in metres, top of
/// the head at the origin. An approximation of the rolled profile with the
/// right envelope — head width, web, foot flare — built from the real
/// dimensions of [`RailProfile`] (EN 13674).
fn rail_section(profile: RailProfile) -> Vec<(f64, f64)> {
    let (h, head, foot) = profile.section();
    // Web thickness scales with the section; 60E1 rolls 16.5 mm.
    let web = foot * 0.11;
    // Depth where the head side ends and the web starts.
    let head_h = match profile {
        RailProfile::R60 => 0.049,
        _ => 0.044,
    };
    let web_y1 = h - foot * 0.28;
    let foot_top = h - 0.008;
    let half = [
        (0.0, 0.0),
        (head / 2.0 - 0.006, 0.0),
        (head / 2.0, 0.008),
        (head / 2.0 - 0.002, head_h),
        (web, head_h + 0.014),
        (web, web_y1 - 0.012),
        (web + 0.010, web_y1),
        (foot / 2.0 - 0.014, foot_top - 0.012),
        (foot / 2.0, foot_top),
        (foot / 2.0, h),
    ];
    let mut pts = half.to_vec();
    for &(x, y) in half.iter().skip(1).rev() {
        pts.push((-x, y));
    }
    pts
}

/// Lateral distance of the rail axis from the track centre [m]: the gauge is
/// measured between the inner head faces, so the axis sits half a head width
/// beyond the half gauge.
fn rail_axis(profile: RailProfile) -> f64 {
    GAUGE / 2.0 + profile.section().1 / 2.0
}

/// Both rails over the whole edge, extruded from the real section, inclined
/// 1:40 toward the gauge, with capped ends so a buffer stop does not look
/// into a hollow rail.
fn build_rails(e: &TrackEdge, frame: &EnuFrame, profile: RailProfile) -> Mesh {
    let section = rail_section(profile);
    let points = section.len();
    let ring = points * 2; // both rails in one row
    let axis = rail_axis(profile);

    let steps = ((e.length() / SAMPLE).ceil() as usize).max(1);
    let mut positions = Vec::with_capacity((steps + 1) * ring + 4);
    let mut uvs = Vec::with_capacity(positions.capacity());
    for i in 0..=steps {
        let s = e.length() * i as f64 / steps as f64;
        let (center, right, _, up) = cross_section(e, frame, s);
        for side in [-1.0, 1.0] {
            for &(x, y) in &section {
                // 1:40 cant: the section shears about the gauge measuring
                // point, the foot of the outer side slides out.
                let x = x + side * (y - GAUGE_MEASURE) * RAIL_CANT;
                let pos = center + right * (side * axis + x) + up * (-y);
                positions.push(to_render(pos));
                uvs.push([(side * axis + x) as f32, (s * 2.5) as f32]);
            }
        }
    }

    let mut indices = Vec::with_capacity(steps * ring * 6 + ring * 6);
    // The rails' skins — per rail: the ring closes within its own `points`,
    // wrapping over the whole row would sew the two rails together with a
    // surface across the gauge.
    for i in 0..steps {
        for rail in 0..2 {
            for j in 0..points {
                let j_next = (j + 1) % points;
                let a = (i * ring + rail * points + j) as u32;
                let b = (i * ring + rail * points + j_next) as u32;
                let c = ((i + 1) * ring + rail * points + j) as u32;
                let d = ((i + 1) * ring + rail * points + j_next) as u32;
                indices.extend_from_slice(&[a, b, c, b, d, c]);
            }
        }
    }
    // End caps: fans around each ring's centre — the first row's caps face
    // against the direction of travel, the last row's with it.
    let mut cap_centres = Vec::with_capacity(4);
    for (row, side) in [(0usize, 0usize), (0, 1), (steps, 0), (steps, 1)] {
        let mut c = [0.0f32; 3];
        for j in 0..points {
            let p = positions[row * ring + side * points + j];
            c = [c[0] + p[0], c[1] + p[1], c[2] + p[2]];
        }
        cap_centres.push([
            c[0] / points as f32,
            c[1] / points as f32,
            c[2] / points as f32,
        ]);
    }
    let first_cap = positions.len() as u32;
    for centre in &cap_centres {
        positions.push(*centre);
        uvs.push([0.0, 0.0]);
    }
    for (k, (row, side)) in [(0, 0), (0, 1), (steps, 0), (steps, 1)].iter().enumerate() {
        let centre = first_cap + k as u32;
        for j in 0..points {
            let j_next = (j + 1) % points;
            let a = (row * ring + side * points + j) as u32;
            let b = (row * ring + side * points + j_next) as u32;
            if *row == 0 {
                indices.extend_from_slice(&[centre, b, a]);
            } else {
                indices.extend_from_slice(&[centre, a, b]);
            }
        }
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh.compute_normals();
    mesh
}

/// The sleepers of a type run, merged into chunk meshes of
/// [`SLEEPERS_PER_CHUNK`] — one entity per chunk keeps the spawn cheap and
/// gives the distance cull something to work with.
fn build_sleepers(e: &TrackEdge, frame: &EnuFrame, s0: f64, s1: f64, ob: &Oberbau) -> Vec<Mesh> {
    let spacing = ob.sleeper_spacing.max(0.01);
    let count = (((s1 - s0) / spacing).floor() as usize) + 1;
    let mut chunks: Vec<Mesh> = Vec::new();
    let mut builder = SleeperBuilder::default();
    for k in 0..count {
        let s = s0 + k as f64 * spacing;
        if s > s1 {
            break;
        }
        builder.push(cross_section(e, frame, s), ob);
        if builder.sleepers() == SLEEPERS_PER_CHUNK {
            chunks.push(builder.build());
            builder = SleeperBuilder::default();
        }
    }
    if builder.sleepers() > 0 {
        chunks.push(builder.build());
    }
    chunks
}

/// One quad: four ring vertices in wind order, four texture coordinates. The
/// normal is computed from the ring, so mesh and shading cannot disagree.
struct Face {
    ring: [DVec3; 4],
    uv: [[f32; 2]; 4],
}

impl Face {
    fn normal(&self) -> [f32; 3] {
        let n = (self.ring[1] - self.ring[0])
            .cross(self.ring[2] - self.ring[1])
            .normalize();
        to_render(n)
    }
}

/// Merges sleeper prisms — one section (width along the track × height),
/// extruded along the sleeper's own length (left–right of the track) — into
/// one flat-shaded mesh. The texture maps one repeat onto the sleeper top,
/// so a wood plank set reads one plank per sleeper.
#[derive(Default)]
struct SleeperBuilder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
    count: usize,
}

impl SleeperBuilder {
    fn sleepers(&self) -> usize {
        self.count
    }

    /// Adds one sleeper at the cross section `(center, right, tangent, up)`
    /// of `s`.
    fn push(&mut self, (center, right, tangent, up): (DVec3, DVec3, DVec3, DVec3), ob: &Oberbau) {
        let h = ob.sleeper_height;
        let w = ob.sleeper_width;
        let half_len = ob.sleeper_length / 2.0;
        // Concrete sleepers taper; a timber sleeper is a beam.
        let top_w = match ob.sleeper {
            SleeperKind::Concrete => w - 0.04,
            _ => w,
        };
        // The section, width along the track, height below the sleeper top:
        // top-left, top-right, bottom-right, bottom-left.
        let (tl, tr, br, bl) = (
            (-top_w / 2.0, 0.0),
            (top_w / 2.0, 0.0),
            (w / 2.0, h),
            (-w / 2.0, h),
        );
        // The sleeper sits on the bed: its top `sleeper_top` under the rail
        // top, its base on the bed the section carves.
        let drop = sleeper_top(ob);
        // `t` = across the sleeper (along the track), `end` = along the
        // sleeper (left–right of the track), `y` = down from the sleeper top.
        let place =
            |t: f64, y: f64, end: f64| center + tangent * t + right * end + up * (-(drop + y));
        let (l, r) = (-half_len, half_len);

        // Texture bands: the top shows one repeat (u along the sleeper, v
        // across); flanks and bottom run u along the sleeper's 2.6 m and v
        // down its height, so the grain follows the timber instead of
        // smearing across it; ends sample a square patch of the same band.
        let top_uv = [[0.0, 0.1], [1.0, 0.1], [1.0, 0.3], [0.0, 0.3]];
        let side_uv = [[0.0, 0.4], [0.0, 0.5], [1.0, 0.5], [1.0, 0.4]];
        let end_uv = [[0.4, 0.4], [0.4, 0.5], [0.5, 0.5], [0.5, 0.4]];

        let faces = [
            // Top, wound to face the sky.
            Face {
                ring: [
                    place(tl.0, tl.1, l),
                    place(tl.0, tl.1, r),
                    place(tr.0, tr.1, r),
                    place(tr.0, tr.1, l),
                ],
                uv: top_uv,
            },
            // Bottom.
            Face {
                ring: [
                    place(bl.0, bl.1, l),
                    place(br.0, br.1, l),
                    place(br.0, br.1, r),
                    place(bl.0, bl.1, r),
                ],
                uv: side_uv,
            },
            // Both flanks, the short sides along the sleeper's length.
            Face {
                ring: [
                    place(tl.0, tl.1, l),
                    place(bl.0, bl.1, l),
                    place(bl.0, bl.1, r),
                    place(tl.0, tl.1, r),
                ],
                uv: side_uv,
            },
            Face {
                ring: [
                    place(tr.0, tr.1, r),
                    place(br.0, br.1, r),
                    place(br.0, br.1, l),
                    place(tr.0, tr.1, l),
                ],
                uv: side_uv,
            },
            // Both ends, the sleeper's own section.
            Face {
                ring: [
                    place(tr.0, tr.1, l),
                    place(br.0, br.1, l),
                    place(bl.0, bl.1, l),
                    place(tl.0, tl.1, l),
                ],
                uv: end_uv,
            },
            Face {
                ring: [
                    place(tl.0, tl.1, r),
                    place(bl.0, bl.1, r),
                    place(br.0, br.1, r),
                    place(tr.0, tr.1, r),
                ],
                uv: end_uv,
            },
        ];
        for face in &faces {
            let base = self.positions.len() as u32;
            let normal = face.normal();
            for (pos, uv) in face.ring.into_iter().zip(face.uv) {
                self.positions.push(to_render(pos));
                self.uvs.push(uv);
                self.normals.push(normal);
            }
            // Both triangles of the quad along the 0–2 diagonal — a pattern
            // over the 1–3 diagonal winds one of them back into the face.
            self.indices
                .extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 3, base]);
        }
        self.count += 1;
    }

    fn build(self) -> Mesh {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs);
        mesh.insert_indices(Indices::U32(self.indices));
        // The normal map needs tangents; the flat normals stay as pushed. The
        // call only fails on a mesh without normals or uvs — both are above.
        mesh.generate_tangents()
            .expect("sleeper mesh has normals and uvs");
        mesh
    }
}

/// One cross-section row: columns left to right, each with its texture
/// coordinates.
type Row = (Vec<[f32; 3]>, Vec<[f32; 2]>);

/// A strip meshed from cross-section rows: each row is the same number of
/// columns, and consecutive columns — bridged left to right — become quads
/// facing upwards, where the camera is (pinned by a test).
#[derive(Default)]
struct SectionBuilder {
    rows: Vec<Row>,
}

impl SectionBuilder {
    fn push_row(&mut self, positions: Vec<[f32; 3]>, uvs: Vec<[f32; 2]>) {
        self.rows.push((positions, uvs));
    }

    fn build(self) -> Mesh {
        let stride = self.rows.first().map_or(0, |(p, _)| p.len()).max(1);
        let mut positions = Vec::with_capacity(self.rows.len() * stride);
        let mut uvs = Vec::with_capacity(positions.capacity());
        let mut indices = Vec::new();
        for (row, (row_positions, row_uvs)) in self.rows.iter().enumerate() {
            for (pos, uv) in row_positions.iter().zip(row_uvs) {
                positions.push(*pos);
                uvs.push(*uv);
            }
            if row + 1 < self.rows.len() {
                for col in 0..stride.saturating_sub(1) {
                    let a = (row * stride + col) as u32;
                    let b = (row * stride + col + 1) as u32;
                    let c = ((row + 1) * stride + col) as u32;
                    let d = ((row + 1) * stride + col + 1) as u32;
                    // Left, right, then along the track: that order faces
                    // upwards — the other way round the strip is a backface
                    // and the bed is not drawn at all.
                    indices.extend_from_slice(&[a, b, c, b, d, c]);
                }
            }
        }
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
        mesh.insert_indices(Indices::U32(indices));
        mesh.compute_normals();
        mesh
    }
}

/// ENU (x = east, y = north, z = up) → render axes (x = east, y = up, z = −north).
fn to_render(p: DVec3) -> [f32; 3] {
    [p.x as f32, p.z as f32, -p.y as f32]
}

#[cfg(test)]
mod tests {
    use super::*;
    use track_model::{EdgeId, NodeId, Segment};
    use world_coords::geo::to_ecef_deg;

    /// A 100 m straight edge heading east at constant height.
    fn straight_edge() -> TrackEdge {
        TrackEdge::new(
            EdgeId(0),
            NodeId(0),
            NodeId(1),
            to_ecef_deg(52.0, 10.0, 100.0),
            90.0f64.to_radians(),
            vec![Segment {
                len: 100.0,
                k0: 0.0,
                dk: 0.0,
            }],
        )
    }

    fn positions_of(mesh: &Mesh) -> Vec<[f32; 3]> {
        match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) => positions.to_vec(),
            _ => panic!("positions"),
        }
    }

    /// The bed is a trapezoid on the RL 853 cross-section: 4.0 m over the
    /// sleeper underside (2.6 m sleeper + twice the 0.7 m shoulder), the
    /// whole build — rail, pad, sleeper, ballast — under the rail top, and
    /// the sides falling 1:1 down to the Planum.
    #[test]
    fn the_ballast_bed_is_a_real_trapezoid() {
        let edge = straight_edge();
        let frame = EnuFrame::at(edge.anchor);
        let ob = Oberbau::default();
        let positions = positions_of(&build_ballast(&edge, &frame, 0.0, 100.0, &ob));

        // The edge heads north: lateral is render x, along-track render z.
        let top_width = positions.iter().map(|p| p[0]).fold(f32::MIN, f32::max)
            - positions.iter().map(|p| p[0]).fold(f32::MAX, f32::min);
        assert!((top_width - 4.0).abs() < 1e-4, "bed width {top_width}");

        // Row 0: bottom-left, top-left, top-right, bottom-right.
        let (rail_h, _, _) = RailProfile::R60.section();
        let want_top = -(rail_h + PAD + ob.sleeper_height) as f32;
        assert!(
            (positions[1][1] - want_top).abs() < 1e-4,
            "bed top {} vs {want_top}",
            positions[1][1]
        );
        assert!(
            (positions[0][1] - (want_top - ob.ballast_depth as f32)).abs() < 1e-4,
            "bed bottom {}",
            positions[0][1]
        );
        // The foot is 0.3 m narrower each side than the top (1:1 slope).
        let bottom_half = positions[0][0].abs();
        assert!(
            (bottom_half - (2.0 - ob.ballast_depth) as f32).abs() < 1e-4,
            "bottom half {bottom_half}"
        );
    }

    /// The rails stand 1435 mm apart between the inner head faces, extruded
    /// from the real 60E1 envelope, and their running surface faces the sky.
    #[test]
    fn the_rails_hold_the_gauge_and_face_up() {
        let edge = straight_edge();
        let frame = EnuFrame::at(edge.anchor);
        let positions = positions_of(&build_rails(&edge, &frame, RailProfile::R60));
        let (_, head, foot) = RailProfile::R60.section();

        // The edge heads north: lateral is render x, along-track render z.
        // Outer extremes of the two rail feet: 2 × (gauge/2 + head/2) + foot,
        // pushed out by the 1:40 cant at foot depth.
        let (rail_h, _, _) = RailProfile::R60.section();
        let cant_spread = (rail_h - GAUGE_MEASURE) * RAIL_CANT;
        let span = positions.iter().map(|p| p[0]).fold(f32::MIN, f32::max)
            - positions.iter().map(|p| p[0]).fold(f32::MAX, f32::min);
        let want = (GAUGE + head + foot + 2.0 * cant_spread) as f32;
        assert!((span - want).abs() < 1e-3, "rail span {span} vs {want}");

        // The inner head faces sit exactly one gauge apart, 14 mm below the
        // rail top: the head-side vertex nearest the measuring depth.
        for sign in [-1.0f32, 1.0] {
            let inner = (GAUGE / 2.0) as f32;
            let best = positions
                .iter()
                .filter(|p| (p[1] + GAUGE_MEASURE as f32).abs() < 0.008)
                .filter(|p| sign * p[0] > inner - head as f32 && sign * p[0] < inner + 0.01)
                .map(|p| (sign * p[0] - inner).abs())
                .fold(f32::MAX, f32::min);
            assert!(
                best < 0.002,
                "inner head face {sign}: {} mm off the gauge",
                best * 1000.0
            );
        }

        // The running surface faces up.
        let rail_mesh = build_rails(&edge, &frame, RailProfile::R60);
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(normals)) =
            rail_mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
        else {
            panic!("normals");
        };
        assert!(
            normals.iter().any(|n| n[1] > 0.9),
            "no face of the rail points at the sky"
        );
    }

    /// Sleepers go in at the type's spacing — 60 cm on the Regeloberbau —
    /// and top out just under the rail pad, sitting on the ballast bed.
    #[test]
    fn sleepers_keep_the_db_spacing() {
        let edge = straight_edge();
        let frame = EnuFrame::at(edge.anchor);
        let ob = Oberbau::default();
        let chunks = build_sleepers(&edge, &frame, 0.0, 10.0, &ob);
        // 10 m / 0.6 m = 16 full gaps, plus the first sleeper = 17.
        let count: usize = chunks.iter().map(|m| positions_of(m).len() / 24).sum();
        assert_eq!(count, 17, "sleepers in 10 m at 60 cm");

        let positions = positions_of(&chunks[0]);
        let (rail_h, _, _) = RailProfile::R60.section();
        let want_top = -(rail_h + PAD);
        let top = positions.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        assert!(
            (top - want_top as f32).abs() < 1e-3,
            "sleeper top {top} vs {want_top}"
        );
        // The base sits on the bed: sleeper top plus the sleeper height.
        let bottom = positions.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
        assert!(
            (bottom - (want_top - ob.sleeper_height) as f32).abs() < 1e-3,
            "sleeper base {bottom}"
        );
        // The second sleeper starts one spacing further down the track
        // (render z, north is negative).
        assert!(
            ((positions[24][2] - positions[0][2]).abs() - ob.sleeper_spacing as f32).abs() < 1e-3,
            "sleeper spacing along the edge"
        );
    }

    /// The bed's strip is wound to face upwards — the other way round it is
    /// a backface and the track is simply not there.
    #[test]
    fn the_track_bed_faces_upwards() {
        let mut strip = SectionBuilder::default();
        for i in 0..3 {
            strip.push_row(
                vec![
                    [i as f32 * 4.0, 0.0, -2.6],
                    [i as f32 * 4.0, 0.0, -2.0],
                    [i as f32 * 4.0, 0.0, 2.0],
                    [i as f32 * 4.0, 0.0, 2.6],
                ],
                vec![[0.0; 2]; 4],
            );
        }
        let mesh = strip.build();
        let Some(bevy::mesh::Indices::U32(indices)) = mesh.indices() else {
            panic!("indices");
        };
        let positions = positions_of(&mesh);
        for triangle in indices.chunks(3) {
            let p = |i: u32| Vec3::from(positions[i as usize]);
            let (a, b, c) = (p(triangle[0]), p(triangle[1]), p(triangle[2]));
            let normal = (b - a).cross(c - a);
            assert!(normal.y > 0.0, "triangle faces down: {normal:?}");
        }
    }

    /// Every sleeper face points away from the sleeper, and every triangle is
    /// wound with its face — one triangle of a quad wound backwards is culled
    /// and the sleeper shows its inside.
    #[test]
    fn sleeper_faces_point_outwards() {
        let ob = Oberbau::default();
        // One sleeper at s = 0, the section of the straight edge: centre at
        // the origin, right = south, up = up (render axes).
        let mut builder = SleeperBuilder::default();
        let up = DVec3::Z;
        let tangent = DVec3::X;
        let right = tangent.cross(up);
        builder.push((DVec3::ZERO, right, tangent, up), &ob);
        let mesh = builder.build();
        let positions = positions_of(&mesh);
        let normals = match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
            Some(bevy::mesh::VertexAttributeValues::Float32x3(normals)) => normals.to_vec(),
            _ => panic!("normals"),
        };
        let Some(bevy::mesh::Indices::U32(indices)) = mesh.indices() else {
            panic!("indices");
        };

        // The sleeper sits just under the rail pad, and its length lies on
        // render z (this hand-built section points right = south).
        let (rail_h, _, _) = RailProfile::R60.section();
        let want_top = -(rail_h + PAD);
        let top = positions.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        assert!((top - want_top as f32).abs() < 1e-4);
        let half_len = ob.sleeper_length / 2.0;
        let lateral = positions.iter().map(|p| p[2]).fold(f32::MIN, f32::max);
        assert!(
            (lateral - half_len as f32).abs() < 1e-4,
            "sleeper reaches only {lateral} of {} m",
            half_len
        );

        // Every triangle wound with its pushed normal — one triangle of a
        // quad against it and the face shows its inside from that half.
        for triangle in indices.chunks(3) {
            let p = |i: u32| Vec3::from(positions[i as usize]);
            let (a, b, c) = (p(triangle[0]), p(triangle[1]), p(triangle[2]));
            let winding = (b - a).cross(c - a);
            let n = Vec3::from(normals[triangle[0] as usize]);
            assert!(
                winding.dot(n) > 0.0,
                "triangle wound against its normal: {winding:?} vs {n:?}"
            );
        }

        // Every face normal points away from the sleeper's middle.
        let middle = positions
            .iter()
            .fold(Vec3::ZERO, |acc, p| acc + Vec3::from(*p))
            / positions.len() as f32;
        for face in indices.chunks(6) {
            let centre = face.iter().fold(Vec3::ZERO, |acc, i| {
                acc + Vec3::from(positions[*i as usize])
            }) / face.len() as f32;
            let n = Vec3::from(normals[face[0] as usize]);
            assert!(
                (centre - middle).dot(n) > 0.0,
                "face normal points inward: {n:?} at {centre:?}"
            );
        }
    }

    /// The real rail sections the profiles are extruded from.
    #[test]
    fn rail_sections_match_the_rolled_profiles() {
        for (profile, height) in [
            (RailProfile::R49, 0.149),
            (RailProfile::R54, 0.154),
            (RailProfile::R60, 0.172),
        ] {
            let section = rail_section(profile);
            assert_eq!(section[0], (0.0, 0.0), "starts at the rail top");
            assert!(
                section.iter().all(|&(_, y)| y <= height + 1e-9),
                "section higher than the rail"
            );
            let foot = section.iter().map(|&(x, _)| x).fold(0.0f64, f64::max);
            assert!(
                (foot - profile.section().2 / 2.0).abs() < 1e-9,
                "foot half width {foot}"
            );
            // The section is a closed ring: it ends with the mirror of where
            // it left the rail top.
            let second = section[1];
            let last = *section.last().expect("non-empty");
            assert!(
                (last.0 + second.0).abs() < 1e-9 && (last.1 - second.1).abs() < 1e-9,
                "section not closed: {last:?} vs mirror of {second:?}"
            );
        }
    }
}
