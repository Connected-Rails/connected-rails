//! Fahrzeug- und Zugverbandsmodell.

use crate::brakes::{BrakeSpec, BrakeState};
use crate::electric::{TractionSpec, TractionState};
use crate::safety::SafetySystems;
use serde::{Deserialize, Serialize};
use track_model::TrackPosition;

/// Fahrwiderstand nach Davis: `R = a + b·v + c·v²` [N], `v` in m/s.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Davis {
    pub a: f64,
    pub b: f64,
    pub c: f64,
}

impl Davis {
    pub fn resistance(&self, v: f64) -> f64 {
        let av = v.abs();
        self.a + self.b * av + self.c * av * av
    }
}

/// Kupplungsparameter. Schraubenkupplung: Zugfeder und Puffer getrennt, dazwischen Spiel.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CouplerSpec {
    /// Gesamtspiel zwischen Zug- und Druckanlage [m] (Schraubenkupplung ~ 0,06–0,10 m).
    pub slack: f64,
    /// Steifigkeit der Zugeinrichtung [N/m].
    pub draw_stiffness: f64,
    /// Steifigkeit der Puffer [N/m].
    pub buffer_stiffness: f64,
    /// Dämpfung [N·s/m].
    pub damping: f64,
    /// Bruchkraft [N] (Schraubenkupplung ~ 1 MN Mindestbruchlast).
    pub breaking_force: f64,
}

impl CouplerSpec {
    /// Übliche Schraubenkupplung (UIC 520) mit Seitenpuffern.
    pub fn screw() -> Self {
        Self {
            slack: 0.08,
            draw_stiffness: 3.0e6,
            buffer_stiffness: 8.0e6,
            damping: 1.2e5,
            breaking_force: 1.0e6,
        }
    }

    /// Mittelpufferkupplung (Triebzug): steifer, praktisch spielfrei.
    pub fn center_buffer() -> Self {
        Self {
            slack: 0.005,
            draw_stiffness: 2.0e7,
            buffer_stiffness: 2.0e7,
            damping: 4.0e5,
            breaking_force: 1.5e6,
        }
    }
}

/// Schienenzustand — beeinflusst den Kraftschlussbeiwert.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RailCondition {
    #[default]
    Dry,
    Wet,
    /// Laub, Reif, Rollrost — deutlich reduzierter Kraftschluss.
    Slippery,
}

impl RailCondition {
    /// Faktor auf den Kraftschlussbeiwert nach Curtius/Kniffler.
    pub fn factor(self) -> f64 {
        match self {
            RailCondition::Dry => 1.0,
            RailCondition::Wet => 0.6,
            RailCondition::Slippery => 0.35,
        }
    }
}

/// Statische Fahrzeugbeschreibung (aus der Fahrzeugdatenbank, RON).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VehicleSpec {
    pub name: String,
    /// Länge über Puffer [m].
    pub length: f64,
    /// Eigenmasse [kg].
    pub mass_empty: f64,
    /// Zuschlag für rotierende Massen (0,05 Wagen … 0,25 Triebfahrzeug).
    pub rotating_mass_factor: f64,
    pub davis: Davis,
    pub brake: BrakeSpec,
    #[serde(default)]
    pub traction: Option<TractionSpec>,
    pub coupler: CouplerSpec,
    /// Anteil der Fahrzeugmasse auf angetriebenen Achsen (Lok: 1,0; Wagen: 0,0).
    #[serde(default)]
    pub adhesive_mass_fraction: f64,
    /// Fahrzeug hat Schleuder-/Gleitschutz.
    #[serde(default)]
    pub slip_control: bool,
}

/// Laufzeitzustand eines Fahrzeugs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vehicle {
    pub spec: VehicleSpec,
    /// Zuladung [kg].
    pub load: f64,
    /// Zurückgelegter Weg entlang des Gleises [m], monoton in Fahrtrichtung.
    pub x: f64,
    /// Geschwindigkeit [m/s], positiv = Fahrtrichtung des Zuges.
    pub v: f64,
    /// Position des Fahrzeugmittelpunkts auf dem Gleisgraph.
    pub pos: TrackPosition,
    pub brake: BrakeState,
    pub traction: TractionState,
    /// Schlupfgeschwindigkeit der Treibachsen [m/s] (v1: pro Fahrzeug).
    /// ponytail: kein Modell je Radsatz — für Schleuder-/Gleitschutz und Sound reicht das;
    /// auf Radsätze aufteilen, sobald einzelne Achsen sichtbar/hörbar unterschieden werden.
    pub slip: f64,
    /// Sanden aktiv.
    pub sanding: bool,
    /// Tatsächlich auf die Schiene übertragene Zugkraft [N] (nach Kraftschlussgrenze).
    #[serde(default)]
    pub tractive_effort: f64,
    /// Tatsächlich wirkende Bremskraft [N] (nach Blending und Kraftschluss).
    #[serde(default)]
    pub brake_effort: f64,
    /// Zugsicherungsausrüstung dieses Fahrzeugs.
    #[serde(default)]
    pub safety: SafetySystems,
}

impl Vehicle {
    pub fn new(spec: VehicleSpec, pos: TrackPosition) -> Self {
        Self {
            brake: BrakeState::new(&spec.brake),
            traction: TractionState::default(),
            spec,
            load: 0.0,
            x: 0.0,
            v: 0.0,
            pos,
            slip: 0.0,
            sanding: false,
            tractive_effort: 0.0,
            brake_effort: 0.0,
            safety: SafetySystems::default(),
        }
    }

    /// Gesamtmasse [kg].
    pub fn mass(&self) -> f64 {
        self.spec.mass_empty + self.load
    }

    /// Wirksame Masse inkl. rotierender Massen [kg].
    pub fn inertial_mass(&self) -> f64 {
        self.mass() * (1.0 + self.spec.rotating_mass_factor)
    }

    /// Masse auf angetriebenen Achsen [kg].
    pub fn adhesive_mass(&self) -> f64 {
        self.mass() * self.spec.adhesive_mass_fraction
    }

    pub fn is_powered(&self) -> bool {
        self.spec.traction.is_some()
    }
}

/// Zustand einer Kupplung zwischen zwei benachbarten Fahrzeugen.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct CouplerState {
    /// Kraft [N], positiv = Zug, negativ = Druck (Puffer).
    pub force: f64,
    /// Auslenkung aus der Sollage [m].
    pub extension: f64,
    pub broken: bool,
}

/// Ein Zugverband.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Train {
    /// Fahrzeuge von der Spitze zum Schluss.
    pub vehicles: Vec<Vehicle>,
    /// Kupplungen: `couplers[i]` verbindet `vehicles[i]` und `vehicles[i+1]`.
    pub couplers: Vec<CouplerState>,
    /// Index des besetzten Führerstands.
    pub cab: usize,
    pub rail: RailCondition,
    /// Zugnummer für Fahrplan/Zugfunk.
    #[serde(default)]
    pub number: String,
}

impl Train {
    /// Baut einen Zug; die Fahrzeuge werden ab `head` nach hinten aufgereiht.
    pub fn assemble(
        mut vehicles: Vec<Vehicle>,
        head: TrackPosition,
        net: &track_model::TrackNetwork,
    ) -> Self {
        let mut x = 0.0;
        let mut scratch = Vec::new();
        for vehicle in &mut vehicles {
            // Fahrzeugmitte liegt eine halbe Fahrzeuglänge hinter der Kupplungsstelle.
            let half = vehicle.spec.length / 2.0;
            x -= half;
            let mut p = head;
            let _ = p.advance(net, x, &mut scratch);
            vehicle.pos = p;
            vehicle.x = x;
            x -= half;
        }
        let couplers = vec![CouplerState::default(); vehicles.len().saturating_sub(1)];
        Self {
            vehicles,
            couplers,
            cab: 0,
            rail: RailCondition::Dry,
            number: String::new(),
        }
    }

    /// Gesamtmasse [kg].
    pub fn mass(&self) -> f64 {
        self.vehicles.iter().map(Vehicle::mass).sum()
    }

    /// Zuglänge [m].
    pub fn length(&self) -> f64 {
        self.vehicles.iter().map(|v| v.spec.length).sum()
    }

    /// Geschwindigkeit des Zuges [m/s] (Mittel über alle Fahrzeuge).
    pub fn speed(&self) -> f64 {
        if self.vehicles.is_empty() {
            return 0.0;
        }
        self.vehicles.iter().map(|v| v.v).sum::<f64>() / self.vehicles.len() as f64
    }

    pub fn speed_kmh(&self) -> f64 {
        self.speed() * 3.6
    }

    /// Position der Zugspitze.
    pub fn head_position(&self) -> TrackPosition {
        let front = &self.vehicles[0];
        front.pos.offset_by_unchecked(front.spec.length / 2.0)
    }

    /// Bremshundertstel des Zuges: Summe Bremsgewichte / Summe Massen · 100.
    pub fn brake_percentage(&self) -> f64 {
        let weight: f64 = self
            .vehicles
            .iter()
            .map(|v| v.spec.brake.brake_weight)
            .sum();
        let mass: f64 = self.vehicles.iter().map(|v| v.mass() / 1000.0).sum();
        if mass <= 0.0 {
            0.0
        } else {
            weight / mass * 100.0
        }
    }
}

/// Hilfs-Trait: Position um einen Betrag verschieben ohne Netzzugriff (nur `s`),
/// für Zwecke, bei denen Kantenwechsel egal sind (z. B. Anzeige).
trait OffsetUnchecked {
    fn offset_by_unchecked(&self, d: f64) -> Self;
}

impl OffsetUnchecked for TrackPosition {
    fn offset_by_unchecked(&self, d: f64) -> Self {
        let mut p = *self;
        p.s += d * p.dir as f64;
        p
    }
}
