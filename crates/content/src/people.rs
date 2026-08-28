//! The people on the platforms and the ways (plan ch. 12): every `Platform`
//! device of a line gets a waiting crowd, every footpath and walk area of the
//! line (and of a model that carries its own, see MODS.md *Track objects*) its
//! walkers — placed here and drawn by the renderer.
//!
//! Nobody is stored and nobody is sent. A crowd is a pure function of the line —
//! its name is the seed, the device index is mixed in — so every restart and every
//! client of a multiplayer run shows the same people in the same places, and a
//! line of a hundred stations costs a few numbers per station rather than a file
//! of positions. The instances are prepared the way [`crate::Scenery`] prepares
//! its objects: resolved against the track once, bucketed by terrain tile, and
//! placed on the tile's ground when that tile is built, so they stream with it.
//!
//! A walker goes one step further: where it is at any moment is a pure function
//! of its walkway, its seed and the scenario clock ([`stroll_pose`]). Nothing
//! integrates from frame to frame, so a client that joins late, a run that was
//! paused, and a server that never draws anything all agree on where everybody
//! stands — and a lookup costs a walk over a handful of vertices, not a history.

use crate::route::{DeviceSource, LineSource, WalkAreaSource, WalkPathSource, WalkPoint};
use crate::terrain::{CellMap, HeightGrid, Rng, TileKey, bucket, model_rotation, to_render};
use glam::{DQuat, DVec2, DVec3, Quat, Vec2, Vec3};
use std::collections::BTreeMap;
use track_model::{DeviceKind, PlatformPayload, TrackEdge, TrackNetwork};
use world_coords::{EcefPos, EnuFrame, geo};

/// One person per this much platform [m] — a quiet suburban platform, not the
/// rush hour. The count is clamped to [`MIN_CROWD`]..=[`MAX_CROWD`].
pub const PERSON_SPACING: f64 = 6.0;
/// A platform is never empty — a station with nobody on it reads as abandoned.
pub const MIN_CROWD: usize = 1;
/// A long platform stops filling up here: sixty skinned people at one station
/// is what the renderer is budgeted for.
pub const MAX_CROWD: usize = 60;
/// Share of a platform's crowd that walks the platform's length instead of
/// waiting — enough that a station is not a waxworks, few enough that the
/// people who wait still read as the crowd.
pub const PLATFORM_WALKING_SHARE: f64 = 0.3;
/// Nearest a person stands to the track centre [m] — the platform edge is at
/// about 1.65 m, the safety line half a metre behind it.
const NEAREST: f64 = 2.3;
/// The waiting crowd spreads out to here from the track centre [m]; the lane the
/// walking share of it walks runs behind it, so nobody walks through anybody.
const FARTHEST: f64 = 2.9;
/// Where a platform's walkers walk [m from the track centre], and how wide their
/// lane is — a strip behind the waiting crowd, wide enough for two to pass.
const LANE_DISTANCE: f64 = 3.8;
const LANE_WIDTH: f32 = 1.2;
/// A path agent keeps to the right-hand side of its way, this far from the middle
/// at least [m] — two meeting then pass a shoulder's width and more apart, and the
/// half circles it turns at the ends have this radius at least. A way narrower than
/// that is walked in single file down its middle.
const MIN_LATERAL: f64 = 0.35;

/// How close an area's wanderer comes to anybody standing in it [m]: its spots
/// and its ways keep this clear of them.
const CLEARANCE: f32 = 0.7;
/// Tries at a clear spot before one is taken as it comes.
const CLEARANCE_TRIES: usize = 24;
/// A person facing the track looks up to this far away from it [deg].
const FACING_SPREAD_DEG: f64 = 40.0;
/// Share of the crowd that looks along the platform instead of at the track.
const ALONG_SHARE: f64 = 0.15;
/// The strip a platform's walkers use is sampled along the track this often
/// [m], so it follows a curved platform instead of cutting the chord.
const STRIP_STEP: f64 = 25.0;

/// Spots an area agent wanders between before its round repeats — enough that
/// the round is not read as one, few enough that a lookup is a short walk.
pub const AREA_WAYPOINTS: usize = 8;
/// A path agent's pace [m/s]: a stroll to a brisk walk, drawn per agent.
const PATH_SPEED: (f64, f64) = (1.0, 1.6);
/// An area agent's pace [m/s] — nobody hurries across a forecourt.
const AREA_SPEED: (f64, f64) = (0.8, 1.3);
/// How long an area agent stands at each of its spots [s].
const AREA_PAUSE: (f64, f64) = (3.0, 15.0);
/// A path agent keeps inside this share of the way's half width, so a shoulder
/// never hangs over the edge of a footbridge.
const PATH_INSIDE: f64 = 0.8;
/// A pace below this is treated as this [m/s] — a zero would make a cycle of
/// infinite length.
const MIN_SPEED: f32 = 0.05;
/// Draws at a point inside a polygon before its centroid is taken instead. A
/// polygon that fails this often is a sliver, and somebody standing on its
/// centre line is the right answer for a sliver.
const SAMPLE_ATTEMPTS: usize = 64;
/// The crowd a model-embedded walkway gets when its `_0` node says nothing
/// (MODS.md, *Track objects*).
const EMBEDDED_PATH_PEOPLE: u32 = 4;
const EMBEDDED_AREA_PEOPLE: u32 = 6;
const EMBEDDED_WIDTH: f64 = 2.0;
const EMBEDDED_WALKING_SHARE: f64 = 0.5;

/// How a person stands — which of the model's clips it plays
/// ([`crate::characters`] lists them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pose {
    /// Looping stand with a little life in it.
    Idle,
    /// The second looping stand.
    Idle2,
    /// Single-frame standing poses.
    Stand,
    Stand2,
    Stand3,
    /// Single-frame, feet on the floor, seat about 0.45 m up.
    Sit,
}

impl Pose {
    /// The clip the pose plays.
    pub fn clip(self) -> &'static str {
        match self {
            Pose::Idle => "idle",
            Pose::Idle2 => "idle2",
            Pose::Stand => "stand",
            Pose::Stand2 => "stand2",
            Pose::Stand3 => "stand3",
            Pose::Sit => "sit",
        }
    }

    /// Whether the clip runs (looping) or is a held frame.
    pub fn is_looping(self) -> bool {
        matches!(self, Pose::Idle | Pose::Idle2)
    }
}

/// One person on a tile, in the tile's own frame — what the renderer spawns.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PersonInstance {
    /// The origin between the feet in render axes, relative to the tile anchor.
    pub pos: [f32; 3],
    /// Orientation in render axes (`x, y, z, w`): the face along the chosen
    /// direction, up the local vertical.
    pub rotation: [f32; 4],
    /// Index into [`Crowd::characters`].
    pub character: u16,
    pub pose: Pose,
    /// Where in its clip the person starts, 0..1 — so no two move in step.
    pub phase: f32,
}

/// One person, resolved against the track: UTM position for the tile lookup,
/// the rail-plane base at its lateral distance and the two directions that
/// orient it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PlacedPerson {
    pos: DVec2,
    /// Point on the rail plane at the person's lateral distance.
    base: EcefPos,
    /// Local vertical at the base.
    up: DVec3,
    /// Where the face points.
    dir: DVec3,
    /// The platform surface above the rail plane [m]; 0 = on the ground.
    height: f64,
    character: u16,
    pose: Pose,
    phase: f32,
}

// ---------------------------------------------------------------------------
// The motion model: walkways in a local metre frame, and where an agent is on
// one at a moment of the clock.
// ---------------------------------------------------------------------------

/// What a walkway is: a way walked up and down, or a place wandered about on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WalkwayKind {
    /// A footpath; `width` [m] is what its agents spread across.
    Path { width: f32 },
    /// A polygon its agents wander inside.
    Area,
}

/// A walkway in a local metre frame — Y up, the frame of the terrain tile or
/// of the model it came out of — with the agents on it. The vertices carry
/// their heights, so a footbridge climbs and a platform stays on its surface;
/// where an agent is at a moment is [`stroll_pose`].
#[derive(Debug, Clone, PartialEq)]
pub struct Walkway {
    /// The source's label — for the log, nothing else.
    pub name: String,
    pub kind: WalkwayKind,
    /// A path's vertices in walking order, an area's corners in ring order.
    pub points: Vec<[f32; 3]>,
    pub agents: Vec<StrollAgent>,
}

/// One person on a walkway: its pace, where it starts in its cycle, and — for
/// an area — the spots it wanders between. Everything here was drawn once
/// from the walkway's seed; nothing changes while the run goes.
#[derive(Debug, Clone, PartialEq)]
pub struct StrollAgent {
    /// Index into the crowd's characters.
    pub character: u16,
    /// Walking pace [m/s].
    pub speed: f32,
    /// A path agent's distance from the centreline [m], kept on its right-hand
    /// side whichever way it walks — right-hand traffic, so two meeting pass
    /// each other instead of walking through each other; 0 for an area agent.
    pub lateral: f32,
    /// Where in its cycle the agent starts, 0..1 — so no two walk in step.
    pub phase: f32,
    /// The agent's own stream, mixed out of the walkway's: what its pauses and
    /// spots were drawn from.
    pub seed: u64,
    /// Seconds stood at each stop — an area agent's spots in the order they
    /// are visited; empty for a path agent, who never stops.
    pub pauses: Vec<f32>,
    /// An area agent's spots in visiting order; empty for a path agent, whose
    /// way is the walkway's own polyline.
    pub waypoints: Vec<[f32; 3]>,
}

/// Where an agent is at a moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrollPose {
    /// The origin between the feet, in the walkway's frame.
    pub position: [f32; 3],
    /// Turn about the vertical [rad] that points the model's face (−Z) the way
    /// it walks — `Quat::from_rotation_y(yaw)`. Kept through a pause.
    pub yaw: f32,
    /// Walking, as opposed to standing at a stop.
    pub moving: bool,
}

impl Walkway {
    /// A footpath with `people` agents walking it round and round: up on the
    /// right-hand lane, round the far end, back on the other, all at the one pace
    /// the way has. Without characters, or with fewer than two points, nobody
    /// walks it.
    pub fn path(
        name: &str,
        points: Vec<[f32; 3]>,
        width: f32,
        people: u32,
        characters: u16,
        seed: u64,
    ) -> Self {
        let mut rng = Rng(seed ^ 0x7061_7468);
        let mut agents = Vec::new();
        if characters > 0 && points.len() >= 2 {
            let half = f64::from(width.max(0.0)) / 2.0;
            // One pace, and so one lap time, for the whole way: nobody overtakes
            // anybody, and the spacing the phases give stays as it is for ever. The
            // ovals differ a little in length with the lane offset, so whoever walks
            // a longer one walks a touch faster to keep the lap time.
            let pace = rng.range(PATH_SPEED.0, PATH_SPEED.1);
            let edge = half * PATH_INSIDE;
            let reference =
                PathLoop::new(&points, (edge.max(MIN_LATERAL) + MIN_LATERAL) as f32 / 2.0);
            let period = reference.length() / pace;
            for index in 0..people {
                let character = rng.below(usize::from(characters)) as u16;
                // On the right-hand half, between the passing distance and the edge.
                let lateral = if edge <= MIN_LATERAL {
                    edge
                } else {
                    rng.range(MIN_LATERAL, edge)
                } as f32;
                let speed = (PathLoop::new(&points, lateral).length() / period) as f32;
                // Spread round the oval with half a gap of jitter, so nobody ever walks
                // in anybody's back — the gaps are kept by the shared lap time.
                let phase = ((f64::from(index) + 0.5 * rng.f64()) / f64::from(people)) as f32;
                let seed = rng.next();
                agents.push(StrollAgent {
                    character,
                    speed,
                    lateral,
                    phase,
                    seed,
                    pauses: Vec::new(),
                    waypoints: Vec::new(),
                });
            }
        }
        Self {
            name: name.to_string(),
            kind: WalkwayKind::Path { width },
            points,
            agents,
        }
    }

    /// A walk area: `walking_share` of its `people` wander between
    /// [`AREA_WAYPOINTS`] seeded spots inside the polygon, the rest stand at
    /// spots of their own facing whichever way — those are returned as
    /// ordinary people in the same frame, for the caller to place. Without
    /// characters, or with fewer than three corners, the area is empty.
    pub fn area(
        name: &str,
        polygon: Vec<[f32; 3]>,
        people: u32,
        walking_share: f64,
        characters: u16,
        seed: u64,
    ) -> (Self, Vec<PersonInstance>) {
        let mut rng = Rng(seed ^ 0x6172_6561);
        let mut agents = Vec::new();
        let mut standing = Vec::new();
        if characters > 0 && polygon.len() >= 3 {
            let walking = walking_count(people, walking_share);
            // The standing ones are placed first: the wanderers then keep their
            // spots and their ways clear of them, so nobody walks through anybody
            // who stands (two wanderers may still cross — they are few and moving).
            let mut wanderers = Vec::new();
            for i in 0..people {
                let character = rng.below(usize::from(characters)) as u16;
                let phase = rng.f64() as f32;
                let seed = rng.next();
                if i < walking {
                    wanderers.push((character, phase, seed));
                    continue;
                }
                let mut own = Rng(seed);
                let pos = sample_inside(&mut own, &polygon);
                let yaw = own.range(0.0, std::f64::consts::TAU) as f32;
                let pick = own.f64();
                let stand = own.below(3);
                standing.push(PersonInstance {
                    pos,
                    rotation: Quat::from_rotation_y(yaw).to_array(),
                    character,
                    pose: standing_pose(pick, stand),
                    phase,
                });
            }
            // The wanderers keep clear of the standers, and of the spots the wanderers
            // before them stop at — a stop lasts long enough to be walked into.
            let mut obstacles: Vec<Vec2> = standing
                .iter()
                .map(|p| Vec2::new(p.pos[0], p.pos[2]))
                .collect();
            for (character, phase, seed) in wanderers {
                let mut own = Rng(seed);
                let speed = own.range(AREA_SPEED.0, AREA_SPEED.1) as f32;
                let mut waypoints: Vec<[f32; 3]> = Vec::with_capacity(AREA_WAYPOINTS);
                for _ in 0..AREA_WAYPOINTS {
                    let spot =
                        sample_clear(&mut own, &polygon, &obstacles, waypoints.last().copied());
                    waypoints.push(spot);
                }
                obstacles.extend(waypoints.iter().map(|p| Vec2::new(p[0], p[2])));
                let pauses = (0..AREA_WAYPOINTS)
                    .map(|_| own.range(AREA_PAUSE.0, AREA_PAUSE.1) as f32)
                    .collect();
                agents.push(StrollAgent {
                    character,
                    speed,
                    lateral: 0.0,
                    phase,
                    seed,
                    pauses,
                    waypoints,
                });
            }
        }
        (
            Self {
                name: name.to_string(),
                kind: WalkwayKind::Area,
                points: polygon,
                agents,
            },
            standing,
        )
    }

    /// Where agent `agent` is at clock second `t`; `None` for an agent the
    /// walkway does not have.
    pub fn pose(&self, agent: usize, t: f64) -> Option<StrollPose> {
        self.agents.get(agent).map(|a| stroll_pose(self, a, t))
    }

    /// How long one round of an agent takes [s] — after it, the agent is where
    /// it was, doing what it did.
    pub fn period(&self, agent: &StrollAgent) -> f64 {
        let speed = f64::from(agent.speed.max(MIN_SPEED));
        match self.kind {
            WalkwayKind::Path { .. } => PathLoop::new(&self.points, agent.lateral).length() / speed,
            WalkwayKind::Area => {
                let k = agent.waypoints.len();
                (0..k)
                    .map(|i| pause(agent, i) + leg_length(&agent.waypoints, i, k) / speed)
                    .sum()
            }
        }
    }

    /// How many people walk it.
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }
}

/// Where `agent` of `walkway` is at clock second `t` — deterministic in `t`,
/// periodic with [`Walkway::period`], and a walk over the vertices, nothing
/// more: the renderer asks this for every walker every frame.
pub fn stroll_pose(walkway: &Walkway, agent: &StrollAgent, t: f64) -> StrollPose {
    match walkway.kind {
        WalkwayKind::Path { .. } => path_pose(&walkway.points, agent, t),
        WalkwayKind::Area => area_pose(&walkway.points, agent, t),
    }
}

/// How many of `people` wander, for a share of them.
fn walking_count(people: u32, walking_share: f64) -> u32 {
    (f64::from(people) * walking_share.clamp(0.0, 1.0)).round() as u32
}

/// The agent's stop `i`, held to a sensible number.
fn pause(agent: &StrollAgent, i: usize) -> f64 {
    f64::from(agent.pauses.get(i).copied().unwrap_or(0.0).max(0.0))
}

/// Where in its cycle an agent is at `t`: its phase shifts the whole cycle, so
/// two agents on one way never march in step, and the remainder is what a
/// lookup works on — a clock of a hundred thousand seconds costs nothing.
fn cycle_time(t: f64, phase: f32, period: f64) -> f64 {
    if period <= 0.0 {
        0.0
    } else {
        (t + f64::from(phase) * period).rem_euclid(period)
    }
}

/// The oval a path agent walks: up the way on its right-hand lane, half a circle
/// round the far end onto the other lane, back down, half a circle round the near
/// end. Everybody on a way walks the same oval at the same pace, so nobody meets
/// anybody head-on, nobody overtakes and nobody has to pass anybody who stopped —
/// a path agent never stops. Each turn is the half circle between the two lanes'
/// ends, which sit a lane offset in from the way's ends, so the arcs reach them.
struct PathLoop {
    /// The turn centres along the way [m].
    from: f64,
    to: f64,
    /// The lane offset [m].
    lateral: f32,
    /// Length of one straight [m] — along the way, the lanes may be a little
    /// longer or shorter round a corner.
    straight: f64,
    /// The turns at the far and the near end.
    far: Turn,
    near: Turn,
}

/// A half circle from one lane's end to the other's, out past the way's end.
struct Turn {
    /// Where it starts (the lane the agent arrives on) and its centre.
    start: Vec2,
    centre: Vec2,
    /// The way's direction at the turn, pointing out past the end.
    out: Vec2,
    /// Its height, the way's there.
    height: f32,
    /// Its radius and length [m].
    radius: f64,
    length: f64,
}

impl Turn {
    /// The turn between the lane ends `a` (arrival) and `b` (departure), bulging
    /// along `out`.
    fn between(a: [f32; 3], b: [f32; 3], out: Vec2) -> Self {
        let (a2, b2) = (Vec2::new(a[0], a[2]), Vec2::new(b[0], b[2]));
        let centre = (a2 + b2) * 0.5;
        let radius = f64::from(a2.distance(b2)) * 0.5;
        Self {
            start: a2,
            centre,
            out,
            height: (a[1] + b[1]) * 0.5,
            radius,
            length: std::f64::consts::PI * radius,
        }
    }

    /// Where the agent is `gone` metres into the turn, and which way it faces.
    fn pose(&self, gone: f64) -> ([f32; 3], Vec2) {
        let theta = if self.radius > 1e-6 {
            (gone / self.radius) as f32
        } else {
            0.0
        };
        let spoke = self.start - self.centre;
        let bulge = self.out * spoke.length();
        let at = self.centre + spoke * theta.cos() + bulge * theta.sin();
        let heading = -spoke * theta.sin() + bulge * theta.cos();
        ([at.x, self.height, at.y], heading)
    }
}

impl PathLoop {
    fn new(points: &[[f32; 3]], lateral: f32) -> Self {
        let length = polyline_length(points);
        let lateral = f64::from(lateral.max(0.05)).min(length / 4.0) as f32;
        let (from, to) = (f64::from(lateral), length - f64::from(lateral));
        // The far turn: from the right-hand lane's end round to the other lane.
        let (far_a, tangent) = along_polyline(points, to as f32, -lateral);
        let (far_b, _) = along_polyline(points, to as f32, lateral);
        let far = Turn::between(far_a, far_b, tangent);
        // The near turn: from the other lane back onto the right-hand one.
        let (near_a, tangent) = along_polyline(points, from as f32, lateral);
        let (near_b, _) = along_polyline(points, from as f32, -lateral);
        let near = Turn::between(near_a, near_b, -tangent);
        Self {
            from,
            to,
            lateral,
            straight: (to - from).max(0.0),
            far,
            near,
        }
    }

    /// One lap [m].
    fn length(&self) -> f64 {
        2.0 * self.straight + self.far.length + self.near.length
    }
}

/// Where a path agent is at `t`: along its oval, always walking.
fn path_pose(points: &[[f32; 3]], agent: &StrollAgent, t: f64) -> StrollPose {
    let speed = f64::from(agent.speed.max(MIN_SPEED));
    let lap = PathLoop::new(points, agent.lateral);
    let u = cycle_time(t, agent.phase, lap.length() / speed) * speed;
    // Right-hand traffic: the lane to the way's right going up, to its left coming back.
    let (position, heading) = if u < lap.straight {
        let (p, tangent) = along_polyline(points, (lap.from + u) as f32, -lap.lateral);
        (p, tangent)
    } else if u < lap.straight + lap.far.length {
        lap.far.pose(u - lap.straight)
    } else if u < 2.0 * lap.straight + lap.far.length {
        let back = u - lap.straight - lap.far.length;
        let (p, tangent) = along_polyline(points, (lap.to - back) as f32, lap.lateral);
        (p, -tangent)
    } else {
        lap.near.pose(u - 2.0 * lap.straight - lap.far.length)
    };
    StrollPose {
        position,
        yaw: yaw_of(heading),
        moving: true,
    }
}

/// From spot to spot in visiting order and back to the first, a stand at
/// each. The agent faces the way it came while it stands, which is the way
/// it walked in — a person does not turn on the spot for no reason.
fn area_pose(polygon: &[[f32; 3]], agent: &StrollAgent, t: f64) -> StrollPose {
    let spots = &agent.waypoints;
    let k = spots.len();
    if k == 0 {
        return StrollPose {
            position: centroid(polygon),
            yaw: 0.0,
            moving: false,
        };
    }
    let speed = f64::from(agent.speed.max(MIN_SPEED));
    let period: f64 = (0..k)
        .map(|i| pause(agent, i) + leg_length(spots, i, k) / speed)
        .sum();
    let u = cycle_time(t, agent.phase, period);
    // The first spot is reached along the last leg — that is the heading held there.
    let mut heading = leg_heading(spots, k - 1);
    let mut elapsed = 0.0;
    for i in 0..k {
        let stand = pause(agent, i);
        if u < elapsed + stand {
            return StrollPose {
                position: spots[i],
                yaw: yaw_of(heading),
                moving: false,
            };
        }
        elapsed += stand;
        let step = Vec3::from(spots[(i + 1) % k]) - Vec3::from(spots[i]);
        let across = Vec2::new(step.x, step.z);
        let length = across.length();
        if length > f32::EPSILON {
            heading = across / length;
        }
        let duration = f64::from(length) / speed;
        if u < elapsed + duration {
            let f = ((u - elapsed) / duration) as f32;
            return StrollPose {
                position: (Vec3::from(spots[i]) + step * f).to_array(),
                yaw: yaw_of(heading),
                moving: true,
            };
        }
        elapsed += duration;
    }
    // Rounding at the very end of the round: back at the first spot.
    StrollPose {
        position: spots[0],
        yaw: yaw_of(heading),
        moving: false,
    }
}

/// Horizontal length of leg `i` of a closed ring of `k` spots [m].
fn leg_length(spots: &[[f32; 3]], i: usize, k: usize) -> f64 {
    let (a, b) = (spots[i], spots[(i + 1) % k]);
    f64::from(Vec2::new(b[0] - a[0], b[2] - a[2]).length())
}

/// The way leg `i` of a closed ring points; a leg of no length takes the
/// heading of the one before it, all the way round, and north failing that.
fn leg_heading(spots: &[[f32; 3]], i: usize) -> Vec2 {
    let k = spots.len();
    for back in 0..k {
        let j = (i + k - back) % k;
        let (a, b) = (spots[j], spots[(j + 1) % k]);
        let d = Vec2::new(b[0] - a[0], b[2] - a[2]);
        if d.length() > f32::EPSILON {
            return d / d.length();
        }
    }
    Vec2::NEG_Y
}

/// Horizontal length of a polyline [m].
fn polyline_length(points: &[[f32; 3]]) -> f64 {
    points
        .windows(2)
        .map(|w| f64::from(Vec2::new(w[1][0] - w[0][0], w[1][2] - w[0][2]).length()))
        .sum()
}

/// A spot inside the polygon that keeps [`CLEARANCE`] from every obstacle, and
/// whose way in from `previous` does too — or, after [`CLEARANCE_TRIES`], the
/// last one drawn: a crowded area is still walked, only closer.
fn sample_clear(
    rng: &mut Rng,
    polygon: &[[f32; 3]],
    obstacles: &[Vec2],
    previous: Option<[f32; 3]>,
) -> [f32; 3] {
    let mut spot = sample_inside(rng, polygon);
    for _ in 0..CLEARANCE_TRIES {
        let here = Vec2::new(spot[0], spot[2]);
        let clear = obstacles.iter().all(|o| {
            o.distance(here) >= CLEARANCE
                && previous
                    .is_none_or(|p| segment_distance(Vec2::new(p[0], p[2]), here, *o) >= CLEARANCE)
        });
        if clear {
            break;
        }
        spot = sample_inside(rng, polygon);
    }
    spot
}

/// Distance of `p` from the segment `a`–`b`.
fn segment_distance(a: Vec2, b: Vec2, p: Vec2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 <= f32::EPSILON {
        return a.distance(p);
    }
    let f = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    (a + ab * f).distance(p)
}

/// The point `s` metres along a polyline, `lateral` metres to the left of it,
/// and the leg's direction there. The offset follows the mitre at every
/// vertex, so a person on the outside of a corner goes round it rather than
/// jumping across; the height comes with the vertices.
fn along_polyline(points: &[[f32; 3]], s: f32, lateral: f32) -> ([f32; 3], Vec2) {
    let n = points.len();
    if n < 2 {
        return (points.first().copied().unwrap_or([0.0; 3]), Vec2::NEG_Y);
    }
    let mut start = 0.0f32;
    for i in 0..n - 1 {
        let (a, b) = (Vec3::from(points[i]), Vec3::from(points[i + 1]));
        let length = Vec2::new(b.x - a.x, b.z - a.z).length();
        if s <= start + length || i + 2 == n {
            let f = if length > f32::EPSILON {
                ((s - start) / length).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let from = a + sideways(mitre_normal(points, i), lateral);
            let to = b + sideways(mitre_normal(points, i + 1), lateral);
            return (from.lerp(to, f).to_array(), leg_direction(points, i));
        }
        start += length;
    }
    (points[n - 1], leg_direction(points, n - 2))
}

/// Direction of leg `i` of a polyline; a leg of no length borrows the nearest
/// one that has some, north failing that.
fn leg_direction(points: &[[f32; 3]], i: usize) -> Vec2 {
    let direction = |j: usize| {
        let d = Vec2::new(
            points[j + 1][0] - points[j][0],
            points[j + 1][2] - points[j][2],
        );
        (d.length() > f32::EPSILON).then(|| d / d.length())
    };
    let legs = points.len() - 1;
    (0..legs)
        .find_map(|k| {
            let ahead = (i + k < legs).then(|| direction(i + k)).flatten();
            ahead.or_else(|| (k <= i).then(|| direction(i - k)).flatten())
        })
        .unwrap_or(Vec2::NEG_Y)
}

/// The left normal at vertex `i` of a polyline: the mitre of the legs either
/// side, one leg's own at the ends, and the leg ahead alone at a hairpin.
fn mitre_normal(points: &[[f32; 3]], i: usize) -> Vec2 {
    let legs = points.len() - 1;
    let before = (i > 0).then(|| left_of(leg_direction(points, i - 1)));
    let after = (i < legs).then(|| left_of(leg_direction(points, i)));
    match (before, after) {
        (Some(a), Some(b)) => {
            let sum = a + b;
            if sum.length() > 0.1 {
                sum / sum.length()
            } else {
                b
            }
        }
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => Vec2::NEG_X,
    }
}

/// Left of a horizontal direction `(x, z)` — up × direction, in render axes.
fn left_of(d: Vec2) -> Vec2 {
    Vec2::new(d.y, -d.x)
}

/// A horizontal offset as a step in the frame.
fn sideways(normal: Vec2, by: f32) -> Vec3 {
    Vec3::new(normal.x * by, 0.0, normal.y * by)
}

/// The yaw that faces a horizontal direction `(x, z)`: the model looks down
/// −Z, and `Quat::from_rotation_y(yaw)` turns −Z onto `(−sin yaw, −cos yaw)`.
fn yaw_of(d: Vec2) -> f32 {
    (-d.x).atan2(-d.y)
}

/// A random spot inside a polygon (ring of `(x, y, z)` corners, tested in the
/// horizontal), at the height the corners interpolate to. The draws come
/// before the test, so the stream — and every spot after this one — is the
/// same whatever is rejected.
fn sample_inside(rng: &mut Rng, polygon: &[[f32; 3]]) -> [f32; 3] {
    let (lo, hi) = polygon.iter().fold(
        (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
        |(lo, hi), p| {
            let q = DVec2::new(f64::from(p[0]), f64::from(p[2]));
            (lo.min(q), hi.max(q))
        },
    );
    for _ in 0..SAMPLE_ATTEMPTS {
        let x = rng.range(lo.x, hi.x);
        let z = rng.range(lo.y, hi.y);
        if inside(x, z, polygon) {
            return [x as f32, height_among(x, z, polygon), z as f32];
        }
    }
    centroid(polygon)
}

/// Ray casting in the horizontal: is `(x, z)` inside the ring?
fn inside(x: f64, z: f64, polygon: &[[f32; 3]]) -> bool {
    let mut inside = false;
    let mut j = polygon.len() - 1;
    for i in 0..polygon.len() {
        let (ax, az) = (f64::from(polygon[i][0]), f64::from(polygon[i][2]));
        let (bx, bz) = (f64::from(polygon[j][0]), f64::from(polygon[j][2]));
        if (az > z) != (bz > z) && x < (bx - ax) * (z - az) / (bz - az) + ax {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// The height of a point among the corners: inverse-distance weighted, so a
/// sloping forecourt slopes and a flat platform stays flat.
fn height_among(x: f64, z: f64, polygon: &[[f32; 3]]) -> f32 {
    let (mut sum, mut weight) = (0.0, 0.0);
    for p in polygon {
        let d2 = (f64::from(p[0]) - x).powi(2) + (f64::from(p[2]) - z).powi(2);
        if d2 < 1e-6 {
            return p[1];
        }
        sum += f64::from(p[1]) / d2;
        weight += 1.0 / d2;
    }
    if weight > 0.0 {
        (sum / weight) as f32
    } else {
        0.0
    }
}

/// The mean of the corners.
fn centroid(polygon: &[[f32; 3]]) -> [f32; 3] {
    if polygon.is_empty() {
        return [0.0; 3];
    }
    let sum = polygon
        .iter()
        .fold(Vec3::ZERO, |acc, p| acc + Vec3::from(*p));
    (sum / polygon.len() as f32).to_array()
}

// ---------------------------------------------------------------------------
// Walkways a model carries: `wp_<name>_<i>` / `wa_<name>_<i>` nodes.
// ---------------------------------------------------------------------------

/// A `wp_*` / `wa_*` node of a model, as the renderer finds it in the spawned
/// scene.
#[derive(Debug, Clone, PartialEq)]
pub struct WalkwayNode {
    /// The node's full name, `wp_edge_3`.
    pub name: String,
    /// The node's origin in the model's frame — the parents' transforms
    /// accumulated, so a nested empty is where the modeller sees it.
    pub position: [f32; 3],
    /// The node's glTF `extras` as JSON text; the `_0` node's size the crowd.
    pub extras: Option<String>,
}

/// Which kind of walkway a node belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WalkwayTag {
    Path,
    Area,
}

/// What a node name says: `wp_edge_3` is vertex 3 of the footpath `edge`,
/// `wa_lobby_0` corner 0 of the area `lobby`. A name may carry underscores of
/// its own; the index is what follows the last one.
pub fn parse_walkway_node(name: &str) -> Option<(WalkwayTag, &str, u32)> {
    let (tag, rest) = name
        .strip_prefix("wp_")
        .map(|rest| (WalkwayTag::Path, rest))
        .or_else(|| {
            name.strip_prefix("wa_")
                .map(|rest| (WalkwayTag::Area, rest))
        })?;
    let (walkway, index) = rest.rsplit_once('_')?;
    if walkway.is_empty() {
        return None;
    }
    Some((tag, walkway, index.parse().ok()?))
}

/// The crowd a `_0` node's extras ask for (Blender custom properties, exported
/// as glTF `extras`). Anything else in the extras is somebody else's.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WalkwayExtras {
    pub people: Option<u32>,
    pub width: Option<f64>,
    pub walking_share: Option<f64>,
}

impl WalkwayExtras {
    /// Reads the extras field by field: a model is content, not code, so a
    /// property that is missing, misspelt or of the wrong type is its default
    /// and never a reason to lose the others. `people` typed as `6.0` counts
    /// as six — Blender exports a property in the type it was given.
    pub fn parse(json: &str) -> Self {
        let Some(object) = serde_json::from_str::<serde_json::Value>(json)
            .ok()
            .and_then(|value| value.as_object().cloned())
        else {
            return Self::default();
        };
        let number = |key: &str| object.get(key).and_then(serde_json::Value::as_f64);
        Self {
            people: number("people").map(|p| p.round().max(0.0) as u32),
            width: number("width"),
            walking_share: number("walking_share"),
        }
    }
}

/// The vertices of one named walkway as found: index, position, extras.
type FoundVertices<'a> = Vec<(u32, [f32; 3], Option<&'a str>)>;

/// The walkways a model carries, out of its `wp_*` / `wa_*` nodes, in the
/// model's own frame, each with the standing rest of an area. Seeded by the
/// placement and the walkway's name, so the same model placed twice gets two
/// crowds and a re-placed one keeps its own.
pub fn embedded_walkways(
    nodes: &[WalkwayNode],
    placement: u32,
    characters: u16,
) -> Vec<(Walkway, Vec<PersonInstance>)> {
    // A tree map: the walkways come out in one order whatever order the scene
    // hands its nodes over in.
    let mut groups: BTreeMap<(WalkwayTag, &str), FoundVertices> = BTreeMap::new();
    for node in nodes {
        if let Some((tag, name, index)) = parse_walkway_node(&node.name) {
            groups.entry((tag, name)).or_default().push((
                index,
                node.position,
                node.extras.as_deref(),
            ));
        }
    }
    groups
        .into_iter()
        .filter_map(|((tag, name), mut vertices)| {
            vertices.sort_by_key(|(index, _, _)| *index);
            // A numbered vertex twice is a modelling slip; the first one wins.
            vertices.dedup_by_key(|(index, _, _)| *index);
            let extras = vertices
                .first()
                .and_then(|(_, _, extras)| *extras)
                .map(WalkwayExtras::parse)
                .unwrap_or_default();
            let points: Vec<[f32; 3]> = vertices.iter().map(|(_, p, _)| *p).collect();
            let seed = embedded_seed(placement, name);
            match tag {
                WalkwayTag::Path => (points.len() >= 2).then(|| {
                    let width = extras.width.unwrap_or(EMBEDDED_WIDTH) as f32;
                    let people = extras.people.unwrap_or(EMBEDDED_PATH_PEOPLE);
                    (
                        Walkway::path(name, points, width, people, characters, seed),
                        Vec::new(),
                    )
                }),
                WalkwayTag::Area => (points.len() >= 3).then(|| {
                    let people = extras.people.unwrap_or(EMBEDDED_AREA_PEOPLE);
                    let share = extras.walking_share.unwrap_or(EMBEDDED_WALKING_SHARE);
                    Walkway::area(name, points, people, share, characters, seed)
                }),
            }
        })
        .collect()
}

/// The seed of a model-embedded walkway: the placement's index and the
/// walkway's name, so each way of each placed object has a crowd of its own.
pub fn embedded_seed(placement: u32, name: &str) -> u64 {
    line_seed(name) ^ (u64::from(placement) + 1).wrapping_mul(0xD6E8_FEB8_6659_FD93)
}

// ---------------------------------------------------------------------------
// The crowd of a line, prepared for tile builds.
// ---------------------------------------------------------------------------

/// One walkway of the line, resolved for the tile builds: its vertices with
/// their footing, its centroid for the tile lookup, and what it is.
#[derive(Debug, Clone, PartialEq)]
struct PlacedWalkway {
    name: String,
    kind: PlacedKind,
    vertices: Vec<PlacedVertex>,
    /// UTM centroid — the tile the walkway is filed under. A way that crosses
    /// a tile border belongs to one tile whole, so its walkers never split.
    centroid: DVec2,
    seed: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum PlacedKind {
    Path { width: f32, people: u32 },
    Area { people: u32, walking_share: f64 },
}

impl PlacedKind {
    fn people(self) -> usize {
        match self {
            PlacedKind::Path { people, .. } | PlacedKind::Area { people, .. } => people as usize,
        }
    }

    fn walking(self) -> usize {
        match self {
            PlacedKind::Path { people, .. } => people as usize,
            PlacedKind::Area {
                people,
                walking_share,
            } => walking_count(people, walking_share) as usize,
        }
    }
}

/// A vertex of a walkway, resolved: UTM position for the tile lookup and the
/// height grid, and how it stands.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PlacedVertex {
    pos: DVec2,
    /// On the rail plane at the vertex's lateral distance (a platform's strip),
    /// or any point on the vertex's vertical (a line's walkway — the ground
    /// answers the height).
    base: EcefPos,
    /// Above the rail plane or above the ground [m], as `footing` says.
    height: f64,
    footing: Footing,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Footing {
    /// On the tile's height grid, `height` above it.
    Ground,
    /// On the rail plane, `height` up the local vertical — a platform with a
    /// body, which the grid knows nothing about.
    Rail { up: DVec3 },
}

/// The crowd of a line, prepared for tile builds.
#[derive(Debug, Clone, Default)]
pub struct Crowd {
    characters: Vec<String>,
    placed: Vec<PlacedPerson>,
    by_tile: CellMap<Vec<u32>>,
    walkways: Vec<PlacedWalkway>,
    walkways_by_tile: CellMap<Vec<u32>>,
}

impl Crowd {
    /// The crowd of every `Platform` device of the line whose payload parses,
    /// and the walkers of its footpaths and walk areas, drawn from
    /// `characters` (the passenger-role characters of the installed mods,
    /// `"<mod>:<name>"`). No characters, no crowd.
    pub fn from_line(
        line: &LineSource,
        net: &TrackNetwork,
        zone: u8,
        characters: &[String],
        seed: u64,
    ) -> Self {
        Self::from_parts(
            &line.devices,
            &line.walk_paths,
            &line.walk_areas,
            net,
            zone,
            characters,
            seed,
        )
    }

    pub fn from_parts(
        devices: &[DeviceSource],
        paths: &[WalkPathSource],
        areas: &[WalkAreaSource],
        net: &TrackNetwork,
        zone: u8,
        characters: &[String],
        seed: u64,
    ) -> Self {
        let mut placed = Vec::new();
        let mut walkways = Vec::new();
        if !characters.is_empty() {
            for (index, device) in devices.iter().enumerate() {
                if device.kind != DeviceKind::Platform {
                    continue;
                }
                // A payload that is not a platform's is a file defect the editor's
                // rule check reports; here it is simply a platform without a crowd.
                let Some(platform) = PlatformPayload::parse(&device.payload) else {
                    continue;
                };
                // Compile refused dangling indices; a guard keeps a stale file
                // harmless.
                let Some(edge) = net.edges().get(device.edge as usize) else {
                    continue;
                };
                let mut rng = Rng(device_seed(seed, index));
                let count = crowd_size(platform.length);
                let walking = walking_count(count as u32, PLATFORM_WALKING_SHARE) as usize;
                for _ in 0..count - walking {
                    let s = (device.s + rng.f64() * platform.length).clamp(0.0, edge.length());
                    let pose = edge.eval(s);
                    let side = platform_side(&pose.tangent, &pose.up, device.lateral_offset);
                    let distance = rng.range(NEAREST, FARTHEST);
                    let base = EcefPos(pose.pos.0 + side * distance);
                    let dir = if rng.f64() < ALONG_SHARE {
                        if rng.f64() < 0.5 {
                            pose.tangent
                        } else {
                            -pose.tangent
                        }
                    } else {
                        let turn = rng
                            .range(-FACING_SPREAD_DEG, FACING_SPREAD_DEG)
                            .to_radians();
                        DQuat::from_axis_angle(pose.up, turn) * -side
                    };
                    let character = rng.below(characters.len()) as u16;
                    let pick = rng.f64();
                    let stand = rng.below(3);
                    let phase = rng.f64() as f32;
                    let (lat, lon, _) = geo::from_ecef(base);
                    let (e, n) = geo::to_utm(lat, lon, zone);
                    placed.push(PlacedPerson {
                        pos: DVec2::new(e, n),
                        base,
                        up: pose.up,
                        dir,
                        height: platform.height.max(0.0),
                        character,
                        pose: standing_pose(pick, stand),
                        phase,
                    });
                }
                if walking > 0 {
                    walkways.push(platform_strip(
                        edge,
                        device,
                        &platform,
                        walking as u32,
                        zone,
                        device_seed(seed, index) ^ 0x0073_7472_6970,
                    ));
                }
            }
            for (index, path) in paths.iter().enumerate() {
                // Fewer than two points is a file defect the rule check reports.
                if path.points.len() < 2 {
                    continue;
                }
                let vertices: Vec<PlacedVertex> = path
                    .points
                    .iter()
                    .map(|p| ground_vertex(p, path.height, zone))
                    .collect();
                walkways.push(PlacedWalkway {
                    name: path.name.clone(),
                    kind: PlacedKind::Path {
                        width: path.width.max(0.0) as f32,
                        people: path.people,
                    },
                    centroid: centroid_of(&vertices),
                    vertices,
                    seed: walkway_seed(seed, 1, index),
                });
            }
            for (index, area) in areas.iter().enumerate() {
                if area.polygon.len() < 3 {
                    continue;
                }
                let vertices: Vec<PlacedVertex> = area
                    .polygon
                    .iter()
                    .map(|p| ground_vertex(p, area.height, zone))
                    .collect();
                walkways.push(PlacedWalkway {
                    name: area.name.clone(),
                    kind: PlacedKind::Area {
                        people: area.people,
                        walking_share: area.walking_share,
                    },
                    centroid: centroid_of(&vertices),
                    vertices,
                    seed: walkway_seed(seed, 2, index),
                });
            }
        }
        Self {
            characters: characters.to_vec(),
            placed,
            by_tile: CellMap::default(),
            walkways,
            walkways_by_tile: CellMap::default(),
        }
    }

    /// The character names (`"<mod>:<name>"`) that [`PersonInstance::character`]
    /// and [`StrollAgent::character`] index.
    pub fn characters(&self) -> &[String] {
        &self.characters
    }

    /// How many people the line carries altogether — standing on its
    /// platforms and areas, walking its ways.
    pub fn len(&self) -> usize {
        self.placed.len() + self.walkways.iter().map(|w| w.kind.people()).sum::<usize>()
    }

    /// How many of them walk.
    pub fn walking(&self) -> usize {
        self.walkways.iter().map(|w| w.kind.walking()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.placed.is_empty() && self.walkways.is_empty()
    }

    /// Sorts the people and the walkways into the tile grid.
    pub(crate) fn bucket(&mut self, tile_size: f64) {
        self.by_tile = bucket(self.placed.iter().map(|p| p.pos), tile_size);
        self.walkways_by_tile = bucket(self.walkways.iter().map(|w| w.centroid), tile_size);
    }
}

/// The seed of a line: its name hashed (FNV-1a), so it is the same on every
/// machine and every start, whatever else changes.
pub fn line_seed(name: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in name.as_bytes() {
        h ^= u64::from(*byte);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// The line's seed mixed with the device index — each platform its own stream,
/// so adding a station leaves the others' crowds where they were.
fn device_seed(seed: u64, index: usize) -> u64 {
    seed ^ (index as u64 + 1).wrapping_mul(0xD6E8_FEB8_6659_FD93)
}

/// The line's seed mixed with a walkway's kind and index, for the same reason.
fn walkway_seed(seed: u64, kind: u64, index: usize) -> u64 {
    seed ^ (index as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ kind.wrapping_mul(0xBF58_476D_1CE4_E5B9)
}

/// How many people a platform of this length gets.
fn crowd_size(length: f64) -> usize {
    ((length / PERSON_SPACING).round().max(0.0) as usize).clamp(MIN_CROWD, MAX_CROWD)
}

/// The standing pose for a draw `pick` in 0..1 and a choice `stand` in 0..3:
/// 45 % `idle`, 25 % `idle2`, the rest the three held stands.
fn standing_pose(pick: f64, stand: usize) -> Pose {
    if pick < 0.45 {
        Pose::Idle
    } else if pick < 0.70 {
        Pose::Idle2
    } else {
        [Pose::Stand, Pose::Stand2, Pose::Stand3][stand % 3]
    }
}

/// The side of the track a platform device is on, as a unit vector across the
/// track: the device convention says positive = left of increasing arc length.
fn platform_side(tangent: &DVec3, up: &DVec3, lateral_offset: f64) -> DVec3 {
    let right = tangent.cross(*up).normalize();
    if lateral_offset >= 0.0 { -right } else { right }
}

/// The strip a platform's walkers use: [`NEAREST`] to [`FARTHEST`] (or the
/// device's own offset) beside the track over the platform's length, as a
/// ring that follows the track — near edge up, far edge back.
fn platform_strip(
    edge: &TrackEdge,
    device: &DeviceSource,
    platform: &PlatformPayload,
    walking: u32,
    zone: u8,
    seed: u64,
) -> PlacedWalkway {
    let start = device.s.clamp(0.0, edge.length());
    let end = (device.s + platform.length).clamp(0.0, edge.length());
    let steps = ((end - start) / STRIP_STEP).ceil().max(1.0) as usize;
    let at = |i: usize, distance: f64| {
        let s = start + (end - start) * i as f64 / steps as f64;
        let pose = edge.eval(s);
        let side = platform_side(&pose.tangent, &pose.up, device.lateral_offset);
        let base = EcefPos(pose.pos.0 + side * distance);
        let (lat, lon, _) = geo::from_ecef(base);
        let (e, n) = geo::to_utm(lat, lon, zone);
        PlacedVertex {
            pos: DVec2::new(e, n),
            base,
            height: platform.height.max(0.0),
            footing: if platform.height > 0.0 {
                Footing::Rail { up: pose.up }
            } else {
                Footing::Ground
            },
        }
    };
    // A lane along the platform behind the waiting crowd, walked up and down.
    let vertices: Vec<PlacedVertex> = (0..=steps).map(|i| at(i, LANE_DISTANCE)).collect();
    PlacedWalkway {
        name: platform.name.clone(),
        kind: PlacedKind::Path {
            width: LANE_WIDTH,
            people: walking,
        },
        centroid: centroid_of(&vertices),
        vertices,
        seed,
    }
}

/// A line walkway's vertex: on the ground of the tile it lands on, `height`
/// above it.
fn ground_vertex(point: &WalkPoint, height: f64, zone: u8) -> PlacedVertex {
    let (lat, lon) = (point.lat.to_radians(), point.lon.to_radians());
    let (e, n) = geo::to_utm(lat, lon, zone);
    PlacedVertex {
        pos: DVec2::new(e, n),
        base: geo::to_ecef(lat, lon, 0.0),
        height: height.max(0.0),
        footing: Footing::Ground,
    }
}

fn centroid_of(vertices: &[PlacedVertex]) -> DVec2 {
    vertices.iter().map(|v| v.pos).sum::<DVec2>() / vertices.len().max(1) as f64
}

/// Where a walkway vertex is in the world, the tile's ground asked where the
/// footing is the ground.
fn vertex_anchor(vertex: &PlacedVertex, grid: &HeightGrid) -> EcefPos {
    match vertex.footing {
        Footing::Rail { up } => EcefPos(vertex.base.0 + up * vertex.height),
        Footing::Ground => {
            let (lat, lon, _) = geo::from_ecef(vertex.base);
            geo::to_ecef(lat, lon, grid.at(vertex.pos) + vertex.height)
        }
    }
}

/// Places the crowd of the tile: on the platform's height above the rail
/// plane, or — where the line models no platform (`height` 0) — on the height
/// grid, which near the track is the formation beside the ballast.
pub(crate) fn scatter_people(
    k: TileKey,
    grid: &HeightGrid,
    frame: &EnuFrame,
    crowd: &Crowd,
) -> Vec<PersonInstance> {
    let Some(indices) = crowd.by_tile.get(&k) else {
        return Vec::new();
    };
    indices
        .iter()
        .map(|&i| {
            let person = &crowd.placed[i as usize];
            let anchor = if person.height > 0.0 {
                EcefPos(person.base.0 + person.up * person.height)
            } else {
                let ground = grid.at(person.pos);
                let (lat, lon, _) = geo::from_ecef(person.base);
                geo::to_ecef(lat, lon, ground)
            };
            PersonInstance {
                pos: to_render(frame.to_local(anchor)),
                rotation: model_rotation(frame, person.dir, person.up).to_array(),
                character: person.character,
                pose: person.pose,
                phase: person.phase,
            }
        })
        .collect()
}

/// Builds the walkways filed under the tile in the tile's frame, their
/// vertices on the ground (or the platform) where they stand, and draws their
/// people: the walkways with their agents, and the standing rest of the areas
/// as ordinary people of the tile.
// ponytail: a vertex outside the tile takes the height of the tile's border —
// the grid ends there. A way that crosses into the next tile is level with
// the border from there on, which on a footbridge is right and on a hillside
// is a few centimetres of floating.
pub(crate) fn scatter_walkways(
    k: TileKey,
    grid: &HeightGrid,
    frame: &EnuFrame,
    crowd: &Crowd,
) -> (Vec<Walkway>, Vec<PersonInstance>) {
    let Some(indices) = crowd.walkways_by_tile.get(&k) else {
        return (Vec::new(), Vec::new());
    };
    let characters = crowd.characters.len().min(usize::from(u16::MAX)) as u16;
    let mut walkways = Vec::with_capacity(indices.len());
    let mut standing = Vec::new();
    for &i in indices {
        let placed = &crowd.walkways[i as usize];
        let points: Vec<[f32; 3]> = placed
            .vertices
            .iter()
            .map(|v| to_render(frame.to_local(vertex_anchor(v, grid))))
            .collect();
        match placed.kind {
            PlacedKind::Path { width, people } => walkways.push(Walkway::path(
                &placed.name,
                points,
                width,
                people,
                characters,
                placed.seed,
            )),
            PlacedKind::Area {
                people,
                walking_share,
            } => {
                let (walkway, rest) = Walkway::area(
                    &placed.name,
                    points,
                    people,
                    walking_share,
                    characters,
                    placed.seed,
                );
                walkways.push(walkway);
                standing.extend(rest);
            }
        }
    }
    (walkways, standing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::{TerrainBuilder, TerrainOptions, TerrainStats, TerrainTile, key};
    use track_model::{EdgeId, Facing, NodeKind, Segment, TrackEdge};

    /// Straight 1 km test line at 52° N, 10° E, heading east.
    fn test_net() -> TrackNetwork {
        let mut net = TrackNetwork::new();
        let a = net.add_node(NodeKind::Buffer);
        let b = net.add_node(NodeKind::Buffer);
        net.add_edge(TrackEdge::new(
            EdgeId(0),
            a,
            b,
            geo::to_ecef_deg(52.0, 10.0, 100.0),
            0.0,
            vec![Segment::straight(1000.0)],
        ));
        net
    }

    fn platform(s: f64, lateral_offset: f64, payload: &str) -> DeviceSource {
        DeviceSource {
            kind: DeviceKind::Platform,
            edge: 0,
            s,
            facing: Facing::Both,
            lateral_offset,
            payload: payload.to_string(),
        }
    }

    fn characters() -> Vec<String> {
        vec!["people:a".into(), "people:b".into(), "people:c".into()]
    }

    fn crowd_of(devices: &[DeviceSource], seed: u64) -> Crowd {
        Crowd::from_parts(devices, &[], &[], &test_net(), 32, &characters(), seed)
    }

    /// The person's place along the edge and beside it [m]: `(s, left)`,
    /// positive `left` on the left of increasing arc length.
    fn along_and_beside(net: &TrackNetwork, base: EcefPos) -> (f64, f64) {
        let edge = &net.edges()[0];
        let start = edge.eval(0.0);
        let frame = EnuFrame::at(start.pos);
        let local = frame.to_local(base);
        // Heading east: east is `s`, north is left.
        (local.x, local.y)
    }

    /// The way a pose faces, as a horizontal unit vector `(x, z)`.
    fn facing(pose: &StrollPose) -> Vec2 {
        let f = Quat::from_rotation_y(pose.yaw) * Vec3::NEG_Z;
        Vec2::new(f.x, f.z).normalize()
    }

    /// Distance from a horizontal point to a polyline [m].
    fn distance_to_polyline(p: [f32; 3], points: &[[f32; 3]]) -> f32 {
        let q = Vec2::new(p[0], p[2]);
        points
            .windows(2)
            .map(|w| {
                let (a, b) = (Vec2::new(w[0][0], w[0][2]), Vec2::new(w[1][0], w[1][2]));
                let ab = b - a;
                let f = ((q - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
                (q - (a + ab * f)).length()
            })
            .fold(f32::INFINITY, f32::min)
    }

    #[test]
    fn the_crowd_is_deterministic_in_its_seed() {
        let devices = vec![platform(200.0, 5.0, "(name:\"A\",length:210.0)")];
        let seed = line_seed("Beispielstrecke");
        let once = crowd_of(&devices, seed);
        let again = crowd_of(&devices, seed);
        assert_eq!(once.placed, again.placed);
        assert_eq!(once.walkways, again.walkways);
        assert_eq!(once.len(), 35, "one person per six metres");
        assert_eq!(once.walking(), 11, "three in ten of them walk the platform");
        assert_eq!(once.placed.len(), 24);
        let other = crowd_of(&devices, seed ^ 1);
        assert_ne!(once.placed, other.placed);
        assert_ne!(once.walkways, other.walkways);
        // The seed of a name is the same on every machine.
        assert_eq!(line_seed("Beispielstrecke"), line_seed("Beispielstrecke"));
        assert_ne!(line_seed("Beispielstrecke"), line_seed("Musterbahn"));
    }

    #[test]
    fn everyone_stands_within_the_platform_on_its_side() {
        let net = test_net();
        for (offset, sign) in [(5.0, 1.0), (-2.0, -1.0)] {
            let devices = vec![platform(300.0, offset, "(name:\"A\",length:120.0)")];
            let crowd = crowd_of(&devices, 7);
            assert_eq!(crowd.len(), 20);
            assert_eq!((crowd.placed.len(), crowd.walking()), (14, 6));
            let spread = FARTHEST;
            for person in &crowd.placed {
                let (s, left) = along_and_beside(&net, person.base);
                assert!((300.0 - 0.01..=420.01).contains(&s), "s = {s}");
                let beside = left * sign;
                assert!(
                    (NEAREST - 0.01..=spread + 0.01).contains(&beside),
                    "beside = {beside}"
                );
                assert!((person.character as usize) < characters().len());
                assert!((0.0..1.0).contains(&person.phase));
                // Facing the track (give or take), or along it.
                let towards = -(person.dir.dot(person.base.0 - net.edges()[0].eval(s).pos.0));
                let along = person.dir.dot(net.edges()[0].eval(s).tangent).abs();
                assert!(
                    towards > 0.0 || along > 0.99,
                    "looks away from the track: {:?}",
                    person.dir
                );
            }
            // A mix of poses, not one of them.
            let idle = crowd.placed.iter().filter(|p| p.pose == Pose::Idle).count();
            assert!(idle > 0 && idle < crowd.placed.len());
            // The walkers' lane lies on the platform too: it follows the track
            // behind the waiting crowd, so the two never cross.
            assert_eq!(crowd.walkways.len(), 1);
            let lane = &crowd.walkways[0];
            assert_eq!(lane.name, "A");
            assert!(lane.vertices.len() >= 2);
            for vertex in &lane.vertices {
                let (s, left) = along_and_beside(&net, vertex.base);
                assert!((300.0 - 0.01..=420.01).contains(&s), "s = {s}");
                let beside = left * sign;
                assert!(
                    (LANE_DISTANCE - 0.01..=LANE_DISTANCE + 0.01).contains(&beside),
                    "beside = {beside}"
                );
            }
        }
    }

    #[test]
    fn a_platform_without_a_payload_is_skipped_and_nothing_else_makes_a_crowd() {
        let net = test_net();
        let none = crowd_of(&[], 1);
        assert!(none.is_empty());
        let devices = vec![
            DeviceSource {
                kind: DeviceKind::Signal,
                edge: 0,
                s: 100.0,
                facing: Facing::Forward,
                lateral_offset: 3.0,
                payload: "(signal:Some(0))".into(),
            },
            platform(100.0, 5.0, "(frequency:Hz1000)"),
            platform(100.0, 5.0, "garbage"),
            platform(100.0, 5.0, "(name:\"Short\",length:2.0)"),
        ];
        let crowd = crowd_of(&devices, 1);
        assert_eq!(
            crowd.len(),
            1,
            "the broken payloads are skipped, the short platform gets one"
        );
        assert_eq!(crowd.walking(), 0, "one person stands; nobody walks alone");
        // A platform on an edge the network does not have is skipped too.
        let stale = vec![DeviceSource {
            edge: 9,
            ..platform(100.0, 5.0, "(name:\"X\",length:60.0)")
        }];
        assert!(crowd_of(&stale, 1).is_empty());
        // Without characters there is nobody to place.
        let devices = vec![platform(100.0, 5.0, "(name:\"A\",length:60.0)")];
        assert!(Crowd::from_parts(&devices, &[], &[], &net, 32, &[], 1).is_empty());
    }

    #[test]
    fn the_crowd_stands_on_the_platform_or_on_the_ground() {
        let net = test_net();
        let devices = vec![
            platform(100.0, 5.0, "(name:\"Ground\",length:60.0)"),
            platform(700.0, -5.0, "(name:\"Built\",length:60.0,height:0.76)"),
        ];
        let crowd = crowd_of(&devices, 3);
        // Flat ground at rail height (the test line is at 100 m ellipsoidal): the
        // formation beside the ballast then lies `rail_offset` below the rail.
        let options = TerrainOptions {
            radius: 400.0,
            fallback_height: 100.0 - 46.0,
            ..Default::default()
        };
        let builder = TerrainBuilder::new(&net, vec![], options).with_crowd(crowd.clone());
        assert_eq!(builder.crowd_characters(), characters().as_slice());
        let mut stats = TerrainStats::default();
        let tiles: Vec<TerrainTile> = builder
            .corridor_keys()
            .into_iter()
            .filter_map(|k| builder.build_key(k, &mut stats))
            .collect();
        let placed: Vec<(&TerrainTile, &PersonInstance)> = tiles
            .iter()
            .flat_map(|t| t.people.iter().map(move |p| (t, p)))
            .collect();
        assert_eq!(
            placed.len(),
            14,
            "every standing person is on exactly one tile"
        );
        let world = |tile: &TerrainTile, q: [f32; 3]| {
            let frame = EnuFrame::at(tile.anchor);
            frame.to_ecef(DVec3::new(q[0] as f64, -q[2] as f64, q[1] as f64))
        };
        let rail = |s: f64| geo::from_ecef(net.edges()[0].eval(s).pos).2;
        let line_frame = EnuFrame::at(net.edges()[0].eval(0.0).pos);
        let mut on_ground = 0;
        let mut on_platform = 0;
        for (tile, person) in &placed {
            let foot = world(tile, person.pos);
            let (_, _, h) = geo::from_ecef(foot);
            let s = line_frame.to_local(foot).x;
            if s < 400.0 {
                // No platform body: on the ground as the tile draws it, which
                // beside the ballast is the formation below the rail head. The
                // tile is a 4 m grid, so the exact surface is only nearly it.
                let ground = builder.surface_height(foot);
                assert!((h - ground).abs() < 0.25, "{h:.2} on ground {ground:.2}");
                assert!(h <= rail(s) + 0.01 && h >= rail(s) - 0.41, "{h:.2} vs rail");
                on_ground += 1;
            } else {
                assert!((h - rail(s) - 0.76).abs() < 0.05, "{h:.2} on platform");
                on_platform += 1;
            }
            // Upright: the rotation's up is the local vertical.
            let up = glam::Quat::from_array(person.rotation) * glam::Vec3::Y;
            assert!(up.y > 0.999, "leaning: {up:?}");
        }
        assert_eq!((on_ground, on_platform), (7, 7));

        // The walkers: one strip per platform, three people on each, walking
        // the platform's surface — or the ground — over the whole of a round.
        let strips: Vec<(&TerrainTile, &Walkway)> = tiles
            .iter()
            .flat_map(|t| t.walkways.iter().map(move |w| (t, w)))
            .collect();
        assert_eq!(strips.len(), 2, "every strip is on exactly one tile");
        for (tile, strip) in &strips {
            assert_eq!(strip.len(), 3);
            assert!(matches!(strip.kind, WalkwayKind::Path { .. }));
            for agent in &strip.agents {
                let period = strip.period(agent);
                for step in 0..40 {
                    let pose = stroll_pose(strip, agent, period * step as f64 / 40.0);
                    let foot = world(tile, pose.position);
                    let (_, _, h) = geo::from_ecef(foot);
                    let s = line_frame.to_local(foot).x;
                    if strip.name == "Ground" {
                        assert!((100.0 - 0.5..=160.5).contains(&s), "s = {s}");
                        let ground = builder.surface_height(foot);
                        assert!((h - ground).abs() < 0.25, "{h:.2} on ground {ground:.2}");
                    } else {
                        assert!((700.0 - 0.5..=760.5).contains(&s), "s = {s}");
                        assert!((h - rail(s) - 0.76).abs() < 0.05, "{h:.2} on platform");
                    }
                }
            }
        }

        // The tile a person is filed under is the tile it lands on.
        let mut bucketed = crowd.clone();
        bucketed.bucket(options.tile_size);
        for person in &bucketed.placed {
            let k = key(person.pos, options.tile_size);
            assert!(!bucketed.by_tile[&k].is_empty());
        }
        for strip in &bucketed.walkways {
            let k = key(strip.centroid, options.tile_size);
            assert!(!bucketed.walkways_by_tile[&k].is_empty());
        }
        // Rescattering onto the built tile gives the same placement.
        let tile = placed[0].0;
        let (_, _, again) = builder.rescatter(tile);
        assert_eq!(again, tile.people);
    }

    #[test]
    fn sizes_and_poses_follow_their_shares() {
        assert_eq!(crowd_size(210.0), 35);
        assert_eq!(crowd_size(2.0), MIN_CROWD);
        assert_eq!(crowd_size(1_000.0), MAX_CROWD);
        assert_eq!(standing_pose(0.1, 0), Pose::Idle);
        assert_eq!(standing_pose(0.5, 0), Pose::Idle2);
        assert_eq!(standing_pose(0.8, 1), Pose::Stand2);
        assert!(Pose::Idle.is_looping() && !Pose::Stand3.is_looping());
        assert_eq!(Pose::Sit.clip(), "sit");
        assert_eq!(walking_count(35, PLATFORM_WALKING_SHARE), 11);
        assert_eq!(walking_count(1, PLATFORM_WALKING_SHARE), 0);
        assert_eq!(walking_count(10, 0.4), 4);
        assert_eq!(walking_count(10, 7.0), 10, "a share is held to 0 … 1");
    }

    /// Six people on a short way: spread round its oval by construction, with one
    /// lap time, they never come within a shoulder's width of one another.
    #[test]
    fn path_agents_keep_their_distance_for_ever() {
        let points = vec![[0.0, 0.0, 0.0], [12.0, 0.0, 0.0]];
        let way = Walkway::path("p", points, 2.0, 6, 4, 5);
        assert_eq!(way.len(), 6);
        let period = way.period(&way.agents[0]);
        let mut closest = f32::MAX;
        // Two laps with everybody's lap time the same is every configuration there is;
        // a third makes sure of it.
        let steps = (3.0 * period / 0.05) as usize;
        for step in 0..steps {
            let t = step as f64 * 0.05;
            let poses: Vec<_> = way.agents.iter().map(|a| stroll_pose(&way, a, t)).collect();
            for (i, a) in poses.iter().enumerate() {
                for b in &poses[i + 1..] {
                    let gap =
                        Vec2::new(a.position[0] - b.position[0], a.position[2] - b.position[2]);
                    closest = closest.min(gap.length());
                }
            }
        }
        assert!(closest > 0.6, "two walkers came within {closest} m");
    }

    /// An L-shaped footpath: three people walk it round and round inside its
    /// width — up one lane, round the ends, back down the other — never stopping,
    /// facing the way they go, at one pace.
    #[test]
    fn a_path_agent_walks_an_oval_inside_its_corridor() {
        let points = vec![[0.0, 0.0, 0.0], [10.0, 0.5, 0.0], [10.0, 1.0, -10.0]];
        let way = Walkway::path("l", points.clone(), 2.0, 3, 4, 11);
        assert_eq!(way.len(), 3);
        for agent in &way.agents {
            assert!((PATH_SPEED.0 as f32..=PATH_SPEED.1 as f32).contains(&agent.speed));
            // Right-hand traffic: everybody keeps to their right, a passing distance out.
            assert!(agent.lateral >= MIN_LATERAL as f32 - 1e-6);
            assert!(agent.lateral <= 1.0 * PATH_INSIDE as f32 + 1e-6);
            // One pace for the whole way, give or take the lane's length — nobody
            // overtakes — and so one lap time.
            let first = &way.agents[0];
            assert!((agent.speed - first.speed).abs() < 0.05 * first.speed);
            assert!((way.period(agent) - way.period(first)).abs() < 1e-3);
            assert!(agent.pauses.is_empty() && agent.waypoints.is_empty());
            let lap = PathLoop::new(&points, agent.lateral);
            let period = way.period(agent);
            let expected = lap.length() / f64::from(agent.speed);
            assert!((period - expected).abs() < 1e-3, "{period} vs {expected}");
            // A lap: two straights and two half circles of about the lane offset.
            let r = f64::from(agent.lateral);
            let oval = 2.0 * (20.0 - 2.0 * r) + 2.0 * std::f64::consts::PI * r;
            assert!(
                (lap.length() - oval).abs() < 1.0,
                "{} vs {oval}",
                lap.length()
            );

            let (mut up, mut down, mut walked) = (false, false, 0);
            let mut distance = 0.0f32;
            let mut previous: Option<StrollPose> = None;
            let dt = 0.1;
            let steps = (2.0 * period / dt) as usize;
            for step in 0..steps {
                let t = step as f64 * dt;
                let pose = stroll_pose(&way, agent, t);
                assert!(pose.moving, "a path agent never stands");
                assert!(
                    distance_to_polyline(pose.position, &points) <= 1.0 + 1e-3,
                    "off the way at {t}: {:?}",
                    pose.position
                );
                // The height comes with the vertices: never below the way, never
                // above its top.
                assert!((-0.01..=1.01).contains(&pose.position[1]));
                let f = facing(&pose);
                up |= f.x > 0.95;
                down |= f.x < -0.95;
                if let Some(last) = previous {
                    let step = Vec2::new(
                        pose.position[0] - last.position[0],
                        pose.position[2] - last.position[2],
                    );
                    if step.length() > 0.02 {
                        walked += 1;
                        distance += step.length();
                        // Faces its step — at a corner or on a turn, the step lies
                        // somewhere between the heading before and the one after.
                        let before = facing(&last);
                        let mean = (before + f).normalize_or_zero();
                        let direction = step.normalize();
                        assert!(
                            [before, mean, f].iter().any(|d| d.dot(direction) > 0.9),
                            "walks {step:?} but faces {f:?} at {t}"
                        );
                        // Never a jump, never a stop — a corner cuts a step short.
                        let pace = step.length() / dt as f32;
                        assert!(
                            pace > 0.6 * agent.speed && pace < 1.3 * agent.speed,
                            "{pace} vs {} at {t}: {:?} -> {:?}",
                            agent.speed,
                            last.position,
                            pose.position
                        );
                    }
                }
                previous = Some(pose);
            }
            assert!(up && down, "walks the first leg both ways");
            assert!(walked > 100);
            // And keeps its pace over the laps, give or take the lanes' corners.
            let pace = distance / (steps as f32 * dt as f32);
            assert!(
                (pace - agent.speed).abs() < 0.1 * agent.speed,
                "{pace} vs {}",
                agent.speed
            );
        }
    }

    /// The cycle repeats after its period and is the same for the same clock —
    /// which is what lets every client compute the same person in the same place.
    #[test]
    fn the_cycle_is_periodic_and_deterministic() {
        let path = Walkway::path("p", vec![[0.0; 3], [30.0, 0.0, 0.0]], 1.5, 2, 3, 5);
        let (area, _) = Walkway::area(
            "a",
            vec![
                [0.0; 3],
                [20.0, 0.0, 0.0],
                [20.0, 0.0, 15.0],
                [0.0, 0.0, 15.0],
            ],
            4,
            0.5,
            3,
            5,
        );
        for way in [&path, &area] {
            for (i, agent) in way.agents.iter().enumerate() {
                let period = way.period(agent);
                assert!(period > 0.0);
                for t in [0.0, 1.7, 33.3, 1234.5, 86_400.0 * 3.0] {
                    let a = way.pose(i, t).unwrap();
                    let b = way.pose(i, t + period).unwrap();
                    let c = way.pose(i, t).unwrap();
                    assert_eq!(a, c);
                    assert!(
                        (Vec3::from(a.position) - Vec3::from(b.position)).length() < 1e-2,
                        "{a:?} vs {b:?} a period later"
                    );
                    assert_eq!(a.moving, b.moving);
                }
            }
            assert!(way.pose(way.len(), 0.0).is_none());
        }
        // The same seed makes the same way; another seed another.
        assert_eq!(
            Walkway::path("p", vec![[0.0; 3], [30.0, 0.0, 0.0]], 1.5, 2, 3, 5),
            path
        );
        assert_ne!(
            Walkway::path("p", vec![[0.0; 3], [30.0, 0.0, 0.0]], 1.5, 2, 3, 6),
            path
        );
    }

    /// Area agents stay inside their polygon, stand at each spot facing the
    /// way they came, and walk on facing the way they go; the standing rest
    /// is inside too.
    // ponytail: a leg between two spots is a straight line, so in a concave
    // area it may cut a corner — the test polygon is convex.
    #[test]
    fn an_area_agent_stays_inside_its_polygon_and_keeps_its_heading_at_a_stop() {
        let polygon = vec![
            [0.0, 0.76, 0.0],
            [12.0, 0.76, 0.0],
            [14.0, 0.76, 8.0],
            [2.0, 0.76, 10.0],
        ];
        let (area, standing) = Walkway::area("wait", polygon.clone(), 10, 0.4, 3, 42);
        assert_eq!(area.len(), 4);
        assert_eq!(standing.len(), 6);
        for person in &standing {
            assert!(inside(person.pos[0].into(), person.pos[2].into(), &polygon));
            assert!((person.pos[1] - 0.76).abs() < 1e-5, "on the surface");
            let up = Quat::from_array(person.rotation) * Vec3::Y;
            assert!(up.y > 0.999);
            assert!(person.pose != Pose::Sit);
        }
        for agent in &area.agents {
            assert_eq!(agent.waypoints.len(), AREA_WAYPOINTS);
            assert_eq!(agent.pauses.len(), AREA_WAYPOINTS);
            assert!((AREA_SPEED.0 as f32..=AREA_SPEED.1 as f32).contains(&agent.speed));
            for spot in &agent.waypoints {
                assert!(inside(spot[0].into(), spot[2].into(), &polygon));
            }
            let period = area.period(agent);
            let mut previous: Option<StrollPose> = None;
            let mut stops = 0;
            let mut walked = 0;
            let dt = 0.1;
            for step in 0..(period / dt) as usize {
                let pose = stroll_pose(&area, agent, step as f64 * dt);
                assert!(
                    inside(pose.position[0].into(), pose.position[2].into(), &polygon),
                    "outside at {:?}",
                    pose.position
                );
                assert!((pose.position[1] - 0.76).abs() < 1e-4);
                if let Some(last) = previous {
                    match (last.moving, pose.moving) {
                        // Arriving: the heading is kept through the stand.
                        (true, false) => {
                            stops += 1;
                            assert!((pose.yaw - last.yaw).abs() < 1e-6);
                        }
                        (false, false) => assert!((pose.yaw - last.yaw).abs() < 1e-6),
                        (true, true) => {
                            let step = Vec2::new(
                                pose.position[0] - last.position[0],
                                pose.position[2] - last.position[2],
                            );
                            if step.length() > 0.02 {
                                walked += 1;
                                assert!(facing(&pose).dot(step.normalize()) > 0.99);
                            }
                        }
                        (false, true) => {}
                    }
                }
                previous = Some(pose);
            }
            assert!(stops >= AREA_WAYPOINTS - 1, "{stops} stops");
            assert!(walked > 20);
        }
        // Everything is a share: all wander, or all stand.
        let (all, none) = Walkway::area("w", polygon.clone(), 5, 1.0, 3, 1);
        assert_eq!((all.len(), none.len()), (5, 0));
        let (nobody, everybody) = Walkway::area("w", polygon.clone(), 5, 0.0, 3, 1);
        assert_eq!((nobody.len(), everybody.len()), (0, 5));
        // A sliver, or no polygon at all, is not a panic.
        let (sliver, rest) = Walkway::area(
            "s",
            vec![[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
            2,
            0.5,
            3,
            1,
        );
        assert_eq!(sliver.len() + rest.len(), 2);
        assert!(sliver.pose(0, 3.0).is_some());
        assert!(
            Walkway::area("e", vec![[0.0; 3], [1.0, 0.0, 0.0]], 2, 0.5, 3, 1)
                .0
                .is_empty()
        );
    }

    /// The yaw turns the model's face (−Z) onto the direction of travel.
    #[test]
    fn yaw_faces_the_direction_of_travel() {
        for (direction, expected) in [
            (Vec2::new(0.0, -1.0), 0.0),
            (Vec2::new(1.0, 0.0), -std::f32::consts::FRAC_PI_2),
            (Vec2::new(-1.0, 0.0), std::f32::consts::FRAC_PI_2),
            (Vec2::new(0.0, 1.0), std::f32::consts::PI),
        ] {
            let yaw = yaw_of(direction);
            assert!(
                (yaw - expected).abs() < 1e-6 || (yaw.abs() - expected.abs()).abs() < 1e-6,
                "{direction:?}: {yaw} vs {expected}"
            );
            let faced = Quat::from_rotation_y(yaw) * Vec3::NEG_Z;
            assert!((Vec2::new(faced.x, faced.z) - direction).length() < 1e-5);
        }
        // Left of north is west, in render axes.
        assert!((left_of(Vec2::new(0.0, -1.0)) - Vec2::new(-1.0, 0.0)).length() < 1e-6);
        // The mitre halves a right-angle corner; the ends take their leg's own.
        let corner = [[0.0; 3], [10.0, 0.0, 0.0], [10.0, 0.0, -10.0]];
        let m = mitre_normal(&corner, 1);
        assert!(
            (m - Vec2::new(-1.0, -1.0).normalize()).length() < 1e-5,
            "{m:?}"
        );
        assert!((mitre_normal(&corner, 0) - Vec2::new(0.0, -1.0)).length() < 1e-6);
        assert!((mitre_normal(&corner, 2) - Vec2::new(-1.0, 0.0)).length() < 1e-6);
        // A hairpin takes the leg ahead rather than a normal of no length.
        let hairpin = [[0.0; 3], [10.0, 0.0, 0.0], [0.0, 0.0, 0.0]];
        assert!((mitre_normal(&hairpin, 1) - Vec2::new(0.0, 1.0)).length() < 1e-6);
    }

    /// Node names: prefix, a name that may carry underscores, an index.
    #[test]
    fn walkway_node_names_parse() {
        assert_eq!(
            parse_walkway_node("wp_x_3"),
            Some((WalkwayTag::Path, "x", 3))
        );
        assert_eq!(
            parse_walkway_node("wa_lobby_0"),
            Some((WalkwayTag::Area, "lobby", 0))
        );
        assert_eq!(
            parse_walkway_node("wa_lobby_north_12"),
            Some((WalkwayTag::Area, "lobby_north", 12))
        );
        for bad in [
            "platform",
            "wp_3",
            "wp__3",
            "wp_x_a",
            "wq_x_1",
            "wp_x_",
            "char_LOD0",
        ] {
            assert_eq!(parse_walkway_node(bad), None, "{bad}");
        }
        let extras = WalkwayExtras::parse(r#"{"people": 6, "width": 1.6, "ts_function": "x"}"#);
        assert_eq!(extras.people, Some(6));
        assert_eq!(extras.width, Some(1.6));
        assert_eq!(extras.walking_share, None);
        // A float where an integer was meant is rounded; a wrong type is the
        // default for that field alone; anything but an object is all defaults.
        let odd = WalkwayExtras::parse(r#"{"people": 7.0, "width": "wide", "walking_share": 1}"#);
        assert_eq!(odd.people, Some(7));
        assert_eq!(odd.width, None);
        assert_eq!(odd.walking_share, Some(1.0));
        assert_eq!(WalkwayExtras::parse("garbage"), WalkwayExtras::default());
        assert_eq!(WalkwayExtras::parse("[1,2]"), WalkwayExtras::default());
    }

    /// The platform model's nodes become its footpath and its waiting area,
    /// sized by the `_0` node's extras, in a stable order, seeded per placement.
    #[test]
    fn embedded_walkways_are_built_out_of_their_nodes() {
        let node = |name: &str, x: f32, z: f32, extras: Option<&str>| WalkwayNode {
            name: name.into(),
            position: [x, 0.76, z],
            extras: extras.map(str::to_string),
        };
        // Handed over out of order, with a mesh node and a duplicate in between.
        let nodes = vec![
            node("wa_middle_2", -5.0, 202.0, None),
            node("platform", 0.0, 0.0, None),
            node("wp_edge_1", -2.0, 100.0, None),
            node(
                "wp_edge_0",
                -2.0,
                5.0,
                Some(r#"{"people": 6, "width": 1.6}"#),
            ),
            node(
                "wa_middle_0",
                -1.6,
                8.0,
                Some(r#"{"people": 10, "walking_share": 0.4}"#),
            ),
            node("wa_middle_1", -5.0, 8.0, None),
            node("wa_middle_3", -1.6, 202.0, None),
            node("wp_edge_2", -2.0, 205.0, None),
            node("wp_edge_2", -2.0, 999.0, None),
            node("wp_stub_0", 0.0, 0.0, None),
        ];
        let built = embedded_walkways(&nodes, 3, 24);
        assert_eq!(built.len(), 2, "the stub has one vertex and is no way");
        let (path, nobody) = &built[0];
        assert_eq!(path.name, "edge");
        assert_eq!(path.kind, WalkwayKind::Path { width: 1.6 });
        assert_eq!(
            path.points,
            vec![[-2.0, 0.76, 5.0], [-2.0, 0.76, 100.0], [-2.0, 0.76, 205.0]]
        );
        assert_eq!(path.len(), 6);
        assert!(nobody.is_empty());
        let (area, standing) = &built[1];
        assert_eq!(area.name, "middle");
        assert_eq!(area.kind, WalkwayKind::Area);
        assert_eq!(area.points.len(), 4);
        assert_eq!((area.len(), standing.len()), (4, 6));
        // Deterministic, and another placement is another crowd.
        assert_eq!(embedded_walkways(&nodes, 3, 24), built);
        assert_ne!(embedded_walkways(&nodes, 4, 24)[0].0, built[0].0);
        assert_ne!(embedded_seed(3, "edge"), embedded_seed(3, "middle"));
        // Without extras the documented defaults apply.
        let plain = vec![
            node("wp_a_0", 0.0, 0.0, None),
            node("wp_a_1", 5.0, 0.0, None),
            node("wa_b_0", 0.0, 0.0, None),
            node("wa_b_1", 5.0, 0.0, None),
            node("wa_b_2", 5.0, 5.0, None),
        ];
        let built = embedded_walkways(&plain, 0, 2);
        assert_eq!(built[0].0.kind, WalkwayKind::Path { width: 2.0 });
        assert_eq!(built[0].0.len(), 4);
        assert_eq!(built[1].0.len() + built[1].1.len(), 6);
        assert_eq!(built[1].0.len(), 3);
        // No characters: the ways are there, nobody is on them.
        assert!(
            embedded_walkways(&plain, 0, 0)
                .iter()
                .all(|(w, s)| w.is_empty() && s.is_empty())
        );
    }

    /// A footpath and a walk area of the line land on the tile of their
    /// centroid, their vertices on that tile's ground plus their own height.
    #[test]
    fn line_walkways_are_filed_under_one_tile_on_its_ground() {
        let net = test_net();
        let point = |lat: f64, lon: f64| WalkPoint { lat, lon };
        // 40 m north of the line, a bridge climbing 3 m; a forecourt 60 m north.
        let paths = vec![WalkPathSource {
            name: "Steg".into(),
            points: vec![point(52.0004, 10.0010), point(52.0004, 10.0018)],
            width: 2.0,
            people: 3,
            height: 3.0,
            tags: vec![],
        }];
        let areas = vec![WalkAreaSource {
            name: "Vorplatz".into(),
            polygon: vec![
                point(52.0005, 10.0020),
                point(52.0005, 10.0026),
                point(52.0008, 10.0026),
                point(52.0008, 10.0020),
            ],
            people: 8,
            walking_share: 0.5,
            height: 0.0,
            tags: vec![],
        }];
        let short = vec![WalkPathSource {
            points: vec![point(52.0, 10.0)],
            ..paths[0].clone()
        }];
        let crowd = Crowd::from_parts(&[], &paths, &areas, &net, 32, &characters(), 9);
        assert_eq!(crowd.walkways.len(), 2);
        assert_eq!((crowd.len(), crowd.walking()), (11, 7));
        assert!(Crowd::from_parts(&[], &short, &[], &net, 32, &characters(), 9).is_empty());

        let options = TerrainOptions {
            radius: 400.0,
            fallback_height: 100.0 - 46.0,
            ..Default::default()
        };
        let builder = TerrainBuilder::new(&net, vec![], options).with_crowd(crowd);
        let mut stats = TerrainStats::default();
        let tiles: Vec<TerrainTile> = builder
            .corridor_keys()
            .into_iter()
            .filter_map(|k| builder.build_key(k, &mut stats))
            .collect();
        let ways: Vec<(&TerrainTile, &Walkway)> = tiles
            .iter()
            .flat_map(|t| t.walkways.iter().map(move |w| (t, w)))
            .collect();
        assert_eq!(ways.len(), 2, "each way is on exactly one tile");
        let standing: usize = tiles.iter().map(|t| t.people.len()).sum();
        assert_eq!(
            standing, 4,
            "the forecourt's standing half is the tile's people"
        );
        for (tile, way) in &ways {
            let frame = EnuFrame::at(tile.anchor);
            let lift = if way.name == "Steg" { 3.0 } else { 0.0 };
            for p in &way.points {
                let foot = frame.to_ecef(DVec3::new(p[0] as f64, -p[2] as f64, p[1] as f64));
                let (_, _, h) = geo::from_ecef(foot);
                let ground = builder.surface_height(foot);
                assert!(
                    (h - ground - lift).abs() < 0.25,
                    "{h:.2} vs {ground:.2} + {lift}"
                );
            }
        }
        let bridge = ways.iter().find(|(_, w)| w.name == "Steg").unwrap().1;
        assert_eq!(bridge.kind, WalkwayKind::Path { width: 2.0 });
        assert_eq!(bridge.len(), 3);
        let forecourt = ways.iter().find(|(_, w)| w.name == "Vorplatz").unwrap().1;
        assert_eq!(forecourt.len(), 4);
        // Rescattering gives the same people, and the same ways.
        for tile in &tiles {
            let (_, _, again) = builder.rescatter(tile);
            assert_eq!(again, tile.people);
        }
    }
}
