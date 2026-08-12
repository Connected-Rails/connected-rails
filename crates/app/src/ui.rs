//! Input, cameras and HUD (plan ch. 12).
//!
//! Full operability via the keyboard (MaSzyna principle); the clickable
//! 3D controls are added in M6.

use crate::{Origin, PlayerTrain, SimResource, TerrainInfo, ViewDistance};
use bevy::prelude::*;
use sim_core::brakes::DriverBrakeValve;
use sim_core::safety::{LampState, SafetySystems};

/// Camera of the player.
#[derive(Component)]
pub struct CabCamera;

/// Text node of the HUD.
#[derive(Component)]
pub struct HudText;

/// Camera perspectives (plan 12.4).
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq)]
pub enum CameraMode {
    /// View from the cab.
    #[default]
    Cab,
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

/// Key bindings → cab inputs.
pub fn player_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut sim: ResMut<SimResource>,
    player: Res<PlayerTrain>,
    time: Res<Time>,
) {
    let dt = time.delta_secs_f64();
    let index = player.0;
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

    // Sifa and train protection.
    cab.sifa = keys.pressed(KeyCode::Space);
    cab.pzb_acknowledge = keys.pressed(KeyCode::PageDown);
    cab.pzb_exempt = keys.pressed(KeyCode::End);
    cab.pzb_override = keys.pressed(KeyCode::Delete);
    cab.lzb_takeover = keys.pressed(KeyCode::KeyN);
    cab.lzb_end = keys.pressed(KeyCode::KeyM);
    cab.horn = keys.pressed(KeyCode::KeyH);

    // Preparation: battery, pantograph, main switch, compressor.
    let train = &mut sim.0.trains[index];
    for v in &mut train.vehicles {
        if v.spec.traction.is_none() {
            continue;
        }
        if keys.just_pressed(KeyCode::Digit1) {
            v.traction.battery = !v.traction.battery;
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
}

/// Camera control: F1/F2/F3 switch the perspective, arrow keys pan.
pub fn camera_control(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    sim: Res<SimResource>,
    origin: Res<Origin>,
    player: Res<PlayerTrain>,
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
    let turn = 1.2 * dt;
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

    let Ok(mut transform) = camera.single_mut() else {
        return;
    };
    let train = &sim.0.trains[player.0];
    let pose = train.vehicles[0].pos.pose(&sim.0.net);
    let pos = origin.0.to_render(pose.pos);
    let up = origin.0.dir_to_render(pose.up);
    let forward = origin.0.dir_to_render(pose.tangent);
    let right = forward.cross(up).normalize_or_zero();

    match state.mode {
        CameraMode::Cab => {
            // Cab window: 8 m ahead of the vehicle centre, 2.8 m above the rail head.
            let eye = pos + forward * 8.0 + up * 2.8 - right * 0.6;
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

/// Create the HUD text.
pub fn spawn_hud(commands: &mut Commands) {
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
pub fn update_hud(
    sim: Res<SimResource>,
    player: Res<PlayerTrain>,
    terrain: Res<TerrainInfo>,
    view: Res<ViewDistance>,
    mut query: Query<&mut Text, With<HudText>>,
) {
    let Ok(mut text) = query.single_mut() else {
        return;
    };
    let sim = &sim.0;
    let train = &sim.trains[player.0];
    let loco = &train.vehicles[0];
    let runtime = &sim.runtime[player.0];
    let cab = &sim.controls[player.0];

    let mut lines = Vec::new();
    lines.push(format!(
        "v = {:6.1} km/h   zul. {:5.0} km/h   Weg {:8.0} m   t = {:6.0} s",
        train.speed_kmh(),
        loco.pos.speed_limit(&sim.net),
        runtime.odometer,
        sim.time
    ));
    lines.push(format!(
        "HL {:4.2} bar   C {:4.2} bar   R {:4.2} bar   Zusatz {:4.2} bar",
        loco.brake.pipe, loco.brake.cylinder, loco.brake.aux_reservoir, loco.brake.direct_cylinder
    ));
    lines.push(format!(
        "Fahrschalter {:+.2}   Zugkraft {:6.0} kN   Bremskraft {:6.0} kN   Bremse {:?}",
        cab.throttle,
        loco.tractive_effort / 1000.0,
        train.vehicles.iter().map(|v| v.brake_effort).sum::<f64>() / 1000.0,
        cab.brake_valve
    ));
    lines.push(format!(
        "Batterie {}   Bügel {:.0}%   Hauptschalter {}   Fahrdraht {:5.0} V",
        onoff(loco.traction.battery),
        loco.traction.pantograph * 100.0,
        onoff(loco.traction.main_switch),
        loco.traction.line_voltage
    ));

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
    lines.push(format!(
        "Zugsicherung: {:?}   Überwachung {}   LM: {}",
        runtime.protection.action,
        runtime
            .protection
            .speed_limit
            .map(|v| format!("{v:.0} km/h"))
            .unwrap_or_else(|| "—".into()),
        if lamps.is_empty() {
            "—".to_string()
        } else {
            lamps.join(" ")
        }
    ));
    if let SafetySystems::De(de) = &loco.safety
        && let Some(lzb) = de.lzb
        && lzb.is_guiding()
    {
        lines.push(format!(
            "LZB {:?}: v-Soll {:5.0}   v-Ziel {:5.0}   Zielentfernung {:6.0} m",
            lzb.mode,
            lzb.permitted_speed().unwrap_or(0.0),
            lzb.target_speed().unwrap_or(0.0),
            lzb.target_distance().unwrap_or(0.0)
        ));
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
    lines.push(format!("Signale: {}", aspects.join("  ")));
    lines.push(format!(
        "Gelände: {} Kacheln, {} Dreiecke, {:.1} MB, Sichtweite {:.0} m",
        terrain.0.tiles,
        terrain.0.triangles,
        terrain.0.memory() as f64 / 1e6,
        view.0
    ));

    // Scenario: timetable, recent messages, scoring.
    if !sim.scenario.scenario.name.is_empty() {
        lines.push(String::new());
        lines.push(format!(
            "{} — {}",
            sim.score.timetable.number, sim.scenario.scenario.name
        ));
        for m in sim.scenario.recent_messages(3) {
            let marker = if m.announcement { "»" } else { "•" };
            lines.push(format!("{marker} [{:.0} s] {}", m.time, m.text));
        }
        if let Some(outcome) = &sim.scenario.outcome {
            lines.push(format!(
                "{}: {}",
                if outcome.success {
                    "Szenario bestanden"
                } else {
                    "Szenario gescheitert"
                },
                outcome.reason
            ));
            lines.push(sim.score.report(sim.scenario.bonus).summary());
        } else {
            let report = sim.score.report(sim.scenario.bonus);
            lines.push(format!(
                "Wertung {} | Zwangsbremsungen {} | {:.0} kWh",
                report.total, sim.score.forced_brakes, sim.score.energy_kwh
            ));
        }
        lines.push(String::new());
    }

    lines.push(
        "W/S Fahrschalter  A/D Bremse  E Schnellbremsung  Q Abschluss  Z Füllen  C/V Zusatzbremse"
            .into(),
    );
    lines.push(
        "Leertaste Sifa  Bild↓ Wachsam  Ende Frei  Entf Befehl  N/M LZB  1–4 Aufrüsten  F1–F3 Kamera"
            .into(),
    );

    **text = lines.join("\n");
}

fn onoff(b: bool) -> &'static str {
    if b { "ein" } else { "aus" }
}
