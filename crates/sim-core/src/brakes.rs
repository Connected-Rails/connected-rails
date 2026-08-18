//! Air brake (plan ch. 7).
//!
//! Real pneumatics are modelled, not a brake-force slider: the brake pipe as a chain of
//! nodes along the train, one control valve per vehicle with control chamber, auxiliary
//! reservoir and brake cylinder, the main reservoir with its compressor behind it.
//!
//! Every vehicle brakes for itself. A train is not "one brake force" — the rear of a long
//! freight train applies seconds after the front, and a wagon whose auxiliary reservoir has
//! run out stops contributing while its neighbours still hold.
//!
//! The named control valve types ([`ControlValve`]) are presets: what is simulated is their
//! observable behaviour — graduated release or not, cylinder pressure stages, the release
//! button of a loco valve — and every one of those parameters can be overridden per vehicle.

use crate::G;
use crate::cab::CabInputs;
use crate::drive::interpolate;
use crate::train::Train;
use serde::{Deserialize, Serialize};

/// Nominal operating pressure of the brake pipe [bar].
pub const PIPE_NOMINAL: f64 = 5.0;
/// Charging pressure during the release surge [bar].
pub const PIPE_OVERCHARGE: f64 = 5.4;
/// Pressure drop at which the control valve responds [bar].
pub const RESPONSE_DROP: f64 = 0.3;
/// Pressure drop for a full service application [bar].
pub const FULL_SERVICE_DROP: f64 = 1.5;
/// Compressor cut-in pressure of the main reservoir [bar].
pub const COMPRESSOR_CUT_IN: f64 = 8.0;
/// Compressor cut-out pressure of the main reservoir [bar].
pub const COMPRESSOR_CUT_OUT: f64 = 10.0;
/// Below this main reservoir pressure a spring-applied parking brake applies by itself [bar].
pub const SPRING_RELEASE_PRESSURE: f64 = 4.5;

/// Working vacuum of a vacuum brake [bar below atmosphere] — 20 inHg, the British figure.
pub const VACUUM_NOMINAL: f64 = 0.68;
/// Vacuum destroyed for a full application [bar].
pub const VACUUM_FULL_SERVICE: f64 = 0.51;
/// Loss of vacuum at which the brake starts to come on [bar].
pub const VACUUM_RESPONSE: f64 = 0.05;

/// What the brake works with.
///
/// Two systems, one set of equations: in both of them the pipe carries a value that stands
/// for "released", the driver destroys it to brake, and the vehicle compares what is left
/// against a reference it holds. Only the direction of the pump and the numbers differ —
/// which is why [`BrakeState::pipe`] means bar of pressure on an air brake and bar of
/// vacuum on a vacuum brake, and why everything downstream of it can stay as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BrakeMedium {
    /// Compressed air (Westinghouse/Knorr). Everything built since about 1930.
    #[default]
    Air,
    /// Vacuum brake: an exhauster pulls the pipe down, and destroying the vacuum applies
    /// the brake. British and Indian stock, and every steam-era train that kept it.
    Vacuum,
}

impl BrakeMedium {
    /// Value of a fully released pipe.
    pub fn nominal(self) -> f64 {
        match self {
            BrakeMedium::Air => PIPE_NOMINAL,
            BrakeMedium::Vacuum => VACUUM_NOMINAL,
        }
    }

    /// Highest value the pipe may reach — an air brake overcharges, a vacuum brake cannot
    /// pull below the exhauster's limit.
    pub fn ceiling(self) -> f64 {
        match self {
            BrakeMedium::Air => PIPE_OVERCHARGE,
            BrakeMedium::Vacuum => VACUUM_NOMINAL,
        }
    }

    /// Drop that makes a full service application.
    pub fn full_service_drop(self) -> f64 {
        match self {
            BrakeMedium::Air => FULL_SERVICE_DROP,
            BrakeMedium::Vacuum => VACUUM_FULL_SERVICE,
        }
    }

    /// Drop at which the brake responds.
    pub fn response_drop(self) -> f64 {
        match self {
            BrakeMedium::Air => RESPONSE_DROP,
            BrakeMedium::Vacuum => VACUUM_RESPONSE,
        }
    }

    /// A vacuum brake has no reservoir to exhaust and no relay valve.
    pub fn is_vacuum(self) -> bool {
        matches!(self, BrakeMedium::Vacuum)
    }

    pub fn key(self) -> &'static str {
        match self {
            BrakeMedium::Air => "brk-medium-air",
            BrakeMedium::Vacuum => "brk-medium-vacuum",
        }
    }
}

/// Electropneumatic brake (EP): the application is transmitted by wire, not by a pressure
/// wave, so every vehicle of the train applies in the same moment.
///
/// This is what makes a modern multiple unit stop the way it does, and what a long freight
/// train has to do without.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EpBrake {
    /// Filling rate of the cylinder on an EP application [bar/s].
    pub apply_rate: f64,
    /// Release rate [bar/s].
    pub release_rate: f64,
    /// The EP application also destroys the brake pipe, so the pneumatic brake follows it
    /// as a back-up (Bremsstellung "R + E" of a German multiple unit). Without it the EP is
    /// laid on top of an untouched pipe and drops away completely when the wire fails.
    #[serde(default)]
    pub vents_pipe: bool,
    /// Number of steps of the EP application; 0 = continuous.
    #[serde(default)]
    pub steps: u32,
}

impl Default for EpBrake {
    fn default() -> Self {
        Self {
            apply_rate: 2.5,
            release_rate: 2.5,
            vents_pipe: true,
            steps: 0,
        }
    }
}

/// Retaining valve (US practice): held in a position, it keeps a residual pressure in the
/// brake cylinder while the rest of the train releases and recharges. What a freight train
/// on a long descent is set up with, one wagon at a time, by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Retainer {
    /// Direct exhaust — the cylinder empties normally.
    #[default]
    Off,
    /// Slow direct release: the cylinder empties, but takes its time about it.
    SlowDirect,
    /// Low pressure retained [see `Retainer::held`].
    Low,
    /// High pressure retained.
    High,
}

impl Retainer {
    /// Cylinder pressure the setting holds back [bar].
    pub fn held(self) -> f64 {
        match self {
            Retainer::Off | Retainer::SlowDirect => 0.0,
            Retainer::Low => 0.7,
            Retainer::High => 1.4,
        }
    }

    /// Factor on the release rate.
    pub fn release_factor(self) -> f64 {
        match self {
            Retainer::SlowDirect => 0.25,
            _ => 1.0,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Retainer::Off => "off",
            Retainer::SlowDirect => "slow",
            Retainer::Low => "low",
            Retainer::High => "high",
        }
    }
}

/// Brake position (changeover handle on the vehicle).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BrakePosition {
    /// Freight train, long transition times.
    G,
    /// Passenger train.
    #[default]
    P,
    /// Rapid brake position, higher brake force in the upper speed range. A vehicle
    /// fitted with a magnetic track brake (`BrakeSpec::has_mg`) uses it here — that
    /// pair is the "R + Mg" of the anscription.
    R,
}

impl BrakePosition {
    /// Filling time of the brake cylinder (0 → 95 %) [s].
    pub fn apply_time(self) -> f64 {
        match self {
            BrakePosition::G => 22.0,
            _ => 4.0,
        }
    }

    /// Release time of the brake cylinder [s].
    pub fn release_time(self) -> f64 {
        match self {
            BrakePosition::G => 50.0,
            _ => 17.0,
        }
    }

    pub fn is_rapid(self) -> bool {
        matches!(self, BrakePosition::R)
    }

    /// Force bonus in the R range above 60 km/h.
    pub fn high_speed_factor(self, v_kmh: f64) -> f64 {
        match self {
            BrakePosition::R if v_kmh > 60.0 => 1.35,
            _ => 1.0,
        }
    }
}

/// Axle load [t] the light support curve of the friction pairings is stated for — an empty
/// wagon, whose rigging presses the blocks on with correspondingly little force.
pub const LIGHT_AXLE_LOAD: f64 = 5.0;

/// Axle load [t] of the second support curve — a loaded wagon or a locomotive. The curve
/// of a vehicle for which no axle count is stated.
pub const REFERENCE_AXLE_LOAD: f64 = 20.0;

/// Friction behaviour of the brake — how the friction coefficient runs over speed.
///
/// The predefined curves carry the shape of the usual German friction pairings; the level
/// sits in [`BrakeSpec::max_force`], so what matters here is only the drop with speed.
/// Each pairing brings two of them, for a light and for a loaded vehicle — see
/// [`BrakeKind::friction_factor_at`]. Where a data sheet states its own measurements,
/// [`BrakeKind::Custom`] takes them directly.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub enum BrakeKind {
    /// Block brake with cast iron blocks (Grauguss, P10). The friction coefficient
    /// collapses with speed — the shape follows Karwatzki's speed term.
    #[default]
    Block,
    /// Disc brake: nearly constant friction coefficient.
    Disc,
    /// Block brake with composite blocks, K type (organic): considerably flatter than cast
    /// iron, which is why a K-block wagon brakes so much better from high speed.
    CompositeK,
    /// Block brake with composite blocks, LL type (low noise, low friction).
    CompositeLl,
    /// Magnetic track brake: rubs on the rail, the friction coefficient falls sharply.
    Magnetic,
    /// Own friction characteristic: (speed [km/h], friction coefficient). Interpolated
    /// linearly, held beyond the ends. Only the shape is used — the value at a stand is
    /// the reference.
    Custom(Vec<(f64, f64)>),
}

impl BrakeKind {
    /// Friction factor relative to standstill, at the reference axle load.
    pub fn friction_factor(&self, v_kmh: f64) -> f64 {
        self.friction_factor_at(v_kmh, REFERENCE_AXLE_LOAD)
    }

    /// Friction factor relative to standstill for a vehicle with `axle_load_t` [t] per axle.
    ///
    /// Two support curves per pairing — [`LIGHT_AXLE_LOAD`] and [`REFERENCE_AXLE_LOAD`] —
    /// interpolated linearly over the axle load and held beyond the two. What decides the
    /// shape is the block force per block, and the axle load is the number that stands in
    /// for it: the heavier the vehicle, the harder its rigging presses the blocks on, and
    /// the more steeply the friction coefficient falls away with speed.
    ///
    /// Only the *shape* follows the load. Every curve of the family is 1 at a stand,
    /// because the friction level of the vehicle is already in its braked weight, and that
    /// is where [`BrakeSpec::max_force`] comes from.
    ///
    /// ponytail: a family of two closed curves instead of Karwatzki's full
    /// pressure-dependent formula — the block force per block is in no vehicle data sheet,
    /// the axle load is in every one. `Custom` stays the upgrade path where measurements
    /// exist.
    pub fn friction_factor_at(&self, v_kmh: f64, axle_load_t: f64) -> f64 {
        let v = v_kmh.abs();
        // µ(v)/µ(0) = (1 + rise·v)/(1 + fall·v) — light vehicle first, loaded second.
        let (light, loaded) = match self {
            // Cast iron: the classic collapse. The loaded curve is Karwatzki's speed term;
            // with little block force the same pairing holds up noticeably better.
            BrakeKind::Block => ((0.01, 0.038), (0.01, 0.05)),
            // Disc pads hardly care about the pressure — the two curves nearly coincide.
            BrakeKind::Disc => ((0.0, 0.0025), (0.0, 0.003)),
            BrakeKind::CompositeK => ((0.0, 0.0038), (0.0, 0.0045)),
            BrakeKind::CompositeLl => ((0.0, 0.005), (0.0, 0.006)),
            // The magnet presses with its own force; the load of the vehicle is not in it.
            BrakeKind::Magnetic => ((0.0, 0.008), (0.0, 0.008)),
            BrakeKind::Custom(points) => {
                let at_rest = interpolate(points, 0.0);
                return if at_rest.abs() < 1e-9 {
                    1.0
                } else {
                    (interpolate(points, v) / at_rest).max(0.0)
                };
            }
        };
        let curve = |(rise, fall): (f64, f64)| (1.0 + rise * v) / (1.0 + fall * v);
        let t = ((axle_load_t - LIGHT_AXLE_LOAD) / (REFERENCE_AXLE_LOAD - LIGHT_AXLE_LOAD))
            .clamp(0.0, 1.0);
        curve(light) * (1.0 - t) + curve(loaded) * t
    }
}

/// Load-proportional braking (Lastabbremsung).
///
/// [`BrakeSpec::brake_weight`] and [`BrakeSpec::max_force`] are the figures of the fully
/// loaded vehicle. What is set here is how much of them is left when it runs empty — a
/// wagon that brakes with its loaded force while empty flattens its wheels, which is why
/// the device exists.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum LoadBraking {
    /// None: the same brake force empty and loaded — locomotives, and every wagon whose
    /// data sheet states a single braked weight.
    #[default]
    None,
    /// Weighing valve (Wiegeventil, ALB): the cylinder pressure follows the load
    /// steplessly, so the braked weight percentage — and with it the deceleration — stays
    /// where it belongs whatever the vehicle carries. It needs no figure of its own: the
    /// tare mass and the payload of the data sheet are the whole characteristic.
    Weighing,
    /// Empty/loaded changeover (Umstellvorrichtung "Leer/Beladen"): two rigging ratios,
    /// changed over at `changeover_mass_t` [t] total mass. `empty_share` is the share of
    /// the loaded brake force left in the empty position — both braked weights and the
    /// changeover mass are written on the wagon.
    ///
    /// ponytail: the position follows the mass, whether the vehicle changes over by hand
    /// or by weighing valve. A lever left in the wrong position (the classic way to flat
    /// a wheel) needs a state of its own — upgrade path when shunting can set it.
    Changeover {
        empty_share: f64,
        changeover_mass_t: f64,
    },
}

impl LoadBraking {
    /// Share of the fully loaded brake force at a total mass of `mass_t`, for a vehicle
    /// weighing `laden_t` [t] fully loaded.
    pub fn share(self, mass_t: f64, laden_t: f64) -> f64 {
        match self {
            LoadBraking::None => 1.0,
            LoadBraking::Weighing if laden_t > 0.0 => (mass_t / laden_t).clamp(0.0, 1.0),
            LoadBraking::Weighing => 1.0,
            LoadBraking::Changeover {
                empty_share,
                changeover_mass_t,
            } => {
                if mass_t >= changeover_mass_t {
                    1.0
                } else {
                    empty_share.clamp(0.0, 1.0)
                }
            }
        }
    }

    /// The weighing valve sits in the feed to the brake cylinder: its share *is* a
    /// cylinder pressure, so it shows in the gauge and in the air consumption. A
    /// changeover moves the rigging instead — full pressure, less force.
    pub fn at_the_cylinder(self) -> bool {
        matches!(self, LoadBraking::Weighing)
    }
}

/// How a second, higher brake cylinder pressure stage is switched in.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum HighStage {
    /// No second stage.
    #[default]
    None,
    /// Above this speed [km/h] — the changeover of a speed-dependent loco brake.
    Speed(f64),
    /// Only on a full or emergency application.
    Emergency,
}

/// Observable behaviour of a control valve. [`ControlValve`] picks a preset;
/// [`BrakeSpec`] may override each field.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ValveBehaviour {
    /// Graduated release (mehrlösig). A K valve cannot do it: once the brake pipe rises,
    /// the cylinder empties completely.
    pub graduated_release: bool,
    /// The R brake position may be selected.
    pub rapid_position: bool,
    /// Second cylinder pressure stage as a factor of [`BrakeSpec::max_cylinder`].
    pub high_stage: f64,
    /// What switches that stage in.
    pub high_stage_trigger: HighStage,
    /// Traction unit valve: has a release button and can be pre-controlled from the main
    /// reservoir through a relay valve.
    pub loco: bool,
    /// Pressure drop at which the valve responds [bar].
    pub response_drop: f64,
    /// Pressure drop of a full service application [bar].
    pub full_service_drop: f64,
}

/// Control valve type (Steuerventil).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ControlValve {
    /// Knorr K valve, two-pressure system, G/P changeover. Wagons and older locos:
    /// graduated application, but single-release — every release step releases fully.
    KGp,
    /// KE valve (Einheitsbauart), three-pressure system, G/P changeover. Graduated release.
    #[default]
    KeGp,
    /// KE valve with the additional R position — high-speed wagons, block or disc brake.
    KeGpr,
    /// KE valve of a traction unit with the T position: the loco brake can be released on
    /// its own with the release button while the train brake stays applied.
    KeTm,
    /// KE valve of a traction unit with two cylinder pressure stages, changed over by
    /// speed — the high stage above the changeover speed.
    KeL2a,
    /// KE valve of a traction unit with two cylinder pressure stages, the high stage
    /// switched in by a full or emergency application instead of by speed.
    KeL2d,
}

impl ControlValve {
    /// The preset behind the type designation.
    pub fn behaviour(self) -> ValveBehaviour {
        let base = ValveBehaviour {
            graduated_release: true,
            rapid_position: false,
            high_stage: 1.0,
            high_stage_trigger: HighStage::None,
            loco: false,
            response_drop: RESPONSE_DROP,
            full_service_drop: FULL_SERVICE_DROP,
        };
        match self {
            ControlValve::KGp => ValveBehaviour {
                graduated_release: false,
                response_drop: 0.35,
                ..base
            },
            ControlValve::KeGp => base,
            ControlValve::KeGpr => ValveBehaviour {
                rapid_position: true,
                ..base
            },
            ControlValve::KeTm => ValveBehaviour {
                rapid_position: true,
                loco: true,
                ..base
            },
            ControlValve::KeL2a => ValveBehaviour {
                rapid_position: true,
                loco: true,
                high_stage: 1.45,
                high_stage_trigger: HighStage::Speed(55.0),
                ..base
            },
            ControlValve::KeL2d => ValveBehaviour {
                rapid_position: true,
                loco: true,
                high_stage: 1.45,
                high_stage_trigger: HighStage::Emergency,
                ..base
            },
        }
    }
}

/// Wheel slip / wheel slide protection of a powered vehicle (plan ch. 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SlipProtection {
    /// None — a spinning wheelset stays spinning until the driver notices.
    #[default]
    None,
    /// Wheel slip brake: the spinning wheelset is braked briefly. Costs tractive effort,
    /// but catches the slip quickly; the classic answer on older locos.
    SlipBrake,
    /// Throttling of the traction: the drive cuts its own effort back until the slip is
    /// gone and feels its way back up again.
    TractionCutback,
    /// Electronic creep control: holds the creep at the maximum of the adhesion curve
    /// instead of avoiding it — and therefore gets more out of the rail than any cutback.
    CreepControl,
}

impl SlipProtection {
    /// Is there any wheel slide protection when braking?
    pub fn protects(self) -> bool {
        !matches!(self, SlipProtection::None)
    }

    /// Factor on the adhesion limit. Creep control genuinely uses more of the rail.
    pub fn adhesion_bonus(self) -> f64 {
        match self {
            SlipProtection::CreepControl => 1.10,
            _ => 1.0,
        }
    }
}

fn default_aux_volume() -> f64 {
    100.0
}
fn default_pipe_volume() -> f64 {
    20.0
}
fn default_leakage() -> f64 {
    3.0
}

/// Brake equipment of a vehicle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrakeSpec {
    pub kind: BrakeKind,
    pub position: BrakePosition,
    /// Control valve type — decides how the cylinder pressure follows the brake pipe.
    #[serde(default)]
    pub valve: ControlValve,
    /// Overrides the preset of `valve` field by field, for a vehicle whose valve does not
    /// behave like the type designation suggests.
    #[serde(default)]
    pub valve_params: Option<ValveBehaviour>,
    /// Braked weight [t] of the fully loaded vehicle — basis of the braked weight
    /// percentage. What is left of it when the vehicle runs empty is decided by
    /// `load_braking`.
    pub brake_weight: f64,
    /// Load-proportional braking (Lastabbremsung).
    #[serde(default)]
    pub load_braking: LoadBraking,
    /// Brake force at full brake cylinder pressure and standstill [N], fully loaded.
    pub max_force: f64,
    /// Highest brake cylinder pressure [bar].
    pub max_cylinder: f64,
    /// Volume ratio brake cylinder / auxiliary reservoir (exhaustibility).
    pub cylinder_to_reservoir: f64,
    /// Magnetic track brake fitted. It works in brake position `R` only.
    #[serde(default)]
    pub has_mg: bool,
    /// Force of the magnetic track brake [N].
    #[serde(default)]
    pub mg_force: f64,
    /// Direct (additional) brake fitted — powered vehicles only.
    #[serde(default)]
    pub has_direct: bool,
    /// Highest cylinder pressure of the direct brake [bar]; 0 = same as `max_cylinder`.
    #[serde(default)]
    pub direct_max_cylinder: f64,
    /// Parking / hand brake force [N].
    #[serde(default)]
    pub parking_force: f64,
    /// The parking brake is spring-applied (Federspeicher): it is held off by air and
    /// applies by itself once the main reservoir runs empty.
    #[serde(default)]
    pub spring_parking: bool,
    /// Pre-controlled air brake: the cylinder is filled from the main reservoir through a
    /// relay valve instead of from the auxiliary reservoir. Fills faster and cannot be
    /// exhausted — the arrangement on every modern traction unit.
    #[serde(default)]
    pub pilot_controlled: bool,
    /// Air supplement brake: whatever the dynamic brake falls short of is filled up
    /// pneumatically, so the demanded braking force is reached at any speed.
    #[serde(default)]
    pub supplement_brake: bool,
    /// Equalising device (Angleicher): makes up brake pipe leakage in lap position so the
    /// pressure stays where the driver put it.
    #[serde(default)]
    pub angleicher: bool,
    /// Volume of the auxiliary reservoir [l] — reference for the air consumption.
    #[serde(default = "default_aux_volume")]
    pub aux_volume: f64,
    /// Volume of this vehicle's share of the brake pipe [l].
    #[serde(default = "default_pipe_volume")]
    pub pipe_volume: f64,
    /// Volume of the main reservoir [l]; 0 = no main reservoir.
    #[serde(default)]
    pub main_volume: f64,
    /// Compressor delivery [l/min of free air]; 0 = no compressor.
    #[serde(default)]
    pub compressor_delivery: f64,
    /// Leakage of the brake pipe [l/min of free air].
    #[serde(default = "default_leakage")]
    pub leakage: f64,
    /// Air or vacuum.
    #[serde(default)]
    pub medium: BrakeMedium,
    /// Electropneumatic brake fitted.
    #[serde(default)]
    pub ep: Option<EpBrake>,
    /// Limiting valve in the feed to the brake cylinder [bar]; 0 = none. It caps what any
    /// source may put into the cylinder — the direct brake of a loco included, which is how
    /// a driver is stopped from flatting the wheels with the independent brake alone.
    #[serde(default)]
    pub limit_pressure: f64,
    /// Retaining valve fitted (US freight stock).
    #[serde(default)]
    pub has_retainer: bool,
    /// Emergency valve fitted — the handle in the passenger compartment or the cab. It
    /// vents this vehicle's brake pipe straight to atmosphere, which brings the whole
    /// train to a stand whatever the driver's valve says.
    #[serde(default)]
    pub has_emergency_valve: bool,
    /// The brake pipe reaches the front end of the vehicle (cock and hose fitted there).
    /// A vehicle whose pipe stops short cannot pass the brake through, and everything
    /// behind it in the train is dead weight.
    #[serde(default = "yes")]
    pub pipe_front: bool,
    /// The same at the rear end.
    #[serde(default = "yes")]
    pub pipe_rear: bool,
}

fn yes() -> bool {
    true
}

impl BrakeSpec {
    /// Derive the brake equipment from the braked weight.
    ///
    /// The factor is calibrated against the braking table: a train with a braked weight
    /// percentage of 100 comes to a stand from 100 km/h with emergency braking in the
    /// order of 500 m (see test `emergency_braking_from_100_kmh_matches_the_brake_table`).
    pub fn from_brake_weight(brake_weight_t: f64, kind: BrakeKind) -> Self {
        Self {
            kind,
            position: BrakePosition::P,
            valve: ControlValve::KeGp,
            valve_params: None,
            brake_weight: brake_weight_t,
            load_braking: LoadBraking::None,
            max_force: brake_weight_t * 1000.0 * G * 0.145,
            max_cylinder: 3.8,
            cylinder_to_reservoir: 0.35,
            has_mg: false,
            mg_force: 0.0,
            has_direct: false,
            direct_max_cylinder: 0.0,
            parking_force: 0.0,
            spring_parking: false,
            pilot_controlled: false,
            supplement_brake: false,
            angleicher: false,
            aux_volume: default_aux_volume(),
            pipe_volume: default_pipe_volume(),
            main_volume: 0.0,
            compressor_delivery: 0.0,
            leakage: default_leakage(),
            medium: BrakeMedium::Air,
            ep: None,
            limit_pressure: 0.0,
            has_retainer: false,
            has_emergency_valve: false,
            pipe_front: true,
            pipe_rear: true,
        }
    }

    /// Vacuum-braked stock: no auxiliary reservoir to exhaust, no relay valve, and the
    /// exhauster in place of the compressor.
    pub fn as_vacuum(mut self) -> Self {
        self.medium = BrakeMedium::Vacuum;
        self.pilot_controlled = false;
        self.supplement_brake = false;
        self
    }

    pub fn with_ep(mut self, ep: EpBrake) -> Self {
        self.ep = Some(ep);
        self
    }

    pub fn with_position(mut self, position: BrakePosition) -> Self {
        self.position = position;
        self
    }

    pub fn with_valve(mut self, valve: ControlValve) -> Self {
        self.valve = valve;
        self
    }

    pub fn with_load_braking(mut self, load_braking: LoadBraking) -> Self {
        self.load_braking = load_braking;
        self
    }

    pub fn with_direct_brake(mut self) -> Self {
        self.has_direct = true;
        self
    }

    pub fn with_mg(mut self, force: f64) -> Self {
        self.has_mg = true;
        self.mg_force = force;
        self
    }

    /// Traction unit equipment: main reservoir, compressor, pre-control, supplement brake,
    /// equalising device, EP brake and a spring-applied parking brake.
    pub fn as_traction_unit(mut self, valve: ControlValve, parking_force: f64) -> Self {
        self.valve = valve;
        self.has_direct = true;
        self.pilot_controlled = true;
        self.supplement_brake = true;
        self.angleicher = true;
        self.spring_parking = true;
        self.parking_force = parking_force;
        self.main_volume = 1000.0;
        self.compressor_delivery = 2400.0;
        self.ep = Some(EpBrake::default());
        self
    }

    /// Behaviour of the fitted control valve — the type's preset unless the vehicle
    /// overrides it.
    pub fn behaviour(&self) -> ValveBehaviour {
        self.valve_params.unwrap_or_else(|| self.valve.behaviour())
    }

    /// Brake position actually in effect: a valve without an R position falls back to P.
    pub fn effective_position(&self) -> BrakePosition {
        if self.position.is_rapid() && !self.behaviour().rapid_position {
            BrakePosition::P
        } else {
            self.position
        }
    }

    /// Volume of the brake cylinder [l], derived from the exhaustibility ratio.
    pub fn cylinder_volume(&self) -> f64 {
        self.aux_volume * self.cylinder_to_reservoir
    }
}

/// Runtime state of a vehicle's brake. All pressures in bar (gauge pressure).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BrakeState {
    /// Brake pipe at this vehicle.
    pub pipe: f64,
    /// Control chamber (reference pressure of the control valve).
    pub control_reservoir: f64,
    /// Auxiliary reservoir (R reservoir).
    pub aux_reservoir: f64,
    /// Brake cylinder pressure from the automatic brake.
    pub cylinder: f64,
    /// Brake cylinder pressure from the direct (additional) brake.
    pub direct_cylinder: f64,
    /// Main reservoir (powered vehicles only).
    pub main_reservoir: f64,
    /// Magnetic track brake applied.
    pub mg_applied: bool,
    /// Spring-applied parking / hand brake applied.
    pub parking_applied: bool,
    /// Current brake force [N] (output to the longitudinal dynamics).
    pub force: f64,
    /// Braking force of the dynamic brake at this vehicle [N], positive.
    #[serde(default)]
    pub dynamic_force: f64,
    /// Air taken from the main reservoirs since the start [normal litres].
    #[serde(default)]
    pub air_consumed: f64,
    /// Set point of the equalising device [bar]; 0 = not active. It is captured when the
    /// handle is lapped and thrown away again as soon as it leaves — the model has
    /// deliberately **no memory** across positions.
    #[serde(default)]
    pub angleicher_target: f64,
    /// Lowest brake pipe pressure since the current application — a single-release K valve
    /// needs it to notice that the driver is releasing.
    #[serde(default)]
    pub pipe_low: f64,
    /// A single-release valve is in the middle of releasing and cannot be graduated back.
    #[serde(default)]
    pub releasing: bool,
    /// Spring-applied parking brake: air pressure holding it off [bar].
    #[serde(default)]
    pub spring_chamber: f64,
    /// Compressor currently delivering — latched between cut-in and cut-out pressure.
    #[serde(default)]
    pub compressor_running: bool,
    /// Angle cock at the front end of the vehicle is open.
    #[serde(default)]
    pub cock_front: bool,
    /// Angle cock at the rear end is open.
    #[serde(default)]
    pub cock_rear: bool,
    /// Setting of the retaining valve.
    #[serde(default)]
    pub retainer: Retainer,
    /// Cylinder pressure the EP brake is commanding [bar].
    #[serde(default)]
    pub ep_cylinder: f64,
    /// The emergency valve of this vehicle has been pulled.
    #[serde(default)]
    pub emergency_pulled: bool,
}

impl BrakeState {
    pub fn new(spec: &BrakeSpec) -> Self {
        let nominal = spec.medium.nominal();
        Self {
            pipe: nominal,
            control_reservoir: nominal,
            aux_reservoir: nominal,
            cylinder: 0.0,
            direct_cylinder: 0.0,
            main_reservoir: if spec.main_volume > 0.0 { 9.0 } else { 0.0 },
            mg_applied: false,
            parking_applied: false,
            force: 0.0,
            dynamic_force: 0.0,
            air_consumed: 0.0,
            angleicher_target: 0.0,
            pipe_low: nominal,
            releasing: false,
            spring_chamber: if spec.spring_parking { 6.0 } else { 0.0 },
            compressor_running: false,
            // Both cocks closed is the state a vehicle stands in on its own; coupling a
            // train opens the ones between the vehicles (see `Train::couple_brake_pipe`).
            cock_front: false,
            cock_rear: false,
            retainer: Retainer::Off,
            ep_cylinder: 0.0,
            emergency_pulled: false,
        }
    }

    /// Released?
    pub fn released(&self) -> bool {
        self.applied_cylinder() < 0.15
    }

    /// Cylinder pressure actually acting [bar].
    ///
    /// The highest of the three sources wins — which is exactly what the double check valve
    /// between automatic brake, direct brake and EP brake does in the pipework.
    pub fn applied_cylinder(&self) -> f64 {
        self.cylinder
            .max(self.direct_cylinder)
            .max(self.ep_cylinder)
    }
}

/// Position of the driver's brake valve.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum DriverBrakeValve {
    /// Filling position (release surge, brake pipe above nominal pressure).
    Fill,
    /// Running position — brake pipe at nominal pressure, kept level.
    Release,
    /// Lap position — no connection, pressure stays where it is.
    Lap,
    /// Service application with a pressure drop [bar] below nominal pressure.
    Service(f64),
    /// Emergency braking.
    Emergency,
}

impl DriverBrakeValve {
    /// Target pressure of the brake pipe at the driver's brake valve [bar].
    pub fn target_pressure(self) -> Option<f64> {
        self.target_for(BrakeMedium::Air)
    }

    /// The same for a given medium — a vacuum brake works to the same handle positions,
    /// only the numbers on the gauge are different.
    pub fn target_for(self, medium: BrakeMedium) -> Option<f64> {
        let nominal = medium.nominal();
        let full = medium.full_service_drop();
        match self {
            DriverBrakeValve::Fill => Some(medium.ceiling()),
            DriverBrakeValve::Release => Some(nominal),
            DriverBrakeValve::Lap => None,
            DriverBrakeValve::Service(drop) => {
                // The scale of the handle is the same in both systems: a full application
                // is a full application.
                let drop = drop.clamp(0.0, FULL_SERVICE_DROP) / FULL_SERVICE_DROP * full;
                Some((nominal - drop).max(nominal - full))
            }
            DriverBrakeValve::Emergency => Some(0.0),
        }
    }

    /// Flow towards the target value [bar/s]: charging slower than venting,
    /// emergency braking very fast.
    pub fn flow_rate(self) -> f64 {
        self.flow_rate_for(BrakeMedium::Air)
    }

    /// An exhauster is a far weaker pump than a compressor, which is why a vacuum brake
    /// takes so long to release and why its driver plans further ahead.
    pub fn flow_rate_for(self, medium: BrakeMedium) -> f64 {
        let air = match self {
            DriverBrakeValve::Fill => 1.2,
            DriverBrakeValve::Release => 0.5,
            DriverBrakeValve::Lap => 0.0,
            DriverBrakeValve::Service(_) => 0.6,
            DriverBrakeValve::Emergency => 6.0,
        };
        match medium {
            BrakeMedium::Air => air,
            // Scaled to the vacuum brake's own range, and slower still on the way up.
            BrakeMedium::Vacuum => {
                let scaled = air / FULL_SERVICE_DROP * VACUUM_FULL_SERVICE;
                match self {
                    DriverBrakeValve::Fill | DriverBrakeValve::Release => scaled * 0.45,
                    _ => scaled,
                }
            }
        }
    }

    /// Demand 0…1 the electrically transmitted (pre-controlled) brake passes on — it needs
    /// no pressure wave, every vehicle sees it at the same moment.
    pub fn demand(self) -> f64 {
        match self {
            DriverBrakeValve::Fill | DriverBrakeValve::Release | DriverBrakeValve::Lap => 0.0,
            DriverBrakeValve::Service(drop) => (drop / FULL_SERVICE_DROP).clamp(0.0, 1.0),
            DriverBrakeValve::Emergency => 1.0,
        }
    }

    /// Full or emergency application — the trigger of a `HighStage::Emergency` valve.
    pub fn is_full_application(self) -> bool {
        match self {
            DriverBrakeValve::Emergency => true,
            DriverBrakeValve::Service(drop) => drop >= FULL_SERVICE_DROP - 0.05,
            _ => false,
        }
    }
}

/// Conductance between two neighbouring vehicles [1/s].
///
/// ponytail: node model instead of a pipe PDE (plan 7). The pressure drop therefore
/// propagates diffusively instead of as a wave towards the rear — order and delay are
/// qualitatively right (a long freight train brakes later at the rear), the exact
/// propagation speed is not. Upgrade path: method of characteristics per pipe section.
pub const PIPE_CONDUCTANCE: f64 = 6.0;

/// Rate at which the equalising device makes up leakage [bar/s].
const ANGLEICHER_RATE: f64 = 0.15;

/// One simulation step of the whole brake system of a train.
pub fn step(train: &mut Train, cab: &CabInputs, valve: DriverBrakeValve, dt: f64) {
    let cab_index = train.cab.min(train.vehicles.len().saturating_sub(1));
    // A pure EP application leaves the brake pipe alone — that is what "E" without "R"
    // means, and why losing the wire on such a train releases it completely.
    let pure_ep = cab.ep_brake
        && !matches!(valve, DriverBrakeValve::Emergency)
        && train
            .vehicles
            .get(cab_index)
            .and_then(|v| v.spec.brake.ep.as_ref())
            .is_some_and(|ep| !ep.vents_pipe);
    let pipe_valve = if pure_ep {
        DriverBrakeValve::Release
    } else {
        valve
    };
    // The emergency valve of the occupied cab. A passenger-pulled one is set from outside
    // (scenario or script) and stays pulled until it is reset by hand, as the real one is.
    if let Some(veh) = train.vehicles.get_mut(cab_index)
        && veh.spec.brake.has_emergency_valve
    {
        veh.brake.emergency_pulled = cab.emergency_valve;
    }
    let mut demand_nl = update_pipe(train, pipe_valve, dt);
    let v_kmh = train.speed_kmh().abs();

    for (i, veh) in train.vehicles.iter_mut().enumerate() {
        // Which curve of the friction family this vehicle runs on — the load counts.
        let axle_load = veh.spec.axle_load_t(veh.mass());
        // Load braking: the weighing valve throttles the cylinder pressure, the changeover
        // lever the rigging. Both end in the same force, but only the first one is in the
        // gauge and in the air the cylinder swallows.
        let load = veh.spec.load_share(veh.mass());
        let spec = &veh.spec.brake;
        let (cylinder_share, rigging_share) = if spec.load_braking.at_the_cylinder() {
            (load, 1.0)
        } else {
            (1.0, load)
        };
        let behaviour = spec.behaviour();
        let state = &mut veh.brake;
        state.dynamic_force = veh
            .traction
            .drives
            .iter()
            .map(|d| d.dynamic_force.max(0.0))
            .sum();

        let before_aux = state.aux_reservoir;
        let before_cylinder = state.cylinder;
        update_control_valve(state, spec, &behaviour, valve, v_kmh, cylinder_share, dt);

        // Air that went into the cylinder or back into the auxiliary reservoir has to come
        // from somewhere: everything except the atmosphere is fed through the brake pipe.
        demand_nl += (state.aux_reservoir - before_aux).max(0.0) * spec.aux_volume;
        if spec.pilot_controlled {
            // A pre-controlled cylinder is fed straight from the main reservoir.
            demand_nl += (state.cylinder - before_cylinder).max(0.0) * spec.cylinder_volume();
        }

        // Electrically transmitted brake: no pressure wave, so the whole train applies at
        // once. Only vehicles fitted and wired for it follow.
        match &spec.ep {
            Some(ep) => {
                let ceiling = cylinder_ceiling(spec, &behaviour, valve, v_kmh, cylinder_share);
                demand_nl +=
                    update_ep_brake(state, spec, ep, cab.ep_brake, valve.demand(), ceiling, dt);
            }
            None => state.ep_cylinder = 0.0,
        }

        // Direct (additional) brake — acts on every powered vehicle of the consist, fed
        // from that vehicle's main reservoir.
        if spec.has_direct && (veh.spec.powered() || i == cab_index) {
            let ceiling = if spec.direct_max_cylinder > 0.0 {
                spec.direct_max_cylinder
            } else {
                spec.max_cylinder
            };
            // The weighing valve sits in front of the cylinder, so it throttles whatever
            // fills it — the direct brake of a railcar included, and so does the limiting
            // valve behind it.
            let ceiling = if spec.limit_pressure > 0.0 {
                ceiling.min(spec.limit_pressure)
            } else {
                ceiling
            };
            let target = cab.direct_brake.clamp(0.0, 1.0) * ceiling * cylinder_share;
            let before = state.direct_cylinder;
            approach(&mut state.direct_cylinder, target, 2.0, dt);
            demand_nl += (state.direct_cylinder - before).max(0.0) * spec.cylinder_volume();
        } else if spec.has_direct {
            approach(&mut state.direct_cylinder, 0.0, 2.0, dt);
        }

        // Release button of a loco valve: releases this vehicle's brake alone while the
        // train brake stays applied.
        if behaviour.loco && cab.brake_release {
            state.cylinder = 0.0;
        }

        update_parking_brake(state, spec, cab, dt);

        state.mg_applied = spec.has_mg
            && spec.effective_position().is_rapid()
            && v_kmh > 50.0
            && state.pipe < PIPE_NOMINAL - 1.0;

        state.force = brake_force(spec, state, v_kmh, axle_load, rigging_share);
    }

    update_main_reservoirs(train, demand_nl, dt);
}

/// Highest cylinder pressure the valve currently allows, including the second stage of a
/// two-stage loco valve and the throttling by a weighing valve (`cylinder_share`).
fn cylinder_ceiling(
    spec: &BrakeSpec,
    behaviour: &ValveBehaviour,
    valve: DriverBrakeValve,
    v_kmh: f64,
    cylinder_share: f64,
) -> f64 {
    let high = match behaviour.high_stage_trigger {
        HighStage::None => false,
        HighStage::Speed(threshold) => v_kmh > threshold,
        HighStage::Emergency => valve.is_full_application(),
    };
    spec.max_cylinder * if high { behaviour.high_stage } else { 1.0 } * cylinder_share
}

/// Pressure equalisation in the brake pipe including the driver's brake valve.
/// Returns the air taken from the main reservoirs in this step [normal litres].
fn update_pipe(train: &mut Train, valve: DriverBrakeValve, dt: f64) -> f64 {
    let n = train.vehicles.len();
    if n == 0 {
        return 0.0;
    }
    let mut demand_nl = 0.0;
    let pressures: Vec<f64> = train.vehicles.iter().map(|v| v.brake.pipe).collect();
    // A closed angle cock parts the brake pipe. Everything behind it keeps the pressure it
    // had and never hears the driver again — the fault that leaves the rear of a train
    // unbraked, and the one worth having in the model.
    let joined: Vec<bool> = (1..n)
        .map(|i| train.vehicles[i - 1].brake.cock_rear && train.vehicles[i].brake.cock_front)
        .collect();
    for i in 0..n {
        let mut flow = 0.0;
        if i > 0 && joined[i - 1] {
            flow += PIPE_CONDUCTANCE * (pressures[i - 1] - pressures[i]);
        }
        if i + 1 < n && joined[i] {
            flow += PIPE_CONDUCTANCE * (pressures[i + 1] - pressures[i]);
        }
        let veh = &train.vehicles[i];
        let medium = veh.spec.brake.medium;
        // Consumption by the control valve while recharging the auxiliary reservoir. A
        // vacuum brake has none to recharge.
        if !medium.is_vacuum() && veh.brake.aux_reservoir < veh.brake.pipe {
            flow -= 0.15 * (veh.brake.pipe - veh.brake.aux_reservoir);
        }
        // An open cock at the end of the train vents the pipe to atmosphere. That is the
        // other half of the pair: the train that will not charge.
        let end_open = (i == 0 && veh.brake.cock_front) || (i + 1 == n && veh.brake.cock_rear);
        // A pulled emergency valve does the same thing on purpose, and the driver's valve
        // cannot make it up: that is what "emergency" means.
        let emergency = veh.spec.brake.has_emergency_valve && veh.brake.emergency_pulled;
        if end_open || emergency {
            flow -= 4.0 * veh.brake.pipe;
        }
        // Leakage: a brake pipe is never tight. This is what the equalising device fights.
        let leak_bar_per_s = veh.spec.brake.leakage / 60.0 / veh.spec.brake.pipe_volume.max(1.0);
        if veh.brake.pipe > 0.0 {
            flow -= leak_bar_per_s;
            demand_nl += veh.spec.brake.leakage / 60.0 * dt;
        }
        let ceiling = medium.ceiling();
        let p = &mut train.vehicles[i].brake.pipe;
        *p = (*p + flow * dt).clamp(0.0, ceiling);
    }

    // The driver's brake valve acts at the occupied cab.
    let cab = train.cab.min(n - 1);
    let angleicher = train.vehicles[cab].spec.brake.angleicher;
    let medium = train.vehicles[cab].spec.brake.medium;
    let before = train.vehicles[cab].brake.pipe;
    let state = &mut train.vehicles[cab].brake;
    match valve.target_for(medium) {
        Some(target) => {
            state.angleicher_target = 0.0;
            approach(&mut state.pipe, target, valve.flow_rate_for(medium), dt);
        }
        // Lap. With an equalising device the leakage is made up, without it the pressure
        // slowly sinks and the brake creeps on by itself.
        None if angleicher => {
            if state.angleicher_target <= 0.0 {
                state.angleicher_target = state.pipe;
            }
            let target = state.angleicher_target;
            if state.pipe < target {
                approach(&mut state.pipe, target, ANGLEICHER_RATE, dt);
            }
        }
        None => {}
    }
    demand_nl += (train.vehicles[cab].brake.pipe - before).max(0.0)
        * train.vehicles[cab].spec.brake.pipe_volume;
    demand_nl
}

/// Control valve: brake pipe drop → brake cylinder pressure.
fn update_control_valve(
    state: &mut BrakeState,
    spec: &BrakeSpec,
    behaviour: &ValveBehaviour,
    valve: DriverBrakeValve,
    v_kmh: f64,
    cylinder_share: f64,
    dt: f64,
) {
    let medium = spec.medium;
    // The control chamber follows the brake pipe only while releasing/charging (and never
    // beyond nominal pressure, otherwise the release surge would "overcharge" the brake).
    // On a vacuum brake the same chamber is the vacuum held behind the ball valve, and it
    // follows the pipe the same way round.
    if state.pipe >= state.control_reservoir {
        approach(
            &mut state.control_reservoir,
            state.pipe.min(medium.nominal()),
            0.35 * medium.nominal() / PIPE_NOMINAL,
            dt,
        );
    }
    // The auxiliary reservoir is recharged from the brake pipe. A vacuum brake has none.
    if !medium.is_vacuum() && state.pipe > state.aux_reservoir {
        approach(&mut state.aux_reservoir, state.pipe, 0.15, dt);
    }

    let ceiling = cylinder_ceiling(spec, behaviour, valve, v_kmh, cylinder_share);
    let drop = state.control_reservoir - state.pipe;
    // A vacuum brake has no valve to characterise: the piston sees the difference between
    // the chamber and the pipe, and that difference *is* the brake force.
    let (response, span_full) = if medium.is_vacuum() {
        (medium.response_drop(), medium.full_service_drop())
    } else {
        (behaviour.response_drop, behaviour.full_service_drop)
    };
    let mut target = if drop <= response {
        0.0
    } else {
        // Full cylinder pressure at the full service pressure drop.
        let span = (span_full - response).max(0.02);
        ((drop - response) * ceiling / span).min(ceiling)
    };

    // Single-release valve: once the brake pipe rises noticeably, the cylinder empties
    // completely and cannot be graduated back — the reason a K-valve train must be
    // released in one go and then recharged before it can brake again.
    if !behaviour.graduated_release {
        state.pipe_low = state.pipe_low.min(state.pipe);
        if state.pipe > state.pipe_low + 0.1 {
            state.releasing = true;
        }
        if state.releasing {
            target = 0.0;
            if state.cylinder < 0.05 && state.pipe >= state.control_reservoir - 0.05 {
                state.releasing = false;
                state.pipe_low = state.pipe;
            }
        }
    }

    // Exhaustibility: from the auxiliary reservoir the cylinder can never be charged
    // beyond what is left in it. A pre-controlled cylinder hangs on the main reservoir
    // instead and does not know the problem, and a vacuum cylinder has no reservoir at all.
    if !spec.pilot_controlled && !medium.is_vacuum() {
        target = target.min(state.aux_reservoir);
    }

    // Limiting valve in the feed: nothing gets past it, whatever asked.
    if spec.limit_pressure > 0.0 {
        target = target.min(spec.limit_pressure);
    }

    // Retaining valve: on the way down the cylinder stops at what the setting holds back.
    let retained = if spec.has_retainer {
        state.retainer.held().min(ceiling)
    } else {
        0.0
    };
    target = target.max(retained);

    let position = spec.effective_position();
    let rate = if target > state.cylinder {
        // 0 → 95 % in apply_time. A relay valve fills considerably faster.
        let base = spec.max_cylinder / position.apply_time() * 3.0;
        if spec.pilot_controlled {
            base * 2.0
        } else {
            base
        }
    } else {
        let base = spec.max_cylinder / position.release_time() * 3.0;
        if spec.has_retainer {
            base * state.retainer.release_factor()
        } else {
            base
        }
    };
    let before = state.cylinder;
    approach(&mut state.cylinder, target, rate, dt);

    // Air consumption from the auxiliary reservoir.
    let delta = state.cylinder - before;
    if delta > 0.0 && !spec.pilot_controlled && !medium.is_vacuum() {
        state.aux_reservoir = (state.aux_reservoir - delta * spec.cylinder_to_reservoir).max(0.0);
    }
}

/// Electropneumatic brake: the demand travels by wire, so this vehicle applies the moment
/// the driver moves the handle instead of waiting for the pressure wave.
fn update_ep_brake(
    state: &mut BrakeState,
    spec: &BrakeSpec,
    ep: &EpBrake,
    wired: bool,
    demand: f64,
    ceiling: f64,
    dt: f64,
) -> f64 {
    if !wired {
        // Wire down: the EP drops out and the pneumatic brake underneath it takes over.
        approach(&mut state.ep_cylinder, 0.0, ep.release_rate, dt);
        return 0.0;
    }
    let demand = if ep.steps == 0 {
        demand.clamp(0.0, 1.0)
    } else {
        let steps = ep.steps as f64;
        (demand.clamp(0.0, 1.0) * steps).round() / steps
    };
    let target = demand * ceiling;
    let before = state.ep_cylinder;
    let rate = if target > state.ep_cylinder {
        ep.apply_rate
    } else {
        ep.release_rate
    };
    approach(&mut state.ep_cylinder, target, rate, dt);
    // Fed from the main reservoir like every other pre-controlled cylinder.
    (state.ep_cylinder - before).max(0.0) * spec.cylinder_volume()
}

/// Spring-applied parking brake: air holds it off, the spring applies it.
fn update_parking_brake(state: &mut BrakeState, spec: &BrakeSpec, cab: &CabInputs, dt: f64) {
    if spec.parking_force <= 0.0 {
        return;
    }
    if !spec.spring_parking {
        // Plain hand brake — set by hand, stays where it is.
        state.parking_applied = cab.parking_brake;
        return;
    }
    // The spring chamber is vented to apply and charged from the main reservoir to release.
    let want_release = !cab.parking_brake && state.main_reservoir > SPRING_RELEASE_PRESSURE;
    let target = if want_release {
        state.main_reservoir.min(6.0)
    } else {
        0.0
    };
    approach(&mut state.spring_chamber, target, 1.5, dt);
    state.parking_applied = state.spring_chamber < SPRING_RELEASE_PRESSURE;
}

/// Main reservoirs and compressors. The demand of the whole train is shared by the
/// vehicles that have one.
fn update_main_reservoirs(train: &mut Train, demand_nl: f64, dt: f64) {
    let suppliers: Vec<usize> = train
        .vehicles
        .iter()
        .enumerate()
        .filter(|(_, v)| v.spec.brake.main_volume > 0.0)
        .map(|(i, _)| i)
        .collect();
    if suppliers.is_empty() {
        return;
    }
    let share = demand_nl / suppliers.len() as f64;
    for i in suppliers {
        let spec = &train.vehicles[i].spec.brake;
        let volume = spec.main_volume;
        let delivery = spec.compressor_delivery / 60.0 * dt;
        let state = &mut train.vehicles[i].brake;

        // Pressure switch: the compressor runs from cut-in up to cut-out.
        if state.main_reservoir <= COMPRESSOR_CUT_IN {
            state.compressor_running = true;
        } else if state.main_reservoir >= COMPRESSOR_CUT_OUT {
            state.compressor_running = false;
        }
        let running = state.compressor_running;
        let supplied = if train.vehicles[i].traction.compressor && running {
            delivery
        } else {
            0.0
        };
        let state = &mut train.vehicles[i].brake;
        state.air_consumed += share;
        // Stored air [Nl] = volume [l] × absolute pressure [bar].
        let stored = volume * (state.main_reservoir + 1.0) + supplied - share;
        state.main_reservoir = (stored / volume - 1.0).clamp(0.0, COMPRESSOR_CUT_OUT);
    }
}

/// Brake force of a vehicle [N] — pneumatic brake plus magnetic and parking brake.
///
/// `rigging_share` is the load braking that sits in the rigging rather than in the
/// cylinder pressure (see [`LoadBraking::at_the_cylinder`]).
fn brake_force(
    spec: &BrakeSpec,
    state: &BrakeState,
    v_kmh: f64,
    axle_load_t: f64,
    rigging_share: f64,
) -> f64 {
    let cylinder = state.applied_cylinder();
    let mut f = cylinder / spec.max_cylinder
        * spec.max_force
        * rigging_share
        * spec.kind.friction_factor_at(v_kmh, axle_load_t)
        * spec.effective_position().high_speed_factor(v_kmh);
    // Air supplement brake: the air brake only fills up what the dynamic brake falls short
    // of, and gets out of the way again as soon as the dynamic brake can do it alone.
    // Without it, the two are blended in the longitudinal dynamics instead.
    if spec.supplement_brake {
        f = (f - state.dynamic_force).max(0.0);
    }
    if state.mg_applied {
        f += spec.mg_force * BrakeKind::Magnetic.friction_factor(v_kmh);
    }
    if state.parking_applied {
        f += spec.parking_force;
    }
    f.max(0.0)
}

/// Moves `value` towards `target` at a maximum rate of `rate` [unit/s].
pub(crate) fn approach(value: &mut f64, target: f64, rate: f64, dt: f64) {
    let max_step = rate * dt;
    let diff = target - *value;
    *value += diff.clamp(-max_step, max_step);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cast_iron_loses_far_more_friction_than_a_disc() {
        let cast = BrakeKind::Block.friction_factor(100.0);
        let disc = BrakeKind::Disc.friction_factor(100.0);
        let k = BrakeKind::CompositeK.friction_factor(100.0);
        assert!(cast < k, "cast iron {cast:.3} vs K block {k:.3}");
        assert!(k < disc, "K block {k:.3} vs disc {disc:.3}");
        // Cast iron keeps about a third of its friction at 100 km/h — the classic figure.
        assert!((0.28..0.38).contains(&cast), "{cast:.3}");
        for kind in [
            BrakeKind::Block,
            BrakeKind::Disc,
            BrakeKind::CompositeK,
            BrakeKind::CompositeLl,
            BrakeKind::Magnetic,
        ] {
            assert!((kind.friction_factor(0.0) - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn a_light_vehicle_keeps_more_friction_at_speed() {
        let light = BrakeKind::Block.friction_factor_at(100.0, LIGHT_AXLE_LOAD);
        let loaded = BrakeKind::Block.friction_factor_at(100.0, REFERENCE_AXLE_LOAD);
        assert!(light > loaded, "light {light:.3} vs loaded {loaded:.3}");
        // Halfway between the two axle loads lies halfway between the two curves, and
        // outside them the nearer curve is held.
        let middle = (LIGHT_AXLE_LOAD + REFERENCE_AXLE_LOAD) / 2.0;
        let mixed = BrakeKind::Block.friction_factor_at(100.0, middle);
        assert!((mixed - (light + loaded) / 2.0).abs() < 1e-9);
        assert!((BrakeKind::Block.friction_factor_at(100.0, 1.0) - light).abs() < 1e-9);
        assert!((BrakeKind::Block.friction_factor_at(100.0, 40.0) - loaded).abs() < 1e-9);
        // The level stays with the braked weight: every curve of the family is 1 at a
        // stand, whatever the vehicle weighs.
        for load in [1.0, 5.0, 12.5, 20.0, 40.0] {
            assert!((BrakeKind::Block.friction_factor_at(0.0, load) - 1.0).abs() < 1e-9);
        }
        // The magnet presses with its own force — the load of the vehicle is not in it.
        let empty = BrakeKind::Magnetic.friction_factor_at(100.0, LIGHT_AXLE_LOAD);
        let full = BrakeKind::Magnetic.friction_factor_at(100.0, REFERENCE_AXLE_LOAD);
        assert!((empty - full).abs() < 1e-9);
    }

    #[test]
    fn a_custom_characteristic_is_normalised_to_standstill() {
        let kind = BrakeKind::Custom(vec![(0.0, 0.4), (100.0, 0.2), (200.0, 0.1)]);
        assert!((kind.friction_factor(0.0) - 1.0).abs() < 1e-9);
        assert!((kind.friction_factor(100.0) - 0.5).abs() < 1e-9);
        assert!((kind.friction_factor(50.0) - 0.75).abs() < 1e-9);
        // Beyond the end the last value is held.
        assert!((kind.friction_factor(400.0) - 0.25).abs() < 1e-9);
    }

    #[test]
    fn load_braking_follows_the_load() {
        // Weighing valve: the brake force keeps the ratio to the mass, so the braked
        // weight percentage of the empty vehicle is the one of the loaded vehicle.
        let alb = LoadBraking::Weighing;
        assert!((alb.share(20.0, 80.0) - 0.25).abs() < 1e-9);
        assert!((alb.share(80.0, 80.0) - 1.0).abs() < 1e-9);
        // Overloaded is not braked harder than the design allows, and a vehicle without a
        // payload (a locomotive) keeps its full brake.
        assert!((alb.share(90.0, 80.0) - 1.0).abs() < 1e-9);
        assert!((alb.share(84.0, 0.0) - 1.0).abs() < 1e-9);

        // Changeover lever: two steps, nothing in between.
        let lever = LoadBraking::Changeover {
            empty_share: 0.4,
            changeover_mass_t: 40.0,
        };
        assert!((lever.share(21.0, 78.0) - 0.4).abs() < 1e-9);
        assert!((lever.share(39.9, 78.0) - 0.4).abs() < 1e-9);
        assert!((lever.share(40.0, 78.0) - 1.0).abs() < 1e-9);
        assert!((lever.share(78.0, 78.0) - 1.0).abs() < 1e-9);

        assert!((LoadBraking::None.share(21.0, 78.0) - 1.0).abs() < 1e-9);
        // Where the share acts decides whether the cylinder gauge sees it.
        assert!(alb.at_the_cylinder());
        assert!(!lever.at_the_cylinder());
    }

    #[test]
    fn valve_presets_differ_where_they_should() {
        assert!(!ControlValve::KGp.behaviour().graduated_release);
        assert!(ControlValve::KeGp.behaviour().graduated_release);
        assert!(!ControlValve::KeGp.behaviour().rapid_position);
        assert!(ControlValve::KeGpr.behaviour().rapid_position);
        assert!(ControlValve::KeTm.behaviour().loco);
        assert!(matches!(
            ControlValve::KeL2a.behaviour().high_stage_trigger,
            HighStage::Speed(_)
        ));
        assert!(matches!(
            ControlValve::KeL2d.behaviour().high_stage_trigger,
            HighStage::Emergency
        ));
    }

    #[test]
    fn a_valve_without_an_r_position_falls_back_to_p() {
        let spec = BrakeSpec::from_brake_weight(40.0, BrakeKind::Disc)
            .with_position(BrakePosition::R)
            .with_valve(ControlValve::KeGp);
        assert_eq!(spec.effective_position(), BrakePosition::P);
        let rapid = spec.clone().with_valve(ControlValve::KeGpr);
        assert_eq!(rapid.effective_position(), BrakePosition::R);
    }

    #[test]
    fn the_two_stage_loco_valve_raises_the_ceiling_at_speed() {
        let spec =
            BrakeSpec::from_brake_weight(85.0, BrakeKind::Disc).with_valve(ControlValve::KeL2a);
        let b = spec.behaviour();
        let slow = cylinder_ceiling(&spec, &b, DriverBrakeValve::Service(1.5), 30.0, 1.0);
        let fast = cylinder_ceiling(&spec, &b, DriverBrakeValve::Service(1.5), 120.0, 1.0);
        assert!(fast > slow * 1.3, "{slow:.2} → {fast:.2} bar");

        let d = BrakeSpec::from_brake_weight(85.0, BrakeKind::Disc).with_valve(ControlValve::KeL2d);
        let b = d.behaviour();
        assert!(
            cylinder_ceiling(&d, &b, DriverBrakeValve::Emergency, 20.0, 1.0)
                > cylinder_ceiling(&d, &b, DriverBrakeValve::Service(0.5), 200.0, 1.0)
        );
    }

    #[test]
    fn the_equalising_device_has_no_memory() {
        let valve = DriverBrakeValve::Service(0.8);
        assert!(valve.demand() > 0.5);
        assert_eq!(DriverBrakeValve::Lap.demand(), 0.0);
        assert!(DriverBrakeValve::Emergency.is_full_application());
        assert!(!DriverBrakeValve::Service(0.5).is_full_application());
    }

    /// Runs a single vehicle's control valve for `seconds`, holding the pipe wherever the
    /// caller puts it — enough to test the valve without a whole train around it.
    fn run_valve(spec: &BrakeSpec, pipe: f64, seconds: f64) -> BrakeState {
        let mut state = BrakeState::new(spec);
        let behaviour = spec.behaviour();
        let dt = 1.0 / 200.0;
        for _ in 0..((seconds / dt) as u32) {
            state.pipe = pipe;
            update_control_valve(
                &mut state,
                spec,
                &behaviour,
                DriverBrakeValve::Service(1.5),
                0.0,
                1.0,
                dt,
            );
        }
        state
    }

    #[test]
    fn the_vacuum_brake_works_to_the_same_handle_and_its_own_numbers() {
        let v = BrakeMedium::Vacuum;
        assert!((v.nominal() - VACUUM_NOMINAL).abs() < 1e-9);
        // Release is full vacuum, a full application destroys it down to the bottom.
        assert_eq!(
            DriverBrakeValve::Release.target_for(v),
            Some(VACUUM_NOMINAL)
        );
        assert_eq!(DriverBrakeValve::Emergency.target_for(v), Some(0.0));
        let full = DriverBrakeValve::Service(FULL_SERVICE_DROP)
            .target_for(v)
            .unwrap();
        assert!((full - (VACUUM_NOMINAL - VACUUM_FULL_SERVICE)).abs() < 1e-9);
        // Half the handle is half the application in either system.
        let half_air = PIPE_NOMINAL
            - DriverBrakeValve::Service(FULL_SERVICE_DROP / 2.0)
                .target_for(BrakeMedium::Air)
                .unwrap();
        let half_vac = VACUUM_NOMINAL
            - DriverBrakeValve::Service(FULL_SERVICE_DROP / 2.0)
                .target_for(v)
                .unwrap();
        assert!((half_air / FULL_SERVICE_DROP - half_vac / VACUUM_FULL_SERVICE).abs() < 1e-9);
        // The exhauster is the slower pump — that is the whole feel of the type.
        assert!(
            DriverBrakeValve::Release.flow_rate_for(v)
                < DriverBrakeValve::Release.flow_rate_for(BrakeMedium::Air)
        );
    }

    #[test]
    fn destroying_the_vacuum_applies_the_brake_without_a_reservoir() {
        let spec = BrakeSpec::from_brake_weight(20.0, BrakeKind::Block).as_vacuum();
        let released = run_valve(&spec, VACUUM_NOMINAL, 5.0);
        assert!(released.released());
        let applied = run_valve(&spec, VACUUM_NOMINAL - VACUUM_FULL_SERVICE, 20.0);
        assert!(
            (applied.cylinder - spec.max_cylinder).abs() < 0.2,
            "{:.2} bar",
            applied.cylinder
        );
        // Half destroyed is roughly half the cylinder pressure.
        let half = run_valve(&spec, VACUUM_NOMINAL - VACUUM_FULL_SERVICE / 2.0, 20.0);
        assert!(
            half.cylinder > spec.max_cylinder * 0.3 && half.cylinder < spec.max_cylinder * 0.7,
            "{:.2} bar",
            half.cylinder
        );
        // No auxiliary reservoir is drawn on — there is none.
        assert_eq!(applied.aux_reservoir, VACUUM_NOMINAL);
    }

    #[test]
    fn the_limiting_valve_caps_whatever_asked() {
        let mut spec = BrakeSpec::from_brake_weight(85.0, BrakeKind::Disc);
        spec.limit_pressure = 1.5;
        let applied = run_valve(&spec, PIPE_NOMINAL - FULL_SERVICE_DROP, 20.0);
        assert!(
            applied.cylinder <= 1.5 + 1e-6,
            "{:.2} bar",
            applied.cylinder
        );
        assert!(applied.cylinder > 1.4);
    }

    #[test]
    fn a_retainer_holds_a_residual_pressure_through_the_release() {
        let mut spec = BrakeSpec::from_brake_weight(30.0, BrakeKind::Block);
        spec.has_retainer = true;
        let mut state = BrakeState::new(&spec);
        state.retainer = Retainer::High;
        let behaviour = spec.behaviour();
        let dt = 1.0 / 200.0;
        // Apply, then release the pipe completely.
        for i in 0..(200 * 60) {
            state.pipe = if i < 200 * 20 {
                PIPE_NOMINAL - FULL_SERVICE_DROP
            } else {
                PIPE_NOMINAL
            };
            update_control_valve(
                &mut state,
                &spec,
                &behaviour,
                DriverBrakeValve::Release,
                0.0,
                1.0,
                dt,
            );
        }
        assert!(
            (state.cylinder - Retainer::High.held()).abs() < 0.05,
            "{:.2} bar left, expected {:.2}",
            state.cylinder,
            Retainer::High.held()
        );
        // With the retainer off the same run releases completely.
        state.retainer = Retainer::Off;
        for _ in 0..(200 * 60) {
            state.pipe = PIPE_NOMINAL;
            update_control_valve(
                &mut state,
                &spec,
                &behaviour,
                DriverBrakeValve::Release,
                0.0,
                1.0,
                dt,
            );
        }
        assert!(state.released(), "{:.2} bar", state.cylinder);
    }

    #[test]
    fn the_double_check_valve_lets_the_highest_source_through() {
        let spec = BrakeSpec::from_brake_weight(85.0, BrakeKind::Disc);
        let mut state = BrakeState::new(&spec);
        state.cylinder = 1.0;
        state.direct_cylinder = 2.2;
        state.ep_cylinder = 0.4;
        assert!((state.applied_cylinder() - 2.2).abs() < 1e-9);
        state.ep_cylinder = 3.1;
        assert!((state.applied_cylinder() - 3.1).abs() < 1e-9);
        assert!(!state.released());
    }

    #[test]
    fn the_ep_brake_fills_the_cylinder_at_its_own_rate_and_drops_out_with_the_wire() {
        let spec = BrakeSpec::from_brake_weight(85.0, BrakeKind::Disc).with_ep(EpBrake::default());
        let ep = spec.ep.unwrap();
        let mut state = BrakeState::new(&spec);
        let dt = 1.0 / 200.0;
        for _ in 0..(200 * 2) {
            update_ep_brake(&mut state, &spec, &ep, true, 1.0, spec.max_cylinder, dt);
        }
        assert!(
            (state.ep_cylinder - spec.max_cylinder).abs() < 0.05,
            "{:.2} bar",
            state.ep_cylinder
        );
        // Wire down: the EP lets go and leaves the pneumatic brake to it.
        for _ in 0..(200 * 3) {
            update_ep_brake(&mut state, &spec, &ep, false, 1.0, spec.max_cylinder, dt);
        }
        assert!(state.ep_cylinder < 0.05, "{:.2} bar", state.ep_cylinder);
    }

    #[test]
    fn a_stepped_ep_brake_lands_on_its_notches() {
        let spec = BrakeSpec::from_brake_weight(85.0, BrakeKind::Disc).with_ep(EpBrake {
            steps: 4,
            ..EpBrake::default()
        });
        let ep = spec.ep.unwrap();
        let mut state = BrakeState::new(&spec);
        for _ in 0..(200 * 3) {
            update_ep_brake(&mut state, &spec, &ep, true, 0.3, 4.0, 1.0 / 200.0);
        }
        // 0.3 of the range rounds to the second of four notches: 0.25 × 4 bar.
        assert!(
            (state.ep_cylinder - 1.0).abs() < 0.05,
            "{:.2}",
            state.ep_cylinder
        );
    }
}
