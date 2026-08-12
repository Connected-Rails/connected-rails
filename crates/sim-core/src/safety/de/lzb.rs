//! LZB 80/CE — Linienzugbeeinflussung, Fahrzeugseite (Plan 9.4).
//!
//! Die Streckenseite (LZB-Zentrale im Stellwerk) liefert über die Linienleiterabschnitte
//! Fahrterlaubnisse als [`LzbTelegram`]. Das Fahrzeug führt daraus v-Soll, v-Ziel und
//! Zielentfernung und überwacht die Bremskurve.

use crate::cab::{CabInputs, Edge};
use crate::safety::{
    Indicator, LampState, ProtectionAction, ProtectionOutput, SafetyTrainState, TracksideEvent,
    TrainProtectionSystem,
};
use serde::{Deserialize, Serialize};
use track_model::DeviceKind;

/// Telegramm der LZB-Zentrale (Payload eines Linienleiterabschnitts).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LzbTelegram {
    /// Zulässige Geschwindigkeit im Abschnitt [km/h].
    pub permitted_speed: f64,
    /// Zielgeschwindigkeit [km/h] (0 = Halt).
    pub target_speed: f64,
    /// Entfernung zum Ziel ab dem Telegrammort [m].
    pub target_distance: f64,
    /// Dieses Telegramm kündigt das LZB-Ende an.
    #[serde(default)]
    pub end_of_authority: bool,
    /// Länge des Linienleiterabschnitts, über die dieses Telegramm gesendet wird [m].
    /// Der Linienleiter sendet fortlaufend — die Simulation wiederholt das Telegramm,
    /// solange der Zug im Abschnitt ist.
    #[serde(default = "default_conductor_length")]
    pub length: f64,
}

fn default_conductor_length() -> f64 {
    1000.0
}

/// Verzögerung der LZB-Bremskurve [m/s²].
///
/// ponytail: eine feste Bremskurve statt zugspezifischer Bremsbewertung. Reicht für
/// LZB-Führung und das Ende-Verfahren; ersetzen, sobald Bremshundertstel je Zug in die
/// Zentrale gemeldet werden.
pub const LZB_DECELERATION: f64 = 0.6;
/// Ohne Telegramm über diesen Weg gilt die LZB als ausgefallen [m].
pub const D_LOSS: f64 = 300.0;
/// Überwachungsgeschwindigkeit im Ausfall-/Ende-Verfahren („V40") [km/h].
pub const V_END: f64 = 40.0;

/// Betriebszustand der LZB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LzbMode {
    /// Kein Linienleiter — PZB führt.
    #[default]
    Off,
    /// Aufnahme läuft, Übernahme durch den Tf noch offen („Ü" blinkt).
    Acceptance,
    /// LZB-Führung aktiv.
    Guiding,
    /// LZB-Ende angekündigt („ENDE" blinkt).
    Ending,
    /// LZB ausgefallen — Ausfallverfahren.
    Failure,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Lzb80 {
    pub mode: LzbMode,
    isolated: bool,
    /// Letztes empfangenes Telegramm.
    telegram: Option<LzbTelegram>,
    /// Odometerstand, auf den sich die Zielentfernung des Telegramms bezieht [m].
    telegram_odo: f64,
    /// Odometerstand des letzten Empfangs — für die Ausfallerkennung [m].
    last_contact_odo: f64,
    /// Zielentfernung, aktuell [m].
    target_distance: f64,
    /// Zwangsbremsung aktiv.
    tripped: bool,
    uebernahme: Edge,
    ende: Edge,
}

impl Lzb80 {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_guiding(&self) -> bool {
        matches!(self.mode, LzbMode::Guiding | LzbMode::Ending)
    }

    /// v-Soll [km/h] — Sollgeschwindigkeit der Führung (auch AFB-Eingang).
    pub fn permitted_speed(&self) -> Option<f64> {
        if !self.is_guiding() {
            return None;
        }
        let t = self.telegram?;
        // Bremskurve zum Ziel: v² = v_ziel² + 2·a·s
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

    /// Zielentfernung [m].
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

        let uebernahme = self.uebernahme.rising(cab.lzb_uebernahme);
        let ende = self.ende.rising(cab.lzb_ende);

        // Telegramme aus dem Linienleiter aufnehmen.
        for e in events {
            if e.device != DeviceKind::LineConductor || !e.active {
                continue;
            }
            let Ok(t) = ron::from_str::<LzbTelegram>(&e.payload) else {
                continue;
            };
            // Der Linienleiter sendet fortlaufend. Ein unverändertes Telegramm ist nur
            // ein Lebenszeichen und darf die Zielentfernung nicht zurücksetzen.
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

        // Zielentfernung mit dem Weg abbauen.
        if let Some(t) = self.telegram {
            self.target_distance = t.target_distance - (train.odometer - self.telegram_odo);
        }

        // Übernahme durch den Triebfahrzeugführer.
        if self.mode == LzbMode::Acceptance && uebernahme {
            self.mode = LzbMode::Guiding;
        }

        // Telegrammverlust → Ausfallverfahren.
        if matches!(self.mode, LzbMode::Guiding | LzbMode::Acceptance)
            && train.odometer - self.last_contact_odo > D_LOSS
        {
            self.mode = LzbMode::Failure;
        }

        // Ende-Verfahren quittieren → zurück an die PZB.
        if self.mode == LzbMode::Ending && ende {
            self.mode = LzbMode::Off;
            self.telegram = None;
        }
        if self.mode == LzbMode::Failure && ende {
            self.mode = LzbMode::Off;
            self.telegram = None;
        }

        // Überwachung.
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
                // LZB bremst zuerst betrieblich, nicht mit Schnellbremsung.
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
        /// Was der Linienleiter gerade sendet (None = kein Leiter/Ausfall).
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
    fn aufnahme_erst_nach_uebernahme() {
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
        r.press(|c| c.lzb_uebernahme = true);
        assert_eq!(r.lzb.mode, LzbMode::Guiding);
        assert!(r.lzb.permitted_speed().is_some());
    }

    #[test]
    fn bremskurve_zum_halt_senkt_v_soll() {
        let mut r = Rig::new(160.0);
        r.send(LzbTelegram {
            permitted_speed: 160.0,
            target_speed: 0.0,
            target_distance: 6000.0,
            end_of_authority: false,
            length: 1000.0,
        });
        r.press(|c| c.lzb_uebernahme = true);
        assert!(
            r.lzb.permitted_speed().unwrap() >= 160.0,
            "weit weg: volle v-Soll"
        );
        r.drive(5000.0);
        let v = r.lzb.permitted_speed().unwrap();
        // Rest 1000 m bis Halt: sqrt(2·0,6·1000) = 34,6 m/s = 124 km/h.
        assert!(v > 110.0 && v < 135.0, "v-Soll = {v}");
        assert!(r.lzb.target_distance().unwrap() < 1100.0);
        r.drive(900.0);
        assert!(r.lzb.permitted_speed().unwrap() < 45.0);
        assert_eq!(
            r.out.action,
            ProtectionAction::ForcedServiceBrake,
            "160 km/h ist zu schnell"
        );
    }

    #[test]
    fn ende_verfahren_uebergibt_an_pzb() {
        let mut r = Rig::new(100.0);
        r.send(LzbTelegram {
            permitted_speed: 160.0,
            target_speed: 100.0,
            target_distance: 2000.0,
            end_of_authority: false,
            length: 1000.0,
        });
        r.press(|c| c.lzb_uebernahme = true);
        r.send(LzbTelegram {
            permitted_speed: 100.0,
            target_speed: 100.0,
            target_distance: 1000.0,
            end_of_authority: true,
            length: 1000.0,
        });
        assert_eq!(r.lzb.mode, LzbMode::Ending);
        r.telegram = None; // Linienleiter endet
        r.press(|c| c.lzb_ende = true);
        assert_eq!(r.lzb.mode, LzbMode::Off);
        assert!(!r.lzb.is_guiding(), "PZB übernimmt wieder");
    }

    #[test]
    fn telegrammverlust_fuehrt_ins_ausfallverfahren() {
        let mut r = Rig::new(100.0);
        r.send(LzbTelegram {
            permitted_speed: 160.0,
            target_speed: 160.0,
            target_distance: 9000.0,
            end_of_authority: false,
            length: 1000.0,
        });
        r.press(|c| c.lzb_uebernahme = true);
        r.telegram = None; // Linienleiter endet unangekündigt
        r.drive(400.0);
        assert_eq!(r.lzb.mode, LzbMode::Failure);
        assert_eq!(r.out.speed_limit, Some(V_END));
    }
}
