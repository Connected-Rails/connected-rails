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

use crate::brakes::{BrakeKind, BrakePosition, BrakeSpec, ControlValve, LoadBraking, SlipProtection};
use crate::doors::DoorSystem;
use crate::drive::{
    Circuit, DieselEngine, DynamicBrake, Governor, HydrodynamicBrake, SeriesMotor, TractionSpec,
    Transmission,
};
use crate::safety::SafetyEquipment;
use crate::safety::de::{PzbVariant, SifaKind, TrainType};
use crate::train::VehicleSpec;

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
    /// Fuel flow.
    Fuel,
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
        }
    }
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
    Curve { x_unit: String, y_unit: String },
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
    Brake,
    RunningGear,
    Control,
    Equipment,
}

impl BlockCategory {
    pub const ALL: [BlockCategory; 7] = [
        BlockCategory::Energy,
        BlockCategory::Drivetrain,
        BlockCategory::Electric,
        BlockCategory::Brake,
        BlockCategory::RunningGear,
        BlockCategory::Control,
        BlockCategory::Equipment,
    ];

    pub fn key(self) -> &'static str {
        match self {
            BlockCategory::Energy => "blkcat-energy",
            BlockCategory::Drivetrain => "blkcat-drivetrain",
            BlockCategory::Electric => "blkcat-electric",
            BlockCategory::Brake => "blkcat-brake",
            BlockCategory::RunningGear => "blkcat-running-gear",
            BlockCategory::Control => "blkcat-control",
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

        let defs = vec![
            // --- Energy -----------------------------------------------------
            def("battery", Energy, vec![], vec![elec_out()], vec![]),
            def(
                "fuel-tank",
                Energy,
                vec![],
                vec![port("out", "port-fuel", Fuel)],
                vec![num("capacity", "drv-fuel-capacity", "l", 0.0, 20_000.0, 10.0, 3000.0)],
            ),
            def("pantograph", Energy, vec![], vec![elec_out()], vec![]),
            def(
                "diesel-engine",
                Energy,
                vec![port("fuel", "port-fuel", Fuel), ctrl_in()],
                vec![shaft_out()],
                vec![
                    num("max_force", "drv-start-force-diesel", "N", 0.0, 2.0e6, 500.0, 235_000.0),
                    num("max_power", "drv-power", "W", 0.0, 20.0e6, 5000.0, 900_000.0),
                    num("v_max", "drv-vmax", "km/h", 0.0, 500.0, 1.0, 140.0),
                    num("ramp_time", "drv-ramp", "s", 0.1, 60.0, 0.1, 8.0),
                    num("start_time", "drv-crank-time", "s", 0.0, 60.0, 0.1, 8.0),
                    flag("engine_map", "eng-map", true),
                    num("idle_rpm", "eng-idle", "1/min", 0.0, 3000.0, 5.0, 650.0),
                    num("rated_rpm", "eng-rated", "1/min", 0.0, 3000.0, 5.0, 1500.0),
                    num("max_rpm", "eng-overspeed", "1/min", 0.0, 3500.0, 5.0, 1600.0),
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
                    num("fill_steps", "trm-fill-steps", "", 0.0, 40.0, 1.0, 0.0),
                    num("fill_time", "trm-fill-time", "s", 0.05, 10.0, 0.05, 1.2),
                    num("drain_time", "trm-drain-time", "s", 0.0, 10.0, 0.05, 0.0),
                    num("hysteresis_kmh", "trm-hysteresis", "km/h", 0.0, 30.0, 0.5, 8.0),
                    num("final_ratio", "trm-final-ratio", "", 0.1, 20.0, 0.01, 1.9),
                    num("wheel_diameter", "drv-wheel-diameter", "m", 0.3, 2.0, 0.01, 1.0),
                    num("count", "trm-count", "", 1.0, 8.0, 1.0, 1.0),
                    num("efficiency", "trm-efficiency", "", 0.5, 1.0, 0.01, 0.96),
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
                    num("wheel_diameter", "drv-wheel-diameter", "m", 0.3, 2.0, 0.01, 1.0),
                    num("max_force", "ret-brake-force", "N", 0.0, 500_000.0, 500.0, 80_000.0),
                    num("max_power", "ret-brake-power", "W", 0.0, 5.0e6, 5000.0, 1.0e6),
                    num("fill_time", "ret-fill-time", "s", 0.05, 10.0, 0.05, 1.5),
                    num("fade_out_kmh", "drv-fade", "km/h", 0.0, 100.0, 1.0, 10.0),
                ],
            ),
            def("generator", Drivetrain, vec![shaft_in()], vec![elec_out()], vec![]),
            def(
                "traction-motor",
                Drivetrain,
                vec![elec_in()],
                vec![shaft_out()],
                vec![],
            ),
            def(
                "series-motor",
                Drivetrain,
                vec![elec_in()],
                vec![shaft_out()],
                vec![
                    num("count", "mot-count", "", 1.0, 16.0, 1.0, 4.0),
                    num("resistance", "mot-resistance", "Ω", 0.001, 5.0, 0.001, 0.05),
                    num("flux_constant", "mot-machine-constant", "V·s/A", 0.001, 1.0, 0.001, 0.011),
                    num("saturation_current", "mot-saturation", "A", 10.0, 5000.0, 5.0, 550.0),
                    num("max_current", "mot-max-current", "A", 10.0, 5000.0, 5.0, 620.0),
                    num("max_voltage", "mot-max-voltage", "V", 10.0, 5000.0, 5.0, 590.0),
                    list("field_steps", "mot-field-steps", vec![1.0]),
                    num("gear_ratio", "mot-gear-ratio", "", 0.5, 10.0, 0.01, 3.17),
                    num("wheel_diameter", "drv-wheel-diameter", "m", 0.3, 2.0, 0.01, 1.25),
                    num("efficiency", "mot-efficiency", "", 0.5, 1.0, 0.01, 0.9),
                ],
            ),
            // --- Electric ---------------------------------------------------
            def("main-switch", Electric, vec![elec_in()], vec![elec_out()], vec![]),
            def("transformer", Electric, vec![elec_in()], vec![elec_out()], vec![]),
            def(
                "tap-changer",
                Electric,
                vec![elec_in(), ctrl_in()],
                vec![elec_out()],
                vec![
                    num("steps", "tap-steps", "", 1.0, 60.0, 1.0, 28.0),
                    num("max_force", "drv-start-force", "N", 0.0, 2.0e6, 500.0, 275_000.0),
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
                    num("max_force", "drv-start-force", "N", 0.0, 2.0e6, 500.0, 300_000.0),
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
                vec![],
                vec![
                    num("max_force", "drv-brake-force", "N", 0.0, 1.0e6, 500.0, 150_000.0),
                    num("max_power", "drv-brake-power", "W", 0.0, 10.0e6, 5000.0, 4.0e6),
                    num("fade_out_kmh", "drv-brake-fade", "km/h", 0.0, 100.0, 1.0, 5.0),
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
            // --- Air supply and brake --------------------------------------
            def(
                "compressor",
                Brake,
                vec![elec_in()],
                vec![air_out()],
                vec![num(
                    "delivery",
                    "brk-compressor-delivery",
                    "l/min",
                    0.0,
                    10_000.0,
                    10.0,
                    2400.0,
                )],
            ),
            def(
                "main-reservoir",
                Brake,
                vec![air_in()],
                vec![air_out()],
                vec![num("volume", "brk-main-volume", "l", 0.0, 5000.0, 10.0, 1000.0)],
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
                    choice("position", "brk-position", &["g", "p", "r"], "p"),
                    num("brake_weight", "brk-weight", "t", 0.0, 500.0, 1.0, 50.0),
                    choice(
                        "load_braking",
                        "brk-load",
                        &["none", "weighing", "changeover"],
                        "none",
                    ),
                    num("empty_share", "brk-load-empty", "", 0.0, 1.0, 0.01, 0.6),
                    num("changeover_mass", "brk-load-mass", "t", 0.0, 200.0, 0.5, 0.0),
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
                        &["block", "disc", "composite-k", "composite-ll", "magnetic", "custom"],
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
                vec![num("max_cylinder", "brk-direct-cylinder", "bar", 0.0, 10.0, 0.05, 0.0)],
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
                vec![num("force", "brk-mg-force", "N", 0.0, 500_000.0, 500.0, 90_000.0)],
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
            def("sander", Brake, vec![air_in(), ctrl_in()], vec![], vec![]),
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
                        &["i54", "i60", "i60m", "i60r", "pzb60", "pzb90-v15", "pzb90-v20"],
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
                    choice("system", "eq-doors", &["none", "tb0", "tav", "uic-wtb"], "tav"),
                    flag("passenger_doors", "eq-passenger-doors", true),
                ],
            ),
            def("script", Equipment, vec![], vec![], vec![text("script", "veh-script")]),
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

    /// Warns when two present kinds are not wired to each other.
    fn expect_wire(&mut self, from: &str, to: &str) {
        if let (Some(f), Some(_)) = (self.find(from), self.find(to))
            && !self.wired(from, to)
        {
            self.issues.push(BakeIssue::warn(Some(f.id), "bake-missing-wire"));
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

    // Unknown kinds and duplicate singletons.
    let mut seen: BTreeMap<&str, u32> = BTreeMap::new();
    for block in &graph.blocks {
        let Some(kind) = reg.base_kind(&block.kind) else {
            b.issues.push(BakeIssue::error(Some(block.id), "bake-unknown-block"));
            continue;
        };
        if seen.insert(kind, block.id).is_some() {
            b.issues.push(BakeIssue::error(Some(block.id), "bake-duplicate-block"));
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
            b.issues.push(BakeIssue::warn(Some(block.id), "bake-unconnected"));
        }
    }

    bake_traction(&mut b, spec);
    bake_brakes(&mut b, spec);
    bake_equipment(&mut b, spec);

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
    }
}

fn bake_traction(b: &mut Baker, spec: &mut VehicleSpec) {
    let drives = ["traction-curve", "tap-changer", "traction-converter", "diesel-engine"];
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

    spec.traction = match present.first().copied() {
        None => {
            if let Some(blk) = dynamic {
                b.issues.push(BakeIssue::warn(Some(blk.id), "bake-brake-needs-drive"));
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
            if b.find("pantograph").is_none() {
                b.issues.push(BakeIssue::warn(Some(blk.id), "bake-no-pantograph"));
            }
            let motor = b.find("series-motor").map(|m| SeriesMotor {
                count: b.num(m, "count").max(1.0) as u32,
                resistance: b.num(m, "resistance"),
                flux_constant: b.num(m, "flux_constant"),
                saturation_current: b.num(m, "saturation_current"),
                max_current: b.num(m, "max_current"),
                max_voltage: b.num(m, "max_voltage"),
                field_steps: b.param(m, "field_steps").list().to_vec(),
                gear_ratio: b.num(m, "gear_ratio"),
                wheel_diameter: b.num(m, "wheel_diameter"),
                efficiency: b.num(m, "efficiency"),
            });
            Some(TractionSpec::TapChanger {
                steps: b.num(blk, "steps").max(1.0) as u32,
                max_force: b.num(blk, "max_force"),
                max_power: b.num(blk, "max_power"),
                v_max: b.num(blk, "v_max"),
                step_time: b.num(blk, "step_time"),
                motor,
                dynamic_brake: dynamic.map(|d| dynamic_brake_from(b, d)),
            })
        }
        Some("traction-converter") => {
            let blk = b.find("traction-converter").unwrap();
            if b.find("pantograph").is_none() {
                b.issues.push(BakeIssue::warn(Some(blk.id), "bake-no-pantograph"));
            }
            let brake = dynamic.map(|d| dynamic_brake_from(b, d));
            Some(TractionSpec::Converter {
                max_force: b.num(blk, "max_force"),
                max_power: b.num(blk, "max_power"),
                v_max: b.num(blk, "v_max"),
                brake_force: brake.map_or(0.0, |br| br.max_force),
                brake_power: brake.map_or(0.0, |br| br.max_power),
                ramp_time: b.num(blk, "ramp_time"),
                v_pullout: b.num(blk, "v_pullout"),
                regenerative: brake.is_some_and(|br| br.regenerative),
                brake_fade_kmh: brake.map_or(0.0, |br| br.fade_out_kmh),
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
            let transmission = b.find("hydro-transmission").map(|t| Transmission {
                circuits: b.param(t, "circuits").circuits().to_vec(),
                fill_steps: b.num(t, "fill_steps").max(0.0) as u32,
                fill_time: b.num(t, "fill_time"),
                drain_time: b.num(t, "drain_time"),
                hysteresis_kmh: b.num(t, "hysteresis_kmh"),
                final_ratio: b.num(t, "final_ratio"),
                wheel_diameter: b.num(t, "wheel_diameter"),
                count: b.num(t, "count").max(1.0) as u32,
                efficiency: b.num(t, "efficiency"),
            });
            if transmission.is_some() && engine.is_none() {
                b.issues.push(BakeIssue::warn(Some(blk.id), "bake-transmission-needs-map"));
            }
            if transmission.is_some() && b.find("generator").is_some() {
                b.issues.push(BakeIssue::warn(Some(blk.id), "bake-hydro-and-generator"));
            }
            if dynamic.is_some() && b.find("generator").is_none() {
                let id = dynamic.map(|d| d.id);
                b.issues.push(BakeIssue::warn(id, "bake-brake-needs-generator"));
            }
            Some(TractionSpec::Diesel {
                max_force: b.num(blk, "max_force"),
                max_power: b.num(blk, "max_power"),
                v_max: b.num(blk, "v_max"),
                ramp_time: b.num(blk, "ramp_time"),
                start_time: b.num(blk, "start_time"),
                engine,
                transmission,
                hydrodynamic_brake: b.find("retarder").map(|r| HydrodynamicBrake {
                    absorption: b.num(r, "absorption"),
                    ratio: b.num(r, "ratio"),
                    wheel_diameter: b.num(r, "wheel_diameter"),
                    max_force: b.num(r, "max_force"),
                    max_power: b.num(r, "max_power"),
                    fill_time: b.num(r, "fill_time"),
                    fade_out_kmh: b.num(r, "fade_out_kmh"),
                }),
                dynamic_brake: dynamic.map(|d| dynamic_brake_from(b, d)),
            })
        }
        Some(_) => unreachable!(),
    };

    if b.find("series-motor").is_some() && b.find("tap-changer").is_none() {
        let id = b.find("series-motor").map(|m| m.id);
        b.issues.push(BakeIssue::warn(id, "bake-series-motor-unused"));
    }

    // The chains the canvas should show as connected.
    b.expect_wire("diesel-engine", "hydro-transmission");
    b.expect_wire("hydro-transmission", "wheelset");
    b.expect_wire("tap-changer", "series-motor");
    b.expect_wire("series-motor", "wheelset");
    b.expect_wire("traction-converter", "traction-motor");
    b.expect_wire("traction-motor", "wheelset");
    b.expect_wire("pantograph", "main-switch");
    b.expect_wire("main-switch", "transformer");
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
        b.issues.push(BakeIssue::error(None, "bake-no-control-valve"));
        return;
    };
    let Some(cylinder) = b.find("brake-cylinder") else {
        b.issues.push(BakeIssue::error(None, "bake-no-brake-cylinder"));
        return;
    };
    let Some(rigging) = b.find("brake-rigging") else {
        b.issues.push(BakeIssue::error(None, "bake-no-brake-rigging"));
        return;
    };
    let Some(pipe) = b.find("brake-pipe") else {
        b.issues.push(BakeIssue::error(None, "bake-no-brake-pipe"));
        return;
    };
    let aux = b.find("aux-reservoir");
    if aux.is_none() {
        b.issues.push(BakeIssue::warn(Some(cv.id), "bake-no-aux-reservoir"));
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
    let position = match b.param(cv, "position").choice() {
        "g" => BrakePosition::G,
        "r" => BrakePosition::R,
        _ => BrakePosition::P,
    };

    spec.brake = BrakeSpec {
        kind: brake_kind_from(b, rigging),
        position,
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
    };

    if mg.is_some() && !spec.brake.behaviour().rapid_position {
        b.issues.push(BakeIssue::warn(mg.map(|m| m.id), "bake-mg-needs-r"));
    }
    if relay.is_some() && main.is_none() {
        b.issues.push(BakeIssue::warn(relay.map(|r| r.id), "bake-needs-main-reservoir"));
    }
    if direct.is_some() && main.is_none() {
        b.issues.push(BakeIssue::warn(direct.map(|d| d.id), "bake-needs-main-reservoir"));
    }
    if b.find("compressor").is_some() && main.is_none() {
        let id = b.find("compressor").map(|c| c.id);
        b.issues.push(BakeIssue::warn(id, "bake-needs-main-reservoir"));
    }
    if spec.brake.spring_parking && parking.is_some() && main.is_none() {
        b.issues.push(BakeIssue::warn(parking.map(|p| p.id), "bake-needs-main-reservoir"));
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
}

fn bake_equipment(b: &mut Baker, spec: &mut VehicleSpec) {
    let Some(wheelset) = b.find("wheelset") else {
        b.issues.push(BakeIssue::error(None, "bake-no-wheelset"));
        return;
    };
    spec.axles = b.num(wheelset, "axles").clamp(0.0, 255.0) as u8;
    spec.adhesive_mass_fraction = b.num(wheelset, "adhesive_mass_fraction").clamp(0.0, 1.0);

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

    // Wheels first — traction and brakes both end here.
    let wheelset = s.add(reg, "wheelset", 4.0, 1.0);
    s.set_num(wheelset, "axles", spec.axles as f64);
    s.set_num(wheelset, "adhesive_mass_fraction", spec.adhesive_mass_fraction);

    let cab = spec.traction.is_some().then(|| {
        let cab = s.add(reg, "cab", 0.0, 0.0);
        let battery = s.add(reg, "battery", 0.0, 1.0);
        s.wire(battery, "out", cab, "elec");
        cab
    });

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

    match &spec.traction {
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
            dynamic_brake,
        }) => {
            let pantograph = s.add(reg, "pantograph", 0.0, 2.0);
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
            match motor {
                Some(motor) => {
                    let m = s.add(reg, "series-motor", 3.0, 1.0);
                    s.set_num(m, "count", motor.count as f64);
                    s.set_num(m, "resistance", motor.resistance);
                    s.set_num(m, "flux_constant", motor.flux_constant);
                    s.set_num(m, "saturation_current", motor.saturation_current);
                    s.set_num(m, "max_current", motor.max_current);
                    s.set_num(m, "max_voltage", motor.max_voltage);
                    s.set(m, "field_steps", ParamValue::List(motor.field_steps.clone()));
                    s.set_num(m, "gear_ratio", motor.gear_ratio);
                    s.set_num(m, "wheel_diameter", motor.wheel_diameter);
                    s.set_num(m, "efficiency", motor.efficiency);
                    s.wire(tap, "out", m, "elec");
                    s.wire(m, "out", wheelset, "shaft");
                }
                None => {
                    let m = s.add(reg, "traction-motor", 3.0, 1.0);
                    s.wire(tap, "out", m, "elec");
                    s.wire(m, "out", wheelset, "shaft");
                }
            }
            if let Some(brake) = dynamic_brake {
                let d = s.add(reg, "dynamic-brake", 3.0, 2.0);
                synth_dynamic_brake(&mut s, d, brake);
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
        }) => {
            let pantograph = s.add(reg, "pantograph", 0.0, 2.0);
            let main_switch = s.add(reg, "main-switch", 1.0, 2.0);
            let transformer = s.add(reg, "transformer", 2.0, 2.0);
            let conv = s.add(reg, "traction-converter", 2.0, 1.0);
            s.set_num(conv, "max_force", *max_force);
            s.set_num(conv, "max_power", *max_power);
            s.set_num(conv, "v_max", *v_max);
            s.set_num(conv, "ramp_time", *ramp_time);
            s.set_num(conv, "v_pullout", *v_pullout);
            let motor = s.add(reg, "traction-motor", 3.0, 1.0);
            s.wire(pantograph, "out", main_switch, "elec");
            s.wire(main_switch, "out", transformer, "elec");
            s.wire(transformer, "out", conv, "elec");
            s.wire(conv, "out", motor, "elec");
            s.wire(motor, "out", wheelset, "shaft");
            wire_throttle(&mut s, conv, "ctrl");
            if *brake_force > 0.0 {
                let d = s.add(reg, "dynamic-brake", 3.0, 2.0);
                synth_dynamic_brake(
                    &mut s,
                    d,
                    &DynamicBrake {
                        max_force: *brake_force,
                        max_power: *brake_power,
                        fade_out_kmh: *brake_fade_kmh,
                        regenerative: *regenerative,
                        ramp_time: *ramp_time,
                    },
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
                    s.set(eng, "torque_curve", ParamValue::Curve(map.torque_curve.clone()));
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
                    s.set_num(trans, "fill_steps", t.fill_steps as f64);
                    s.set_num(trans, "fill_time", t.fill_time);
                    s.set_num(trans, "drain_time", t.drain_time);
                    s.set_num(trans, "hysteresis_kmh", t.hysteresis_kmh);
                    s.set_num(trans, "final_ratio", t.final_ratio);
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
                None => {
                    if dynamic_brake.is_some() {
                        // Diesel-electric shape: engine → generator → motor.
                        let generator = s.add(reg, "generator", 2.0, 1.0);
                        let motor = s.add(reg, "traction-motor", 3.0, 1.0);
                        s.wire(eng, "out", generator, "shaft");
                        s.wire(generator, "out", motor, "elec");
                        s.wire(motor, "out", wheelset, "shaft");
                        if let Some(brake) = dynamic_brake {
                            let d = s.add(reg, "dynamic-brake", 3.0, 2.0);
                            synth_dynamic_brake(&mut s, d, brake);
                            s.wire(generator, "out", d, "elec");
                        }
                    } else {
                        s.wire(eng, "out", wheelset, "shaft");
                    }
                }
            }
        }
    }

    synth_brakes(&mut s, reg, spec, wheelset, cab);
    synth_equipment(&mut s, reg, spec);
    s.graph
}

fn synth_dynamic_brake(s: &mut Synth, id: u32, brake: &DynamicBrake) {
    s.set_num(id, "max_force", brake.max_force);
    s.set_num(id, "max_power", brake.max_power);
    s.set_num(id, "fade_out_kmh", brake.fade_out_kmh);
    s.set(id, "regenerative", ParamValue::Bool(brake.regenerative));
    s.set_num(id, "ramp_time", brake.ramp_time);
}

fn synth_brakes(s: &mut Synth, reg: &Registry, spec: &VehicleSpec, wheelset: u32, cab: Option<u32>) {
    let brake = &spec.brake;
    let row = 3.0;

    let pipe = s.add(reg, "brake-pipe", 1.0, row);
    s.set_num(pipe, "volume", brake.pipe_volume);
    s.set_num(pipe, "leakage", brake.leakage);

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
            match brake.position {
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
    s.set_num(cylinder, "cylinder_to_reservoir", brake.cylinder_to_reservoir);

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
    if spec.traction.is_some() {
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
        s.set(relay, "supplement", ParamValue::Bool(brake.supplement_brake));
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

    if spec.traction.is_some() {
        let sander = s.add(reg, "sander", 5.0, row + 2.0);
        if let Some(main) = main {
            s.wire(main, "out", sander, "air");
        }
        if let Some(cab) = cab {
            s.wire(cab, "sanding", sander, "ctrl");
        }
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
        s.set(id, "passenger_doors", ParamValue::Bool(spec.passenger_doors));
    }

    if let Some(script) = &spec.script {
        let id = place(s, "script");
        s.set(id, "script", ParamValue::Text(script.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                assert!(matches, "default of {}::{} has the wrong type", def.id, param.id);
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
        assert_eq!(reg.base_kind("example:voith-l620"), Some("hydro-transmission"));
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
            traction: None,
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
        }
    }

    /// The fields the graph owns must survive spec → graph → spec unchanged.
    fn assert_round_trip(spec: &VehicleSpec) {
        let reg = Registry::builtin();
        let graph = from_spec(spec, &reg);
        let mut baked = spec.clone();
        // Scramble the owned fields so the test fails when bake does not write them.
        baked.traction = None;
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
        assert_eq!(baked.traction, spec.traction, "{}", spec.name);
        assert_eq!(baked.brake, spec.brake, "{}", spec.name);
        assert_eq!(baked.safety, spec.safety, "{}", spec.name);
        assert_eq!(baked.doors, spec.doors, "{}", spec.name);
        assert_eq!(baked.passenger_doors, spec.passenger_doors, "{}", spec.name);
        assert_eq!(baked.afb, spec.afb, "{}", spec.name);
        assert_eq!(baked.slip_protection, spec.slip_protection, "{}", spec.name);
        assert_eq!(baked.axles, spec.axles, "{}", spec.name);
        assert_eq!(baked.script, spec.script, "{}", spec.name);
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
        spec.traction = Some(TractionSpec::Curve {
            force: vec![(0.0, 150_000.0), (120.0, 60_000.0)],
            v_max: 120.0,
            brake: vec![(0.0, 0.0), (50.0, 80_000.0)],
            ramp_time: 2.0,
        });
        spec.brake = BrakeSpec::from_brake_weight(70.0, BrakeKind::Block)
            .as_traction_unit(ControlValve::KeTm, 30_000.0);
        spec.adhesive_mass_fraction = 1.0;
        assert_round_trip(&spec);
    }

    #[test]
    fn round_trip_diesel_electric_with_dynamic_brake() {
        let mut spec = test_spec();
        spec.traction = Some(TractionSpec::Diesel {
            max_force: 400_000.0,
            max_power: 2_460_000.0,
            v_max: 120.0,
            ramp_time: 6.0,
            start_time: 10.0,
            engine: None,
            transmission: None,
            hydrodynamic_brake: None,
            dynamic_brake: Some(DynamicBrake {
                max_force: 200_000.0,
                max_power: 2_000_000.0,
                fade_out_kmh: 15.0,
                regenerative: false,
                ramp_time: 4.0,
            }),
        });
        spec.brake = BrakeSpec::from_brake_weight(100.0, BrakeKind::Block)
            .as_traction_unit(ControlValve::KeGp, 60_000.0);
        assert_round_trip(&spec);
    }
}
