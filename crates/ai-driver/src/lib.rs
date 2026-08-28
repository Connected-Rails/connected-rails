//! AI train driver and timetable (plan ch. 11).
//!
//! The AI drives the same vehicle simulation as the player — no cheating: it only
//! sets [`CabInputs`], nothing else.

pub mod shunt;
pub mod timetable;

use serde::{Deserialize, Serialize};
pub use shunt::{ShuntJob, ShuntMove, ShuntPhase, ShuntState, ShuntTarget};
use sim_core::Sim;
use sim_core::brakes::DriverBrakeValve;
use sim_core::cab::CabInputs;
use sim_core::lookahead::{self, Lookahead};
pub use timetable::{ScheduledStop, Timetable, TimetableKind};

/// Driving behaviour of the AI driver.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DrivingStyle {
    /// Planned braking deceleration [m/s²].
    pub decel: f64,
    /// Safety distance ahead of target points [m].
    pub margin: f64,
    /// Target speed below the permitted one [km/h].
    pub speed_reserve: f64,
    /// Look-ahead range [m].
    pub lookahead: f64,
    /// Brake response time plus reaction time [s] — the distance the train still
    /// covers unbraked before the deceleration takes effect.
    pub reaction_time: f64,
}

impl Default for DrivingStyle {
    fn default() -> Self {
        Self {
            decel: 0.5,
            margin: 30.0,
            speed_reserve: 2.0,
            lookahead: 4_000.0,
            reaction_time: 5.0,
        }
    }
}

/// What the driver is currently doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DriverState {
    /// Running to the timetable.
    #[default]
    Driving,
    /// Stopped at the platform, waiting for the departure time.
    Dwelling,
    /// Standing in front of a signal at stop.
    WaitingAtSignal,
    /// Run finished (terminus reached).
    Finished,
    /// Working a shunt job — drawing up, setting back, coupling (plan ch. 11).
    Shunting,
}

/// An AI train driver.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiDriver {
    pub timetable: Timetable,
    pub style: DrivingStyle,
    pub state: DriverState,
    /// A shunt job, driven instead of the timetable when there are no stops, and after it
    /// once the last one has been worked (plan ch. 11). `None` = an ordinary driver.
    #[serde(default)]
    pub shunt: Option<ShuntState>,
    /// Index of the next stop in the timetable.
    pub next_stop: usize,
    /// Sifa pedal rhythm [s].
    sifa_timer: f64,
    sifa_pressed: bool,
    /// Remaining time of the PZB acknowledge operation [s].
    acknowledge_timer: f64,
    /// Time from which departure is allowed again [s].
    depart_at: f64,
}

impl AiDriver {
    pub fn new(timetable: Timetable) -> Self {
        Self {
            timetable,
            style: DrivingStyle::default(),
            state: DriverState::Driving,
            shunt: None,
            next_stop: 0,
            sifa_timer: 0.0,
            sifa_pressed: false,
            acknowledge_timer: 0.0,
            depart_at: 0.0,
        }
    }

    /// A driver who does nothing but shunt — a job instead of a timetable.
    pub fn shunting(job: ShuntJob) -> Self {
        let mut driver = Self::new(Timetable::default());
        driver.shunt = Some(ShuntState::new(job));
        driver
    }

    /// Hangs a shunt job on a driver who already has a timetable; it is worked once the
    /// last stop has been made.
    pub fn with_shunt(mut self, job: ShuntJob) -> Self {
        self.shunt = Some(ShuntState::new(job));
        self
    }

    /// One driver step: reads the simulation and writes the cab inputs.
    pub fn drive(&mut self, sim: &mut Sim, train: usize, dt: f64) {
        // A train that has been coupled away is nobody's to drive.
        let Some(head) = sim.trains[train].vehicles.first().map(|v| v.pos) else {
            return;
        };
        // Shunting takes over where the timetable has nothing left — or has nothing at
        // all, which is a driver who was only ever given a job.
        let shunting = self.timetable.stops.is_empty() || self.state == DriverState::Finished;
        if shunting && let Some(job) = &mut self.shunt {
            if job.active() {
                self.state = DriverState::Shunting;
                job.drive(sim, train);
                self.operate_sifa(sim, train, dt);
                return;
            }
            // The job is worked. A driver who was given one stands where it left him
            // rather than setting off down the line again.
            self.state = DriverState::Finished;
            shunt::hold(sim, train);
            self.operate_sifa(sim, train, dt);
            return;
        }
        let v_kmh = sim.trains[train].speed_kmh();
        let view = lookahead::scan(
            &sim.net,
            &sim.interlock,
            head,
            self.style.lookahead,
            sim.trains[train].movement,
        );

        let mut target = self.target_speed(sim, &view, head, v_kmh);

        // Train protection: never exceed the supervised speed.
        if let Some(limit) = sim.runtime[train].protection.speed_limit {
            target = target.min(limit - 1.0);
        }
        target = target.max(0.0);

        let cab = &mut sim.controls[train];
        cab.reverser = 1;
        Self::apply_speed_control(cab, v_kmh, target);

        self.operate_sifa(sim, train, dt);

        // Always confirm the LZB takeover.
        sim.controls[train].lzb_takeover = sim.runtime[train].protection.target_distance.is_some();
        self.update_state(sim, train, v_kmh, &view);
    }

    /// The pedal and the acknowledge button — the same rhythm whether the driver is
    /// running to a timetable or shunting.
    fn operate_sifa(&mut self, sim: &mut Sim, train: usize, dt: f64) {
        let v_kmh = sim.trains[train].speed_kmh();
        // Operate the Sifa every 20 s (short pedal press) — but at least every 800 m, so a
        // time-distance or RZM Sifa is served in time at speed as well.
        let interval = 20.0_f64.min(800.0 / (v_kmh.abs() / 3.6).max(1.0));
        self.sifa_timer += dt;
        if self.sifa_timer > interval {
            self.sifa_pressed = true;
            if self.sifa_timer > interval + 0.5 {
                self.sifa_pressed = false;
                self.sifa_timer = 0.0;
            }
        }
        sim.controls[train].sifa = self.sifa_pressed;

        // Acknowledge the PZB as soon as the train protection demands it.
        if sim.runtime[train].protection.alert {
            self.acknowledge_timer = 0.6;
        }
        if self.acknowledge_timer > 0.0 {
            self.acknowledge_timer -= dt;
            sim.controls[train].pzb_acknowledge = self.acknowledge_timer > 0.3;
        } else {
            sim.controls[train].pzb_acknowledge = false;
        }
    }

    /// Target speed from the line profile, signals and the timetable stop.
    fn target_speed(
        &self,
        sim: &Sim,
        view: &Lookahead,
        head: track_model::TrackPosition,
        v_kmh: f64,
    ) -> f64 {
        // Lead distance: fixed margin + the distance covered during reaction and
        // response time.
        let margin = self.style.margin + v_kmh / 3.6 * self.style.reaction_time;
        let mut target = view.permitted(self.style.decel, margin) - self.style.speed_reserve;

        // Timetable stop: braking curve onto the stopping point.
        if let Some(stop) = self.timetable.stops.get(self.next_stop)
            && let Some(d) = stop.distance_from(&sim.net, head, self.style.lookahead)
        {
            let curve = (2.0 * self.style.decel * (d - margin).max(0.0)).sqrt() * 3.6;
            target = target.min(curve);
        }
        if self.state == DriverState::Dwelling && sim.time < self.depart_at {
            target = 0.0;
        }
        target
    }

    /// Simple speed controller acting on throttle and driver's brake valve.
    ///
    /// `v_kmh` is the speed the target is compared against. Which way the train runs is
    /// the reverser's business, so a move that sets back hands in the magnitude and the
    /// controller works exactly as it does going forwards.
    pub(crate) fn apply_speed_control(cab: &mut CabInputs, v_kmh: f64, target: f64) {
        let err = target - v_kmh;
        if err > 2.0 {
            cab.throttle = (err / 15.0).clamp(0.1, 1.0);
            cab.brake_valve = DriverBrakeValve::Release;
        } else if err < -1.0 {
            cab.throttle = 0.0;
            // The larger the overspeed, the stronger the service braking.
            let drop = (-err / 20.0).clamp(0.4, 1.5);
            cab.brake_valve = DriverBrakeValve::Service(drop);
        } else {
            cab.throttle = 0.0;
            cab.brake_valve = if v_kmh < 0.5 {
                DriverBrakeValve::Service(0.8)
            } else {
                DriverBrakeValve::Lap
            };
        }
    }

    fn update_state(&mut self, sim: &Sim, train: usize, v_kmh: f64, view: &Lookahead) {
        match self.state {
            DriverState::Driving => {
                if v_kmh < 0.5 {
                    if let Some(stop) = self.timetable.stops.get(self.next_stop) {
                        let Some(head) = sim.trains[train].vehicles.first().map(|v| v.pos) else {
                            return;
                        };
                        if stop
                            .distance_from(&sim.net, head, 200.0)
                            .is_some_and(|d| d < 50.0)
                        {
                            self.state = DriverState::Dwelling;
                            self.depart_at = self
                                .timetable
                                .next_occurrence(sim.time, sim.start.seconds(), stop.departure)
                                .max(sim.time + 20.0);
                            return;
                        }
                    }
                    if view.distance_to_stop().is_some_and(|d| d < 100.0) {
                        self.state = DriverState::WaitingAtSignal;
                    }
                }
            }
            DriverState::Dwelling => {
                if sim.time >= self.depart_at {
                    self.next_stop += 1;
                    // A daily timetable starts over instead of finishing.
                    if self.next_stop >= self.timetable.stops.len()
                        && self.timetable.kind == TimetableKind::Daily
                    {
                        self.next_stop = 0;
                    }
                    self.state = if self.next_stop >= self.timetable.stops.len() {
                        DriverState::Finished
                    } else {
                        DriverState::Driving
                    };
                }
            }
            DriverState::WaitingAtSignal => {
                if view.distance_to_stop().is_none_or(|d| d > 150.0) {
                    self.state = DriverState::Driving;
                }
            }
            DriverState::Finished | DriverState::Shunting => {}
        }
    }
}
