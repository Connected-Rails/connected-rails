//! Scoring of a run (plan ch. 11): timetable adherence, stopping accuracy,
//! forbidden forced brake applications, overspeed, energy consumption.

use crate::Sim;
use crate::safety::ProtectionAction;
use crate::timetable::{Timetable, TimetableKind};
use serde::{Deserialize, Serialize};

/// Weighting of the scoring criteria.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ScoringRules {
    /// Initial number of points.
    pub base: i32,
    /// Deduction per metre of stopping point deviation beyond the tolerance.
    pub per_meter_off: f64,
    /// Tolerance of the stopping point [m].
    pub stop_tolerance: f64,
    /// Deduction per minute late (and half that rate for being early).
    pub per_minute_late: f64,
    /// Deduction per forced brake application.
    pub per_forced_brake: f64,
    /// Deduction per second of overspeed.
    pub per_overspeed_second: f64,
    /// Deduction per kilowatt hour of traction energy.
    pub per_kwh: f64,
}

impl Default for ScoringRules {
    fn default() -> Self {
        Self {
            base: 1000,
            per_meter_off: 2.0,
            stop_tolerance: 5.0,
            per_minute_late: 20.0,
            per_forced_brake: 100.0,
            per_overspeed_second: 5.0,
            per_kwh: 0.1,
        }
    }
}

/// Result of a scheduled stop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StopReport {
    pub name: String,
    /// Deviation from the stopping point [m], positive = overshot.
    pub position_error: f64,
    /// Deviation from the arrival time [s], positive = late.
    pub delay: f64,
}

/// Observes the run and collects the scoring quantities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreKeeper {
    /// The train being scored.
    pub train: usize,
    pub timetable: Timetable,
    pub rules: ScoringRules,
    pub next_stop: usize,
    pub stops: Vec<StopReport>,
    pub forced_brakes: u32,
    pub overspeed_seconds: f64,
    /// Highest measured overspeed [km/h].
    pub max_overspeed: f64,
    /// Traction energy taken at the wheel [kWh].
    pub energy_kwh: f64,
    pub distance_m: f64,
    prev_action: ProtectionAction,
    was_moving: bool,
}

impl Default for ScoreKeeper {
    fn default() -> Self {
        Self {
            train: 0,
            timetable: Timetable::default(),
            rules: ScoringRules::default(),
            next_stop: 0,
            stops: Vec::new(),
            forced_brakes: 0,
            overspeed_seconds: 0.0,
            max_overspeed: 0.0,
            energy_kwh: 0.0,
            distance_m: 0.0,
            prev_action: ProtectionAction::None,
            was_moving: false,
        }
    }
}

impl ScoreKeeper {
    pub fn new(train: usize, timetable: Timetable) -> Self {
        Self {
            train,
            timetable,
            ..Default::default()
        }
    }

    /// Evaluates one simulation step.
    pub fn update(&mut self, sim: &Sim, dt: f64) {
        let Some(train) = sim.trains.get(self.train) else {
            return;
        };
        let v_kmh = train.speed_kmh().abs();
        let v = train.speed().abs();
        self.distance_m += v * dt;

        // Traction energy at the wheel (traction only; regeneration of the dynamic brake
        // does not count in v1).
        let power: f64 = train
            .vehicles
            .iter()
            .map(|veh| veh.tractive_effort.max(0.0) * veh.v.abs())
            .sum();
        self.energy_kwh += power * dt / 3_600_000.0;

        // Forced brake applications: every new request counts once.
        let action = sim.runtime[self.train].protection.action;
        if action != ProtectionAction::None && self.prev_action == ProtectionAction::None {
            self.forced_brakes += 1;
        }
        self.prev_action = action;

        // Overspeed with respect to the permitted speed.
        let limit = train.vehicles[0].pos.speed_limit(&sim.net);
        if v_kmh > limit + 3.0 {
            self.overspeed_seconds += dt;
            self.max_overspeed = self.max_overspeed.max(v_kmh - limit);
        }

        // Detect a stop at the platform: transition running → standstill near a stopping point.
        let moving = v_kmh > 1.0;
        if self.was_moving && !moving {
            self.record_stop(sim);
        }
        self.was_moving = moving;
    }

    fn record_stop(&mut self, sim: &Sim) {
        let Some(stop) = self.timetable.stops.get(self.next_stop) else {
            return;
        };
        let head = sim.trains[self.train].vehicles[0].pos;
        if head.edge != stop.edge {
            return;
        }
        let error = (head.s - stop.s) * head.dir as f64;
        if error.abs() > 200.0 {
            return;
        }
        self.stops.push(StopReport {
            name: stop.name.clone(),
            position_error: error,
            delay: self.timetable.delay(sim.time, stop.arrival),
        });
        self.next_stop += 1;
        if self.timetable.kind == TimetableKind::Daily {
            self.next_stop %= self.timetable.stops.len();
        }
    }

    /// Score with a breakdown.
    pub fn report(&self, bonus: i32) -> ScoreReport {
        let r = &self.rules;
        let mut items = Vec::new();

        for stop in &self.stops {
            let off = (stop.position_error.abs() - r.stop_tolerance).max(0.0);
            if off > 0.0 {
                items.push(ScoreItem {
                    reason: i18n::t!(
                        "score-stop-missed",
                        stop = stop.name,
                        metres = format!("{off:.0}")
                    ),
                    points: -(off * r.per_meter_off) as i32,
                });
            }
            let minutes = stop.delay / 60.0;
            if minutes.abs() > 0.5 {
                let factor = if minutes > 0.0 { 1.0 } else { 0.5 };
                items.push(ScoreItem {
                    reason: i18n::t!(
                        "score-timetable",
                        stop = stop.name,
                        minutes = format!("{minutes:+.1}")
                    ),
                    points: -(minutes.abs() * r.per_minute_late * factor) as i32,
                });
            }
        }
        if self.forced_brakes > 0 {
            items.push(ScoreItem {
                reason: i18n::t!("score-forced-brakes", count = self.forced_brakes),
                points: -((self.forced_brakes as f64 * r.per_forced_brake) as i32),
            });
        }
        if self.overspeed_seconds > 0.0 {
            items.push(ScoreItem {
                reason: i18n::t!(
                    "score-overspeed",
                    seconds = format!("{:.0}", self.overspeed_seconds),
                    excess = format!("{:+.0}", self.max_overspeed)
                ),
                points: -((self.overspeed_seconds * r.per_overspeed_second) as i32),
            });
        }
        if self.energy_kwh > 0.0 {
            items.push(ScoreItem {
                reason: i18n::t!("score-energy", energy = format!("{:.0}", self.energy_kwh)),
                points: -((self.energy_kwh * r.per_kwh) as i32),
            });
        }
        if bonus != 0 {
            items.push(ScoreItem {
                reason: i18n::t!("score-scenario"),
                points: bonus,
            });
        }

        let total = r.base + items.iter().map(|i| i.points).sum::<i32>();
        ScoreReport {
            total: total.max(0),
            base: r.base,
            items,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreItem {
    pub reason: String,
    pub points: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreReport {
    pub total: i32,
    pub base: i32,
    pub items: Vec<ScoreItem>,
}

impl ScoreReport {
    /// Multi-line summary for HUD and log.
    pub fn summary(&self) -> String {
        let mut lines = vec![i18n::t!(
            "score-summary",
            total = self.total,
            base = self.base
        )];
        for item in &self.items {
            lines.push(format!("  {:+5}  {}", item.points, item.reason));
        }
        lines.join("\n")
    }
}
