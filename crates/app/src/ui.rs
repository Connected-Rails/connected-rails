//! Input, cameras and HUD (plan ch. 12).
//!
//! Full operability via the keyboard (MaSzyna principle); the clickable
//! 3D controls are added in M6.

use crate::mods_ui::ModManager;
use crate::settings::Gameplay;
use crate::streaming::TerrainStreamer;
use crate::{Origin, PlayerTrain, SimResource, TerrainInfo, ViewDistance};
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};
use i18n::t;
use sim_core::brakes::DriverBrakeValve;
use sim_core::safety::{LampState, SafetySystems, SelfTestPhase};

/// Camera of the player.
#[derive(Component)]
pub struct CabCamera;

/// Text node of the HUD.
#[derive(Component)]
pub struct HudText;

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

/// Create the HUD text.
pub fn spawn_hud(commands: &mut Commands) {
    // Crosshair of the walk — the caught cursor is invisible, this shows where it points.
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
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: bevy::text::FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(12.0),
            left: Val::Px(12.0),
            ..default()
        },
        HudText,
    ));
}

/// Fill the HUD with speedometer, brake pressures and train protection displays (plan 16.3).
#[allow(clippy::too_many_arguments)]
pub fn update_hud(
    sim: Res<SimResource>,
    player: Res<PlayerTrain>,
    terrain: Res<TerrainInfo>,
    streamer: Res<TerrainStreamer>,
    view: Res<ViewDistance>,
    mouse: Res<crate::cab::CabMouse>,
    gameplay: Res<Gameplay>,
    camera: Res<CameraState>,
    mut query: Query<(&mut Text, &mut Visibility), With<HudText>>,
) {
    let Ok((mut text, mut visibility)) = query.single_mut() else {
        return;
    };
    // Switched off in the settings: hide it and skip the whole readout — nothing else
    // reads the text node.
    *visibility = if gameplay.hud {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if !gameplay.hud {
        return;
    }
    let sim = &sim.0;
    let train = &sim.trains[player.0];
    let loco = &train.vehicles[0];
    let runtime = &sim.runtime[player.0];
    let cab = &sim.controls[player.0];

    let mut lines = Vec::new();
    let clock = sim.clock().rem_euclid(sim_core::timetable::DAY);
    lines.push(t!(
        "hud-speed",
        speed = format!("{:6.1}", train.speed_kmh()),
        limit = format!("{:5.0}", loco.pos.speed_limit(&sim.net)),
        distance = format!("{:8.0}", runtime.odometer),
        time = format!(
            "{:02}:{:02}:{:02}",
            (clock / 3600.0) as u32,
            (clock / 60.0) as u32 % 60,
            clock as u32 % 60
        ),
    ));
    lines.push(t!(
        "hud-brakes",
        pipe = format!("{:4.2}", loco.brake.pipe),
        cylinder = format!("{:4.2}", loco.brake.cylinder),
        auxiliary = format!("{:4.2}", loco.brake.aux_reservoir),
        main = format!("{:5.2}", loco.brake.main_reservoir),
        direct = format!("{:4.2}", loco.brake.direct_cylinder),
        air = format!("{:6.0}", loco.brake.air_consumed),
    ));
    lines.push(t!(
        "hud-traction",
        throttle = format!("{:+.2}", cab.throttle),
        tractive = format!("{:6.0}", loco.tractive_effort / 1000.0),
        braking = format!(
            "{:6.0}",
            train.vehicles.iter().map(|v| v.brake_effort).sum::<f64>() / 1000.0
        ),
        valve = format!("{:?}", cab.brake_valve),
    ));
    // The running gear, but only when it has something to say: an axle spinning or
    // sliding is what the line is there for, and a train running cleanly has none.
    let slipping = loco.axles.iter().filter(|a| a.slip.abs() > 0.02).count();
    if slipping > 0 {
        lines.push(t!(
            "hud-axles",
            slipping = slipping,
            axles = loco.axles.len(),
            worst = format!("{:+4.2}", loco.slip),
            state = t!(if loco.slip > 0.0 {
                "hud-axles-spinning"
            } else {
                "hud-axles-sliding"
            }),
        ));
    }
    if train.vehicles.get(train.cab).is_some_and(|v| v.spec.afb) {
        lines.push(t!(
            "hud-afb",
            state = onoff(cab.afb),
            target = format!("{:3.0}", cab.afb_target),
        ));
    }
    lines.push(t!(
        "hud-electrics",
        battery = onoff(loco.traction.battery),
        pantograph = format!("{:.0}", loco.traction.pantograph * 100.0),
        switch = onoff(loco.traction.main_switch),
        voltage = format!("{:5.0}", loco.traction.line_voltage),
        parking = onoff(loco.brake.parking_applied),
    ));
    // Whatever the drive of this vehicle has to say about itself.
    // ponytail: the HUD reads the first chain — one drive block per vehicle in the UI.
    let drive = loco.traction.drives[0];
    match loco.spec.traction() {
        Some(sim_core::drive::TractionSpec::TapChanger { steps, .. }) => lines.push(t!(
            "hud-tap",
            step = format!("{:4.1}", drive.step),
            steps = steps,
            current = format!("{:5.0}", drive.motor_current),
            field = format!("{:3.0}", drive.field * 100.0),
            force = format!("{:5.0}", drive.dynamic_force / 1000.0),
        )),
        Some(sim_core::drive::TractionSpec::Diesel { electric, .. }) => {
            lines.push(t!(
                "hud-diesel",
                rpm = format!("{:5.0}", drive.engine_rpm),
                fill = format!("{:3.0}", drive.engine_fill * 100.0),
                circuit = drive.circuit + 1,
                nu = format!("{:4.2}", drive.circuit_nu),
                retarder = format!("{:3.0}", drive.retarder_fill * 100.0),
            ));
            // A diesel-electric drive is read at the generator, not at the transmission.
            if electric.is_some() {
                lines.push(t!(
                    "hud-generator",
                    voltage = format!("{:5.0}", drive.generator_voltage),
                    current = format!("{:5.0}", drive.motor_current),
                    regulator = format!("{:3.0}", drive.regulator * 100.0),
                ));
            }
        }
        // The boiler is the instrument panel of a steam locomotive; without it there is
        // nothing to drive by.
        Some(sim_core::drive::TractionSpec::Steam { loco: steam, .. }) => {
            if let Some(boiler) = drive.steam {
                lines.push(t!(
                    "hud-boiler",
                    pressure = format!("{:4.1}", boiler.pressure),
                    glass = format!("{:3.0}", boiler.glass(steam).clamp(0.0, 1.0) * 100.0),
                    fire = format!("{:3.0}", boiler.fire_intensity * 100.0),
                    coal = format!("{:4.0}", boiler.fire_mass),
                ));
                lines.push(t!(
                    "hud-tender",
                    water = format!("{:6.0}", boiler.tender_water),
                    coal = format!("{:5.0}", boiler.tender_coal),
                    blowing = onoff(boiler.blowing_off),
                ));
            }
        }
        Some(_) => lines.push(t!(
            "hud-dynamic",
            force = format!("{:5.0}", drive.dynamic_force / 1000.0)
        )),
        None => {}
    }
    // Temperature only shows up where something can actually get hot — a vehicle without a
    // cooling system in its diagram would just show the ambient figure for ever.
    if drive.peak_temp() > sim_core::electric::AMBIENT_TEMP + 1.0 {
        lines.push(t!(
            "hud-temperature",
            motor = format!("{:3.0}", drive.motor_temp),
            resistor = format!("{:3.0}", drive.resistor_temp.max(drive.brake_resistor_temp)),
        ));
    }

    // Train protection.
    let lamps: Vec<String> = loco
        .safety
        .indicators()
        .iter()
        .filter(|i| i.lamp != LampState::Off)
        .map(|i| match i.lamp {
            LampState::Blinking => format!("{}*", i.name),
            _ => i.name.to_string(),
        })
        .collect();
    lines.push(t!(
        "hud-protection",
        action = format!("{:?}", runtime.protection.action),
        limit = runtime
            .protection
            .speed_limit
            .map(|v| format!("{v:.0} km/h"))
            .unwrap_or_else(|| t!("common-none")),
        lamps = if lamps.is_empty() {
            t!("common-none")
        } else {
            lamps.join(" ")
        },
    ));
    if let SafetySystems::De(de) = &loco.safety {
        if let Some(pzb) = de.pzb {
            lines.push(t!(
                "hud-pzb",
                variant = pzb.variant.name(),
                category = format!("{:?}", pzb.train_type),
                note = match pzb.self_test().phase() {
                    SelfTestPhase::Passed if pzb.is_restrictive() => t!("hud-pzb-restrictive"),
                    SelfTestPhase::Passed => String::new(),
                    p => t!("hud-pzb-selftest", phase = format!("{p:?}")),
                },
            ));
        }
        if let Some(lzb) = de.lzb {
            if !lzb.self_test().is_passed() {
                lines.push(t!(
                    "hud-lzb-selftest",
                    phase = format!("{:?}", lzb.self_test().phase())
                ));
            } else if lzb.is_guiding() {
                lines.push(t!(
                    "hud-lzb",
                    mode = format!("{:?}", lzb.mode),
                    block = format!("{:?}", lzb.block_mode()),
                    cirelke = if lzb.is_cir_elke() { " CIR-ELKE" } else { "" },
                    permitted = format!("{:5.0}", lzb.permitted_speed().unwrap_or(0.0)),
                    target = format!("{:5.0}", lzb.target_speed().unwrap_or(0.0)),
                    distance = format!("{:6.0}", lzb.target_distance().unwrap_or(0.0)),
                ));
            }
        }
    }

    // Signals ahead.
    let aspects: Vec<String> = sim
        .interlock
        .signals
        .iter()
        .map(|s| {
            format!(
                "{:?}{}",
                s.aspect.main.map(|m| format!("{m:?}")).unwrap_or_default(),
                s.aspect
                    .distant
                    .map(|d| format!("/{d:?}"))
                    .unwrap_or_default()
            )
        })
        .collect();
    lines.push(t!("hud-signals", aspects = aspects.join("  ")));
    lines.push(t!(
        "hud-terrain",
        tiles = terrain.0.tiles,
        pending = streamer.pending_tiles(),
        triangles = terrain.0.triangles,
        megabytes = format!("{:.1}", terrain.0.memory() as f64 / 1e6),
        view = format!("{:.0}", view.0),
    ));

    // Scenario: timetable, recent messages, scoring.
    if !sim.scenario.scenario.name.is_empty() {
        lines.push(String::new());
        lines.push(t!(
            "hud-scenario",
            number = sim.score.timetable.number,
            name = sim.scenario.scenario.name
        ));
        for m in sim.scenario.recent_messages(3) {
            let marker = if m.announcement { "»" } else { "•" };
            lines.push(format!("{marker} [{:.0} s] {}", m.time, m.text));
        }
        if let Some(outcome) = &sim.scenario.outcome {
            lines.push(t!(
                "hud-outcome",
                result = if outcome.success {
                    t!("hud-scenario-passed")
                } else {
                    t!("hud-scenario-failed")
                },
                reason = outcome.reason,
            ));
            lines.push(sim.score.report(sim.scenario.bonus).summary());
        } else {
            let report = sim.score.report(sim.scenario.bonus);
            lines.push(t!(
                "hud-score",
                total = report.total,
                forced = sim.score.forced_brakes,
                energy = format!("{:.0}", sim.score.energy_kwh),
            ));
        }
        lines.push(String::new());
    }

    // The cab control under the mouse, with its position in percent.
    if let Some((key, value)) = mouse.hover_info {
        lines.push(t!(
            "hud-control",
            name = t!(key),
            value = format!("{:3.0}", value * 100.0)
        ));
    }

    lines.push(if camera.mode == CameraMode::Walk {
        t!("hud-keys-walk")
    } else {
        t!("hud-keys-drive")
    });
    lines.push(t!("hud-keys-safety"));
    lines.push(t!("hud-keys-lights"));

    **text = lines.join("\n");
}

fn onoff(b: bool) -> String {
    if b { t!("common-on") } else { t!("common-off") }
}
