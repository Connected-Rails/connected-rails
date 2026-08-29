//! Cab displays rendered to texture (plan ch. 12) — MFA, EBuLa and the like.
//!
//! Every display of the player train gets an offscreen `Camera2d` drawing into
//! an `Image`, which glows as the emissive texture of the display's glTF node.
//! Content, in order of precedence: an HTML page ([`DisplaySpec::html`],
//! rendered by the `html-display` engine) wins, then a script's draw list from
//! [`mod_runtime::ModRuntime::vehicle_display`], then the declarative widget
//! list of the [`DisplaySpec`] compiled to the same commands — and none of the
//! three means a dark screen. Draw entities live on a render layer of their
//! own per display, so neither the main camera nor the other displays ever see
//! them, and they are only rebuilt when the command list actually changes.
//!
//! Only the player train: nobody looks at the screens of an AI train, and
//! every camera here renders every frame.

use crate::models::{Bound, ModelRoot};
use crate::{Mods, PlayerTrain, SimResource};
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{RenderTarget, ScalingMode};
use bevy::image::Image;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::sprite::Anchor;
use bevy::text::FontSize;
use html_display::{HtmlGauge, PaintCmd, SimFrame};
use mod_runtime::display::{DrawCmd, TextAlign};
use sim_core::Sim;
use sim_core::cab::{CabInputs, DisplaySource, DisplaySpec, Widget};
use sim_core::safety::{Indicator, LampState};
use sim_core::sound::SoundState;
use sim_core::train::Train;
use std::collections::HashMap;

/// First render layer used for displays; the world keeps layer 0.
const FIRST_LAYER: usize = 8;

/// Interval between two ticks of the html gauges [s] (~30 Hz) — see
/// [`update_displays`] for why they are throttled at all.
const HTML_TICK: f32 = 1.0 / 30.0;

/// One cab screen of the player train: its render target, its camera and the
/// draw list it currently shows.
struct DisplayView {
    /// Vehicle within the player train.
    vehicle: usize,
    /// Index into `VehicleModel::displays`.
    display: usize,
    /// Render layer of the camera and its draw entities.
    layer: usize,
    /// The texture the camera renders and the display node's material shows.
    image: Handle<Image>,
    camera: Entity,
    /// Commands drawn last — entities are only rebuilt when they change.
    last: Option<Vec<DrawCmd>>,
}

/// All display views of the player train.
#[derive(Resource)]
pub struct Displays(Vec<DisplayView>);

/// The loaded [`HtmlGauge`]s, keyed by display view index. A gauge holds a boa
/// script context and is `!Send`, so it cannot live inside [`Displays`]; a
/// non-send resource pins the systems that touch it to the main thread, where
/// the display chain runs anyway.
#[derive(Default)]
pub struct HtmlGauges(HashMap<usize, HtmlGauge>);

/// Tick throttle state of the html gauges, kept in a `Local` of
/// [`update_displays`].
#[derive(Default)]
pub struct HtmlThrottle {
    /// Time accumulated since the last html tick [s].
    since: f32,
    /// Softkey state at the last html tick, to catch edges between ticks.
    buttons: [bool; 8],
}

/// A draw entity of the display with this index; despawned on rebuild.
#[derive(Component, Clone, Copy)]
pub struct DisplayDraw(usize);

/// The display textures of this model have been hooked onto their nodes.
#[derive(Component)]
pub struct DisplaysBound;

/// Creates one render target and offscreen camera per display of the player
/// train.
///
/// Runs in `PostStartup` like the audio setup: the trains only exist once the
/// `Startup` commands are applied.
pub fn setup_displays(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    sim: Res<SimResource>,
    player: Res<PlayerTrain>,
    mut gauges: NonSendMut<HtmlGauges>,
) {
    let mut views = Vec::new();
    let vehicles = sim
        .0
        .trains
        .get(player.0)
        .map(|t| t.vehicles.as_slice())
        .unwrap_or_default();
    for (v, vehicle) in vehicles.iter().enumerate() {
        let Some(model) = vehicle.spec.model.as_ref() else {
            continue;
        };
        for (d, spec) in model.displays.iter().enumerate() {
            let index = views.len();
            let (width, height) = (spec.width.max(1), spec.height.max(1));
            // HTML content path: the page lives below `mods/`, next to the
            // other files of its mod. A page that does not load costs one
            // warning and the display falls back to script and widgets.
            if let Some(path) = spec.html.as_ref() {
                match std::fs::read_to_string(std::path::Path::new("mods").join(path))
                    .map_err(|e| e.to_string())
                    .and_then(|source| HtmlGauge::new(&source, width as f32, height as f32))
                {
                    Ok(gauge) => {
                        gauges.0.insert(index, gauge);
                    }
                    Err(message) => warn!("display {}: html {path}: {message}", spec.name),
                }
            }
            let image = images.add(Image::new_target_texture(
                width,
                height,
                TextureFormat::Rgba8UnormSrgb,
                None,
            ));
            let layer = FIRST_LAYER + index;
            let camera = commands
                .spawn((
                    Camera2d,
                    Camera {
                        // Before the main pass, so the texture is finished when
                        // the cab that shows it is drawn.
                        order: -1 - index as isize,
                        clear_color: ClearColorConfig::Custom(Color::BLACK),
                        ..default()
                    },
                    RenderTarget::Image(image.clone().into()),
                    // Fixed to the pixel size: draw coordinates are pixels.
                    Projection::Orthographic(OrthographicProjection {
                        scaling_mode: ScalingMode::Fixed {
                            width: width as f32,
                            height: height as f32,
                        },
                        ..OrthographicProjection::default_2d()
                    }),
                    RenderLayers::layer(layer),
                ))
                .id();
            views.push(DisplayView {
                vehicle: v,
                display: d,
                layer,
                image,
                camera,
                last: None,
            });
        }
    }
    if !views.is_empty() {
        info!(
            "Displays: {} render targets on the player train",
            views.len()
        );
    }
    commands.insert_resource(Displays(views));
}

/// Hooks the display textures onto their glTF nodes: every mesh below the node
/// named in the spec gets a material clone — the same pattern as the cab
/// control highlight — whose emissive is the render target, so the screen
/// glows readable in a dark cab.
///
/// Waits for [`Bound`], which `bind_nodes` only sets once the scene's named
/// nodes exist.
// A Bevy system takes its resources as parameters, and a query filter is one
// type — neither count says anything here.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn bind_display_nodes(
    mut commands: Commands,
    sim: Res<SimResource>,
    player: Res<PlayerTrain>,
    displays: Res<Displays>,
    roots: Query<(Entity, &ModelRoot), (With<Bound>, Without<DisplaysBound>)>,
    children: Query<&Children>,
    named: Query<&Name>,
    handles: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (root, model) in roots.iter() {
        commands.entity(root).insert(DisplaysBound);
        if model.train != player.0 {
            continue;
        }
        let Some(specs) = sim
            .0
            .trains
            .get(model.train)
            .and_then(|t| t.vehicles.get(model.vehicle))
            .and_then(|v| v.spec.model.as_ref())
            .map(|m| m.displays.as_slice())
        else {
            continue;
        };
        let mut stack = vec![root];
        while let Some(entity) = stack.pop() {
            if let Ok(kids) = children.get(entity) {
                stack.extend(kids.iter());
            }
            let Ok(name) = named.get(entity) else {
                continue;
            };
            let Some(view) = displays.0.iter().find(|view| {
                view.vehicle == model.vehicle
                    && specs
                        .get(view.display)
                        .is_some_and(|s| s.node == name.as_str())
            }) else {
                continue;
            };
            let mut meshes = vec![entity];
            while let Some(entity) = meshes.pop() {
                if let Ok(kids) = children.get(entity) {
                    meshes.extend(kids.iter());
                }
                let Ok(handle) = handles.get(entity) else {
                    continue;
                };
                let Some(mut material) = materials.get(&handle.0).cloned() else {
                    continue;
                };
                // Emissive, not base color: the screen is a light source and
                // stays legible however dark the cab is lit.
                material.base_color = Color::BLACK;
                material.emissive = LinearRgba::WHITE;
                material.emissive_texture = Some(view.image.clone());
                commands
                    .entity(entity)
                    .insert(MeshMaterial3d(materials.add(material)));
            }
        }
    }
}

/// Redraws every display whose content changed. Sits in the frame chain right
/// after `run_mod_scripts`, so a script's `display(ctx)` answer is from the
/// state of this very frame.
// A Bevy system takes its resources as parameters — the count says nothing here.
#[allow(clippy::too_many_arguments)]
pub fn update_displays(
    mut commands: Commands,
    sim: Res<SimResource>,
    player: Res<PlayerTrain>,
    mut mods: ResMut<Mods>,
    mut displays: ResMut<Displays>,
    mut cameras: Query<&mut Camera>,
    draws: Query<(Entity, &DisplayDraw)>,
    time: Res<Time>,
    mut gauges: NonSendMut<HtmlGauges>,
    mut throttle: Local<HtmlThrottle>,
) {
    let Some(train) = sim.0.trains.get(player.0) else {
        return;
    };
    let cab = sim.0.controls.get(player.0).copied().unwrap_or_default();
    let protection = sim
        .0
        .runtime
        .get(player.0)
        .map(|r| r.protection.clone())
        .unwrap_or_default();
    // The html gauges tick at ~30 Hz, not every frame: binding updates and the
    // page's `onFrame` script are real work per tick, and no screen content is
    // read faster than that. The softkeys are the exception — a menu that
    // answers a button press frames late feels broken, so any change of the
    // button state since the last tick forces an immediate one.
    throttle.since += time.delta_secs();
    let html_now = throttle.since >= HTML_TICK || cab.display_buttons != throttle.buttons;
    if html_now {
        throttle.since = 0.0;
        throttle.buttons = cab.display_buttons;
    }
    // One frame for all html displays: they all read the head vehicle, exactly
    // like the Lua `display(ctx)` hook.
    let frame = (html_now && !gauges.0.is_empty()).then(|| sim_frame(&sim.0, train, &cab));
    // One reading per vehicle per frame; every widget of it draws from the
    // same numbers. `AirFlow` needs a previous state and stays 0 — a pressure
    // gauge widget reads the pressures themselves.
    let mut sampled: HashMap<usize, (SoundState, Vec<Indicator>)> = HashMap::new();
    for (index, view) in displays.0.iter_mut().enumerate() {
        let Some(vehicle) = train.vehicles.get(view.vehicle) else {
            continue;
        };
        let Some(spec) = vehicle
            .spec
            .model
            .as_ref()
            .and_then(|m| m.displays.get(view.display))
        else {
            continue;
        };
        // Content precedence: the html page wins, then the script draw list,
        // then the widget list.
        if let Some(gauge) = gauges.0.get_mut(&index) {
            if let Some(frame) = frame.as_ref()
                && let Some(paint) = gauge.tick(frame)
            {
                // No `view.last` comparison on this path: the engine already
                // answers `None` for an unchanged picture, and between
                // throttled ticks the old entities must stay regardless of
                // what `last` holds — the cache check would defeat both.
                let cmds = paint.into_iter().map(draw_cmd).collect();
                rebuild(&mut commands, &draws, &mut cameras, view, index, spec, cmds);
            }
            // The engine reports each error once — no seen-set needed here.
            for message in gauge.take_errors() {
                warn!("display {}: {message}", spec.name);
            }
            continue;
        }
        // The script draw list wins; without one the widget list draws itself.
        let cmds = match mods.0.vehicle_display(&sim.0, player.0, spec) {
            Some(cmds) => cmds,
            None => {
                let (state, indicators) = sampled.entry(view.vehicle).or_insert_with(|| {
                    (
                        SoundState::sample(vehicle, &cab, &protection, None, 0.0),
                        vehicle.safety.indicators(),
                    )
                });
                widget_cmds(spec, state, indicators)
            }
        };
        if view.last.as_ref() == Some(&cmds) {
            continue;
        }
        rebuild(&mut commands, &draws, &mut cameras, view, index, spec, cmds);
    }
}

/// Replaces the draw entities of one display with a new command list and
/// remembers it in `view.last`.
fn rebuild(
    commands: &mut Commands,
    draws: &Query<(Entity, &DisplayDraw)>,
    cameras: &mut Query<&mut Camera>,
    view: &mut DisplayView,
    index: usize,
    spec: &DisplaySpec,
    cmds: Vec<DrawCmd>,
) {
    let Ok(mut camera) = cameras.get_mut(view.camera) else {
        return;
    };
    // Rebuild from scratch — the lists are small and change rarely enough
    // (a value ticking once a second) that diffing would not pay for itself.
    for (entity, draw) in draws.iter() {
        if draw.0 == index {
            commands.entity(entity).despawn();
        }
    }
    spawn_cmds(commands, &cmds, view.layer, index, spec, &mut camera);
    view.last = Some(cmds);
}

/// One reading of the simulation for an html gauge — the same values, under
/// the same names, that the Lua `display(ctx)` hook gets, so a page and a
/// script can be swapped for one another without renaming any binding.
fn sim_frame(sim: &Sim, train: &Train, cab: &CabInputs) -> SimFrame {
    let Some(head) = train.vehicles.first() else {
        return SimFrame::default();
    };
    let mut numbers: Vec<(String, f64)> = vec![
        ("v_kmh".into(), train.speed_kmh()),
        ("speed_limit_kmh".into(), head.pos.speed_limit(&sim.net)),
        ("throttle".into(), cab.throttle),
        ("reverser".into(), f64::from(cab.reverser)),
        ("afb".into(), f64::from(cab.afb)),
        ("afb_target".into(), cab.afb_target),
        ("brake_pipe".into(), head.brake.pipe),
        ("brake_cylinder".into(), head.brake.cylinder),
        ("main_reservoir".into(), head.brake.main_reservoir),
        ("line_voltage".into(), head.traction.line_voltage),
        ("tractive_effort".into(), head.tractive_effort),
    ];
    let mut lamps = Vec::new();
    for indicator in head.safety.indicators() {
        match indicator.value {
            Some(v) => numbers.push((format!("value.{}", indicator.name), v)),
            // The engine prefixes `lamp.` itself.
            None => lamps.push((indicator.name.to_string(), indicator.lamp != LampState::Off)),
        }
    }
    SimFrame {
        // Wall clock: a display clock shows the time of day; blink phases only care
        // about the fractional part and survive the offset.
        time: sim.clock(),
        numbers,
        lamps,
        buttons: cab.display_buttons,
    }
}

/// One paint command of the html engine as the draw command the display
/// pipeline already renders — the two lists are deliberately congruent.
fn draw_cmd(cmd: PaintCmd) -> DrawCmd {
    match cmd {
        PaintCmd::Clear { color } => DrawCmd::Clear { color },
        PaintCmd::Rect {
            x,
            y,
            w,
            h,
            color,
            filled,
        } => DrawCmd::Rect {
            x,
            y,
            w,
            h,
            color,
            filled,
        },
        PaintCmd::Text {
            x,
            y,
            text,
            size,
            color,
        } => DrawCmd::Text {
            x,
            y,
            text,
            size,
            color,
            // The html layout has already resolved `text-align` into `x`.
            align: TextAlign::Left,
        },
    }
}

/// Compiles the widget list of a display into draw commands — the code-free
/// content path, sharing the renderer with the script path.
fn widget_cmds(spec: &DisplaySpec, state: &SoundState, indicators: &[Indicator]) -> Vec<DrawCmd> {
    let value = |source: &DisplaySource| -> f64 {
        match source {
            DisplaySource::Quantity(q) => state.get(*q),
            DisplaySource::Indicator(name) => indicators
                .iter()
                .find(|i| i.name == name.as_str())
                .and_then(|i| i.value)
                .unwrap_or(0.0),
        }
    };
    let mut cmds = Vec::new();
    for widget in &spec.widgets {
        match widget {
            Widget::Label {
                x,
                y,
                size,
                text,
                color,
            } => cmds.push(DrawCmd::Text {
                x: *x,
                y: *y,
                text: text.clone(),
                size: *size,
                color: *color,
                align: TextAlign::Left,
            }),
            Widget::Value {
                x,
                y,
                size,
                source,
                decimals,
                unit,
                scale,
                color,
            } => {
                let mut text = format!("{:.*}", usize::from(*decimals), value(source) * scale);
                if !unit.is_empty() {
                    text.push(' ');
                    text.push_str(unit);
                }
                cmds.push(DrawCmd::Text {
                    x: *x,
                    y: *y,
                    text,
                    size: *size,
                    color: *color,
                    align: TextAlign::Left,
                });
            }
            Widget::Bar {
                x,
                y,
                w,
                h,
                source,
                max,
                color,
            } => {
                let fill = if *max > 0.0 {
                    (value(source) / max).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                cmds.push(DrawCmd::Rect {
                    x: *x,
                    y: *y,
                    w: w * fill as f32,
                    h: *h,
                    color: *color,
                    filled: true,
                });
                cmds.push(DrawCmd::Rect {
                    x: *x,
                    y: *y,
                    w: *w,
                    h: *h,
                    color: *color,
                    filled: false,
                });
            }
        }
    }
    cmds
}

/// Spawns the draw entities of one command list and applies its `Clear` to the
/// camera. Top-left y-down pixel coordinates become the centered y-up space of
/// the fixed orthographic projection here.
fn spawn_cmds(
    commands: &mut Commands,
    cmds: &[DrawCmd],
    layer: usize,
    index: usize,
    spec: &DisplaySpec,
    camera: &mut Camera,
) {
    let (width, height) = (spec.width.max(1) as f32, spec.height.max(1) as f32);
    let world = |x: f32, y: f32, z: f32| Vec3::new(x - width / 2.0, height / 2.0 - y, z);
    let layer = RenderLayers::layer(layer);
    let mark = DisplayDraw(index);
    // Without a `Clear` the background falls back to black.
    let mut clear = Color::BLACK;
    for (i, cmd) in cmds.iter().enumerate() {
        // Later commands draw on top — the painter's order a script assumes.
        let z = i as f32 * 0.01;
        match cmd {
            DrawCmd::Clear { color } => clear = tint(*color),
            DrawCmd::Rect {
                x,
                y,
                w,
                h,
                color,
                filled: true,
            } => {
                commands.spawn((
                    Sprite::from_color(tint(*color), Vec2::new(*w, *h)),
                    Transform::from_translation(world(x + w / 2.0, y + h / 2.0, z)),
                    layer.clone(),
                    mark,
                ));
            }
            DrawCmd::Rect {
                x,
                y,
                w,
                h,
                color,
                filled: false,
            } => {
                // Four 1 px edges — enough for widget frames and menu boxes.
                let edges = [
                    (x + w / 2.0, y + 0.5, *w, 1.0),
                    (x + w / 2.0, y + h - 0.5, *w, 1.0),
                    (x + 0.5, y + h / 2.0, 1.0, *h),
                    (x + w - 0.5, y + h / 2.0, 1.0, *h),
                ];
                for (cx, cy, ew, eh) in edges {
                    commands.spawn((
                        Sprite::from_color(tint(*color), Vec2::new(ew, eh)),
                        Transform::from_translation(world(cx, cy, z)),
                        layer.clone(),
                        mark,
                    ));
                }
            }
            DrawCmd::Line {
                x1,
                y1,
                x2,
                y2,
                width: w,
                color,
            } => {
                let length = Vec2::new(x2 - x1, y2 - y1).length();
                // Screen y grows downward, world y upward — the angle flips too.
                let angle = (y1 - y2).atan2(x2 - x1);
                commands.spawn((
                    Sprite::from_color(tint(*color), Vec2::new(length.max(1.0), w.max(1.0))),
                    Transform::from_translation(world((x1 + x2) / 2.0, (y1 + y2) / 2.0, z))
                        .with_rotation(Quat::from_rotation_z(angle)),
                    layer.clone(),
                    mark,
                ));
            }
            DrawCmd::Text {
                x,
                y,
                text,
                size,
                color,
                align,
            } => {
                let anchor = match align {
                    TextAlign::Left => Anchor::TOP_LEFT,
                    TextAlign::Center => Anchor::TOP_CENTER,
                    TextAlign::Right => Anchor::TOP_RIGHT,
                };
                commands.spawn((
                    Text2d::new(text.clone()),
                    TextFont {
                        font_size: FontSize::Px(*size),
                        ..default()
                    },
                    TextColor(tint(*color)),
                    anchor,
                    Transform::from_translation(world(*x, *y, z)),
                    layer.clone(),
                    mark,
                ));
            }
        }
    }
    camera.clear_color = ClearColorConfig::Custom(clear);
}

/// Linear RGBA of a draw command as a Bevy color.
fn tint(rgba: [f32; 4]) -> Color {
    Color::linear_rgba(rgba[0], rgba[1], rgba[2], rgba[3])
}
