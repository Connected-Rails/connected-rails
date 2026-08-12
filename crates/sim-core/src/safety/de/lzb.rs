//! LZB 80/CE — continuous train protection, on-board side (plan 9.4).
//!
//! The trackside (LZB centre in the interlocking) supplies movement authorities as
//! [`LzbTelegram`] over the loop cable sections. From these the vehicle derives v-Soll,
//! v-Ziel and the distance to target and supervises the braking curve.

use crate::cab::{CabInputs, Edge};
use crate::safety::{
    Indicator, LampState, ProtectionAction, ProtectionOutput, SafetyTrainState, TracksideEvent,
    TrainProtectionSystem,
};
use serde::{Deserialize, Serialize};
use track_model::DeviceKind;

/// Telegram of the LZB centre (payload of a loop cable section).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LzbTelegram {
    /// Permitted speed in the section [km/h].
    pub permitted_speed: f64,
    /// Target speed [km/h] (0 = stop).
    pub target_speed: f64,
    /// Distance to the target from the telegram location [m].
    pub target_distance: f64,
    /// This telegram announces the end of the LZB.
    #[serde(default)]
    pub end_of_authority: bool,
    /// Length of the loop cable section over which this telegram is transmitted [m].
    /// The loop cable transmits continuously — the simulation repeats the telegram
    /// as long as the train is inside the section.
    #[serde(default = "default_conductor_length")]
    pub length: f64,
}

fn default_conductor_length() -> f64 {
    1000.0
}

/// Deceleration of the LZB braking curve [m/s²].
///
/// ponytail: a fixed braking curve instead of a train-specific brake assessment. Enough
/// for LZB guidance and the end procedure; replace it once the braked weight percentage
/// per train is reported to the centre.
pub const LZB_DECELERATION: f64 = 0.6;
/// Without a telegram over this distance the LZB counts as failed [m].
pub const D_LOSS: f64 = 300.0;
/// Supervision speed in the failure/end procedure ("V40") [km/h].
pub const V_END: f64 = 40.0;

/// Operating state of the LZB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LzbMode {
    /// No loop cable — the PZB is in charge.
    #[default]
    Off,
    /// Pick-up running, takeover by the driver still pending ("Ü" blinking).
    Acceptance,
    /// LZB guidance active.
    Guiding,
    /// End of the LZB announced ("ENDE" blinking).
    Ending,
    /// LZB failed — failure procedure.
    Failure,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Lzb80 {
    pub mode: LzbMode,
    isolated: bool,
    /// Last received telegram.
    telegram: Option<LzbTelegram>,
    /// Odometer reading the telegram's distance to target refers to [m].
    telegram_odo: f64,
    /// Odometer reading of the last reception — for the failure detection [m].
    last_contact_odo: f64,
    /// Distance to target, current [m].
    target_distance: f64,
    /// Forced braking active.
    tripped: bool,
    takeover: Edge,
    end: Edge,
}

impl Lzb80 {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_guiding(&self) -> bool {
        matches!(self.mode, LzbMode::Guiding | LzbMode::Ending)
    }

    /// v-Soll [km/h] — the guidance's target speed (also the AFB input).
    pub fn permitted_speed(&self) -> Option<f64> {
        if !self.is_guiding() {
            return None;
        }
        let t = self.telegram?;
        // Braking curve to the target: v² = v_target² + 2·a·s
        let v_target = t.target_speed / 3.6;
        let curve = (v_target * v_target + 2.0 * LZB_DECELERATION * self.target_distance.max(0.0))
            .sqrt()
            * 3.6;
        Some(t.permitted_speed.min(curve))
    }

    /// v-Ziel [km/h].
    pub fn target_speed(&self) -> Option<f64> {
        self.telegram
            .map(|t| t.target_speed)
            .filter(|_| self.is_guiding())
    }

    /// Distance to target [m].
    pub fn target_distance(&self) -> Option<f64> {
        self.is_guiding().then_some(self.target_distance.max(0.0))
    }

    pub fn tripped(&self) -> bool {
        self.tripped
    }
}

impl TrainProtectionSystem for Lzb80 {
    fn update(
        &mut self,
        _dt: f64,
        train: &SafetyTrainState,
        cab: &CabInputs,
        events: &[TracksideEvent],
    ) -> ProtectionOutput {
        if self.isolated {
            self.mode = LzbMode::Off;
            return ProtectionOutput::default();
        }

        let takeover = self.takeover.rising(cab.lzb_takeover);
        let end = self.end.rising(cab.lzb_end);

        // Pick up telegrams from the loop cable.
        for e in events {
            if e.device != DeviceKind::LineConductor || !e.active {
                continue;
            }
            let Ok(t) = ron::from_str::<LzbTelegram>(&e.payload) else {
                continue;
            };
            // The loop cable transmits continuously. An unchanged telegram is only a sign
            // of life and must not reset the distance to target.
            self.last_contact_odo = train.odometer;
            if self.telegram == Some(t) {
                continue;
            }
            self.telegram = Some(t);
            self.telegram_odo = train.odometer - e.s_offset;
            self.mode = match self.mode {
                LzbMode::Off | LzbMode::Failure => LzbMode::Acceptance,
                LzbMode::Ending if !t.end_of_authority => LzbMode::Guiding,
                m => m,
            };
            if t.end_of_authority && self.is_guiding() {
                self.mode = LzbMode::Ending;
            }
        }

        // Reduce the distance to target with the distance travelled.
        if let Some(t) = self.telegram {
            self.target_distance = t.target_distance - (train.odometer - self.telegram_odo);
        }

        // Takeover by the driver.
        if self.mode == LzbMode::Acceptance && takeover {
            self.mode = LzbMode::Guiding;
        }

        // Loss of telegram → failure procedure.
        if matches!(self.mode, LzbMode::Guiding | LzbMode::Acceptance)
            && train.odometer - self.last_contact_odo > D_LOSS
        {
            self.mode = LzbMode::Failure;
        }

        // Acknowledge the end procedure → back to the PZB.
        if self.mode == LzbMode::Ending && end {
            self.mode = LzbMode::Off;
            self.telegram = None;
        }
        if self.mode == LzbMode::Failure && end {
            self.mode = LzbMode::Off;
            self.telegram = None;
        }

        // Supervision.
        let limit = match self.mode {
            LzbMode::Guiding | LzbMode::Ending => self.permitted_speed(),
            LzbMode::Failure => Some(V_END),
            _ => None,
        };
        if let Some(l) = limit {
            if train.v_kmh > l + 3.0 {
                self.tripped = true;
            } else if train.v_kmh <= l {
                self.tripped = false;
            }
        } else {
            self.tripped = false;
        }

        ProtectionOutput {
            action: if self.tripped {
                // The LZB brakes with a service application first, not an emergency one.
                ProtectionAction::ForcedServiceBrake
            } else {
                ProtectionAction::None
            },
            speed_limit: limit,
            target_speed: self.target_speed(),
            target_distance: self.target_distance(),
            alert: self.mode == LzbMode::Acceptance || self.mode == LzbMode::Ending,
        }
    }

    fn indicators(&self) -> Vec<Indicator> {
        let mut v = vec![
            Indicator::state(
                "lzb_ue",
                match self.mode {
                    LzbMode::Acceptance => LampState::Blinking,
                    LzbMode::Guiding | LzbMode::Ending => LampState::On,
                    _ => LampState::Off,
                },
            ),
            Indicator::state(
                "lzb_ende",
                match self.mode {
                    LzbMode::Ending => LampState::Blinking,
                    _ => LampState::Off,
                },
            ),
            Indicator::lamp("lzb_stoerung", self.mode == LzbMode::Failure),
            Indicator::lamp("lzb_b", self.tripped),
            Indicator::lamp("lzb_v40", self.mode == LzbMode::Failure),
        ];
        if let Some(s) = self.permitted_speed() {
            v.push(Indicator::value("mfa_v_soll", s));
        }
        if let Some(s) = self.target_speed() {
            v.push(Indicator::value("mfa_v_ziel", s));
        }
        if let Some(d) = self.target_distance() {
            v.push(Indicator::value("mfa_zielentfernung", d));
        }
        v
    }

    fn isolate(&mut self, isolated: bool) {
        self.isolated = isolated;
    }

    fn is_isolated(&self) -> bool {
        self.isolated
    }

    fn name(&self) -> &'static str {
        "LZB 80"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn telegram_event(t: LzbTelegram, s_offset: f64) -> TracksideEvent {
        TracksideEvent {
            device: DeviceKind::LineConductor,
            payload: ron::to_string(&t).unwrap(),
            s_offset,
            active: true,
        }
    }

    struct Rig {
        lzb: Lzb80,
        state: SafetyTrainState,
        cab: CabInputs,
        out: ProtectionOutput,
        /// What the loop cable is currently transmitting (None = no cable/failure).
        telegram: Option<LzbTelegram>,
    }

    impl Rig {
        fn new(v_kmh: f64) -> Self {
            Self {
                lzb: Lzb80::new(),
                state: SafetyTrainState {
                    v_kmh,
                    ..Default::default()
                },
                cab: CabInputs::default(),
                out: ProtectionOutput::default(),
                telegram: None,
            }
        }
        fn events(&self) -> Vec<TracksideEvent> {
            self.telegram
                .map(|t| vec![telegram_event(t, 0.0)])
                .unwrap_or_default()
        }
        fn send(&mut self, t: LzbTelegram) {
            self.telegram = Some(t);
            self.out = self.lzb.update(0.0, &self.state, &self.cab, &self.events());
        }
        fn drive(&mut self, meters: f64) {
            let dt = 0.1;
            let step = self.state.v_kmh / 3.6 * dt;
            let n = if step > 0.0 {
                (meters / step).round() as u32
            } else {
                (meters as u32).max(1)
            };
            for _ in 0..n {
                self.state.odometer += step;
                let ev = self.events();
                self.out = self.lzb.update(dt, &self.state, &self.cab, &ev);
            }
        }
        fn press(&mut self, set: impl Fn(&mut CabInputs)) {
            set(&mut self.cab);
            let ev = self.events();
            self.out = self.lzb.update(0.05, &self.state, &self.cab, &ev);
            self.cab = CabInputs::default();
            let ev = self.events();
            self.out = self.lzb.update(0.05, &self.state, &self.cab, &ev);
        }
    }

    #[test]
    fn acceptance_only_after_takeover() {
        let mut r = Rig::new(120.0);
        r.send(LzbTelegram {
            permitted_speed: 160.0,
            target_speed: 0.0,
            target_distance: 5000.0,
            end_of_authority: false,
            length: 1000.0,
        });
        assert_eq!(r.lzb.mode, LzbMode::Acceptance);
        assert!(!r.lzb.is_guiding());
        r.press(|c| c.lzb_takeover = true);
        assert_eq!(r.lzb.mode, LzbMode::Guiding);
        assert!(r.lzb.permitted_speed().is_some());
    }

    #[test]
    fn braking_curve_to_stop_lowers_permitted_speed() {
        let mut r = Rig::new(160.0);
        r.send(LzbTelegram {
            permitted_speed: 160.0,
            target_speed: 0.0,
            target_distance: 6000.0,
            end_of_authority: false,
            length: 1000.0,
        });
        r.press(|c| c.lzb_takeover = true);
        assert!(
            r.lzb.permitted_speed().unwrap() >= 160.0,
            "far away: full permitted speed"
        );
        r.drive(5000.0);
        let v = r.lzb.permitted_speed().unwrap();
        // 1000 m left until the stop: sqrt(2·0.6·1000) = 34.6 m/s = 124 km/h.
        assert!(v > 110.0 && v < 135.0, "permitted speed = {v}");
        assert!(r.lzb.target_distance().unwrap() < 1100.0);
        r.drive(900.0);
        assert!(r.lzb.permitted_speed().unwrap() < 45.0);
        assert_eq!(
            r.out.action,
            ProtectionAction::ForcedServiceBrake,
            "160 km/h is too fast"
        );
    }

    #[test]
    fn end_procedure_hands_over_to_pzb() {
        let mut r = Rig::new(100.0);
        r.send(LzbTelegram {
            permitted_speed: 160.0,
            target_speed: 100.0,
            target_distance: 2000.0,
            end_of_authority: false,
            length: 1000.0,
        });
        r.press(|c| c.lzb_takeover = true);
        r.send(LzbTelegram {
            permitted_speed: 100.0,
            target_speed: 100.0,
            target_distance: 1000.0,
            end_of_authority: true,
            length: 1000.0,
        });
        assert_eq!(r.lzb.mode, LzbMode::Ending);
        r.telegram = None; // loop cable ends
        r.press(|c| c.lzb_end = true);
        assert_eq!(r.lzb.mode, LzbMode::Off);
        assert!(!r.lzb.is_guiding(), "PZB takes over again");
    }

    #[test]
    fn telegram_loss_leads_to_failure_procedure() {
        let mut r = Rig::new(100.0);
        r.send(LzbTelegram {
            permitted_speed: 160.0,
            target_speed: 160.0,
            target_distance: 9000.0,
            end_of_authority: false,
            length: 1000.0,
        });
        r.press(|c| c.lzb_takeover = true);
        r.telegram = None; // loop cable ends without warning
        r.drive(400.0);
        assert_eq!(r.lzb.mode, LzbMode::Failure);
        assert_eq!(r.out.speed_limit, Some(V_END));
    }
}
