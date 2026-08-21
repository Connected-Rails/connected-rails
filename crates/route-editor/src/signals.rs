//! Signals on the map as the run shows them (plan ch. 15.3).
//!
//! The assembly, the mounting and the lamp binding are the simulator's
//! (`world-render`); what the editor supplies is the state, because it runs no
//! interlocking: every signal stands at **stop** — the rule of its type matched
//! against the untouched situation, which is exactly what a line shows before
//! the first route is set.

use bevy::prelude::*;
use sim_core::interlock::{Aspect, SignalModel, SignalType, Situation};
use std::collections::BTreeMap;
use world_render::{SignalLamp, SignalLodNode, SignalView};

use crate::{Line, Origin};

/// Signal types of every installed mod (`mods/*/signals/*.ron`) — they name the
/// default model and the lamp image of an aspect.
#[derive(Resource, Default)]
pub struct SignalTypes {
    pub map: BTreeMap<String, SignalType>,
}

/// Signal models of every installed mod (`mods/*/signal_models/*.ron`).
#[derive(Resource, Default)]
pub struct SignalModelFiles {
    pub map: BTreeMap<String, SignalModel>,
}

/// Lamp image per signal, refreshed with every rebuild.
#[derive(Resource, Default)]
pub struct LampImages(pub Vec<Vec<String>>);

/// Spawns the line's signals with their models — called by the rebuild while
/// the world view is on. Returns the lamp image per signal.
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    assets: &AssetServer,
    line: &Line,
    types: &SignalTypes,
    files: &SignalModelFiles,
    origin: &Origin,
) -> (Vec<Option<SignalModel>>, Vec<Vec<String>>) {
    let mut models = Vec::with_capacity(line.source.signals.len());
    let mut lamps = Vec::with_capacity(line.source.signals.len());
    for source in &line.source.signals {
        let ty = source
            .signal_type
            .as_deref()
            .and_then(|name| types.map.get(name));
        // The placement's override wins over the type's default, as in the run.
        let name = source
            .model
            .as_deref()
            .or_else(|| ty.and_then(|t| t.model.as_deref()));
        models.push(name.and_then(|n| files.map.get(n).cloned()));
        // At rest: the first rule that matches an untouched situation. A signal
        // without a type stays dark — its aspect is the built-in logic's, which
        // needs a running interlocking.
        lamps.push(
            ty.and_then(|t| {
                t.rules
                    .iter()
                    .find(|r| r.when.matches(&Situation::default()))
            })
            .map(|r| r.lamps.clone())
            .unwrap_or_default(),
        );
    }

    let views: Vec<SignalView> = line
        .source
        .signals
        .iter()
        .enumerate()
        .map(|(i, source)| SignalView {
            device: track_model::DeviceId(source.device),
            kind: source.kind,
            aspect: Aspect::stop(),
            model: models[i].as_ref(),
        })
        .collect();
    world_render::spawn_signals(
        commands, meshes, materials, assets, &line.net, &views, &origin.0,
    );
    drop(views);
    (models, lamps)
}

/// Lights the lamp nodes of the resting aspect.
pub fn light_lamps(images: Res<LampImages>, mut lamps: Query<(&SignalLamp, &mut Visibility)>) {
    for (lamp, mut visibility) in lamps.iter_mut() {
        let lit = images
            .0
            .get(lamp.signal)
            .is_some_and(|image| image.iter().any(|l| l == &lamp.lamp));
        // `set_if_neq`: writing the same value every frame marks every lamp
        // changed every frame, and the renderer re-extracts what changed.
        visibility.set_if_neq(if lit {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

/// The editor looks down from hundreds of metres, where the run would have
/// switched every signal off long ago — so it shows the finest level, always.
pub fn show_finest_lod(mut nodes: Query<(&SignalLodNode, &mut Visibility)>) {
    for (node, mut visibility) in nodes.iter_mut() {
        visibility.set_if_neq(if node.level == 0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}
