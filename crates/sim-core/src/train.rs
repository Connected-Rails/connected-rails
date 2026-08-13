//! Vehicle and train consist model.

use crate::brakes::{BrakeKind, BrakeSpec, BrakeState, SlipProtection};
use crate::doors::{DoorControl, DoorSystem, VehicleDoors};
use crate::drive::TractionSpec;
use crate::electric::TractionState;
use crate::safety::{SafetyEquipment, SafetySystems};
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

/// Density of air [kg/m³] at 15 °C and 1013 hPa — for the cw·A air resistance.
pub const AIR_DENSITY: f64 = 1.225;

/// Standard gauge [m].
pub const STANDARD_GAUGE: f64 = 1.435;

/// Axle base sum the Röckl curve resistance is calibrated for [m] — a bogie vehicle with
/// 2.5 m per bogie.
pub const REFERENCE_AXLE_BASE: f64 = 5.0;

fn standard_gauge() -> f64 {
    STANDARD_GAUGE
}

fn one() -> f64 {
    1.0
}

/// Motion of a model node between function value 0 and 1.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum Motion {
    /// The node is only shown or hidden.
    #[default]
    Visibility,
    /// Rotation about a local axis [°] — doors, pantographs, instrument needles.
    Rotate { axis: [f32; 3], degrees: f32 },
    /// Translation along a local axis [m] — sliding doors, switches, levers.
    Translate { axis: [f32; 3], metres: f32 },
}

/// A moving part of the model, bound to a glTF node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Part {
    /// Name of the glTF node.
    pub node: String,
    /// What the node represents — free-form, like the lamp images of a signal:
    /// `"door_left"`, `"pantograph"`, `"switch:throttle"`, `"gauge:speed"`.
    /// The app maps the names it knows; mods may invent their own.
    pub function: String,
    #[serde(default)]
    pub motion: Motion,
}

/// One level of detail. The convention is glTF nodes named `<name>_LOD<level>`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Lod {
    pub level: u8,
    /// Visible up to this distance [m].
    pub distance: f64,
}

/// Visual description of a vehicle: glTF file, levels of detail, moving parts.
///
/// Pure data — `sim-core` never renders. It sits next to the physical data so that a
/// vehicle stays a single file (plan ch. 15).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct VehicleModel {
    /// glTF/GLB file, relative to the `mods/` directory — e.g.
    /// `example/assets/br101.gltf`, loaded as `mods://example/assets/br101.gltf`.
    pub file: String,
    /// Levels of detail, coarsest last. Empty = the whole scene at every distance.
    #[serde(default)]
    pub lods: Vec<Lod>,
    /// Moving parts.
    #[serde(default)]
    pub parts: Vec<Part>,
}

/// Static vehicle description (from the vehicle database, RON).
///
/// `PartialEq` compares the fields bit for bit, floats included. That is what
/// the editor wants: it asks "has the user changed anything since the last
/// frame", not "are these two vehicles physically alike".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VehicleSpec {
    pub name: String,
    /// Length over buffers (LÜP) [m] — determines the spacing of the following vehicle.
    /// The buffers of the model should be drawn 1–2 cm compressed so that vehicles do not
    /// intersect in curves.
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
    /// Wheel slip / wheel slide protection of the vehicle.
    #[serde(default)]
    pub slip_protection: SlipProtection,
    /// Track gauge [m] — checked against the infrastructure and used for the curve
    /// resistance.
    #[serde(default = "standard_gauge")]
    pub gauge: f64,
    /// Highest permitted running speed [km/h] — the running gear limit, independent of the
    /// traction characteristic. 0 = not stated.
    #[serde(default)]
    pub v_max: f64,
    /// Number of axles — information for consist lists and brake sheets, no influence on
    /// the simulation.
    #[serde(default)]
    pub axles: u8,
    /// Total axle base [m]: the sum over all bogies (two bogies of 2.5 m → 5.0), not the
    /// vehicle length. The larger the value, the more the axles are forced in a curve.
    #[serde(default)]
    pub axle_base_sum: f64,
    /// Air resistance cw·A [m²]. When set it replaces the quadratic Davis term with
    /// `½·ρ·cw·A·v²`.
    #[serde(default)]
    pub cw_a: Option<f64>,
    /// Factor on the curve resistance after Röckl. 1 = as calculated from the axle base
    /// sum; raise it for a stiff running gear, lower it for radial steering bogies.
    #[serde(default = "one")]
    pub curve_resistance_factor: f64,
    /// Maximum payload [kg] — passenger coaches roughly 5 t, freight per the anscriptions.
    #[serde(default)]
    pub max_payload: f64,
    /// Maximum tilt angle [°]: 0 for conventional vehicles, ~8 for German tilting units.
    #[serde(default)]
    pub tilt_angle_deg: f64,
    /// The vehicle has passenger doors on both sides — only those follow the door control
    /// of the train (plan ch. 9, [`crate::doors`]).
    #[serde(default)]
    pub passenger_doors: bool,
    /// Train protection fitted to the vehicle (plan 9.1) — vehicle equipment, not a
    /// run-time option.
    #[serde(default)]
    pub safety: SafetyEquipment,
    /// Door control the vehicle brings along; the leading vehicle determines the one the
    /// train runs with ([`crate::doors`]).
    #[serde(default)]
    pub doors: DoorSystem,
    /// Hunting factor −1 … 1: −1 = no hunting, 0 = standard (tuned for bogie vehicles),
    /// above 0 = more than standard (sensible for single-axle running gear).
    #[serde(default)]
    pub hunting: f64,
    /// Optional Lua behaviour script `"<mod>:<script>"` — tap changer logic, AFB, start-up
    /// procedure. Everything physical stays declarative; the script only decides
    /// *behaviour* (plan ch. 19).
    #[serde(default)]
    pub script: Option<String>,
    /// Visual model. The simulation ignores it; app and editor read it (plan ch. 15).
    #[serde(default)]
    pub model: Option<VehicleModel>,
}

impl VehicleSpec {
    /// Braked weight percentage of the empty vehicle: braked weight / mass · 100.
    ///
    /// The figure a German brake sheet is written in.
    /// [`Train::brake_percentage`] is the same quantity for a whole train,
    /// where the load counts as well.
    pub fn brake_percentage(&self) -> f64 {
        if self.mass_empty <= 0.0 {
            return 0.0;
        }
        self.brake.brake_weight / (self.mass_empty / 1000.0) * 100.0
    }
}

impl Default for VehicleSpec {
    /// A blank vehicle — starting point for "New" in the vehicle editor.
    fn default() -> Self {
        Self {
            name: "New vehicle".into(),
            length: 20.0,
            mass_empty: 40_000.0,
            rotating_mass_factor: 0.08,
            davis: Davis {
                a: 800.0,
                b: 20.0,
                c: 5.0,
            },
            brake: BrakeSpec::from_brake_weight(40.0, BrakeKind::Disc),
            traction: None,
            coupler: CouplerSpec::screw(),
            adhesive_mass_fraction: 0.0,
            slip_protection: SlipProtection::None,
            gauge: STANDARD_GAUGE,
            v_max: 160.0,
            axles: 4,
            axle_base_sum: REFERENCE_AXLE_BASE,
            cw_a: None,
            curve_resistance_factor: 1.0,
            max_payload: 0.0,
            tilt_angle_deg: 0.0,
            passenger_doors: false,
            safety: SafetyEquipment::None,
            doors: DoorSystem::None,
            hunting: 0.0,
            script: None,
            model: None,
        }
    }
}

impl VehicleSpec {
    /// Running resistance [N] at speed `v` [m/s].
    ///
    /// `davis` is the basis; where `cw_a` is stated, the quadratic term comes from the
    /// air resistance instead — that is the value found in data sheets.
    pub fn resistance(&self, v: f64) -> f64 {
        match self.cw_a {
            Some(cw_a) => {
                let av = v.abs();
                self.davis.a + self.davis.b * av + 0.5 * AIR_DENSITY * cw_a * av * av
            }
            None => self.davis.resistance(v),
        }
    }

    /// Suggested rolling resistance `a` [N] from the mass — about 2 ‰ of the weight,
    /// the usual starting value for the "suggest" button in the editor.
    pub fn suggested_rolling_resistance(mass_kg: f64) -> f64 {
        mass_kg * crate::G * 0.002
    }
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
    /// Position of the passenger doors (only used with `spec.passenger_doors`).
    #[serde(default)]
    pub doors: VehicleDoors,
}

impl Vehicle {
    pub fn new(spec: VehicleSpec, pos: TrackPosition) -> Self {
        Self {
            brake: BrakeState::new(&spec.brake),
            safety: spec.safety.build(),
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
            doors: VehicleDoors::default(),
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
    /// Door control of the train (TB0/TAV/UIC-WTB).
    #[serde(default)]
    pub doors: DoorControl,
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
        // The driver's desk of the leading vehicle carries the door control.
        let doors = DoorControl::new(vehicles.first().map(|v| v.spec.doors).unwrap_or_default());
        Self {
            vehicles,
            couplers,
            cab: 0,
            rail: RailCondition::Dry,
            number: String::new(),
            doors,
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

#[cfg(test)]
mod vehicle_spec_tests {
    use super::*;

    /// A BR 101: 84 t empty, 90 t braked weight — the brake sheet says 107.
    #[test]
    fn braked_weight_percentage_matches_the_brake_sheet() {
        let mut spec = VehicleSpec {
            mass_empty: 84_000.0,
            ..VehicleSpec::default()
        };
        spec.brake.brake_weight = 90.0;
        assert_eq!(spec.brake_percentage().round(), 107.0);
        // A vehicle without a mass must not divide by zero.
        spec.mass_empty = 0.0;
        assert_eq!(spec.brake_percentage(), 0.0);
    }
}
