//! Coupling and uncoupling — shunting (plan ch. 11, "train formation").
//!
//! Two consists become one, and one becomes two. Everything else that shunting looks
//! like — creeping up on a rake, setting back onto a road, standing with the brake on —
//! is the ordinary simulation with the reverser the other way round; only the moment the
//! coupling gear is worked needs a place of its own, and this is it.
//!
//! **Train indices never move.** [`Sim::trains`], [`Sim::runtime`] and [`Sim::controls`]
//! are addressed by index by the AI drivers, by the network protocol, by the render
//! components and by the score keeper, so nothing is ever removed from them: a consist
//! that has been coupled away keeps its slot and becomes an empty [`Train`] marked
//! [`stabled`](Train::stabled), which [`Sim::step`] already skips. An uncoupled part gets
//! a fresh slot at the end; a slot is never handed out twice.
//!
//! **Multiplayer.** The driver's side is a setpoint in [`CabInputs::shunt`], which is the
//! struct that travels — a client sends the command, the server applies it, and every peer
//! runs the same deterministic step over the same setpoints. Nothing about the *result* is
//! replicated: which vehicles ended up in which consist follows from the command and the
//! geometry, both of which every peer already has. Because a client's geometry may be a
//! few centimetres out at the moment of the command, a request that is refused stays on the
//! ground for [`PATIENCE`] seconds and is tried again every step, so the peer that was
//! short couples a moment later and lands on the same world rather than a different one.

use crate::Sim;
use crate::train::{ConsistEnd, CouplerState, Train};
use serde::{Deserialize, Serialize};
use track_model::{EdgeId, TrackNetwork, TrackPosition};

/// Shunting speed (Rangiergeschwindigkeit) [km/h] — what a shunting move is driven at in
/// Germany, and the cap the AI shunter holds itself to.
pub const SHUNTING_SPEED_KMH: f64 = 25.0;

/// What kind of movement a train is making (Ril 408 and 301).
///
/// German practice draws a hard line between the two, and almost everything about how a
/// movement is signalled hangs off which side of it the movement is on:
///
/// * A **Zugfahrt** carries a train number, is authorised by the main signals, and is only
///   let onto track that has been proved clear. It runs at the line speed.
/// * A **Rangierfahrt** carries none, is authorised by the shunting signals (Sh 1) and by
///   nothing else — Hp 1 does not apply to it — and *may* be let into an occupied track,
///   which is the whole point of shunting. It runs on sight at
///   [`SHUNTING_SPEED_KMH`], and the 2000 Hz magnet of a signal showing Sh 1 is switched
///   off, because otherwise every shunting movement past a signal at stop would be tripped.
///
/// A movement changes kind by passing a signal: under Sh 1 it becomes a shunting movement,
/// under a main proceed aspect a train movement. That is exactly how a shunt draws up to
/// the starting signal, is given a train route, and leaves as a train
/// ([`Sim::step`](crate::Sim::step) reads the aspect as the head passes it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Movement {
    /// A train movement — main signals, proved track, line speed.
    #[default]
    Train,
    /// A shunting movement — shunting signals, on sight, 25 km/h.
    Shunt,
}

impl Movement {
    /// The message key its name is shown under.
    pub fn key(self) -> &'static str {
        match self {
            Movement::Train => "movement-train",
            Movement::Shunt => "movement-shunt",
        }
    }

    /// The speed it is driven at where nothing else is more restrictive \[km/h\];
    /// `None` for a train movement, which takes the line speed.
    pub fn speed_limit(self) -> Option<f64> {
        match self {
            Movement::Train => None,
            Movement::Shunt => Some(SHUNTING_SPEED_KMH),
        }
    }
}

/// Where a shunting move ends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ShuntTarget {
    /// A point on the track graph, addressed like a timetable stop.
    At {
        edge: EdgeId,
        s: f64,
        /// Module whose local `edge` index this uses — resolved against the composed line
        /// by the mod runtime, then cleared, exactly like
        /// [`ScheduledStop::module`](crate::timetable::ScheduledStop::module).
        #[serde(default)]
        module: Option<String>,
    },
    /// A stabling road or portal of the line, by name
    /// ([`Sim::yard`](sim_core::Sim::yard)).
    Yard(String),
}

impl ShuntTarget {
    /// Where on the graph the move ends, as far as the run knows.
    pub fn position(&self, sim: &Sim) -> Option<TrackPosition> {
        match self {
            ShuntTarget::At { edge, s, .. } => Some(TrackPosition::new(*edge, *s, 1)),
            ShuntTarget::Yard(name) => sim.yard(name).map(|y| y.at),
        }
    }

    /// The module this target's `edge` index is local to, if it names one.
    pub fn module(&self) -> Option<&str> {
        match self {
            ShuntTarget::At { module, .. } => module.as_deref(),
            ShuntTarget::Yard(_) => None,
        }
    }

    /// Shifts a module-local edge index by that module's offset and forgets the module —
    /// the mod runtime's half of the `module` field. A target that names no module is an
    /// index of the composed line already and is left alone, and taking the name is what
    /// makes a second pass a no-op.
    pub fn shift(&mut self, offset: u32) {
        if let ShuntTarget::At { edge, module, .. } = self
            && module.take().is_some()
        {
            edge.0 += offset;
        }
    }
}

/// One move of a shunt job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ShuntMove {
    /// Draw forward until the head of the train is at the target, and stop.
    DrawUp(ShuntTarget),
    /// Set back until the rear of the train is at the target, and stop.
    ///
    /// The train is measured from its *tail*, which is the end that leads a reversing
    /// move — that is what "set back onto a road" means: the road takes the rear first.
    SetBack(ShuntTarget),
    /// Couple to whatever the train stands up against.
    Couple,
    /// Uncouple behind vehicle `n` of the consist.
    Uncouple(u16),
    /// Finished: stand with the brake applied and stay there.
    Stand,
}

/// A shunt job: the moves in the order they are driven.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ShuntJob {
    /// Name of the job, for the HUD and the log. Content, not translated.
    #[serde(default)]
    pub name: String,
    pub moves: Vec<ShuntMove>,
}

impl ShuntJob {
    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }

    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).expect("serializable")
    }
}

/// A consist counts as standing below this speed [m/s] (≈ 0.36 km/h).
///
/// Coupling gear is worked by hand between two vehicles; a train that is still rolling is
/// a train nobody steps in front of.
pub const STANDSTILL: f64 = 0.1;

/// Largest speed the two ends may still have towards each other [m/s] (≈ 1 km/h).
///
/// Buffers lock at walking pace; anything faster is a collision, not a coupling.
pub const CLOSING_SPEED: f64 = 0.3;

/// How far apart two buffer beams may be and still be within reach of the gear [m].
///
/// A screw coupling has enough travel in the link to be thrown over a shackle about a
/// metre away, which is what a shunter actually does — the two vehicles are not touching
/// when the coupling is made, they are pulled together by it.
pub const BUFFER_REACH: f64 = 1.0;

/// How long a refused request stays on the ground before it is given up [s].
///
/// It is the shunter waiting for the driver to finish easing up, and it is what keeps two
/// peers of a multiplayer session from ending up with different consists over a few
/// centimetres of position error.
pub const PATIENCE: f64 = 10.0;

/// The driver's shunting command — a setpoint like every other lever, and therefore
/// networked by [`CabInputs`](crate::cab::CabInputs) with no further work.
///
/// It is an order to the shunter on the ground, not a report of what happened: the answer
/// comes back as [`TrainRuntime::shunt`]. Holding the command down does not repeat it —
/// the rising edge is what counts, exactly like a push button on the desk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ShuntCommand {
    /// Nothing asked for.
    #[default]
    None,
    /// Couple to whatever the train stands up against, at either of its ends.
    Couple,
    /// Uncouple behind vehicle `n` of the consist — the coupler
    /// [`Train::couplers`]`[n]`, between `vehicles[n]` and `vehicles[n + 1]`.
    Uncouple(u16),
}

/// Why the shunter refused.
///
/// Every one of them is a refusal to do something surprising: nothing is ever half done,
/// and the driver is told which of the conditions was not met.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShuntError {
    /// No such train, or it is out of service.
    NoTrain,
    /// The consist has no coupler of that number.
    NoCoupler,
    /// The coupler is a bar inside a fixed unit, or the two heads do not match.
    IncompatibleCouplers,
    /// Somebody is still moving. Coupling gear is worked by hand.
    Moving,
    /// The two ends are closing on each other too fast to couple.
    TooFast,
    /// Nothing stands within reach of either end, on the track the switches are set for.
    NothingInReach,
    /// The two trains named are the same one.
    SameTrain,
}

impl ShuntError {
    /// Message key of the refusal, for the HUD.
    pub fn key(self) -> &'static str {
        match self {
            ShuntError::NoTrain => "shunt-refused-no-train",
            ShuntError::NoCoupler => "shunt-refused-no-coupler",
            ShuntError::IncompatibleCouplers => "shunt-refused-couplers",
            ShuntError::Moving => "shunt-refused-moving",
            ShuntError::TooFast => "shunt-refused-too-fast",
            ShuntError::NothingInReach => "shunt-refused-nothing-in-reach",
            ShuntError::SameTrain => "shunt-refused-same-train",
        }
    }
}

/// What became of the driver's last shunting command.
///
/// Local state, derived from the setpoints every peer already has — nothing here goes over
/// the wire.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum ShuntReport {
    /// Nothing asked for since the last command was answered.
    #[default]
    Idle,
    /// The shunter is on the ground and the conditions are not met yet; he waits.
    Waiting(ShuntError),
    /// Coupled: the other consist's vehicles are in this one now, and that consist is the
    /// empty slot named here.
    Coupled { emptied: usize },
    /// Uncoupled: the rear part is the train named here.
    Uncoupled { rear: usize },
    /// Given up after [`PATIENCE`] seconds.
    Refused(ShuntError),
}

/// A pending request: what was asked for and how long the shunter has been waiting.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct ShuntRequest {
    /// What the driver last put on the lever — the value the rising edge is measured
    /// against, so holding it down does not repeat the order.
    pub last: ShuntCommand,
    /// What is still to be done, and for how much longer it is tried.
    pub pending: Option<(ShuntCommand, f64)>,
    /// Simulation time the answer in [`crate::TrainRuntime::shunt`] was last written [s].
    /// The HUD lets it stand for a while and then drops it, like a scenario message.
    pub answered: f64,
}

/// Runs the pending shunting request of one train for a step.
///
/// Called from [`Sim::step`] before the physics, so a coupling made this step is one
/// consist by the time the couplers are worked out.
pub(crate) fn step(sim: &mut Sim, index: usize, dt: f64) {
    let command = sim.controls[index].shunt;
    let request = &mut sim.runtime[index].shunt_request;

    // Rising edge: a new order, and only a new one. A command held on the lever is the
    // shunter still standing there, not a second coupling.
    if command != request.last {
        request.last = command;
        request.pending = (command != ShuntCommand::None).then_some((command, PATIENCE));
        // Taking the order off calls the shunter back in; what he already did stands, so
        // the driver still reads that he coupled.
        if command == ShuntCommand::None
            && matches!(sim.runtime[index].shunt, ShuntReport::Waiting(_))
        {
            answer(sim, index, ShuntReport::Idle);
        }
    }

    let Some((pending, left)) = sim.runtime[index].shunt_request.pending else {
        return;
    };
    let outcome = match pending {
        ShuntCommand::None => return,
        ShuntCommand::Couple => sim
            .couple(index)
            .map(|emptied| ShuntReport::Coupled { emptied }),
        ShuntCommand::Uncouple(n) => sim
            .uncouple(index, n as usize)
            .map(|rear| ShuntReport::Uncoupled { rear }),
    };
    match outcome {
        Ok(report) => {
            answer(sim, index, report);
            sim.runtime[index].shunt_request.pending = None;
        }
        Err(why) => {
            let left = left - dt;
            if left > 0.0 {
                answer(sim, index, ShuntReport::Waiting(why));
                sim.runtime[index].shunt_request.pending = Some((pending, left));
            } else {
                answer(sim, index, ShuntReport::Refused(why));
                sim.runtime[index].shunt_request.pending = None;
            }
        }
    }
}

/// Writes the shunter's answer and stamps it with the clock.
fn answer(sim: &mut Sim, index: usize, report: ShuntReport) {
    sim.runtime[index].shunt = report;
    sim.runtime[index].shunt_request.answered = sim.time;
}

impl Sim {
    /// Splits `train` at the coupler behind vehicle `coupler`.
    ///
    /// The rear part becomes a train of its own — a new slot in [`Sim::trains`] with its
    /// own [`TrainRuntime`] and [`CabInputs`](crate::cab::CabInputs) entry — and its index
    /// is returned. The front part keeps `train`, so whoever asked keeps their index.
    ///
    /// **The brake pipe parts at that coupler.** The part that keeps the occupied cab
    /// keeps its pipe: the shunter closes its cock before he pulls the coupling, which is
    /// what lets the loco draw away. The other part's hose is left hanging — its pipe
    /// vents to atmosphere, its control valves apply, and it stands. None of that is
    /// modelled here; the cocks are set and [`crate::brakes`] does the rest, exactly as it
    /// does for a train that parts by accident.
    ///
    /// Refuses (and changes nothing) when the consist is still moving, when there is no
    /// such coupler, or when the coupler is a bar between two vehicles of one fixed unit.
    pub fn uncouple(&mut self, train: usize, coupler: usize) -> Result<usize, ShuntError> {
        let front = self.trains.get(train).ok_or(ShuntError::NoTrain)?;
        if front.stabled || front.vehicles.is_empty() {
            return Err(ShuntError::NoTrain);
        }
        if coupler + 1 >= front.vehicles.len() {
            return Err(ShuntError::NoCoupler);
        }
        // A bar is undone in the works, not on the ground. It joins two vehicles of the
        // same fixed unit, so both sides have to carry one — where the unit ends, the gear
        // is whatever the neighbour brings, and that comes apart like any other.
        if front.vehicles[coupler].spec.coupler.kind == crate::train::CouplerKind::Bar
            && front.vehicles[coupler + 1].spec.coupler.kind == crate::train::CouplerKind::Bar
        {
            return Err(ShuntError::IncompatibleCouplers);
        }
        if front.vehicles.iter().any(|v| v.v.abs() > STANDSTILL) {
            return Err(ShuntError::Moving);
        }

        let front = &mut self.trains[train];
        let mut vehicles = front.vehicles.split_off(coupler + 1);
        let mut couplers = front.couplers.split_off(coupler + 1);
        // `couplers[coupler]` is the one being pulled — it belongs to neither part.
        front.couplers.pop();
        couplers.truncate(vehicles.len().saturating_sub(1));

        // Whoever has the driver keeps the pipe; the other part is the one that brakes.
        let driver_stays = front.cab <= coupler;
        let cab = if driver_stays {
            front.cab
        } else {
            let moved = front.cab - (coupler + 1);
            front.cab = 0;
            moved
        };
        if let Some(last) = front.vehicles.last_mut() {
            last.brake.cock_rear = !driver_stays && last.spec.brake.pipe_rear;
        }
        if let Some(first) = vehicles.first_mut() {
            first.brake.cock_front = driver_stays && first.spec.brake.pipe_front;
        }

        let doors = crate::doors::DoorControl::new(
            vehicles.first().map(|v| v.spec.doors).unwrap_or_default(),
        );
        let rear = Train {
            vehicles,
            couplers,
            cab,
            rail: front.rail,
            number: String::new(),
            doors,
            // A part left behind is a movement of its own, and it has no number — which
            // is what a shunting movement is.
            movement: Movement::Shunt,
            stabled: false,
        };
        Ok(self.add_train(rear))
    }

    /// Couples `train` to whatever stands within reach of either of its ends.
    ///
    /// The other consist's vehicles move into this one — so whoever asked keeps their
    /// index and simply grows — and the other keeps its slot as an empty stabled consist,
    /// whose index is returned.
    ///
    /// See [`Sim::couple_to`] for the guards; this only picks the neighbour.
    pub fn couple(&mut self, train: usize) -> Result<usize, ShuntError> {
        let (other, _, _) = self.neighbour(train).ok_or(ShuntError::NothingInReach)?;
        self.couple_to(train, other)
    }

    /// Couples exactly these two consists.
    ///
    /// Refuses unless
    ///
    /// * both are in service and carry vehicles,
    /// * both stand still ([`STANDSTILL`]) and are not closing on each other faster than
    ///   [`CLOSING_SPEED`],
    /// * two of their ends are within [`BUFFER_REACH`] of each other **along the track
    ///   graph** — measured with [`TrackPosition::distance_to`], so a switch lying the
    ///   other way puts the two ends out of reach however near they are through the air,
    ///   and both ends point at each other rather than merely being close, and
    /// * the coupling gear matches ([`CouplerKind`](crate::train::CouplerKind)).
    ///
    /// The brake pipe is coupled through the joined train and the cocks at the two outer
    /// ends are shut — what a shunter does when a train is made up.
    pub fn couple_to(&mut self, train: usize, other: usize) -> Result<usize, ShuntError> {
        if train == other {
            return Err(ShuntError::SameTrain);
        }
        let a = self.trains.get(train).ok_or(ShuntError::NoTrain)?;
        let b = self.trains.get(other).ok_or(ShuntError::NoTrain)?;
        if a.stabled || b.stabled || a.vehicles.is_empty() || b.vehicles.is_empty() {
            return Err(ShuntError::NoTrain);
        }
        if a.speed().abs() > STANDSTILL || b.speed().abs() > STANDSTILL {
            return Err(ShuntError::Moving);
        }
        let (mine, theirs) = adjacent_ends(&self.net, a, b).ok_or(ShuntError::NothingInReach)?;

        // Closing speed: each train's own speed resolved along the direction that points
        // out of it at the coupling end. Both positive = the two are running together.
        let closing = a.speed() * mine.outward() + b.speed() * theirs.outward();
        if closing.abs() > CLOSING_SPEED {
            return Err(ShuntError::TooFast);
        }

        let head_of = |t: &Train, end: ConsistEnd| match end {
            ConsistEnd::Head => t.vehicles.first().expect("checked above").spec.coupler,
            ConsistEnd::Tail => t.vehicles.last().expect("checked above").spec.coupler,
        };
        if !head_of(a, mine).kind.couples_to(head_of(b, theirs).kind) {
            return Err(ShuntError::IncompatibleCouplers);
        }

        join(self, train, other, mine, theirs);
        Ok(other)
    }

    /// The consist within reach of one of `train`'s ends, with the two ends that meet.
    ///
    /// Only geometry — no speeds, no coupling gear. `None` when nothing is up against
    /// either end on the track the switches are currently set for.
    pub fn neighbour(&self, train: usize) -> Option<(usize, ConsistEnd, ConsistEnd)> {
        let a = self.trains.get(train)?;
        if a.stabled || a.vehicles.is_empty() {
            return None;
        }
        for (index, b) in self.trains.iter().enumerate() {
            if index == train || b.stabled || b.vehicles.is_empty() {
                continue;
            }
            if let Some((mine, theirs)) = adjacent_ends(&self.net, a, b) {
                return Some((index, mine, theirs));
            }
        }
        None
    }
}

/// The pair of ends of `a` and `b` that stand buffer to buffer, if any.
///
/// Reach is measured **both ways**: `a`'s end has to see `b`'s end ahead of it along the
/// graph, and `b`'s end has to see `a`'s. One direction alone would accept two ends that
/// are near each other but both pointing the same way — a train standing behind another
/// one, nose to tail, which is a following move and not a coupling.
fn adjacent_ends(net: &TrackNetwork, a: &Train, b: &Train) -> Option<(ConsistEnd, ConsistEnd)> {
    for mine in [ConsistEnd::Head, ConsistEnd::Tail] {
        let Some(pa) = outward_end(net, a, mine) else {
            continue;
        };
        for theirs in [ConsistEnd::Head, ConsistEnd::Tail] {
            let Some(pb) = outward_end(net, b, theirs) else {
                continue;
            };
            if in_reach(net, &pa, &pb) && in_reach(net, &pb, &pa) {
                return Some((mine, theirs));
            }
        }
    }
    None
}

/// The position of one end of the consist, turned to point *out* of it.
fn outward_end(net: &TrackNetwork, train: &Train, end: ConsistEnd) -> Option<TrackPosition> {
    let mut pos = train.end_position(net, end)?;
    if end == ConsistEnd::Tail {
        pos.dir = -pos.dir;
    }
    Some(pos)
}

/// Does `other` lie ahead of `from`, within buffer reach, along the track?
fn in_reach(net: &TrackNetwork, from: &TrackPosition, other: &TrackPosition) -> bool {
    from.distance_to(net, other, BUFFER_REACH)
        .is_some_and(|d| (-1e-6..=BUFFER_REACH).contains(&d))
}

/// Moves `other`'s vehicles into `train` and leaves `other` an empty stabled consist.
///
/// The ordering falls out of the two ends: the other consist goes in front when the
/// coupling is at our head and behind when it is at our tail, and it is turned round
/// exactly when the two ends are the same kind — two heads meeting face each other, a head
/// meeting a tail runs the same way. See the tests below, which walk all four cases.
fn join(sim: &mut Sim, train: usize, other: usize, mine: ConsistEnd, theirs: ConsistEnd) {
    let flip = mine == theirs;
    let mut taken = std::mem::take(&mut sim.trains[other].vehicles);
    let mut their_couplers = std::mem::take(&mut sim.trains[other].couplers);
    sim.trains[other].stabled = true;
    sim.trains[other].cab = 0;
    if flip {
        taken.reverse();
        their_couplers.reverse();
        for vehicle in &mut taken {
            // Turned round on the same track: it now runs the other way along `dir`, and
            // so do its speed and its acceleration.
            vehicle.pos.dir = -vehicle.pos.dir;
            vehicle.v = -vehicle.v;
            vehicle.a = -vehicle.a;
        }
    }

    // `x` only ever appears as a difference (the coupler extensions), so each part keeps
    // its own scale and the incoming one is shifted so the new coupler starts at rest.
    // Turning a part round mirrors its `x` axis with it.
    let scale = if flip { -1.0 } else { 1.0 };
    let ours = &sim.trains[train];
    let offset = match mine {
        ConsistEnd::Head => {
            let (last, first) = (taken.last(), ours.vehicles.first());
            match (last, first) {
                (Some(last), Some(first)) => {
                    let nominal = (last.spec.length + first.spec.length) / 2.0;
                    first.x + nominal - scale * last.x
                }
                _ => 0.0,
            }
        }
        ConsistEnd::Tail => {
            let (last, first) = (ours.vehicles.last(), taken.first());
            match (last, first) {
                (Some(last), Some(first)) => {
                    let nominal = (last.spec.length + first.spec.length) / 2.0;
                    last.x - nominal - scale * first.x
                }
                _ => 0.0,
            }
        }
    };
    for vehicle in &mut taken {
        vehicle.x = scale * vehicle.x + offset;
    }

    let count = taken.len();
    let ours = &mut sim.trains[train];
    match mine {
        ConsistEnd::Head => {
            taken.append(&mut ours.vehicles);
            ours.vehicles = taken;
            their_couplers.push(CouplerState::default());
            their_couplers.append(&mut ours.couplers);
            ours.couplers = their_couplers;
            // Everything of ours moved back by the length of what came in front.
            ours.cab += count;
        }
        ConsistEnd::Tail => {
            ours.vehicles.append(&mut taken);
            ours.couplers.push(CouplerState::default());
            ours.couplers.append(&mut their_couplers);
        }
    }

    // The hoses are connected through the joined train and the two end cocks shut — the
    // rest (charging the pipe, releasing the brake) is the driver's and the brake's.
    ours.couple_brake_pipe();
    ours.doors = crate::doors::DoorControl::new(
        ours.vehicles
            .first()
            .map(|v| v.spec.doors)
            .unwrap_or_default(),
    );
}

/// Default answer of the shunter — `serde`'s hook for [`crate::TrainRuntime::shunt`].
pub(crate) fn default_report() -> ShuntReport {
    ShuntReport::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brakes::BrakeSpec;
    use crate::train::{CouplerKind, CouplerSpec, Vehicle, VehicleSpec};
    use track_model::{EdgeId, NodeKind, Segment, TrackEdge, TrackNetwork};
    use world_coords::geo::to_ecef_deg;

    /// One 4 km straight, buffer stop at each end.
    fn net() -> TrackNetwork {
        let mut net = TrackNetwork::new();
        let a = net.add_node(NodeKind::Buffer);
        let b = net.add_node(NodeKind::Buffer);
        net.add_edge(TrackEdge::new(
            EdgeId(0),
            a,
            b,
            to_ecef_deg(52.0, 10.0, 100.0),
            0.0,
            vec![Segment::straight(4000.0)],
        ));
        net.finish();
        net
    }

    fn wagon(name: &str, kind: CouplerKind) -> VehicleSpec {
        VehicleSpec {
            name: name.into(),
            length: 20.0,
            mass_empty: 40_000.0,
            brake: BrakeSpec::from_brake_weight(30.0, crate::brakes::BrakeKind::Block),
            coupler: CouplerSpec {
                kind,
                ..CouplerSpec::screw()
            },
            ..VehicleSpec::default()
        }
    }

    fn sim_with(trains: Vec<(Vec<VehicleSpec>, f64, i8)>) -> Sim {
        let net = net();
        let mut sim = Sim::new(net, crate::interlock::Interlock::default(), 1);
        for (specs, head_s, dir) in trains {
            let head = TrackPosition::new(EdgeId(0), head_s, dir);
            let vehicles: Vec<Vehicle> = specs
                .into_iter()
                .map(|spec| Vehicle::new(spec, head))
                .collect();
            let train = Train::assemble(vehicles, head, &sim.net);
            sim.add_train(train);
        }
        sim
    }

    /// Two rakes standing buffer to buffer become one train, and the one that asked keeps
    /// its index while the other keeps its slot as an empty stabled consist.
    #[test]
    fn two_rakes_standing_against_each_other_become_one_train() {
        // A runs towards increasing s with its head at 1000; B stands beyond it facing
        // back, so its head is at 1000.4 — four decimetres of gap, well inside the gear.
        let mut sim = sim_with(vec![
            (vec![wagon("a0", CouplerKind::Screw); 2], 1000.0, 1),
            (vec![wagon("b0", CouplerKind::Screw); 3], 1000.4, -1),
        ]);
        let emptied = sim.couple_to(0, 1).expect("couples");
        assert_eq!(emptied, 1);
        assert_eq!(sim.trains[0].vehicles.len(), 5);
        assert_eq!(sim.trains[0].couplers.len(), 4);
        assert!(sim.trains[1].vehicles.is_empty());
        assert!(sim.trains[1].stabled, "the empty slot is out of service");
        // Nothing was removed anywhere.
        assert_eq!(sim.trains.len(), 2);
        assert_eq!(sim.runtime.len(), 2);
        assert_eq!(sim.controls.len(), 2);
        // Head to head: what came in front is turned round, so it runs our way now.
        for vehicle in &sim.trains[0].vehicles {
            assert_eq!(vehicle.pos.dir, 1, "the whole train faces one way");
        }
        // The vehicles line up head to rear with no gap and no overlap.
        for pair in sim.trains[0].vehicles.windows(2) {
            let nominal = (pair[0].spec.length + pair[1].spec.length) / 2.0;
            assert!(
                (pair[0].x - pair[1].x - nominal).abs() < 0.5,
                "x runs down the joined consist"
            );
        }
        // Pipe coupled through, end cocks shut.
        let n = sim.trains[0].vehicles.len();
        for (i, vehicle) in sim.trains[0].vehicles.iter().enumerate() {
            assert_eq!(vehicle.brake.cock_front, i > 0);
            assert_eq!(vehicle.brake.cock_rear, i + 1 < n);
        }
    }

    /// The four ways two consists can meet all give one consist that runs one way, with
    /// the asker's own vehicles in their own order.
    #[test]
    fn all_four_ends_join_into_one_consist() {
        // A stands on 960 … 1000 with its head at 1000. Each of the four is the other
        // consist put buffer to buffer against one of A's two ends, one of its own two
        // ends towards it: head–tail, head–head, tail–head, tail–tail.
        for (b_head, b_dir) in [(1040.4, 1i8), (1000.4, -1), (959.6, 1), (919.6, -1)] {
            let mut sim = sim_with(vec![
                (vec![wagon("a", CouplerKind::Screw); 2], 1000.0, 1),
                (vec![wagon("b", CouplerKind::Screw); 2], b_head, b_dir),
            ]);
            let before: Vec<String> = sim.trains[0]
                .vehicles
                .iter()
                .map(|v| v.spec.name.clone())
                .collect();
            sim.couple_to(0, 1)
                .unwrap_or_else(|e| panic!("b at {b_head} dir {b_dir}: {e:?}"));
            let joined = &sim.trains[0];
            assert_eq!(joined.vehicles.len(), 4);
            let dir = joined.vehicles[0].pos.dir;
            assert!(
                joined.vehicles.iter().all(|v| v.pos.dir == dir),
                "one direction of travel for the whole train"
            );
            let names: Vec<String> = joined
                .vehicles
                .iter()
                .map(|v| v.spec.name.clone())
                .collect();
            assert!(
                names.starts_with(&before) || names.ends_with(&before),
                "our own vehicles stay in their own order"
            );
            for pair in joined.vehicles.windows(2) {
                let nominal = (pair[0].spec.length + pair[1].spec.length) / 2.0;
                assert!((pair[0].x - pair[1].x - nominal).abs() < 0.5);
            }
        }
    }

    /// The guards refuse rather than doing something surprising.
    #[test]
    fn coupling_is_refused_when_a_condition_is_not_met() {
        // Too far apart: five metres is not buffer reach.
        let mut sim = sim_with(vec![
            (vec![wagon("a", CouplerKind::Screw); 2], 1000.0, 1),
            (vec![wagon("b", CouplerKind::Screw); 2], 1005.0, -1),
        ]);
        assert_eq!(sim.couple_to(0, 1), Err(ShuntError::NothingInReach));

        // Two consists a few decimetres apart along the same road, both facing the same
        // way: near enough, and no pair of ends is against another. Reach is measured
        // from both sides for exactly this — one side alone would call it a coupling.
        let mut sim = sim_with(vec![
            (vec![wagon("a", CouplerKind::Screw); 2], 1000.0, 1),
            (vec![wagon("b", CouplerKind::Screw); 2], 1000.4, 1),
        ]);
        assert_eq!(sim.couple_to(0, 1), Err(ShuntError::NothingInReach));

        // In reach, but rolling.
        let mut sim = sim_with(vec![
            (vec![wagon("a", CouplerKind::Screw); 2], 1000.0, 1),
            (vec![wagon("b", CouplerKind::Screw); 2], 1000.4, -1),
        ]);
        for vehicle in &mut sim.trains[0].vehicles {
            vehicle.v = 1.0;
        }
        assert_eq!(sim.couple_to(0, 1), Err(ShuntError::Moving));
        for vehicle in &mut sim.trains[0].vehicles {
            vehicle.v = 0.0;
        }

        // A screw coupling and a Scharfenberg head do not meet.
        let mut sim = sim_with(vec![
            (vec![wagon("a", CouplerKind::Screw); 2], 1000.0, 1),
            (vec![wagon("b", CouplerKind::CenterBuffer); 2], 1000.4, -1),
        ]);
        assert_eq!(sim.couple_to(0, 1), Err(ShuntError::IncompatibleCouplers));

        // And a train never couples to itself.
        assert_eq!(sim.couple_to(0, 0), Err(ShuntError::SameTrain));
        assert_eq!(sim.couple_to(0, 9), Err(ShuntError::NoTrain));
    }

    /// A switch lying the other way puts two ends out of reach however close they are
    /// through the air — the reach is measured along the track, not across the ballast.
    #[test]
    fn a_switch_the_other_way_puts_the_two_ends_out_of_reach() {
        use track_model::{EdgeEnd, EdgeSide, Switch, SwitchPosition};
        let mut net = TrackNetwork::new();
        let a = net.add_node(NodeKind::Buffer);
        let b = net.add_node(NodeKind::Joint);
        let c = net.add_node(NodeKind::Buffer);
        let d = net.add_node(NodeKind::Buffer);
        let anchor = to_ecef_deg(52.0, 10.0, 100.0);
        let e0 = net.add_edge(TrackEdge::new(
            EdgeId(0),
            a,
            b,
            anchor,
            0.0,
            vec![Segment::straight(500.0)],
        ));
        let p = net.edge(e0).end_pose();
        let e1 = net.add_edge(TrackEdge::new(
            EdgeId(0),
            b,
            c,
            p.pos,
            0.0,
            vec![Segment::straight(500.0)],
        ));
        let e2 = net.add_edge(TrackEdge::new(
            EdgeId(0),
            b,
            d,
            p.pos,
            0.0,
            vec![
                Segment::transition(60.0, 0.0, -1.0 / 300.0),
                Segment::arc(100.0, -300.0),
            ],
        ));
        net.node_mut(b).kind = NodeKind::Switch(Switch::new(
            EdgeEnd::new(e0, EdgeSide::End),
            EdgeEnd::new(e1, EdgeSide::Start),
            EdgeEnd::new(e2, EdgeSide::Start),
        ));
        net.finish();

        let mut sim = Sim::new(net, crate::interlock::Interlock::default(), 1);
        // One wagon with its head right at the points, one on the diverging leg with its
        // head right past them, facing back — eight decimetres apart over the node.
        for (edge, s, dir) in [(e0, 499.6, 1), (e2, 0.4, -1)] {
            let head = TrackPosition::new(edge, s, dir);
            let vehicles = vec![Vehicle::new(wagon("w", CouplerKind::Screw), head)];
            let train = Train::assemble(vehicles, head, &sim.net);
            sim.add_train(train);
        }
        // Points set for the straight: the two are metres apart and still out of reach.
        assert_eq!(sim.couple_to(0, 1), Err(ShuntError::NothingInReach));
        sim.net
            .switch_mut(b)
            .expect("switch")
            .command(SwitchPosition::Diverging)
            .expect("commands");
        sim.net.update_switches(10.0);
        assert!(sim.couple_to(0, 1).is_ok(), "now the road leads there");
    }

    /// Uncoupling makes a train of the rear part, keeps every index and parts the pipe so
    /// the rear brakes itself while the front keeps its air.
    #[test]
    fn uncoupling_makes_a_train_of_the_rear_part_and_parts_the_pipe() {
        let mut sim = sim_with(vec![(vec![wagon("w", CouplerKind::Screw); 5], 1000.0, 1)]);
        sim.trains[0].couple_brake_pipe();
        let rear = sim.uncouple(0, 1).expect("uncouples");
        assert_eq!(rear, 1, "the rear part gets a slot of its own");
        assert_eq!(sim.trains[0].vehicles.len(), 2);
        assert_eq!(sim.trains[0].couplers.len(), 1);
        assert_eq!(sim.trains[1].vehicles.len(), 3);
        assert_eq!(sim.trains[1].couplers.len(), 2);
        assert_eq!(sim.runtime.len(), 2);
        assert_eq!(sim.controls.len(), 2);
        // The driver stayed at the front, so the front kept its pipe and the rear's hose
        // hangs — which is what makes the rear part brake itself.
        assert!(!sim.trains[0].vehicles[1].brake.cock_rear);
        assert!(sim.trains[1].vehicles[0].brake.cock_front);

        // Charged pipes, then let the brake run: the parted part loses its air.
        for train in &mut sim.trains {
            for vehicle in &mut train.vehicles {
                vehicle.brake.pipe = 5.0;
                vehicle.brake.aux_reservoir = 5.0;
                vehicle.brake.main_reservoir = 9.0;
            }
        }
        for _ in 0..2000 {
            sim.step(Sim::DT);
        }
        assert!(
            sim.trains[1].vehicles[0].brake.pipe < 1.0,
            "the parted pipe drops"
        );
        assert!(
            sim.trains[1].vehicles[0].brake.cylinder > 1.0,
            "and the rear part brakes itself"
        );
    }

    /// A bar inside a fixed unit is not a coupler a shunter works.
    #[test]
    fn a_bar_inside_a_unit_cannot_be_uncoupled() {
        let mut sim = sim_with(vec![(
            vec![
                wagon("a", CouplerKind::Bar),
                wagon("b", CouplerKind::Bar),
                wagon("c", CouplerKind::Screw),
            ],
            1000.0,
            1,
        )]);
        assert_eq!(sim.uncouple(0, 0), Err(ShuntError::IncompatibleCouplers));
        assert_eq!(sim.uncouple(0, 5), Err(ShuntError::NoCoupler));
        // The last coupler of the unit is a screw one and comes apart.
        assert!(sim.uncouple(0, 1).is_ok());
    }

    /// A moving consist is never split — coupling gear is worked by hand.
    #[test]
    fn a_rolling_consist_is_not_uncoupled() {
        let mut sim = sim_with(vec![(vec![wagon("w", CouplerKind::Screw); 3], 1000.0, 1)]);
        for vehicle in &mut sim.trains[0].vehicles {
            vehicle.v = 2.0;
        }
        assert_eq!(sim.uncouple(0, 1), Err(ShuntError::Moving));
        assert_eq!(sim.trains.len(), 1);
    }

    /// Stepping a simulation that holds an empty stabled consist does not panic — that is
    /// what the whole index rule rests on.
    #[test]
    fn stepping_a_sim_with_an_empty_stabled_train_does_not_panic() {
        let mut sim = sim_with(vec![
            (vec![wagon("a", CouplerKind::Screw); 2], 1000.0, 1),
            (vec![wagon("b", CouplerKind::Screw); 2], 1000.4, -1),
        ]);
        sim.couple_to(0, 1).expect("couples");
        assert!(sim.trains[1].vehicles.is_empty());
        for _ in 0..400 {
            sim.step(Sim::DT);
        }
        // And an empty consist that was never coupled away is just as harmless.
        let head = sim.trains[0].head_position();
        let net = std::mem::replace(&mut sim.net, track_model::TrackNetwork::new());
        let empty = sim.add_train(Train::assemble(Vec::new(), head, &net));
        sim.net = net;
        sim.trains[empty].stabled = true;
        for _ in 0..400 {
            sim.step(Sim::DT);
        }
        assert_eq!(sim.trains.len(), 3);
        // A train with no vehicles answers "nowhere" instead of panicking.
        assert!(sim.trains[empty].head().is_none());
        assert!(
            sim.trains[empty]
                .end_position(&sim.net, ConsistEnd::Head)
                .is_none()
        );
    }

    /// The command in the cab is a push button: held down it couples once, and a refusal
    /// waits on the ground before it is given up.
    #[test]
    fn the_cab_command_fires_once_and_a_refusal_waits() {
        let mut sim = sim_with(vec![
            (vec![wagon("a", CouplerKind::Screw); 2], 1000.0, 1),
            (vec![wagon("b", CouplerKind::Screw); 2], 1000.4, -1),
            (vec![wagon("c", CouplerKind::Screw); 2], 3000.0, 1),
        ]);
        sim.controls[0].shunt = ShuntCommand::Couple;
        sim.step(Sim::DT);
        assert!(matches!(
            sim.runtime[0].shunt,
            ShuntReport::Coupled { emptied: 1 }
        ));
        // Held down: nothing further happens, however long it is left there.
        for _ in 0..200 {
            sim.step(Sim::DT);
        }
        assert_eq!(sim.trains[0].vehicles.len(), 4);
        assert_eq!(sim.trains[2].vehicles.len(), 2);

        // A command that cannot be met waits, then is given up with a reason.
        sim.controls[2].shunt = ShuntCommand::Couple;
        sim.step(Sim::DT);
        assert_eq!(
            sim.runtime[2].shunt,
            ShuntReport::Waiting(ShuntError::NothingInReach)
        );
        for _ in 0..(PATIENCE / Sim::DT) as usize + 2 {
            sim.step(Sim::DT);
        }
        assert_eq!(
            sim.runtime[2].shunt,
            ShuntReport::Refused(ShuntError::NothingInReach)
        );
    }
}
