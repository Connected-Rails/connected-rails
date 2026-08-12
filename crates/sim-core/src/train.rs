//! Vehicle and train consist model.

use crate::brakes::{BrakeSpec, BrakeState};
use crate::electric::{TractionSpec, TractionState};
use crate::safety::SafetySystems;
use serde::{Deserialize, Serialize};
use track_model::TrackPosition;

/// Running resistance after Davis: `R = a + b·v + c·v²` [N], `v` in m/s.
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

/// Coupler parameters. Screw coupler: draw gear and buffers separate, slack in between.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CouplerSpec {
    /// Total slack between draw gear and buffing gear [m] (screw coupler ~ 0.06–0.10 m).
    pub slack: f64,
    /// Stiffness of the draw gear [N/m].
    pub draw_stiffness: f64,
    /// Stiffness of the buffers [N/m].
    pub buffer_stiffness: f64,
    /// Damping [N·s/m].
    pub damping: f64,
    /// Breaking force [N] (screw coupler ~ 1 MN minimum breaking load).
    pub breaking_force: f64,
}

impl CouplerSpec {
    /// Common screw coupler (UIC 520) with side buffers.
    pub fn screw() -> Self {
        Self {
            slack: 0.08,
            draw_stiffness: 3.0e6,
            buffer_stiffness: 8.0e6,
            damping: 1.2e5,
            breaking_force: 1.0e6,
        }
    }

    /// Centre buffer coupler (multiple unit): stiffer, practically free of slack.
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

/// Rail condition — influences the adhesion coefficient.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RailCondition {
    #[default]
    Dry,
    Wet,
    /// Leaves, frost, surface rust — considerably reduced adhesion.
    Slippery,
}

impl RailCondition {
    /// Factor applied to the adhesion coefficient after Curtius/Kniffler.
    pub fn factor(self) -> f64 {
        match self {
            RailCondition::Dry => 1.0,
            RailCondition::Wet => 0.6,
            RailCondition::Slippery => 0.35,
        }
    }
}

/// Static vehicle description (from the vehicle database, RON).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VehicleSpec {
    pub name: String,
    /// Length over buffers [m].
    pub length: f64,
    /// Tare mass [kg].
    pub mass_empty: f64,
    /// Allowance for rotating masses (0.05 coach … 0.25 powered vehicle).
    pub rotating_mass_factor: f64,
    pub davis: Davis,
    pub brake: BrakeSpec,
    #[serde(default)]
    pub traction: Option<TractionSpec>,
    pub coupler: CouplerSpec,
    /// Share of the vehicle mass on driven axles (loco: 1.0; coach: 0.0).
    #[serde(default)]
    pub adhesive_mass_fraction: f64,
    /// Vehicle has wheel slip / wheel slide protection.
    #[serde(default)]
    pub slip_control: bool,
}

/// Runtime state of a vehicle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vehicle {
    pub spec: VehicleSpec,
    /// Payload [kg].
    pub load: f64,
    /// Distance travelled along the track [m], monotonic in the direction of travel.
    pub x: f64,
    /// Speed [m/s], positive = direction of travel of the train.
    pub v: f64,
    /// Position of the vehicle centre on the track graph.
    pub pos: TrackPosition,
    pub brake: BrakeState,
    pub traction: TractionState,
    /// Slip speed of the driven axles [m/s] (v1: per vehicle).
    /// ponytail: no model per wheelset — enough for wheel slip/slide protection and sound;
    /// split it up per wheelset as soon as individual axles are distinguished
    /// visually/audibly.
    pub slip: f64,
    /// Sanding active.
    pub sanding: bool,
    /// Tractive effort actually transmitted to the rail [N] (after the adhesion limit).
    #[serde(default)]
    pub tractive_effort: f64,
    /// Brake force actually acting [N] (after blending and adhesion).
    #[serde(default)]
    pub brake_effort: f64,
    /// Train protection equipment of this vehicle.
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

    /// Total mass [kg].
    pub fn mass(&self) -> f64 {
        self.spec.mass_empty + self.load
    }

    /// Effective mass including rotating masses [kg].
    pub fn inertial_mass(&self) -> f64 {
        self.mass() * (1.0 + self.spec.rotating_mass_factor)
    }

    /// Mass on driven axles [kg].
    pub fn adhesive_mass(&self) -> f64 {
        self.mass() * self.spec.adhesive_mass_fraction
    }

    pub fn is_powered(&self) -> bool {
        self.spec.traction.is_some()
    }
}

/// State of a coupler between two neighbouring vehicles.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct CouplerState {
    /// Force [N], positive = draw, negative = buff (buffers).
    pub force: f64,
    /// Deflection from the nominal position [m].
    pub extension: f64,
    pub broken: bool,
}

/// A train consist.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Train {
    /// Vehicles from the head to the rear.
    pub vehicles: Vec<Vehicle>,
    /// Couplers: `couplers[i]` connects `vehicles[i]` and `vehicles[i+1]`.
    pub couplers: Vec<CouplerState>,
    /// Index of the occupied cab.
    pub cab: usize,
    pub rail: RailCondition,
    /// Train number for timetable/train radio.
    #[serde(default)]
    pub number: String,
}

impl Train {
    /// Assembles a train; the vehicles are lined up backwards starting at `head`.
    pub fn assemble(
        mut vehicles: Vec<Vehicle>,
        head: TrackPosition,
        net: &track_model::TrackNetwork,
    ) -> Self {
        let mut x = 0.0;
        let mut scratch = Vec::new();
        for vehicle in &mut vehicles {
            // The vehicle centre lies half a vehicle length behind the coupling point.
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

    /// Total mass [kg].
    pub fn mass(&self) -> f64 {
        self.vehicles.iter().map(Vehicle::mass).sum()
    }

    /// Train length [m].
    pub fn length(&self) -> f64 {
        self.vehicles.iter().map(|v| v.spec.length).sum()
    }

    /// Speed of the train [m/s] (mean over all vehicles).
    pub fn speed(&self) -> f64 {
        if self.vehicles.is_empty() {
            return 0.0;
        }
        self.vehicles.iter().map(|v| v.v).sum::<f64>() / self.vehicles.len() as f64
    }

    pub fn speed_kmh(&self) -> f64 {
        self.speed() * 3.6
    }

    /// Position of the head of the train.
    pub fn head_position(&self) -> TrackPosition {
        let front = &self.vehicles[0];
        front.pos.offset_by_unchecked(front.spec.length / 2.0)
    }

    /// Braked weight percentage of the train: sum of braked weights / sum of masses · 100.
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

/// Helper trait: shift a position by an amount without network access (only `s`),
/// for purposes where edge changes do not matter (e.g. display).
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
