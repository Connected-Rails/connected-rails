//! The driver on foot (plan ch. 12): away from the seat he walks the vehicle, through
//! the gangway into the next one, out of a door and over platform, ballast and forecourt.
//!
//! The character controller is a body that falls. One ray downwards carries it —
//! terrain, platform, the floor and the stairs of an interior — and one ahead at chest
//! height stops it. Ground that rises no higher than [`STEP_UP`] he climbs, anything
//! above that blocks, and where the ground drops away he falls until he lands. There is
//! no physics engine and no collision data behind this: the meshes that are drawn anyway
//! are what is asked, so a platform or a staircase needs nothing but its geometry.
//!
//! Where he stands is kept in the frame that moves him — inside a vehicle in its model
//! space, so he rides along by himself, outside in world coordinates, which an origin
//! rebase leaves alone. `--character` hangs a model on him (one of the mods' people by
//! default); it is only ever seen from the outside cameras, because in the walk the eye
//! sits inside its head. The model is animated off the same place: how far it moved
//! between two frames is his pace, and the pace picks and speeds the walk cycle.

use crate::bindings::{Action, Input};
use bevy::animation::transition::AnimationTransitions;
use bevy::picking::mesh_picking::ray_cast::{MeshRayCast, MeshRayCastSettings, RayCastVisibility};
use bevy::prelude::*;
use sim_core::doors::DoorPhase;
use sim_core::train::{Train, Vehicle};
use world_coords::{EcefPos, RenderOrigin};
use world_render::{CharacterGraphs, Dressed, Gait, Person, gait, play_gait};

use crate::ui::{CameraMode, CameraState};
use crate::{Origin, PlayerTrain, Precipitation, SimResource};

/// Eye above the floor the walker stands on [m].
const EYE_HEIGHT: f32 = 1.7;
/// Chest above that floor [m] — the height at which a wall stops him.
const CHEST: f32 = 1.3;
/// Shoulder width kept from a wall [m].
const RADIUS: f32 = 0.35;
/// Pace [m/s]: walking, and running with shift.
const WALK: f32 = 1.5;
const RUN: f32 = 5.0;
/// What he climbs without stairs [m].
// ponytail: a platform edge is 0.55–0.96 m and no route models a ramp, so he climbs it.
// Lower this the day platforms come with a way up.
const STEP_UP: f32 = 1.0;
/// Fall acceleration [m/s²] and the speed the fall is held at [m/s].
const GRAVITY: f32 = 9.81;
const TERMINAL: f32 = 40.0;
/// How far down ground is looked for [m] — deep enough for a fall off a bridge.
const DROP: f32 = 60.0;
/// Standing tolerance [m]: closer than this to the ground he is carried by it.
const CONTACT: f32 = 0.03;
/// Sideways distance from the middle of the vehicle at which its door can be used [m].
const DOOR_REACH: f32 = 3.5;
/// Where he is put down beside the vehicle when he gets off [m].
const STEP_ASIDE: f32 = 2.4;
/// Sideways distance from the middle of the vehicle he stands at inside [m].
const AISLE: f32 = 1.0;
/// Doors are only used at a stand [m/s].
const STANDSTILL: f32 = 0.2;
/// Half the width of the inside of a vehicle [m] — the bound for interiors that carry no
/// walls of their own to run a ray against.
const BODY_HALF_WIDTH: f32 = 1.4;
/// A step longer than this between two frames is not a walk but a teleport (a door,
/// a stand-up out of the seat) and is not measured [m].
const JUMP: f32 = 5.0;

/// The walker: where he is and how fast he is falling. `place` is `None` while he sits at
/// the desk — F4 stands him up, F1 puts him back.
#[derive(Resource, Default)]
pub struct Walker {
    pub place: Option<Place>,
    /// Vertical speed [m/s]: negative falls, zero stands.
    pub fall: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Place {
    /// In a vehicle: the eye in its model space (X right, Y above the rail head, −Z
    /// ahead), so it rides along without any bookkeeping.
    Aboard { vehicle: usize, eye: Vec3 },
    /// On the ground: the eye in world coordinates.
    Outside { eye: EcefPos },
}

/// The model `--character` hangs on the walker.
#[derive(Component)]
pub struct CharacterModel;

/// Walks the player: four bound keys or the left stick over the ground and through the
/// train, one more through a door.
// A Bevy system takes its resources as parameters — the argument count says nothing here.
#[allow(clippy::too_many_arguments)]
pub fn walk_player(
    input: Input,
    time: Res<Time>,
    sim: Res<SimResource>,
    mut player: ResMut<PlayerTrain>,
    origin: Res<Origin>,
    mut state: ResMut<CameraState>,
    mut walker: ResMut<Walker>,
    mut ray: MeshRayCast,
    precipitation: Query<(), With<Precipitation>>,
    console: Res<crate::console::Console>,
) {
    // The console holds the keyboard while it is open (`crate::console`) — W is a
    // letter there, not a step forward.
    if console.open {
        return;
    }
    match state.mode {
        // Back on the seat the body is gone; the next walk starts at the eye point again.
        CameraMode::Cab => {
            walker.place = None;
            walker.fall = 0.0;
            return;
        }
        CameraMode::Walk => {}
        // The outside and wayside cameras leave him standing where he is.
        _ => return,
    }
    let dt = time.delta_secs().min(0.1);
    let train = &sim.0.trains[player.0];
    if train.vehicles.is_empty() {
        return;
    }
    let seat = train.cab.min(train.vehicles.len() - 1);
    let mut place = walker.place.unwrap_or(Place::Aboard {
        vehicle: seat,
        eye: eye_point(&train.vehicles[seat]),
    });
    let pace = if input.pressed(Action::WalkRun) {
        RUN
    } else {
        WALK
    };
    // The rain field follows the camera, so its quads sit in the way of every ray.
    let filter = |entity: Entity| !precipitation.contains(entity);
    let door = input.just_pressed(Action::WalkDoor);

    // Everything below happens in render space, aboard as much as outside; the frame of
    // the vehicle only carries the walk in and out of it.
    let aboard = match place {
        Place::Aboard { vehicle, .. } => match train.vehicles.get(vehicle) {
            Some(riding) => Some((vehicle, frame(&sim.0, riding, &origin.0))),
            None => {
                walker.place = None;
                return;
            }
        },
        Place::Outside { .. } => None,
    };
    let mut feet = match (place, aboard) {
        (Place::Aboard { eye, .. }, Some((_, frame))) => {
            to_render(frame, eye - Vec3::Y * EYE_HEIGHT)
        }
        (Place::Outside { eye }, _) => origin.0.to_render(eye) - Vec3::Y * EYE_HEIGHT,
        // `aboard` is built from `place`, so the two always match.
        _ => return,
    };
    // Aboard the view angle counts from the vehicle, outside from north.
    let turn = match aboard {
        Some((_, (_, right, up, ahead))) => {
            model_rotation(right, up, ahead) * Quat::from_rotation_y(state.yaw)
        }
        None => Quat::from_rotation_y(state.yaw),
    };
    let direction = turn * pressed_direction(&input).normalize_or_zero();
    if direction.length_squared() > 0.5 {
        feet = step(&mut ray, feet, direction, pace * dt, &filter);
    }
    let mut ground = ground_below(&mut ray, feet + Vec3::Y * STEP_UP, &filter);
    if let Some((vehicle, frame)) = aboard {
        // The floor of the vehicle holds him up where its model carries no interior to
        // stand on: without it the ray finds the ballast below the box and he drops
        // through his own floor. Stairs above the floor still count.
        // ponytail: one floor per vehicle — a lower deck would need the interior itself
        // to say where its floors are.
        let floor = eye_point(&train.vehicles[vehicle]).y - EYE_HEIGHT;
        let deck = to_render(frame, to_model(frame, feet).with_y(floor)).y;
        ground = Some(ground.map_or(deck, |ground| ground.max(deck)));
    }
    let (height, fall) = fall_step(feet.y, ground, walker.fall, dt);
    feet.y = height;
    walker.fall = fall;

    // Back into the frame the walker is kept in.
    match aboard {
        Some((vehicle, frame)) => {
            let (vehicle, local) =
                gangway(|i| half_length(train, i), vehicle, to_model(frame, feet));
            let eye = Vec3::new(
                local.x.clamp(-BODY_HALF_WIDTH, BODY_HALF_WIDTH),
                local.y + EYE_HEIGHT,
                local.z,
            );
            place = Place::Aboard { vehicle, eye };
            let riding = &train.vehicles[vehicle];
            if door
                && door_usable(train, riding, eye.x)
                && let Some(outside) = beside(&mut ray, &sim.0, riding, &origin.0, eye, &filter)
            {
                state.yaw += heading(frame.3);
                place = Place::Outside { eye: outside };
                walker.fall = 0.0;
            }
        }
        None => {
            place = Place::Outside {
                eye: origin.0.from_render(feet + Vec3::Y * EYE_HEIGHT),
            };
            // The door of any train standing there, not only of the one he got out of:
            // walking from one train into another is how a driver changes trains at all
            // (`crate::crew`). Being aboard makes him a passenger; taking the levers is
            // a separate act.
            if door
                && let Some((boarded, vehicle, local, ahead)) =
                    door_within_reach(&sim.0, &origin.0, feet + Vec3::Y * EYE_HEIGHT)
            {
                state.yaw -= heading(ahead);
                place = Place::Aboard {
                    vehicle,
                    eye: local,
                };
                walker.fall = 0.0;
                player.0 = boarded;
            }
        }
    }
    walker.place = Some(place);
}

/// Puts the character model where the walker stands. It shows whenever the camera is not
/// looking out of its head — in the walk it would sit in the picture and in the way of
/// the walker's own ray casts.
pub fn place_character(
    walker: Res<Walker>,
    state: Res<CameraState>,
    sim: Res<SimResource>,
    player: Res<PlayerTrain>,
    origin: Res<Origin>,
    mut model: Query<(&mut Transform, &mut Visibility), With<CharacterModel>>,
) {
    let Ok((mut transform, mut visibility)) = model.single_mut() else {
        return;
    };
    let train = &sim.0.trains[player.0];
    let stance = walker
        .place
        .filter(|_| state.mode != CameraMode::Walk)
        .and_then(|place| match place {
            Place::Outside { eye } => Some((
                origin.0.to_render(eye) - Vec3::Y * EYE_HEIGHT,
                Quat::from_rotation_y(state.yaw),
            )),
            Place::Aboard { vehicle, eye } => {
                let riding = train.vehicles.get(vehicle)?;
                let frame = frame(&sim.0, riding, &origin.0);
                let (_, right, up, ahead) = frame;
                Some((
                    to_render(frame, eye - Vec3::Y * EYE_HEIGHT),
                    model_rotation(right, up, ahead) * Quat::from_rotation_y(state.yaw),
                ))
            }
        });
    match stance {
        Some((position, rotation)) => {
            transform.translation = position;
            transform.rotation = rotation;
            *visibility = Visibility::Inherited;
        }
        None => *visibility = Visibility::Hidden,
    }
}

/// How far the walker moved over the ground between two frames [m], measured in the
/// frame he is kept in — aboard in the vehicle's model space, so a train under way
/// does not read as a walk, outside in world coordinates through the current origin,
/// so a rebase does not either. `None` where the two places are not comparable
/// (boarding, alighting, sitting down) or the step is a jump.
pub fn horizontal_step(
    previous: Option<Place>,
    current: Option<Place>,
    origin: &RenderOrigin,
) -> Option<f32> {
    let step = match (previous?, current?) {
        (
            Place::Aboard {
                vehicle: a,
                eye: from,
            },
            Place::Aboard {
                vehicle: b,
                eye: to,
            },
        ) if a == b => to - from,
        (Place::Outside { eye: from }, Place::Outside { eye: to }) => {
            origin.to_render(to) - origin.to_render(from)
        }
        _ => return None,
    };
    let distance = Vec2::new(step.x, step.z).length();
    (distance <= JUMP).then_some(distance)
}

/// What the walker's model was doing last frame.
#[derive(Default)]
pub struct WalkerGait {
    previous: Option<Place>,
    gait: Gait,
    /// The player the gait was last set on — a new run dresses a new model, which
    /// starts out standing whatever the old one was doing.
    player: Option<Entity>,
}

/// Plays the character model's clips off the walker's pace: `idle` at a stand, `walk`
/// on the move, cross-faded, the cycle sped up with the pace — the same gaits the
/// crowd's walkers play (`world_render::people`), driven here by a measured pace
/// instead of a clock. A model without those clips (or without any) simply stands as
/// `dress_people` left it.
pub fn animate_walker(
    walker: Res<Walker>,
    origin: Res<Origin>,
    time: Res<Time>,
    graphs: Res<CharacterGraphs>,
    model: Query<(&Person, &Dressed), With<CharacterModel>>,
    mut players: Query<(&mut AnimationTransitions, &mut AnimationPlayer)>,
    mut state: Local<WalkerGait>,
) {
    let Ok((person, dressed)) = model.single() else {
        return;
    };
    if state.player != dressed.player {
        state.player = dressed.player;
        state.gait = Gait::Idle;
    }
    let step = horizontal_step(state.previous, walker.place, &origin.0);
    state.previous = walker.place;
    let dt = time.delta_secs();
    let Some(pace) = step.filter(|_| dt > 0.0).map(|d| d / dt) else {
        return;
    };
    let wanted = gait(pace);
    let Some(graph) = graphs.get(person.gltf.id()) else {
        return;
    };
    let Some((mut transitions, mut player)) = dressed.player.and_then(|e| players.get_mut(e).ok())
    else {
        return;
    };
    if play_gait(&mut transitions, &mut player, graph, state.gait, wanted) {
        match wanted {
            Gait::Walk { rate } => info!("walker: walk at {rate:.2}x"),
            Gait::Idle => info!("walker: idle"),
        }
    }
    state.gait = wanted;
}

/// One step over the ground: stopped by what stands at chest height and by ground that
/// rises higher than a step. A step down is left to the fall.
fn step(
    cast: &mut MeshRayCast,
    feet: Vec3,
    direction: Vec3,
    reach: f32,
    filter: &dyn Fn(Entity) -> bool,
) -> Vec3 {
    // ponytail: one ray at chest height. A fence at knee height is walked through — a
    // second ray down there would first have to tell a step from an obstacle.
    if hit(
        cast,
        feet + Vec3::Y * CHEST,
        direction,
        reach + RADIUS,
        filter,
    )
    .is_some()
    {
        return feet;
    }
    let target = feet + direction * reach;
    match ground_below(cast, target + Vec3::Y * STEP_UP, filter) {
        // Stairs and platform edges are climbed; higher ground is a wall.
        Some(ground) if ground - feet.y <= STEP_UP => target.with_y(feet.y.max(ground)),
        Some(_) => feet,
        // Nothing ahead to stand on: he walks on and falls.
        None => target,
    }
}

/// Standing and falling: on the ground he is carried by it, off it he falls until he
/// lands. With nothing below at all — unloaded terrain — he waits instead of dropping
/// out of the world.
fn fall_step(height: f32, ground: Option<f32>, fall: f32, dt: f32) -> (f32, f32) {
    let Some(ground) = ground else {
        return (height, 0.0);
    };
    if height - ground <= CONTACT && fall <= 0.0 {
        return (ground, 0.0);
    }
    let fall = (fall - GRAVITY * dt).max(-TERMINAL);
    let height = height + fall * dt;
    if height <= ground {
        (ground, 0.0)
    } else {
        (height, fall)
    }
}

/// Walking past the end of a vehicle: on into the next one of the train, or held at the
/// end where there is none.
fn gangway(half: impl Fn(usize) -> Option<f32>, vehicle: usize, local: Vec3) -> (usize, Vec3) {
    let here = half(vehicle).unwrap_or_default();
    // −Z is ahead, so the vehicle in front carries the lower index.
    let (next, ahead) = if local.z < -here {
        (vehicle.checked_sub(1), true)
    } else if local.z > here {
        (Some(vehicle + 1), false)
    } else {
        return (vehicle, local);
    };
    match next.and_then(|next| Some((next, half(next)?))) {
        // ponytail: the gangway is a hole at the end of the box — no bellows to stand in,
        // and open whether the two vehicles are gangwayed or not.
        Some((next, over)) if ahead => (next, local.with_z(over + (local.z + here))),
        Some((next, over)) => (next, local.with_z(-over + (local.z - here))),
        // The end of the train: he stays in the vehicle he is in.
        None => (vehicle, local.with_z(local.z.clamp(-here, here))),
    }
}

/// Half the length a walker has inside a vehicle [m] — the box less half a metre at
/// each end.
fn half_length(train: &Train, vehicle: usize) -> Option<f32> {
    let length = train.vehicles.get(vehicle)?.spec.length as f32;
    Some((length / 2.0 - 0.5).max(0.0))
}

/// The spot on the ground beside the vehicle he steps out onto — `None` where there is
/// no ground to step onto.
fn beside(
    cast: &mut MeshRayCast,
    sim: &sim_core::Sim,
    riding: &Vehicle,
    origin: &RenderOrigin,
    eye: Vec3,
    filter: &dyn Fn(Entity) -> bool,
) -> Option<EcefPos> {
    let (base, right, up, ahead) = frame(sim, riding, origin);
    let side = if eye.x >= 0.0 { 1.0 } else { -1.0 };
    let outside = base + right * (side * STEP_ASIDE) - ahead * eye.z;
    let ground = ground_below(cast, outside + up * 2.0, filter)?;
    Some(origin.from_render(outside.with_y(ground + EYE_HEIGHT)))
}

/// The four walking keys and a controller's left stick as one direction in the walker's
/// own frame: −Z ahead, X to the right. The stick is added rather than chosen between, so
/// a hand on each device does not fight itself.
fn pressed_direction(input: &Input) -> Vec3 {
    let mut step = Vec3::ZERO;
    if input.pressed(Action::WalkForward) {
        step += Vec3::NEG_Z;
    }
    if input.pressed(Action::WalkBack) {
        step += Vec3::Z;
    }
    if input.pressed(Action::WalkLeft) {
        step += Vec3::NEG_X;
    }
    if input.pressed(Action::WalkRight) {
        step += Vec3::X;
    }
    let stick = input.walk();
    step + Vec3::new(stick.x, 0.0, -stick.y)
}

/// Eye point of a vehicle in its model space — the cab's own, or the guess
/// [`sim_core::cab::CabSpec::default`] carries for vehicles without cab data.
fn eye_point(vehicle: &Vehicle) -> Vec3 {
    vehicle
        .spec
        .model
        .as_ref()
        .and_then(|m| m.cab.as_ref())
        .map(|cab| Vec3::from(cab.eye))
        .unwrap_or_else(|| Vec3::from(sim_core::cab::CabSpec::default().eye))
}

/// Render frame of a vehicle: the point on the rail head, right, up and the direction of
/// travel.
type Frame = (Vec3, Vec3, Vec3, Vec3);

fn frame(sim: &sim_core::Sim, vehicle: &Vehicle, origin: &RenderOrigin) -> Frame {
    let pose = vehicle.pos.pose(&sim.net);
    let base = origin.to_render(pose.pos);
    let up = origin.dir_to_render(pose.up);
    let ahead = origin.dir_to_render(pose.tangent);
    let right = ahead.cross(up).normalize_or_zero();
    (base, right, up, ahead)
}

/// Model space of a vehicle → render space, and back.
fn to_render((base, right, up, ahead): Frame, local: Vec3) -> Vec3 {
    base + right * local.x + up * local.y - ahead * local.z
}

fn to_model((base, right, up, ahead): Frame, point: Vec3) -> Vec3 {
    let offset = point - base;
    Vec3::new(offset.dot(right), offset.dot(up), -offset.dot(ahead))
}

/// Orientation of a vehicle's model space in render space (X right, Y up, −Z ahead).
fn model_rotation(right: Vec3, up: Vec3, ahead: Vec3) -> Quat {
    Quat::from_mat3(&Mat3::from_cols(right, up, -ahead))
}

/// View angle of a direction of travel, so that stepping in and out of a vehicle leaves
/// the walker looking the way he already looked.
fn heading(ahead: Vec3) -> f32 {
    (-ahead.x).atan2(-ahead.z)
}

/// May the walker pass through this side of the vehicle? The passenger door of that side
/// has to be open — or the vehicle carries a cab, whose door he opens himself. Both only
/// at a stand.
fn door_usable(train: &Train, vehicle: &Vehicle, x: f32) -> bool {
    if train.speed().abs() as f32 > STANDSTILL {
        return false;
    }
    let side = if x >= 0.0 {
        vehicle.doors.right
    } else {
        vehicle.doors.left
    };
    side.phase == DoorPhase::Open || vehicle.spec.model.as_ref().is_some_and(|m| m.cab.is_some())
}

/// The train and vehicle whose door the walker stands at, with the eye point inside it
/// and that vehicle's direction of travel.
///
/// Every train standing there is searched, not only the one he came out of: a train left
/// in a platform by its own driver is a train he can walk into. One that is out of service
/// is not there to be boarded at all.
fn door_within_reach(
    sim: &sim_core::Sim,
    origin: &RenderOrigin,
    here: Vec3,
) -> Option<(usize, usize, Vec3, Vec3)> {
    sim.trains
        .iter()
        .enumerate()
        .filter(|(_, train)| !train.stabled)
        .find_map(|(t, train)| {
            train.vehicles.iter().enumerate().find_map(|(i, vehicle)| {
                let frame = frame(sim, vehicle, origin);
                let local = to_model(frame, here);
                (local.x.abs() <= DOOR_REACH
                    && local.z.abs() <= vehicle.spec.length as f32 / 2.0
                    && local.y.abs() <= 3.0
                    && door_usable(train, vehicle, local.x))
                .then(|| {
                    let eye = eye_point(vehicle);
                    (
                        t,
                        i,
                        Vec3::new(if local.x >= 0.0 { AISLE } else { -AISLE }, eye.y, local.z),
                        frame.3,
                    )
                })
            })
        })
}

/// Height of the ground below a point, from the meshes drawn there.
fn ground_below(
    cast: &mut MeshRayCast,
    from: Vec3,
    filter: &dyn Fn(Entity) -> bool,
) -> Option<f32> {
    hit(cast, from, Vec3::NEG_Y, DROP, filter).map(|point| point.y)
}

/// First mesh the ray meets within `distance`, ignoring what the filter rejects.
fn hit(
    cast: &mut MeshRayCast,
    from: Vec3,
    direction: Vec3,
    distance: f32,
    filter: &dyn Fn(Entity) -> bool,
) -> Option<Vec3> {
    let settings = MeshRayCastSettings {
        // Hierarchy visibility, not the frustum: the ground under the feet is often out
        // of view, and it still has to be stood on.
        visibility: RayCastVisibility::Visible,
        filter,
        early_exit_test: &|_| true,
    };
    let ray = Ray3d::new(from, Dir3::new(direction).ok()?);
    cast.cast_ray(ray, &settings)
        .first()
        .filter(|(_, hit)| hit.distance <= distance)
        .map(|(_, hit)| hit.point)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standing_falling_and_landing() {
        // On the ground he is carried by it, however long the frame.
        assert_eq!(fall_step(2.0, Some(2.0), 0.0, 0.016), (2.0, 0.0));
        // Off it he picks up speed downwards …
        let (height, fall) = fall_step(5.0, Some(0.0), 0.0, 0.1);
        assert!(fall < 0.0 && height < 5.0, "{height} {fall}");
        // … and a step that would carry him through the ground lands him on it.
        assert_eq!(fall_step(0.2, Some(0.0), -20.0, 0.1), (0.0, 0.0));
        // Nothing below: he waits rather than falling out of the world.
        assert_eq!(fall_step(3.0, None, -5.0, 0.1), (3.0, 0.0));
    }

    #[test]
    fn the_gangway_carries_the_walk_into_the_next_vehicle() {
        // Three vehicles of 20, 26 and 20 m, each less half a metre at either end.
        let train = |i: usize| [9.5_f32, 12.5, 9.5].get(i).copied();
        let (here, next) = (9.5_f32, 12.5_f32);
        // Out of the back of the first vehicle: on at the front of the second, with the
        // half metre of overshoot kept.
        let (vehicle, local) = gangway(train, 0, Vec3::new(0.0, 0.0, here + 0.5));
        assert_eq!(vehicle, 1);
        assert!((local.z - (-next + 0.5)).abs() < 1e-4, "{local:?}");
        // And back out of the front of the second into the first.
        let (vehicle, local) = gangway(train, 1, Vec3::new(0.0, 0.0, -next - 0.5));
        assert_eq!(vehicle, 0);
        assert!((local.z - (here - 0.5)).abs() < 1e-4, "{local:?}");
        // The end of the train is the end of the walk.
        let (vehicle, local) = gangway(train, 2, Vec3::new(0.0, 0.0, here + 2.0));
        assert_eq!(vehicle, 2);
        assert!((local.z - here).abs() < 1e-4, "{local:?}");
        // In the middle of a vehicle nothing happens at all.
        assert_eq!(gangway(train, 1, Vec3::ZERO), (1, Vec3::ZERO));
    }

    /// Standing below a fifth of the walk, the cycle at its own speed at the walk,
    /// three times it at the run — and never outside its band.
    #[test]
    fn the_gait_follows_the_pace() {
        assert_eq!(gait(0.0), Gait::Idle);
        assert_eq!(gait(0.2), Gait::Idle);
        assert_eq!(gait(WALK), Gait::Walk { rate: 1.0 });
        assert_eq!(gait(RUN), Gait::Walk { rate: 3.0 });
        assert_eq!(gait(0.4), Gait::Walk { rate: 0.6 });
        assert_eq!(Gait::Idle.clip(), "idle");
        assert_eq!(gait(WALK).clip(), "walk");
    }

    /// The pace is read off the place the walker is kept in: aboard in the vehicle's
    /// own space (so the train's speed is not his), outside on the earth (so an
    /// origin rebase is not a step); a change of frame or a jump is not measured.
    #[test]
    fn the_step_is_measured_in_the_walkers_own_frame() {
        let origin = RenderOrigin::new(world_coords::geo::to_ecef_deg(52.0, 10.0, 100.0));
        let aboard = |vehicle, x: f32, z: f32| Place::Aboard {
            vehicle,
            eye: Vec3::new(x, EYE_HEIGHT, z),
        };
        let step = horizontal_step(
            Some(aboard(1, 0.0, 0.0)),
            Some(aboard(1, 0.3, -0.4)),
            &origin,
        );
        assert!((step.unwrap() - 0.5).abs() < 1e-5);
        // Climbing a stair is not walking forward.
        let up = Place::Aboard {
            vehicle: 1,
            eye: Vec3::new(0.0, EYE_HEIGHT + 0.8, 0.0),
        };
        assert!(horizontal_step(Some(aboard(1, 0.0, 0.0)), Some(up), &origin).unwrap() < 1e-5);
        assert_eq!(
            horizontal_step(
                Some(aboard(1, 0.0, 0.0)),
                Some(aboard(2, 0.0, 0.0)),
                &origin
            ),
            None
        );
        let outside = |east: f32| Place::Outside {
            eye: origin.from_render(Vec3::new(east, EYE_HEIGHT, 0.0)),
        };
        let step = horizontal_step(Some(outside(0.0)), Some(outside(1.5)), &origin);
        assert!((step.unwrap() - 1.5).abs() < 1e-3, "{step:?}");
        assert_eq!(
            horizontal_step(Some(outside(0.0)), Some(outside(9.0)), &origin),
            None
        );
        assert_eq!(
            horizontal_step(Some(outside(0.0)), Some(aboard(0, 0.0, 0.0)), &origin),
            None
        );
        assert_eq!(horizontal_step(None, Some(outside(0.0)), &origin), None);
    }

    #[test]
    fn model_space_and_render_space_are_inverse() {
        let frame = (Vec3::new(10.0, 3.0, -4.0), Vec3::X, Vec3::Y, Vec3::NEG_Z);
        let local = Vec3::new(-0.6, 2.8, -8.0);
        assert!((to_model(frame, to_render(frame, local)) - local).length() < 1e-4);
    }

    #[test]
    fn the_heading_turns_the_cab_view_into_a_view_over_the_ground() {
        // The view angle outside is the one in the cab plus the vehicle's heading —
        // which is what `heading` reads back out of the direction of travel.
        for turn in [-2.0, -0.4, 0.0, 0.7, 2.9] {
            let ahead = Quat::from_rotation_y(turn) * Vec3::NEG_Z;
            assert!((heading(ahead) - turn).abs() < 1e-5, "{turn} -> {ahead:?}");
        }
    }
}
