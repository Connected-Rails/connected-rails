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

use crate::render::WorldAnchored;
use crate::{SimResource, models};
use bevy::gltf::GltfAssetLabel;
use bevy::prelude::*;
use sim_core::Sim;
use sim_core::interlock::{DistantAspect, MainAspect, SignalModel};
use sim_core::train::{Motion, lod_level};
use track_model::Facing;
use world_coords::{EcefPos, RenderOrigin};

/// 3D models of the line's signals, resolved at setup — index = signal index.
#[derive(Resource, Default)]
pub struct SignalModels(pub Vec<Option<SignalModel>>);

/// Root of one spawned part's glTF scene.
#[derive(Component)]
pub struct SignalPartRoot {
    pub signal: usize,
    pub part: usize,
}

/// The part still waits to be hung onto its mount node; hidden until then.
#[derive(Component)]
pub struct Unmounted {
    pub parent: u32,
    pub node: String,
}

/// The part's lamp, motion and LOD nodes have been bound.
#[derive(Component)]
pub struct LampsBound;

/// A lamp node: visible while `lamp` is in the signal's current lamp image.
#[derive(Component)]
pub struct SignalLamp {
    pub signal: usize,
    pub lamp: String,
}

/// A moving node (semaphore arm): travels towards 1 while its string is in the
/// signal's lamp image, back to 0 without it.
#[derive(Component)]
pub struct MotionNode {
    signal: usize,
    /// Index into `SignalModel::motions`.
    motion: usize,
    /// Transform as it comes out of the file — the motion is applied on top.
    base: Transform,
    /// Current travel 0 … 1.
    value: f32,
}

/// A node of one level of detail (`<name>_LOD<level>`).
#[derive(Component)]
pub struct SignalLodNode {
    signal: usize,
    level: u8,
}

/// Placeholder light whose material follows the aspect.
#[derive(Component)]
pub struct PlaceholderHead {
    pub signal: usize,
}

/// Materials of the placeholder light, one per shown colour.
#[derive(Resource)]
pub struct AspectMaterials {
    off: Handle<StandardMaterial>,
    red: Handle<StandardMaterial>,
    green: Handle<StandardMaterial>,
    yellow: Handle<StandardMaterial>,
    white: Handle<StandardMaterial>,
}

impl AspectMaterials {
    pub fn new(materials: &mut Assets<StandardMaterial>) -> Self {
        let lamp = |materials: &mut Assets<StandardMaterial>, colour: Color, lit: bool| {
            materials.add(StandardMaterial {
                base_color: colour,
                emissive: if lit {
                    colour.to_linear() * 4.0
                } else {
                    LinearRgba::BLACK
                },
                perceptual_roughness: 0.4,
                ..default()
            })
        };
        Self {
            off: lamp(materials, Color::srgb(0.12, 0.12, 0.12), false),
            red: lamp(materials, Color::srgb(0.9, 0.1, 0.1), true),
            green: lamp(materials, Color::srgb(0.1, 0.85, 0.3), true),
            yellow: lamp(materials, Color::srgb(0.95, 0.75, 0.1), true),
            white: lamp(materials, Color::srgb(0.95, 0.95, 0.9), true),
        }
    }

    /// Placeholder colour of an aspect: the main aspect first, a pure distant
    /// signal shows what it announces.
    fn handle(&self, aspect: &sim_core::interlock::Aspect) -> Handle<StandardMaterial> {
        match aspect.main {
            Some(MainAspect::Stop) => self.red.clone(),
            Some(MainAspect::Proceed) => self.green.clone(),
            Some(MainAspect::ProceedSlow) => self.yellow.clone(),
            Some(MainAspect::Substitute) => self.white.clone(),
            Some(MainAspect::DarkLight) => self.off.clone(),
            None => match aspect.distant {
                Some(DistantAspect::ExpectProceed) => self.green.clone(),
                Some(DistantAspect::ExpectStop) | Some(DistantAspect::ExpectSlow) => {
                    self.yellow.clone()
                }
                None => self.off.clone(),
            },
        }
    }
}

/// Spawns every signal of the line: its resolved model, or the placeholder.
pub fn spawn_signals(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    assets: &AssetServer,
    sim: &Sim,
    origin: &RenderOrigin,
    signal_models: &[Option<SignalModel>],
) {
    let aspect_materials = AspectMaterials::new(materials);
    let mast_mesh = meshes.add(Cuboid::new(0.15, 4.0, 0.15));
    let head_mesh = meshes.add(Sphere::new(0.25));
    let mast_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.55, 0.56, 0.58),
        perceptual_roughness: 0.7,
        ..default()
    });

    for (i, signal) in sim.interlock.signals.iter().enumerate() {
        let device = sim.net.device(signal.device);
        let pose = sim.net.edge(device.edge).eval(device.s);
        // The signal faces the trains its device applies to; `Both` picks one side.
        let dir = match device.facing {
            Facing::Backward => -pose.tangent,
            _ => pose.tangent,
        };
        // Positive offset = left of the direction of travel (device.rs).
        let left = pose.up.cross(dir).normalize();
        let anchor = EcefPos(pose.pos.0 + left * device.lateral_offset);

        // The geometry lives in the ENU frame of the signal itself: on an origin
        // rebase `resync_anchored` resets the frame transform, the local rotation
        // (mast plumb, front towards the approaching driver, +Z in the model)
        // survives as the child's transform.
        let local = RenderOrigin::new(anchor);
        let rotation = local.look_rotation(dir, local.frame().up);
        let (translation, frame_rotation) = origin.frame_transform(local.frame());

        let root = commands
            .spawn((
                Transform::from_translation(translation).with_rotation(frame_rotation),
                Visibility::default(),
                WorldAnchored { anchor },
            ))
            .id();
        let view = commands
            .spawn((
                Transform::from_rotation(rotation),
                Visibility::default(),
                ChildOf(root),
            ))
            .id();

        match signal_models.get(i).and_then(|m| m.as_ref()) {
            Some(model) => {
                for (p, part) in model.parts.iter().enumerate() {
                    let scene = assets
                        .load(GltfAssetLabel::Scene(0).from_asset(models::asset_path(&part.file)));
                    let mut entity = commands.spawn((
                        WorldAssetRoot(scene),
                        Transform::default(),
                        SignalPartRoot { signal: i, part: p },
                        ChildOf(view),
                    ));
                    match &part.mount {
                        // A cyclic mount chain (hand-written file) can never
                        // resolve — the part stands at the signal foot instead.
                        Some(_) if mounts_cyclically(model, p) => {
                            warn!("signal {i}: part {p} mounts in a cycle — placed at the root");
                            entity.insert(Visibility::default());
                        }
                        // Hidden until it hangs on its mount node — a mounted part
                        // must not flash at the signal foot while the parent loads.
                        Some((parent, node)) => {
                            entity.insert((
                                Visibility::Hidden,
                                Unmounted {
                                    parent: *parent,
                                    node: node.clone(),
                                },
                            ));
                        }
                        None => {
                            entity.insert(Visibility::default());
                        }
                    }
                }
            }
            None => {
                commands.spawn((
                    Mesh3d(mast_mesh.clone()),
                    MeshMaterial3d(mast_material.clone()),
                    Transform::from_xyz(0.0, 2.0, 0.0),
                    ChildOf(view),
                ));
                commands.spawn((
                    Mesh3d(head_mesh.clone()),
                    MeshMaterial3d(aspect_materials.handle(&signal.aspect)),
                    Transform::from_xyz(0.0, 4.3, 0.0),
                    PlaceholderHead { signal: i },
                    ChildOf(view),
                ));
            }
        }
    }

    commands.insert_resource(aspect_materials);
}

/// Does `part`'s mount chain loop back on itself? More hops than parts is a cycle.
fn mounts_cyclically(model: &SignalModel, part: usize) -> bool {
    let mut current = part;
    for _ in 0..model.parts.len() {
        match model.parts.get(current).and_then(|p| p.mount.as_ref()) {
            Some((next, _)) => current = *next as usize,
            None => return false,
        }
    }
    true
}

/// Hangs waiting parts onto their mount nodes once the parent part's scene exists.
pub fn mount_parts(
    mut commands: Commands,
    unmounted: Query<(Entity, &SignalPartRoot, &Unmounted)>,
    parts: Query<(Entity, &SignalPartRoot)>,
    children: Query<&Children>,
    named: Query<&Name>,
) {
    for (entity, part, mount) in unmounted.iter() {
        let Some((parent_root, _)) = parts
            .iter()
            .find(|(_, p)| p.signal == part.signal && p.part == mount.parent as usize)
        else {
            // Dangling part index — the editor validates, a hand-written file may not.
            warn!(
                "signal {}: part {} mounts on missing part {}",
                part.signal, part.part, mount.parent
            );
            commands.entity(entity).remove::<Unmounted>();
            continue;
        };
        // Walk the parent's subtree; the scene is only there a few frames after spawn.
        // Parts already mounted inside it belong to other files and are skipped.
        let mut stack = vec![parent_root];
        let mut target = None;
        let mut loaded = false;
        while let Some(e) = stack.pop() {
            if e != parent_root && parts.contains(e) {
                continue;
            }
            if let Ok(kids) = children.get(e) {
                stack.extend(kids.iter());
            }
            if let Ok(name) = named.get(e) {
                loaded = true;
                if name.as_str() == mount.node {
                    target = Some(e);
                    break;
                }
            }
        }
        match target {
            Some(node) => {
                commands
                    .entity(entity)
                    .insert((ChildOf(node), Visibility::Inherited))
                    .remove::<Unmounted>();
                info!(
                    "signal {}: part {} mounted on {:?} of part {}",
                    part.signal, part.part, mount.node, mount.parent
                );
            }
            None if loaded => {
                // The parent scene is there and the node is not in it: permanent.
                warn!(
                    "signal {}: mount node {:?} not found in part {}",
                    part.signal, mount.node, mount.parent
                );
                commands.entity(entity).remove::<Unmounted>();
            }
            None => {}
        }
    }
}

/// Binds the lamp, motion and LOD nodes of a part once its scene has been spawned.
pub fn bind_lamps(
    mut commands: Commands,
    models: Res<SignalModels>,
    roots: Query<(Entity, &SignalPartRoot), Without<LampsBound>>,
    all_parts: Query<(), With<SignalPartRoot>>,
    children: Query<&Children>,
    named: Query<(&Name, &Transform)>,
) {
    for (root, part) in roots.iter() {
        let Some(model) = models.0.get(part.signal).and_then(|m| m.as_ref()) else {
            continue;
        };
        let lamps: Vec<_> = model
            .lamps
            .iter()
            .filter(|l| l.part as usize == part.part)
            .collect();
        let motions: Vec<_> = model
            .motions
            .iter()
            .enumerate()
            .filter(|(_, m)| m.part as usize == part.part)
            .collect();
        if lamps.is_empty() && motions.is_empty() && model.lods.is_empty() {
            commands.entity(root).insert(LampsBound);
            continue;
        }
        let mut stack = vec![root];
        let mut found = false;
        let (mut lamps_bound, mut motions_bound) = (0, 0);
        while let Some(entity) = stack.pop() {
            // A part mounted inside this one binds its own lamps.
            if entity != root && all_parts.contains(entity) {
                continue;
            }
            if let Ok(kids) = children.get(entity) {
                stack.extend(kids.iter());
            }
            let Ok((name, transform)) = named.get(entity) else {
                continue;
            };
            found = true;
            // The LOD table spans all parts; nodes without the suffix are
            // every level's furniture and stay as they are.
            if !model.lods.is_empty()
                && let Some(level) = lod_level(name.as_str())
            {
                commands.entity(entity).insert((
                    SignalLodNode {
                        signal: part.signal,
                        level,
                    },
                    Visibility::Inherited,
                ));
            }
            if let Some((index, _)) = motions.iter().find(|(_, m)| m.node == name.as_str()) {
                commands.entity(entity).insert((
                    MotionNode {
                        signal: part.signal,
                        motion: *index,
                        base: *transform,
                        value: 0.0,
                    },
                    Visibility::Inherited,
                ));
                motions_bound += 1;
            }
            if let Some(binding) = lamps.iter().find(|l| l.node == name.as_str()) {
                // Dark until the first update — and a glTF node does not have to
                // carry `Visibility`, without one it could not be switched.
                commands.entity(entity).insert((
                    SignalLamp {
                        signal: part.signal,
                        lamp: binding.lamp.clone(),
                    },
                    Visibility::Hidden,
                ));
                lamps_bound += 1;
            }
        }
        if found {
            commands.entity(root).insert(LampsBound);
            info!(
                "signal {}: part {}: {lamps_bound} of {} lamp nodes, {motions_bound} of {} motion nodes bound",
                part.signal,
                part.part,
                lamps.len(),
                motions.len()
            );
        }
    }
}

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
        let wanted = materials.handle(&signal.aspect);
        if material.0 != wanted {
            material.0 = wanted;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::interlock::SignalPart;

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

    #[test]
    fn a_mount_cycle_is_detected() {
        let part = |mount| SignalPart {
            file: "a.gltf".into(),
            mount,
        };
        let chain = SignalModel {
            parts: vec![
                part(None),
                part(Some((0, "mp".into()))),
                part(Some((1, "mp".into()))),
            ],
            ..Default::default()
        };
        assert!(!mounts_cyclically(&chain, 2));
        let cycle = SignalModel {
            parts: vec![part(Some((1, "mp".into()))), part(Some((0, "mp".into())))],
            ..Default::default()
        };
        assert!(mounts_cyclically(&cycle, 0));
    }
}
