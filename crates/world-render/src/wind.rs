//! Wind turbines in motion: the rotor turns, the nacelle yaws, the lamp blinks.
//!
//! A turbine model (`mods/wind`, `tools/wind`) is a scene with two moving
//! nodes — `nacelle` at hub height and `rotor` in front of it — and, where the
//! machine is tall enough to carry the aviation marking, a `blink` node under
//! its `_NIGHT` lamps. Nothing in the line file says how they move; the
//! **weather** does, and the weather is a shared function of the scenario
//! clock (`sim_core::weather`), so two clients of a multiplayer run see the
//! same park turning the same way without a byte crossing the network.
//!
//! What moves and why:
//!
//! - **The rotor** turns at the speed the wind at hub height gives it: a
//!   tip-speed ratio of about seven below rated power, the rated speed above
//!   it, idling under the cut-in wind and stopped over the cut-out. The speed
//!   follows the target over a few seconds — a rotor has inertia — and the
//!   phase accumulates locally. It is not state: nobody can compare the
//!   blade angle of one client with another's, so nothing is sent.
//! - **The nacelle** yaws to the wind's bearing, slowly and with a dead band,
//!   the way a yaw drive does. The line file's own bearing is where it starts.
//! - **The lamp** (Feuer W, rot) blinks on the scenario clock — one second on,
//!   half a second off — so every machine of a park blinks in step, which is
//!   what the regulation asks of a real park and what the eye expects.
//!
//! The rotor's size and rated speed come off its node's glTF extras, so a
//! model that is scaled at placement is read at the size it stands at.

use bevy::gltf::GltfExtras;
use bevy::prelude::*;

use crate::sky::Sky;

/// The wind below which a rotor only idles [m/s], and above which it is
/// stopped and feathered.
const CUT_IN: f32 = 3.0;
const CUT_OUT: f32 = 25.0;
/// The idling speed under the cut-in wind [rpm] — a rotor that is free wheels.
const IDLE_RPM: f32 = 1.5;
/// Blade tip speed over wind speed below rated power. Six to eight on every
/// modern machine; seven is the middle of the fleet.
const TIP_SPEED_RATIO: f32 = 7.0;
/// How fast the rotor speed follows its target [s] — the inertia of a rotor
/// weighing tens of tonnes.
const SPIN_UP: f32 = 8.0;
/// The wind's growth with height over open country: the Hellmann exponent of
/// farmland with hedges. The ten-metre wind the weather reports is half again
/// as strong a hundred metres up.
const HELLMANN: f32 = 0.2;
/// How fast a yaw drive turns the nacelle [deg/s], and the error it tolerates
/// before it bothers [deg].
const YAW_RATE: f32 = 0.5;
const YAW_DEAD_BAND: f32 = 4.0;
/// The lamp's blink: one second on, half a second off (Feuer W, rot).
const BLINK_PERIOD: f64 = 1.5;
const BLINK_ON: f64 = 1.0;

/// The rotor node of a turbine: what it is, and how it is turning.
#[derive(Component, Debug, Clone)]
pub struct Rotor {
    /// The node's own rotation in the model — the axis tilt — that the spin
    /// composes with.
    pub base: Quat,
    /// Rotor radius [m] and hub height [m] as the model was built; the
    /// placement's scale is read off the transform.
    pub radius: f32,
    pub hub_height: f32,
    /// The speed at rated power [rad/s].
    pub rated: f32,
    /// Where the blades are [rad] and how fast they go [rad/s].
    pub angle: f32,
    pub omega: f32,
    /// A little slower or faster than the neighbour: no two machines of a park
    /// ever turn in step.
    pub trim: f32,
}

/// The nacelle node of a turbine — what yaws.
#[derive(Component, Debug, Clone, Copy)]
pub struct Nacelle;

/// The `blink` node under a turbine's lamps.
#[derive(Component, Debug, Clone, Copy)]
pub struct ObstructionLight;

/// What the rotor node's extras say about the machine.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RotorSpec {
    pub rotor_diameter: f32,
    pub rated_rpm: f32,
    pub hub_height: f32,
}

/// Reads the rotor's extras (`{"rotor_diameter": 80, "rated_rpm": 18,
/// "hub_height": 95}`). A node without them is not a turbine's rotor.
pub fn parse_rotor(extras: &str) -> Option<RotorSpec> {
    let value: serde_json::Value = serde_json::from_str(extras).ok()?;
    let number = |key: &str| value.get(key)?.as_f64().map(|v| v as f32);
    let spec = RotorSpec {
        rotor_diameter: number("rotor_diameter")?,
        rated_rpm: number("rated_rpm")?,
        hub_height: number("hub_height")?,
    };
    (spec.rotor_diameter > 0.0 && spec.rated_rpm > 0.0 && spec.hub_height > 0.0).then_some(spec)
}

/// Finds the moving nodes of freshly spawned turbine scenes by name.
pub fn bind_parts(
    mut commands: Commands,
    fresh: Query<(Entity, &Name, &Transform, Option<&GltfExtras>), Added<Name>>,
) {
    for (entity, name, transform, extras) in &fresh {
        match name.as_str() {
            "rotor" => {
                let Some(spec) = extras.and_then(|e| parse_rotor(&e.value)) else {
                    continue;
                };
                // The phase off the entity id, which is arbitrary — what
                // matters is that two turbines of a park do not start
                // together, and that a rebuilt tile does not restart them
                // visibly in step.
                let seed =
                    entity.to_bits().wrapping_mul(2_654_435_761) as u32 as f32 / u32::MAX as f32;
                commands.entity(entity).try_insert(Rotor {
                    base: transform.rotation,
                    radius: spec.rotor_diameter / 2.0,
                    hub_height: spec.hub_height,
                    rated: spec.rated_rpm * std::f32::consts::TAU / 60.0,
                    angle: seed * std::f32::consts::TAU,
                    omega: 0.0,
                    trim: 0.97 + 0.06 * ((seed * 7.0).fract()),
                });
            }
            "nacelle" => {
                commands.entity(entity).try_insert(Nacelle);
            }
            "blink" => {
                commands
                    .entity(entity)
                    .try_insert((ObstructionLight, Visibility::Inherited));
            }
            _ => {}
        }
    }
}

/// The wind at hub height from the ten-metre wind the weather reports [m/s].
pub fn wind_at(wind_10m: f32, hub_height: f32) -> f32 {
    wind_10m * (hub_height.max(10.0) / 10.0).powf(HELLMANN)
}

/// The speed a rotor of `radius` settles at in this wind [rad/s].
///
/// Below the cut-in it idles, above the cut-out it is stopped — the blades
/// are feathered and the brake is on — and in between the tip runs at seven
/// times the wind until the machine reaches its rated speed, where the pitch
/// control holds it.
pub fn rotor_target(wind: f32, radius: f32, rated: f32) -> f32 {
    if wind >= CUT_OUT {
        0.0
    } else if wind < CUT_IN {
        IDLE_RPM * std::f32::consts::TAU / 60.0 * (wind / CUT_IN).clamp(0.0, 1.0)
    } else {
        (TIP_SPEED_RATIO * wind / radius.max(1.0)).min(rated)
    }
}

/// Turns every rotor: the speed towards what the wind asks, the angle on.
pub fn turn_rotors(
    time: Res<Time>,
    sky: Option<Res<Sky>>,
    mut rotors: Query<(&mut Rotor, &mut Transform, &GlobalTransform)>,
) {
    let dt = time.delta_secs();
    // Without a sky — the route editor's preview — a steady breeze, so the
    // park turns and a builder sees that it does.
    let wind_10m = sky.as_ref().map_or(6.0, |sky| sky.weather.wind);
    for (mut rotor, mut transform, global) in &mut rotors {
        let scale = global.compute_transform().scale.y.max(0.01);
        let wind = wind_at(wind_10m, rotor.hub_height * scale);
        let target = rotor_target(wind, rotor.radius * scale, rotor.rated / scale) * rotor.trim;
        let k = (dt / SPIN_UP).min(1.0);
        rotor.omega += (target - rotor.omega) * k;
        rotor.angle = (rotor.angle + rotor.omega * dt).rem_euclid(std::f32::consts::TAU);
        // Clockwise seen from the wind, the way every three-blade machine
        // turns — which in the model's frame, looked at from −Z, is a negative
        // turn about Z.
        transform.rotation = rotor.base * Quat::from_rotation_z(-rotor.angle);
    }
}

/// The bearing a rotation's front (−Z) faces in render space [rad, clockwise
/// from north] — render space is +x east and −z north.
pub fn bearing_of(rotation: Quat) -> f32 {
    let forward = rotation * Vec3::NEG_Z;
    forward.x.atan2(-forward.z)
}

/// How far the nacelle turns this frame [rad] towards the wind: nothing
/// inside the dead band, the drive's rate outside it, and never past the
/// target.
pub fn yaw_step(current: f32, target: f32, dt: f32) -> f32 {
    let error = (target - current + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;
    if error.abs() < YAW_DEAD_BAND.to_radians() {
        return 0.0;
    }
    let step = YAW_RATE.to_radians() * dt;
    error.clamp(-step, step)
}

/// Yaws every nacelle towards where the wind comes from.
pub fn yaw_nacelles(
    time: Res<Time>,
    sky: Option<Res<Sky>>,
    mut nacelles: Query<(&mut Transform, &GlobalTransform), With<Nacelle>>,
) {
    let Some(sky) = sky else {
        return;
    };
    let target = sky.weather.bearing;
    let dt = time.delta_secs();
    for (mut transform, global) in &mut nacelles {
        let current = bearing_of(global.rotation());
        let step = yaw_step(current, target, dt);
        if step != 0.0 {
            // A turn about +Y takes the front towards the west, which is a
            // smaller bearing — so a bearing that has to grow is a negative
            // turn.
            transform.rotate_local_y(-step);
        }
    }
}

/// Whether the lamp is on at this second of the scenario clock.
pub fn lamp_lit(seconds: f64) -> bool {
    seconds.rem_euclid(BLINK_PERIOD) < BLINK_ON
}

/// Blinks the lamps on the scenario clock — every machine in step.
pub fn blink_lights(
    time: Res<Time>,
    sky: Option<Res<Sky>>,
    mut lamps: Query<&mut Visibility, With<ObstructionLight>>,
) {
    let seconds = sky
        .as_ref()
        .map_or(time.elapsed_secs_f64(), |sky| sky.seconds);
    let wanted = if lamp_lit(seconds) {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut visibility in &mut lamps {
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rotor_extras_are_read() {
        let spec = parse_rotor(r#"{"rotor_diameter": 80, "rated_rpm": 18, "hub_height": 95}"#)
            .expect("a rotor");
        assert_eq!(spec.rotor_diameter, 80.0);
        assert_eq!(spec.rated_rpm, 18.0);
        assert_eq!(spec.hub_height, 95.0);
        // A walkway node's extras are not a rotor's.
        assert_eq!(parse_rotor(r#"{"people": 6}"#), None);
        assert_eq!(parse_rotor("not json"), None);
    }

    #[test]
    fn the_wind_grows_with_height() {
        assert_eq!(wind_at(6.0, 10.0), 6.0);
        let hub = wind_at(6.0, 100.0);
        assert!((9.3..9.7).contains(&hub), "{hub}");
    }

    /// An 80 m rotor rated at 18 rpm: idle in a breath of wind, on the tip
    /// speed ratio in a breeze, held at rated speed in a strong wind, stopped
    /// in a storm.
    #[test]
    fn the_rotor_follows_the_wind() {
        let radius = 40.0;
        let rated = 18.0 * std::f32::consts::TAU / 60.0;
        assert_eq!(rotor_target(0.0, radius, rated), 0.0);
        let idle = rotor_target(2.0, radius, rated);
        assert!(idle > 0.0 && idle < 0.2, "{idle}");
        let breeze = rotor_target(6.0, radius, rated);
        // 7 × 6 / 40 = 1.05 rad/s, ten revolutions a minute.
        assert!((1.0..1.1).contains(&breeze), "{breeze}");
        assert_eq!(rotor_target(14.0, radius, rated), rated);
        assert_eq!(rotor_target(26.0, radius, rated), 0.0);
    }

    #[test]
    fn a_bearing_is_read_off_the_front() {
        let north = bearing_of(Quat::IDENTITY);
        assert!(north.abs() < 1e-5);
        // A turn of −90° about Y puts the front towards +x, which is east.
        let east = bearing_of(Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2));
        assert!((east - std::f32::consts::FRAC_PI_2).abs() < 1e-5, "{east}");
    }

    #[test]
    fn the_yaw_drive_turns_slowly_and_the_short_way() {
        let deg = |d: f32| d.to_radians();
        // Inside the dead band nothing happens.
        assert_eq!(yaw_step(deg(250.0), deg(252.0), 1.0), 0.0);
        // Outside it, the drive's rate — and never past the target.
        let step = yaw_step(deg(250.0), deg(270.0), 1.0);
        assert!((step - deg(0.5)).abs() < 1e-6, "{step}");
        assert!((yaw_step(deg(250.0), deg(250.2 + 4.0), 100.0) - deg(4.2)).abs() < 1e-5);
        // The short way round the compass.
        assert!(yaw_step(deg(10.0), deg(350.0), 1.0) < 0.0);
    }

    #[test]
    fn the_lamp_blinks_on_the_clock() {
        assert!(lamp_lit(0.0));
        assert!(lamp_lit(0.9));
        assert!(!lamp_lit(1.2));
        assert!(lamp_lit(1.5));
        assert!(!lamp_lit(1.5 * 1000.0 + 1.4));
    }
}

#[cfg(test)]
mod scene_tests {
    use super::*;
    use crate::scatter::{LodsApplied, SceneLods, apply_scene_lods};
    use crate::{Daylight, NightNode, switch_night_nodes};
    use bevy::camera::visibility::VisibilityRange;
    use std::sync::Arc;

    /// The lamp of a spawned turbine at night: the `_NIGHT` node is shown,
    /// the `blink` node is bound and switched, the lamp mesh carries the
    /// level's band. This is the hierarchy `tools/wind` writes, run through
    /// the real systems.
    #[test]
    fn a_turbines_lamp_is_lit_at_night() {
        let mut app = App::new();
        app.add_plugins(bevy::time::TimePlugin);
        app.insert_resource(Daylight(0.0));
        app.add_systems(
            Update,
            (
                switch_night_nodes,
                bind_parts,
                apply_scene_lods,
                blink_lights,
            )
                .chain(),
        );
        let root = app
            .world_mut()
            .spawn((
                SceneLods(Arc::from([637.0f32, 1530.0, 3650.0, 8000.0].as_slice())),
                Transform::default(),
                Visibility::default(),
            ))
            .id();
        let nacelle = app
            .world_mut()
            .spawn((
                Name::new("nacelle"),
                Transform::default(),
                Visibility::default(),
                ChildOf(root),
            ))
            .id();
        let night = app
            .world_mut()
            .spawn((
                Name::new("feuer_NIGHT"),
                Transform::default(),
                Visibility::default(),
                ChildOf(nacelle),
            ))
            .id();
        let blink = app
            .world_mut()
            .spawn((
                Name::new("blink"),
                Transform::default(),
                Visibility::default(),
                ChildOf(night),
            ))
            .id();
        let level = app
            .world_mut()
            .spawn((
                Name::new("lampe_LOD0"),
                Transform::default(),
                Visibility::default(),
                ChildOf(blink),
            ))
            .id();
        let mesh = app
            .world_mut()
            .spawn((
                Mesh3d(Handle::default()),
                Transform::default(),
                Visibility::default(),
                ChildOf(level),
            ))
            .id();
        // A second level, so the first one has somewhere to hand over to.
        let coarse = app
            .world_mut()
            .spawn((
                Name::new("lampe_LOD1"),
                Transform::default(),
                Visibility::default(),
                ChildOf(blink),
            ))
            .id();
        app.world_mut().spawn((
            Mesh3d(Handle::default()),
            Transform::default(),
            Visibility::default(),
            ChildOf(coarse),
        ));
        app.update();
        app.update();

        let world = app.world();
        assert!(
            world.get::<NightNode>(night).is_some(),
            "the lamp node is night furniture"
        );
        assert_eq!(
            *world.get::<Visibility>(night).expect("visibility"),
            Visibility::Inherited
        );
        assert!(
            world.get::<ObstructionLight>(blink).is_some(),
            "blink is bound"
        );
        assert!(world.get::<Nacelle>(nacelle).is_some());
        assert!(world.get::<LodsApplied>(root).is_some());
        let range = world
            .get::<VisibilityRange>(mesh)
            .expect("the mesh has its band");
        assert_eq!(range.end_margin.start, 637.0);
        // Two frames in, the clock is under a second: the lamp is on.
        assert_eq!(
            *world.get::<Visibility>(blink).expect("visibility"),
            Visibility::Inherited
        );
    }
}
