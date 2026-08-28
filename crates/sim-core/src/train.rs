//! Vehicle and train consist model.

use crate::brakes::{BrakeKind, BrakePosition, BrakeSpec, BrakeState, SlipProtection};
use crate::doors::{DoorControl, DoorSystem, VehicleDoors};
use crate::drive::{DriveMode, DriveSpec, MAX_DRIVES, TractionSpec};
use crate::electric::TractionState;
use crate::safety::{SafetyEquipment, SafetySystems};
use crate::sound::SoundSpec;
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

/// What kind of coupling gear a vehicle carries — what decides whether two vehicles can
/// be coupled to each other at all (plan ch. 11).
///
/// The physical parameters below say how a coupler *behaves* once it is made; this says
/// whether a shunter can make it. A screw coupling and a Scharfenberg head do not meet,
/// however alike their draw gear is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CouplerKind {
    /// Screw coupling with side buffers (UIC 520) — the European standard.
    #[default]
    Screw,
    /// Automatic centre buffer coupler of a multiple unit (Scharfenberg and its
    /// relatives). It couples itself, and only ever to its own kind.
    CenterBuffer,
    /// A bar inside a fixed unit. Not a coupler a shunter works: it is undone in the
    /// works, not on the ground, so neither [`crate::Sim::couple`] nor
    /// [`crate::Sim::uncouple`] will touch it.
    Bar,
}

impl CouplerKind {
    /// Whether a shunter can join these two ends.
    ///
    /// Only like to like, and never a bar — which is the whole rule, and the reason a
    /// railcar cannot be put in front of a rake of freight wagons.
    pub fn couples_to(self, other: Self) -> bool {
        self == other && self != CouplerKind::Bar
    }

    /// Message key of the kind's name.
    pub fn key(self) -> &'static str {
        match self {
            CouplerKind::Screw => "coupler-screw",
            CouplerKind::CenterBuffer => "coupler-center-buffer",
            CouplerKind::Bar => "coupler-bar",
        }
    }
}

/// Coupler parameters. Screw coupler: draw gear and buffers separate, slack in between.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CouplerSpec {
    /// What gear the vehicle carries — see [`CouplerKind`].
    #[serde(default)]
    pub kind: CouplerKind,
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
            kind: CouplerKind::Screw,
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
            kind: CouplerKind::CenterBuffer,
            slack: 0.005,
            draw_stiffness: 2.0e7,
            buffer_stiffness: 2.0e7,
            damping: 4.0e5,
            breaking_force: 1.5e6,
        }
    }

    /// A bar inside a fixed unit: like a centre buffer coupler to the physics, but no
    /// shunter takes it apart.
    pub fn bar() -> Self {
        Self {
            kind: CouplerKind::Bar,
            ..Self::center_buffer()
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
    /// The node does not move: its emissive colour is scaled by the value, so
    /// the glow of the material follows a dimmer instead of popping on and off
    /// — instrument backlighting, a dimmable lamp.
    Emissive,
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

/// `body_LOD2` → `Some(2)` — the naming convention app and editors share.
pub fn lod_level(name: &str) -> Option<u8> {
    let (_, tail) = name.rsplit_once("_LOD")?;
    tail.parse().ok()
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
    /// Interactive 3D cab (plan ch. 12). `None` = keyboard only, the cab camera
    /// falls back to its built-in position.
    #[serde(default)]
    pub cab: Option<crate::cab::CabSpec>,
    /// Screens in the cab, rendered to texture (plan ch. 12).
    #[serde(default)]
    pub displays: Vec<crate::cab::DisplaySpec>,
}

/// One axle of the running gear.
///
/// The running gear is what carries the vehicle's forces to the rail, and it does so one
/// axle at a time: each of them has its own share of the weight, and therefore its own
/// adhesion limit and its own slip. A locomotive on a greasy rail loses an axle, not the
/// whole machine — which is exactly what a driver feels and what a wheel slip protection
/// is there to answer.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AxleSpec {
    /// The axle takes traction.
    pub driven: bool,
    /// Share of the vehicle's mass this axle carries (the shares sum to 1).
    pub load_share: f64,
}

impl AxleSpec {
    /// A layout of `axles` axles of which the share `driven` of the weight is on driven
    /// ones — what a vehicle that states only those two numbers has.
    ///
    /// The driven axles are the leading ones and carry exactly the stated share between
    /// them, so `adhesive_mass()` comes out at the figure the data sheet gives whatever the
    /// axle count divides into.
    pub fn layout(axles: u8, driven: f64) -> Vec<AxleSpec> {
        let count = axles as usize;
        if count == 0 {
            return Vec::new();
        }
        let driven_share = driven.clamp(0.0, 1.0);
        let driven_count = if driven_share <= 0.0 {
            0
        } else {
            ((count as f64 * driven_share).round() as usize).clamp(1, count)
        };
        let idle_count = count - driven_count;
        (0..count)
            .map(|i| {
                let driven = i < driven_count;
                let load_share = if driven {
                    driven_share / driven_count as f64
                } else if idle_count > 0 {
                    (1.0 - driven_share) / idle_count as f64
                } else {
                    0.0
                };
                AxleSpec { driven, load_share }
            })
            .collect()
    }
}

/// What the data sheet says about the vehicle rather than about its physics: the entries a
/// vehicle browser lists and sorts by (plan ch. 15).
///
/// Nothing here is read by the simulation. Everything is a plain value with a default, so a
/// vehicle that states none of it costs no bytes in the file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VehicleMeta {
    /// Class or type designation: `"BR 101"`, `"Bmz 236"`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub class: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub manufacturer: String,
    /// Year the vehicle was built; 0 = not stated.
    #[serde(default, skip_serializing_if = "is_zero_year")]
    pub build_year: u16,
    /// Era, free-form: `"V"`, `"IV–VI"`. Not an enum — the scheme is a national one, and
    /// a vehicle that spans two of them says so in one string.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub epoch: String,
    /// Country of use, ISO 3166-1 alpha-2: `"DE"`, `"AT"`, `"CH"`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub country: String,
    /// Railway the vehicle runs for: `"DB Fernverkehr"`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub operator: String,
    /// Who built the file, not who built the vehicle.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub author: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Preview image below the `mods/` directory, addressed like [`VehicleModel::file`].
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub thumbnail: String,
}

fn is_zero_year(year: &u16) -> bool {
    *year == 0
}

impl VehicleMeta {
    /// Nothing filled in — then the whole block stays out of the file.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// The same vehicle in a different dress: a livery, a running number series, an era.
///
/// A variant overrides **appearance and numbering only**. Everything the physics reads
/// stays with the vehicle — otherwise every livery would be a second vehicle to balance,
/// and a consist would no longer be reproducible from the vehicle it names.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VehicleVariant {
    /// How the browser lists it: `"verkehrsrot"`, `"Epoche IV"`. Mod content, like a
    /// scenario text — not an i18n key.
    pub name: String,
    /// Replaces [`VehicleModel::file`]; `None` = the vehicle's own model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Running numbers the consist builder draws from, e.g. `["101 001-6", "101 002-4"]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub numbers: Vec<String>,
    /// Replaces [`VehicleMeta::epoch`]; empty = the vehicle's.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub epoch: String,
    /// Replaces [`VehicleMeta::description`]; empty = the vehicle's.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

impl VehicleVariant {
    /// The running number this variant carries for `seed`, or `None` where it lists none.
    ///
    /// Deterministic on purpose: the consist is built from the scenario's seed, so every
    /// client puts the same number on the same wagon without a byte crossing the network.
    pub fn number(&self, seed: u64) -> Option<&str> {
        if self.numbers.is_empty() {
            return None;
        }
        let i = crate::rng::Rng::new(seed).next_u64() as usize % self.numbers.len();
        self.numbers.get(i).map(String::as_str)
    }
}

/// One load a wagon can carry: coal, containers, timber — a mass and the model node that
/// shows it.
///
/// ponytail: the load is a node of the vehicle's own glTF, switched by
/// [`Motion::Visibility`] like any other part — no second scene, no load model of its own.
/// That covers a coal heap or a stack of containers modelled into the wagon, which is what
/// a wagon has. A mod that really needs interchangeable load models gets a
/// `model: Option<VehicleModel>` here and a second scene spawned at the vehicle.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LoadSpec {
    /// Name of the goods: `"Kohle"`, `"Container"`. Mod content, like a scenario text.
    pub name: String,
    /// Mass of the load [kg].
    pub mass: f64,
    /// glTF node shown while this load is carried; `None` = the load is not visible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
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
    /// Traction chains of the vehicle. Empty = unpowered. More than one is a
    /// multi-engine vehicle or, where the modes differ, a dual-mode one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drives: Vec<DriveSpec>,
    /// The single chain older vehicle files carry. [`VehicleSpec::normalise`] folds it
    /// into `drives`; nothing reads it afterwards and it is never written back.
    #[serde(default, rename = "traction", skip_serializing)]
    pub legacy_traction: Option<TractionSpec>,
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
    /// Number of axles — for consist lists and brake sheets, and the axle load behind
    /// [`VehicleSpec::axle_load_t`].
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
    /// AFB fitted (plan 9.4) — a target speed controller on the power controller,
    /// not a train protection system. Under LZB guidance the LZB's v-soll caps
    /// the dial, so the train runs down the braking curve by itself.
    #[serde(default)]
    pub afb: bool,
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
    /// Sound table (plan ch. 13, [`crate::sound`]): which sample follows which quantity,
    /// under which conditions, started by which trigger. Empty means the vehicle runs on
    /// [`crate::sound::default_table`] — the generated loops.
    #[serde(default)]
    pub sounds: Vec<SoundSpec>,
    /// Block diagram of the vehicle ([`crate::blocks`]). When present, loading bakes it
    /// over `traction`, `brake`, `safety`, `doors`, `afb`, `slip_protection` and the
    /// wheelset figures — the graph is the source of truth, the baked fields are the
    /// runtime format.
    #[serde(default)]
    pub graph: Option<crate::blocks::VehicleGraph>,
    /// Control logic compiled out of the diagram's logic blocks
    /// ([`crate::signal`]). Empty = the vehicle runs on its hardwired behaviour.
    #[serde(
        default,
        skip_serializing_if = "crate::signal::SignalProgram::is_empty"
    )]
    pub signal: crate::signal::SignalProgram,
    /// Battery and current collector — the vehicle's own electrical system, as distinct
    /// from its traction chains.
    #[serde(default)]
    pub supply: crate::electric::PowerSupply,
    /// Sand the sander lays down [kg/min]; 0 = no sanding gear.
    #[serde(default = "default_sand_rate")]
    pub sand_rate: f64,
    /// The running gear axle by axle. Empty = derived from `axles` and
    /// `adhesive_mass_fraction`, which is what a vehicle that states only those two has;
    /// the diagram's `axle` blocks fill it in where the layout is not that even.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub running_gear: Vec<AxleSpec>,
    /// Data sheet entries — class, era, author, preview picture. The simulation ignores
    /// them; the vehicle browser of the consist editor lists and sorts by them.
    #[serde(default, skip_serializing_if = "VehicleMeta::is_empty")]
    pub meta: VehicleMeta,
    /// Liveries and running number series of this vehicle. Empty = the vehicle comes in
    /// exactly one dress, the one `model` and `meta` describe.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<VehicleVariant>,
    /// The loads the vehicle can carry, capped by `max_payload`. Empty = it carries
    /// nothing visible, and `Vehicle::load` is a bare number as before.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loads: Vec<LoadSpec>,
}

fn default_sand_rate() -> f64 {
    4.0
}

impl VehicleSpec {
    /// Folds a legacy `traction` field into `drives`. Every loader calls it; running it
    /// twice is harmless.
    pub fn normalise(&mut self) {
        if let Some(traction) = self.legacy_traction.take()
            && self.drives.is_empty()
        {
            self.drives.push(DriveSpec::new(traction));
        }
        self.drives.truncate(MAX_DRIVES);
    }

    /// Does the vehicle drive at all?
    pub fn powered(&self) -> bool {
        !self.drives.is_empty()
    }

    /// The first traction chain — what single-drive code used to read as `traction`.
    pub fn traction(&self) -> Option<&TractionSpec> {
        self.drives.first().map(|d| &d.traction)
    }

    /// Highest speed any of the chains allows [km/h]; 0 without a drive.
    pub fn drive_v_max(&self) -> f64 {
        self.drives
            .iter()
            .map(|d| d.traction.v_max())
            .fold(0.0, f64::max)
    }

    /// Tractive effort of all chains of `mode` together at `v` [m/s].
    pub fn available_force(&self, mode: DriveMode, v: f64) -> f64 {
        self.drives
            .iter()
            .filter(|d| d.mode == mode)
            .map(|d| d.traction.available_force(v))
            .sum()
    }

    /// Dynamic brake force of all chains of `mode` together at `v` [m/s].
    pub fn available_brake_force(&self, mode: DriveMode, v: f64) -> f64 {
        self.drives
            .iter()
            .filter(|d| d.mode == mode)
            .map(|d| d.traction.available_brake_force(v))
            .sum()
    }

    /// Does any chain have a dynamic brake?
    pub fn has_dynamic_brake(&self) -> bool {
        self.drives.iter().any(|d| d.traction.has_dynamic_brake())
    }

    /// The power sources the vehicle can run on, in the order the chains are listed.
    /// More than one means it has a mode selector.
    pub fn modes(&self) -> Vec<DriveMode> {
        let mut modes: Vec<DriveMode> = Vec::new();
        for drive in &self.drives {
            if !modes.contains(&drive.mode) {
                modes.push(drive.mode);
            }
        }
        modes
    }

    /// Braked weight percentage of the empty vehicle: braked weight / mass · 100.
    ///
    /// The figure a German brake sheet is written in.
    /// [`Train::brake_percentage`] is the same quantity for a whole train,
    /// where the load counts as well.
    pub fn brake_percentage(&self) -> f64 {
        if self.mass_empty <= 0.0 {
            return 0.0;
        }
        self.brake_weight_at(self.mass_empty) / (self.mass_empty / 1000.0) * 100.0
    }

    /// Total mass fully loaded [kg].
    pub fn mass_laden(&self) -> f64 {
        self.mass_empty + self.max_payload
    }

    /// The variant at `index`, or `None` where the vehicle has none there.
    pub fn variant(&self, index: usize) -> Option<&VehicleVariant> {
        self.variants.get(index)
    }

    /// The glTF file the vehicle is drawn with in this variant — the variant's own model
    /// where it states one, otherwise the vehicle's. App and editor read it here so that
    /// the fallback is decided once.
    pub fn model_file(&self, variant: Option<usize>) -> Option<&str> {
        variant
            .and_then(|i| self.variants.get(i))
            .and_then(|v| v.model.as_deref())
            .or_else(|| self.model.as_ref().map(|m| m.file.as_str()))
    }

    /// Mass of the load at `index` [kg], capped at `max_payload`. `None` = the vehicle runs
    /// empty. A vehicle that states no maximum payload (0 = not stated) carries the load
    /// in full.
    pub fn payload(&self, load: Option<usize>) -> f64 {
        let mass = load.and_then(|i| self.loads.get(i)).map_or(0.0, |l| l.mass);
        if self.max_payload > 0.0 {
            mass.min(self.max_payload)
        } else {
            mass
        }
    }

    /// Total mass [kg] with the load at `index`: tare plus what [`Self::payload`] allows.
    pub fn mass_with_load(&self, load: Option<usize>) -> f64 {
        self.mass_empty + self.payload(load)
    }

    /// Share of the fully loaded brake force the vehicle brakes with at a total mass of
    /// `mass_kg` — see [`crate::brakes::LoadBraking`].
    pub fn load_share(&self, mass_kg: f64) -> f64 {
        self.brake
            .load_braking
            .share(mass_kg / 1000.0, self.mass_laden() / 1000.0)
    }

    /// Braked weight [t] at a total mass of `mass_kg`: the anscribed figure of the loaded
    /// vehicle, reduced by the load braking.
    pub fn brake_weight_at(&self, mass_kg: f64) -> f64 {
        self.brake.brake_weight * self.load_share(mass_kg)
    }

    /// The running gear of this vehicle: what it states, or the even layout its axle
    /// count and adhesive mass imply.
    pub fn running_gear(&self) -> Vec<AxleSpec> {
        if self.running_gear.is_empty() {
            AxleSpec::layout(self.axles, self.adhesive_mass_fraction)
        } else {
            self.running_gear.clone()
        }
    }

    /// Axle load [t] at a total mass of `mass_kg` — the curve of the friction family the
    /// vehicle runs on ([`crate::brakes::BrakeKind::friction_factor_at`]). Without an
    /// axle count in the data sheet it stays on the reference curve.
    pub fn axle_load_t(&self, mass_kg: f64) -> f64 {
        if self.axles == 0 {
            return crate::brakes::REFERENCE_AXLE_LOAD;
        }
        mass_kg / 1000.0 / self.axles as f64
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
            drives: Vec::new(),
            legacy_traction: None,
            signal: crate::signal::SignalProgram::default(),
            supply: crate::electric::PowerSupply::default(),
            sand_rate: default_sand_rate(),
            running_gear: Vec::new(),
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
            afb: false,
            doors: DoorSystem::None,
            hunting: 0.0,
            script: None,
            model: None,
            sounds: Vec::new(),
            graph: None,
            meta: VehicleMeta::default(),
            variants: Vec::new(),
            loads: Vec::new(),
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
    /// The load out of `spec.loads` the vehicle carries; `None` = empty or a bare payload
    /// mass. It decides `load` and which model node is shown, so it is part of the
    /// deterministic state and travels with the consist, not with the renderer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_index: Option<usize>,
    /// The variant out of `spec.variants` the vehicle runs in; `None` = the base vehicle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<usize>,
    /// Distance travelled along the track [m], monotonic in the direction of travel.
    pub x: f64,
    /// Speed [m/s], positive = direction of travel of the train.
    pub v: f64,
    /// Acceleration of the last step [m/s²] — what the physics actually applied, after the
    /// standstill hold and the brake's zero crossing. The network sends it so a client can
    /// extrapolate a correction over the transit time (`net.rs`).
    #[serde(default)]
    pub a: f64,
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
    /// The running gear, axle by axle.
    #[serde(default)]
    pub axles: Vec<AxleState>,
    /// Memory of the vehicle's signal graph.
    #[serde(default)]
    pub signal: crate::signal::SignalState,
    /// What the signal graph wrote last step.
    #[serde(default)]
    pub signal_out: crate::signal::SignalOutputs,
}

impl Vehicle {
    pub fn new(spec: VehicleSpec, pos: TrackPosition) -> Self {
        Self {
            brake: BrakeState::new(&spec.brake),
            axles: spec
                .running_gear()
                .into_iter()
                .map(AxleState::new)
                .collect(),
            safety: spec.safety.build(),
            traction: TractionState::default(),
            signal: crate::signal::SignalState::new(&spec.signal),
            signal_out: crate::signal::SignalOutputs::default(),
            spec,
            load: 0.0,
            load_index: None,
            variant: None,
            x: 0.0,
            v: 0.0,
            a: 0.0,
            pos,
            slip: 0.0,
            sanding: false,
            tractive_effort: 0.0,
            brake_effort: 0.0,
            doors: VehicleDoors::default(),
        }
    }

    /// Loads the vehicle with `index` out of `spec.loads` (`None` = empty) and sets `load`
    /// to the mass that goes with it.
    pub fn set_load(&mut self, index: Option<usize>) {
        self.load_index = index;
        self.load = self.spec.payload(index);
    }

    /// The glTF node the current load shows, if it shows one.
    pub fn load_node(&self) -> Option<&str> {
        self.load_index
            .and_then(|i| self.spec.loads.get(i))
            .and_then(|l| l.node.as_deref())
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
    /// Mass on the driven axles [kg] — the sum of their load shares, which for a vehicle
    /// that states nothing but `adhesive_mass_fraction` is exactly that fraction.
    pub fn adhesive_mass(&self) -> f64 {
        let share: f64 = self
            .axles
            .iter()
            .filter(|a| a.spec.driven)
            .map(|a| a.spec.load_share)
            .sum();
        self.mass() * share
    }

    pub fn is_powered(&self) -> bool {
        self.spec.powered()
    }
}

/// Running state of one axle.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AxleState {
    pub spec: AxleSpec,
    /// Slip speed of this axle [m/s]: positive = spinning under traction, negative =
    /// sliding under the brake.
    pub slip: f64,
    /// Tractive effort this axle is actually putting on the rail [N].
    #[serde(default)]
    pub tractive_effort: f64,
    /// Brake force this axle is actually putting on the rail [N].
    #[serde(default)]
    pub brake_effort: f64,
}

impl AxleState {
    pub fn new(spec: AxleSpec) -> Self {
        Self {
            spec,
            slip: 0.0,
            tractive_effort: 0.0,
            brake_effort: 0.0,
        }
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

/// One end of a consist — the two places a coupling can be made.
///
/// `Head` is the end the first vehicle shows, `Tail` the one the last vehicle shows. It
/// has nothing to do with which way the train is running: a train that sets back is
/// running out of its tail, and its head is still its head.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsistEnd {
    Head,
    Tail,
}

impl ConsistEnd {
    /// The other end.
    pub fn opposite(self) -> Self {
        match self {
            ConsistEnd::Head => ConsistEnd::Tail,
            ConsistEnd::Tail => ConsistEnd::Head,
        }
    }

    /// `+1` at the head, `−1` at the tail: the sign that turns the train's own direction
    /// of travel into the direction that points *out of* the consist at this end.
    pub fn outward(self) -> f64 {
        match self {
            ConsistEnd::Head => 1.0,
            ConsistEnd::Tail => -1.0,
        }
    }

    /// Message key of the end's name.
    pub fn key(self) -> &'static str {
        match self {
            ConsistEnd::Head => "shunt-end-head",
            ConsistEnd::Tail => "shunt-end-tail",
        }
    }
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
    /// Train movement or shunting movement — which signals bind it, how fast it may run,
    /// and whether it may be let into an occupied track (plan ch. 11).
    ///
    /// It is not set by hand: passing a signal sets it from that signal's aspect, so a
    /// shunt that is given a train route leaves the starting signal as a train.
    #[serde(default)]
    pub movement: crate::shunt::Movement,
    /// Out of service: the train belongs to no working at the moment, is not driven, is
    /// not drawn, and does not occupy the track (plan ch. 11).
    ///
    /// An operating day reuses its trains — the unit that arrives at ten past forms the
    /// service that leaves at half past, and between the two it stands in the sidings.
    /// Stabling it rather than deleting it is what keeps the train indices stable, which
    /// is what everything from the AI driver to the network protocol addresses trains by.
    #[serde(default)]
    pub stabled: bool,
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
        let mut train = Self {
            vehicles,
            couplers,
            cab: 0,
            rail: RailCondition::Dry,
            number: String::new(),
            doors,
            movement: crate::shunt::Movement::default(),
            stabled: false,
        };
        train.couple_brake_pipe();
        train
    }

    /// Couples the brake pipe through the whole train: every cock between two vehicles is
    /// opened, the two at the ends stay shut. That is what a shunter does when the train is
    /// made up, and what [`crate::brakes`] needs before the pipe will charge.
    pub fn couple_brake_pipe(&mut self) {
        let n = self.vehicles.len();
        for (i, vehicle) in self.vehicles.iter_mut().enumerate() {
            // A cock can only be opened where the pipe actually reaches the end.
            vehicle.brake.cock_front = i > 0 && vehicle.spec.brake.pipe_front;
            vehicle.brake.cock_rear = i + 1 < n && vehicle.spec.brake.pipe_rear;
        }
    }

    /// Total mass [kg].
    pub fn mass(&self) -> f64 {
        self.vehicles.iter().map(Vehicle::mass).sum()
    }

    /// Sets the changeover handle of every vehicle — what the shunter does when the train
    /// is made up (a freight train on G, a passenger train on P or R). A vehicle whose
    /// valve does not know the position falls back to P when it brakes, see
    /// [`BrakeState::effective_position`].
    pub fn set_brake_position(&mut self, position: BrakePosition) {
        for vehicle in &mut self.vehicles {
            vehicle.brake.position = position;
        }
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

    /// Position of the head of the train, or `None` when the consist is empty.
    ///
    /// A train that has been coupled away keeps its slot in [`crate::Sim::trains`] and
    /// becomes an empty consist (plan ch. 11) — it stands nowhere, and everything that
    /// needs a place on the line has to be able to hear that.
    pub fn head(&self) -> Option<TrackPosition> {
        let front = self.vehicles.first()?;
        Some(front.pos.offset_by_unchecked(front.spec.length / 2.0))
    }

    /// Position of the head of the train.
    ///
    /// An empty consist answers with the start of the first edge: it is nowhere, and this
    /// is the nowhere the track graph has. Callers that can act on "nowhere" take
    /// [`Train::head`] instead.
    pub fn head_position(&self) -> TrackPosition {
        self.head()
            .unwrap_or_else(|| TrackPosition::new(track_model::EdgeId(0), 0.0, 1))
    }

    /// Position of one end of the consist, walked along the graph rather than along the
    /// current edge alone — a train standing over a joint has one end on each edge.
    ///
    /// The direction of the returned position is always the train's own direction of
    /// travel, so at the head it points away from the consist and at the tail into it.
    /// `None` for an empty consist, or where the walk runs into something it cannot pass.
    pub fn end_position(
        &self,
        net: &track_model::TrackNetwork,
        end: ConsistEnd,
    ) -> Option<TrackPosition> {
        match end {
            ConsistEnd::Head => {
                let front = self.vehicles.first()?;
                front.pos.offset_by(net, front.spec.length / 2.0)
            }
            ConsistEnd::Tail => {
                let rear = self.vehicles.last()?;
                rear.pos.offset_by(net, -rear.spec.length / 2.0)
            }
        }
    }

    /// Braked weight percentage of the train: sum of braked weights / sum of masses · 100.
    ///
    /// Load braking counts: a wagon that carries nothing brings the braked weight of the
    /// empty position into the brake sheet, not the anscribed loaded one.
    pub fn brake_percentage(&self) -> f64 {
        // Per vehicle in the position its changeover handle actually stands in: a train
        // in G is anscribed with G's braked weights, and that is the figure the brake
        // sheet — and the LZB braking curve behind it — is worked out from.
        let weight: f64 = self
            .vehicles
            .iter()
            .map(|v| {
                let position = v.brake.effective_position(&v.spec.brake);
                v.spec.brake.brake_weight_at_position(position) * v.spec.load_share(v.mass())
            })
            .sum();
        let mass: f64 = self.vehicles.iter().map(|v| v.mass() / 1000.0).sum();
        if mass <= 0.0 {
            0.0
        } else {
            weight / mass * 100.0
        }
    }

    /// Shifts the whole consist `d` metres along the track, speeds untouched.
    ///
    /// This is how a network correction is worked off (`app/src/net.rs`): a train that is a
    /// few centimetres behind runs a hair faster for a moment instead of being set to the
    /// server's position, which the eye would read as a jump. Trackside devices passed on
    /// the way are deliberately dropped — a correction must not trip a PZB magnet twice.
    pub fn nudge(&mut self, net: &track_model::TrackNetwork, d: f64) {
        let mut scratch = Vec::new();
        for vehicle in &mut self.vehicles {
            let _ = vehicle.pos.advance(net, d, &mut scratch);
            scratch.clear();
            vehicle.x += d;
        }
    }

    /// Places the consist with its head at `head`, lining the vehicles up behind it.
    ///
    /// The hard resync of the network layer, for the cases a nudge cannot cover: a client
    /// joining, a switch taken the other way, a stall long enough to lose the train.
    pub fn place_head_at(&mut self, head: TrackPosition, net: &track_model::TrackNetwork) {
        // `x` keeps running where it was: the couplers read differences of it, and a jump
        // in one of them would be a shove through the whole train.
        let head_x = self
            .vehicles
            .first()
            .map_or(0.0, |v| v.x + v.spec.length / 2.0);
        let mut offset = 0.0;
        let mut scratch = Vec::new();
        for vehicle in &mut self.vehicles {
            let half = vehicle.spec.length / 2.0;
            offset -= half;
            let mut p = head;
            let _ = p.advance(net, offset, &mut scratch);
            scratch.clear();
            vehicle.pos = p;
            vehicle.x = head_x + offset;
            offset -= half;
        }
    }

    /// Filling time of the slowest brake in the train [s] — the brake position (BRA) of the
    /// brake sheet, in the form the braking curve needs it: a train is only braked through
    /// when its last vehicle is, so a single wagon in G decides for the whole train.
    pub fn brake_apply_time(&self) -> f64 {
        self.vehicles
            .iter()
            .map(|v| {
                v.spec
                    .brake
                    .apply_time_at_position(v.brake.effective_position(&v.spec.brake))
            })
            .fold(0.0, f64::max)
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

    /// A vehicle that states no metadata, no variant and no load must write the same file
    /// as before the three were introduced — an existing `.ron` may not grow empty blocks
    /// just from being opened and saved.
    #[test]
    fn empty_metadata_variants_and_loads_do_not_reach_the_file() {
        let text = ron::ser::to_string(&VehicleSpec::default()).unwrap();
        for field in ["meta:", "variants:", "loads:"] {
            assert!(!text.contains(field), "{field} written although empty");
        }
        // A vehicle that does state them writes them, and reads them back unchanged.
        let spec = VehicleSpec {
            meta: VehicleMeta {
                class: "BR 101".into(),
                build_year: 1996,
                ..VehicleMeta::default()
            },
            loads: vec![LoadSpec {
                name: "Kohle".into(),
                mass: 25_000.0,
                node: Some("load_coal".into()),
            }],
            ..VehicleSpec::default()
        };
        let round: VehicleSpec = ron::from_str(&ron::ser::to_string(&spec).unwrap()).unwrap();
        assert_eq!(round, spec);
    }

    /// The load is capped by the anscribed payload; a vehicle without one carries it whole.
    #[test]
    fn mass_with_load_respects_the_maximum_payload() {
        let mut spec = VehicleSpec {
            mass_empty: 20_000.0,
            max_payload: 25_000.0,
            loads: vec![
                LoadSpec {
                    name: "Kohle".into(),
                    mass: 25_000.0,
                    node: None,
                },
                LoadSpec {
                    name: "Erz".into(),
                    mass: 40_000.0,
                    node: None,
                },
            ],
            ..VehicleSpec::default()
        };
        assert_eq!(spec.mass_with_load(None), 20_000.0);
        assert_eq!(spec.mass_with_load(Some(0)), 45_000.0);
        // Overloaded: the wagon takes what it is anscribed for, not what the load weighs.
        assert_eq!(spec.mass_with_load(Some(1)), 45_000.0);
        // An index that does not exist is an empty wagon, not a panic.
        assert_eq!(spec.mass_with_load(Some(9)), 20_000.0);
        // 0 = not stated, so nothing caps the load.
        spec.max_payload = 0.0;
        assert_eq!(spec.mass_with_load(Some(1)), 60_000.0);
    }

    /// Every client builds the consist from the same seed, so it has to pick the same
    /// running number — otherwise the same wagon is numbered differently on every screen.
    #[test]
    fn running_number_is_reproducible_from_the_seed() {
        let variant = VehicleVariant {
            name: "verkehrsrot".into(),
            numbers: vec!["101 001-6".into(), "101 002-4".into(), "101 003-2".into()],
            ..VehicleVariant::default()
        };
        for seed in 0..50u64 {
            let picked = variant.number(seed).unwrap();
            assert_eq!(variant.number(seed), Some(picked));
            assert!(variant.numbers.iter().any(|n| n == picked));
        }
        // Different seeds must not all land on the same number.
        let spread: std::collections::BTreeSet<_> =
            (0..50u64).filter_map(|s| variant.number(s)).collect();
        assert!(spread.len() > 1);
        // A variant without numbers has none — the caller falls back to the vehicle name.
        assert_eq!(VehicleVariant::default().number(1), None);
    }

    /// A variant is a dress, not a vehicle: without a model of its own it shows the base
    /// one.
    #[test]
    fn variant_without_a_model_falls_back_to_the_base_model() {
        let spec = VehicleSpec {
            model: Some(VehicleModel {
                file: "example/assets/br101.gltf".into(),
                ..VehicleModel::default()
            }),
            variants: vec![
                VehicleVariant {
                    name: "verkehrsrot".into(),
                    ..VehicleVariant::default()
                },
                VehicleVariant {
                    name: "orientrot".into(),
                    model: Some("example/assets/br101_orientrot.gltf".into()),
                    ..VehicleVariant::default()
                },
            ],
            ..VehicleSpec::default()
        };
        assert_eq!(spec.model_file(None), Some("example/assets/br101.gltf"));
        assert_eq!(spec.model_file(Some(0)), Some("example/assets/br101.gltf"));
        assert_eq!(
            spec.model_file(Some(1)),
            Some("example/assets/br101_orientrot.gltf")
        );
        // An index out of range is the base vehicle, not a missing model.
        assert_eq!(spec.model_file(Some(9)), Some("example/assets/br101.gltf"));
        assert_eq!(spec.variant(1).map(|v| v.name.as_str()), Some("orientrot"));
        assert!(spec.variant(9).is_none());
    }
}
