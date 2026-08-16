//! Vehicle models from mods: glTF scene, levels of detail, moving parts (plan ch. 15.3).
//!
//! The vehicle editor writes which glTF node is which level of detail and which node moves
//! how; here that is put to work. Nodes are found by **name**, exactly as the editor
//! records them — the model itself carries no simulator-specific data.

use crate::SimResource;
use crate::cab::{self, ControlMesh, ControlNode, Highlightable};
use bevy::gltf::GltfAssetLabel;
use bevy::picking::Pickable;
use bevy::prelude::*;
use render::VehicleView;
use sim_core::cab::CabInputs;
use sim_core::safety::LampState;
use sim_core::train::{Motion, Part, Vehicle};

use crate::render;

pub use world_render::asset_path;

/// Root of a spawned vehicle model.
#[derive(Component)]
pub struct ModelRoot {
    pub train: usize,
    pub vehicle: usize,
}

/// The descendants of this model have been bound to LODs and parts.
#[derive(Component)]
pub struct Bound;

/// A node belonging to a level of detail (`<name>_LOD<level>`).
#[derive(Component)]
pub struct LodNode {
    train: usize,
    vehicle: usize,
    level: u8,
}

/// A mesh below a part node that glows with the part's value ([`Motion::Emissive`]) —
/// the material is a clone of its own, so dimming one panel dims nothing else.
#[derive(Component)]
pub struct GlowMesh {
    /// The part node whose value drives the glow.
    node: Entity,
    /// Emissive colour as it came out of the file; the value scales it.
    base: LinearRgba,
}

/// A node moved by the simulation.
#[derive(Component)]
pub struct PartNode {
    train: usize,
    vehicle: usize,
    /// Index into `VehicleModel::parts`.
    part: usize,
    /// Transform as it comes out of the file — the motion is applied on top of it.
    base: Transform,
}

/// Spawns the glTF scene of a vehicle under its view entity.
pub fn spawn(
    commands: &mut Commands,
    assets: &AssetServer,
    entity: Entity,
    view: &VehicleView,
    file: &str,
) {
    let scene = assets.load(GltfAssetLabel::Scene(0).from_asset(asset_path(file)));
    commands.entity(entity).with_children(|parent| {
        parent.spawn((
            WorldAssetRoot(scene),
            Transform::default(),
            ModelRoot {
                train: view.train,
                vehicle: view.vehicle,
            },
        ));
    });
}

/// Binds LOD, part and cab control nodes once the scene has been spawned.
#[allow(clippy::too_many_arguments)]
pub fn bind_nodes(
    mut commands: Commands,
    sim: Res<SimResource>,
    roots: Query<(Entity, &ModelRoot), Without<Bound>>,
    children: Query<&Children>,
    named: Query<(&Name, &Transform)>,
    handles: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (root, model) in roots.iter() {
        let Some(spec) = sim
            .0
            .trains
            .get(model.train)
            .and_then(|t| t.vehicles.get(model.vehicle))
            .map(|v| &v.spec)
        else {
            continue;
        };
        let Some(description) = spec.model.as_ref() else {
            continue;
        };
        let controls = description
            .cab
            .as_ref()
            .map(|c| c.controls.as_slice())
            .unwrap_or_default();

        // Walk the whole subtree; the scene is only there a few frames after the spawn.
        let mut stack = vec![root];
        let mut found = false;
        let mut control_nodes = Vec::new();
        let mut glow_nodes = Vec::new();
        let (mut lods, mut parts) = (0, 0);
        while let Some(entity) = stack.pop() {
            if let Ok(kids) = children.get(entity) {
                stack.extend(kids.iter());
            }
            let Ok((name, transform)) = named.get(entity) else {
                continue;
            };
            found = true;
            if let Some(level) = lod_level(name.as_str()) {
                // A glTF node does not have to carry `Visibility` — without it the level
                // could not be switched.
                commands.entity(entity).insert((
                    LodNode {
                        train: model.train,
                        vehicle: model.vehicle,
                        level,
                    },
                    Visibility::Inherited,
                ));
                lods += 1;
            }
            if let Some(control) = controls.iter().position(|c| c.node == name.as_str()) {
                // A control node is driven by its bound input; a `parts` entry on the
                // same node would fight over the transform and is ignored.
                commands.entity(entity).insert((
                    ControlNode {
                        train: model.train,
                        vehicle: model.vehicle,
                        control,
                        base: *transform,
                    },
                    Visibility::Inherited,
                ));
                control_nodes.push(entity);
                continue;
            }
            if let Some(part) = description
                .parts
                .iter()
                .position(|p| p.node == name.as_str())
            {
                commands.entity(entity).insert((
                    PartNode {
                        train: model.train,
                        vehicle: model.vehicle,
                        part,
                        base: *transform,
                    },
                    Visibility::Inherited,
                ));
                // The children of a digit node ("0" … "9") are switched by
                // `animate_digits` — like their parent they come out of the file
                // without a `Visibility` to switch.
                if description.parts[part].function.starts_with("digit:")
                    && let Ok(kids) = children.get(entity)
                {
                    for kid in kids.iter() {
                        commands.entity(kid).insert(Visibility::Inherited);
                    }
                }
                if description.parts[part].motion == Motion::Emissive {
                    glow_nodes.push(entity);
                }
                parts += 1;
            }
        }
        // Every mesh below a control node becomes pickable, with its own material
        // clone so the hover glow lights only this control.
        for node in &control_nodes {
            let mut stack = vec![*node];
            while let Some(entity) = stack.pop() {
                if let Ok(kids) = children.get(entity) {
                    stack.extend(kids.iter());
                }
                let Ok(handle) = handles.get(entity) else {
                    continue;
                };
                let Some(material) = materials.get(&handle.0).cloned() else {
                    continue;
                };
                let emissive = material.emissive;
                commands
                    .entity(entity)
                    .insert((
                        Pickable::default(),
                        ControlMesh(*node),
                        Highlightable { emissive },
                        MeshMaterial3d(materials.add(material)),
                    ))
                    .observe(cab::on_over)
                    .observe(cab::on_out)
                    .observe(cab::on_press)
                    .observe(cab::on_drag_start)
                    .observe(cab::on_drag)
                    .observe(cab::on_scroll);
            }
        }
        // A glowing part dims its own material, so every mesh below it takes a
        // clone of it and remembers what the file lit it with.
        for node in &glow_nodes {
            let mut stack = vec![*node];
            while let Some(entity) = stack.pop() {
                if let Ok(kids) = children.get(entity) {
                    stack.extend(kids.iter());
                }
                let Ok(handle) = handles.get(entity) else {
                    continue;
                };
                let Some(material) = materials.get(&handle.0).cloned() else {
                    continue;
                };
                let base = material.emissive;
                commands.entity(entity).insert((
                    GlowMesh { node: *node, base },
                    MeshMaterial3d(materials.add(material)),
                ));
            }
        }
        if found {
            commands.entity(root).insert(Bound);
            info!(
                "Model {} (train {}, vehicle {}): {lods} LOD nodes, {parts} of {} parts, {} of {} controls bound",
                description.file,
                model.train,
                model.vehicle,
                description.parts.len(),
                control_nodes.len(),
                controls.len()
            );
        }
    }
}

pub use sim_core::train::lod_level;

/// Shows exactly the level of detail whose distance the vehicle is within.
pub fn update_lod(
    sim: Res<SimResource>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    vehicles: Query<(&VehicleView, &GlobalTransform)>,
    mut nodes: Query<(&LodNode, &mut Visibility)>,
) {
    let Ok(eye) = camera.single() else {
        return;
    };
    let eye = eye.translation();
    for (node, mut visibility) in nodes.iter_mut() {
        let distance = vehicles
            .iter()
            .find(|(v, _)| v.train == node.train && v.vehicle == node.vehicle)
            .map(|(_, t)| eye.distance(t.translation()))
            .unwrap_or(0.0);
        let lods = sim
            .0
            .trains
            .get(node.train)
            .and_then(|t| t.vehicles.get(node.vehicle))
            .and_then(|v| v.spec.model.as_ref())
            .map(|m| m.lods.as_slice())
            .unwrap_or_default();
        // The first level whose distance is not yet exceeded wins; beyond the last one
        // everything stays hidden.
        let wanted = lods
            .iter()
            .find(|l| distance as f64 <= l.distance)
            .map(|l| l.level);
        *visibility = if Some(node.level) == wanted {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

/// Transform a [`Motion`] produces at `value` (0 … 1); identity for visibility.
pub fn motion_transform(motion: &Motion, value: f32) -> Transform {
    match *motion {
        Motion::Visibility | Motion::Emissive => Transform::IDENTITY,
        Motion::Rotate { axis, degrees } => Transform::from_rotation(Quat::from_axis_angle(
            Vec3::from(axis).normalize_or_zero(),
            (degrees * value).to_radians(),
        )),
        Motion::Translate { axis, metres } => {
            Transform::from_translation(Vec3::from(axis) * metres * value)
        }
    }
}

fn apply_motion(
    motion: &Motion,
    base: &Transform,
    value: f32,
    transform: &mut Transform,
    visibility: &mut Visibility,
) {
    match motion {
        Motion::Visibility => {
            *visibility = if value >= 0.5 {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
        // A glowing part does not move; `animate_backlight` dims its material.
        Motion::Emissive => {}
        _ => *transform = *base * motion_transform(motion, value),
    }
}

/// The part a bound node belongs to and its current value, 0 … 1.
fn part_of<'a>(sim: &'a SimResource, node: &PartNode) -> Option<(&'a Part, f32)> {
    let vehicle = sim.0.trains.get(node.train)?.vehicles.get(node.vehicle)?;
    let part = vehicle.spec.model.as_ref()?.parts.get(node.part)?;
    let cab = sim.0.controls.get(node.train).copied().unwrap_or_default();
    let value = part_value(&part.function, vehicle, &cab, sim.0.time)?;
    Some((part, value))
}

/// Moves the bound parts according to the simulation state.
pub fn animate_parts(
    sim: Res<SimResource>,
    mut nodes: Query<(&PartNode, &mut Transform, &mut Visibility)>,
) {
    for (node, mut transform, mut visibility) in nodes.iter_mut() {
        let Some((part, value)) = part_of(&sim, node) else {
            continue;
        };
        apply_motion(
            &part.motion,
            &node.base,
            value,
            &mut transform,
            &mut visibility,
        );
    }
}

/// Dims the glowing parts ([`Motion::Emissive`]): the emissive colour of the
/// file scaled by the part's value, so instrument backlighting follows its
/// dimmer over the whole travel instead of switching on at half.
pub fn animate_backlight(
    sim: Res<SimResource>,
    nodes: Query<&PartNode>,
    glows: Query<(&GlowMesh, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (glow, handle) in &glows {
        let Ok(node) = nodes.get(glow.node) else {
            continue;
        };
        let Some((_, value)) = part_of(&sim, node) else {
            continue;
        };
        let Some(mut material) = materials.get_mut(&handle.0) else {
            continue;
        };
        material.emissive = glow.base * value.clamp(0.0, 1.0);
    }
}

/// Moves the cab controls to the value of their bound input — a lever follows
/// the keyboard and the AFB exactly as it follows the mouse.
pub fn animate_controls(
    sim: Res<SimResource>,
    mut nodes: Query<(&ControlNode, &mut Transform, &mut Visibility)>,
) {
    for (node, mut transform, mut visibility) in nodes.iter_mut() {
        let Some(train) = sim.0.trains.get(node.train) else {
            continue;
        };
        let Some(spec) = train
            .vehicles
            .get(node.vehicle)
            .and_then(|v| v.spec.model.as_ref())
            .and_then(|m| m.cab.as_ref())
            .and_then(|c| c.controls.get(node.control))
        else {
            continue;
        };
        let Some(cab) = sim.0.controls.get(node.train) else {
            continue;
        };
        let value = spec.input.get(train, cab) as f32;
        apply_motion(
            &spec.motion,
            &node.base,
            value,
            &mut transform,
            &mut visibility,
        );
    }
}

/// Value of a part function, 0 … 1 (or an angle fraction for gauges).
///
/// `time` is the simulation clock — lamps blink and wipers sweep with it, not
/// with the render clock, so replays look the same. `digit:` functions return
/// `None`; their children are switched by [`animate_digits`] instead.
///
/// ponytail: only the functions for which the simulation actually has state.
/// Destination displays need state that `sim-core` does not model yet — they
/// stay at their rest position instead of being faked here.
fn part_value(function: &str, vehicle: &Vehicle, cab: &CabInputs, time: f64) -> Option<f32> {
    let blink = time % 1.0 < 0.5;
    let value = match function {
        "pantograph" => vehicle.traction.pantograph,
        "door_left" => vehicle.doors.left.travel,
        "door_right" => vehicle.doors.right.travel,
        "gauge:speed" => {
            let v_max = if vehicle.spec.v_max > 0.0 {
                vehicle.spec.v_max
            } else {
                160.0
            };
            (vehicle.v.abs() * 3.6 / v_max).clamp(0.0, 1.0)
        }
        "gauge:brake_pipe" => (vehicle.brake.pipe / 6.0).clamp(0.0, 1.0),
        "gauge:cylinder" => (vehicle.brake.cylinder / 6.0).clamp(0.0, 1.0),
        "gauge:main_reservoir" => (vehicle.brake.main_reservoir / 12.0).clamp(0.0, 1.0),
        "gauge:tractive_effort" => (vehicle.tractive_effort / 400_000.0).clamp(-1.0, 1.0),
        "switch:throttle" => cab.throttle,
        "switch:reverser" => cab.reverser as f64,
        "switch:direct_brake" => cab.direct_brake,
        "switch:cab_light" => f64::from(cab.cab_light),
        // Instrument backlighting: an emissive panel node on `Motion::Emissive`
        // glows with the dimmer, so the dials read after dark (M6 polish).
        "switch:instrument_light" => cab.instrument_light,
        "wiper" => wiper_position(cab.wipers, time),
        "lamp:main_switch" => f64::from(vehicle.traction.main_switch),
        "lamp:sanding" => f64::from(vehicle.sanding),
        // Any indicator of the fitted train protection, by prefix: `lamp:<name>`
        // (`lamp:pzb_1000hz`, `lamp:sifa`, … — the names the HUD already prints)
        // follows the lamp state, `gauge:<name>` the numeric value of an MFA
        // pointer (`gauge:mfa_v_soll`, `gauge:mfa_zielentfernung`, …). An absent
        // indicator leaves the part at its rest position.
        _ => {
            if let Some(name) = function.strip_prefix("gauge:") {
                let value = vehicle
                    .safety
                    .indicators()
                    .iter()
                    .find(|i| i.name == name)?
                    .value?;
                match name {
                    // Target speeds share the speedometer scale.
                    "mfa_v_soll" | "mfa_v_ziel" => {
                        let v_max = if vehicle.spec.v_max > 0.0 {
                            vehicle.spec.v_max
                        } else {
                            160.0
                        };
                        (value / v_max).clamp(0.0, 1.0)
                    }
                    // LZB target distance over the full 4000 m of its bar column.
                    "mfa_zielentfernung" => (value / 4000.0).clamp(0.0, 1.0),
                    _ => value.clamp(0.0, 1.0),
                }
            } else {
                let name = function.strip_prefix("lamp:")?;
                let lamp = vehicle
                    .safety
                    .indicators()
                    .iter()
                    .find(|i| i.name == name)?
                    .lamp;
                match lamp {
                    LampState::Off => 0.0,
                    LampState::On => 1.0,
                    LampState::Blinking => f64::from(blink),
                }
            }
        }
    };
    Some(value as f32)
}

/// Wiper travel 0 … 1: a triangle sweep (0 → 1 → 0) driven by the simulation
/// clock. Interval mode does one sweep at the start of a 5 s period and parks
/// for the rest of it; slow and fast sweep continuously.
fn wiper_position(mode: u8, time: f64) -> f64 {
    let triangle = |phase: f64| 1.0 - (2.0 * phase.fract() - 1.0).abs();
    match mode {
        0 => 0.0,
        // 0.2 Hz over the whole period, sweeping only in its first third.
        1 => {
            let phase = (time * 0.2).fract();
            if phase < 1.0 / 3.0 {
                triangle(phase * 3.0)
            } else {
                0.0
            }
        }
        2 => triangle(time * 0.45),
        _ => triangle(time * 0.8),
    }
}

/// `digit:<indicator>:<place>` → indicator name and decimal place (0 = ones).
fn digit_function(function: &str) -> Option<(&str, u32)> {
    let rest = function.strip_prefix("digit:")?;
    let (name, place) = rest.rsplit_once(':')?;
    Some((name, place.parse().ok()?))
}

/// Digit a `digit:` node shows at `place`, or `None` when every child stays
/// hidden: indicator absent (dark display) or a blanked leading zero.
fn digit_at(value: Option<f64>, place: u32) -> Option<u32> {
    let whole = value?.max(0.0) as u64;
    let scale = 10u64.checked_pow(place)?;
    if place > 0 && whole < scale {
        return None;
    }
    Some(((whole / scale) % 10) as u32)
}

/// Shows exactly the child named after the current digit of a
/// `digit:<indicator>:<place>` part and hides its nine siblings — the model
/// simply carries ten meshes "0" … "9" under the bound node.
pub fn animate_digits(
    sim: Res<SimResource>,
    nodes: Query<(&PartNode, &Children)>,
    mut digits: Query<(&Name, &mut Visibility)>,
) {
    for (node, kids) in nodes.iter() {
        let Some(vehicle) = sim
            .0
            .trains
            .get(node.train)
            .and_then(|t| t.vehicles.get(node.vehicle))
        else {
            continue;
        };
        let Some(part) = vehicle
            .spec
            .model
            .as_ref()
            .and_then(|m| m.parts.get(node.part))
        else {
            continue;
        };
        let Some((name, place)) = digit_function(&part.function) else {
            continue;
        };
        let value = vehicle
            .safety
            .indicators()
            .iter()
            .find(|i| i.name == name)
            .and_then(|i| i.value);
        let wanted = digit_at(value, place);
        for kid in kids.iter() {
            let Ok((child, mut visibility)) = digits.get_mut(kid) else {
                continue;
            };
            // Only the digit children are switched — a decimal point or a
            // frame mesh under the same node keeps its own visibility.
            let Ok(digit) = child.as_str().parse::<u32>() else {
                continue;
            };
            *visibility = if Some(digit) == wanted {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lod_levels_come_from_the_node_name() {
        assert_eq!(lod_level("body_LOD0"), Some(0));
        assert_eq!(lod_level("body"), None);
    }

    #[test]
    fn asset_paths_go_through_the_mod_source() {
        assert_eq!(
            asset_path("example/assets/br101.gltf"),
            "mods://example/assets/br101.gltf"
        );
    }

    #[test]
    fn the_wiper_parks_sweeps_and_rests() {
        // Off: parked, whatever the clock says.
        assert_eq!(wiper_position(0, 12.34), 0.0);
        // A sweep is a triangle: 0 at the ends, 1 in the middle.
        assert_eq!(wiper_position(3, 0.0), 0.0);
        assert!((wiper_position(3, 0.625) - 1.0).abs() < 1e-9); // half of a 1.25 s period
        // Interval mode: one sweep in the first third of 5 s, then parked.
        assert!((wiper_position(1, 5.0 / 6.0) - 1.0).abs() < 1e-9); // middle of the sweep
        assert_eq!(wiper_position(1, 3.0), 0.0);
        assert_eq!(wiper_position(1, 4.9), 0.0);
    }

    #[test]
    fn digits_come_from_their_decimal_place() {
        assert_eq!(
            digit_function("digit:lzb_v_soll:1"),
            Some(("lzb_v_soll", 1))
        );
        assert_eq!(digit_function("gauge:speed"), None);
        assert_eq!(digit_at(Some(123.7), 0), Some(3));
        assert_eq!(digit_at(Some(123.7), 1), Some(2));
        assert_eq!(digit_at(Some(123.7), 2), Some(1));
        // Leading zeros are blanked, but the ones digit always shows.
        assert_eq!(digit_at(Some(123.7), 3), None);
        assert_eq!(digit_at(Some(7.0), 1), None);
        assert_eq!(digit_at(Some(0.0), 0), Some(0));
        // No indicator (LZB not guiding): the whole display stays dark.
        assert_eq!(digit_at(None, 0), None);
    }
}
