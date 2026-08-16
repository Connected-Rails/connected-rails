//! Signal models from mods: glTF parts on mount points, lamp image → node visibility
//! (plan ch. 15.3 — the vehicle model path, applied to signals).
//!
//! A signal model is an assembly: shared glTF files (mast, screen, indicator) chained
//! by named mount-point nodes, the way Zusi links signal files. Each part's scene is
//! spawned flat under the signal and reparented onto its mount node once the parent
//! scene exists. Lamp nodes are found by **name** and shown while their lamp-image
//! string is in the signal's current lamp image.
//!
//! A signal without a model gets a placeholder: a mast and one light whose colour
//! follows the aspect — so every line shows its signals, modded or not.
//!
//! Spawning, mounting and binding live in `world-render`, because the route
//! editor puts the same assemblies on the map; what stays here is everything
//! that follows the running interlocking: lamp image, arm travel, placeholder
//! colour.

use crate::{SimResource, models};
use bevy::prelude::*;
use sim_core::train::Motion;
use world_render::{
    AspectMaterials, MotionNode, PlaceholderHead, SignalLamp, SignalLodNode, SignalModels,
};

/// Moves the motion-bound nodes: linear travel towards their target — the
/// characteristic swing of a semaphore arm, intermediate positions included.
pub fn animate_motions(
    time: Res<Time>,
    sim: Res<SimResource>,
    models: Res<SignalModels>,
    mut nodes: Query<(&mut MotionNode, &mut Transform, &mut Visibility)>,
) {
    let dt = time.delta_secs();
    for (mut node, mut transform, mut visibility) in nodes.iter_mut() {
        let Some(binding) = models
            .0
            .get(node.signal)
            .and_then(|m| m.as_ref())
            .and_then(|m| m.motions.get(node.motion))
        else {
            continue;
        };
        let lit = sim
            .0
            .interlock
            .signals
            .get(node.signal)
            .is_some_and(|s| s.lamps.iter().any(|l| l == &binding.lamp));
        let target = if lit { 1.0 } else { 0.0 };
        node.value = slew(node.value, target, dt, binding.seconds as f32);
        match binding.motion {
            Motion::Visibility => {
                *visibility = if node.value >= 0.5 {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
            _ => *transform = node.base * models::motion_transform(&binding.motion, node.value),
        }
    }
}

/// One step of the travel towards `target`: linear, the full swing in `seconds`.
fn slew(value: f32, target: f32, dt: f32, seconds: f32) -> f32 {
    if seconds <= 0.0 {
        return target;
    }
    let step = dt / seconds;
    (value + (target - value).clamp(-step, step)).clamp(0.0, 1.0)
}

/// Shows exactly the level of detail whose distance the signal is within;
/// beyond the last one the LOD nodes disappear.
pub fn update_signal_lods(
    models: Res<SignalModels>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut nodes: Query<(&SignalLodNode, &GlobalTransform, &mut Visibility)>,
) {
    let Ok(eye) = camera.single() else {
        return;
    };
    let eye = eye.translation();
    for (node, transform, mut visibility) in nodes.iter_mut() {
        let Some(lods) = models
            .0
            .get(node.signal)
            .and_then(|m| m.as_ref())
            .map(|m| m.lods.as_slice())
        else {
            continue;
        };
        let distance = f64::from(eye.distance(transform.translation()));
        let wanted = lods
            .iter()
            .find(|l| distance <= l.distance)
            .map(|l| l.level);
        *visibility = if Some(node.level) == wanted {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

/// Shows exactly the lamp nodes whose string is in the signal's lamp image.
pub fn update_lamps(sim: Res<SimResource>, mut lamps: Query<(&SignalLamp, &mut Visibility)>) {
    for (lamp, mut visibility) in lamps.iter_mut() {
        let lit = sim
            .0
            .interlock
            .signals
            .get(lamp.signal)
            .is_some_and(|s| s.lamps.iter().any(|l| l == &lamp.lamp));
        *visibility = if lit {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

/// Colours the placeholder lights by their signal's aspect.
pub fn update_placeholders(
    sim: Res<SimResource>,
    materials: Res<AspectMaterials>,
    mut heads: Query<(&PlaceholderHead, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    for (head, mut material) in heads.iter_mut() {
        let Some(signal) = sim.0.interlock.signals.get(head.signal) else {
            continue;
        };
        let wanted = materials.of(&signal.aspect);
        if material.0 != wanted {
            material.0 = wanted;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_swing_is_linear_and_clamped() {
        // Half the travel time covers half the travel, whatever the start.
        assert_eq!(slew(0.0, 1.0, 0.75, 1.5), 0.5);
        assert_eq!(slew(1.0, 0.0, 0.75, 1.5), 0.5);
        // It stops at the target instead of overshooting.
        assert_eq!(slew(0.9, 1.0, 1.0, 1.5), 1.0);
        // No travel time: a hard switch.
        assert_eq!(slew(0.0, 1.0, 0.001, 0.0), 1.0);
    }
}
