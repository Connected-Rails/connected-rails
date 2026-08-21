//! Viewport camera: a free 3D view of the module.
//!
//! The camera is an orbit — a pivot ([`Focus::position`]), a distance
//! ([`Focus::height`]) and a look direction ([`Focus::yaw`]/[`Focus::pitch`]).
//!
//! It moves the way an Unreal viewport does, because that is the muscle memory
//! a level builder arrives with: hold the right button to look and fly with
//! WASD (Q/E down and up, Shift faster), Alt+left orbits the pivot, the middle
//! button pans, the wheel dollies — or sets the fly speed while the right
//! button is down. F frames the selection.

use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use glam::DVec3;
use world_coords::{EcefPos, EnuFrame};

use crate::tools::{self, EditorState};
use crate::{Focus, Line, Origin};

/// Pitch the view opens at — steep enough to keep the overview, shallow enough
/// that the ground has depth.
pub const DEFAULT_PITCH: f64 = 0.9;
/// The orbit never quite reaches the pole: a look direction parallel to `up`
/// leaves `look_at` without a reference and the view snaps over.
const PITCH_LIMIT: f64 = std::f64::consts::FRAC_PI_2 - 0.01;
/// Radians of look and orbit per pixel of mouse travel.
const LOOK_SPEED: f64 = 0.005;
/// Half the vertical field of view — the default perspective projection.
const HALF_FOV: f64 = std::f64::consts::FRAC_PI_8;
/// Closest the camera comes to its pivot [m].
const MIN_DISTANCE: f64 = 3.0;
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
    marks: Res<crate::terrain::Marks>,
    mut state: ResMut<EditorState>,
    mut focus: ResMut<Focus>,
    mut camera: Query<&mut Transform, With<Camera3d>>,
) {
    let window = windows.single().ok();
    let dt = time.delta_secs_f64();
    let scroll: f64 = wheel.read().map(|w| w.y as f64).sum();
    let drag: Vec2 = motion.read().map(|m| m.delta).sum();
    // Mouse input only inside the viewport rect the panels leave free — the
    // hand-built panel layout is invisible to egui's own hit test, so the
    // check is ours (see `EditorState::viewport`).
    let over_viewport = window
        .and_then(|w| w.cursor_position())
        .is_some_and(|p| state.over_viewport(p));

    if !state.typing {
        // F frames the selection, as it does in every 3D editor.
        if keys.just_pressed(KeyCode::KeyF)
            && let Some(p) = tools::selection_pos(&line, state.selection, &focus, &marks)
        {
            focus.position = p;
            focus.height = focus.height.clamp(MIN_DISTANCE, FRAME_DISTANCE);
            state.map_used = true;
        }
        fly(&keys, &buttons, dt, &mut state, &mut focus);
    }

    if over_viewport {
        mouse_look(&buttons, &keys, drag, scroll, &mut state, &mut focus);
    }

    let Ok(mut transform) = camera.single_mut() else {
        return;
    };
    let center = origin.0.to_render(focus.position);
    let up = origin.0.dir_to_render(EnuFrame::at(focus.position).up);
    let dir = origin.0.dir_to_render(focus.look_dir());
    transform.translation = center - dir * focus.height as f32;
    transform.look_at(center, up);
    // …and then off to the side, so the pivot ends up in the middle of what is
    // actually visible.
    if let Some(window) = window {
        let shift = viewport_shift(&transform, &focus, &state, window);
        transform.translation += shift;
    }
}

/// How far the camera has to move sideways for the pivot to sit in the middle
/// of the free viewport instead of the middle of the window.
///
/// The scene is rendered into the whole window and the panels are drawn on top
/// of it — `bevy_egui` hangs its context on this same camera, so giving the
/// camera a `viewport` of its own would squeeze the UI into that rect as well.
/// Without this shift the pivot sits behind the side panel, and with it the
/// point the imagery tiles are loaded around: the aerial picture is off-centre
/// in the part of the window one can see, and half of what is fetched is under
/// a panel.
///
/// The shift is applied to the camera, not to [`Focus::position`] — the focus
/// stays what the status bar reports and what the tiles are centred on.
fn viewport_shift(
    transform: &Transform,
    focus: &Focus,
    state: &EditorState,
    window: &Window,
) -> Vec3 {
    let free = state.viewport;
    let (width, height) = (window.width(), window.height());
    if free.width() < 1.0 || free.height() < 1.0 || height < 1.0 {
        return Vec3::ZERO;
    }
    let offset = free.center() - Vec2::new(width, height) / 2.0;
    // Metres per pixel on the plane through the pivot: the vertical field of
    // view spans `2 · d · tan(fov/2)` there. Screen y counts downwards, so a
    // free rect below the window's middle moves the camera up.
    let per_pixel = (2.0 * focus.height * HALF_FOV.tan() / height as f64) as f32;
    (transform.up() * offset.y - transform.right() * offset.x) * per_pixel
}

/// WASDQE fly the view while the right button is held — the letters belong to
/// the gizmo the moment it is let go, exactly as in Unreal.
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

    if scroll != 0.0 {
        // Right button held, the wheel is Unreal's camera speed dial.
        if buttons.pressed(MouseButton::Right) {
            focus.fly_speed = (focus.fly_speed * (1.0 + scroll * 0.2)).clamp(0.05, 20.0);
        } else {
            focus.height = (focus.height * (1.0 - scroll * 0.15)).clamp(MIN_DISTANCE, MAX_DISTANCE);
        }
        state.map_used = true;
    }

    if drag == Vec2::ZERO {
        return;
    }
    // Looking turns the camera where it stands; orbiting swings it around the
    // pivot. Same two angles — only one of them moves the pivot after.
    if buttons.pressed(MouseButton::Right) || (alt && buttons.pressed(MouseButton::Left)) {
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

    fn focus_at(yaw: f64, pitch: f64) -> Focus {
        Focus {
            position: geo::to_ecef_deg(52.0, 10.0, 146.0),
            height: 500.0,
            yaw,
            pitch,
            fly_speed: 1.0,
        }
    }

    /// Yaw is a compass heading: 0 looks north, a quarter turn looks east.
    #[test]
    fn yaw_is_a_compass_heading() {
        let frame = EnuFrame::at(focus_at(0.0, 0.0).position);
        let north = focus_at(0.0, 0.0).look_dir();
        let east = focus_at(std::f64::consts::FRAC_PI_2, 0.0).look_dir();
        assert!((north.dot(frame.north) - 1.0).abs() < 1e-9, "{north}");
        assert!((east.dot(frame.east) - 1.0).abs() < 1e-9, "{east}");
    }

    /// Looking around leaves the camera where it stands — that is the whole
    /// difference between it and orbiting.
    #[test]
    fn looking_around_keeps_the_camera_put() {
        let mut focus = focus_at(0.0, DEFAULT_PITCH);
        let before = focus.camera_pos();
        focus.yaw += 0.4;
        focus.pitch -= 0.2;
        focus.look_from(before);
        assert!(focus.camera_pos().0.distance(before.0) < 1e-6);
        // …and the pivot moved, because the camera did not.
        let orbit = focus_at(0.4, DEFAULT_PITCH - 0.2);
        assert!(focus.position.0.distance(orbit.position.0) > 1.0);
    }

    fn window(width: f32, height: f32) -> Window {
        let mut window = Window::default();
        window.resolution.set(width, height);
        window
    }

    #[test]
    fn the_camera_moves_away_from_the_panels() {
        let mut state = EditorState::default();
        let mut focus = focus_at(0.0, DEFAULT_PITCH);
        focus.height = 900.0;
        // Identity transform: right is +X, up is +Y.
        let transform = Transform::IDENTITY;
        let window = window(1280.0, 720.0);

        // A side panel on the left leaves the free rect to the right of the
        // window's middle, so the camera has to move left for the pivot to end
        // up in it.
        state.viewport = Rect::new(480.0, 0.0, 1280.0, 720.0);
        let shift = viewport_shift(&transform, &focus, &state, &window);
        assert!(shift.x < 0.0, "{shift:?}");
        assert_eq!(shift.y, 0.0);
        // 240 px at the plane's metres per pixel.
        let per_pixel = 2.0 * 900.0 * HALF_FOV.tan() / 720.0;
        assert!(
            (shift.x as f64 + 240.0 * per_pixel).abs() < 1e-3,
            "{shift:?}"
        );

        // A bar at the top pushes the free rect down; the camera goes up.
        state.viewport = Rect::new(0.0, 80.0, 1280.0, 720.0);
        let shift = viewport_shift(&transform, &focus, &state, &window);
        assert!(shift.y > 0.0, "{shift:?}");
        assert_eq!(shift.x, 0.0);

        // No panels, no shift.
        state.viewport = Rect::new(0.0, 0.0, 1280.0, 720.0);
        assert_eq!(
            viewport_shift(&transform, &focus, &state, &window),
            Vec3::ZERO
        );
        // An empty rect (before the first UI pass) must not move anything.
        state.viewport = Rect::default();
        assert_eq!(
            viewport_shift(&transform, &focus, &state, &window),
            Vec3::ZERO
        );
    }
}
