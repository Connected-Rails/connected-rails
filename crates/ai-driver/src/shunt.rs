//! Shunt jobs an AI driver can be given (plan ch. 11, "train formation").
//!
//! A job is a list of moves worked off in order: draw forward to a point, set back onto a
//! road, couple to what stands there, uncouple at a coupler, stand. It is the same driver
//! as always — [`AiDriver`](crate::AiDriver) writes [`CabInputs`] and nothing else, so a
//! shunting move is driven with the reverser, the power controller and the brake valve the
//! way the player would drive it. The coupling itself is
//! [`ShuntCommand`](sim_core::shunt::ShuntCommand) on the same struct, which is what makes
//! the whole job work over the network without a message of its own.
//!
//! Shunting speed is the German Rangiergeschwindigkeit,
//! [`SHUNTING_SPEED_KMH`](sim_core::shunt::SHUNTING_SPEED_KMH) — 25 km/h, and the driver
//! creeps well below it over the last few metres onto a coupling.

use serde::{Deserialize, Serialize};
use sim_core::Sim;
use sim_core::shunt::{Movement, SHUNTING_SPEED_KMH, ShuntCommand, ShuntReport};
use sim_core::train::ConsistEnd;

// The job itself is content, so it lives with the rest of the shunting model in
// `sim-core` — a scenario and an operating day both hand one out, and neither of them
// knows what an AI driver is. What stays here is the *driving* of it.
pub use sim_core::shunt::{ShuntJob, ShuntMove, ShuntTarget};

/// How near the target the move counts as finished [m].
///
/// A shunting move is stopped by eye, at a signal, a mark or a shunter's arm; a couple of
/// metres is what that comes to and is well inside the reach of the coupling gear.
pub const ARRIVED: f64 = 3.0;

/// Speed the driver creeps at over the last stretch onto a coupling [km/h].
///
/// Buffers are met at walking pace, not at shunting speed.
pub const CREEP_SPEED_KMH: f64 = 5.0;

/// Distance the creep starts at [m].
pub const CREEP_FROM: f64 = 25.0;

/// Deceleration a shunting move is planned with [m/s²] — gentler than a running brake,
/// because a shunter is walking beside the train.
pub const SHUNT_DECEL: f64 = 0.35;

/// How far ahead a target is looked for [m]. A shunting move is short by nature.
pub const SHUNT_LOOKAHEAD: f64 = 2_000.0;

/// What the driver is doing inside a shunt job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ShuntPhase {
    /// Running the current move.
    #[default]
    Running,
    /// Waiting for the shunter on the ground to answer a coupling command.
    Working,
    /// The job is done; the train stands.
    Done,
}

/// State of a driver working a shunt job.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ShuntState {
    pub job: ShuntJob,
    /// Index of the move being driven.
    pub next: usize,
    pub phase: ShuntPhase,
}

impl ShuntState {
    pub fn new(job: ShuntJob) -> Self {
        Self {
            job,
            next: 0,
            phase: ShuntPhase::Running,
        }
    }

    /// The move being driven, if the job is not finished.
    pub fn current(&self) -> Option<&ShuntMove> {
        self.job.moves.get(self.next)
    }

    /// Whether there is still something to do.
    pub fn active(&self) -> bool {
        self.phase != ShuntPhase::Done && self.next < self.job.moves.len()
    }

    /// One step of the shunting driver: reads the simulation and writes the cab inputs.
    ///
    /// Only [`CabInputs`](sim_core::cab::CabInputs) is written — the driver has no more
    /// power over the world than the player does.
    pub fn drive(&mut self, sim: &mut Sim, train: usize) {
        if sim.trains[train].vehicles.is_empty() {
            self.phase = ShuntPhase::Done;
            return;
        }
        // A driver working a job is making a shunting movement, and everything downstream
        // reads that: shunting speed, Sh 1 rather than Hp 1, and a track that may be
        // occupied (`sim_core::shunt::Movement`). It is the standing order he was given;
        // a signal he passes may still change it back.
        sim.trains[train].movement = Movement::Shunt;
        let Some(current) = self.job.moves.get(self.next).cloned() else {
            self.phase = ShuntPhase::Done;
            hold(sim, train);
            return;
        };

        match current {
            ShuntMove::DrawUp(target) => self.run(sim, train, &target, ConsistEnd::Head),
            ShuntMove::SetBack(target) => self.run(sim, train, &target, ConsistEnd::Tail),
            ShuntMove::Couple => self.work(sim, train, ShuntCommand::Couple),
            ShuntMove::Uncouple(n) => self.work(sim, train, ShuntCommand::Uncouple(n)),
            ShuntMove::Stand => {
                hold(sim, train);
                self.phase = ShuntPhase::Done;
            }
        }
    }

    /// Drives one end of the train onto a target and stops there.
    fn run(&mut self, sim: &mut Sim, train: usize, target: &ShuntTarget, leading: ConsistEnd) {
        let Some(mark) = target.position(sim) else {
            // A target the line does not have is a job that cannot be driven. The train
            // stands rather than running on to nowhere.
            hold(sim, train);
            self.phase = ShuntPhase::Done;
            return;
        };
        let Some(from) = sim.trains[train].end_position(&sim.net, leading) else {
            self.phase = ShuntPhase::Done;
            return;
        };
        // Distance measured along the graph in the direction the move runs: at the tail
        // that is against the consist's own direction of travel.
        let reversing = leading == ConsistEnd::Tail;
        let mut ahead = from;
        if reversing {
            ahead.dir = -ahead.dir;
        }
        let distance = ahead
            .distance_to(&sim.net, &mark, SHUNT_LOOKAHEAD)
            .unwrap_or(0.0);
        let v_kmh = sim.trains[train].speed_kmh().abs();

        // A shunting move ends when the buffers are met, whatever the mark said — the
        // shunter's arm is what stops it, and only ever at the end that leads. The rake
        // left behind by an uncoupling sits against the *other* end and is none of this
        // move's business.
        let met = sim
            .neighbour(train)
            .is_some_and(|(_, mine, _)| mine == leading);
        if met || (distance <= ARRIVED && v_kmh < 0.5) {
            hold(sim, train);
            self.next += 1;
            return;
        }

        // The cap: shunting speed, the line's own limit where it is lower, whatever the
        // shunting signals ahead permit, the braking curve onto the target, and a creep
        // over the last few metres. A shunting movement is stopped by Sh 0 like any
        // other — a job is an order to move, not a licence to pass a signal.
        let line = from.speed_limit(&sim.net);
        let mut target_kmh = SHUNTING_SPEED_KMH.min(line);
        let ahead_view = sim_core::lookahead::scan(
            &sim.net,
            &sim.interlock,
            ahead,
            SHUNT_LOOKAHEAD,
            Movement::Shunt,
        );
        target_kmh = target_kmh.min(ahead_view.permitted(SHUNT_DECEL, ARRIVED));
        let remaining = (distance - ARRIVED).max(0.0);
        target_kmh = target_kmh.min((2.0 * SHUNT_DECEL * remaining).sqrt() * 3.6);
        if distance < CREEP_FROM {
            target_kmh = target_kmh.min(CREEP_SPEED_KMH);
        }
        if let Some(limit) = sim.runtime[train].protection.speed_limit {
            target_kmh = target_kmh.min(limit - 1.0);
        }
        let target_kmh = target_kmh.max(0.0);

        let cab = &mut sim.controls[train];
        cab.shunt = ShuntCommand::None;
        cab.reverser = if reversing { -1 } else { 1 };
        crate::AiDriver::apply_speed_control(cab, v_kmh, target_kmh);
    }

    /// Gives the shunter on the ground his order and waits for the answer.
    ///
    /// The handshake runs over the request rather than over the report: the order goes on
    /// the lever, stays there while the shunter is on the ground, and comes off again the
    /// moment he is finished — which is what leaves the lever clean for the next move.
    /// Reading the report alone would take the *previous* move's answer for this one's.
    fn work(&mut self, sim: &mut Sim, train: usize, command: ShuntCommand) {
        hold(sim, train);
        let request = sim.runtime[train].shunt_request;
        if request.last != command || request.pending.is_some() {
            sim.controls[train].shunt = command;
            self.phase = ShuntPhase::Working;
            return;
        }
        sim.controls[train].shunt = ShuntCommand::None;
        if matches!(sim.runtime[train].shunt, ShuntReport::Refused(_)) {
            // The shunter gave up. So does the job — a move that cannot be made is not
            // driven round.
            self.phase = ShuntPhase::Done;
            return;
        }
        self.next += 1;
        self.phase = ShuntPhase::Running;
    }
}

/// Brings the train to a stand and holds it there — the end of every shunting move, and
/// what a driver whose job is worked does for the rest of the run.
pub fn hold(sim: &mut Sim, train: usize) {
    let cab = &mut sim.controls[train];
    cab.throttle = 0.0;
    cab.brake_valve = sim_core::brakes::DriverBrakeValve::Service(1.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use track_model::EdgeId;

    /// A job reads back out of RON exactly as it went in — it is content like a timetable.
    #[test]
    fn a_job_survives_the_round_trip_through_ron() {
        let job = ShuntJob {
            name: "Übergabe Musterstadt".into(),
            moves: vec![
                ShuntMove::DrawUp(ShuntTarget::At {
                    edge: EdgeId(2),
                    s: 400.0,
                    module: Some("example:modul_ost".into()),
                }),
                ShuntMove::SetBack(ShuntTarget::Yard("Abstellgleis 1".into())),
                ShuntMove::Couple,
                ShuntMove::Uncouple(3),
                ShuntMove::Stand,
            ],
        };
        assert_eq!(ShuntJob::from_ron(&job.to_ron()).expect("parses"), job);
    }

    /// A module-local target is shifted by the composition's offset once, and forgets its
    /// module so a second pass cannot shift it again.
    #[test]
    fn a_module_local_target_shifts_once() {
        let mut target = ShuntTarget::At {
            edge: EdgeId(1),
            s: 0.0,
            module: Some("example:modul_ost".into()),
        };
        assert_eq!(target.module(), Some("example:modul_ost"));
        target.shift(4);
        assert_eq!(target.module(), None);
        // A second pass finds no module and leaves the index where the first one put it.
        target.shift(4);
        assert!(matches!(target, ShuntTarget::At { edge, .. } if edge == EdgeId(5)));
    }
}
