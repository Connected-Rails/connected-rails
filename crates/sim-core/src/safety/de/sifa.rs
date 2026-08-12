//! Sifa (driver's safety device) in its three builds (plan 9.2).
//!
//! Common to all of them: the driver has to operate the pedal periodically; failing to do
//! so leads via an indicator lamp and a horn to a forced braking, and the release requires
//! a change of the pedal. They differ in what ends the base period:
//!
//! * [`SifaKind::TimeTime`] — 30 s, regardless of the speed. The German standard.
//! * [`SifaKind::TimeDistance`] — 30 s **or** 1250 m, whichever comes first: the faster the
//!   train runs, the more often the pedal has to be operated.
//! * [`SifaKind::Rzm`] — reaction time measurement: like time-distance, but an operation
//!   only counts once a minimum time has passed since the last one. Beating the pedal
//!   continuously therefore does not satisfy the device.

use crate::cab::{CabInputs, Edge};
use crate::safety::{
    Indicator, LampState, ProtectionAction, ProtectionOutput, SafetyTrainState, TracksideEvent,
    TrainProtectionSystem,
};
use serde::{Deserialize, Serialize};

/// Build of the Sifa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SifaKind {
    /// Time-time: the base period is a fixed time.
    #[default]
    TimeTime,
    /// Time-distance: the base period ends with the time **or** the distance.
    TimeDistance,
    /// Reaction time measurement: time-distance plus a minimum interval between operations.
    Rzm,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SifaParams {
    /// Time until the indicator lamp [s].
    pub lamp_after: f64,
    /// Additional time until the horn [s].
    pub horn_after: f64,
    /// Additional time until forced braking [s].
    pub brake_after: f64,
    /// Below this speed the Sifa is inactive [km/h].
    pub inactive_below: f64,
    /// Distance until the indicator lamp [m] — time-distance and RZM only.
    pub lamp_distance: f64,
    /// An operation earlier than this after the previous one does not count [s] — RZM only.
    pub min_interval: f64,
}

impl Default for SifaParams {
    fn default() -> Self {
        Self {
            lamp_after: 30.0,
            horn_after: 2.5,
            brake_after: 2.5,
            inactive_below: 0.5,
            lamp_distance: 1250.0,
            min_interval: 5.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Sifa {
    pub kind: SifaKind,
    pub params: SifaParams,
    isolated: bool,
    /// Time since the last operation [s].
    timer: f64,
    /// Distance since the last operation [m], integrated from the speed — the Sifa has no
    /// odometer of its own.
    distance: f64,
    /// Forced braking triggered.
    braking: bool,
    /// After forced braking a change of the pedal is required.
    needs_release: bool,
    pedal: Edge,
}

impl Sifa {
    /// Time-time Sifa, the German standard build.
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_kind(kind: SifaKind) -> Self {
        Self {
            kind,
            ..Self::default()
        }
    }

    pub fn timer(&self) -> f64 {
        self.timer
    }

    /// Distance since the last operation [m].
    pub fn distance(&self) -> f64 {
        self.distance
    }

    /// How far the base period has run, 0 … 1. Time and distance count in parallel,
    /// whichever is further along wins.
    fn progress(&self) -> f64 {
        let by_time = self.timer / self.params.lamp_after;
        match self.kind {
            SifaKind::TimeTime => by_time,
            SifaKind::TimeDistance | SifaKind::Rzm => {
                by_time.max(self.distance / self.params.lamp_distance)
            }
        }
    }

    /// Seconds since the base period ended (negative while it still runs).
    fn overrun(&self) -> f64 {
        (self.progress() - 1.0) * self.params.lamp_after
    }

    pub fn lamp(&self) -> bool {
        self.overrun() >= 0.0
    }

    pub fn horn(&self) -> bool {
        self.overrun() >= self.params.horn_after
    }

    pub fn is_braking(&self) -> bool {
        self.braking
    }

    /// Does an operation count right now? The RZM ignores one that comes too early.
    fn operation_counts(&self) -> bool {
        match self.kind {
            SifaKind::TimeTime | SifaKind::TimeDistance => true,
            // Releasing the forced braking is always possible — the minimum interval
            // supervises the rhythm, not the release.
            SifaKind::Rzm => self.braking || self.timer >= self.params.min_interval,
        }
    }
}

impl TrainProtectionSystem for Sifa {
    fn update(
        &mut self,
        dt: f64,
        train: &SafetyTrainState,
        cab: &CabInputs,
        _events: &[TracksideEvent],
    ) -> ProtectionOutput {
        if self.isolated {
            self.timer = 0.0;
            self.distance = 0.0;
            self.braking = false;
            return ProtectionOutput::default();
        }

        let pressed = self.pedal.rising(cab.sifa);

        if train.v_kmh < self.params.inactive_below && !self.braking {
            // At standstill the Sifa does not run.
            self.timer = 0.0;
            self.distance = 0.0;
            self.needs_release = false;
            return ProtectionOutput::default();
        }

        if pressed && self.operation_counts() {
            if self.braking {
                // Release the forced braking: only by changing the pedal
                // (letting go was the precondition).
                self.braking = false;
            }
            self.timer = 0.0;
            self.distance = 0.0;
            self.needs_release = false;
        } else {
            self.timer += dt;
            self.distance += train.v_kmh / 3.6 * dt;
        }

        if self.overrun() >= self.params.horn_after + self.params.brake_after {
            self.braking = true;
        }

        ProtectionOutput {
            action: if self.braking {
                ProtectionAction::EmergencyBrake
            } else {
                ProtectionAction::None
            },
            alert: self.horn(),
            ..Default::default()
        }
    }

    fn indicators(&self) -> Vec<Indicator> {
        vec![Indicator::state(
            "sifa",
            if self.braking {
                LampState::Blinking
            } else if self.lamp() {
                LampState::On
            } else {
                LampState::Off
            },
        )]
    }

    fn isolate(&mut self, isolated: bool) {
        self.isolated = isolated;
    }

    fn is_isolated(&self) -> bool {
        self.isolated
    }

    fn name(&self) -> &'static str {
        match self.kind {
            SifaKind::TimeTime => "Sifa Zeit-Zeit",
            SifaKind::TimeDistance => "Sifa Zeit-Weg",
            SifaKind::Rzm => "Sifa RZM",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moving() -> SafetyTrainState {
        SafetyTrainState {
            v_kmh: 80.0,
            ..Default::default()
        }
    }

    fn run(sifa: &mut Sifa, seconds: f64, pedal: bool) -> ProtectionOutput {
        run_at(sifa, seconds, pedal, moving().v_kmh)
    }

    /// Runs `seconds` at `v_kmh`.
    fn run_at(sifa: &mut Sifa, seconds: f64, pedal: bool, v_kmh: f64) -> ProtectionOutput {
        let cab = CabInputs {
            sifa: pedal,
            ..Default::default()
        };
        let state = SafetyTrainState {
            v_kmh,
            ..Default::default()
        };
        let mut out = ProtectionOutput::default();
        let dt = 0.1;
        for _ in 0..(seconds / dt).round() as u32 {
            out = sifa.update(dt, &state, &cab, &[]);
        }
        out
    }

    #[test]
    fn lamp_horn_brake_sequence() {
        let mut sifa = Sifa::new();
        run(&mut sifa, 29.0, false);
        assert!(!sifa.lamp(), "no indicator lamp before 30 s");
        run(&mut sifa, 1.5, false);
        assert!(
            sifa.lamp() && !sifa.horn(),
            "30 s: indicator lamp, no horn yet"
        );
        run(&mut sifa, 2.5, false);
        assert!(sifa.horn(), "32.5 s: horn");
        assert!(!sifa.is_braking());
        let out = run(&mut sifa, 2.6, false);
        assert_eq!(out.action, ProtectionAction::EmergencyBrake);
    }

    #[test]
    fn pedal_resets_and_releases() {
        let mut sifa = Sifa::new();
        run(&mut sifa, 20.0, false);
        run(&mut sifa, 0.2, true); // operation
        assert!(sifa.timer() < 1.0);

        // Let it run until forced braking.
        run(&mut sifa, 40.0, false);
        assert!(sifa.is_braking());
        // Holding the pedal without changing it does not help — the rising edge counts.
        let out = run(&mut sifa, 1.0, true);
        assert_eq!(
            out.action,
            ProtectionAction::None,
            "changing the pedal releases"
        );
        let out = run(&mut sifa, 1.0, true);
        assert_eq!(out.action, ProtectionAction::None);
    }

    #[test]
    fn isolated_does_nothing() {
        let mut sifa = Sifa::new();
        sifa.isolate(true);
        let out = run(&mut sifa, 60.0, false);
        assert_eq!(out.action, ProtectionAction::None);
    }

    #[test]
    fn time_distance_demands_earlier_operation_at_speed() {
        // 200 km/h = 55.6 m/s → 1250 m are reached after 22.5 s, before the 30 s.
        let mut sifa = Sifa::with_kind(SifaKind::TimeDistance);
        run_at(&mut sifa, 20.0, false, 200.0);
        assert!(!sifa.lamp(), "1250 m not reached yet");
        run_at(&mut sifa, 3.0, false, 200.0);
        assert!(sifa.lamp(), "indicator lamp by distance, not by time");
        assert!(sifa.timer() < 30.0);
    }

    #[test]
    fn time_distance_falls_back_to_the_time_when_slow() {
        // 40 km/h: after 30 s only 333 m — the time decides.
        let mut sifa = Sifa::with_kind(SifaKind::TimeDistance);
        run_at(&mut sifa, 29.0, false, 40.0);
        assert!(!sifa.lamp());
        run_at(&mut sifa, 1.5, false, 40.0);
        assert!(sifa.lamp());
        assert!(sifa.distance() < 400.0);
    }

    #[test]
    fn rzm_ignores_an_operation_that_comes_too_early() {
        let mut sifa = Sifa::with_kind(SifaKind::Rzm);
        run_at(&mut sifa, 3.0, false, 80.0);
        run_at(&mut sifa, 0.2, true, 80.0); // too early — does not count
        assert!(
            sifa.timer() > 3.0,
            "an operation within the minimum interval does not reset the timer"
        );
        run_at(&mut sifa, 3.0, false, 80.0);
        run_at(&mut sifa, 0.2, true, 80.0); // now beyond 5 s
        assert!(sifa.timer() < 1.0, "operation counts");
    }

    #[test]
    fn rzm_forced_braking_is_always_releasable() {
        let mut sifa = Sifa::with_kind(SifaKind::Rzm);
        run_at(&mut sifa, 40.0, false, 80.0);
        assert!(sifa.is_braking());
        let out = run_at(&mut sifa, 0.3, true, 80.0);
        assert_eq!(out.action, ProtectionAction::None);
    }
}
