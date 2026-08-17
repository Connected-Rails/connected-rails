//! Viewport camera: the top-down map and a free 3D view, switched with F4.
//!
//! Both are the same orbit — a pivot ([`Focus::position`]), a distance
//! ([`Focus::height`]) and a look direction ([`Focus::yaw`]/[`Focus::pitch`]).
//! Top-down is the special case that looks straight down with north up, so the
//! map the editor has always been keeps its bindings exactly.
//!
//! The 3D view moves the way an Unreal viewport does, because that is the
//! muscle memory a level builder arrives with: hold the right button to look
//! and fly with WASD (Q/E down and up, Shift faster), Alt+left orbits the
//! pivot, the middle button pans, the wheel dollies — or sets the fly speed
//! while the right button is down. F frames the selection in both views.

use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use glam::DVec3;
use world_coords::{EcefPos, EnuFrame};

use crate::tools::{self, EditorState};
use crate::{Focus, Line, Origin};

/// How the viewport looks at the world.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum ViewMode {
    /// Straight down, north up — the schematic map over the aerial imagery.
    #[default]
    TopDown,
    /// Free 3D view, flown like an Unreal viewport.
    Perspective,
}

/// Looking straight down: the top-down pitch, and the pole of the orbit.
const STRAIGHT_DOWN: f64 = std::f64::consts::FRAC_PI_2;
/// Pitch the 3D view opens at — steep enough to keep the overview, shallow
/// enough that the ground has depth.
const DEFAULT_PITCH: f64 = 0.9;
/// The orbit never quite reaches the pole: a look direction parallel to `up`
/// leaves `look_at` without a reference and the view snaps over.
const PITCH_LIMIT: f64 = STRAIGHT_DOWN - 0.01;
/// Radians of look and orbit per pixel of mouse travel.
const LOOK_SPEED: f64 = 0.005;
/// Half the vertical field of view — the default perspective projection.
const HALF_FOV: f64 = std::f64::consts::FRAC_PI_8;
/// Closest the 3D view comes to its pivot [m]; the map keeps its own floor,
/// where a lower camera would only show four imagery tiles.
const MIN_DISTANCE_3D: f64 = 3.0;
const MIN_DISTANCE_MAP: f64 = 60.0;
const MAX_DISTANCE: f64 = 20_000.0;
/// Distance `F` leaves between the camera and what it framed [m].
const FRAME_DISTANCE: f64 = 80.0;

impl Focus {
    /// Unit look direction of the camera — from the camera towards the pivot.
    pub fn look_dir(&self) -> DVec3 {
        let frame = EnuFrame::at(self.position);
        let horizontal = frame.north * self.yaw.cos() + frame.east * self.yaw.sin();
        (horizontal * self.pitch.cos() - frame.up * self.pitch.sin()).normalize()
    }

    /// Where the camera stands: `height` metres back along the look direction.
    pub fn camera_pos(&self) -> EcefPos {
        EcefPos(self.position.0 - self.look_dir() * self.height)
    }

    /// Moves the pivot so the camera ends up back at `camera` — what turns a
    /// change of `yaw`/`pitch` into looking around on the spot instead of
    /// orbiting.
    ///
    /// The look direction is read in the ENU frame *at the pivot*, so moving
    /// the pivot turns it a little: the fixed point is iterated instead of
    /// solved. The step shrinks by the distance over the earth's radius, so a
    /// few rounds leave micrometres — and without them the camera creeps away
    /// centimetre by centimetre while someone looks around.
    fn look_from(&mut self, camera: EcefPos) {
        for _ in 0..4 {
            self.position = EcefPos(camera.0 + self.look_dir() * self.height);
        }
    }

    fn min_distance(&self) -> f64 {
        match self.mode {
            ViewMode::TopDown => MIN_DISTANCE_MAP,
            ViewMode::Perspective => MIN_DISTANCE_3D,
        }
    }

    /// Screen right and screen up of the current view in ECEF — what a pan
    /// drag moves along.
    fn screen_axes(&self) -> (DVec3, DVec3) {
        let dir = self.look_dir();
        let right = dir
            .cross(EnuFrame::at(self.position).up)
            .normalize_or_zero();
        (right, right.cross(dir))
    }
}

/// Switches the two views, keeping the point they look at.
pub fn toggle_mode(focus: &mut Focus) {
    match focus.mode {
        ViewMode::TopDown => {
            focus.mode = ViewMode::Perspective;
            focus.pitch = DEFAULT_PITCH;
        }
        ViewMode::Perspective => {
            focus.mode = ViewMode::TopDown;
            focus.yaw = 0.0;
            focus.pitch = STRAIGHT_DOWN;
            focus.height = focus.height.max(MIN_DISTANCE_MAP);
        }
    }
}

/// Moves the view point, the distance and the look direction, then puts the
/// camera where the two of them say.
#[allow(clippy::too_many_arguments)]
pub fn camera_control(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<MouseWheel>,
    mut motion: MessageReader<MouseMotion>,
    windows: Query<&Window, With<PrimaryWindow>>,
    time: Res<Time>,
    origin: Res<Origin>,
    line: Res<Line>,
    mut state: ResMut<EditorState>,
    mut focus: ResMut<Focus>,
    mut camera: Query<&mut Transform, With<Camera3d>>,
) {
    let dt = time.delta_secs_f64();
    let scroll: f64 = wheel.read().map(|w| w.y as f64).sum();
    let drag: Vec2 = motion.read().map(|m| m.delta).sum();
    // Mouse input only inside the viewport rect the panels leave free — the
    // hand-built panel layout is invisible to egui's own hit test, so the
    // check is ours (see `EditorState::viewport`).
    let over_map = windows
        .single()
        .ok()
        .and_then(|w| w.cursor_position())
        .is_some_and(|p| state.viewport.contains(p));

    if !state.typing {
        if keys.just_pressed(KeyCode::F4) {
            toggle_mode(&mut focus);
            state.map_used = true;
        }
        // F frames the selection, as it does in every 3D editor.
        if keys.just_pressed(KeyCode::KeyF)
            && let Some(p) = tools::selection_pos(&line, state.selection, &focus)
        {
            focus.position = p;
            focus.height = focus.height.min(FRAME_DISTANCE).max(focus.min_distance());
            state.map_used = true;
        }
        match focus.mode {
            ViewMode::TopDown => keyboard_pan(&keys, dt, &mut state, &mut focus),
            ViewMode::Perspective => fly(&keys, &buttons, dt, &mut state, &mut focus),
        }
    }

    if over_map {
        mouse_look(&buttons, &keys, drag, scroll, &mut state, &mut focus);
    }

    let Ok(mut transform) = camera.single_mut() else {
        return;
    };
    let center = origin.0.to_render(focus.position);
    let frame = EnuFrame::at(focus.position);
    let up = origin.0.dir_to_render(frame.up);
    match focus.mode {
        // Straight down is the one angle `look_at(_, up)` cannot express: the
        // map takes north as its reference instead.
        ViewMode::TopDown => {
            transform.translation = center + up * focus.height as f32;
            transform.look_at(center, origin.0.dir_to_render(frame.north));
        }
        ViewMode::Perspective => {
            let dir = origin.0.dir_to_render(focus.look_dir());
            transform.translation = center - dir * focus.height as f32;
            transform.look_at(center, up);
        }
    }
}

/// WASD and the arrows pan the map, PgUp/PgDn change the height — the bindings
/// the top-down editor has always had.
fn keyboard_pan(keys: &ButtonInput<KeyCode>, dt: f64, state: &mut EditorState, focus: &mut Focus) {
    let frame = EnuFrame::at(focus.position);
    // Movement scales with the height: far up, panning is generous.
    let speed = focus.height * 0.8 * dt;
    let mut shift = DVec3::ZERO;
    if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
        shift += frame.north * speed;
    }
    if keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown) {
        shift -= frame.north * speed;
    }
    if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
        shift -= frame.east * speed;
    }
    if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
        shift += frame.east * speed;
    }
    if shift != DVec3::ZERO {
        focus.position = EcefPos(focus.position.0 + shift);
        state.map_used = true;
    }
    if keys.pressed(KeyCode::PageUp) || keys.pressed(KeyCode::NumpadSubtract) {
        focus.height = (focus.height * (1.0 + dt)).min(MAX_DISTANCE);
    }
    if keys.pressed(KeyCode::PageDown) || keys.pressed(KeyCode::NumpadAdd) {
        focus.height = (focus.height * (1.0 - dt)).max(MIN_DISTANCE_MAP);
    }
}

/// WASDQE fly the 3D view while the right button is held — the letters belong
/// to the gizmo the moment it is let go, exactly as in Unreal.
fn fly(
    keys: &ButtonInput<KeyCode>,
    buttons: &ButtonInput<MouseButton>,
    dt: f64,
    state: &mut EditorState,
    focus: &mut Focus,
) {
    if !buttons.pressed(MouseButton::Right) {
        return;
    }
    let frame = EnuFrame::at(focus.position);
    let (right, _) = focus.screen_axes();
    let forward = focus.look_dir();
    let boost = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
        4.0
    } else {
        1.0
    };
    let speed = focus.height * 0.8 * focus.fly_speed * boost * dt;
    let mut shift = DVec3::ZERO;
    for (key, dir) in [
        (KeyCode::KeyW, forward),
        (KeyCode::KeyS, -forward),
        (KeyCode::KeyD, right),
        (KeyCode::KeyA, -right),
        (KeyCode::KeyE, frame.up),
        (KeyCode::KeyQ, -frame.up),
    ] {
        if keys.pressed(key) {
            shift += dir * speed;
        }
    }
    if shift != DVec3::ZERO {
        focus.position = EcefPos(focus.position.0 + shift);
        state.map_used = true;
    }
}

/// Look, orbit, pan and zoom. Which button does what is the Unreal split:
/// right looks, Alt+left orbits, middle pans; the wheel dollies, or sets the
/// fly speed while the right button is down.
fn mouse_look(
    buttons: &ButtonInput<MouseButton>,
    keys: &ButtonInput<KeyCode>,
    drag: Vec2,
    scroll: f64,
    state: &mut EditorState,
    focus: &mut Focus,
) {
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    let three_d = focus.mode == ViewMode::Perspective;

    if scroll != 0.0 {
        // Right button held, the wheel is Unreal's camera speed dial.
        if three_d && buttons.pressed(MouseButton::Right) {
            focus.fly_speed = (focus.fly_speed * (1.0 + scroll * 0.2)).clamp(0.05, 20.0);
        } else {
            focus.height =
                (focus.height * (1.0 - scroll * 0.15)).clamp(focus.min_distance(), MAX_DISTANCE);
        }
        state.map_used = true;
    }

    if drag == Vec2::ZERO {
        return;
    }
    // Looking turns the camera where it stands; orbiting swings it around the
    // pivot. Same two angles — only one of them moves the pivot after.
    if three_d
        && (buttons.pressed(MouseButton::Right) || (alt && buttons.pressed(MouseButton::Left)))
    {
        let camera = focus.camera_pos();
        let orbit = !buttons.pressed(MouseButton::Right);
        focus.yaw += drag.x as f64 * LOOK_SPEED;
        focus.pitch = (focus.pitch + drag.y as f64 * LOOK_SPEED).clamp(-PITCH_LIMIT, PITCH_LIMIT);
        if !orbit {
            focus.look_from(camera);
        }
        state.map_used = true;
        return;
    }

    if buttons.pressed(MouseButton::Middle) {
        // Metres per pixel on the pivot plane, so what is dragged sticks to
        // the cursor.
        let metres_per_px =
            focus.height * 2.0 * HALF_FOV.tan() / (state.viewport.height().max(1.0) as f64);
        let (right, up) = focus.screen_axes();
        let shift = right * (drag.x as f64 * metres_per_px) - up * (drag.y as f64 * metres_per_px);
        focus.position = EcefPos(focus.position.0 - shift);
        state.map_used = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use world_coords::geo;

    fn focus_at(mode: ViewMode, yaw: f64, pitch: f64) -> Focus {
        Focus {
            position: geo::to_ecef_deg(52.0, 10.0, 146.0),
            height: 500.0,
            mode,
            yaw,
            pitch,
            fly_speed: 1.0,
        }
    }

    /// Top-down is the orbit looking straight down: the camera stands above
    /// the pivot, whatever the yaw.
    #[test]
    fn straight_down_puts_the_camera_overhead() {
        let focus = focus_at(ViewMode::TopDown, 1.2, STRAIGHT_DOWN);
        let frame = EnuFrame::at(focus.position);
        let offset = focus.camera_pos().0 - focus.position.0;
        assert!((offset.dot(frame.up) - 500.0).abs() < 1e-3, "{offset}");
        assert!(offset.dot(frame.north).abs() < 1e-3);
        assert!(offset.dot(frame.east).abs() < 1e-3);
    }

    /// Yaw is a compass heading: 0 looks north, a quarter turn looks east.
    #[test]
    fn yaw_is_a_compass_heading() {
        let frame = EnuFrame::at(focus_at(ViewMode::Perspective, 0.0, 0.0).position);
        let north = focus_at(ViewMode::Perspective, 0.0, 0.0).look_dir();
        let east = focus_at(ViewMode::Perspective, STRAIGHT_DOWN, 0.0).look_dir();
        assert!((north.dot(frame.north) - 1.0).abs() < 1e-9, "{north}");
        assert!((east.dot(frame.east) - 1.0).abs() < 1e-9, "{east}");
    }

    /// Looking around leaves the camera where it stands — that is the whole
    /// difference between it and orbiting.
    #[test]
    fn looking_around_keeps_the_camera_put() {
        let mut focus = focus_at(ViewMode::Perspective, 0.0, DEFAULT_PITCH);
        let before = focus.camera_pos();
        focus.yaw += 0.4;
        focus.pitch -= 0.2;
        focus.look_from(before);
        assert!(focus.camera_pos().0.distance(before.0) < 1e-6);
        // …and the pivot moved, because the camera did not.
        let orbit = focus_at(ViewMode::Perspective, 0.4, DEFAULT_PITCH - 0.2);
        assert!(focus.position.0.distance(orbit.position.0) > 1.0);
    }

    /// Switching back to the map looks straight down again, at the same point.
    #[test]
    fn toggling_back_restores_the_map() {
        let mut focus = focus_at(ViewMode::TopDown, 0.0, STRAIGHT_DOWN);
        let pivot = focus.position;
        toggle_mode(&mut focus);
        assert_eq!(focus.mode, ViewMode::Perspective);
        toggle_mode(&mut focus);
        assert_eq!(focus.mode, ViewMode::TopDown);
        assert_eq!(focus.pitch, STRAIGHT_DOWN);
        assert_eq!(focus.position.0, pivot.0);
    }
}
