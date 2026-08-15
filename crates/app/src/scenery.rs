//! Scenery objects with a track reference (plan ch. 15): a mod's glTF placed
//! at the offset, rotation and height its placement stores — the same
//! root/view entity pattern as the signal models, so the local rotation
//! survives an origin rebase.

use crate::models;
use crate::render::WorldAnchored;
use bevy::gltf::GltfAssetLabel;
use bevy::prelude::*;
use content::LineSource;
use glam::DQuat;
use std::collections::BTreeMap;
use track_model::TrackNetwork;
use track_model::TrackObject;
use world_coords::{EcefPos, RenderOrigin};

/// Spawns every scenery object of the line. An unknown object kind gets a
/// placeholder block — visible in the world instead of silently absent.
#[allow(clippy::too_many_arguments)]
pub fn spawn_objects(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    assets: &AssetServer,
    line: &LineSource,
    net: &TrackNetwork,
    origin: &RenderOrigin,
    registry: &BTreeMap<String, TrackObject>,
) {
    let placeholder_mesh = meshes.add(Cuboid::new(0.8, 2.0, 0.8));
    let placeholder_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.75, 0.30, 0.65),
        ..default()
    });

    for (i, placement) in line.objects.iter().enumerate() {
        // Compile refused dangling indices; a guard keeps a stale file harmless.
        let Some(edge) = net.edges().get(placement.edge as usize) else {
            continue;
        };
        let pose = edge.eval(placement.s.clamp(0.0, edge.length()));
        // Positive offset = right of increasing arc length.
        let right = pose.tangent.cross(pose.up).normalize();
        let anchor =
            EcefPos(pose.pos.0 + right * placement.lateral_offset + pose.up * placement.height);
        // Yaw is clockwise seen from above; 0 = front along increasing s.
        let dir = DQuat::from_axis_angle(pose.up, -placement.yaw_deg.to_radians()) * pose.tangent;

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

        match registry.get(&placement.object) {
            Some(object) => {
                let scene = assets
                    .load(GltfAssetLabel::Scene(0).from_asset(models::asset_path(&object.model)));
                commands.spawn((WorldAssetRoot(scene), Transform::default(), ChildOf(view)));
            }
            None => {
                warn!(
                    "object {i}: unknown object {:?} — placeholder shown",
                    placement.object
                );
                commands.spawn((
                    Mesh3d(placeholder_mesh.clone()),
                    MeshMaterial3d(placeholder_material.clone()),
                    Transform::from_xyz(0.0, 1.0, 0.0),
                    ChildOf(view),
                ));
            }
        }
    }
}
