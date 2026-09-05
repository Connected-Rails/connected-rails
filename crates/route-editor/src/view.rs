//! Viewport camera: a free 3D view of the module.
//!
//! The camera is an orbit — a pivot ([`Focus::position`]), a distance
//! ([`Focus::height`]) and a look direction ([`Focus::yaw`]/[`Focus::pitch`]).
//!
//! It moves the way an Unreal viewport does, because that is the muscle memory
//! a level builder arrives with: hold the right button to look and fly with
//! WASD (Q/E down and up, Shift faster), Alt+left orbits the pivot, the middle
//! button pans, the wheel dollies — or turns the camera speed dial while the
//! right button is down. F frames the selection.
//!
//! The dial is Unreal's: eight steps, each doubling the flight speed, and a
//! free multiplier on top of them (`CameraSpeedScalar`) for the distances
//! eight steps do not cover. See [`Focus::fly_speed`].

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
/// Radians of look and orbit per pixel of mouse travel — Unreal's default
/// mouse sensitivity, which is a fifth of a degree per pixel.
const LOOK_SPEED: f64 = 0.2 * std::f64::consts::PI / 180.0;
/// Half the vertical field of view — the default perspective projection.
const HALF_FOV: f64 = std::f64::consts::FRAC_PI_8;
/// Closest the camera comes to its pivot [m].
const MIN_DISTANCE: f64 = 3.0;
const MAX_DISTANCE: f64 = 20_000.0;
/// Distance `F` leaves between the camera and what it framed [m].
const FRAME_DISTANCE: f64 = 80.0;
/// Steps on the camera speed dial. Unreal's `MaxCameraSpeeds` is eight; the
/// dial here runs four steps further, because a module is tens of kilometres
/// long and the wheel under the right button is the one control a flying
/// hand can reach — 320 m/s at the old top step took a minute to cross one,
/// and the scalar that would have helped sits in a menu.
pub const SPEED_STEPS: i32 = 12;
/// The step the dial opens at, the one that leaves the speed unscaled.
pub const DEFAULT_SPEED_STEP: i32 = 4;
/// Ceiling of the fine multiplier — Unreal's `CameraSpeedScalar` UI range.
pub const MAX_SPEED_SCALAR: f64 = 128.0;
/// Flight speed at [`DEFAULT_SPEED_STEP`] and a scalar of one [m/s]. Fast
/// enough to cross a module, slow enough to place a signal by eye.
const BASE_FLY_SPEED: f64 = 20.0;
/// What holding Shift multiplies the flight speed by. Shift is Unreal's
/// *precision* modifier — it slows the camera down for the last metres up to a
/// signal; the dial is what makes it fast.
const PRECISION: f64 = 0.5;

impl Focus {
    /// Whether the view is the vertical map view rather than the free one.
    pub fn is_top_down(&self) -> bool {
        self.pitch > 1.5
    }

    /// Switches between the vertical map view and the free view — the World
    /// Editor's 2D-map gesture for track work over the imagery. Pivot and
    /// height stay; only the look direction tips.
    pub fn toggle_top_down(&mut self) {
        self.pitch = if self.is_top_down() {
            DEFAULT_PITCH
        } else {
            PITCH_LIMIT
        };
    }

    /// Turns the view to face north — the compass click.
    pub fn face_north(&mut self) {
        self.yaw = 0.0;
    }

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

    /// Moves the pivot so the camera renders from `eye` again — what turns a
    /// change of `yaw`/`pitch` into looking around on the spot instead of
    /// orbiting.
    ///
    /// It is the *rendered* eye that has to stay put, not [`Self::camera_pos`]:
    /// [`Self::viewport_offset`] pushes the camera sideways by the width of the
    /// panels, and that push turns with the camera. Hold the unpushed point
    /// still and the eye runs around a circle a few hundred metres wide while
    /// someone looks around — the view swings as if it were orbiting something
    /// off to the side, which is exactly what it is doing.
    ///
    /// Both the look direction and the offset are read in the ENU frame *at
    /// the pivot*, so moving the pivot turns them a little: the fixed point is
    /// iterated instead of solved. The step shrinks by the distance over the
    /// earth's radius, so a few rounds leave micrometres.
    fn look_from(&mut self, eye: EcefPos, free: Rect, window: Vec2) {
        for _ in 0..4 {
            self.position =
                EcefPos(eye.0 - self.viewport_offset(free, window) + self.look_dir() * self.height);
        }
    }

    /// How far the camera stands from [`Self::camera_pos`] so the pivot sits in
    /// the middle of the free viewport instead of the middle of the window.
    ///
    /// The scene is rendered into the whole window and the panels are drawn on
    /// top of it — `bevy_egui` hangs its context on this same camera, so giving
    /// the camera a `viewport` of its own would squeeze the UI into that rect
    /// as well. Without this offset the pivot sits behind the side panel, and
    /// with it the point the imagery tiles are loaded around: the aerial
    /// picture is off-centre in the part of the window one can see, and half of
    /// what is fetched is under a panel.
    ///
    /// It moves the camera, not [`Self::position`] — the focus stays what the
    /// status bar reports and what the tiles are centred on.
    fn viewport_offset(&self, free: Rect, window: Vec2) -> DVec3 {
        if free.width() < 1.0 || free.height() < 1.0 || window.y < 1.0 {
            return DVec3::ZERO;
        }
        let offset = free.center() - window / 2.0;
        // Metres per pixel on the plane through the pivot: the vertical field
        // of view spans `2 · d · tan(fov/2)` there, measured over the whole
        // window, because that is what the scene is rendered into. Screen y
        // counts downwards, so a free rect below the window's middle moves the
        // camera up.
        let per_pixel = 2.0 * self.height * HALF_FOV.tan() / window.y as f64;
        let (right, up) = self.screen_axes();
        (up * offset.y as f64 - right * offset.x as f64) * per_pixel
    }

    /// Where the camera renders from.
    fn eye(&self, free: Rect, window: Vec2) -> EcefPos {
        EcefPos(self.camera_pos().0 + self.viewport_offset(free, window))
    }

    /// Flight speed [m/s]. Unreal's dial doubles per step, so the twelve
    /// steps span 0.125x to 256x the base (2.5 m/s to 5 km/s), and the
    /// scalar multiplies on top of them.
    ///
    /// Deliberately not scaled by the distance to the pivot: a speed that
    /// changes under the builder as they zoom is a speed they do not control,
    /// and controlling it is what the dial is for. Unreal keeps this off by
    /// default too (`bUseDistanceScaledCameraSpeed`).
    pub fn fly_speed(&self) -> f64 {
        BASE_FLY_SPEED * 2f64.powi(self.speed_step - DEFAULT_SPEED_STEP) * self.speed_scalar
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

    let size = window.map_or(Vec2::ZERO, |w| Vec2::new(w.width(), w.height()));
    if over_viewport {
        mouse_look(&buttons, &keys, drag, scroll, size, &mut state, &mut focus);
    }

    let Ok(mut transform) = camera.single_mut() else {
        return;
    };
    let center = origin.0.to_render(focus.position);
    let up = origin.0.dir_to_render(EnuFrame::at(focus.position).up);
    let dir = origin.0.dir_to_render(focus.look_dir());
    // Aimed from where the camera stands, then carried to where it renders
    // from: the offset must move the camera without turning it, or the pivot
    // would swing straight back out of the free viewport.
    transform.translation = center - dir * focus.height as f32;
    transform.look_at(center, up);
    transform.translation = origin.0.to_render(focus.eye(state.viewport, size));
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
    let precision = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
        PRECISION
    } else {
        1.0
    };
    let speed = focus.fly_speed() * precision * dt;
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
    window: Vec2,
    state: &mut EditorState,
    focus: &mut Focus,
) {
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);

    if scroll != 0.0 {
        // Right button held, the wheel is Unreal's camera speed dial: one
        // notch, one step. A frame that batches several notches still moves
        // one step — at 60 Hz that costs a flick of the wheel, and reading the
        // batch as a jump of four makes the dial impossible to land on.
        if buttons.pressed(MouseButton::Right) {
            focus.speed_step = (focus.speed_step + scroll.signum() as i32).clamp(1, SPEED_STEPS);
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
        let eye = focus.eye(state.viewport, window);
        let orbit = !buttons.pressed(MouseButton::Right);
        focus.yaw += drag.x as f64 * LOOK_SPEED;
        focus.pitch = (focus.pitch + drag.y as f64 * LOOK_SPEED).clamp(-PITCH_LIMIT, PITCH_LIMIT);
        if !orbit {
            focus.look_from(eye, state.viewport, window);
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
            speed_step: DEFAULT_SPEED_STEP,
            speed_scalar: 1.0,
        }
    }

    /// The dial is Unreal's: every step doubles, the scalar multiplies on top,
    /// and the default step leaves the base speed alone.
    #[test]
    fn every_dial_step_doubles_the_speed() {
        let mut focus = focus_at(0.0, DEFAULT_PITCH);
        assert_eq!(focus.fly_speed(), BASE_FLY_SPEED);
        for step in 1..SPEED_STEPS {
            focus.speed_step = step;
            let slow = focus.fly_speed();
            focus.speed_step = step + 1;
            assert!((focus.fly_speed() - 2.0 * slow).abs() < 1e-9, "step {step}");
        }
        focus.speed_step = DEFAULT_SPEED_STEP;
        focus.speed_scalar = MAX_SPEED_SCALAR;
        assert_eq!(focus.fly_speed(), BASE_FLY_SPEED * MAX_SPEED_SCALAR);
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
        let (free, window) = (Rect::new(0.0, 0.0, 1280.0, 720.0), Vec2::new(1280.0, 720.0));
        let mut focus = focus_at(0.0, DEFAULT_PITCH);
        let before = focus.eye(free, window);
        focus.yaw += 0.4;
        focus.pitch -= 0.2;
        focus.look_from(before, free, window);
        assert!(focus.eye(free, window).0.distance(before.0) < 1e-6);
        // …and the pivot moved, because the camera did not.
        let orbit = focus_at(0.4, DEFAULT_PITCH - 0.2);
        assert!(focus.position.0.distance(orbit.position.0) > 1.0);
    }

    /// The same, with a side panel in the way. The offset that pushes the pivot
    /// clear of the panel turns with the camera, so holding the *unpushed*
    /// point still walks the eye around a circle hundreds of metres wide — the
    /// view then swings sideways while someone only meant to look around.
    #[test]
    fn looking_around_keeps_the_camera_put_behind_a_panel() {
        let window = Vec2::new(1600.0, 900.0);
        let free = Rect::new(660.0, 40.0, 1600.0, 860.0);
        let mut focus = focus_at(0.0, DEFAULT_PITCH);
        focus.height = 900.0;
        // The offset is worth hundreds of metres at this height — the error it
        // used to leave behind was not a rounding one.
        assert!(focus.viewport_offset(free, window).length() > 100.0);

        let before = focus.eye(free, window);
        focus.yaw += 0.6;
        focus.look_from(before, free, window);
        assert!(
            focus.eye(free, window).0.distance(before.0) < 1e-6,
            "eye moved by {} m",
            focus.eye(free, window).0.distance(before.0)
        );
    }

    #[test]
    fn the_camera_moves_away_from_the_panels() {
        // Looking due north and level, so screen right is east and screen up
        // is the local up — the offset can be read off the compass.
        let mut focus = focus_at(0.0, 0.0);
        focus.height = 900.0;
        let frame = EnuFrame::at(focus.position);
        let window = Vec2::new(1280.0, 720.0);

        // A side panel on the left leaves the free rect to the right of the
        // window's middle, so the camera has to move left — west — for the
        // pivot to end up in it.
        let offset = focus.viewport_offset(Rect::new(480.0, 0.0, 1280.0, 720.0), window);
        assert!(offset.dot(frame.east) < 0.0, "{offset}");
        assert!(offset.dot(frame.up).abs() < 1e-6, "{offset}");
        // 240 px at the plane's metres per pixel.
        let per_pixel = 2.0 * 900.0 * HALF_FOV.tan() / 720.0;
        assert!(
            (offset.length() - 240.0 * per_pixel).abs() < 1e-6,
            "{offset}"
        );

        // A bar at the top pushes the free rect down; the camera goes up.
        let offset = focus.viewport_offset(Rect::new(0.0, 80.0, 1280.0, 720.0), window);
        assert!(offset.dot(frame.up) > 0.0, "{offset}");
        assert!(offset.dot(frame.east).abs() < 1e-6, "{offset}");

        // No panels, no offset.
        let free = Rect::new(0.0, 0.0, 1280.0, 720.0);
        assert_eq!(focus.viewport_offset(free, window), DVec3::ZERO);
        // An empty rect (before the first UI pass) must not move anything.
        assert_eq!(focus.viewport_offset(Rect::default(), window), DVec3::ZERO);
        // Nor must a window that has not been sized yet.
        assert_eq!(focus.viewport_offset(free, Vec2::ZERO), DVec3::ZERO);
    }
}
