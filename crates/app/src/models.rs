//! Vehicle models from mods: glTF scene, levels of detail, moving parts (plan ch. 15.3).
//!
//! The vehicle editor writes which glTF node is which level of detail and which node moves
//! how; here that is put to work. Nodes are found by **name**, exactly as the editor
//! records them — the model itself carries no simulator-specific data.

use crate::SimResource;
use bevy::gltf::GltfAssetLabel;
use bevy::prelude::*;
use render::VehicleView;
use sim_core::cab::CabInputs;
use sim_core::train::{Motion, Vehicle};

use crate::render;

/// Asset source of the mods, registered in `main`: `mods://<mod>/assets/…`.
pub const SOURCE: &str = "mods";

/// Full asset path of a model file stated relative to the `mods/` directory.
pub fn asset_path(file: &str) -> String {
    format!("{SOURCE}://{file}")
}

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

/// Binds LOD and part nodes once the scene has been spawned.
pub fn bind_nodes(
    mut commands: Commands,
    sim: Res<SimResource>,
    roots: Query<(Entity, &ModelRoot), Without<Bound>>,
    children: Query<&Children>,
    named: Query<(&Name, &Transform)>,
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

        // Walk the whole subtree; the scene is only there a few frames after the spawn.
        let mut stack = vec![root];
        let mut found = false;
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
                parts += 1;
            }
        }
        if found {
            commands.entity(root).insert(Bound);
            info!(
                "Model {} (train {}, vehicle {}): {lods} LOD nodes, {parts} of {} parts bound",
                description.file,
                model.train,
                model.vehicle,
                description.parts.len()
            );
        }
    }
}

/// `body_LOD2` → `Some(2)` — the same convention the vehicle editor writes.
fn lod_level(name: &str) -> Option<u8> {
    let (_, tail) = name.rsplit_once("_LOD")?;
    tail.parse().ok()
}

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

/// Moves the bound parts according to the simulation state.
pub fn animate_parts(
    sim: Res<SimResource>,
    mut nodes: Query<(&PartNode, &mut Transform, &mut Visibility)>,
) {
    for (node, mut transform, mut visibility) in nodes.iter_mut() {
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
        let cab = sim.0.controls.get(node.train).copied().unwrap_or_default();
        let Some(value) = part_value(&part.function, vehicle, &cab) else {
            continue;
        };
        match part.motion {
            Motion::Visibility => {
                *visibility = if value >= 0.5 {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
            Motion::Rotate { axis, degrees } => {
                let axis = Vec3::from(axis).normalize_or_zero();
                *transform = node.base
                    * Transform::from_rotation(Quat::from_axis_angle(
                        axis,
                        (degrees * value).to_radians(),
                    ));
            }
            Motion::Translate { axis, metres } => {
                *transform =
                    node.base * Transform::from_translation(Vec3::from(axis) * metres * value);
            }
        }
    }
}

/// Value of a part function, 0 … 1 (or an angle fraction for gauges).
///
/// ponytail: only the functions for which the simulation actually has state. Lamps and
/// destination displays need state that `sim-core` does not model yet — they stay at
/// their rest position instead of being faked here.
fn part_value(function: &str, vehicle: &Vehicle, cab: &CabInputs) -> Option<f32> {
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
        "lamp:main_switch" => f64::from(vehicle.traction.main_switch),
        "lamp:sanding" => f64::from(vehicle.sanding),
        _ => return None,
    };
    Some(value as f32)
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
}
