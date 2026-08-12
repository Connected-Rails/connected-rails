//! Zugsicherung: länderneutrale Abstraktion + Länderpakete (Plan Kap. 9).
//!
//! Jedes Zugsicherungssystem ist eine Zustandsmaschine mit definierten Ein-/Ausgängen.
//! Die Fahrzeugseite kennt nur [`TrainProtectionSystem`]; welche Systeme ein Fahrzeug
//! trägt, steht in der Fahrzeugdatenbank.

pub mod de;

use crate::cab::CabInputs;
use serde::{Deserialize, Serialize};
use track_model::DeviceKind;

/// Was die Zugsicherung dem Fahrzeug befiehlt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ProtectionAction {
    #[default]
    None,
    /// Zwangsbremsung als Betriebsbremsung (z. B. LZB-Betriebsbremsung).
    ForcedServiceBrake,
    /// Zwangsbremsung als Schnellbremsung (PZB, Sifa).
    EmergencyBrake,
    /// Nur Traktionsabschaltung.
    TractionCutOff,
}

impl ProtectionAction {
    /// Die schärfere von zwei Anforderungen.
    pub fn max(self, other: Self) -> Self {
        use ProtectionAction::*;
        let rank = |a: Self| match a {
            None => 0,
            TractionCutOff => 1,
            ForcedServiceBrake => 2,
            EmergencyBrake => 3,
        };
        if rank(other) > rank(self) {
            other
        } else {
            self
        }
    }
}

/// Zustand eines Leuchtmelders/einer Anzeige im Führerstand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LampState {
    #[default]
    Off,
    On,
    Blinking,
}

/// Eine Anzeige der Zugsicherung (Leuchtmelder oder Zahlenwert für MFA/EBuLa).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Indicator {
    pub name: &'static str,
    pub lamp: LampState,
    /// Zahlenwert für Anzeigeinstrumente (v-Soll, v-Ziel, Zielentfernung).
    pub value: Option<f64>,
}

impl Indicator {
    pub fn lamp(name: &'static str, on: bool) -> Self {
        Self {
            name,
            lamp: if on { LampState::On } else { LampState::Off },
            value: None,
        }
    }

    pub fn state(name: &'static str, lamp: LampState) -> Self {
        Self {
            name,
            lamp,
            value: None,
        }
    }

    pub fn value(name: &'static str, value: f64) -> Self {
        Self {
            name,
            lamp: LampState::Off,
            value: Some(value),
        }
    }
}

/// Ausgabe eines Zugsicherungssystems nach einem Schritt.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProtectionOutput {
    pub action: ProtectionAction,
    /// Überwachungsgeschwindigkeit [km/h], falls das System eine vorgibt.
    pub speed_limit: Option<f64>,
    /// Zielgeschwindigkeit [km/h] (LZB/ETCS).
    pub target_speed: Option<f64>,
    /// Zielentfernung [m] (LZB/ETCS).
    pub target_distance: Option<f64>,
    /// System verlangt eine Bedienung (für Sound: Hupe/Zwangsbremsung).
    pub alert: bool,
}

impl ProtectionOutput {
    /// Zwei Ausgaben zusammenfassen (mehrere Systeme an einem Fahrzeug).
    pub fn merge(self, other: Self) -> Self {
        Self {
            action: self.action.max(other.action),
            speed_limit: min_option(self.speed_limit, other.speed_limit),
            target_speed: min_option(self.target_speed, other.target_speed),
            target_distance: min_option(self.target_distance, other.target_distance),
            alert: self.alert || other.alert,
        }
    }
}

fn min_option(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, None) => x,
        (None, y) => y,
    }
}

/// Ein von einem antennentragenden Fahrzeug überfahrenes Streckengerät.
#[derive(Debug, Clone, PartialEq)]
pub struct TracksideEvent {
    pub device: DeviceKind,
    /// Nutzdaten als RON-Text (siehe `TracksideDevice::payload`).
    pub payload: String,
    /// Wie weit hinter der Fahrzeugantenne das Gerät inzwischen liegt [m].
    pub s_offset: f64,
    /// Wirksamkeit — bei signalabhängigen Magneten entscheidet das Stellwerk
    /// (1000 Hz nur bei Vr0/Vr2, 2000 Hz nur bei Hp0).
    pub active: bool,
}

/// Fahrzeugzustand, soweit die Zugsicherung ihn braucht.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SafetyTrainState {
    /// Geschwindigkeit [km/h], Betrag.
    pub v_kmh: f64,
    /// Monoton wachsender Fahrweg [m] — Bezug für alle Wegüberwachungen.
    pub odometer: f64,
    /// Zulässige Geschwindigkeit an der aktuellen Stelle [km/h].
    pub line_speed: f64,
    /// Bremsung aktiv (für die Freigabelogik).
    pub braking: bool,
}

impl SafetyTrainState {
    pub fn standstill(&self) -> bool {
        self.v_kmh < 0.5
    }
}

/// Länderneutrale Schnittstelle jedes Zugsicherungssystems.
pub trait TrainProtectionSystem {
    fn update(
        &mut self,
        dt: f64,
        train: &SafetyTrainState,
        cab: &CabInputs,
        events: &[TracksideEvent],
    ) -> ProtectionOutput;

    /// Leuchtmelder/Anzeigen für den Führerstand.
    fn indicators(&self) -> Vec<Indicator>;

    /// Störschalter.
    fn isolate(&mut self, isolated: bool);

    fn is_isolated(&self) -> bool;

    /// Kurzname für Debug-Overlays.
    fn name(&self) -> &'static str;
}

/// Zugsicherungsausrüstung eines Fahrzeugs.
///
/// Länderpakete sind Compile-Zeit-Rust (Plan 9.1); deshalb ein Enum statt `Vec<Box<dyn …>>` —
/// so bleiben Klonen und Serialisieren (Save/Load, Replays) ohne Zusatzcode möglich.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub enum SafetySystems {
    /// Fahrzeug ohne Zugsicherung (Wagen).
    #[default]
    None,
    /// Deutsches Paket: Sifa, PZB 90, LZB 80.
    De(de::DeSafety),
}

impl SafetySystems {
    pub fn update(
        &mut self,
        dt: f64,
        train: &SafetyTrainState,
        cab: &CabInputs,
        events: &[TracksideEvent],
    ) -> ProtectionOutput {
        match self {
            SafetySystems::None => ProtectionOutput::default(),
            SafetySystems::De(de) => de.update(dt, train, cab, events),
        }
    }

    pub fn indicators(&self) -> Vec<Indicator> {
        match self {
            SafetySystems::None => Vec::new(),
            SafetySystems::De(de) => de.indicators(),
        }
    }
}
