//! Per-placement signal furniture: designation plates and German subsidiary
//! signals.  These are assembled procedurally so a route can vary the name and
//! fittings without multiplying the shared mast/arm glTF models.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use content::route::{SignalAddons, Zs1Display, Zs8Display, ZsConstruction, ZsNumber};
use sim_core::interlock::SignalKind;

use crate::{SignalLamp, SignalView};

const DINISH_CONDENSED_BOLD: &[u8] = include_bytes!("../fonts/DINishCondensed-Bold.ttf");
const FRONT_GAP: f32 = 0.004;
const ZS103_WIDTH: f32 = 0.22;
const ZS103_HEIGHT: f32 = 1.05;
const ZS103_DIAMOND: f32 = 0.105;
const ZS103_FIRST_Y: f32 = 0.43;
const ZS103_STEP_Y: f32 = 0.172;
const ZS103_DIAMONDS: usize = 6;

pub(crate) struct SignalFittingMaterials {
    dark: Handle<StandardMaterial>,
    white: Handle<StandardMaterial>,
    steel: Handle<StandardMaterial>,
    lens_off: Handle<StandardMaterial>,
    lamp_white: Handle<StandardMaterial>,
    lamp_yellow: Handle<StandardMaterial>,
}

impl SignalFittingMaterials {
    pub(crate) fn new(materials: &mut Assets<StandardMaterial>) -> Self {
        let solid = |materials: &mut Assets<StandardMaterial>, colour: Color, roughness| {
            materials.add(StandardMaterial {
                base_color: colour,
                perceptual_roughness: roughness,
                ..default()
            })
        };
        let lamp = |materials: &mut Assets<StandardMaterial>, colour: Color| {
            materials.add(StandardMaterial {
                base_color: colour,
                emissive: colour.to_linear() * 7.0,
                perceptual_roughness: 0.22,
                ..default()
            })
        };
        Self {
            dark: solid(materials, Color::srgb(0.045, 0.05, 0.047), 0.72),
            white: solid(materials, Color::srgb(0.86, 0.86, 0.82), 0.48),
            steel: solid(materials, Color::srgb(0.30, 0.32, 0.30), 0.62),
            lens_off: solid(materials, Color::srgb(0.075, 0.080, 0.073), 0.30),
            lamp_white: lamp(materials, Color::srgb(1.0, 0.96, 0.78)),
            lamp_yellow: lamp(materials, Color::srgb(1.0, 0.66, 0.08)),
        }
    }
}

#[derive(Clone, Copy)]
struct Layout {
    front_z: f32,
    designation_y: f32,
    equipment_top: f32,
    semaphore: bool,
}

impl Layout {
    fn of(signal: &SignalView<'_>) -> Self {
        let tags = signal.model.map_or(&[][..], |model| model.tags.as_slice());
        let semaphore = tags.iter().any(|tag| tag == "semaphore");
        let nominal_height = tags
            .iter()
            .find_map(|tag| tag.strip_suffix('m')?.parse::<f32>().ok())
            .unwrap_or(4.5);
        let distant = matches!(signal.kind, SignalKind::Distant);
        if semaphore && distant {
            Self {
                front_z: 0.31,
                designation_y: 0.78_f32.min(nominal_height * 0.30),
                equipment_top: (nominal_height - 1.12).max(1.55),
                semaphore,
            }
        } else if semaphore {
            Self {
                front_z: 0.34,
                // Prototype photographs place the plate in the lower bare
                // lattice, below the additional-light box.
                designation_y: 2.38,
                equipment_top: (nominal_height - 1.55).min(4.55).max(3.30),
                semaphore,
            }
        } else {
            Self {
                front_z: 0.30,
                designation_y: 1.45,
                equipment_top: (nominal_height - 0.65).max(2.35),
                semaphore,
            }
        }
    }
}

struct Stack {
    x: f32,
    top: f32,
}

impl Stack {
    fn take(&mut self, height: f32) -> Vec3 {
        let centre = self.top - height * 0.5;
        self.top -= height + 0.10;
        Vec3::new(self.x, centre, 0.0)
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    shared: &SignalFittingMaterials,
    parent: Entity,
    index: usize,
    signal: &SignalView<'_>,
) {
    let layout = Layout::of(signal);
    // A Hauptsignal carries the plate.  A freestanding Vorsignal may have an
    // operational name in the route data as well, but the prototype normally
    // does not repeat it on a plate at the mast.
    if !signal.designation.trim().is_empty()
        && matches!(signal.kind, SignalKind::Main | SignalKind::Combined)
    {
        spawn_designation(
            commands,
            meshes,
            materials,
            images,
            shared,
            parent,
            signal.designation.trim(),
            Vec3::new(0.0, layout.designation_y, layout.front_z),
        );
    }
    if signal.addons.is_empty() {
        return;
    }

    // Separate side brackets are faithful to dense real installations and,
    // crucially, keep valid combinations from intersecting one another.
    let mut left = Stack {
        // The characteristic A/V box overlaps the mast slightly in front
        // views; it is not suspended half a metre out on a decorative arm.
        x: -0.27,
        top: layout.equipment_top,
    };
    let mut right = Stack {
        x: 0.39,
        top: layout.equipment_top,
    };
    spawn_main_addons(
        commands,
        meshes,
        materials,
        images,
        shared,
        parent,
        index,
        signal.addons,
        layout,
        &mut left,
        &mut right,
    );
    spawn_distant_addons(
        commands,
        meshes,
        materials,
        images,
        shared,
        parent,
        index,
        signal.addons,
        layout,
        &mut right,
    );
}

#[allow(clippy::too_many_arguments)]
fn spawn_main_addons(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    shared: &SignalFittingMaterials,
    parent: Entity,
    index: usize,
    addons: &SignalAddons,
    layout: Layout,
    left: &mut Stack,
    right: &mut Stack,
) {
    let a_zs1 = addons.zs1 == Some(Zs1Display::ThreeLights);
    let a_zs8 = addons.zs8 == Some(Zs8Display::ThreeLights);
    if a_zs1 || a_zs8 {
        let centre = left.take(0.42) + Vec3::Z * layout.front_z;
        let steady = a_zs1.then(|| "zs1".to_owned()).into_iter().collect();
        let flashing = a_zs8.then(|| "zs8".to_owned()).into_iter().collect();
        spawn_three_lights(
            commands, meshes, shared, parent, index, centre, true, steady, flashing, true,
        );
    }
    if addons.zs1 == Some(Zs1Display::BlinkingLight) {
        let centre = left.take(0.30) + Vec3::Z * layout.front_z;
        spawn_single_light(
            commands,
            meshes,
            shared,
            parent,
            index,
            centre,
            Vec::new(),
            vec!["zs1".into()],
        );
    }
    if addons.zs7 {
        let centre = left.take(0.42) + Vec3::Z * layout.front_z;
        spawn_three_lights(
            commands,
            meshes,
            shared,
            parent,
            index,
            centre,
            false,
            vec!["zs7".into()],
            Vec::new(),
            false,
        );
    }

    if let Some(values) = addons.zs2.as_ref() {
        let centre = right.take(0.46) + Vec3::Z * layout.front_z;
        spawn_theatre(
            commands,
            meshes,
            materials,
            images,
            shared,
            parent,
            index,
            centre,
            "zs2",
            values.iter().map(String::as_str),
            [255, 248, 210, 255],
        );
    }
    if let Some(zs3) = addons.zs3.as_ref() {
        let centre = right.take(0.54) + Vec3::Z * layout.front_z;
        spawn_number_indicator(
            commands,
            meshes,
            materials,
            images,
            shared,
            parent,
            index,
            centre,
            "zs3",
            zs3,
            [248, 248, 235, 255],
            layout.semaphore,
        );
    }

    let strip_zs6 = addons.zs6 == Some(ZsConstruction::Light);
    let strip_zs8 = addons.zs8 == Some(Zs8Display::LightStrip);
    if strip_zs6 || strip_zs8 {
        let centre = right.take(0.48) + Vec3::Z * layout.front_z;
        spawn_light_symbol(
            commands,
            meshes,
            materials,
            images,
            shared,
            parent,
            index,
            centre,
            Symbol::Zs6,
            strip_zs6.then(|| "zs6".to_owned()).into_iter().collect(),
            strip_zs8.then(|| "zs8".to_owned()).into_iter().collect(),
            [255, 248, 210, 255],
        );
    }
    if addons.zs6 == Some(ZsConstruction::Form) {
        let centre = right.take(0.48) + Vec3::Z * layout.front_z;
        spawn_form_symbol(
            commands,
            meshes,
            materials,
            images,
            shared,
            parent,
            centre,
            Symbol::Zs6,
            [245, 245, 230, 255],
        );
    }
    if let Some(construction) = addons.zs13 {
        let centre = right.take(0.42) + Vec3::Z * layout.front_z;
        match construction {
            ZsConstruction::Form => spawn_form_symbol(
                commands,
                meshes,
                materials,
                images,
                shared,
                parent,
                centre,
                Symbol::Zs13,
                [244, 174, 24, 255],
            ),
            ZsConstruction::Light => spawn_light_symbol(
                commands,
                meshes,
                materials,
                images,
                shared,
                parent,
                index,
                centre,
                Symbol::Zs13,
                vec!["zs13".into()],
                Vec::new(),
                [255, 176, 20, 255],
            ),
        }
    }

    // Static lower mast plates have fixed, easily readable positions and must
    // not move upward with the blade height.
    if addons.zs12 {
        spawn_zs12(
            commands,
            meshes,
            materials,
            images,
            shared,
            parent,
            // Prototype installations stack the M board immediately above
            // the signal designation in one vertical plate row; placing it
            // beside the designation was especially conspicuous at close
            // range.
            Vec3::new(0.0, layout.designation_y + 0.365, layout.front_z),
        );
    }
    if addons.zs103 {
        spawn_zs103(
            commands,
            meshes,
            shared,
            parent,
            Vec3::new(-0.28, 1.18, layout.front_z),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_distant_addons(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    shared: &SignalFittingMaterials,
    parent: Entity,
    index: usize,
    addons: &SignalAddons,
    layout: Layout,
    right: &mut Stack,
) {
    if let Some(values) = addons.zs2v.as_ref() {
        let centre = right.take(0.46) + Vec3::Z * layout.front_z;
        spawn_theatre(
            commands,
            meshes,
            materials,
            images,
            shared,
            parent,
            index,
            centre,
            "zs2v",
            values.iter().map(String::as_str),
            [255, 176, 20, 255],
        );
    }
    if let Some(zs3v) = addons.zs3v.as_ref() {
        let centre = right.take(0.54) + Vec3::Z * layout.front_z;
        spawn_number_indicator(
            commands,
            meshes,
            materials,
            images,
            shared,
            parent,
            index,
            centre,
            "zs3v",
            zs3v,
            [255, 176, 20, 255],
            true,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_designation(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    shared: &SignalFittingMaterials,
    parent: Entity,
    designation: &str,
    centre: Vec3,
) {
    let (image, _) = designation_image(designation);
    // The current DB standard plate is 285 x 300 mm.  Real multi-part names do
    // not turn it into a banner: their lines share the same standard field
    // (for example `20` over `ZW70` in drawing S 541.1).
    let width = 0.285;
    let height = 0.30;
    let texture = images.add(image);
    let face = materials.add(StandardMaterial {
        base_color_texture: Some(texture),
        perceptual_roughness: 0.46,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(width, height, 0.026))),
        MeshMaterial3d(shared.white.clone()),
        Transform::from_translation(centre),
        ChildOf(parent),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Rectangle::new(width, height))),
        MeshMaterial3d(face),
        Transform::from_translation(centre + Vec3::Z * (0.013 + FRONT_GAP)),
        ChildOf(parent),
    ));
}

#[allow(clippy::too_many_arguments)]
fn spawn_three_lights(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    shared: &SignalFittingMaterials,
    parent: Entity,
    signal: usize,
    centre: Vec3,
    points_up: bool,
    steady: Vec<String>,
    flashing: Vec<String>,
    white: bool,
) {
    spawn_bracket(commands, meshes, shared, parent, centre);
    // Zs 1/Zs 8 use the characteristic A-shaped cast housing shown in Ril
    // 301: a narrow top, sloping shoulders and a broad rectangular foot.  Zs
    // 7, by contrast, sits in a plain rectangular housing.  Treating both as
    // the same cuboid made the most recognisable part of the fitting wrong.
    let body = if points_up {
        meshes.add(polygon_prism_mesh(
            &[
                Vec2::new(-0.074, 0.206),
                Vec2::new(0.074, 0.206),
                Vec2::new(0.220, -0.087),
                Vec2::new(0.220, -0.206),
                Vec2::new(-0.220, -0.206),
                Vec2::new(-0.220, -0.087),
            ],
            0.14,
        ))
    } else {
        meshes.add(Cuboid::new(0.44, 0.408, 0.14))
    };
    commands.spawn((
        Mesh3d(body),
        MeshMaterial3d(shared.dark.clone()),
        Transform::from_translation(centre),
        ChildOf(parent),
    ));
    // Ratios are taken from the rulebook drawings instead of merely placing
    // three oversized lamps at the corners of an arbitrary square.
    let positions = if points_up {
        [
            Vec2::new(-0.110, -0.122),
            Vec2::new(0.110, -0.122),
            Vec2::new(0.0, 0.060),
        ]
    } else {
        [
            Vec2::new(-0.110, 0.116),
            Vec2::new(0.110, 0.116),
            Vec2::new(0.0, -0.109),
        ]
    };
    for p in positions {
        spawn_lens_sized(
            commands,
            meshes,
            shared,
            parent,
            signal,
            centre + Vec3::new(p.x, p.y, 0.075),
            steady.clone(),
            flashing.clone(),
            white,
            0.044,
            0.034,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_single_light(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    shared: &SignalFittingMaterials,
    parent: Entity,
    signal: usize,
    centre: Vec3,
    steady: Vec<String>,
    flashing: Vec<String>,
) {
    spawn_bracket(commands, meshes, shared, parent, centre);
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(0.27, 0.27, 0.14))),
        MeshMaterial3d(shared.dark.clone()),
        Transform::from_translation(centre),
        ChildOf(parent),
    ));
    spawn_lens_sized(
        commands,
        meshes,
        shared,
        parent,
        signal,
        centre + Vec3::Z * 0.075,
        steady,
        flashing,
        true,
        0.070,
        0.054,
    );
}

#[allow(clippy::too_many_arguments)]
fn spawn_lens_sized(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    shared: &SignalFittingMaterials,
    parent: Entity,
    signal: usize,
    centre: Vec3,
    steady: Vec<String>,
    flashing: Vec<String>,
    white: bool,
    rim_radius: f32,
    lit_radius: f32,
) {
    commands.spawn((
        Mesh3d(meshes.add(Circle::new(rim_radius))),
        MeshMaterial3d(shared.lens_off.clone()),
        Transform::from_translation(centre),
        ChildOf(parent),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Circle::new(lit_radius))),
        MeshMaterial3d(if white {
            shared.lamp_white.clone()
        } else {
            shared.lamp_yellow.clone()
        }),
        Transform::from_translation(centre + Vec3::Z * FRONT_GAP),
        SignalLamp {
            signal,
            steady,
            flashing,
        },
        Visibility::Hidden,
        ChildOf(parent),
    ));
}

#[allow(clippy::too_many_arguments)]
fn spawn_theatre<'a>(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    shared: &SignalFittingMaterials,
    parent: Entity,
    signal: usize,
    centre: Vec3,
    prefix: &str,
    values: impl Iterator<Item = &'a str>,
    colour: [u8; 4],
) {
    let values: Vec<_> = values.filter(|value| !value.trim().is_empty()).collect();
    let widest = values
        .iter()
        .map(|value| value.trim().chars().count())
        .max()
        .unwrap_or(1);
    // One-character route indicators use the familiar narrow 5x7 field.  A
    // two-digit speed code needs a wider factory housing rather than squeezing
    // two numerals into that same square.
    let body_width = if widest > 1 { 0.58 } else { 0.40 };
    let body_height = 0.50;
    spawn_indicator_box(
        commands,
        meshes,
        shared,
        parent,
        centre,
        body_width,
        body_height,
    );
    for value in &values {
        let aliases = lamp_aliases(prefix, value.trim(), values.len());
        spawn_lit_texture(
            commands,
            meshes,
            materials,
            images,
            parent,
            signal,
            centre,
            body_width - 0.055,
            body_height - 0.055,
            dot_text_image(value.trim(), colour),
            aliases,
            Vec::new(),
            colour,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_number_indicator(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    shared: &SignalFittingMaterials,
    parent: Entity,
    signal: usize,
    centre: Vec3,
    prefix: &str,
    indicator: &ZsNumber,
    colour: [u8; 4],
    point_down: bool,
) {
    match indicator.construction {
        ZsConstruction::Form => {
            let Some(value) = indicator.values.first() else {
                return;
            };
            spawn_triangle_board(
                commands,
                meshes,
                materials,
                images,
                shared,
                parent,
                centre,
                &value.to_string(),
                colour,
                point_down,
            );
        }
        ZsConstruction::Light => {
            let strings: Vec<_> = indicator.values.iter().map(u8::to_string).collect();
            spawn_theatre(
                commands,
                meshes,
                materials,
                images,
                shared,
                parent,
                signal,
                centre,
                prefix,
                strings.iter().map(String::as_str),
                colour,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_triangle_board(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    shared: &SignalFittingMaterials,
    parent: Entity,
    centre: Vec3,
    value: &str,
    colour: [u8; 4],
    point_down: bool,
) {
    spawn_bracket(commands, meshes, shared, parent, centre);
    let rotation = if point_down {
        Quat::IDENTITY
    } else {
        Quat::from_rotation_z(std::f32::consts::PI)
    };
    commands.spawn((
        Mesh3d(meshes.add(triangle_prism_mesh(0.58, 0.52, 0.035))),
        MeshMaterial3d(shared.steel.clone()),
        Transform::from_translation(centre).with_rotation(rotation),
        ChildOf(parent),
    ));
    let border = materials.add(StandardMaterial {
        base_color: Color::srgba_u8(colour[0], colour[1], colour[2], colour[3]),
        perceptual_roughness: 0.50,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(triangle_mesh(0.58, 0.52))),
        MeshMaterial3d(border),
        Transform::from_translation(centre + Vec3::Z * (0.018 + FRONT_GAP)).with_rotation(rotation),
        ChildOf(parent),
    ));
    commands.spawn((
        Mesh3d(meshes.add(triangle_mesh(0.49, 0.43))),
        MeshMaterial3d(shared.dark.clone()),
        Transform::from_translation(centre + Vec3::Z * (0.018 + FRONT_GAP * 2.0))
            .with_rotation(rotation),
        ChildOf(parent),
    ));
    let texture = images.add(transparent_text_image(value, colour));
    let material = materials.add(StandardMaterial {
        base_color_texture: Some(texture),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Rectangle::new(0.30, 0.30))),
        MeshMaterial3d(material),
        // Turning the triangle over must never turn its number upside down.
        // Its small vertical offset follows the broad half of either outline.
        Transform::from_translation(
            centre
                + Vec3::new(
                    0.0,
                    if point_down { 0.035 } else { -0.035 },
                    0.018 + FRONT_GAP * 3.0,
                ),
        ),
        ChildOf(parent),
    ));
}

#[derive(Clone, Copy)]
enum Symbol {
    Zs6,
    Zs13,
}

impl Symbol {
    fn dimensions(self) -> (f32, f32, f32, f32) {
        match self {
            // Zs 6 is the almost-square rectangular board.
            Self::Zs6 => (0.47, 0.40, 0.43, 0.36),
            // Zs 13 uses the noticeably upright 5x7 indicator field.
            Self::Zs13 => (0.35, 0.50, 0.31, 0.46),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_form_symbol(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    shared: &SignalFittingMaterials,
    parent: Entity,
    centre: Vec3,
    symbol: Symbol,
    colour: [u8; 4],
) {
    let (body_width, body_height, face_width, face_height) = symbol.dimensions();
    spawn_indicator_box(
        commands,
        meshes,
        shared,
        parent,
        centre,
        body_width,
        body_height,
    );
    let texture = images.add(symbol_image(symbol, colour, true));
    let material = materials.add(StandardMaterial {
        base_color_texture: Some(texture),
        perceptual_roughness: 0.52,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Rectangle::new(face_width, face_height))),
        MeshMaterial3d(material),
        Transform::from_translation(centre + Vec3::Z * (0.071 + FRONT_GAP)),
        ChildOf(parent),
    ));
}

#[allow(clippy::too_many_arguments)]
fn spawn_light_symbol(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    shared: &SignalFittingMaterials,
    parent: Entity,
    signal: usize,
    centre: Vec3,
    symbol: Symbol,
    steady: Vec<String>,
    flashing: Vec<String>,
    colour: [u8; 4],
) {
    let (body_width, body_height, face_width, face_height) = symbol.dimensions();
    spawn_indicator_box(
        commands,
        meshes,
        shared,
        parent,
        centre,
        body_width,
        body_height,
    );
    spawn_lit_texture(
        commands,
        meshes,
        materials,
        images,
        parent,
        signal,
        centre,
        face_width,
        face_height,
        symbol_image(symbol, colour, false),
        steady,
        flashing,
        colour,
    );
}

fn spawn_indicator_box(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    shared: &SignalFittingMaterials,
    parent: Entity,
    centre: Vec3,
    width: f32,
    height: f32,
) {
    spawn_bracket(commands, meshes, shared, parent, centre);
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(width, height, 0.14))),
        MeshMaterial3d(shared.dark.clone()),
        Transform::from_translation(centre),
        ChildOf(parent),
    ));
}

#[allow(clippy::too_many_arguments)]
fn spawn_lit_texture(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    parent: Entity,
    signal: usize,
    centre: Vec3,
    width: f32,
    height: f32,
    image: Image,
    steady: Vec<String>,
    flashing: Vec<String>,
    colour: [u8; 4],
) {
    let texture = images.add(image);
    let tint = Color::srgba_u8(colour[0], colour[1], colour[2], colour[3]);
    let material = materials.add(StandardMaterial {
        // The glyph pixels already carry the prescribed colour.  Tinting the
        // texture a second time squares the RGB values and made Zs 2v/Zs 3v
        // conspicuously brown.
        base_color: Color::WHITE,
        base_color_texture: Some(texture.clone()),
        emissive: tint.to_linear() * 7.0,
        emissive_texture: Some(texture),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Rectangle::new(width, height))),
        MeshMaterial3d(material),
        Transform::from_translation(centre + Vec3::Z * (0.071 + FRONT_GAP)),
        SignalLamp {
            signal,
            steady,
            flashing,
        },
        Visibility::Hidden,
        ChildOf(parent),
    ));
}

fn spawn_zs12(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    shared: &SignalFittingMaterials,
    parent: Entity,
    centre: Vec3,
) {
    spawn_bracket(commands, meshes, shared, parent, centre);
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(0.33, 0.33, 0.045))),
        MeshMaterial3d(shared.white.clone()),
        Transform::from_translation(centre),
        ChildOf(parent),
    ));
    let texture = images.add(zs12_image());
    let material = materials.add(StandardMaterial {
        base_color_texture: Some(texture),
        perceptual_roughness: 0.48,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Rectangle::new(0.33, 0.33))),
        MeshMaterial3d(material),
        Transform::from_translation(centre + Vec3::Z * (0.023 + FRONT_GAP)),
        ChildOf(parent),
    ));
}

fn spawn_zs103(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    shared: &SignalFittingMaterials,
    parent: Entity,
    centre: Vec3,
) {
    spawn_bracket(commands, meshes, shared, parent, centre);
    // Ril 301.0301 shows six (not five) touching-looking diamonds in a very
    // slender portrait board.  The previous 250 x 920 mm approximation was
    // both one diamond short and visibly too squat beside the prototype.
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(ZS103_WIDTH, ZS103_HEIGHT, 0.05))),
        MeshMaterial3d(shared.dark.clone()),
        Transform::from_translation(centre),
        ChildOf(parent),
    ));
    for i in 0..ZS103_DIAMONDS {
        commands.spawn((
            Mesh3d(meshes.add(Rectangle::new(ZS103_DIAMOND, ZS103_DIAMOND))),
            MeshMaterial3d(shared.white.clone()),
            Transform::from_translation(
                centre + Vec3::new(0.0, ZS103_FIRST_Y - i as f32 * ZS103_STEP_Y, 0.029),
            )
            .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4)),
            ChildOf(parent),
        ));
    }
}

fn spawn_bracket(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    shared: &SignalFittingMaterials,
    parent: Entity,
    centre: Vec3,
) {
    if centre.x.abs() < 0.08 {
        return;
    }
    let width = centre.x.abs();
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(width, 0.045, 0.045))),
        MeshMaterial3d(shared.steel.clone()),
        Transform::from_xyz(centre.x * 0.5, centre.y, centre.z - 0.09),
        ChildOf(parent),
    ));
}

fn lamp_aliases(prefix: &str, value: &str, count: usize) -> Vec<String> {
    let mut aliases = vec![format!("{prefix}_{value}")];
    if count == 1 {
        aliases.push(prefix.to_owned());
    }
    aliases
}

fn triangle_mesh(width: f32, height: f32) -> Mesh {
    use bevy::render::mesh::{Indices, PrimitiveTopology};
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [-width * 0.5, height * 0.5, 0.0],
            [width * 0.5, height * 0.5, 0.0],
            [0.0, -height * 0.5, 0.0],
        ],
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 3]);
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]],
    );
    // The signal face looks along +Z.  This order is counter-clockwise from
    // that side, so normal and culling agree.
    mesh.insert_indices(Indices::U32(vec![0, 2, 1]));
    mesh
}

/// Shallow sheet-metal body of a triangular form indicator.  The front face
/// is overlaid by the coloured border and black inset; this mesh supplies the
/// real edge thickness and the plain metal rear instead of disappearing when
/// viewed from behind.
fn triangle_prism_mesh(width: f32, height: f32, depth: f32) -> Mesh {
    polygon_prism_mesh(
        &[
            Vec2::new(-width * 0.5, height * 0.5),
            Vec2::new(width * 0.5, height * 0.5),
            Vec2::new(0.0, -height * 0.5),
        ],
        depth,
    )
}

/// Extrudes a clockwise convex outline into a closed shallow sheet-metal
/// body.  Front, rear and edge faces are all present, so a fitting remains
/// plausible when the player walks around the signal instead of becoming a
/// one-sided decal.
fn polygon_prism_mesh(shape: &[Vec2], depth: f32) -> Mesh {
    use bevy::render::mesh::{Indices, PrimitiveTopology};

    assert!(
        shape.len() >= 3,
        "a prism outline needs at least three points"
    );
    let front = depth * 0.5;
    let back = -front;
    let mut positions = Vec::<[f32; 3]>::new();
    let mut normals = Vec::<[f32; 3]>::new();
    let mut uvs = Vec::<[f32; 2]>::new();
    let mut indices = Vec::<u32>::new();
    let mut triangle = |points: [(Vec2, f32); 3], normal: Vec3| {
        let start = positions.len() as u32;
        for (point, z) in points {
            positions.push([point.x, point.y, z]);
            normals.push(normal.to_array());
            uvs.push([0.0, 0.0]);
        }
        indices.extend_from_slice(&[start, start + 1, start + 2]);
    };
    for index in 1..shape.len() - 1 {
        triangle(
            [
                (shape[0], front),
                (shape[index + 1], front),
                (shape[index], front),
            ],
            Vec3::Z,
        );
        triangle(
            [
                (shape[0], back),
                (shape[index], back),
                (shape[index + 1], back),
            ],
            Vec3::NEG_Z,
        );
    }
    for edge in 0..shape.len() {
        let a = shape[edge];
        let b = shape[(edge + 1) % shape.len()];
        let direction = b - a;
        let normal = Vec3::new(-direction.y, direction.x, 0.0).normalize();
        let start = positions.len() as u32;
        for (point, z) in [(a, front), (b, front), (b, back), (a, back)] {
            positions.push([point.x, point.y, z]);
            normals.push(normal.to_array());
            uvs.push([0.0, 0.0]);
        }
        indices.extend_from_slice(&[start, start + 1, start + 2, start, start + 2, start + 3]);
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

fn designation_image(text: &str) -> (Image, usize) {
    const WIDTH: u32 = 244;
    const HEIGHT: u32 = 256;
    const FONT: f32 = 178.0;
    let font = FontRef::try_from_slice(DINISH_CONDENSED_BOLD).expect("bundled DINish font");
    let lines = designation_lines(text);
    let rows = lines.len();
    let row_height = HEIGHT / rows as u32;
    let mut pixels = vec![0_u8; (WIDTH * HEIGHT * 4) as usize];
    fill(&mut pixels, [224, 224, 214, 255]);
    frame(&mut pixels, WIDTH, HEIGHT, 7, [25, 27, 24, 255]);
    for (row, line) in lines.iter().enumerate() {
        let available = WIDTH as f32 - 42.0;
        // Two-line plates use almost the whole half-field, as the current
        // `20` / `ZW70` production example does.  Dividing the one-line font
        // size by two left both rows unnecessarily tiny.
        let row_font = (row_height as f32 * 0.88).min(FONT);
        let at_font = text_advance(&font, line, row_font);
        let font_size = if at_font > available {
            row_font * available / at_font
        } else {
            row_font
        };
        draw_text_in_rect(
            &mut pixels,
            WIDTH,
            HEIGHT,
            &font,
            line,
            font_size,
            [18, 19, 17, 255],
            row as u32 * row_height,
            row_height,
        );
    }
    // Four dark screw heads are visible on both old enamel and newer plates.
    for (x, y) in [
        (18, 18),
        (WIDTH - 19, 18),
        (18, HEIGHT - 19),
        (WIDTH - 19, HEIGHT - 19),
    ] {
        draw_disc(&mut pixels, WIDTH, HEIGHT, x, y, 5, [58, 59, 55, 255]);
    }
    (rgba_image(WIDTH, HEIGHT, pixels), rows)
}

/// Splits a designation the same way as the narrow prototype plates: the
/// location prefix sits over the operational name (`24` / `P3`).  A plain A,
/// N1, P12 or block number stays on one line and is scaled to the field.
fn designation_lines(text: &str) -> Vec<String> {
    let text = text.trim();
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= 2 {
        return vec![text.to_owned()];
    }
    if let Some(split) = chars.iter().position(|character| character.is_alphabetic())
        && split > 0
    {
        return vec![
            chars[..split].iter().collect::<String>().trim().to_owned(),
            chars[split..].iter().collect::<String>().trim().to_owned(),
        ];
    }
    vec![text.to_owned()]
}

fn transparent_text_image(text: &str, colour: [u8; 4]) -> Image {
    let font = FontRef::try_from_slice(DINISH_CONDENSED_BOLD).expect("bundled DINish font");
    let mut pixels = vec![0_u8; 256 * 256 * 4];
    draw_text(&mut pixels, 256, 256, &font, text, 190.0, colour);
    rgba_image(256, 256, pixels)
}

/// Rasterises a switchable theatre indication as individual lamp points.  A
/// filled computer-font glyph looked unlike both the Ril diagrams and real
/// incandescent/LED matrices; the 5x7 cells also remain readable when the
/// same physical display can show several route-dependent values.
fn dot_text_image(text: &str, colour: [u8; 4]) -> Image {
    const WIDTH: u32 = 256;
    const HEIGHT: u32 = 256;
    let characters: Vec<_> = text
        .trim()
        .chars()
        .map(|character| character.to_ascii_uppercase())
        .collect();
    let characters = if characters.is_empty() {
        vec!['?']
    } else {
        characters
    };
    let columns = characters.len() * 5 + characters.len().saturating_sub(1);
    let step_x = if columns > 1 {
        (190.0 / (columns - 1) as f32).min(28.0)
    } else {
        28.0
    };
    let step_y = 28.0;
    let radius = (step_x.min(step_y) * 0.24).clamp(2.0, 7.0).round() as u32;
    let left = (WIDTH as f32 - (columns - 1) as f32 * step_x) * 0.5;
    let top = (HEIGHT as f32 - 6.0 * step_y) * 0.5;
    let mut pixels = vec![0_u8; (WIDTH * HEIGHT * 4) as usize];
    for (character_index, character) in characters.into_iter().enumerate() {
        let rows = dot_glyph(character);
        let first_column = character_index * 6;
        for (row, pattern) in rows.into_iter().enumerate() {
            for column in 0..5 {
                if pattern & (1 << (4 - column)) == 0 {
                    continue;
                }
                let x = (left + (first_column + column) as f32 * step_x).round() as u32;
                let y = (top + row as f32 * step_y).round() as u32;
                draw_disc(&mut pixels, WIDTH, HEIGHT, x, y, radius, colour);
            }
        }
    }
    rgba_image(WIDTH, HEIGHT, pixels)
}

fn dot_glyph(character: char) -> [u8; 7] {
    match character {
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111,
        ],
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'G' => [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110,
        ],
        'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'I' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ],
        'J' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100,
        ],
        'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ],
        '6' => [
            0b01110, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
        ],
        '-' => [0, 0, 0, 0b11111, 0, 0, 0],
        ' ' => [0; 7],
        _ => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0, 0b00100],
    }
}

fn symbol_image(symbol: Symbol, colour: [u8; 4], opaque: bool) -> Image {
    let (width, height) = match symbol {
        Symbol::Zs6 => (256, 224),
        Symbol::Zs13 => (192, 256),
    };
    let mut pixels = vec![0_u8; (width * height * 4) as usize];
    if opaque {
        fill(&mut pixels, [10, 12, 11, 255]);
        if matches!(symbol, Symbol::Zs6) {
            frame(&mut pixels, width, height, 9, [235, 235, 220, 255]);
        }
    }
    match symbol {
        Symbol::Zs6 => {
            // Viewer-facing geometry: the diagonal rises from right to left;
            // both ends bend vertically as prescribed by Ril 301.
            let points = [(45, 58), (45, 92), (211, 166), (211, 198)];
            if opaque {
                draw_polyline(&mut pixels, width, height, &points, 18.0, colour);
            } else {
                draw_dotted_polyline(&mut pixels, width, height, &points, 19.0, 6, colour);
            }
        }
        Symbol::Zs13 => {
            // Upright T rotated 90 degrees counter-clockwise: its former top
            // crossbar is on the left and the stem points right.
            let stem = [(43, 128), (163, 128)];
            let bar = [(43, 48), (43, 208)];
            if opaque {
                draw_polyline(&mut pixels, width, height, &stem, 20.0, colour);
                draw_polyline(&mut pixels, width, height, &bar, 20.0, colour);
            } else {
                draw_dotted_polyline(&mut pixels, width, height, &stem, 24.0, 7, colour);
                draw_dotted_polyline(&mut pixels, width, height, &bar, 24.0, 7, colour);
            }
        }
    }
    rgba_image(width, height, pixels)
}

fn zs12_image() -> Image {
    let (width, height) = (256, 256);
    let red = [170, 24, 27, 255];
    let mut pixels = vec![0_u8; (width * height * 4) as usize];
    fill(&mut pixels, [232, 231, 220, 255]);
    frame(&mut pixels, width, height, 12, red);
    // The rulebook calls for a *script* M.  The former straight zig-zag read
    // as a block-letter W.  These connected Bezier strokes reproduce the two
    // rising pen strokes and curled terminals visible on real Zs 12 boards,
    // while keeping the symbol independent of fonts installed on the host.
    let m = [
        [
            Vec2::new(36.0, 164.0),
            Vec2::new(14.0, 201.0),
            Vec2::new(35.0, 214.0),
            Vec2::new(57.0, 188.0),
        ],
        [
            Vec2::new(57.0, 188.0),
            Vec2::new(79.0, 159.0),
            Vec2::new(78.0, 78.0),
            Vec2::new(103.0, 57.0),
        ],
        [
            Vec2::new(103.0, 57.0),
            Vec2::new(94.0, 103.0),
            Vec2::new(99.0, 158.0),
            Vec2::new(112.0, 184.0),
        ],
        [
            Vec2::new(112.0, 184.0),
            Vec2::new(127.0, 145.0),
            Vec2::new(132.0, 78.0),
            Vec2::new(156.0, 56.0),
        ],
        [
            Vec2::new(156.0, 56.0),
            Vec2::new(146.0, 108.0),
            Vec2::new(151.0, 169.0),
            Vec2::new(166.0, 188.0),
        ],
        [
            Vec2::new(166.0, 188.0),
            Vec2::new(181.0, 208.0),
            Vec2::new(220.0, 195.0),
            Vec2::new(216.0, 161.0),
        ],
    ];
    draw_cubic_path(&mut pixels, width, height, &m, 13.0, red);
    // The enamel face is held by four exposed brass-toned fasteners, as on
    // the preserved board used for the visual reference.
    for (x, y) in [(23, 23), (232, 23), (23, 232), (232, 232)] {
        draw_disc(&mut pixels, width, height, x, y, 6, [112, 91, 54, 255]);
        draw_disc(&mut pixels, width, height, x, y, 2, [45, 42, 34, 255]);
    }
    rgba_image(width, height, pixels)
}

fn text_advance(font: &FontRef<'_>, text: &str, px: f32) -> f32 {
    let scaled = font.as_scaled(PxScale::from(px));
    let mut width = 0.0;
    let mut previous = None;
    for character in text.chars() {
        let id = font.glyph_id(character);
        if let Some(previous) = previous {
            width += scaled.kern(previous, id);
        }
        width += scaled.h_advance(id);
        previous = Some(id);
    }
    width
}

#[allow(clippy::too_many_arguments)]
fn draw_text(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    font: &FontRef<'_>,
    text: &str,
    px: f32,
    colour: [u8; 4],
) {
    draw_text_in_rect(pixels, width, height, font, text, px, colour, 0, height);
}

#[allow(clippy::too_many_arguments)]
fn draw_text_in_rect(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    font: &FontRef<'_>,
    text: &str,
    px: f32,
    colour: [u8; 4],
    top: u32,
    row_height: u32,
) {
    let scale = PxScale::from(px);
    let scaled = font.as_scaled(scale);
    let advance = text_advance(font, text, px);
    let line_height = scaled.ascent() - scaled.descent();
    let baseline = top as f32 + (row_height as f32 - line_height) * 0.5 + scaled.ascent();
    let mut caret = ((width as f32 - advance) * 0.5).max(1.0);
    let mut previous = None;
    for character in text.chars() {
        let id = font.glyph_id(character);
        if let Some(previous) = previous {
            caret += scaled.kern(previous, id);
        }
        let glyph = id.with_scale_and_position(scale, point(caret, baseline));
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|x, y, coverage| {
                let x = bounds.min.x.floor() as i32 + x as i32;
                let y = bounds.min.y.floor() as i32 + y as i32;
                if x >= 0 && y >= 0 && x < width as i32 && y < height as i32 {
                    blend(pixels, width, x as u32, y as u32, colour, coverage);
                }
            });
        }
        caret += scaled.h_advance(id);
        previous = Some(id);
    }
}

fn draw_disc(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    centre_x: u32,
    centre_y: u32,
    radius: u32,
    colour: [u8; 4],
) {
    let radius_squared = (radius * radius) as i32;
    for y in centre_y.saturating_sub(radius)..=(centre_y + radius).min(height - 1) {
        for x in centre_x.saturating_sub(radius)..=(centre_x + radius).min(width - 1) {
            let dx = x as i32 - centre_x as i32;
            let dy = y as i32 - centre_y as i32;
            if dx * dx + dy * dy <= radius_squared {
                set_pixel(pixels, width, x, y, colour);
            }
        }
    }
}

fn fill(pixels: &mut [u8], colour: [u8; 4]) {
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.copy_from_slice(&colour);
    }
}

fn frame(pixels: &mut [u8], width: u32, height: u32, thickness: u32, colour: [u8; 4]) {
    for y in 0..height {
        for x in 0..width {
            if x < thickness || x >= width - thickness || y < thickness || y >= height - thickness {
                set_pixel(pixels, width, x, y, colour);
            }
        }
    }
}

fn draw_polyline(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    points: &[(i32, i32)],
    thickness: f32,
    colour: [u8; 4],
) {
    for y in 0..height {
        for x in 0..width {
            let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let distance = points
                .windows(2)
                .map(|segment| {
                    segment_distance(
                        p,
                        Vec2::new(segment[0].0 as f32, segment[0].1 as f32),
                        Vec2::new(segment[1].0 as f32, segment[1].1 as f32),
                    )
                })
                .fold(f32::INFINITY, f32::min);
            let coverage = (thickness * 0.5 + 1.0 - distance).clamp(0.0, 1.0);
            if coverage > 0.0 {
                blend(pixels, width, x, y, colour, coverage);
            }
        }
    }
}

fn draw_dotted_polyline(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    points: &[(i32, i32)],
    spacing: f32,
    radius: u32,
    colour: [u8; 4],
) {
    if points.len() < 2 {
        return;
    }
    let points: Vec<_> = points
        .iter()
        .map(|(x, y)| Vec2::new(*x as f32, *y as f32))
        .collect();
    let lengths: Vec<_> = points
        .windows(2)
        .map(|segment| segment[0].distance(segment[1]))
        .collect();
    let total: f32 = lengths.iter().sum();
    let intervals = (total / spacing).round().max(1.0) as usize;
    for dot in 0..=intervals {
        let mut distance = total * dot as f32 / intervals as f32;
        let mut position = *points.last().expect("polyline has points");
        for (segment, length) in points.windows(2).zip(lengths.iter().copied()) {
            if distance <= length {
                position = segment[0].lerp(segment[1], distance / length.max(f32::EPSILON));
                break;
            }
            distance -= length;
        }
        draw_disc(
            pixels,
            width,
            height,
            position
                .x
                .round()
                .clamp(0.0, width.saturating_sub(1) as f32) as u32,
            position
                .y
                .round()
                .clamp(0.0, height.saturating_sub(1) as f32) as u32,
            radius,
            colour,
        );
    }
}

fn draw_cubic_path(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    curves: &[[Vec2; 4]],
    thickness: f32,
    colour: [u8; 4],
) {
    let mut points = Vec::with_capacity(curves.len() * 25 + 1);
    for (curve_index, [a, b, c, d]) in curves.iter().copied().enumerate() {
        for step in usize::from(curve_index > 0)..=24 {
            let t = step as f32 / 24.0;
            let one_minus_t = 1.0 - t;
            let point = a * one_minus_t.powi(3)
                + b * (3.0 * one_minus_t.powi(2) * t)
                + c * (3.0 * one_minus_t * t.powi(2))
                + d * t.powi(3);
            points.push((point.x.round() as i32, point.y.round() as i32));
        }
    }
    draw_polyline(pixels, width, height, &points, thickness, colour);
}

fn segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ba = b - a;
    let t = ((p - a).dot(ba) / ba.length_squared()).clamp(0.0, 1.0);
    (p - (a + ba * t)).length()
}

fn set_pixel(pixels: &mut [u8], width: u32, x: u32, y: u32, colour: [u8; 4]) {
    let index = ((y * width + x) * 4) as usize;
    pixels[index..index + 4].copy_from_slice(&colour);
}

fn blend(pixels: &mut [u8], width: u32, x: u32, y: u32, colour: [u8; 4], coverage: f32) {
    let index = ((y * width + x) * 4) as usize;
    let source_alpha = coverage * (colour[3] as f32 / 255.0);
    let destination_alpha = pixels[index + 3] as f32 / 255.0;
    let out_alpha = source_alpha + destination_alpha * (1.0 - source_alpha);
    if out_alpha <= f32::EPSILON {
        return;
    }
    for channel in 0..3 {
        let source = colour[channel] as f32 / 255.0;
        let destination = pixels[index + channel] as f32 / 255.0;
        pixels[index + channel] = (((source * source_alpha
            + destination * destination_alpha * (1.0 - source_alpha))
            / out_alpha)
            * 255.0)
            .round() as u8;
    }
    pixels[index + 3] = (out_alpha * 255.0).round() as u8;
}

fn rgba_image(width: u32, height: u32, pixels: Vec<u8>) -> Image {
    let mut image = Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = bevy::image::ImageSampler::linear();
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn designation_uses_one_fixed_prototype_field_with_stacked_text() {
        let (one_line, one_rows) = designation_image("N1");
        let (two_lines, two_rows) = designation_image("47P3");
        assert_eq!(one_rows, 1);
        assert_eq!(two_rows, 2);
        assert_eq!(
            one_line.texture_descriptor.size, two_lines.texture_descriptor.size,
            "the physical plate does not grow with its text"
        );
        assert_eq!(designation_lines("47P3"), ["47", "P3"]);
        assert_eq!(designation_lines("3P1"), ["3", "P1"]);
        assert_eq!(designation_lines("20ZW70"), ["20", "ZW70"]);
        assert_eq!(designation_lines("P12"), ["P12"]);
    }

    #[test]
    fn single_indicators_accept_generic_and_value_specific_lamp_names() {
        assert_eq!(lamp_aliases("zs3", "4", 1), ["zs3_4", "zs3"]);
        assert_eq!(lamp_aliases("zs2", "K", 2), ["zs2_K"]);
    }

    #[test]
    fn zs103_keeps_the_six_diamonds_of_the_rulebook_board() {
        assert_eq!(ZS103_DIAMONDS, 6);
        let last = ZS103_FIRST_Y - (ZS103_DIAMONDS - 1) as f32 * ZS103_STEP_Y;
        let diamond_half_diagonal = ZS103_DIAMOND / 2.0_f32.sqrt();
        assert!(ZS103_FIRST_Y + diamond_half_diagonal < ZS103_HEIGHT * 0.5);
        assert!(last - diamond_half_diagonal > -ZS103_HEIGHT * 0.5);
    }
}
