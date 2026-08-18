//! Building-block graph of a vehicle: the vehicle as connected components on a node
//! canvas — engines, valves and reservoirs wired together instead of filled into forms.
//!
//! The graph is the *authoring* model of the vehicle editor. [`bake`] compiles it into
//! the runtime spec fields ([`TractionSpec`], [`BrakeSpec`], safety, doors, …), so the
//! 200 Hz simulation keeps its hardwired, fast update path — the graph costs nothing at
//! run time. [`from_spec`] is the reverse direction: it synthesises a graph from a spec,
//! so every existing vehicle opens as a block diagram.
//!
//! Mods extend the palette with presets: a RON file under `mods/<id>/blocks/` names a
//! built-in block as `base` and overrides parameter defaults (a "Voith L 620" is a
//! `hydro-transmission` with the L 620 figures). Behaviour beyond the built-in physics
//! goes through the `script` block, which is the existing Lua hook.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::brakes::{
    BrakeKind, BrakeMedium, BrakePosition, BrakeSpec, ControlValve, EpBrake, LoadBraking,
    SlipProtection,
};
use crate::doors::DoorSystem;
use crate::drive::{
    AsyncMotor, Circuit, DieselElectric, DieselEngine, DriveSpec, DynamicBrake, ElectricMotor,
    Governor, HydrodynamicBrake, HydrostaticDrive, MechanicalGearbox, MotorGroup, SeriesMotor,
    Starter, Thermal, TractionSpec, Transmission,
};
use crate::electric::{PowerSupply, SupplySystem};
use crate::safety::SafetyEquipment;
use crate::safety::de::{PzbVariant, SifaKind, TrainType};
use crate::signal::{Combine, SignalInput, SignalOp, SignalProgram, SignalSink};
use crate::train::{AxleSpec, VehicleSpec};

/// What flows through a port. The editor colours pins and wires by this, and a wire may
/// only join two ports of the same domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PortDomain {
    /// Rotating shaft: torque and speed.
    Mechanical,
    /// Linear force at the wheel or on the rail [N].
    Force,
    /// Electrical supply or traction power.
    Electrical,
    /// Compressed air.
    Pneumatic,
    /// Control value 0…1.
    Signal,
    /// Fuel flow — diesel oil, or coal on a steam locomotive.
    Fuel,
    /// Live steam.
    Steam,
    /// Feed water.
    Water,
    /// Heat: what a fire gives a boiler and what a resistor gives a cooling system.
    Heat,
}

impl PortDomain {
    /// i18n key of the domain name (legend, tooltips).
    pub fn key(self) -> &'static str {
        match self {
            PortDomain::Mechanical => "domain-mech",
            PortDomain::Force => "domain-force",
            PortDomain::Electrical => "domain-elec",
            PortDomain::Pneumatic => "domain-air",
            PortDomain::Signal => "domain-signal",
            PortDomain::Fuel => "domain-fuel",
            PortDomain::Steam => "domain-steam",
            PortDomain::Water => "domain-water",
            PortDomain::Heat => "domain-heat",
        }
    }

    pub const ALL: [PortDomain; 9] = [
        PortDomain::Mechanical,
        PortDomain::Force,
        PortDomain::Electrical,
        PortDomain::Pneumatic,
        PortDomain::Signal,
        PortDomain::Fuel,
        PortDomain::Steam,
        PortDomain::Water,
        PortDomain::Heat,
    ];
}

/// Value of one block parameter, as stored in the vehicle file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ParamValue {
    Number(f64),
    Bool(bool),
    /// Choice out of [`ParamKind::Choice`], by stable kebab-case id.
    Choice(String),
    Text(String),
    /// (x, y) lookup table.
    Curve(Vec<(f64, f64)>),
    /// Plain list of numbers (field weakening steps).
    List(Vec<f64>),
    /// The circuits of a hydraulic transmission — one complex parameter instead of a
    /// block per circuit, mirroring [`Transmission::circuits`].
    Circuits(Vec<Circuit>),
}

impl ParamValue {
    pub fn number(&self) -> f64 {
        match self {
            ParamValue::Number(v) => *v,
            _ => 0.0,
        }
    }

    pub fn flag(&self) -> bool {
        matches!(self, ParamValue::Bool(true))
    }

    pub fn choice(&self) -> &str {
        match self {
            ParamValue::Choice(v) => v,
            _ => "",
        }
    }

    pub fn text(&self) -> &str {
        match self {
            ParamValue::Text(v) => v,
            _ => "",
        }
    }

    pub fn curve(&self) -> &[(f64, f64)] {
        match self {
            ParamValue::Curve(v) => v,
            _ => &[],
        }
    }

    pub fn list(&self) -> &[f64] {
        match self {
            ParamValue::List(v) => v,
            _ => &[],
        }
    }

    pub fn circuits(&self) -> &[Circuit] {
        match self {
            ParamValue::Circuits(v) => v,
            _ => &[],
        }
    }
}

/// How the editor renders a parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ParamKind {
    Number {
        min: f64,
        max: f64,
        speed: f64,
        /// Suffix on the drag value ("N", "bar"); empty for dimensionless.
        unit: String,
    },
    Bool,
    /// Stable option ids. Options that are type designations (`KE-GPR`) are shown
    /// literally; prose options get a label key `<param key>-<option>`.
    Choice(Vec<String>),
    Text,
    /// Units label the axes of the curve editor.
    Curve {
        x_unit: String,
        y_unit: String,
    },
    List,
    Circuits,
}

/// One parameter of a block definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParamDef {
    /// Stable id, the key in [`GraphBlock::params`].
    pub id: String,
    /// i18n key of the label; `<key>-hint` is the tooltip.
    pub key: String,
    pub kind: ParamKind,
    pub default: ParamValue,
}

/// One input or output of a block definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortDef {
    /// Stable id, referenced by [`GraphWire`].
    pub id: String,
    /// i18n key of the pin label.
    pub key: String,
    pub domain: PortDomain,
}

/// Palette group of a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum BlockCategory {
    Energy,
    Drivetrain,
    Electric,
    Steam,
    Brake,
    RunningGear,
    Control,
    Logic,
    Equipment,
}

impl BlockCategory {
    pub const ALL: [BlockCategory; 9] = [
        BlockCategory::Energy,
        BlockCategory::Drivetrain,
        BlockCategory::Electric,
        BlockCategory::Steam,
        BlockCategory::Brake,
        BlockCategory::RunningGear,
        BlockCategory::Control,
        BlockCategory::Logic,
        BlockCategory::Equipment,
    ];

    pub fn key(self) -> &'static str {
        match self {
            BlockCategory::Energy => "blkcat-energy",
            BlockCategory::Drivetrain => "blkcat-drivetrain",
            BlockCategory::Electric => "blkcat-electric",
            BlockCategory::Steam => "blkcat-steam",
            BlockCategory::Brake => "blkcat-brake",
            BlockCategory::RunningGear => "blkcat-running-gear",
            BlockCategory::Control => "blkcat-control",
            BlockCategory::Logic => "blkcat-logic",
            BlockCategory::Equipment => "blkcat-equipment",
        }
    }
}

/// Definition of a block type — built in, or a mod preset of a built-in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlockDef {
    /// `diesel-engine` built in, `<mod>:<id>` for a mod preset.
    pub id: String,
    pub category: BlockCategory,
    /// i18n key of the name (`blk-<id>`); empty for mod blocks, which carry `name`.
    pub name_key: String,
    /// Literal display name of a mod block.
    #[serde(default)]
    pub name: String,
    /// Literal description of a mod block; built-ins use `blk-<id>-hint`.
    #[serde(default)]
    pub description: String,
    pub inputs: Vec<PortDef>,
    pub outputs: Vec<PortDef>,
    pub params: Vec<ParamDef>,
    /// Built-in block this def is a preset of. Baking treats the block as its base.
    #[serde(default)]
    pub base: Option<String>,
    /// May appear more than once in one graph. A vehicle has one control valve but four
    /// axles, two angle cocks and as many PID controllers as its builder wants.
    #[serde(default)]
    pub repeatable: bool,
}

impl BlockDef {
    pub fn param(&self, id: &str) -> Option<&ParamDef> {
        self.params.iter().find(|p| p.id == id)
    }

    pub fn port(&self, id: &str) -> Option<&PortDef> {
        self.inputs
            .iter()
            .chain(self.outputs.iter())
            .find(|p| p.id == id)
    }
}

/// One placed block in a vehicle graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphBlock {
    /// Stable id within the graph, referenced by wires.
    pub id: u32,
    /// [`BlockDef::id`].
    pub kind: String,
    /// Canvas position.
    pub pos: (f32, f32),
    /// Parameter values; a missing entry falls back to the definition's default.
    #[serde(default)]
    pub params: BTreeMap<String, ParamValue>,
}

/// One wire between an output and an input port.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphWire {
    pub from: u32,
    pub from_port: String,
    pub to: u32,
    pub to_port: String,
}

/// Comment frame on the canvas: a coloured background box that groups blocks
/// visually, in the manner of a blueprint comment. Baking ignores it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphGroup {
    pub id: u32,
    pub title: String,
    /// Background colour (RGB); drawn translucent.
    pub color: [u8; 3],
    pub pos: (f32, f32),
    pub size: (f32, f32),
}

/// The block diagram of a vehicle, stored inside [`VehicleSpec`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct VehicleGraph {
    pub blocks: Vec<GraphBlock>,
    pub wires: Vec<GraphWire>,
    #[serde(default)]
    pub groups: Vec<GraphGroup>,
}

impl VehicleGraph {
    pub fn next_id(&self) -> u32 {
        self.blocks.iter().map(|b| b.id + 1).max().unwrap_or(0)
    }

    pub fn next_group_id(&self) -> u32 {
        self.groups.iter().map(|g| g.id + 1).max().unwrap_or(0)
    }

    pub fn block(&self, id: u32) -> Option<&GraphBlock> {
        self.blocks.iter().find(|b| b.id == id)
    }

    /// Removes a block and every wire touching it.
    pub fn remove_block(&mut self, id: u32) {
        self.blocks.retain(|b| b.id != id);
        self.wires.retain(|w| w.from != id && w.to != id);
    }
}

/// All block definitions known to editor and loader.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    pub defs: Vec<BlockDef>,
}

/// Mod-facing block preset (`mods/<id>/blocks/*.ron`).
#[derive(Debug, Clone, Deserialize)]
pub struct ModBlockDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Built-in block the preset specialises.
    pub base: String,
    /// Overridden parameter defaults.
    #[serde(default)]
    pub params: BTreeMap<String, ParamValue>,
}

/// Parses one mod block file.
pub fn parse_mod_block(source: &str) -> Result<ModBlockDef, ron::error::SpannedError> {
    ron::from_str(source)
}

impl Registry {
    pub fn get(&self, id: &str) -> Option<&BlockDef> {
        self.defs.iter().find(|d| d.id == id)
    }

    /// Built-in kind a block bakes as: a preset resolves to its base.
    pub fn base_kind<'a>(&'a self, kind: &'a str) -> Option<&'a str> {
        let def = self.get(kind)?;
        match &def.base {
            Some(base) => Some(base.as_str()),
            None => Some(def.id.as_str()),
        }
    }

    /// Default value of a parameter, following a preset's base for unknown ids.
    pub fn default_of(&self, kind: &str, param: &str) -> Option<ParamValue> {
        self.get(kind)?.param(param).map(|p| p.default.clone())
    }

    /// Creates a block of `kind` with all defaults filled in.
    pub fn instantiate(&self, kind: &str, id: u32, pos: (f32, f32)) -> Option<GraphBlock> {
        let def = self.get(kind)?;
        let params = def
            .params
            .iter()
            .map(|p| (p.id.clone(), p.default.clone()))
            .collect();
        Some(GraphBlock {
            id,
            kind: kind.to_string(),
            pos,
            params,
        })
    }

    /// Registers a mod preset. The ports and parameters come from the base; `params`
    /// overrides defaults. Unknown base or parameter ids are reported, not fatal.
    pub fn add_mod_block(&mut self, mod_id: &str, def: ModBlockDef) -> Result<(), String> {
        let Some(base) = self.get(&def.base).cloned() else {
            return Err(format!("unknown base block '{}'", def.base));
        };
        if base.base.is_some() {
            return Err(format!("base '{}' is itself a preset", def.base));
        }
        let mut block = base.clone();
        block.id = format!("{mod_id}:{}", def.id);
        block.name_key = String::new();
        block.name = def.name;
        block.description = def.description;
        block.base = Some(base.id);
        for (id, value) in def.params {
            let Some(param) = block.params.iter_mut().find(|p| p.id == id) else {
                return Err(format!("unknown parameter '{id}'"));
            };
            if std::mem::discriminant(&param.default) != std::mem::discriminant(&value) {
                return Err(format!("parameter '{id}' has the wrong type"));
            }
            param.default = value;
        }
        self.defs.push(block);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Built-in palette
// ---------------------------------------------------------------------------

fn num(id: &str, key: &str, unit: &str, min: f64, max: f64, speed: f64, default: f64) -> ParamDef {
    ParamDef {
        id: id.to_string(),
        key: key.to_string(),
        kind: ParamKind::Number {
            min,
            max,
            speed,
            unit: unit.to_string(),
        },
        default: ParamValue::Number(default),
    }
}

fn flag(id: &str, key: &str, default: bool) -> ParamDef {
    ParamDef {
        id: id.to_string(),
        key: key.to_string(),
        kind: ParamKind::Bool,
        default: ParamValue::Bool(default),
    }
}

fn choice(id: &str, key: &str, options: &[&str], default: &str) -> ParamDef {
    ParamDef {
        id: id.to_string(),
        key: key.to_string(),
        kind: ParamKind::Choice(options.iter().map(|o| o.to_string()).collect()),
        default: ParamValue::Choice(default.to_string()),
    }
}

fn curve(id: &str, key: &str, x_unit: &str, y_unit: &str, default: Vec<(f64, f64)>) -> ParamDef {
    ParamDef {
        id: id.to_string(),
        key: key.to_string(),
        kind: ParamKind::Curve {
            x_unit: x_unit.to_string(),
            y_unit: y_unit.to_string(),
        },
        default: ParamValue::Curve(default),
    }
}

fn text(id: &str, key: &str) -> ParamDef {
    ParamDef {
        id: id.to_string(),
        key: key.to_string(),
        kind: ParamKind::Text,
        default: ParamValue::Text(String::new()),
    }
}

fn list(id: &str, key: &str, default: Vec<f64>) -> ParamDef {
    ParamDef {
        id: id.to_string(),
        key: key.to_string(),
        kind: ParamKind::List,
        default: ParamValue::List(default),
    }
}

fn port(id: &str, key: &str, domain: PortDomain) -> PortDef {
    PortDef {
        id: id.to_string(),
        key: key.to_string(),
        domain,
    }
}

fn def(
    id: &str,
    category: BlockCategory,
    inputs: Vec<PortDef>,
    outputs: Vec<PortDef>,
    params: Vec<ParamDef>,
) -> BlockDef {
    BlockDef {
        id: id.to_string(),
        category,
        name_key: format!("blk-{id}"),
        name: String::new(),
        description: String::new(),
        inputs,
        outputs,
        params,
        base: None,
        repeatable: false,
    }
}

/// The same for a block a vehicle may carry more than one of.
fn many(
    id: &str,
    category: BlockCategory,
    inputs: Vec<PortDef>,
    outputs: Vec<PortDef>,
    params: Vec<ParamDef>,
) -> BlockDef {
    BlockDef {
        repeatable: true,
        ..def(id, category, inputs, outputs, params)
    }
}

impl Registry {
    /// The built-in palette. Every physical component of the simulation exists here as a
    /// block; baking maps presence and parameters back onto the runtime spec.
    pub fn builtin() -> Self {
        use BlockCategory::*;
        use PortDomain::*;
        let shaft_in = || port("shaft", "port-shaft", Mechanical);
        let shaft_out = || port("out", "port-shaft", Mechanical);
        let elec_in = || port("elec", "port-elec", Electrical);
        let elec_out = || port("out", "port-elec", Electrical);
        let air_in = || port("air", "port-air", Pneumatic);
        let air_out = || port("out", "port-air", Pneumatic);
        let force_out = || port("force", "port-force", Force);
        let ctrl_in = || port("ctrl", "port-ctrl", Signal);

        let steam_out = || port("steam", "port-steam", PortDomain::Steam);
        let water_out = || port("water", "port-water", Water);
        let heat_out = || port("heat", "port-heat", Heat);
        let sig_in = || port("in", "port-value", Signal);
        let sig_out = || port("out", "port-value", Signal);

        let defs = vec![
            // --- Energy -----------------------------------------------------
            def(
                "battery",
                Energy,
                vec![],
                vec![elec_out()],
                vec![
                    num("voltage", "bat-voltage", "V", 12.0, 220.0, 1.0, 110.0),
                    num("capacity", "bat-capacity", "Ah", 1.0, 2000.0, 5.0, 250.0),
                ],
            ),
            def(
                "fuel-tank",
                Energy,
                vec![],
                vec![port("out", "port-fuel", Fuel)],
                vec![num(
                    "capacity",
                    "drv-fuel-capacity",
                    "l",
                    0.0,
                    20_000.0,
                    10.0,
                    3000.0,
                )],
            ),
            many(
                "pantograph",
                Energy,
                vec![],
                vec![elec_out()],
                vec![
                    choice(
                        "system",
                        "pan-system",
                        &["ac-15kv", "ac-25kv", "dc-3kv", "dc-1.5kv", "third-rail"],
                        "ac-15kv",
                    ),
                    num("rise_time", "pan-rise-time", "s", 0.5, 30.0, 0.1, 5.0),
                ],
            ),
            def(
                "voltage-source",
                Energy,
                vec![],
                vec![elec_out()],
                vec![num(
                    "voltage",
                    "src-voltage",
                    "V",
                    0.0,
                    30_000.0,
                    100.0,
                    15_000.0,
                )],
            ),
            def(
                "diesel-engine",
                Energy,
                vec![port("fuel", "port-fuel", Fuel), ctrl_in()],
                vec![shaft_out()],
                vec![
                    num(
                        "max_force",
                        "drv-start-force-diesel",
                        "N",
                        0.0,
                        2.0e6,
                        500.0,
                        235_000.0,
                    ),
                    num(
                        "max_power",
                        "drv-power",
                        "W",
                        0.0,
                        20.0e6,
                        5000.0,
                        900_000.0,
                    ),
                    num("v_max", "drv-vmax", "km/h", 0.0, 500.0, 1.0, 140.0),
                    num("ramp_time", "drv-ramp", "s", 0.1, 60.0, 0.1, 8.0),
                    num("start_time", "drv-crank-time", "s", 0.0, 60.0, 0.1, 8.0),
                    flag("engine_map", "eng-map", true),
                    num("idle_rpm", "eng-idle", "1/min", 0.0, 3000.0, 5.0, 650.0),
                    num("rated_rpm", "eng-rated", "1/min", 0.0, 3000.0, 5.0, 1500.0),
                    num(
                        "max_rpm",
                        "eng-overspeed",
                        "1/min",
                        0.0,
                        3500.0,
                        5.0,
                        1600.0,
                    ),
                    curve(
                        "torque_curve",
                        "eng-torque-curve",
                        "1/min",
                        "N·m",
                        vec![(650.0, 5200.0), (1500.0, 5730.0)],
                    ),
                    choice("governor", "eng-governor", &["speed", "fill"], "speed"),
                    num("governor_steps", "eng-notches", "", 0.0, 40.0, 1.0, 15.0),
                    num("governor_droop", "eng-droop", "", 0.0, 0.2, 0.001, 0.03),
                    num("inertia", "eng-inertia", "kg·m²", 1.0, 500.0, 1.0, 30.0),
                    num("response_time", "eng-rack-time", "s", 0.05, 20.0, 0.1, 1.5),
                ],
            ),
            // --- Drivetrain -------------------------------------------------
            def(
                "hydro-transmission",
                Drivetrain,
                vec![shaft_in()],
                vec![shaft_out()],
                vec![
                    ParamDef {
                        id: "circuits".to_string(),
                        key: "group-circuits".to_string(),
                        kind: ParamKind::Circuits,
                        default: ParamValue::Circuits(vec![]),
                    },
                    choice(
                        "power_control",
                        "trm-power-control",
                        &["filling", "engine-speed"],
                        "filling",
                    ),
                    num("fill_steps", "trm-fill-steps", "", 0.0, 40.0, 1.0, 0.0),
                    num("fill_time", "trm-fill-time", "s", 0.05, 10.0, 0.05, 1.2),
                    num("drain_time", "trm-drain-time", "s", 0.0, 10.0, 0.05, 0.0),
                    num(
                        "hysteresis_kmh",
                        "trm-hysteresis",
                        "km/h",
                        0.0,
                        30.0,
                        0.5,
                        8.0,
                    ),
                    num("final_ratio", "trm-final-ratio", "", 0.1, 20.0, 0.01, 1.9),
                    num(
                        "shunting_ratio",
                        "trm-shunting-ratio",
                        "",
                        0.0,
                        20.0,
                        0.01,
                        0.0,
                    ),
                    num(
                        "wheel_diameter",
                        "drv-wheel-diameter",
                        "m",
                        0.3,
                        2.0,
                        0.01,
                        1.0,
                    ),
                    num("count", "trm-count", "", 1.0, 8.0, 1.0, 1.0),
                    num("efficiency", "trm-efficiency", "", 0.5, 1.0, 0.01, 0.96),
                ],
            ),
            def(
                "mechanical-gearbox",
                Drivetrain,
                vec![shaft_in()],
                vec![shaft_out()],
                vec![
                    list("gears", "gbx-gears", vec![5.5, 3.0, 1.8, 1.0]),
                    num("final_ratio", "trm-final-ratio", "", 0.1, 20.0, 0.01, 3.0),
                    num(
                        "wheel_diameter",
                        "drv-wheel-diameter",
                        "m",
                        0.3,
                        2.0,
                        0.01,
                        0.9,
                    ),
                    num("efficiency", "trm-efficiency", "", 0.5, 1.0, 0.01, 0.95),
                    num(
                        "clutch_torque",
                        "gbx-clutch-torque",
                        "N·m",
                        0.0,
                        20_000.0,
                        10.0,
                        1_200.0,
                    ),
                    num("clutch_time", "gbx-clutch-time", "s", 0.05, 5.0, 0.05, 1.0),
                    num("shift_time", "gbx-shift-time", "s", 0.0, 5.0, 0.05, 1.5),
                    num(
                        "shift_up_rpm",
                        "gbx-shift-up",
                        "1/min",
                        0.0,
                        4000.0,
                        10.0,
                        1_800.0,
                    ),
                    num(
                        "shift_down_rpm",
                        "gbx-shift-down",
                        "1/min",
                        0.0,
                        4000.0,
                        10.0,
                        900.0,
                    ),
                ],
            ),
            def(
                "hydrostatic-drive",
                Drivetrain,
                vec![shaft_in()],
                vec![shaft_out()],
                vec![
                    num(
                        "max_force",
                        "hst-max-force",
                        "N",
                        0.0,
                        400_000.0,
                        500.0,
                        60_000.0,
                    ),
                    num("efficiency", "trm-efficiency", "", 0.3, 1.0, 0.01, 0.8),
                    num(
                        "response_time",
                        "hst-response-time",
                        "s",
                        0.05,
                        10.0,
                        0.05,
                        1.5,
                    ),
                ],
            ),
            def(
                "retarder",
                Drivetrain,
                vec![shaft_in()],
                vec![],
                vec![
                    num("absorption", "ret-absorption", "", 0.0, 10.0, 0.01, 0.4),
                    num("ratio", "ret-ratio", "", 0.1, 10.0, 0.01, 2.0),
                    num(
                        "wheel_diameter",
                        "drv-wheel-diameter",
                        "m",
                        0.3,
                        2.0,
                        0.01,
                        1.0,
                    ),
                    num(
                        "max_force",
                        "ret-brake-force",
                        "N",
                        0.0,
                        500_000.0,
                        500.0,
                        80_000.0,
                    ),
                    num(
                        "max_power",
                        "ret-brake-power",
                        "W",
                        0.0,
                        5.0e6,
                        5000.0,
                        1.0e6,
                    ),
                    num("fill_time", "ret-fill-time", "s", 0.05, 10.0, 0.05, 1.5),
                    num("fade_out_kmh", "drv-fade", "km/h", 0.0, 100.0, 1.0, 10.0),
                ],
            ),
            def(
                "generator",
                Drivetrain,
                vec![shaft_in()],
                vec![elec_out()],
                vec![
                    num("power", "gen-power", "W", 0.0, 20.0e6, 5000.0, 1.8e6),
                    num("efficiency", "gen-efficiency", "", 0.5, 1.0, 0.01, 0.94),
                    num(
                        "max_voltage",
                        "gen-max-voltage",
                        "V",
                        50.0,
                        5000.0,
                        10.0,
                        1200.0,
                    ),
                    num(
                        "max_current",
                        "gen-max-current",
                        "A",
                        10.0,
                        20_000.0,
                        10.0,
                        4000.0,
                    ),
                ],
            ),
            def(
                "rectifier",
                Electric,
                vec![elec_in()],
                vec![elec_out()],
                vec![num(
                    "efficiency",
                    "rec-efficiency",
                    "",
                    0.5,
                    1.0,
                    0.005,
                    0.98,
                )],
            ),
            def(
                "load-regulator",
                Electric,
                vec![ctrl_in()],
                vec![port("out", "port-excitation", Signal)],
                vec![
                    num("response_time", "reg-time", "s", 0.1, 30.0, 0.1, 3.0),
                    num("blower_idle", "reg-blower-idle", "", 0.0, 1.0, 0.01, 0.2),
                ],
            ),
            def(
                "traction-motor",
                Drivetrain,
                vec![elec_in()],
                vec![shaft_out()],
                vec![],
            ),
            def(
                "async-motor",
                Drivetrain,
                vec![elec_in()],
                vec![shaft_out(), heat_out()],
                vec![
                    num("count", "mot-count", "", 1.0, 16.0, 1.0, 4.0),
                    num("pole_pairs", "mot-pole-pairs", "", 1.0, 8.0, 1.0, 2.0),
                    num(
                        "rated_torque",
                        "mot-rated-torque",
                        "N·m",
                        100.0,
                        50_000.0,
                        50.0,
                        5800.0,
                    ),
                    num(
                        "pullout_ratio",
                        "mot-pullout-ratio",
                        "",
                        1.2,
                        4.0,
                        0.05,
                        2.6,
                    ),
                    num(
                        "pullout_slip",
                        "mot-pullout-slip",
                        "",
                        0.02,
                        0.5,
                        0.005,
                        0.14,
                    ),
                    num(
                        "rated_frequency",
                        "mot-rated-freq",
                        "Hz",
                        5.0,
                        200.0,
                        1.0,
                        60.0,
                    ),
                    num(
                        "max_frequency",
                        "mot-max-freq",
                        "Hz",
                        10.0,
                        400.0,
                        1.0,
                        160.0,
                    ),
                    num("gear_ratio", "mot-gear-ratio", "", 0.5, 10.0, 0.01, 2.5),
                    num(
                        "wheel_diameter",
                        "drv-wheel-diameter",
                        "m",
                        0.3,
                        2.0,
                        0.01,
                        1.25,
                    ),
                    num("efficiency", "mot-efficiency", "", 0.5, 1.0, 0.01, 0.9),
                ],
            ),
            def(
                "rheostat",
                Electric,
                vec![elec_in()],
                vec![elec_out(), heat_out()],
                vec![
                    list(
                        "steps",
                        "rhe-steps",
                        vec![1.6, 1.1, 0.75, 0.5, 0.3, 0.15, 0.0],
                    ),
                    num("step_time", "rhe-step-time", "s", 0.05, 10.0, 0.05, 1.2),
                ],
            ),
            def(
                "series-parallel-switch",
                Electric,
                vec![elec_in(), ctrl_in()],
                vec![elec_out()],
                vec![choice(
                    "groups",
                    "spg-groups",
                    &["s-p", "s-sp-p", "s-only", "p-only"],
                    "s-p",
                )],
            ),
            def(
                "chopper",
                Electric,
                vec![elec_in(), ctrl_in()],
                vec![elec_out()],
                vec![num("response_time", "chp-time", "s", 0.02, 5.0, 0.01, 0.2)],
            ),
            many(
                "cooling",
                Electric,
                vec![port("heat", "port-heat", Heat), ctrl_in()],
                vec![],
                vec![
                    num(
                        "heat_capacity",
                        "cool-capacity",
                        "J/K",
                        1000.0,
                        2.0e6,
                        1000.0,
                        120_000.0,
                    ),
                    num("cooling", "cool-rate", "W/K", 10.0, 20_000.0, 10.0, 900.0),
                    num("natural_share", "cool-natural", "", 0.0, 1.0, 0.01, 0.15),
                    num("warn_temp", "cool-warn", "°C", 40.0, 600.0, 5.0, 250.0),
                    num("max_temp", "cool-max", "°C", 60.0, 900.0, 5.0, 400.0),
                    num("ambient", "cool-ambient", "°C", -40.0, 60.0, 1.0, 20.0),
                ],
            ),
            def(
                "series-motor",
                Drivetrain,
                vec![elec_in()],
                vec![shaft_out(), heat_out()],
                vec![
                    num("count", "mot-count", "", 1.0, 16.0, 1.0, 4.0),
                    num("resistance", "mot-resistance", "Ω", 0.001, 5.0, 0.001, 0.05),
                    num(
                        "flux_constant",
                        "mot-machine-constant",
                        "V·s/A",
                        0.001,
                        1.0,
                        0.001,
                        0.011,
                    ),
                    num(
                        "saturation_current",
                        "mot-saturation",
                        "A",
                        10.0,
                        5000.0,
                        5.0,
                        550.0,
                    ),
                    num(
                        "max_current",
                        "mot-max-current",
                        "A",
                        10.0,
                        5000.0,
                        5.0,
                        620.0,
                    ),
                    num(
                        "max_voltage",
                        "mot-max-voltage",
                        "V",
                        10.0,
                        5000.0,
                        5.0,
                        590.0,
                    ),
                    list("field_steps", "mot-field-steps", vec![1.0]),
                    num("gear_ratio", "mot-gear-ratio", "", 0.5, 10.0, 0.01, 3.17),
                    num(
                        "wheel_diameter",
                        "drv-wheel-diameter",
                        "m",
                        0.3,
                        2.0,
                        0.01,
                        1.25,
                    ),
                    num("efficiency", "mot-efficiency", "", 0.5, 1.0, 0.01, 0.9),
                ],
            ),
            // --- Electric ---------------------------------------------------
            def(
                "main-switch",
                Electric,
                vec![elec_in()],
                vec![elec_out()],
                vec![],
            ),
            def(
                "transformer",
                Electric,
                vec![elec_in()],
                vec![elec_out()],
                vec![],
            ),
            def(
                "tap-changer",
                Electric,
                vec![elec_in(), ctrl_in()],
                vec![elec_out()],
                vec![
                    num("steps", "tap-steps", "", 1.0, 60.0, 1.0, 28.0),
                    num(
                        "max_force",
                        "drv-start-force",
                        "N",
                        0.0,
                        2.0e6,
                        500.0,
                        275_000.0,
                    ),
                    num("max_power", "drv-power", "W", 0.0, 20.0e6, 5000.0, 3.7e6),
                    num("v_max", "drv-vmax", "km/h", 0.0, 500.0, 1.0, 150.0),
                    num("step_time", "tap-step-time", "s", 0.01, 5.0, 0.01, 0.6),
                ],
            ),
            def(
                "traction-converter",
                Electric,
                vec![elec_in(), ctrl_in()],
                vec![elec_out()],
                vec![
                    num(
                        "max_force",
                        "drv-start-force",
                        "N",
                        0.0,
                        2.0e6,
                        500.0,
                        300_000.0,
                    ),
                    num("max_power", "drv-power", "W", 0.0, 20.0e6, 5000.0, 6.4e6),
                    num("v_max", "drv-vmax", "km/h", 0.0, 500.0, 1.0, 220.0),
                    num("ramp_time", "drv-ramp", "s", 0.1, 60.0, 0.1, 2.5),
                    num("v_pullout", "drv-pullout", "km/h", 0.0, 500.0, 1.0, 0.0),
                ],
            ),
            def(
                "dynamic-brake",
                Electric,
                vec![elec_in()],
                vec![heat_out()],
                vec![
                    num(
                        "max_force",
                        "drv-brake-force",
                        "N",
                        0.0,
                        1.0e6,
                        500.0,
                        150_000.0,
                    ),
                    num(
                        "max_power",
                        "drv-brake-power",
                        "W",
                        0.0,
                        10.0e6,
                        5000.0,
                        4.0e6,
                    ),
                    num(
                        "fade_out_kmh",
                        "drv-brake-fade",
                        "km/h",
                        0.0,
                        100.0,
                        1.0,
                        5.0,
                    ),
                    flag("regenerative", "drv-regenerative", false),
                    num("ramp_time", "drv-ramp", "s", 0.1, 60.0, 0.1, 2.5),
                ],
            ),
            // --- Curve drive (simplified) ----------------------------------
            def(
                "traction-curve",
                Drivetrain,
                vec![ctrl_in()],
                vec![force_out()],
                vec![
                    curve(
                        "force",
                        "drv-force-plot",
                        "km/h",
                        "N",
                        vec![(0.0, 200_000.0), (80.0, 100_000.0), (160.0, 50_000.0)],
                    ),
                    curve("brake", "drv-brake-curve", "km/h", "N", vec![]),
                    num("v_max", "drv-vmax", "km/h", 0.0, 500.0, 1.0, 160.0),
                    num("ramp_time", "drv-ramp", "s", 0.1, 60.0, 0.1, 2.0),
                ],
            ),
            // --- Steam ------------------------------------------------------
            def(
                "boiler",
                BlockCategory::Steam,
                vec![
                    port("heat", "port-heat", Heat),
                    port("water", "port-water", Water),
                ],
                vec![steam_out()],
                vec![
                    num(
                        "water_space",
                        "stm-water-space",
                        "l",
                        100.0,
                        30_000.0,
                        50.0,
                        8800.0,
                    ),
                    num(
                        "steam_space",
                        "stm-steam-space",
                        "l",
                        100.0,
                        20_000.0,
                        50.0,
                        3200.0,
                    ),
                    num(
                        "working_pressure",
                        "stm-working-pressure",
                        "bar",
                        2.0,
                        30.0,
                        0.1,
                        16.0,
                    ),
                    num(
                        "safety_valve",
                        "stm-safety-valve",
                        "bar",
                        2.0,
                        32.0,
                        0.1,
                        16.5,
                    ),
                    num(
                        "heating_surface",
                        "stm-heating-surface",
                        "m²",
                        5.0,
                        500.0,
                        1.0,
                        177.0,
                    ),
                    flag("superheater", "stm-superheater", true),
                ],
            ),
            def(
                "firebox",
                BlockCategory::Steam,
                vec![port("fuel", "port-fuel", Fuel), ctrl_in()],
                vec![heat_out()],
                vec![
                    num("grate_area", "stm-grate-area", "m²", 0.3, 12.0, 0.05, 3.9),
                    num(
                        "grate_capacity",
                        "stm-grate-capacity",
                        "kg",
                        10.0,
                        1000.0,
                        5.0,
                        260.0,
                    ),
                    num(
                        "burn_rate",
                        "stm-burn-rate",
                        "kg/(m²·s)",
                        0.005,
                        0.2,
                        0.001,
                        0.055,
                    ),
                    num("blower_draught", "stm-blower", "", 0.0, 1.0, 0.01, 0.35),
                    num("shovel_mass", "stm-shovel", "kg", 1.0, 20.0, 0.5, 6.0),
                ],
            ),
            def(
                "steam-cylinders",
                BlockCategory::Steam,
                vec![port("steam", "port-steam", PortDomain::Steam), ctrl_in()],
                vec![force_out()],
                vec![
                    num("count", "stm-cylinders", "", 1.0, 4.0, 1.0, 2.0),
                    num("bore", "stm-bore", "m", 0.1, 1.0, 0.005, 0.6),
                    num("stroke", "stm-stroke", "m", 0.1, 1.2, 0.005, 0.66),
                    num(
                        "wheel_diameter",
                        "drv-wheel-diameter",
                        "m",
                        0.5,
                        2.5,
                        0.01,
                        1.4,
                    ),
                    num("max_cutoff", "stm-max-cutoff", "", 0.3, 0.95, 0.01, 0.75),
                    num(
                        "back_pressure",
                        "stm-back-pressure",
                        "bar",
                        1.0,
                        3.0,
                        0.05,
                        1.3,
                    ),
                    num("efficiency", "stm-efficiency", "", 0.5, 1.0, 0.01, 0.82),
                    num("v_max", "drv-vmax", "km/h", 10.0, 200.0, 1.0, 80.0),
                ],
            ),
            many(
                "injector",
                BlockCategory::Steam,
                vec![
                    port("steam", "port-steam", PortDomain::Steam),
                    port("water", "port-water", Water),
                    ctrl_in(),
                ],
                vec![port("out", "port-water", Water)],
                vec![num("rate", "stm-injector-rate", "l/s", 0.2, 20.0, 0.1, 3.2)],
            ),
            def(
                "tender",
                BlockCategory::Steam,
                vec![],
                vec![water_out(), port("coal", "port-fuel", Fuel)],
                vec![
                    num(
                        "water",
                        "stm-tender-water",
                        "l",
                        0.0,
                        60_000.0,
                        100.0,
                        30_000.0,
                    ),
                    num(
                        "coal",
                        "stm-tender-coal",
                        "kg",
                        0.0,
                        20_000.0,
                        50.0,
                        10_000.0,
                    ),
                ],
            ),
            // --- Air supply and brake --------------------------------------
            def(
                "compressor",
                Brake,
                vec![elec_in()],
                vec![air_out()],
                vec![
                    num(
                        "delivery",
                        "brk-compressor-delivery",
                        "l/min",
                        0.0,
                        10_000.0,
                        10.0,
                        2400.0,
                    ),
                    choice(
                        "kind",
                        "brk-pump-kind",
                        &["compressor", "exhauster"],
                        "compressor",
                    ),
                ],
            ),
            def(
                "main-reservoir",
                Brake,
                vec![air_in()],
                vec![air_out()],
                vec![num(
                    "volume",
                    "brk-main-volume",
                    "l",
                    0.0,
                    5000.0,
                    10.0,
                    1000.0,
                )],
            ),
            def(
                "driver-brake-valve",
                Brake,
                vec![port("supply", "port-supply", Pneumatic), ctrl_in()],
                vec![port("out", "port-brake-pipe", Pneumatic)],
                vec![flag("angleicher", "brk-angleicher", false)],
            ),
            def(
                "brake-pipe",
                Brake,
                vec![port("pipe", "port-brake-pipe", Pneumatic)],
                vec![port("out", "port-brake-pipe", Pneumatic)],
                vec![
                    num("volume", "brk-pipe-volume", "l", 1.0, 200.0, 1.0, 20.0),
                    num("leakage", "brk-leakage", "l/min", 0.0, 50.0, 0.1, 3.0),
                    choice("medium", "brk-medium", &["air", "vacuum"], "air"),
                ],
            ),
            many(
                "angle-cock",
                Brake,
                vec![port("pipe", "port-brake-pipe", Pneumatic)],
                vec![port("out", "port-brake-pipe", Pneumatic)],
                vec![choice("end", "brk-cock-end", &["front", "rear"], "rear")],
            ),
            many(
                "air-hose",
                Brake,
                vec![port("pipe", "port-brake-pipe", Pneumatic)],
                vec![port("out", "port-brake-pipe", Pneumatic)],
                vec![choice("end", "brk-cock-end", &["front", "rear"], "rear")],
            ),
            def(
                "emergency-valve",
                Brake,
                vec![port("pipe", "port-brake-pipe", Pneumatic), ctrl_in()],
                vec![],
                vec![],
            ),
            def(
                "limiting-valve",
                Brake,
                vec![air_in()],
                vec![air_out()],
                vec![num("limit", "brk-limit", "bar", 0.2, 10.0, 0.05, 2.0)],
            ),
            many(
                "double-check-valve",
                Brake,
                vec![
                    port("a", "port-inlet-a", Pneumatic),
                    port("b", "port-inlet-b", Pneumatic),
                ],
                vec![air_out()],
                vec![],
            ),
            def(
                "retainer-valve",
                Brake,
                vec![air_in()],
                vec![air_out()],
                vec![choice(
                    "position",
                    "brk-retainer",
                    &["off", "slow", "low", "high"],
                    "off",
                )],
            ),
            def(
                "ep-brake",
                Brake,
                vec![ctrl_in(), port("supply", "port-supply", Pneumatic)],
                vec![air_out()],
                vec![
                    num("apply_rate", "brk-ep-apply", "bar/s", 0.2, 10.0, 0.1, 2.5),
                    num(
                        "release_rate",
                        "brk-ep-release",
                        "bar/s",
                        0.2,
                        10.0,
                        0.1,
                        2.5,
                    ),
                    flag("vents_pipe", "brk-ep-vents-pipe", true),
                    num("steps", "brk-ep-steps", "", 0.0, 20.0, 1.0, 0.0),
                ],
            ),
            def(
                "control-valve",
                Brake,
                vec![
                    port("pipe", "port-brake-pipe", Pneumatic),
                    port("aux", "port-aux", Pneumatic),
                ],
                vec![air_out()],
                vec![
                    choice(
                        "valve",
                        "brk-valve",
                        &["k-gp", "ke-gp", "ke-gpr", "ke-tm", "ke-l2a", "ke-l2d"],
                        "ke-gp",
                    ),
                    choice("position", "brk-default-position", &["g", "p", "r"], "p"),
                    num("brake_weight", "brk-weight", "t", 0.0, 500.0, 1.0, 50.0),
                    choice(
                        "load_braking",
                        "brk-load",
                        &["none", "weighing", "changeover"],
                        "none",
                    ),
                    num("empty_share", "brk-load-empty", "", 0.0, 1.0, 0.01, 0.6),
                    num(
                        "changeover_mass",
                        "brk-load-mass",
                        "t",
                        0.0,
                        200.0,
                        0.5,
                        0.0,
                    ),
                ],
            ),
            def(
                "aux-reservoir",
                Brake,
                vec![],
                vec![air_out()],
                vec![num("volume", "brk-aux-volume", "l", 1.0, 500.0, 1.0, 100.0)],
            ),
            def(
                "relay-valve",
                Brake,
                vec![
                    port("pilot", "port-pilot", Pneumatic),
                    port("supply", "port-supply", Pneumatic),
                ],
                vec![air_out()],
                vec![flag("supplement", "brk-supplement", false)],
            ),
            def(
                "brake-cylinder",
                Brake,
                vec![air_in()],
                vec![force_out()],
                vec![
                    num("max_cylinder", "brk-cylinder", "bar", 0.5, 10.0, 0.05, 3.8),
                    num(
                        "cylinder_to_reservoir",
                        "brk-cyl-reservoir",
                        "",
                        0.05,
                        1.0,
                        0.01,
                        0.35,
                    ),
                ],
            ),
            def(
                "brake-rigging",
                Brake,
                vec![port("force", "port-force", Force)],
                vec![port("out", "port-force", Force)],
                vec![
                    choice(
                        "kind",
                        "brk-friction",
                        &[
                            "block",
                            "disc",
                            "composite-k",
                            "composite-ll",
                            "magnetic",
                            "custom",
                        ],
                        "block",
                    ),
                    curve("friction_curve", "brk-friction-points", "km/h", "µ", vec![]),
                    num("max_force", "brk-force", "N", 0.0, 2.0e6, 500.0, 60_000.0),
                ],
            ),
            def(
                "direct-brake",
                Brake,
                vec![port("supply", "port-supply", Pneumatic), ctrl_in()],
                vec![air_out()],
                vec![num(
                    "max_cylinder",
                    "brk-direct-cylinder",
                    "bar",
                    0.0,
                    10.0,
                    0.05,
                    0.0,
                )],
            ),
            def(
                "parking-brake",
                Brake,
                vec![air_in()],
                vec![force_out()],
                vec![
                    num("force", "brk-parking", "N", 0.0, 500_000.0, 500.0, 40_000.0),
                    flag("spring", "brk-spring", true),
                ],
            ),
            def(
                "mg-brake",
                Brake,
                vec![air_in()],
                vec![force_out()],
                vec![num(
                    "force",
                    "brk-mg-force",
                    "N",
                    0.0,
                    500_000.0,
                    500.0,
                    90_000.0,
                )],
            ),
            def(
                "wheel-slide-protection",
                Brake,
                vec![port("slip", "port-slip", Signal)],
                vec![],
                vec![choice(
                    "mode",
                    "brk-slip",
                    &["slip-brake", "traction-cutback", "creep-control"],
                    "traction-cutback",
                )],
            ),
            def(
                "sander",
                Brake,
                vec![air_in(), ctrl_in()],
                vec![],
                vec![num("rate", "brk-sand-rate", "kg/min", 0.0, 50.0, 0.1, 4.0)],
            ),
            // --- Running gear ----------------------------------------------
            def(
                "wheelset",
                RunningGear,
                vec![shaft_in(), port("force", "port-force", Force)],
                vec![port("slip", "port-slip", Signal)],
                vec![
                    num("axles", "veh-axles", "", 0.0, 32.0, 1.0, 4.0),
                    num(
                        "adhesive_mass_fraction",
                        "veh-adhesive",
                        "",
                        0.0,
                        1.0,
                        0.01,
                        0.0,
                    ),
                ],
            ),
            // A vehicle may be drawn axle by axle instead of as one wheelset. Baking counts
            // them up into the same two numbers — the running gear is simulated per vehicle
            // (see `physics`), so the diagram is documentation and a count, not a model.
            // ponytail: upgrade path is a per-axle adhesion state; nothing in the HUD or the
            // sound distinguishes axles today, so there is nothing to feed it.
            many(
                "bogie",
                RunningGear,
                vec![port("children", "port-axles", Mechanical)],
                vec![port("parent", "port-body", Mechanical)],
                vec![num("wheelbase", "veh-wheelbase", "m", 0.5, 6.0, 0.05, 2.6)],
            ),
            many(
                "axle",
                RunningGear,
                vec![shaft_in(), port("force", "port-force", Force)],
                vec![
                    port("parent", "port-body", Mechanical),
                    port("slip", "port-slip", Signal),
                ],
                vec![flag("driven", "veh-axle-driven", true)],
            ),
            // --- Control ----------------------------------------------------
            def(
                "cab",
                Control,
                vec![elec_in()],
                vec![
                    port("throttle", "port-throttle", Signal),
                    port("brake", "port-brake-demand", Signal),
                    port("direct", "port-direct", Signal),
                    port("sanding", "port-sanding", Signal),
                    port("regulator", "port-regulator", Signal),
                    port("cutoff", "port-cutoff", Signal),
                ],
                vec![],
            ),
            def(
                "afb",
                Control,
                vec![port("in", "port-throttle", Signal)],
                vec![port("out", "port-throttle", Signal)],
                vec![],
            ),
            // --- Logic ------------------------------------------------------
            // Everything below is a node of the signal graph (see `crate::signal`): it
            // computes, it does not move anything. `signal-out` is where the result leaves
            // the graph and takes hold of a lever.
            many(
                "value-in",
                Logic,
                vec![],
                vec![sig_out()],
                vec![choice(
                    "source",
                    "sig-source",
                    &[
                        "throttle",
                        "brake",
                        "direct",
                        "speed",
                        "speed-kmh",
                        "target-speed",
                        "cylinder",
                        "pipe",
                        "main-res",
                        "current",
                        "rpm",
                        "temp",
                        "effort",
                        "reverser",
                        "sanding",
                    ],
                    "speed-kmh",
                )],
            ),
            many(
                "constant",
                Logic,
                vec![],
                vec![sig_out()],
                vec![num("value", "sig-value", "", -1.0e6, 1.0e6, 0.1, 0.0)],
            ),
            many(
                "value-curve",
                Logic,
                vec![sig_in()],
                vec![sig_out()],
                vec![curve(
                    "points",
                    "sig-curve",
                    "",
                    "",
                    vec![(0.0, 0.0), (1.0, 1.0)],
                )],
            ),
            many(
                "combine",
                Logic,
                vec![
                    port("a", "port-value-a", Signal),
                    port("b", "port-value-b", Signal),
                ],
                vec![sig_out()],
                vec![choice(
                    "how",
                    "sig-combine",
                    &["add", "sub", "mul", "min", "max"],
                    "add",
                )],
            ),
            many(
                "clamp",
                Logic,
                vec![sig_in()],
                vec![sig_out()],
                vec![
                    num("min", "sig-min", "", -1.0e6, 1.0e6, 0.1, 0.0),
                    num("max", "sig-max", "", -1.0e6, 1.0e6, 0.1, 1.0),
                ],
            ),
            many(
                "pid",
                Logic,
                vec![
                    port("value", "port-value-actual", Signal),
                    port("setpoint", "port-value-target", Signal),
                ],
                vec![sig_out()],
                vec![
                    num("kp", "sig-kp", "", 0.0, 100.0, 0.01, 0.1),
                    num("ki", "sig-ki", "", 0.0, 100.0, 0.001, 0.02),
                    num("kd", "sig-kd", "", 0.0, 100.0, 0.001, 0.0),
                    num("min", "sig-min", "", -1.0e6, 1.0e6, 0.1, -1.0),
                    num("max", "sig-max", "", -1.0e6, 1.0e6, 0.1, 1.0),
                ],
            ),
            many(
                "notch",
                Logic,
                vec![sig_in()],
                vec![sig_out()],
                vec![
                    num("steps", "sig-steps", "", 0.0, 60.0, 1.0, 0.0),
                    num("rate", "sig-rate", "1/s", 0.01, 100.0, 0.05, 1.0),
                ],
            ),
            many(
                "rate-of-change",
                Logic,
                vec![sig_in()],
                vec![sig_out()],
                vec![num("smoothing", "sig-smoothing", "s", 0.0, 10.0, 0.05, 0.3)],
            ),
            many(
                "value-switch",
                Logic,
                vec![
                    port("control", "port-value-control", Signal),
                    port("a", "port-value-a", Signal),
                    port("b", "port-value-b", Signal),
                ],
                vec![sig_out()],
                vec![
                    num("threshold", "sig-threshold", "", -1.0e6, 1.0e6, 0.1, 0.5),
                    num("hysteresis", "sig-hysteresis", "", 0.0, 1.0e6, 0.1, 0.0),
                ],
            ),
            many(
                "signal-out",
                Logic,
                vec![sig_in()],
                vec![],
                vec![choice(
                    "sink",
                    "sig-sink",
                    &[
                        "throttle", "brake", "sanding", "blower", "aux0", "aux1", "aux2", "aux3",
                    ],
                    "aux0",
                )],
            ),
            // --- Equipment --------------------------------------------------
            def(
                "sifa",
                Equipment,
                vec![],
                vec![],
                vec![choice(
                    "kind",
                    "eq-sifa",
                    &["time-time", "time-distance", "rzm"],
                    "time-time",
                )],
            ),
            def(
                "pzb",
                Equipment,
                vec![],
                vec![],
                vec![
                    choice(
                        "variant",
                        "eq-pzb",
                        &[
                            "i54",
                            "i60",
                            "i60m",
                            "i60r",
                            "pzb60",
                            "pzb90-v15",
                            "pzb90-v20",
                        ],
                        "pzb90-v20",
                    ),
                    choice("train_type", "eq-train-type", &["o", "m", "u"], "o"),
                ],
            ),
            def("lzb", Equipment, vec![], vec![], vec![]),
            def(
                "doors",
                Equipment,
                vec![],
                vec![],
                vec![
                    choice(
                        "system",
                        "eq-doors",
                        &["none", "tb0", "tav", "uic-wtb"],
                        "tav",
                    ),
                    flag("passenger_doors", "eq-passenger-doors", true),
                ],
            ),
            def(
                "script",
                Equipment,
                vec![],
                vec![],
                vec![text("script", "veh-script")],
            ),
        ];
        Registry { defs }
    }
}

// ---------------------------------------------------------------------------
// Baking: graph → runtime spec
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// One finding of [`bake`]. `key` is an i18n key (`bake-*`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BakeIssue {
    pub block: Option<u32>,
    pub severity: Severity,
    pub key: &'static str,
}

impl BakeIssue {
    fn error(block: Option<u32>, key: &'static str) -> Self {
        BakeIssue {
            block,
            severity: Severity::Error,
            key,
        }
    }

    fn warn(block: Option<u32>, key: &'static str) -> Self {
        BakeIssue {
            block,
            severity: Severity::Warning,
            key,
        }
    }
}

/// Everything bake needs to look up blocks and parameters.
struct Baker<'a> {
    graph: &'a VehicleGraph,
    reg: &'a Registry,
    issues: Vec<BakeIssue>,
}

impl<'a> Baker<'a> {
    /// First block whose resolved kind is `kind`.
    fn find(&self, kind: &str) -> Option<&'a GraphBlock> {
        self.graph
            .blocks
            .iter()
            .find(|b| self.reg.base_kind(&b.kind) == Some(kind))
    }

    /// Parameter value with fallback onto the definition default.
    fn param(&self, block: &GraphBlock, id: &str) -> ParamValue {
        block
            .params
            .get(id)
            .cloned()
            .or_else(|| self.reg.default_of(&block.kind, id))
            .unwrap_or(ParamValue::Number(0.0))
    }

    fn num(&self, block: &GraphBlock, id: &str) -> f64 {
        self.param(block, id).number()
    }

    fn flag(&self, block: &GraphBlock, id: &str) -> bool {
        self.param(block, id).flag()
    }

    /// Is there any wire between a block of `from` kind and one of `to` kind?
    fn wired(&self, from: &str, to: &str) -> bool {
        self.graph.wires.iter().any(|w| {
            let f = self.graph.block(w.from);
            let t = self.graph.block(w.to);
            matches!((f, t), (Some(f), Some(t))
                if self.reg.base_kind(&f.kind) == Some(from)
                && self.reg.base_kind(&t.kind) == Some(to))
        })
    }

    /// Every block whose resolved kind is `kind`, in the order they sit in the file.
    fn find_all(&self, kind: &'a str) -> impl Iterator<Item = &'a GraphBlock> + '_ {
        self.graph
            .blocks
            .iter()
            .filter(move |b| self.reg.base_kind(&b.kind) == Some(kind))
    }

    /// The cooling system wired to this block's heat port, as a thermal model.
    ///
    /// A component without one never gets hot — which is the right default: a data sheet
    /// that says nothing about cooling is a vehicle whose builder did not want to model it.
    fn cooling_of(&self, block: &GraphBlock) -> Option<Thermal> {
        let cooling = self.graph.wires.iter().find_map(|w| {
            if w.from != block.id || w.from_port != "heat" {
                return None;
            }
            let to = self.graph.block(w.to)?;
            (self.reg.base_kind(&to.kind) == Some("cooling")).then_some(to)
        })?;
        Some(Thermal {
            heat_capacity: self.num(cooling, "heat_capacity"),
            cooling: self.num(cooling, "cooling"),
            natural_share: self.num(cooling, "natural_share"),
            warn_temp: self.num(cooling, "warn_temp"),
            max_temp: self.num(cooling, "max_temp"),
            ambient: self.num(cooling, "ambient"),
        })
    }

    /// Warns when two present kinds are not wired to each other.
    fn expect_wire(&mut self, from: &str, to: &str) {
        if let (Some(f), Some(_)) = (self.find(from), self.find(to))
            && !self.wired(from, to)
        {
            self.issues
                .push(BakeIssue::warn(Some(f.id), "bake-missing-wire"));
        }
    }
}

/// Compiles the graph into the runtime fields of `spec`. Returns all findings; the spec
/// is written as far as possible even in the presence of errors, so the editor can show
/// live values while the user is still wiring.
pub fn bake(graph: &VehicleGraph, reg: &Registry, spec: &mut VehicleSpec) -> Vec<BakeIssue> {
    let mut b = Baker {
        graph,
        reg,
        issues: Vec::new(),
    };

    // Unknown kinds and duplicate singletons. A repeatable block may appear as often as
    // the vehicle has of the thing.
    let mut seen: BTreeMap<&str, u32> = BTreeMap::new();
    for block in &graph.blocks {
        let Some(kind) = reg.base_kind(&block.kind) else {
            b.issues
                .push(BakeIssue::error(Some(block.id), "bake-unknown-block"));
            continue;
        };
        let repeatable = reg.get(kind).is_some_and(|d| d.repeatable);
        if seen.insert(kind, block.id).is_some() && !repeatable {
            b.issues
                .push(BakeIssue::error(Some(block.id), "bake-duplicate-block"));
        }
    }

    // Wires: endpoints, ports, direction and domain must line up.
    for wire in &graph.wires {
        let ok = (|| {
            let from = graph.block(wire.from)?;
            let to = graph.block(wire.to)?;
            let from_def = reg.get(&from.kind)?;
            let to_def = reg.get(&to.kind)?;
            let out = from_def.outputs.iter().find(|p| p.id == wire.from_port)?;
            let inp = to_def.inputs.iter().find(|p| p.id == wire.to_port)?;
            (out.domain == inp.domain).then_some(())
        })();
        if ok.is_none() {
            b.issues.push(BakeIssue::error(None, "bake-bad-wire"));
        }
    }

    // A block whose every port hangs in the air is probably a mistake.
    for block in &graph.blocks {
        let Some(def) = reg.get(&block.kind) else {
            continue;
        };
        let has_ports = !def.inputs.is_empty() || !def.outputs.is_empty();
        let touched = graph
            .wires
            .iter()
            .any(|w| w.from == block.id || w.to == block.id);
        if has_ports && !touched {
            b.issues
                .push(BakeIssue::warn(Some(block.id), "bake-unconnected"));
        }
    }

    bake_traction(&mut b, spec);
    bake_brakes(&mut b, spec);
    bake_equipment(&mut b, spec);
    spec.signal = bake_signal(&mut b);

    b.issues
}

fn governor_from(b: &Baker, block: &GraphBlock) -> Governor {
    match b.param(block, "governor").choice() {
        "fill" => Governor::Fill,
        _ => Governor::Speed {
            steps: b.num(block, "governor_steps").max(0.0) as u32,
            droop: b.num(block, "governor_droop"),
        },
    }
}

fn dynamic_brake_from(b: &Baker, block: &GraphBlock) -> DynamicBrake {
    DynamicBrake {
        max_force: b.num(block, "max_force"),
        max_power: b.num(block, "max_power"),
        fade_out_kmh: b.num(block, "fade_out_kmh"),
        regenerative: b.flag(block, "regenerative"),
        ramp_time: b.num(block, "ramp_time"),
        thermal: b.cooling_of(block),
    }
}

/// The starting equipment out of whatever contactor blocks the diagram carries.
fn starter_from(b: &Baker) -> Option<Starter> {
    let rheostat = b.find("rheostat");
    let switch = b.find("series-parallel-switch");
    let chopper = b.find("chopper");
    if rheostat.is_none() && switch.is_none() && chopper.is_none() {
        return None;
    }
    let resistor_steps = match rheostat {
        // A chopper has no resistor bank; the one step is its own continuous range.
        None => vec![0.0],
        Some(block) => {
            let mut steps: Vec<f64> = b.param(block, "steps").list().to_vec();
            if steps.is_empty() {
                steps.push(0.0);
            }
            // The last position always has to be resistance-free, or the drive can never
            // reach its running position.
            if steps.last().copied().unwrap_or(0.0) > 0.0 {
                steps.push(0.0);
            }
            steps
        }
    };
    let groups = match switch {
        None => vec![MotorGroup::Parallel],
        Some(block) => match b.param(block, "groups").choice() {
            "s-sp-p" => vec![
                MotorGroup::Series,
                MotorGroup::SeriesParallel,
                MotorGroup::Parallel,
            ],
            "s-only" => vec![MotorGroup::Series],
            "p-only" => vec![MotorGroup::Parallel],
            _ => vec![MotorGroup::Series, MotorGroup::Parallel],
        },
    };
    let step_time = match (chopper, rheostat) {
        (Some(chopper), _) => b.num(chopper, "response_time"),
        (None, Some(rheostat)) => b.num(rheostat, "step_time"),
        (None, None) => 1.0,
    };
    Some(Starter {
        resistor_steps,
        groups,
        step_time,
        chopper: chopper.is_some(),
        thermal: rheostat.and_then(|r| b.cooling_of(r)),
    })
}

fn async_motor_from(b: &Baker, block: &GraphBlock) -> AsyncMotor {
    AsyncMotor {
        count: b.num(block, "count").max(1.0) as u32,
        pole_pairs: b.num(block, "pole_pairs").max(1.0) as u32,
        rated_torque: b.num(block, "rated_torque"),
        pullout_ratio: b.num(block, "pullout_ratio"),
        pullout_slip: b.num(block, "pullout_slip"),
        rated_frequency: b.num(block, "rated_frequency"),
        max_frequency: b.num(block, "max_frequency"),
        gear_ratio: b.num(block, "gear_ratio"),
        wheel_diameter: b.num(block, "wheel_diameter"),
        efficiency: b.num(block, "efficiency"),
        thermal: b.cooling_of(block),
    }
}

fn series_motor_from(b: &Baker, block: &GraphBlock) -> SeriesMotor {
    SeriesMotor {
        count: b.num(block, "count").max(1.0) as u32,
        resistance: b.num(block, "resistance"),
        flux_constant: b.num(block, "flux_constant"),
        saturation_current: b.num(block, "saturation_current"),
        max_current: b.num(block, "max_current"),
        max_voltage: b.num(block, "max_voltage"),
        field_steps: b.param(block, "field_steps").list().to_vec(),
        gear_ratio: b.num(block, "gear_ratio"),
        wheel_diameter: b.num(block, "wheel_diameter"),
        efficiency: b.num(block, "efficiency"),
        thermal: b.cooling_of(block),
    }
}

/// Generator, load regulator and motors of a diesel-electric drive.
fn diesel_electric_from(b: &mut Baker) -> Option<DieselElectric> {
    let generator = b.find("generator")?;
    let motor = match (b.find("series-motor"), b.find("async-motor")) {
        (_, Some(async_motor)) => ElectricMotor::Ac(async_motor_from(b, async_motor)),
        (Some(dc), None) => ElectricMotor::Dc(series_motor_from(b, dc)),
        (None, None) => {
            b.issues
                .push(BakeIssue::warn(Some(generator.id), "bake-no-motor"));
            return None;
        }
    };
    let regulator = b.find("load-regulator");
    if regulator.is_none() {
        b.issues.push(BakeIssue::warn(
            Some(generator.id),
            "bake-no-load-regulator",
        ));
    }
    // A rectifier between generator and motors costs its own efficiency.
    let rectifier = b.find("rectifier").map_or(1.0, |r| b.num(r, "efficiency"));
    Some(DieselElectric {
        generator_power: b.num(generator, "power"),
        generator_efficiency: b.num(generator, "efficiency") * rectifier,
        max_voltage: b.num(generator, "max_voltage"),
        max_current: b.num(generator, "max_current"),
        regulator_time: regulator.map_or(3.0, |r| b.num(r, "response_time")),
        motor,
        blower_idle_share: regulator.map_or(0.2, |r| b.num(r, "blower_idle")),
    })
}

/// The steam locomotive out of its five blocks.
fn steam_from(b: &mut Baker) -> Option<(crate::steam::SteamLoco, f64)> {
    let cylinders = b.find("steam-cylinders")?;
    let Some(boiler) = b.find("boiler") else {
        b.issues
            .push(BakeIssue::error(Some(cylinders.id), "bake-no-boiler"));
        return None;
    };
    let Some(firebox) = b.find("firebox") else {
        b.issues
            .push(BakeIssue::error(Some(boiler.id), "bake-no-firebox"));
        return None;
    };
    let tender = b.find("tender");
    if tender.is_none() {
        b.issues
            .push(BakeIssue::warn(Some(boiler.id), "bake-no-tender"));
    }
    let injector = b.find("injector");
    if injector.is_none() {
        b.issues
            .push(BakeIssue::warn(Some(boiler.id), "bake-no-injector"));
    }
    let loco = crate::steam::SteamLoco {
        boiler_water: b.num(boiler, "water_space"),
        boiler_steam: b.num(boiler, "steam_space"),
        working_pressure: b.num(boiler, "working_pressure"),
        safety_valve: b.num(boiler, "safety_valve"),
        heating_surface: b.num(boiler, "heating_surface"),
        superheater: b.flag(boiler, "superheater"),
        grate_area: b.num(firebox, "grate_area"),
        grate_capacity: b.num(firebox, "grate_capacity"),
        burn_rate: b.num(firebox, "burn_rate"),
        blower_draught: b.num(firebox, "blower_draught"),
        cylinders: b.num(cylinders, "count").max(1.0) as u32,
        bore: b.num(cylinders, "bore"),
        stroke: b.num(cylinders, "stroke"),
        wheel_diameter: b.num(cylinders, "wheel_diameter"),
        max_cutoff: b.num(cylinders, "max_cutoff"),
        back_pressure: b.num(cylinders, "back_pressure"),
        efficiency: b.num(cylinders, "efficiency"),
        injector_rate: injector.map_or(0.0, |i| b.num(i, "rate")),
        tender_water: tender.map_or(0.0, |t| b.num(t, "water")),
        tender_coal: tender.map_or(0.0, |t| b.num(t, "coal")),
        shovel_mass: b.num(firebox, "shovel_mass"),
    };
    Some((loco, b.num(cylinders, "v_max")))
}

fn bake_traction(b: &mut Baker, spec: &mut VehicleSpec) {
    let drives = [
        "traction-curve",
        "tap-changer",
        "traction-converter",
        "diesel-engine",
        "steam-cylinders",
    ];
    let present: Vec<&str> = drives
        .iter()
        .copied()
        .filter(|k| b.find(k).is_some())
        .collect();
    if present.len() > 1 {
        let id = b.find(present[1]).map(|blk| blk.id);
        b.issues.push(BakeIssue::error(id, "bake-multiple-drives"));
    }
    let dynamic = b.find("dynamic-brake");

    // ponytail: the graph holds one traction chain — a second drive block is an error
    // above. Bake a Vec of chains once the diagram can express more than one.
    let traction = match present.first().copied() {
        None => {
            if let Some(blk) = dynamic {
                b.issues
                    .push(BakeIssue::warn(Some(blk.id), "bake-brake-needs-drive"));
            }
            None
        }
        Some("traction-curve") => {
            let blk = b.find("traction-curve").unwrap();
            Some(TractionSpec::Curve {
                force: b.param(blk, "force").curve().to_vec(),
                v_max: b.num(blk, "v_max"),
                brake: b.param(blk, "brake").curve().to_vec(),
                ramp_time: b.num(blk, "ramp_time"),
            })
        }
        Some("tap-changer") => {
            let blk = b.find("tap-changer").unwrap();
            if b.find("pantograph").is_none() && b.find("voltage-source").is_none() {
                b.issues
                    .push(BakeIssue::warn(Some(blk.id), "bake-no-pantograph"));
            }
            let motor = b.find("series-motor").map(|m| series_motor_from(b, m));
            let starter = starter_from(b);
            if starter.is_some() && motor.is_none() {
                let id = b.find("rheostat").or(b.find("chopper")).map(|blk| blk.id);
                b.issues
                    .push(BakeIssue::warn(id, "bake-starter-needs-motor"));
            }
            Some(TractionSpec::TapChanger {
                steps: b.num(blk, "steps").max(1.0) as u32,
                max_force: b.num(blk, "max_force"),
                max_power: b.num(blk, "max_power"),
                v_max: b.num(blk, "v_max"),
                step_time: b.num(blk, "step_time"),
                motor,
                starter,
                dynamic_brake: dynamic.map(|d| dynamic_brake_from(b, d)),
            })
        }
        Some("traction-converter") => {
            let blk = b.find("traction-converter").unwrap();
            if b.find("pantograph").is_none() && b.find("voltage-source").is_none() {
                b.issues
                    .push(BakeIssue::warn(Some(blk.id), "bake-no-pantograph"));
            }
            let brake = dynamic.map(|d| dynamic_brake_from(b, d));
            let motor = b.find("async-motor").map(|m| async_motor_from(b, m));
            Some(TractionSpec::Converter {
                max_force: b.num(blk, "max_force"),
                max_power: b.num(blk, "max_power"),
                v_max: b.num(blk, "v_max"),
                brake_force: brake.as_ref().map_or(0.0, |br| br.max_force),
                brake_power: brake.as_ref().map_or(0.0, |br| br.max_power),
                ramp_time: b.num(blk, "ramp_time"),
                v_pullout: b.num(blk, "v_pullout"),
                regenerative: brake.as_ref().is_some_and(|br| br.regenerative),
                brake_fade_kmh: brake.as_ref().map_or(0.0, |br| br.fade_out_kmh),
                motor,
            })
        }
        Some("diesel-engine") => {
            let blk = b.find("diesel-engine").unwrap();
            let engine = b.flag(blk, "engine_map").then(|| DieselEngine {
                idle_rpm: b.num(blk, "idle_rpm"),
                rated_rpm: b.num(blk, "rated_rpm"),
                max_rpm: b.num(blk, "max_rpm"),
                torque_curve: b.param(blk, "torque_curve").curve().to_vec(),
                governor: governor_from(b, blk),
                inertia: b.num(blk, "inertia"),
                response_time: b.num(blk, "response_time"),
            });
            let transmission = b.find("hydro-transmission").map(|t| {
                Box::new(Transmission {
                    circuits: b.param(t, "circuits").circuits().to_vec(),
                    speed_controlled: b.param(t, "power_control").choice() == "engine-speed",
                    fill_steps: b.num(t, "fill_steps").max(0.0) as u32,
                    fill_time: b.num(t, "fill_time"),
                    drain_time: b.num(t, "drain_time"),
                    hysteresis_kmh: b.num(t, "hysteresis_kmh"),
                    final_ratio: b.num(t, "final_ratio"),
                    shunting_ratio: b.num(t, "shunting_ratio"),
                    wheel_diameter: b.num(t, "wheel_diameter"),
                    count: b.num(t, "count").max(1.0) as u32,
                    efficiency: b.num(t, "efficiency"),
                })
            });
            let gearbox = b.find("mechanical-gearbox").map(|g| {
                Box::new(MechanicalGearbox {
                    gears: b.param(g, "gears").list().to_vec(),
                    final_ratio: b.num(g, "final_ratio"),
                    wheel_diameter: b.num(g, "wheel_diameter"),
                    efficiency: b.num(g, "efficiency"),
                    clutch_torque: b.num(g, "clutch_torque"),
                    clutch_time: b.num(g, "clutch_time"),
                    shift_time: b.num(g, "shift_time"),
                    shift_up_rpm: b.num(g, "shift_up_rpm"),
                    shift_down_rpm: b.num(g, "shift_down_rpm"),
                })
            });
            let hydrostatic = b.find("hydrostatic-drive").map(|h| HydrostaticDrive {
                max_force: b.num(h, "max_force"),
                efficiency: b.num(h, "efficiency"),
                response_time: b.num(h, "response_time"),
            });
            // One drive path per chain: a vehicle has a transmission, a gearbox, a
            // hydrostatic drive or a generator, never two of them.
            let paths = u8::from(transmission.is_some())
                + u8::from(gearbox.is_some())
                + u8::from(hydrostatic.is_some());
            if paths > 1 {
                b.issues
                    .push(BakeIssue::warn(Some(blk.id), "bake-two-drive-paths"));
            }
            if gearbox.is_some() && engine.is_none() {
                b.issues
                    .push(BakeIssue::warn(Some(blk.id), "bake-gearbox-needs-map"));
            }
            if transmission.is_some() && engine.is_none() {
                b.issues
                    .push(BakeIssue::warn(Some(blk.id), "bake-transmission-needs-map"));
            }
            if transmission.is_some() && b.find("generator").is_some() {
                b.issues
                    .push(BakeIssue::warn(Some(blk.id), "bake-hydro-and-generator"));
            }
            if dynamic.is_some() && b.find("generator").is_none() {
                let id = dynamic.map(|d| d.id);
                b.issues
                    .push(BakeIssue::warn(id, "bake-brake-needs-generator"));
            }
            let hydrodynamic_brake = b.find("retarder").map(|r| HydrodynamicBrake {
                absorption: b.num(r, "absorption"),
                ratio: b.num(r, "ratio"),
                wheel_diameter: b.num(r, "wheel_diameter"),
                max_force: b.num(r, "max_force"),
                max_power: b.num(r, "max_power"),
                fill_time: b.num(r, "fill_time"),
                fade_out_kmh: b.num(r, "fade_out_kmh"),
            });
            let dynamic_brake = dynamic.map(|d| dynamic_brake_from(b, d));
            // The generator branch is the diesel-electric one; without a transmission it is
            // what the drive runs on.
            let electric = if transmission.is_none() && gearbox.is_none() && hydrostatic.is_none() {
                diesel_electric_from(b)
            } else {
                None
            };
            let blk = b.find("diesel-engine").unwrap();
            Some(TractionSpec::Diesel {
                max_force: b.num(blk, "max_force"),
                max_power: b.num(blk, "max_power"),
                v_max: b.num(blk, "v_max"),
                ramp_time: b.num(blk, "ramp_time"),
                start_time: b.num(blk, "start_time"),
                engine,
                transmission,
                electric,
                gearbox,
                hydrostatic,
                hydrodynamic_brake,
                dynamic_brake,
            })
        }
        Some("steam-cylinders") => steam_from(b).map(|(loco, v_max)| TractionSpec::Steam {
            loco: Box::new(loco),
            v_max,
        }),
        Some(_) => unreachable!(),
    };
    spec.drives = traction.into_iter().map(DriveSpec::new).collect();

    if b.find("series-motor").is_some() && b.find("tap-changer").is_none() {
        let id = b.find("series-motor").map(|m| m.id);
        b.issues
            .push(BakeIssue::warn(id, "bake-series-motor-unused"));
    }

    // The chains the canvas should show as connected.
    b.expect_wire("diesel-engine", "hydro-transmission");
    b.expect_wire("hydro-transmission", "wheelset");
    if b.find("rheostat").is_none() && b.find("chopper").is_none() {
        // With contactor equipment the transformer feeds that instead of the motors.
        b.expect_wire("tap-changer", "series-motor");
    }
    b.expect_wire("series-motor", "wheelset");
    b.expect_wire("traction-converter", "traction-motor");
    b.expect_wire("traction-converter", "async-motor");
    b.expect_wire("async-motor", "wheelset");
    b.expect_wire("traction-motor", "wheelset");
    b.expect_wire("pantograph", "main-switch");
    b.expect_wire("main-switch", "transformer");
    b.expect_wire("diesel-engine", "generator");
    b.expect_wire("generator", "rectifier");
    // Contactor equipment sits in a chain: supply → resistors or chopper → grouping →
    // motors. Which of them is there decides what the next link is expected to be.
    if b.find("series-parallel-switch").is_some() {
        b.expect_wire("rheostat", "series-parallel-switch");
        b.expect_wire("chopper", "series-parallel-switch");
        b.expect_wire("series-parallel-switch", "series-motor");
    } else {
        b.expect_wire("rheostat", "series-motor");
        b.expect_wire("chopper", "series-motor");
    }
    // Steam: fire heats the boiler, the boiler feeds the cylinders, the tender feeds both.
    b.expect_wire("firebox", "boiler");
    b.expect_wire("boiler", "steam-cylinders");
    b.expect_wire("steam-cylinders", "wheelset");
    b.expect_wire("tender", "firebox");
    b.expect_wire("injector", "boiler");
}

/// Does the brake pipe reach `end` ("front" / "rear")?
fn pipe_end(b: &Baker, end: &str) -> bool {
    let mut cocks = b.find_all("angle-cock").peekable();
    if cocks.peek().is_none() {
        return true;
    }
    cocks.any(|c| b.param(c, "end").choice() == end)
}

fn brake_kind_from(b: &Baker, rigging: &GraphBlock) -> BrakeKind {
    match b.param(rigging, "kind").choice() {
        "disc" => BrakeKind::Disc,
        "composite-k" => BrakeKind::CompositeK,
        "composite-ll" => BrakeKind::CompositeLl,
        "magnetic" => BrakeKind::Magnetic,
        "custom" => BrakeKind::Custom(b.param(rigging, "friction_curve").curve().to_vec()),
        _ => BrakeKind::Block,
    }
}

fn control_valve_from(value: &str) -> ControlValve {
    match value {
        "k-gp" => ControlValve::KGp,
        "ke-gpr" => ControlValve::KeGpr,
        "ke-tm" => ControlValve::KeTm,
        "ke-l2a" => ControlValve::KeL2a,
        "ke-l2d" => ControlValve::KeL2d,
        _ => ControlValve::KeGp,
    }
}

fn bake_brakes(b: &mut Baker, spec: &mut VehicleSpec) {
    let Some(cv) = b.find("control-valve") else {
        b.issues
            .push(BakeIssue::error(None, "bake-no-control-valve"));
        return;
    };
    let Some(cylinder) = b.find("brake-cylinder") else {
        b.issues
            .push(BakeIssue::error(None, "bake-no-brake-cylinder"));
        return;
    };
    let Some(rigging) = b.find("brake-rigging") else {
        b.issues
            .push(BakeIssue::error(None, "bake-no-brake-rigging"));
        return;
    };
    let Some(pipe) = b.find("brake-pipe") else {
        b.issues.push(BakeIssue::error(None, "bake-no-brake-pipe"));
        return;
    };
    let aux = b.find("aux-reservoir");
    if aux.is_none() {
        b.issues
            .push(BakeIssue::warn(Some(cv.id), "bake-no-aux-reservoir"));
    }
    let main = b.find("main-reservoir");
    let relay = b.find("relay-valve");
    let direct = b.find("direct-brake");
    let parking = b.find("parking-brake");
    let mg = b.find("mg-brake");
    let fbv = b.find("driver-brake-valve");

    let load_braking = match b.param(cv, "load_braking").choice() {
        "weighing" => LoadBraking::Weighing,
        "changeover" => LoadBraking::Changeover {
            empty_share: b.num(cv, "empty_share"),
            changeover_mass_t: b.num(cv, "changeover_mass"),
        },
        _ => LoadBraking::None,
    };
    let default_position = match b.param(cv, "position").choice() {
        "g" => BrakePosition::G,
        "r" => BrakePosition::R,
        _ => BrakePosition::P,
    };

    spec.brake = BrakeSpec {
        kind: brake_kind_from(b, rigging),
        default_position,
        valve: control_valve_from(b.param(cv, "valve").choice()),
        valve_params: None,
        brake_weight: b.num(cv, "brake_weight"),
        load_braking,
        max_force: b.num(rigging, "max_force"),
        max_cylinder: b.num(cylinder, "max_cylinder"),
        cylinder_to_reservoir: b.num(cylinder, "cylinder_to_reservoir"),
        has_mg: mg.is_some(),
        mg_force: mg.map_or(0.0, |m| b.num(m, "force")),
        has_direct: direct.is_some(),
        direct_max_cylinder: direct.map_or(0.0, |d| b.num(d, "max_cylinder")),
        parking_force: parking.map_or(0.0, |p| b.num(p, "force")),
        spring_parking: parking.is_some_and(|p| b.flag(p, "spring")),
        pilot_controlled: relay.is_some(),
        supplement_brake: relay.is_some_and(|r| b.flag(r, "supplement")),
        angleicher: fbv.is_some_and(|f| b.flag(f, "angleicher")),
        aux_volume: aux.map_or(100.0, |a| b.num(a, "volume")),
        pipe_volume: b.num(pipe, "volume"),
        main_volume: main.map_or(0.0, |m| b.num(m, "volume")),
        compressor_delivery: b.find("compressor").map_or(0.0, |c| b.num(c, "delivery")),
        leakage: b.num(pipe, "leakage"),
        medium: match b.param(pipe, "medium").choice() {
            "vacuum" => BrakeMedium::Vacuum,
            _ => BrakeMedium::Air,
        },
        ep: b.find("ep-brake").map(|e| EpBrake {
            apply_rate: b.num(e, "apply_rate"),
            release_rate: b.num(e, "release_rate"),
            vents_pipe: b.flag(e, "vents_pipe"),
            steps: b.num(e, "steps").max(0.0) as u32,
        }),
        limit_pressure: b.find("limiting-valve").map_or(0.0, |l| b.num(l, "limit")),
        has_retainer: b.find("retainer-valve").is_some(),
        has_emergency_valve: b.find("emergency-valve").is_some(),
        // Where the diagram draws no cocks at all it says nothing about the ends and both
        // stay fitted; where it draws some, only the ends it draws one at are.
        pipe_front: pipe_end(b, "front"),
        pipe_rear: pipe_end(b, "rear"),
    };

    if mg.is_some() && !spec.brake.behaviour().rapid_position {
        b.issues
            .push(BakeIssue::warn(mg.map(|m| m.id), "bake-mg-needs-r"));
    }
    if relay.is_some() && main.is_none() {
        b.issues.push(BakeIssue::warn(
            relay.map(|r| r.id),
            "bake-needs-main-reservoir",
        ));
    }
    if direct.is_some() && main.is_none() {
        b.issues.push(BakeIssue::warn(
            direct.map(|d| d.id),
            "bake-needs-main-reservoir",
        ));
    }
    if b.find("compressor").is_some() && main.is_none() {
        let id = b.find("compressor").map(|c| c.id);
        b.issues
            .push(BakeIssue::warn(id, "bake-needs-main-reservoir"));
    }
    if spec.brake.spring_parking && parking.is_some() && main.is_none() {
        b.issues.push(BakeIssue::warn(
            parking.map(|p| p.id),
            "bake-needs-main-reservoir",
        ));
    }

    b.expect_wire("compressor", "main-reservoir");
    b.expect_wire("main-reservoir", "driver-brake-valve");
    b.expect_wire("driver-brake-valve", "brake-pipe");
    b.expect_wire("brake-pipe", "control-valve");
    b.expect_wire("aux-reservoir", "control-valve");
    if relay.is_some() {
        b.expect_wire("control-valve", "relay-valve");
        b.expect_wire("relay-valve", "brake-cylinder");
        b.expect_wire("main-reservoir", "relay-valve");
    } else {
        b.expect_wire("control-valve", "brake-cylinder");
    }
    b.expect_wire("brake-cylinder", "brake-rigging");
    b.expect_wire("brake-rigging", "wheelset");
    b.expect_wire("angle-cock", "air-hose");
    b.expect_wire("ep-brake", "brake-cylinder");
    b.expect_wire("limiting-valve", "brake-cylinder");
    b.expect_wire("retainer-valve", "brake-cylinder");

    // A vacuum brake has no reservoir to exhaust and nothing to pre-control from; an EP
    // brake is fed from the main reservoir and needs one.
    if spec.brake.medium.is_vacuum() {
        if relay.is_some() {
            b.issues
                .push(BakeIssue::warn(relay.map(|r| r.id), "bake-vacuum-no-relay"));
        }
        if b.find("compressor")
            .is_some_and(|c| b.param(c, "kind").choice() != "exhauster")
        {
            let id = b.find("compressor").map(|c| c.id);
            b.issues
                .push(BakeIssue::warn(id, "bake-vacuum-needs-exhauster"));
        }
    }
    if spec.brake.ep.is_some() && main.is_none() {
        let id = b.find("ep-brake").map(|e| e.id);
        b.issues
            .push(BakeIssue::warn(id, "bake-needs-main-reservoir"));
    }
}

fn bake_equipment(b: &mut Baker, spec: &mut VehicleSpec) {
    // The wheelset is where traction and brake force meet the rail — every diagram has
    // one, and the wires end there. Axle blocks next to it say *how* they meet it: how
    // many axles carry the vehicle and which of them take traction. Without them the
    // running gear is the even layout the wheelset's two numbers imply.
    let axles: Vec<&GraphBlock> = b.find_all("axle").collect();
    match b.find("wheelset") {
        Some(wheelset) => {
            spec.axles = b.num(wheelset, "axles").clamp(0.0, 255.0) as u8;
            spec.adhesive_mass_fraction = b.num(wheelset, "adhesive_mass_fraction").clamp(0.0, 1.0);
            if !axles.is_empty() && axles.len() != spec.axles as usize {
                b.issues.push(BakeIssue::warn(
                    Some(wheelset.id),
                    "bake-axle-count-mismatch",
                ));
            }
        }
        None if !axles.is_empty() => {
            spec.axles = axles.len().min(255) as u8;
            let driven = axles.iter().filter(|a| b.flag(a, "driven")).count();
            spec.adhesive_mass_fraction = driven as f64 / axles.len() as f64;
        }
        None => {
            b.issues.push(BakeIssue::error(None, "bake-no-wheelset"));
            return;
        }
    }
    // Drawn out, every axle is its own: the weight is shared evenly between them and each
    // one takes traction or does not.
    spec.running_gear = if axles.is_empty() {
        Vec::new()
    } else {
        let share = 1.0 / axles.len() as f64;
        axles
            .iter()
            .map(|a| AxleSpec {
                driven: b.flag(a, "driven"),
                load_share: share,
            })
            .collect()
    };
    if !axles.is_empty() {
        if !axles.iter().any(|a| b.flag(a, "driven")) && spec.powered() {
            b.issues.push(BakeIssue::warn(None, "bake-no-driven-axle"));
        }
        let bogies = b.find_all("bogie").count();
        if bogies > 0 && !axles.len().is_multiple_of(bogies) {
            b.issues.push(BakeIssue::warn(None, "bake-axles-per-bogie"));
        }
    }

    spec.slip_protection = match b.find("wheel-slide-protection") {
        None => SlipProtection::None,
        Some(w) => match b.param(w, "mode").choice() {
            "slip-brake" => SlipProtection::SlipBrake,
            "creep-control" => SlipProtection::CreepControl,
            _ => SlipProtection::TractionCutback,
        },
    };

    let sifa = b.find("sifa").map(|s| match b.param(s, "kind").choice() {
        "time-distance" => SifaKind::TimeDistance,
        "rzm" => SifaKind::Rzm,
        _ => SifaKind::TimeTime,
    });
    let pzb = b.find("pzb").map(|p| {
        (
            match b.param(p, "variant").choice() {
                "i54" => PzbVariant::I54,
                "i60" => PzbVariant::I60,
                "i60m" => PzbVariant::I60M,
                "i60r" => PzbVariant::I60R,
                "pzb60" => PzbVariant::Pzb60,
                "pzb90-v15" => PzbVariant::Pzb90V15,
                _ => PzbVariant::Pzb90V20,
            },
            match b.param(p, "train_type").choice() {
                "m" => TrainType::M,
                "u" => TrainType::U,
                _ => TrainType::O,
            },
        )
    });
    let lzb = b.find("lzb").is_some();
    spec.safety = if sifa.is_none() && pzb.is_none() && !lzb {
        SafetyEquipment::None
    } else {
        SafetyEquipment::De {
            pzb: pzb.map(|(variant, _)| variant),
            lzb,
            sifa,
            train_type: pzb.map(|(_, t)| t).unwrap_or_default(),
        }
    };

    match b.find("doors") {
        None => {
            spec.doors = DoorSystem::None;
            spec.passenger_doors = false;
        }
        Some(d) => {
            spec.doors = match b.param(d, "system").choice() {
                "none" => DoorSystem::None,
                "tb0" => DoorSystem::Tb0,
                "uic-wtb" => DoorSystem::UicWtb,
                _ => DoorSystem::Tav,
            };
            spec.passenger_doors = b.flag(d, "passenger_doors");
        }
    }

    spec.afb = b.find("afb").is_some();
    spec.script = b
        .find("script")
        .map(|s| b.param(s, "script").text().to_string())
        .filter(|s| !s.is_empty());

    // The vehicle's own electrical system. A diagram without a battery is a vehicle that
    // has none — a wagon — and then nothing on it switches on.
    let battery = b.find("battery");
    let pantographs: Vec<&GraphBlock> = b.find_all("pantograph").collect();
    // One block per system, as a real multi-system vehicle carries one head per system.
    let mut systems: Vec<SupplySystem> = Vec::new();
    for pan in &pantographs {
        if let Some(system) = SupplySystem::from_id(b.param(pan, "system").choice())
            && !systems.contains(&system)
        {
            systems.push(system);
        }
    }
    if systems.is_empty() {
        systems.push(SupplySystem::default());
    }
    let pantograph = pantographs.first().copied();
    spec.supply = PowerSupply {
        systems,
        rise_time: pantograph.map_or(5.0, |p| b.num(p, "rise_time")),
        battery_voltage: battery.map_or(0.0, |bat| b.num(bat, "voltage")),
        battery_capacity: battery.map_or(0.0, |bat| b.num(bat, "capacity")),
        source_voltage: b
            .find("voltage-source")
            .map_or(0.0, |s| b.num(s, "voltage")),
    };

    spec.sand_rate = b.find("sander").map_or(0.0, |s| b.num(s, "rate"));

    // Axle base: drawn out, the bogies' wheelbases add up to it. A single wheelset block
    // says nothing about the geometry, so the hand-edited figure stays.
    let bogies: Vec<&GraphBlock> = b.find_all("bogie").collect();
    if !bogies.is_empty() {
        spec.axle_base_sum = bogies.iter().map(|bg| b.num(bg, "wheelbase")).sum();
    }
}

// ---------------------------------------------------------------------------
// Signal graph
// ---------------------------------------------------------------------------

/// Compiles the logic blocks of the diagram into a [`SignalProgram`].
///
/// The blocks form a directed graph over `Signal` wires; the program is that graph in
/// topological order. A cycle cannot be evaluated — it is reported and the blocks in it are
/// dropped rather than run, because a loop at 200 Hz is not a thing to be forgiving about.
fn bake_signal(b: &mut Baker) -> SignalProgram {
    // Everything that computes. `signal-out` is not in here: it is where a value leaves.
    const LOGIC: [&str; 9] = [
        "value-in",
        "constant",
        "value-curve",
        "combine",
        "clamp",
        "pid",
        "notch",
        "rate-of-change",
        "value-switch",
    ];
    let nodes: Vec<&GraphBlock> = b
        .graph
        .blocks
        .iter()
        .filter(|blk| {
            b.reg
                .base_kind(&blk.kind)
                .is_some_and(|k| LOGIC.contains(&k))
        })
        .collect();
    if nodes.is_empty() {
        return SignalProgram::default();
    }

    // Which logic block feeds which input port of which other one.
    let source_of = |to: u32, port: &str| -> Option<u32> {
        b.graph
            .wires
            .iter()
            .find(|w| w.to == to && w.to_port == port)
            .map(|w| w.from)
    };
    let inputs_of = |blk: &GraphBlock| -> Vec<u32> {
        let kind = b.reg.base_kind(&blk.kind).unwrap_or("");
        let ports: &[&str] = match kind {
            "combine" => &["a", "b"],
            "pid" => &["value", "setpoint"],
            "value-switch" => &["control", "a", "b"],
            "value-in" | "constant" => &[],
            _ => &["in"],
        };
        ports
            .iter()
            .filter_map(|p| source_of(blk.id, p))
            .filter(|id| nodes.iter().any(|n| n.id == *id))
            .collect()
    };

    // Kahn's algorithm, one node at a time and always the first one that is ready. Taking
    // a whole level at once would be quicker and would reorder a diagram that was already
    // in a workable order — this way a program that goes out through `from_spec` comes back
    // in unchanged, which is what the round-trip test checks.
    let mut order: Vec<u32> = Vec::with_capacity(nodes.len());
    let mut open: Vec<&GraphBlock> = nodes.clone();
    while !open.is_empty() {
        let ready = open
            .iter()
            .position(|blk| inputs_of(blk).iter().all(|id| order.contains(id)));
        let Some(ready) = ready else {
            // Whatever is left feeds itself in a circle.
            for blk in &open {
                b.issues
                    .push(BakeIssue::error(Some(blk.id), "bake-signal-cycle"));
            }
            break;
        };
        order.push(open.remove(ready).id);
    }

    let index_of = |id: u32| order.iter().position(|o| *o == id).unwrap_or(0);
    let operand = |blk: &GraphBlock, port: &str| -> usize {
        source_of(blk.id, port)
            .filter(|id| order.contains(id))
            .map_or(0, index_of)
    };

    let mut ops = Vec::with_capacity(order.len());
    for id in &order {
        let Some(blk) = b.graph.block(*id) else {
            continue;
        };
        let kind = b.reg.base_kind(&blk.kind).unwrap_or("");
        ops.push(match kind {
            "value-in" => SignalOp::Read(SignalInput::from_id(b.param(blk, "source").choice())),
            "constant" => SignalOp::Const(b.num(blk, "value")),
            "value-curve" => SignalOp::Curve {
                input: operand(blk, "in"),
                points: b.param(blk, "points").curve().to_vec(),
            },
            "combine" => SignalOp::Combine {
                a: operand(blk, "a"),
                b: operand(blk, "b"),
                how: Combine::from_id(b.param(blk, "how").choice()),
            },
            "clamp" => SignalOp::Clamp {
                input: operand(blk, "in"),
                min: b.num(blk, "min"),
                max: b.num(blk, "max"),
            },
            "pid" => SignalOp::Pid {
                input: operand(blk, "value"),
                setpoint: operand(blk, "setpoint"),
                kp: b.num(blk, "kp"),
                ki: b.num(blk, "ki"),
                kd: b.num(blk, "kd"),
                min: b.num(blk, "min"),
                max: b.num(blk, "max"),
            },
            "notch" => SignalOp::Transition {
                input: operand(blk, "in"),
                steps: b.num(blk, "steps").max(0.0) as u32,
                rate: b.num(blk, "rate"),
            },
            "rate-of-change" => SignalOp::Rate {
                input: operand(blk, "in"),
                smoothing: b.num(blk, "smoothing"),
            },
            _ => SignalOp::Switch {
                control: operand(blk, "control"),
                a: operand(blk, "a"),
                b: operand(blk, "b"),
                threshold: b.num(blk, "threshold"),
                hysteresis: b.num(blk, "hysteresis"),
            },
        });
    }

    // Sinks.
    let mut outputs = Vec::new();
    for sink in b.find_all("signal-out").collect::<Vec<_>>() {
        match source_of(sink.id, "in").filter(|id| order.contains(id)) {
            Some(from) => outputs.push((
                SignalSink::from_id(b.param(sink, "sink").choice()),
                index_of(from),
            )),
            None => b
                .issues
                .push(BakeIssue::warn(Some(sink.id), "bake-signal-out-open")),
        }
    }
    if outputs.is_empty() && !ops.is_empty() {
        b.issues
            .push(BakeIssue::warn(None, "bake-signal-no-output"));
    }

    let program = SignalProgram { ops, outputs };
    debug_assert!(program.is_well_formed(), "bake produced a cyclic program");
    program
}

// ---------------------------------------------------------------------------
// Reverse direction: spec → graph
// ---------------------------------------------------------------------------

/// Builder that keeps layout and wiring readable.
struct Synth {
    graph: VehicleGraph,
    next: u32,
}

impl Synth {
    const COL: f32 = 250.0;
    const ROW: f32 = 170.0;

    fn add(&mut self, reg: &Registry, kind: &str, col: f32, row: f32) -> u32 {
        let id = self.next;
        self.next += 1;
        let pos = (col * Self::COL, row * Self::ROW);
        let block = reg
            .instantiate(kind, id, pos)
            .expect("built-in block kind exists");
        self.graph.blocks.push(block);
        id
    }

    fn set(&mut self, id: u32, param: &str, value: ParamValue) {
        if let Some(block) = self.graph.blocks.iter_mut().find(|b| b.id == id) {
            block.params.insert(param.to_string(), value);
        }
    }

    fn set_num(&mut self, id: u32, param: &str, value: f64) {
        self.set(id, param, ParamValue::Number(value));
    }

    fn wire(&mut self, from: u32, from_port: &str, to: u32, to_port: &str) {
        self.graph.wires.push(GraphWire {
            from,
            from_port: from_port.to_string(),
            to,
            to_port: to_port.to_string(),
        });
    }
}

/// Synthesises the block diagram of an existing spec — the editor uses it so that every
/// vehicle, graph or not, opens as blocks. `bake(from_spec(spec)) == spec` for the fields
/// the graph owns (see the round-trip test).
pub fn from_spec(spec: &VehicleSpec, reg: &Registry) -> VehicleGraph {
    let mut s = Synth {
        graph: VehicleGraph::default(),
        next: 0,
    };

    // Wheels first — traction and brakes both end here. A vehicle that states its running
    // gear axle by axle gets it drawn that way; one that does not gets the wheelset that
    // stands for the whole of it.
    let wheelset = s.add(reg, "wheelset", 4.0, 1.0);
    s.set_num(wheelset, "axles", spec.axles as f64);
    s.set_num(
        wheelset,
        "adhesive_mass_fraction",
        spec.adhesive_mass_fraction,
    );
    if !spec.running_gear.is_empty() {
        synth_running_gear(&mut s, reg, spec);
    }

    let cab = spec.powered().then(|| s.add(reg, "cab", 0.0, 0.0));
    // A battery is a fitting of its own: most wagons have none, but a wagon with a tail
    // lamp or a heating control does, and the diagram is where that is said.
    if spec.supply.battery_voltage > 0.0 {
        let battery = s.add(reg, "battery", 0.0, 1.0);
        s.set_num(battery, "voltage", spec.supply.battery_voltage);
        s.set_num(battery, "capacity", spec.supply.battery_capacity);
        if let Some(cab) = cab {
            s.wire(battery, "out", cab, "elec");
        }
    }

    // Throttle path, optionally through the AFB.
    let throttle_source = match (cab, spec.afb) {
        (Some(cab), true) => {
            let afb = s.add(reg, "afb", 1.0, 0.0);
            s.wire(cab, "throttle", afb, "in");
            Some((afb, "out"))
        }
        (Some(cab), false) => Some((cab, "throttle")),
        (None, _) => None,
    };
    let wire_throttle = |s: &mut Synth, to: u32, port: &str| {
        if let Some((src, src_port)) = throttle_source {
            s.wire(src, src_port, to, port);
        }
    };

    // ponytail: only the first chain reaches the diagram; the rest of a multi-engine
    // vehicle stays in the spec until the graph can hold more than one drive.
    match spec.traction() {
        None => {}
        Some(TractionSpec::Curve {
            force,
            v_max,
            brake,
            ramp_time,
        }) => {
            let curve = s.add(reg, "traction-curve", 2.0, 1.0);
            s.set(curve, "force", ParamValue::Curve(force.clone()));
            s.set(curve, "brake", ParamValue::Curve(brake.clone()));
            s.set_num(curve, "v_max", *v_max);
            s.set_num(curve, "ramp_time", *ramp_time);
            wire_throttle(&mut s, curve, "ctrl");
            s.wire(curve, "force", wheelset, "force");
        }
        Some(TractionSpec::TapChanger {
            steps,
            max_force,
            max_power,
            v_max,
            step_time,
            motor,
            starter,
            dynamic_brake,
        }) => {
            let pantograph = s.add(reg, "pantograph", 0.0, 2.0);
            synth_pantograph(&mut s, reg, pantograph, spec, 0.0, 2.0);
            let main_switch = s.add(reg, "main-switch", 1.0, 2.0);
            let transformer = s.add(reg, "transformer", 2.0, 2.0);
            let tap = s.add(reg, "tap-changer", 2.0, 1.0);
            s.set_num(tap, "steps", *steps as f64);
            s.set_num(tap, "max_force", *max_force);
            s.set_num(tap, "max_power", *max_power);
            s.set_num(tap, "v_max", *v_max);
            s.set_num(tap, "step_time", *step_time);
            s.wire(pantograph, "out", main_switch, "elec");
            s.wire(main_switch, "out", transformer, "elec");
            s.wire(transformer, "out", tap, "elec");
            wire_throttle(&mut s, tap, "ctrl");
            // Contactor equipment sits between the transformer and the motors.
            let supply = match starter {
                None => (tap, "out"),
                Some(starter) => synth_starter(&mut s, reg, starter, tap),
            };
            match motor {
                Some(motor) => {
                    let m = s.add(reg, "series-motor", 3.0, 1.0);
                    synth_series_motor(&mut s, reg, m, motor, 3.0, 0.0);
                    s.wire(supply.0, supply.1, m, "elec");
                    s.wire(m, "out", wheelset, "shaft");
                }
                None => {
                    let m = s.add(reg, "traction-motor", 3.0, 1.0);
                    s.wire(supply.0, supply.1, m, "elec");
                    s.wire(m, "out", wheelset, "shaft");
                }
            }
            if let Some(brake) = dynamic_brake {
                let d = s.add(reg, "dynamic-brake", 3.0, 2.0);
                synth_dynamic_brake(&mut s, reg, d, brake, 4.0, 2.0);
                s.wire(tap, "out", d, "elec");
            }
        }
        Some(TractionSpec::Converter {
            max_force,
            max_power,
            v_max,
            brake_force,
            brake_power,
            ramp_time,
            v_pullout,
            regenerative,
            brake_fade_kmh,
            motor,
        }) => {
            let pantograph = s.add(reg, "pantograph", 0.0, 2.0);
            synth_pantograph(&mut s, reg, pantograph, spec, 0.0, 2.0);
            let main_switch = s.add(reg, "main-switch", 1.0, 2.0);
            let transformer = s.add(reg, "transformer", 2.0, 2.0);
            let conv = s.add(reg, "traction-converter", 2.0, 1.0);
            s.set_num(conv, "max_force", *max_force);
            s.set_num(conv, "max_power", *max_power);
            s.set_num(conv, "v_max", *v_max);
            s.set_num(conv, "ramp_time", *ramp_time);
            s.set_num(conv, "v_pullout", *v_pullout);
            let m = match motor {
                Some(motor) => {
                    let m = s.add(reg, "async-motor", 3.0, 1.0);
                    synth_async_motor(&mut s, reg, m, motor, 3.0, 0.0);
                    m
                }
                None => s.add(reg, "traction-motor", 3.0, 1.0),
            };
            s.wire(pantograph, "out", main_switch, "elec");
            s.wire(main_switch, "out", transformer, "elec");
            s.wire(transformer, "out", conv, "elec");
            s.wire(conv, "out", m, "elec");
            s.wire(m, "out", wheelset, "shaft");
            wire_throttle(&mut s, conv, "ctrl");
            if *brake_force > 0.0 {
                let d = s.add(reg, "dynamic-brake", 3.0, 2.0);
                synth_dynamic_brake(
                    &mut s,
                    reg,
                    d,
                    &DynamicBrake {
                        max_force: *brake_force,
                        max_power: *brake_power,
                        fade_out_kmh: *brake_fade_kmh,
                        regenerative: *regenerative,
                        ramp_time: *ramp_time,
                        thermal: None,
                    },
                    4.0,
                    2.0,
                );
                s.wire(conv, "out", d, "elec");
            }
        }
        Some(TractionSpec::Diesel {
            max_force,
            max_power,
            v_max,
            ramp_time,
            start_time,
            engine,
            transmission,
            electric,
            gearbox,
            hydrostatic,
            hydrodynamic_brake,
            dynamic_brake,
        }) => {
            let tank = s.add(reg, "fuel-tank", 0.0, 2.0);
            let eng = s.add(reg, "diesel-engine", 1.0, 1.0);
            s.set_num(eng, "max_force", *max_force);
            s.set_num(eng, "max_power", *max_power);
            s.set_num(eng, "v_max", *v_max);
            s.set_num(eng, "ramp_time", *ramp_time);
            s.set_num(eng, "start_time", *start_time);
            s.wire(tank, "out", eng, "fuel");
            wire_throttle(&mut s, eng, "ctrl");
            match engine {
                Some(map) => {
                    s.set(eng, "engine_map", ParamValue::Bool(true));
                    s.set_num(eng, "idle_rpm", map.idle_rpm);
                    s.set_num(eng, "rated_rpm", map.rated_rpm);
                    s.set_num(eng, "max_rpm", map.max_rpm);
                    s.set(
                        eng,
                        "torque_curve",
                        ParamValue::Curve(map.torque_curve.clone()),
                    );
                    match map.governor {
                        Governor::Fill => {
                            s.set(eng, "governor", ParamValue::Choice("fill".to_string()));
                        }
                        Governor::Speed { steps, droop } => {
                            s.set(eng, "governor", ParamValue::Choice("speed".to_string()));
                            s.set_num(eng, "governor_steps", steps as f64);
                            s.set_num(eng, "governor_droop", droop);
                        }
                    }
                    s.set_num(eng, "inertia", map.inertia);
                    s.set_num(eng, "response_time", map.response_time);
                }
                None => s.set(eng, "engine_map", ParamValue::Bool(false)),
            }
            match transmission {
                Some(t) => {
                    let trans = s.add(reg, "hydro-transmission", 2.0, 1.0);
                    s.set(trans, "circuits", ParamValue::Circuits(t.circuits.clone()));
                    s.set(
                        trans,
                        "power_control",
                        ParamValue::Choice(
                            if t.speed_controlled {
                                "engine-speed"
                            } else {
                                "filling"
                            }
                            .to_string(),
                        ),
                    );
                    s.set_num(trans, "fill_steps", t.fill_steps as f64);
                    s.set_num(trans, "fill_time", t.fill_time);
                    s.set_num(trans, "drain_time", t.drain_time);
                    s.set_num(trans, "hysteresis_kmh", t.hysteresis_kmh);
                    s.set_num(trans, "final_ratio", t.final_ratio);
                    s.set_num(trans, "shunting_ratio", t.shunting_ratio);
                    s.set_num(trans, "wheel_diameter", t.wheel_diameter);
                    s.set_num(trans, "count", t.count as f64);
                    s.set_num(trans, "efficiency", t.efficiency);
                    s.wire(eng, "out", trans, "shaft");
                    s.wire(trans, "out", wheelset, "shaft");
                    if let Some(r) = hydrodynamic_brake {
                        let ret = s.add(reg, "retarder", 3.0, 2.0);
                        s.set_num(ret, "absorption", r.absorption);
                        s.set_num(ret, "ratio", r.ratio);
                        s.set_num(ret, "wheel_diameter", r.wheel_diameter);
                        s.set_num(ret, "max_force", r.max_force);
                        s.set_num(ret, "max_power", r.max_power);
                        s.set_num(ret, "fill_time", r.fill_time);
                        s.set_num(ret, "fade_out_kmh", r.fade_out_kmh);
                        s.wire(trans, "out", ret, "shaft");
                    }
                }
                // Mechanical gearbox and hydrostatic drive sit in the same place as the
                // transmission and are drawn the same way: engine → box → wheelset.
                None if gearbox.is_some() => {
                    if let Some(g) = gearbox {
                        let box_id = s.add(reg, "mechanical-gearbox", 2.0, 1.0);
                        s.set(box_id, "gears", ParamValue::List(g.gears.clone()));
                        s.set_num(box_id, "final_ratio", g.final_ratio);
                        s.set_num(box_id, "wheel_diameter", g.wheel_diameter);
                        s.set_num(box_id, "efficiency", g.efficiency);
                        s.set_num(box_id, "clutch_torque", g.clutch_torque);
                        s.set_num(box_id, "clutch_time", g.clutch_time);
                        s.set_num(box_id, "shift_time", g.shift_time);
                        s.set_num(box_id, "shift_up_rpm", g.shift_up_rpm);
                        s.set_num(box_id, "shift_down_rpm", g.shift_down_rpm);
                        s.wire(eng, "out", box_id, "shaft");
                        s.wire(box_id, "out", wheelset, "shaft");
                    }
                }
                None if hydrostatic.is_some() => {
                    if let Some(h) = hydrostatic {
                        let hyd = s.add(reg, "hydrostatic-drive", 2.0, 1.0);
                        s.set_num(hyd, "max_force", h.max_force);
                        s.set_num(hyd, "efficiency", h.efficiency);
                        s.set_num(hyd, "response_time", h.response_time);
                        s.wire(eng, "out", hyd, "shaft");
                        s.wire(hyd, "out", wheelset, "shaft");
                    }
                }
                None => match electric {
                    // Diesel-electric: engine → generator → rectifier → motors, with the
                    // load regulator on the generator's excitation.
                    Some(electric) => {
                        let generator = s.add(reg, "generator", 2.0, 1.0);
                        s.set_num(generator, "power", electric.generator_power);
                        s.set_num(generator, "efficiency", electric.generator_efficiency);
                        s.set_num(generator, "max_voltage", electric.max_voltage);
                        s.set_num(generator, "max_current", electric.max_current);
                        let regulator = s.add(reg, "load-regulator", 2.0, 0.0);
                        s.set_num(regulator, "response_time", electric.regulator_time);
                        s.set_num(regulator, "blower_idle", electric.blower_idle_share);
                        wire_throttle(&mut s, regulator, "ctrl");
                        s.wire(eng, "out", generator, "shaft");
                        let motor = match &electric.motor {
                            ElectricMotor::Dc(dc) => {
                                let m = s.add(reg, "series-motor", 3.0, 1.0);
                                synth_series_motor(&mut s, reg, m, dc, 3.0, 0.0);
                                m
                            }
                            ElectricMotor::Ac(ac) => {
                                let m = s.add(reg, "async-motor", 3.0, 1.0);
                                synth_async_motor(&mut s, reg, m, ac, 3.0, 0.0);
                                m
                            }
                        };
                        s.wire(generator, "out", motor, "elec");
                        s.wire(motor, "out", wheelset, "shaft");
                        if let Some(brake) = dynamic_brake {
                            let d = s.add(reg, "dynamic-brake", 4.0, 2.0);
                            synth_dynamic_brake(&mut s, reg, d, brake, 5.0, 2.0);
                            s.wire(generator, "out", d, "elec");
                        }
                    }
                    None if dynamic_brake.is_some() => {
                        // Electric brake but no generator data: the old coarse shape.
                        let generator = s.add(reg, "generator", 2.0, 1.0);
                        let motor = s.add(reg, "traction-motor", 3.0, 1.0);
                        s.wire(eng, "out", generator, "shaft");
                        s.wire(generator, "out", motor, "elec");
                        s.wire(motor, "out", wheelset, "shaft");
                        if let Some(brake) = dynamic_brake {
                            let d = s.add(reg, "dynamic-brake", 3.0, 2.0);
                            synth_dynamic_brake(&mut s, reg, d, brake, 4.0, 2.0);
                            s.wire(generator, "out", d, "elec");
                        }
                    }
                    None => s.wire(eng, "out", wheelset, "shaft"),
                },
            }
        }
        Some(TractionSpec::Steam { loco, v_max }) => {
            let tender = s.add(reg, "tender", 0.0, 2.0);
            s.set_num(tender, "water", loco.tender_water);
            s.set_num(tender, "coal", loco.tender_coal);
            let firebox = s.add(reg, "firebox", 1.0, 2.0);
            s.set_num(firebox, "grate_area", loco.grate_area);
            s.set_num(firebox, "grate_capacity", loco.grate_capacity);
            s.set_num(firebox, "burn_rate", loco.burn_rate);
            s.set_num(firebox, "blower_draught", loco.blower_draught);
            s.set_num(firebox, "shovel_mass", loco.shovel_mass);
            let boiler = s.add(reg, "boiler", 2.0, 1.0);
            s.set_num(boiler, "water_space", loco.boiler_water);
            s.set_num(boiler, "steam_space", loco.boiler_steam);
            s.set_num(boiler, "working_pressure", loco.working_pressure);
            s.set_num(boiler, "safety_valve", loco.safety_valve);
            s.set_num(boiler, "heating_surface", loco.heating_surface);
            s.set(boiler, "superheater", ParamValue::Bool(loco.superheater));
            let cylinders = s.add(reg, "steam-cylinders", 3.0, 1.0);
            s.set_num(cylinders, "count", loco.cylinders as f64);
            s.set_num(cylinders, "bore", loco.bore);
            s.set_num(cylinders, "stroke", loco.stroke);
            s.set_num(cylinders, "wheel_diameter", loco.wheel_diameter);
            s.set_num(cylinders, "max_cutoff", loco.max_cutoff);
            s.set_num(cylinders, "back_pressure", loco.back_pressure);
            s.set_num(cylinders, "efficiency", loco.efficiency);
            s.set_num(cylinders, "v_max", *v_max);
            let injector = s.add(reg, "injector", 1.0, 0.0);
            s.set_num(injector, "rate", loco.injector_rate);
            s.wire(tender, "coal", firebox, "fuel");
            s.wire(tender, "water", injector, "water");
            s.wire(firebox, "heat", boiler, "heat");
            s.wire(boiler, "steam", injector, "steam");
            s.wire(injector, "out", boiler, "water");
            s.wire(boiler, "steam", cylinders, "steam");
            s.wire(cylinders, "force", wheelset, "force");
            if let Some(cab) = cab {
                s.wire(cab, "regulator", cylinders, "ctrl");
                s.wire(cab, "cutoff", firebox, "ctrl");
                s.wire(cab, "direct", injector, "ctrl");
            }
        }
    }

    synth_brakes(&mut s, reg, spec, wheelset, cab);
    synth_equipment(&mut s, reg, spec);
    synth_signal(&mut s, reg, spec);
    s.graph
}

/// Draws the running gear out axle by axle, grouped into bogies where the axle count
/// divides evenly. The `wheelset` block stays as the point traction and brake force meet
/// the rail; the axles are what says *how* they meet it.
fn synth_running_gear(s: &mut Synth, reg: &Registry, spec: &VehicleSpec) {
    let count = spec.running_gear.len();
    // Four axles in two bogies, six in two, two in none — the usual arrangements.
    let per_bogie = if count.is_multiple_of(2) && count >= 4 {
        count / 2
    } else {
        count
    };
    let bogies: Vec<u32> = if per_bogie < count {
        (0..count / per_bogie)
            .map(|i| s.add(reg, "bogie", 5.0, i as f32))
            .collect()
    } else {
        Vec::new()
    };
    for (i, axle) in spec.running_gear.iter().enumerate() {
        let id = s.add(reg, "axle", 4.5, i as f32);
        s.set(id, "driven", ParamValue::Bool(axle.driven));
        if let Some(bogie) = bogies.get(i / per_bogie) {
            s.wire(id, "parent", *bogie, "children");
        }
    }
}

/// Puts a cooling system next to a block that has a thermal model, and wires it up.
fn synth_cooling(
    s: &mut Synth,
    reg: &Registry,
    source: u32,
    thermal: &Thermal,
    col: f32,
    row: f32,
) {
    let cooling = s.add(reg, "cooling", col, row);
    s.set_num(cooling, "heat_capacity", thermal.heat_capacity);
    s.set_num(cooling, "cooling", thermal.cooling);
    s.set_num(cooling, "natural_share", thermal.natural_share);
    s.set_num(cooling, "warn_temp", thermal.warn_temp);
    s.set_num(cooling, "max_temp", thermal.max_temp);
    s.set_num(cooling, "ambient", thermal.ambient);
    s.wire(source, "heat", cooling, "heat");
}

fn synth_dynamic_brake(
    s: &mut Synth,
    reg: &Registry,
    id: u32,
    brake: &DynamicBrake,
    col: f32,
    row: f32,
) {
    s.set_num(id, "max_force", brake.max_force);
    s.set_num(id, "max_power", brake.max_power);
    s.set_num(id, "fade_out_kmh", brake.fade_out_kmh);
    s.set(id, "regenerative", ParamValue::Bool(brake.regenerative));
    s.set_num(id, "ramp_time", brake.ramp_time);
    if let Some(thermal) = &brake.thermal {
        synth_cooling(s, reg, id, thermal, col, row);
    }
}

fn synth_series_motor(
    s: &mut Synth,
    reg: &Registry,
    id: u32,
    motor: &SeriesMotor,
    col: f32,
    row: f32,
) {
    s.set_num(id, "count", motor.count as f64);
    s.set_num(id, "resistance", motor.resistance);
    s.set_num(id, "flux_constant", motor.flux_constant);
    s.set_num(id, "saturation_current", motor.saturation_current);
    s.set_num(id, "max_current", motor.max_current);
    s.set_num(id, "max_voltage", motor.max_voltage);
    s.set(
        id,
        "field_steps",
        ParamValue::List(motor.field_steps.clone()),
    );
    s.set_num(id, "gear_ratio", motor.gear_ratio);
    s.set_num(id, "wheel_diameter", motor.wheel_diameter);
    s.set_num(id, "efficiency", motor.efficiency);
    if let Some(thermal) = &motor.thermal {
        synth_cooling(s, reg, id, thermal, col, row);
    }
}

fn synth_async_motor(
    s: &mut Synth,
    reg: &Registry,
    id: u32,
    motor: &AsyncMotor,
    col: f32,
    row: f32,
) {
    s.set_num(id, "count", motor.count as f64);
    s.set_num(id, "pole_pairs", motor.pole_pairs as f64);
    s.set_num(id, "rated_torque", motor.rated_torque);
    s.set_num(id, "pullout_ratio", motor.pullout_ratio);
    s.set_num(id, "pullout_slip", motor.pullout_slip);
    s.set_num(id, "rated_frequency", motor.rated_frequency);
    s.set_num(id, "max_frequency", motor.max_frequency);
    s.set_num(id, "gear_ratio", motor.gear_ratio);
    s.set_num(id, "wheel_diameter", motor.wheel_diameter);
    s.set_num(id, "efficiency", motor.efficiency);
    if let Some(thermal) = &motor.thermal {
        synth_cooling(s, reg, id, thermal, col, row);
    }
}

/// Contactor equipment between the supply and the motors. Returns the port the motors hang
/// on afterwards.
fn synth_starter(
    s: &mut Synth,
    reg: &Registry,
    starter: &Starter,
    supply: u32,
) -> (u32, &'static str) {
    let mut from = (supply, "out");
    if starter.chopper {
        let chopper = s.add(reg, "chopper", 2.5, 0.0);
        s.set_num(chopper, "response_time", starter.step_time);
        s.wire(from.0, from.1, chopper, "elec");
        from = (chopper, "out");
    } else {
        let rheostat = s.add(reg, "rheostat", 2.5, 0.0);
        s.set(
            rheostat,
            "steps",
            ParamValue::List(starter.resistor_steps.clone()),
        );
        s.set_num(rheostat, "step_time", starter.step_time);
        if let Some(thermal) = &starter.thermal {
            synth_cooling(s, reg, rheostat, thermal, 2.5, -1.0);
        }
        s.wire(from.0, from.1, rheostat, "elec");
        from = (rheostat, "out");
    }
    let groups = match starter.groups.as_slice() {
        [
            MotorGroup::Series,
            MotorGroup::SeriesParallel,
            MotorGroup::Parallel,
        ] => "s-sp-p",
        [MotorGroup::Series] => "s-only",
        [MotorGroup::Parallel] => "p-only",
        _ => "s-p",
    };
    let switch = s.add(reg, "series-parallel-switch", 2.5, 1.0);
    s.set(switch, "groups", ParamValue::Choice(groups.to_string()));
    s.wire(from.0, from.1, switch, "elec");
    (switch, "out")
}

fn synth_brakes(
    s: &mut Synth,
    reg: &Registry,
    spec: &VehicleSpec,
    wheelset: u32,
    cab: Option<u32>,
) {
    let brake = &spec.brake;
    let row = 3.0;

    let pipe = s.add(reg, "brake-pipe", 1.0, row);
    s.set_num(pipe, "volume", brake.pipe_volume);
    s.set_num(pipe, "leakage", brake.leakage);
    s.set(
        pipe,
        "medium",
        ParamValue::Choice(
            match brake.medium {
                BrakeMedium::Vacuum => "vacuum",
                BrakeMedium::Air => "air",
            }
            .to_string(),
        ),
    );
    // The pipe runs to both ends of the vehicle through a cock and a hose apiece.
    for (end, offset) in [("front", -1.0), ("rear", 1.0)] {
        let cock = s.add(reg, "angle-cock", 1.0 + offset * 0.5, row + 2.0);
        s.set(cock, "end", ParamValue::Choice(end.to_string()));
        let hose = s.add(reg, "air-hose", 1.0 + offset, row + 2.0);
        s.set(hose, "end", ParamValue::Choice(end.to_string()));
        s.wire(pipe, "out", cock, "pipe");
        s.wire(cock, "out", hose, "pipe");
    }

    let cv = s.add(reg, "control-valve", 2.0, row);
    s.set(
        cv,
        "valve",
        ParamValue::Choice(
            match brake.valve {
                ControlValve::KGp => "k-gp",
                ControlValve::KeGp => "ke-gp",
                ControlValve::KeGpr => "ke-gpr",
                ControlValve::KeTm => "ke-tm",
                ControlValve::KeL2a => "ke-l2a",
                ControlValve::KeL2d => "ke-l2d",
            }
            .to_string(),
        ),
    );
    s.set(
        cv,
        "position",
        ParamValue::Choice(
            match brake.default_position {
                BrakePosition::G => "g",
                BrakePosition::P => "p",
                BrakePosition::R => "r",
            }
            .to_string(),
        ),
    );
    s.set_num(cv, "brake_weight", brake.brake_weight);
    let (load, empty_share, changeover) = match brake.load_braking {
        LoadBraking::None => ("none", 0.6, 0.0),
        LoadBraking::Weighing => ("weighing", 0.6, 0.0),
        LoadBraking::Changeover {
            empty_share,
            changeover_mass_t,
        } => ("changeover", empty_share, changeover_mass_t),
    };
    s.set(cv, "load_braking", ParamValue::Choice(load.to_string()));
    s.set_num(cv, "empty_share", empty_share);
    s.set_num(cv, "changeover_mass", changeover);
    s.wire(pipe, "out", cv, "pipe");

    let aux = s.add(reg, "aux-reservoir", 1.0, row + 1.0);
    s.set_num(aux, "volume", brake.aux_volume);
    s.wire(aux, "out", cv, "aux");

    let cylinder = s.add(reg, "brake-cylinder", 3.0, row);
    s.set_num(cylinder, "max_cylinder", brake.max_cylinder);
    s.set_num(
        cylinder,
        "cylinder_to_reservoir",
        brake.cylinder_to_reservoir,
    );

    let rigging = s.add(reg, "brake-rigging", 4.0, row);
    let (kind, friction) = match &brake.kind {
        BrakeKind::Block => ("block", vec![]),
        BrakeKind::Disc => ("disc", vec![]),
        BrakeKind::CompositeK => ("composite-k", vec![]),
        BrakeKind::CompositeLl => ("composite-ll", vec![]),
        BrakeKind::Magnetic => ("magnetic", vec![]),
        BrakeKind::Custom(points) => ("custom", points.clone()),
    };
    s.set(rigging, "kind", ParamValue::Choice(kind.to_string()));
    s.set(rigging, "friction_curve", ParamValue::Curve(friction));
    s.set_num(rigging, "max_force", brake.max_force);
    s.wire(cylinder, "force", rigging, "force");
    s.wire(rigging, "out", wheelset, "force");

    let main = (brake.main_volume > 0.0).then(|| {
        let main = s.add(reg, "main-reservoir", 0.0, row + 1.0);
        s.set_num(main, "volume", brake.main_volume);
        main
    });
    if brake.compressor_delivery > 0.0 {
        let comp = s.add(reg, "compressor", 0.0, row + 2.0);
        s.set_num(comp, "delivery", brake.compressor_delivery);
        if let Some(main) = main {
            s.wire(comp, "out", main, "air");
        }
    }

    // A vehicle with traction gets the driver's brake valve; wagons run on the pipe alone.
    if spec.powered() {
        let fbv = s.add(reg, "driver-brake-valve", 0.0, row);
        s.set(fbv, "angleicher", ParamValue::Bool(brake.angleicher));
        s.wire(fbv, "out", pipe, "pipe");
        if let Some(main) = main {
            s.wire(main, "out", fbv, "supply");
        }
        if let Some(cab) = cab {
            s.wire(cab, "brake", fbv, "ctrl");
        }
    }

    if brake.pilot_controlled {
        let relay = s.add(reg, "relay-valve", 2.0, row + 1.0);
        s.set(
            relay,
            "supplement",
            ParamValue::Bool(brake.supplement_brake),
        );
        s.wire(cv, "out", relay, "pilot");
        s.wire(relay, "out", cylinder, "air");
        if let Some(main) = main {
            s.wire(main, "out", relay, "supply");
        }
    } else {
        s.wire(cv, "out", cylinder, "air");
    }

    if brake.has_direct {
        let direct = s.add(reg, "direct-brake", 3.0, row + 1.0);
        s.set_num(direct, "max_cylinder", brake.direct_max_cylinder);
        s.wire(direct, "out", cylinder, "air");
        if let Some(main) = main {
            s.wire(main, "out", direct, "supply");
        }
        if let Some(cab) = cab {
            s.wire(cab, "direct", direct, "ctrl");
        }
    }

    if brake.parking_force > 0.0 || brake.spring_parking {
        let parking = s.add(reg, "parking-brake", 4.0, row + 1.0);
        s.set_num(parking, "force", brake.parking_force);
        s.set(parking, "spring", ParamValue::Bool(brake.spring_parking));
        s.wire(parking, "force", wheelset, "force");
        if let Some(main) = main {
            s.wire(main, "out", parking, "air");
        }
    }

    if brake.has_mg {
        let mg = s.add(reg, "mg-brake", 5.0, row + 1.0);
        s.set_num(mg, "force", brake.mg_force);
        s.wire(mg, "force", wheelset, "force");
        if let Some(main) = main {
            s.wire(main, "out", mg, "air");
        }
    }

    if spec.slip_protection != SlipProtection::None {
        let wsp = s.add(reg, "wheel-slide-protection", 5.0, row);
        s.set(
            wsp,
            "mode",
            ParamValue::Choice(
                match spec.slip_protection {
                    SlipProtection::SlipBrake => "slip-brake",
                    SlipProtection::CreepControl => "creep-control",
                    _ => "traction-cutback",
                }
                .to_string(),
            ),
        );
        s.wire(wheelset, "slip", wsp, "slip");
    }

    if spec.sand_rate > 0.0 {
        let sander = s.add(reg, "sander", 5.0, row + 2.0);
        s.set_num(sander, "rate", spec.sand_rate);
        if let Some(main) = main {
            s.wire(main, "out", sander, "air");
        }
        if let Some(cab) = cab {
            s.wire(cab, "sanding", sander, "ctrl");
        }
    }

    if let Some(ep) = &brake.ep {
        let id = s.add(reg, "ep-brake", 2.0, row + 2.0);
        s.set_num(id, "apply_rate", ep.apply_rate);
        s.set_num(id, "release_rate", ep.release_rate);
        s.set(id, "vents_pipe", ParamValue::Bool(ep.vents_pipe));
        s.set_num(id, "steps", ep.steps as f64);
        s.wire(id, "out", cylinder, "air");
        if let Some(main) = main {
            s.wire(main, "out", id, "supply");
        }
        if let Some(cab) = cab {
            s.wire(cab, "brake", id, "ctrl");
        }
    }

    if brake.limit_pressure > 0.0 {
        let id = s.add(reg, "limiting-valve", 2.5, row - 1.0);
        s.set_num(id, "limit", brake.limit_pressure);
        s.wire(id, "out", cylinder, "air");
    }

    if brake.has_retainer {
        let id = s.add(reg, "retainer-valve", 4.0, row + 2.0);
        s.wire(id, "out", cylinder, "air");
    }

    if brake.has_emergency_valve {
        let id = s.add(reg, "emergency-valve", 0.0, row - 1.0);
        s.wire(pipe, "out", id, "pipe");
    }
}

/// The pantograph carries the vehicle's supply system and rise time. A multi-system
/// vehicle gets one block per system, stacked above the first.
fn synth_pantograph(
    s: &mut Synth,
    reg: &Registry,
    id: u32,
    spec: &VehicleSpec,
    col: f32,
    row: f32,
) {
    let mut systems = spec.supply.systems.iter();
    let first = systems.next().copied().unwrap_or_default();
    s.set(id, "system", ParamValue::Choice(first.id().to_string()));
    s.set_num(id, "rise_time", spec.supply.rise_time);
    for (i, system) in systems.enumerate() {
        let extra = s.add(reg, "pantograph", col, row - 1.0 - i as f32);
        s.set(extra, "system", ParamValue::Choice(system.id().to_string()));
        s.set_num(extra, "rise_time", spec.supply.rise_time);
    }
}

fn synth_equipment(s: &mut Synth, reg: &Registry, spec: &VehicleSpec) {
    let col = 6.0;
    let mut row = 0.0;
    let mut place = |s: &mut Synth, kind: &str| {
        let id = s.add(reg, kind, col, row);
        row += 1.0;
        id
    };

    if let SafetyEquipment::De {
        pzb,
        lzb,
        sifa,
        train_type,
    } = spec.safety
    {
        if let Some(kind) = sifa {
            let id = place(s, "sifa");
            s.set(
                id,
                "kind",
                ParamValue::Choice(
                    match kind {
                        SifaKind::TimeDistance => "time-distance",
                        SifaKind::Rzm => "rzm",
                        _ => "time-time",
                    }
                    .to_string(),
                ),
            );
        }
        if let Some(variant) = pzb {
            let id = place(s, "pzb");
            s.set(
                id,
                "variant",
                ParamValue::Choice(
                    match variant {
                        PzbVariant::I54 => "i54",
                        PzbVariant::I60 => "i60",
                        PzbVariant::I60M => "i60m",
                        PzbVariant::I60R => "i60r",
                        PzbVariant::Pzb60 => "pzb60",
                        PzbVariant::Pzb90V15 => "pzb90-v15",
                        PzbVariant::Pzb90V20 => "pzb90-v20",
                    }
                    .to_string(),
                ),
            );
            s.set(
                id,
                "train_type",
                ParamValue::Choice(
                    match train_type {
                        TrainType::M => "m",
                        TrainType::U => "u",
                        TrainType::O => "o",
                    }
                    .to_string(),
                ),
            );
        }
        if lzb {
            place(s, "lzb");
        }
    }

    if spec.doors != DoorSystem::None || spec.passenger_doors {
        let id = place(s, "doors");
        s.set(
            id,
            "system",
            ParamValue::Choice(
                match spec.doors {
                    DoorSystem::None => "none",
                    DoorSystem::Tb0 => "tb0",
                    DoorSystem::UicWtb => "uic-wtb",
                    DoorSystem::Tav => "tav",
                }
                .to_string(),
            ),
        );
        s.set(
            id,
            "passenger_doors",
            ParamValue::Bool(spec.passenger_doors),
        );
    }

    if let Some(script) = &spec.script {
        let id = place(s, "script");
        s.set(id, "script", ParamValue::Text(script.clone()));
    }
}

/// Draws the compiled signal graph back out as logic blocks. The program is already in
/// topological order, so one block per operation in the same order and the wires follow.
fn synth_signal(s: &mut Synth, reg: &Registry, spec: &VehicleSpec) {
    let program = &spec.signal;
    if program.ops.is_empty() {
        return;
    }
    let col = 8.0;
    let mut ids = Vec::with_capacity(program.ops.len());
    for (i, op) in program.ops.iter().enumerate() {
        let row = i as f32;
        let id = match op {
            SignalOp::Read(input) => {
                let id = s.add(reg, "value-in", col, row);
                s.set(id, "source", ParamValue::Choice(input.id().to_string()));
                id
            }
            SignalOp::Const(value) => {
                let id = s.add(reg, "constant", col, row);
                s.set_num(id, "value", *value);
                id
            }
            SignalOp::Curve { points, .. } => {
                let id = s.add(reg, "value-curve", col, row);
                s.set(id, "points", ParamValue::Curve(points.clone()));
                id
            }
            SignalOp::Combine { how, .. } => {
                let id = s.add(reg, "combine", col, row);
                s.set(id, "how", ParamValue::Choice(how.id().to_string()));
                id
            }
            SignalOp::Clamp { min, max, .. } => {
                let id = s.add(reg, "clamp", col, row);
                s.set_num(id, "min", *min);
                s.set_num(id, "max", *max);
                id
            }
            SignalOp::Pid {
                kp,
                ki,
                kd,
                min,
                max,
                ..
            } => {
                let id = s.add(reg, "pid", col, row);
                s.set_num(id, "kp", *kp);
                s.set_num(id, "ki", *ki);
                s.set_num(id, "kd", *kd);
                s.set_num(id, "min", *min);
                s.set_num(id, "max", *max);
                id
            }
            SignalOp::Transition { steps, rate, .. } => {
                let id = s.add(reg, "notch", col, row);
                s.set_num(id, "steps", *steps as f64);
                s.set_num(id, "rate", *rate);
                id
            }
            SignalOp::Rate { smoothing, .. } => {
                let id = s.add(reg, "rate-of-change", col, row);
                s.set_num(id, "smoothing", *smoothing);
                id
            }
            SignalOp::Switch {
                threshold,
                hysteresis,
                ..
            } => {
                let id = s.add(reg, "value-switch", col, row);
                s.set_num(id, "threshold", *threshold);
                s.set_num(id, "hysteresis", *hysteresis);
                id
            }
        };
        ids.push(id);
    }

    for (i, op) in program.ops.iter().enumerate() {
        let to = ids[i];
        let mut connect = |from: usize, port: &str| {
            if let Some(from) = ids.get(from) {
                s.wire(*from, "out", to, port);
            }
        };
        match op {
            SignalOp::Read(_) | SignalOp::Const(_) => {}
            SignalOp::Curve { input, .. }
            | SignalOp::Clamp { input, .. }
            | SignalOp::Transition { input, .. }
            | SignalOp::Rate { input, .. } => connect(*input, "in"),
            SignalOp::Combine { a, b, .. } => {
                connect(*a, "a");
                connect(*b, "b");
            }
            SignalOp::Pid {
                input, setpoint, ..
            } => {
                connect(*input, "value");
                connect(*setpoint, "setpoint");
            }
            SignalOp::Switch { control, a, b, .. } => {
                connect(*control, "control");
                connect(*a, "a");
                connect(*b, "b");
            }
        }
    }

    for (row, (sink, index)) in program.outputs.iter().enumerate() {
        let id = s.add(reg, "signal-out", col + 1.0, row as f32);
        s.set(id, "sink", ParamValue::Choice(sink.id()));
        if let Some(from) = ids.get(*index) {
            s.wire(*from, "out", id, "in");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The figure the documentation quotes. It is here so that adding a block without
    /// saying so in `MODS.md` and `STATUS.md` fails rather than drifts.
    #[test]
    fn the_palette_has_the_documented_number_of_blocks() {
        assert_eq!(Registry::builtin().defs.len(), 71);
        // Every category is actually used — an empty group in the palette is a mistake.
        for category in BlockCategory::ALL {
            assert!(
                Registry::builtin()
                    .defs
                    .iter()
                    .any(|d| d.category == category),
                "{category:?} has no blocks"
            );
        }
    }

    #[test]
    fn every_block_name_and_parameter_has_a_translation() {
        let reg = Registry::builtin();
        // The keys themselves; `i18n` checks that both locales carry them.
        for def in &reg.defs {
            assert!(def.name_key.starts_with("blk-"), "{}", def.id);
            for param in &def.params {
                assert!(!param.key.is_empty(), "{}::{}", def.id, param.id);
            }
            for port in def.inputs.iter().chain(def.outputs.iter()) {
                assert!(port.key.starts_with("port-"), "{}::{}", def.id, port.id);
            }
        }
    }

    #[test]
    fn registry_ids_are_unique_and_defaults_match_kinds() {
        let reg = Registry::builtin();
        for (i, def) in reg.defs.iter().enumerate() {
            assert!(
                !reg.defs[i + 1..].iter().any(|d| d.id == def.id),
                "duplicate id {}",
                def.id
            );
            for param in &def.params {
                let matches = matches!(
                    (&param.kind, &param.default),
                    (ParamKind::Number { .. }, ParamValue::Number(_))
                        | (ParamKind::Bool, ParamValue::Bool(_))
                        | (ParamKind::Choice(_), ParamValue::Choice(_))
                        | (ParamKind::Text, ParamValue::Text(_))
                        | (ParamKind::Curve { .. }, ParamValue::Curve(_))
                        | (ParamKind::List, ParamValue::List(_))
                        | (ParamKind::Circuits, ParamValue::Circuits(_))
                );
                assert!(
                    matches,
                    "default of {}::{} has the wrong type",
                    def.id, param.id
                );
                if let (ParamKind::Choice(options), ParamValue::Choice(value)) =
                    (&param.kind, &param.default)
                {
                    assert!(
                        options.contains(value),
                        "default of {}::{} not in options",
                        def.id,
                        param.id
                    );
                }
            }
        }
    }

    #[test]
    fn missing_wheelset_is_an_error() {
        let reg = Registry::builtin();
        let graph = VehicleGraph::default();
        let mut spec = crate::train::VehicleSpec {
            graph: None,
            ..test_spec()
        };
        let issues = bake(&graph, &reg, &mut spec);
        assert!(issues.iter().any(|i| i.key == "bake-no-wheelset"));
    }

    #[test]
    fn mod_preset_bakes_as_its_base() {
        let mut reg = Registry::builtin();
        let preset = ModBlockDef {
            id: "voith-l620".to_string(),
            name: "Voith L 620 reU2".to_string(),
            description: String::new(),
            base: "hydro-transmission".to_string(),
            params: BTreeMap::from([("final_ratio".to_string(), ParamValue::Number(2.7))]),
        };
        reg.add_mod_block("example", preset).unwrap();
        assert_eq!(
            reg.base_kind("example:voith-l620"),
            Some("hydro-transmission")
        );
        assert_eq!(
            reg.default_of("example:voith-l620", "final_ratio"),
            Some(ParamValue::Number(2.7))
        );
    }

    #[test]
    fn mod_preset_rejects_bad_base_and_params() {
        let mut reg = Registry::builtin();
        let bad_base = ModBlockDef {
            id: "x".into(),
            name: "X".into(),
            description: String::new(),
            base: "warp-drive".into(),
            params: BTreeMap::new(),
        };
        assert!(reg.add_mod_block("m", bad_base).is_err());
        let bad_param = ModBlockDef {
            id: "x".into(),
            name: "X".into(),
            description: String::new(),
            base: "battery".into(),
            params: BTreeMap::from([("flux".to_string(), ParamValue::Number(1.0))]),
        };
        assert!(reg.add_mod_block("m", bad_param).is_err());
    }

    fn test_spec() -> VehicleSpec {
        // The graph owns traction, brake, safety, doors, afb, slip protection, axles —
        // everything else keeps whatever the spec says.
        crate::train::VehicleSpec {
            name: "Test".to_string(),
            length: 19.0,
            mass_empty: 84_000.0,
            rotating_mass_factor: 0.15,
            davis: crate::train::Davis {
                a: 1200.0,
                b: 30.0,
                c: 6.0,
            },
            brake: BrakeSpec::from_brake_weight(80.0, BrakeKind::Disc),
            drives: Vec::new(),
            legacy_traction: None,
            coupler: crate::train::CouplerSpec::screw(),
            adhesive_mass_fraction: 1.0,
            slip_protection: SlipProtection::None,
            gauge: 1.435,
            v_max: 160.0,
            axles: 4,
            axle_base_sum: 5.0,
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
            signal: Default::default(),
            supply: Default::default(),
            sand_rate: 4.0,
            running_gear: Vec::new(),
        }
    }

    /// The fields the graph owns must survive spec → graph → spec unchanged.
    fn assert_round_trip(spec: &VehicleSpec) {
        let reg = Registry::builtin();
        let graph = from_spec(spec, &reg);
        let mut baked = spec.clone();
        // Scramble the owned fields so the test fails when bake does not write them.
        baked.drives.clear();
        baked.brake = BrakeSpec::from_brake_weight(1.0, BrakeKind::Block);
        baked.safety = SafetyEquipment::None;
        baked.afb = false;
        let issues = bake(&graph, &reg, &mut baked);
        let errors: Vec<_> = issues
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{}: bake errors {errors:?}", spec.name);
        assert!(
            !issues.iter().any(|i| i.key == "bake-missing-wire"),
            "{}: from_spec left an expected wire out",
            spec.name
        );
        assert_eq!(baked.drives, spec.drives, "{}", spec.name);
        assert_eq!(baked.brake, spec.brake, "{}", spec.name);
        assert_eq!(baked.safety, spec.safety, "{}", spec.name);
        assert_eq!(baked.doors, spec.doors, "{}", spec.name);
        assert_eq!(baked.passenger_doors, spec.passenger_doors, "{}", spec.name);
        assert_eq!(baked.afb, spec.afb, "{}", spec.name);
        assert_eq!(baked.slip_protection, spec.slip_protection, "{}", spec.name);
        assert_eq!(baked.axles, spec.axles, "{}", spec.name);
        assert_eq!(baked.script, spec.script, "{}", spec.name);
        assert_eq!(baked.supply, spec.supply, "{}", spec.name);
        assert_eq!(baked.sand_rate, spec.sand_rate, "{}", spec.name);
        assert_eq!(baked.running_gear, spec.running_gear, "{}", spec.name);
        assert!(
            (baked.adhesive_mass_fraction - spec.adhesive_mass_fraction).abs() < 1e-12,
            "{}",
            spec.name
        );
    }

    #[test]
    fn round_trip_plain_wagon() {
        assert_round_trip(&test_spec());
    }

    #[test]
    fn round_trip_curve_drive() {
        let mut spec = test_spec();
        spec.drives = vec![DriveSpec::new(TractionSpec::Curve {
            force: vec![(0.0, 150_000.0), (120.0, 60_000.0)],
            v_max: 120.0,
            brake: vec![(0.0, 0.0), (50.0, 80_000.0)],
            ramp_time: 2.0,
        })];
        spec.brake = BrakeSpec::from_brake_weight(70.0, BrakeKind::Block)
            .as_traction_unit(ControlValve::KeTm, 30_000.0);
        spec.adhesive_mass_fraction = 1.0;
        assert_round_trip(&spec);
    }

    /// The graph owns the signal program too — it has to survive the round trip.
    fn assert_signal_round_trip(spec: &VehicleSpec) {
        let reg = Registry::builtin();
        let graph = from_spec(spec, &reg);
        let mut baked = spec.clone();
        baked.signal = SignalProgram::default();
        let issues = bake(&graph, &reg, &mut baked);
        assert!(
            !issues.iter().any(|i| i.severity == Severity::Error),
            "{issues:?}"
        );
        assert_eq!(baked.signal, spec.signal);
    }

    #[test]
    fn round_trip_three_phase_drive_with_motor_data() {
        let mut spec = test_spec();
        spec.drives = vec![DriveSpec::new(TractionSpec::Converter {
            max_force: 300_000.0,
            max_power: 6_400_000.0,
            v_max: 220.0,
            brake_force: 150_000.0,
            brake_power: 2_600_000.0,
            ramp_time: 2.5,
            v_pullout: 0.0,
            regenerative: true,
            brake_fade_kmh: 10.0,
            motor: Some(AsyncMotor::default()),
        })];
        spec.brake = BrakeSpec::from_brake_weight(100.0, BrakeKind::Disc)
            .as_traction_unit(ControlValve::KeL2a, 60_000.0);
        assert_round_trip(&spec);
    }

    #[test]
    fn round_trip_contactor_drive() {
        let mut spec = test_spec();
        spec.drives = vec![DriveSpec::new(TractionSpec::TapChanger {
            steps: 1,
            max_force: 200_000.0,
            max_power: 1_200_000.0,
            v_max: 80.0,
            step_time: 0.5,
            motor: Some(SeriesMotor {
                count: 4,
                resistance: 0.05,
                flux_constant: 0.0289,
                saturation_current: 600.0,
                max_current: 1600.0,
                max_voltage: 1000.0,
                field_steps: vec![1.0, 0.8],
                gear_ratio: 2.17,
                wheel_diameter: 1.25,
                efficiency: 0.95,
                thermal: Some(Thermal::default()),
            }),
            starter: Some(Starter::default()),
            dynamic_brake: None,
        })];
        spec.brake = BrakeSpec::from_brake_weight(70.0, BrakeKind::Block)
            .as_traction_unit(ControlValve::KeTm, 30_000.0);
        assert_round_trip(&spec);
    }

    #[test]
    fn round_trip_diesel_electric_with_load_regulator() {
        let mut spec = test_spec();
        spec.drives = vec![DriveSpec::new(TractionSpec::Diesel {
            max_force: 400_000.0,
            max_power: 1_800_000.0,
            v_max: 120.0,
            ramp_time: 6.0,
            start_time: 10.0,
            engine: None,
            transmission: None,
            electric: Some(DieselElectric::default()),
            gearbox: None,
            hydrostatic: None,
            hydrodynamic_brake: None,
            dynamic_brake: None,
        })];
        spec.brake = BrakeSpec::from_brake_weight(100.0, BrakeKind::Block)
            .as_traction_unit(ControlValve::KeGp, 60_000.0);
        assert_round_trip(&spec);
    }

    #[test]
    fn round_trip_steam_locomotive() {
        let mut spec = test_spec();
        spec.drives = vec![DriveSpec::new(TractionSpec::Steam {
            loco: Box::new(crate::steam::SteamLoco::default()),
            v_max: 80.0,
        })];
        spec.brake = BrakeSpec::from_brake_weight(80.0, BrakeKind::Block)
            .as_traction_unit(ControlValve::KeGp, 40_000.0);
        assert_round_trip(&spec);
    }

    #[test]
    fn round_trip_vacuum_brake_with_a_retainer() {
        let mut spec = test_spec();
        spec.brake = BrakeSpec::from_brake_weight(20.0, BrakeKind::Block).as_vacuum();
        spec.brake.has_retainer = true;
        spec.brake.limit_pressure = 2.0;
        assert_round_trip(&spec);
    }

    #[test]
    fn round_trip_signal_graph() {
        let mut spec = test_spec();
        spec.signal = SignalProgram {
            ops: vec![
                SignalOp::Read(SignalInput::SpeedKmh),
                SignalOp::Read(SignalInput::TargetSpeedKmh),
                SignalOp::Pid {
                    input: 0,
                    setpoint: 1,
                    kp: 0.12,
                    ki: 0.03,
                    kd: 0.0,
                    min: -1.0,
                    max: 1.0,
                },
                SignalOp::Curve {
                    input: 0,
                    points: vec![(0.0, 0.2), (100.0, 1.0)],
                },
                SignalOp::Const(0.5),
                SignalOp::Combine {
                    a: 3,
                    b: 4,
                    how: Combine::Multiply,
                },
                SignalOp::Clamp {
                    input: 5,
                    min: 0.0,
                    max: 1.0,
                },
                SignalOp::Transition {
                    input: 6,
                    steps: 4,
                    rate: 0.5,
                },
                SignalOp::Rate {
                    input: 0,
                    smoothing: 0.4,
                },
                SignalOp::Switch {
                    control: 0,
                    a: 4,
                    b: 6,
                    threshold: 40.0,
                    hysteresis: 5.0,
                },
            ],
            outputs: vec![
                (SignalSink::Throttle, 2),
                (SignalSink::Blower, 7),
                (SignalSink::Aux(1), 8),
                (SignalSink::Aux(2), 9),
            ],
        };
        assert!(spec.signal.is_well_formed());
        assert_signal_round_trip(&spec);
    }

    #[test]
    fn a_cycle_in_the_logic_blocks_is_reported_and_not_evaluated() {
        let reg = Registry::builtin();
        let mut graph = VehicleGraph::default();
        // Two clamps feeding each other, plus the running gear so the rest bakes.
        graph
            .blocks
            .push(reg.instantiate("wheelset", 0, (0.0, 0.0)).unwrap());
        graph
            .blocks
            .push(reg.instantiate("clamp", 1, (0.0, 0.0)).unwrap());
        graph
            .blocks
            .push(reg.instantiate("clamp", 2, (0.0, 0.0)).unwrap());
        graph.wires.push(GraphWire {
            from: 1,
            from_port: "out".into(),
            to: 2,
            to_port: "in".into(),
        });
        graph.wires.push(GraphWire {
            from: 2,
            from_port: "out".into(),
            to: 1,
            to_port: "in".into(),
        });
        let mut spec = test_spec();
        let issues = bake(&graph, &reg, &mut spec);
        assert_eq!(
            issues
                .iter()
                .filter(|i| i.key == "bake-signal-cycle")
                .count(),
            2
        );
        assert!(spec.signal.ops.is_empty());
    }

    #[test]
    fn round_trip_running_gear_drawn_axle_by_axle() {
        let mut spec = test_spec();
        spec.axles = 4;
        // A Bo'2': two driven axles leading, two carrying.
        spec.running_gear = vec![
            AxleSpec {
                driven: true,
                load_share: 0.25,
            },
            AxleSpec {
                driven: true,
                load_share: 0.25,
            },
            AxleSpec {
                driven: false,
                load_share: 0.25,
            },
            AxleSpec {
                driven: false,
                load_share: 0.25,
            },
        ];
        spec.adhesive_mass_fraction = 0.5;
        assert_round_trip(&spec);
    }

    #[test]
    fn a_vehicle_drawn_axle_by_axle_counts_its_axles() {
        let reg = Registry::builtin();
        let mut graph = VehicleGraph::default();
        for id in 0..4u32 {
            let mut axle = reg.instantiate("axle", id, (0.0, id as f32)).unwrap();
            // Two driven, two running.
            axle.params
                .insert("driven".into(), ParamValue::Bool(id < 2));
            graph.blocks.push(axle);
        }
        graph
            .blocks
            .push(reg.instantiate("bogie", 10, (1.0, 0.0)).unwrap());
        graph
            .blocks
            .push(reg.instantiate("bogie", 11, (1.0, 1.0)).unwrap());
        let mut spec = test_spec();
        bake(&graph, &reg, &mut spec);
        assert_eq!(spec.axles, 4);
        assert!((spec.adhesive_mass_fraction - 0.5).abs() < 1e-12);
    }

    #[test]
    fn round_trip_diesel_electric_with_dynamic_brake() {
        let mut spec = test_spec();
        spec.drives = vec![DriveSpec::new(TractionSpec::Diesel {
            max_force: 400_000.0,
            max_power: 2_460_000.0,
            v_max: 120.0,
            ramp_time: 6.0,
            start_time: 10.0,
            engine: None,
            transmission: None,
            electric: None,
            gearbox: None,
            hydrostatic: None,
            hydrodynamic_brake: None,
            dynamic_brake: Some(DynamicBrake {
                max_force: 200_000.0,
                max_power: 2_000_000.0,
                fade_out_kmh: 15.0,
                regenerative: false,
                ramp_time: 4.0,
                thermal: None,
            }),
        })];
        spec.brake = BrakeSpec::from_brake_weight(100.0, BrakeKind::Block)
            .as_traction_unit(ControlValve::KeGp, 60_000.0);
        assert_round_trip(&spec);
    }
}
