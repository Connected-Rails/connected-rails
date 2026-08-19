//! Input and cameras (plan ch. 12). The display itself is `hud.rs`.
//!
//! Full operability via the keyboard (MaSzyna principle); the clickable
//! 3D controls are added in M6.

use crate::mods_ui::ModManager;
use crate::settings::Gameplay;
use crate::{Origin, PlayerTrain, SimResource};
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};
use sim_core::brakes::DriverBrakeValve;

/// Camera of the player.
#[derive(Component)]
pub struct CabCamera;

/// Aiming dot in the middle of the screen, shown while walking.
#[derive(Component)]
pub struct Crosshair;

/// Camera perspectives (plan 12.4).
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq)]
pub enum CameraMode {
    /// View from the driver's seat.
    #[default]
    Cab,
    /// First person: the driver stands up and walks through the vehicle.
    Walk,
    /// External camera, orbits the train.
    Outside,
    /// Lineside camera: fixed at the spot where it was activated.
    Wayside,
}

/// View direction in the cab and orbit outside.
#[derive(Resource, Default)]
pub struct CameraState {
    pub mode: CameraMode,
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub wayside: Option<Vec3>,
}

impl CameraMode {
    /// Both views ride inside the vehicle: the seat and the walk.
    pub fn inside(self) -> bool {
        matches!(self, CameraMode::Cab | CameraMode::Walk)
    }
}

/// Key bindings → cab inputs.
pub fn player_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut sim: ResMut<SimResource>,
    player: Res<PlayerTrain>,
    time: Res<Time>,
    camera: Res<CameraState>,
) {
    // Away from the seat WASD walks; the cab keys only answer to the driver sitting
    // at the desk.
    if camera.mode == CameraMode::Walk {
        return;
    }
    let dt = time.delta_secs_f64();
    let index = player.0;
    // AFB dial ceiling: the running-gear limit of the occupied vehicle.
    let afb_max = {
        let train = &sim.0.trains[index];
        train
            .vehicles
            .get(train.cab)
            .map(|v| v.spec.v_max)
            .filter(|v| *v > 0.0)
            .unwrap_or(160.0)
    };
    let cab = &mut sim.0.controls[index];

    // Power controller (W/S), including electric brake in the negative range.
    if keys.pressed(KeyCode::KeyW) {
        cab.throttle = (cab.throttle + dt * 0.5).min(1.0);
    }
    if keys.pressed(KeyCode::KeyS) {
        cab.throttle = (cab.throttle - dt * 0.5).max(-1.0);
    }
    if keys.just_pressed(KeyCode::KeyX) {
        cab.throttle = 0.0;
    }

    // Reverser.
    if keys.just_pressed(KeyCode::KeyR) {
        cab.reverser = 1;
    }
    if keys.just_pressed(KeyCode::KeyF) {
        cab.reverser = -1;
    }
    if keys.just_pressed(KeyCode::KeyT) {
        cab.reverser = 0;
    }

    // Driver's brake valve (A = release, D = brake, E = emergency brake, Q = lap).
    let drop = match cab.brake_valve {
        DriverBrakeValve::Service(d) => d,
        DriverBrakeValve::Emergency => 1.5,
        _ => 0.0,
    };
    if keys.pressed(KeyCode::KeyD) {
        cab.brake_valve = DriverBrakeValve::Service((drop + dt * 0.4).min(1.5));
    }
    if keys.pressed(KeyCode::KeyA) {
        let next = drop - dt * 0.4;
        cab.brake_valve = if next <= 0.0 {
            DriverBrakeValve::Release
        } else {
            DriverBrakeValve::Service(next)
        };
    }
    if keys.just_pressed(KeyCode::KeyQ) {
        cab.brake_valve = DriverBrakeValve::Lap;
    }
    if keys.just_pressed(KeyCode::KeyE) {
        cab.brake_valve = DriverBrakeValve::Emergency;
    }
    if keys.just_pressed(KeyCode::KeyZ) {
        cab.brake_valve = DriverBrakeValve::Fill;
    }

    // Direct brake (Y/C), sanding (G).
    if keys.pressed(KeyCode::KeyC) {
        cab.direct_brake = (cab.direct_brake + dt).min(1.0);
    }
    if keys.pressed(KeyCode::KeyV) {
        cab.direct_brake = (cab.direct_brake - dt).max(0.0);
    }
    cab.sanding = keys.pressed(KeyCode::KeyG);
    // Release button of the loco brake (L), parking brake (P), pre-controlled brake (O).
    cab.brake_release = keys.pressed(KeyCode::KeyL);
    if keys.just_pressed(KeyCode::KeyP) {
        cab.parking_brake = !cab.parking_brake;
    }
    if keys.just_pressed(KeyCode::KeyO) {
        cab.ep_brake = !cab.ep_brake;
    }
    // Starter button of the diesel engine.
    cab.engine_start = keys.pressed(KeyCode::Digit5);

    // Doors: release left (J) / right (K), close (I).
    cab.door_release_left = keys.pressed(KeyCode::KeyJ);
    cab.door_release_right = keys.pressed(KeyCode::KeyK);
    cab.door_close = keys.pressed(KeyCode::KeyI);

    // Sifa and train protection.
    cab.sifa = keys.pressed(KeyCode::Space);
    cab.pzb_acknowledge = keys.pressed(KeyCode::PageDown);
    cab.pzb_exempt = keys.pressed(KeyCode::End);
    cab.pzb_override = keys.pressed(KeyCode::Delete);
    cab.lzb_takeover = keys.pressed(KeyCode::KeyN);
    cab.lzb_end = keys.pressed(KeyCode::KeyM);
    cab.lzb_test = keys.pressed(KeyCode::KeyB);
    cab.horn = keys.pressed(KeyCode::KeyH);

    // Wipers: Y steps through off – interval – slow – fast, wrapping around.
    if keys.just_pressed(KeyCode::KeyY) {
        cab.wipers = (cab.wipers + 1) % 4;
    }

    // Lights: 9 headlights (Spitzensignal), 0 cab light, ,/. the instrument
    // backlighting dimmer — held down like the direct brake, since it is a knob.
    if keys.just_pressed(KeyCode::Digit9) {
        cab.headlights = !cab.headlights;
    }
    if keys.just_pressed(KeyCode::Digit0) {
        cab.cab_light = !cab.cab_light;
    }
    if keys.pressed(KeyCode::Period) {
        cab.instrument_light = (cab.instrument_light + dt).min(1.0);
    }
    if keys.pressed(KeyCode::Comma) {
        cab.instrument_light = (cab.instrument_light - dt).max(0.0);
    }

    // AFB: 6 on/off, 7/8 dial the target speed in 10 km/h steps.
    if keys.just_pressed(KeyCode::Digit6) {
        cab.afb = !cab.afb;
    }
    if keys.just_pressed(KeyCode::Digit7) {
        cab.afb_target = (cab.afb_target - 10.0).max(0.0);
    }
    if keys.just_pressed(KeyCode::Digit8) {
        cab.afb_target = (cab.afb_target + 10.0).min(afb_max);
    }

    // Range selector of a two-range gearbox: shunting gear ↔ road gear. The switch can be
    // turned at any time; the drive only lets the change take at a stand.
    if keys.just_pressed(KeyCode::Backquote) {
        cab.road_gear = !cab.road_gear;
    }

    // Preparation: battery, pantograph, main switch, compressor.
    let train = &mut sim.0.trains[index];
    for v in &mut train.vehicles {
        if !v.spec.powered() {
            continue;
        }
        if keys.just_pressed(KeyCode::Digit1) {
            v.traction.battery = !v.traction.battery;
            // Switching the battery on sets the train protection up: it runs its function
            // test and holds the brake until the driver acknowledges it (plan 9.3/9.4).
            if v.traction.battery {
                v.safety.power_on();
            }
        }
        if keys.just_pressed(KeyCode::Digit2) {
            v.traction.pantograph_command = !v.traction.pantograph_command;
        }
        if keys.just_pressed(KeyCode::Digit3) {
            v.traction.main_switch_command = !v.traction.main_switch_command;
        }
        if keys.just_pressed(KeyCode::Digit4) {
            v.traction.compressor = !v.traction.compressor;
        }
    }

    // Train type switch (Zugartschalter): U cycles O → M → U, standstill only.
    if keys.just_pressed(KeyCode::KeyU) {
        use sim_core::safety::de::TrainType;
        let speed = train.speed();
        if let Some(current) = train.vehicles.iter().find_map(|v| v.safety.train_type()) {
            let next = match current {
                TrainType::O => TrainType::M,
                TrainType::M => TrainType::U,
                TrainType::U => TrainType::O,
            };
            for v in &mut train.vehicles {
                v.safety.set_train_type(next, speed);
            }
        }
    }
}

/// Camera control: F1–F4 switch the perspective, arrow keys pan.
// A Bevy system takes its resources as parameters — the argument count says nothing here.
#[allow(clippy::too_many_arguments)]
pub fn camera_control(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    time: Res<Time>,
    sim: Res<SimResource>,
    origin: Res<Origin>,
    player: Res<PlayerTrain>,
    manager: Res<ModManager>,
    gameplay: Res<Gameplay>,
    walker: Res<crate::walk::Walker>,
    mut state: ResMut<CameraState>,
    mut camera: Query<&mut Transform, With<CabCamera>>,
) {
    let dt = time.delta_secs();
    if keys.just_pressed(KeyCode::F1) {
        state.mode = CameraMode::Cab;
    }
    if keys.just_pressed(KeyCode::F2) {
        state.mode = CameraMode::Outside;
        if state.distance <= 0.0 {
            state.distance = 40.0;
        }
    }
    if keys.just_pressed(KeyCode::F3) {
        state.mode = CameraMode::Wayside;
        state.wayside = None;
    }
    if keys.just_pressed(KeyCode::F4) {
        state.mode = CameraMode::Walk;
    }
    // With the mod manager open the arrow keys belong to its list, not to the camera.
    let turn = if manager.open { 0.0 } else { 1.2 * dt };
    if keys.pressed(KeyCode::ArrowLeft) {
        state.yaw += turn;
    }
    if keys.pressed(KeyCode::ArrowRight) {
        state.yaw -= turn;
    }
    if keys.pressed(KeyCode::ArrowUp) {
        state.pitch = (state.pitch + turn).min(1.2);
    }
    if keys.pressed(KeyCode::ArrowDown) {
        state.pitch = (state.pitch - turn).max(-1.2);
    }
    if keys.pressed(KeyCode::NumpadAdd) {
        state.distance = (state.distance - 30.0 * dt).max(10.0);
    }
    if keys.pressed(KeyCode::NumpadSubtract) {
        state.distance = (state.distance + 30.0 * dt).min(400.0);
    }
    // Right-drag looks around; the left button stays free for the cab controls. While
    // walking the mouse looks on its own — the cursor is caught in the middle of the
    // screen, so there is nothing else for it to do.
    if buttons.pressed(MouseButton::Right) || state.mode == CameraMode::Walk {
        let speed = 0.003 * gameplay.look_speed;
        state.yaw -= motion.delta.x * speed;
        state.pitch = (state.pitch - motion.delta.y * speed).clamp(-1.2, 1.2);
    }

    let Ok(mut transform) = camera.single_mut() else {
        return;
    };
    let train = &sim.0.trains[player.0];
    let pose = train.vehicles[0].pos.pose(&sim.0.net);
    let pos = origin.0.to_render(pose.pos);
    let up = origin.0.dir_to_render(pose.up);
    let forward = origin.0.dir_to_render(pose.tangent);
    let right = forward.cross(up).normalize_or_zero();

    // On foot outside the train the view no longer hangs on the vehicle: yaw counts
    // from north, so a departing train does not drag the head around.
    if let Some(crate::walk::Place::Outside { eye }) = walker.place {
        transform.translation = origin.0.to_render(eye);
        transform.rotation = Quat::from_euler(EulerRot::YXZ, state.yaw, state.pitch, 0.0);
        return;
    }

    match state.mode {
        CameraMode::Cab | CameraMode::Walk => {
            // Eye point from the vehicle the driver is in — his seat, or the vehicle he
            // walked into. Vehicles without cab data fall back to the old guess: 8 m
            // ahead of the centre, 2.8 m up.
            let (aboard, local) = match walker.place {
                Some(crate::walk::Place::Aboard { vehicle, eye }) => (Some(vehicle), Some(eye)),
                _ => (None, None),
            };
            let seat = aboard
                .and_then(|v| train.vehicles.get(v))
                .or_else(|| train.vehicles.get(train.cab))
                .unwrap_or(&train.vehicles[0]);
            let eye = match seat.spec.model.as_ref().and_then(|m| m.cab.as_ref()) {
                Some(cab) => {
                    let pose = seat.pos.pose(&sim.0.net);
                    let anchor =
                        origin.0.to_render(pose.pos) + origin.0.dir_to_render(pose.up) * 2.2;
                    // Vehicle views sit 2.2 m above the rail head (`sync_vehicles`);
                    // the eye is model space below that anchor.
                    anchor
                        + origin.0.look_rotation(pose.tangent, pose.up)
                            * local.unwrap_or_else(|| Vec3::from(cab.eye))
                }
                // No cab data: the old guess, 8 m ahead of the centre and 2.8 m up,
                // which is what `CabSpec::default` holds — so the walk offset applies
                // to it just the same.
                None => {
                    let local =
                        local.unwrap_or_else(|| Vec3::from(sim_core::cab::CabSpec::default().eye));
                    pos + right * local.x + up * local.y - forward * local.z
                }
            };
            let look =
                Quat::from_axis_angle(up, state.yaw) * Quat::from_axis_angle(right, state.pitch);
            transform.translation = eye;
            transform.look_to(look * forward, up);
        }
        CameraMode::Outside => {
            let offset = Quat::from_axis_angle(up, state.yaw) * (right * state.distance)
                + up * (state.distance * 0.35 + 5.0);
            transform.translation = pos + offset;
            transform.look_at(pos, up);
        }
        CameraMode::Wayside => {
            let anchor = *state.wayside.get_or_insert(pos + right * 25.0 + up * 6.0);
            transform.translation = anchor;
            transform.look_at(pos, Vec3::Y);
        }
    }
}

/// Catches the mouse while walking: hidden, held inside the window and put back into
/// the middle of the screen every frame, so the picking ray of the cab controls stays
/// on the crosshair instead of wandering off with the cursor.
pub fn grab_cursor(
    state: Res<CameraState>,
    game: Res<State<crate::GameState>>,
    mut windows: Query<(&mut Window, &mut CursorOptions)>,
    mut crosshair: Query<&mut Visibility, With<Crosshair>>,
    mut flip: Local<bool>,
) {
    let walking = state.mode == CameraMode::Walk && *game.get() == crate::GameState::Driving;
    for (mut window, mut cursor) in windows.iter_mut() {
        if cursor.visible == walking {
            cursor.visible = !walking;
            cursor.grab_mode = if walking {
                CursorGrabMode::Confined
            } else {
                CursorGrabMode::None
            };
        }
        if walking {
            // ponytail: Bevy passes the position on to the window only when it differs
            // from the cache of the last frame — the half pixel keeps it different.
            *flip = !*flip;
            let centre = window.size() / 2.0 + Vec2::X * if *flip { 0.5 } else { 0.0 };
            window.set_cursor_position(Some(centre));
        }
    }
    for mut visibility in crosshair.iter_mut() {
        *visibility = if walking {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

/// The aiming dot of the walk. The rest of the interface is `hud.rs`; this one belongs
/// to the camera, because it marks where the caught cursor points rather than saying
/// anything about the train.
pub fn spawn_crosshair(commands: &mut Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(50.0),
            top: Val::Percent(50.0),
            width: Val::Px(5.0),
            height: Val::Px(5.0),
            ..default()
        },
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.65)),
        bevy::picking::Pickable::IGNORE,
        Visibility::Hidden,
        Crosshair,
    ));
}
