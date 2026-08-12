//! Line source format (RON) and compiler into track network + interlocking (plan ch. 15).

use serde::{Deserialize, Serialize};
use sim_core::interlock::{
    Interlock, Route as IlRoute, RouteId, Signal, SignalId, SignalKind, SignalSystem,
};
use track_model::{
    DeviceKind, EdgeId, Facing, NodeId, NodeKind, Segment, StepProfile, Switch, SwitchPosition,
    TrackEdge, TrackNetwork, TracksideDevice,
};
use track_model::{EdgeEnd, EdgeSide};
use world_coords::geo::to_ecef_deg;

/// Georeferenced start of the line.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GeoPoint {
    pub lat: f64,
    pub lon: f64,
    /// Normal height [m] (DHHN2016).
    pub height: f64,
}

/// Node of the source file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeSource {
    Buffer,
    Joint,
    /// Switch: root/straight/diverging are resolved through the edge indices.
    Switch {
        root: (u32, bool),
        straight: (u32, bool),
        diverging: (u32, bool),
        #[serde(default = "default_throw_time")]
        throw_time: f64,
    },
}

fn default_throw_time() -> f64 {
    6.0
}

/// Where an edge begins.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum EdgeStart {
    /// Georeferenced with heading (0° = north, clockwise).
    Geo { point: GeoPoint, heading_deg: f64 },
    /// Joins the end of an earlier edge.
    Continue { edge: u32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EdgeSource {
    pub from: u32,
    pub to: u32,
    pub start: EdgeStart,
    pub segments: Vec<Segment>,
    /// Gradient [‰] as steps `(s, value)`.
    #[serde(default)]
    pub grade: Vec<(f64, f64)>,
    /// Cant [mm].
    #[serde(default)]
    pub cant: Vec<(f64, f64)>,
    /// Permitted speed [km/h].
    #[serde(default)]
    pub speed: Vec<(f64, f64)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceSource {
    pub kind: DeviceKind,
    pub edge: u32,
    pub s: f64,
    #[serde(default)]
    pub facing: Facing,
    #[serde(default)]
    pub lateral_offset: f64,
    /// Country-specific payload as RON text.
    #[serde(default)]
    pub payload: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SectionSource {
    pub edges: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignalSource {
    pub kind: SignalKind,
    #[serde(default = "default_system")]
    pub system: SignalSystem,
    /// Index into `devices`.
    pub device: u32,
    #[serde(default)]
    pub next: Option<u32>,
    #[serde(default)]
    pub guarded: Vec<u32>,
    #[serde(default)]
    pub requires_route: bool,
    #[serde(default)]
    pub diverging_speed: Option<f64>,
}

fn default_system() -> SignalSystem {
    SignalSystem::Ks
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteSource {
    pub entry: u32,
    pub exit: u32,
    #[serde(default)]
    pub switches: Vec<(u32, SwitchPosition)>,
    #[serde(default)]
    pub sections: Vec<u32>,
    #[serde(default)]
    pub overlap: Vec<u32>,
    #[serde(default)]
    pub diverging: bool,
}

/// A complete line in source form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LineSource {
    pub name: String,
    /// Geoid undulation for the height conversion [m] (plan 4.2).
    #[serde(default = "default_geoid")]
    pub geoid_offset: f64,
    pub nodes: Vec<NodeSource>,
    pub edges: Vec<EdgeSource>,
    #[serde(default)]
    pub devices: Vec<DeviceSource>,
    #[serde(default)]
    pub sections: Vec<SectionSource>,
    #[serde(default)]
    pub signals: Vec<SignalSource>,
    #[serde(default)]
    pub routes: Vec<RouteSource>,
}

fn default_geoid() -> f64 {
    46.0
}

/// Result of the compilation.
pub struct CompiledLine {
    pub net: TrackNetwork,
    pub interlock: Interlock,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    UnknownEdge(u32),
    UnknownNode(u32),
    UnknownDevice(u32),
    /// An edge refers to an edge that has not been compiled yet.
    ForwardReference(u32),
}

impl LineSource {
    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }

    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).expect("serializable")
    }

    /// Compiles the source file into track network and interlocking.
    pub fn compile(&self) -> Result<CompiledLine, CompileError> {
        let mut net = TrackNetwork::new();

        // Nodes first (switches get their edge ends later).
        let node_ids: Vec<NodeId> = self
            .nodes
            .iter()
            .map(|n| {
                net.add_node(match n {
                    NodeSource::Buffer => NodeKind::Buffer,
                    NodeSource::Joint | NodeSource::Switch { .. } => NodeKind::Joint,
                })
            })
            .collect();

        // Edges in source order; `Continue` may only refer backwards.
        let mut edge_ids: Vec<EdgeId> = Vec::new();
        for (i, e) in self.edges.iter().enumerate() {
            let (anchor, heading) = match e.start {
                EdgeStart::Geo { point, heading_deg } => (
                    to_ecef_deg(
                        point.lat,
                        point.lon,
                        world_coords::geo::ellipsoidal_height(point.height, self.geoid_offset),
                    ),
                    // Source data gives the heading as a compass bearing, internally
                    // 0 = east and mathematically positive.
                    (90.0 - heading_deg).to_radians(),
                ),
                EdgeStart::Continue { edge } => {
                    let prev = *edge_ids
                        .get(edge as usize)
                        .ok_or(CompileError::ForwardReference(edge))?;
                    if edge as usize >= i {
                        return Err(CompileError::ForwardReference(edge));
                    }
                    let prev_edge = net.edge(prev);
                    let end = prev_edge.end_pose();
                    let heading: f64 = prev_edge.heading0
                        + prev_edge
                            .segments
                            .iter()
                            .map(|s| s.heading_delta(s.len))
                            .sum::<f64>();
                    // The joint gets its own ENU frame; the heading is the same in the
                    // new frame, because ENU frames are only rotated against each other
                    // over long distances (meridian convergence, negligible here).
                    (end.pos, heading)
                }
            };

            let from = *node_ids
                .get(e.from as usize)
                .ok_or(CompileError::UnknownNode(e.from))?;
            let to = *node_ids
                .get(e.to as usize)
                .ok_or(CompileError::UnknownNode(e.to))?;
            let mut edge = TrackEdge::new(EdgeId(0), from, to, anchor, heading, e.segments.clone());
            if !e.grade.is_empty() {
                edge = edge.with_grade(StepProfile::new(e.grade.clone()));
            }
            if !e.cant.is_empty() {
                edge = edge.with_cant(StepProfile::new(e.cant.clone()));
            }
            if !e.speed.is_empty() {
                edge = edge.with_speed(StepProfile::new(e.speed.clone()));
            }
            edge_ids.push(net.add_edge(edge));
        }

        // Wire up the switches.
        for (i, n) in self.nodes.iter().enumerate() {
            if let NodeSource::Switch {
                root,
                straight,
                diverging,
                throw_time,
            } = n
            {
                let resolve = |(edge, at_end): (u32, bool)| -> Result<EdgeEnd, CompileError> {
                    let id = *edge_ids
                        .get(edge as usize)
                        .ok_or(CompileError::UnknownEdge(edge))?;
                    Ok(EdgeEnd::new(
                        id,
                        if at_end {
                            EdgeSide::End
                        } else {
                            EdgeSide::Start
                        },
                    ))
                };
                let mut sw =
                    Switch::new(resolve(*root)?, resolve(*straight)?, resolve(*diverging)?);
                sw.throw_time = *throw_time;
                net.node_mut(node_ids[i]).kind = NodeKind::Switch(sw);
            }
        }

        // Trackside devices.
        let mut device_ids = Vec::new();
        for d in &self.devices {
            let edge = *edge_ids
                .get(d.edge as usize)
                .ok_or(CompileError::UnknownEdge(d.edge))?;
            let mut device = TracksideDevice::new(d.kind.clone(), edge, d.s);
            device.facing = d.facing;
            device.lateral_offset = d.lateral_offset;
            if !d.payload.is_empty() {
                device.payload = d.payload.clone();
            }
            device_ids.push(net.add_device(device));
        }

        // Interlocking.
        let mut interlock = Interlock::new();
        for s in &self.sections {
            let edges = s
                .edges
                .iter()
                .map(|e| {
                    edge_ids
                        .get(*e as usize)
                        .copied()
                        .ok_or(CompileError::UnknownEdge(*e))
                })
                .collect::<Result<Vec<_>, _>>()?;
            interlock.add_section(edges);
        }
        for s in &self.signals {
            let device = *device_ids
                .get(s.device as usize)
                .ok_or(CompileError::UnknownDevice(s.device))?;
            let mut signal = Signal::new(SignalId(0), s.kind, device);
            signal.system = s.system;
            signal.next = s.next.map(SignalId);
            signal.guarded = s
                .guarded
                .iter()
                .map(|g| sim_core::interlock::SectionId(*g))
                .collect();
            signal.requires_route = s.requires_route;
            signal.diverging_speed = s.diverging_speed;
            interlock.add_signal(signal);
        }
        for r in &self.routes {
            let mut route = IlRoute::new(RouteId(0), SignalId(r.entry), SignalId(r.exit));
            route.switches = r
                .switches
                .iter()
                .map(|(n, p)| {
                    node_ids
                        .get(*n as usize)
                        .copied()
                        .map(|id| (id, *p))
                        .ok_or(CompileError::UnknownNode(*n))
                })
                .collect::<Result<Vec<_>, _>>()?;
            route.sections = r
                .sections
                .iter()
                .map(|s| sim_core::interlock::SectionId(*s))
                .collect();
            route.overlap = r
                .overlap
                .iter()
                .map(|s| sim_core::interlock::SectionId(*s))
                .collect();
            route.diverging = r.diverging;
            interlock.add_route(route);
        }

        Ok(CompiledLine { net, interlock })
    }
}
