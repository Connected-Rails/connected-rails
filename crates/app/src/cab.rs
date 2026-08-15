//! Mouse interaction with the 3D cab (plan ch. 12).
//!
//! Control meshes carry per-entity picking observers; they only record what the
//! mouse does in [`CabMouse`]. [`apply_mouse`] then writes that into the
//! simulation right after the keyboard system, so keyboard and mouse operate
//! the same [`sim_core::cab::CabInputs`] without fighting each other.
//!
//! Gestures, in TSW fashion: push buttons hold while pressed, switches cycle on
//! press and step on scroll, levers/valves follow a drag along their on-screen
//! direction of travel and step on scroll.

use crate::{SimResource, models};
use bevy::input::mouse::MouseScrollUnit;
use bevy::picking::events::{Drag, DragStart, Out, Over, Pointer, Press, Scroll};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use sim_core::Sim;
use sim_core::cab::CabControlSpec;

/// A named glTF node bound to a cab control ([`sim_core::cab::CabControlSpec`]).
#[derive(Component)]
pub struct ControlNode {
    pub train: usize,
    pub vehicle: usize,
    /// Index into `CabSpec::controls`.
    pub control: usize,
    /// Transform as it comes out of the file — the motion is applied on top.
    pub base: Transform,
}

/// A pickable mesh below a control node; points at the [`ControlNode`] entity.
#[derive(Component)]
pub struct ControlMesh(pub Entity);

/// Rest emissive colour of a control mesh, to undo the hover highlight.
#[derive(Component)]
pub struct Highlightable {
    pub emissive: LinearRgba,
}

/// What the mouse is doing to the cab, applied to the sim once per frame.
#[derive(Resource, Default)]
pub struct CabMouse {
    /// Control node under the cursor.
    pub hover: Option<Entity>,
    /// i18n key and value of the hovered control, for the HUD.
    pub hover_info: Option<(&'static str, f64)>,
    /// Push button held down.
    held: Option<Entity>,
    drag: Option<DragState>,
    /// One-shot changes from clicks and scroll notches.
    queued: Vec<(Entity, Queued)>,
}

struct DragState {
    node: Entity,
    /// Value when the drag started.
    start: f64,
    /// Screen direction of one unit of travel [px] — dragging along it moves
    /// the control, dragging across it does nothing.
    gain: Vec2,
    /// Total drag distance so far [px].
    distance: Vec2,
}

enum Queued {
    /// Advance a switch to its next position (with wrap-around).
    Cycle,
    /// Move by this much travel (scroll), clamped instead of wrapping.
    Step(f64),
}

fn control_spec<'a>(sim: &'a Sim, node: &ControlNode) -> Option<&'a CabControlSpec> {
    sim.trains
        .get(node.train)?
        .vehicles
        .get(node.vehicle)?
        .spec
        .model
        .as_ref()?
        .cab
        .as_ref()?
        .controls
        .get(node.control)
}

pub fn on_over(on: On<Pointer<Over>>, meshes: Query<&ControlMesh>, mut mouse: ResMut<CabMouse>) {
    if let Ok(mesh) = meshes.get(on.event().entity) {
        mouse.hover = Some(mesh.0);
    }
}

pub fn on_out(on: On<Pointer<Out>>, meshes: Query<&ControlMesh>, mut mouse: ResMut<CabMouse>) {
    if let Ok(mesh) = meshes.get(on.event().entity)
        && mouse.hover == Some(mesh.0)
    {
        mouse.hover = None;
    }
}

pub fn on_press(
    on: On<Pointer<Press>>,
    meshes: Query<&ControlMesh>,
    nodes: Query<&ControlNode>,
    sim: Res<SimResource>,
    mut mouse: ResMut<CabMouse>,
) {
    if on.event().event.button != PointerButton::Primary {
        return;
    }
    let Ok(mesh) = meshes.get(on.event().entity) else {
        return;
    };
    let Some(spec) = nodes.get(mesh.0).ok().and_then(|n| control_spec(&sim.0, n)) else {
        return;
    };
    if spec.input.momentary() {
        mouse.held = Some(mesh.0);
    } else if spec.input.positions() > 0 {
        mouse.queued.push((mesh.0, Queued::Cycle));
    }
}

pub fn on_drag_start(
    on: On<Pointer<DragStart>>,
    meshes: Query<&ControlMesh>,
    nodes: Query<&ControlNode>,
    transforms: Query<(&GlobalTransform, &Transform)>,
    camera: Query<(&Camera, &GlobalTransform), With<crate::ui::CabCamera>>,
    sim: Res<SimResource>,
    mut mouse: ResMut<CabMouse>,
) {
    if on.event().event.button != PointerButton::Primary {
        return;
    }
    let Ok(mesh) = meshes.get(on.event().entity) else {
        return;
    };
    let Ok(node) = nodes.get(mesh.0) else {
        return;
    };
    let Some(spec) = control_spec(&sim.0, node) else {
        return;
    };
    if spec.input.momentary() {
        return;
    }
    let Some(cab) = sim.0.controls.get(node.train) else {
        return;
    };
    let start = spec.input.get(&sim.0.trains[node.train], cab);
    let gain = on
        .event()
        .event
        .hit
        .position
        .and_then(|hit| {
            let camera = camera.single().ok()?;
            let (global, local) = transforms.get(mesh.0).ok()?;
            drag_gain(camera, global, local, node, spec, start as f32, hit)
        })
        // Axis on screen degenerate (or pointing at the camera): a plain
        // "150 px upwards = full travel" drag still works.
        .unwrap_or(Vec2::new(0.0, -150.0));
    mouse.drag = Some(DragState {
        node: mesh.0,
        start,
        gain,
        distance: Vec2::ZERO,
    });
}

pub fn on_drag(on: On<Pointer<Drag>>, mut mouse: ResMut<CabMouse>) {
    if let Some(drag) = &mut mouse.drag {
        drag.distance = on.event().event.distance;
    }
}

pub fn on_scroll(
    on: On<Pointer<Scroll>>,
    meshes: Query<&ControlMesh>,
    nodes: Query<&ControlNode>,
    sim: Res<SimResource>,
    mut mouse: ResMut<CabMouse>,
) {
    let Ok(mesh) = meshes.get(on.event().entity) else {
        return;
    };
    let Some(spec) = nodes.get(mesh.0).ok().and_then(|n| control_spec(&sim.0, n)) else {
        return;
    };
    let event = &on.event().event;
    let notches = match event.unit {
        MouseScrollUnit::Line => event.y,
        MouseScrollUnit::Pixel => event.y / 100.0,
    };
    let step = match spec.input.positions() {
        0 => spec.input.scroll_step(),
        n => 1.0 / f64::from(n - 1),
    };
    mouse
        .queued
        .push((mesh.0, Queued::Step(step * f64::from(notches))));
}

/// Screen-space direction of one unit of control travel: how far the grabbed
/// point moves on screen when the value changes by `DELTA`.
fn drag_gain(
    (camera, cam_tf): (&Camera, &GlobalTransform),
    node_global: &GlobalTransform,
    node_local: &Transform,
    node: &ControlNode,
    spec: &CabControlSpec,
    value: f32,
    hit: Vec3,
) -> Option<Vec2> {
    const DELTA: f32 = 0.25;
    let parent = node_global.affine() * node_local.compute_affine().inverse();
    let at =
        |v: f32| parent * (node.base * models::motion_transform(&spec.motion, v)).compute_affine();
    let grabbed = at(value).inverse().transform_point3(hit);
    let moved = at(value + DELTA).transform_point3(grabbed);
    let s0 = camera.world_to_viewport(cam_tf, hit).ok()?;
    let s1 = camera.world_to_viewport(cam_tf, moved).ok()?;
    let gain = (s1 - s0) / DELTA;
    // Less than ~40 px over the full travel would make the control jumpy.
    (gain.length() > 40.0).then_some(gain)
}

/// Writes the mouse state into the simulation. Sits in the frame chain right
/// after `ui::player_input`: the keyboard resets the push buttons every frame,
/// the mouse re-asserts the ones it holds.
pub fn apply_mouse(
    buttons: Res<ButtonInput<MouseButton>>,
    nodes: Query<&ControlNode>,
    mut mouse: ResMut<CabMouse>,
    mut sim: ResMut<SimResource>,
) {
    if !buttons.pressed(MouseButton::Left) {
        mouse.held = None;
        mouse.drag = None;
    }

    let sim = &mut sim.0;
    let mut set = |entity: Entity, value: fn(f64, u8, f64) -> f64, arg: f64| {
        let Ok(node) = nodes.get(entity) else {
            return;
        };
        let Some(spec) = control_spec(sim, node) else {
            return;
        };
        let (input, train) = (spec.input, node.train);
        if train >= sim.trains.len() || train >= sim.controls.len() {
            return;
        }
        let current = input.get(&sim.trains[train], &sim.controls[train]);
        let next = value(current, input.positions(), arg);
        let (trains, controls) = (&mut sim.trains, &mut sim.controls);
        input.set(&mut trains[train], &mut controls[train], next);
    };

    if let Some(held) = mouse.held {
        set(held, |_, _, _| 1.0, 0.0);
    }
    if let Some(drag) = &mouse.drag {
        let travel = f64::from(drag.distance.dot(drag.gain) / drag.gain.length_squared());
        set(drag.node, |_, _, v| v, drag.start + travel);
    }
    for (entity, action) in std::mem::take(&mut mouse.queued) {
        match action {
            Queued::Cycle => set(entity, cycle, 0.0),
            Queued::Step(dv) => set(entity, |v, _, dv| (v + dv).clamp(0.0, 1.0), dv),
        }
    }

    // Hovered control for the HUD, value read back after everything applied.
    mouse.hover_info = mouse.hover.and_then(|entity| {
        let node = nodes.get(entity).ok()?;
        let spec = control_spec(sim, node)?;
        let cab = sim.controls.get(node.train)?;
        Some((
            spec.input.key(),
            spec.input.get(&sim.trains[node.train], cab),
        ))
    });
}

/// Next detent of a switch, wrapping around like repeated clicks on the real thing.
fn cycle(current: f64, positions: u8, _: f64) -> f64 {
    let n = positions.max(2);
    let last = f64::from(n - 1);
    let index = (current * last).round() as u8;
    f64::from((index + 1) % n) / last
}

const HIGHLIGHT: LinearRgba = LinearRgba {
    red: 0.25,
    green: 0.22,
    blue: 0.08,
    alpha: 1.0,
};

/// Emissive glow on the meshes of the hovered control (their materials are
/// per-control clones made at bind time, so nothing else lights up with them).
pub fn update_highlight(
    mouse: Res<CabMouse>,
    meshes: Query<(
        &ControlMesh,
        &MeshMaterial3d<StandardMaterial>,
        &Highlightable,
    )>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut last: Local<Option<Entity>>,
) {
    if mouse.hover == *last {
        return;
    }
    for (mesh, handle, rest) in meshes.iter() {
        if Some(mesh.0) != mouse.hover && Some(mesh.0) != *last {
            continue;
        }
        if let Some(mut material) = materials.get_mut(&handle.0) {
            material.emissive = if Some(mesh.0) == mouse.hover {
                rest.emissive + HIGHLIGHT
            } else {
                rest.emissive
            };
        }
    }
    *last = mouse.hover;
}
