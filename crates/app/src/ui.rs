//! Input and cameras (plan ch. 12). The display itself is `hud.rs`.
//!
//! Full operability via the keyboard (MaSzyna principle); the clickable
//! 3D controls are added in M6.

use crate::bindings::{Action, Input, Lever};
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

/// Bound keys and controller buttons → cab inputs. Which key does what is
/// `bindings.rs`; this is only what each action moves.
pub fn player_input(
    input: Input,
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

    // Power controller, including electric brake in the negative range.
    if input.pressed(Action::ThrottleUp) {
        cab.throttle = (cab.throttle + dt * 0.5).min(1.0);
    }
    if input.pressed(Action::ThrottleDown) {
        cab.throttle = (cab.throttle - dt * 0.5).max(-1.0);
    }
    if input.just_pressed(Action::ThrottleOff) {
        cab.throttle = 0.0;
    }

    // Reverser.
    if input.just_pressed(Action::ReverserForward) {
        cab.reverser = 1;
    }
    if input.just_pressed(Action::ReverserBack) {
        cab.reverser = -1;
    }
    if input.just_pressed(Action::ReverserNeutral) {
        cab.reverser = 0;
    }

    // Driver's brake valve: release, apply, lap, fill, emergency.
    let drop = match cab.brake_valve {
        DriverBrakeValve::Service(d) => d,
        DriverBrakeValve::Emergency => 1.5,
        _ => 0.0,
    };
    if input.pressed(Action::BrakeApply) {
        cab.brake_valve = DriverBrakeValve::Service((drop + dt * 0.4).min(1.5));
    }
    if input.pressed(Action::BrakeRelease) {
        let next = drop - dt * 0.4;
        cab.brake_valve = if next <= 0.0 {
            DriverBrakeValve::Release
        } else {
            DriverBrakeValve::Service(next)
        };
    }
    if input.just_pressed(Action::BrakeLap) {
        cab.brake_valve = DriverBrakeValve::Lap;
    }
    if input.just_pressed(Action::BrakeEmergency) {
        cab.brake_valve = DriverBrakeValve::Emergency;
    }
    if input.just_pressed(Action::BrakeFill) {
        cab.brake_valve = DriverBrakeValve::Fill;
    }

    // A lever put on a stick or a trigger holds its position absolutely: what the axis
    // says is where the lever is. That is the whole point of binding one — a rate key can
    // nudge a notch, only an axis can hold it — so the axis is applied after the keys and
    // wins over them.
    //
    // The brake valve keeps its detents: lap, fill and emergency are positions the axis
    // has no travel for. Emergency latches until a key leaves it; the other two the axis
    // takes back on the next frame, which is right — a hand on the lever is the lap.
    if let Some(notch) = input.lever(Lever::Throttle) {
        cab.throttle = f64::from(notch).clamp(-1.0, 1.0);
    }
    if let Some(notch) = input.lever(Lever::BrakeValve)
        && !matches!(cab.brake_valve, DriverBrakeValve::Emergency)
    {
        let drop = f64::from(notch).clamp(0.0, 1.0) * 1.5;
        cab.brake_valve = if drop <= 0.0 {
            DriverBrakeValve::Release
        } else {
            DriverBrakeValve::Service(drop)
        };
    }
    if let Some(notch) = input.lever(Lever::DirectBrake) {
        cab.direct_brake = f64::from(notch).clamp(0.0, 1.0);
    }

    // Direct brake and sanding.
    if input.pressed(Action::DirectBrakeApply) {
        cab.direct_brake = (cab.direct_brake + dt).min(1.0);
    }
    if input.pressed(Action::DirectBrakeRelease) {
        cab.direct_brake = (cab.direct_brake - dt).max(0.0);
    }
    cab.sanding = input.pressed(Action::Sanding);
    // Release button of the loco brake, the parking brake, the pre-controlled brake.
    cab.brake_release = input.pressed(Action::LocoBrakeRelease);
    if input.just_pressed(Action::ParkingBrake) {
        cab.parking_brake = !cab.parking_brake;
    }
    if input.just_pressed(Action::EpBrake) {
        cab.ep_brake = !cab.ep_brake;
    }
    // Starter button of the diesel engine.
    cab.engine_start = input.pressed(Action::EngineStart);

    // Doors: release left, release right, close.
    cab.door_release_left = input.pressed(Action::DoorLeft);
    cab.door_release_right = input.pressed(Action::DoorRight);
    cab.door_close = input.pressed(Action::DoorClose);

    // Sifa and train protection.
    cab.sifa = input.pressed(Action::Sifa);
    cab.pzb_acknowledge = input.pressed(Action::PzbAcknowledge);
    cab.pzb_exempt = input.pressed(Action::PzbFree);
    cab.pzb_override = input.pressed(Action::PzbOverride);
    cab.lzb_takeover = input.pressed(Action::LzbTakeover);
    cab.lzb_end = input.pressed(Action::LzbEnd);
    cab.lzb_test = input.pressed(Action::LzbTest);
    cab.horn = input.pressed(Action::Horn);

    // Wipers: one press steps through off – interval – slow – fast, wrapping around.
    if input.just_pressed(Action::Wipers) {
        cab.wipers = (cab.wipers + 1) % 4;
    }

    // Lights: headlights (Spitzensignal), cab light, and the instrument backlighting
    // dimmer — held down like the direct brake, since it is a knob.
    if input.just_pressed(Action::Headlights) {
        cab.headlights = !cab.headlights;
    }
    if input.just_pressed(Action::CabLight) {
        cab.cab_light = !cab.cab_light;
    }
    if input.pressed(Action::InstrumentLightUp) {
        cab.instrument_light = (cab.instrument_light + dt).min(1.0);
    }
    if input.pressed(Action::InstrumentLightDown) {
        cab.instrument_light = (cab.instrument_light - dt).max(0.0);
    }

    // AFB: on/off, and the target speed dialled in 10 km/h steps.
    if input.just_pressed(Action::Afb) {
        cab.afb = !cab.afb;
    }
    if input.just_pressed(Action::AfbDown) {
        cab.afb_target = (cab.afb_target - 10.0).max(0.0);
    }
    if input.just_pressed(Action::AfbUp) {
        cab.afb_target = (cab.afb_target + 10.0).min(afb_max);
    }

    // Range selector of a two-range gearbox: shunting gear ↔ road gear. The switch can be
    // turned at any time; the drive only lets the change take at a stand.
    if input.just_pressed(Action::RoadGear) {
        cab.road_gear = !cab.road_gear;
    }

    // Preparation: battery, pantograph, main switch, compressor.
    let train = &mut sim.0.trains[index];
    for v in &mut train.vehicles {
        if !v.spec.powered() {
            continue;
        }
        if input.just_pressed(Action::Battery) {
            v.traction.battery = !v.traction.battery;
            // Switching the battery on sets the train protection up: it runs its function
            // test and holds the brake until the driver acknowledges it (plan 9.3/9.4).
            if v.traction.battery {
                v.safety.power_on();
            }
        }
        if input.just_pressed(Action::Pantograph) {
            v.traction.pantograph_command = !v.traction.pantograph_command;
        }
        if input.just_pressed(Action::MainSwitch) {
            v.traction.main_switch_command = !v.traction.main_switch_command;
        }
        if input.just_pressed(Action::Compressor) {
            v.traction.compressor = !v.traction.compressor;
        }
    }

    // Train type switch (Zugartschalter): cycles O → M → U, standstill only.
    if input.just_pressed(Action::TrainType) {
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

/// Camera control: four actions switch the perspective, four more pan, and a
/// controller's right stick looks around on its own.
// A Bevy system takes its resources as parameters — the argument count says nothing here.
#[allow(clippy::too_many_arguments)]
pub fn camera_control(
    input: Input,
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
    if input.just_pressed(Action::ViewCab) {
        state.mode = CameraMode::Cab;
    }
    if input.just_pressed(Action::ViewOutside) {
        state.mode = CameraMode::Outside;
        if state.distance <= 0.0 {
            state.distance = 40.0;
        }
    }
    if input.just_pressed(Action::ViewWayside) {
        state.mode = CameraMode::Wayside;
        state.wayside = None;
    }
    if input.just_pressed(Action::ViewWalk) {
        state.mode = CameraMode::Walk;
    }
    // With the mod manager open the arrow keys belong to its list, not to the camera.
    let turn = if manager.open { 0.0 } else { 1.2 * dt };
    if input.pressed(Action::LookLeft) {
        state.yaw += turn;
    }
    if input.pressed(Action::LookRight) {
        state.yaw -= turn;
    }
    if input.pressed(Action::LookUp) {
        state.pitch = (state.pitch + turn).min(1.2);
    }
    if input.pressed(Action::LookDown) {
        state.pitch = (state.pitch - turn).max(-1.2);
    }
    if input.pressed(Action::ZoomIn) {
        state.distance = (state.distance - 30.0 * dt).max(10.0);
    }
    if input.pressed(Action::ZoomOut) {
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
    // The right stick looks around without a binding and without a button being held:
    // an axis is not a lever, and the mouse asks no permission either.
    let stick = input.look();
    if stick != Vec2::ZERO {
        let speed = 2.5 * gameplay.look_speed * dt;
        state.yaw -= stick.x * speed;
        state.pitch = (state.pitch + stick.y * speed).clamp(-1.2, 1.2);
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
                    // Vehicle views sit on the rail head (`sync_vehicles`), which is
                    // the origin of the model the eye point is measured in.
                    origin.0.to_render(pose.pos)
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
