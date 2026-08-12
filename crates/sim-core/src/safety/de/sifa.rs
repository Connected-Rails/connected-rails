//! Sifa (Sicherheitsfahrschaltung), Zeit-Zeit-Ausführung (Plan 9.2).
//!
//! Ablauf ab letzter Bedienung: 30 s → Leuchtmelder, +2,5 s → Hupe,
//! +2,5 s → Zwangsbremsung. Lösen erst nach Pedalwechsel.

use crate::cab::{CabInputs, Edge};
use crate::safety::{
    Indicator, LampState, ProtectionAction, ProtectionOutput, SafetyTrainState, TracksideEvent,
    TrainProtectionSystem,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SifaParams {
    /// Zeit bis zum Leuchtmelder [s].
    pub lamp_after: f64,
    /// Zusatzzeit bis zur Hupe [s].
    pub horn_after: f64,
    /// Zusatzzeit bis zur Zwangsbremsung [s].
    pub brake_after: f64,
    /// Unterhalb dieser Geschwindigkeit ist die Sifa unwirksam [km/h].
    pub inactive_below: f64,
}

impl Default for SifaParams {
    fn default() -> Self {
        Self {
            lamp_after: 30.0,
            horn_after: 2.5,
            brake_after: 2.5,
            inactive_below: 0.5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Sifa {
    pub params: SifaParams,
    isolated: bool,
    /// Zeit seit der letzten Bedienung [s].
    timer: f64,
    /// Zwangsbremsung ausgelöst.
    braking: bool,
    /// Nach der Zwangsbremsung ist ein Pedalwechsel nötig.
    needs_release: bool,
    pedal: Edge,
}

impl Sifa {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn timer(&self) -> f64 {
        self.timer
    }

    pub fn lamp(&self) -> bool {
        self.timer >= self.params.lamp_after
    }

    pub fn horn(&self) -> bool {
        self.timer >= self.params.lamp_after + self.params.horn_after
    }

    pub fn is_braking(&self) -> bool {
        self.braking
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
            self.braking = false;
            return ProtectionOutput::default();
        }

        let pressed = self.pedal.rising(cab.sifa);

        if train.v_kmh < self.params.inactive_below && !self.braking {
            // Im Stillstand läuft die Sifa nicht mit.
            self.timer = 0.0;
            self.needs_release = false;
            return ProtectionOutput::default();
        }

        if pressed {
            if self.braking {
                // Zwangsbremsung lösen: nur mit Pedalwechsel (Loslassen war Voraussetzung).
                self.braking = false;
            }
            self.timer = 0.0;
            self.needs_release = false;
        } else {
            self.timer += dt;
        }

        if self.timer >= self.params.lamp_after + self.params.horn_after + self.params.brake_after {
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
        "Sifa"
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
        let cab = CabInputs {
            sifa: pedal,
            ..Default::default()
        };
        let mut out = ProtectionOutput::default();
        let dt = 0.1;
        for _ in 0..(seconds / dt).round() as u32 {
            out = sifa.update(dt, &moving(), &cab, &[]);
        }
        out
    }

    #[test]
    fn lamp_horn_brake_sequence() {
        let mut sifa = Sifa::new();
        run(&mut sifa, 29.0, false);
        assert!(!sifa.lamp(), "vor 30 s kein Leuchtmelder");
        run(&mut sifa, 1.5, false);
        assert!(
            sifa.lamp() && !sifa.horn(),
            "30 s: Leuchtmelder, noch keine Hupe"
        );
        run(&mut sifa, 2.5, false);
        assert!(sifa.horn(), "32,5 s: Hupe");
        assert!(!sifa.is_braking());
        let out = run(&mut sifa, 2.6, false);
        assert_eq!(out.action, ProtectionAction::EmergencyBrake);
    }

    #[test]
    fn pedal_resets_and_releases() {
        let mut sifa = Sifa::new();
        run(&mut sifa, 20.0, false);
        run(&mut sifa, 0.2, true); // Bedienung
        assert!(sifa.timer() < 1.0);

        // Bis zur Zwangsbremsung laufen lassen.
        run(&mut sifa, 40.0, false);
        assert!(sifa.is_braking());
        // Dauerdruck ohne Wechsel hilft nicht — es zählt die steigende Flanke.
        let out = run(&mut sifa, 1.0, true);
        assert_eq!(out.action, ProtectionAction::None, "Pedalwechsel löst");
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
}
