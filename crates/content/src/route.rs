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
    /// Signal type from a mod (`"<mod>:<name>"`) — the aspect then comes from that rule
    /// table instead of the built-in logic. Resolved by the mod runtime (plan ch. 19).
    #[serde(default)]
    pub signal_type: Option<String>,
    /// 3D model override (`"<mod>:<name>"` below `signal_models/`) — wins over the
    /// signal type's default model.
    #[serde(default)]
    pub model: Option<String>,
}

fn default_system() -> SignalSystem {
    SignalSystem::Ks
}

/// Named connection point of a module: a `Buffer` node at which another module may attach
/// (plan ch. 15; the composition is in [`crate::compose`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoundarySource {
    pub name: String,
    /// Index into `nodes`; must be a `Buffer` at the open end of an edge.
    pub node: u32,
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
    /// Connection points for the module composition; a line that is never composed
    /// simply has none.
    #[serde(default)]
    pub boundaries: Vec<BoundarySource>,
    /// Optional Lua script hook (plan 19.7), named `"<mod>:<file stem>"`.
    #[serde(default)]
    pub script: Option<String>,
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

    /// Removes device `index`. Signals on it disappear with it; every other
    /// device and signal index in the file is remapped.
    pub fn remove_device(&mut self, index: usize) {
        if index >= self.devices.len() {
            return;
        }
        self.devices.remove(index);
        let removed = index as u32;
        let removed_signals: Vec<u32> = self
            .signals
            .iter()
            .enumerate()
            .filter(|(_, s)| s.device == removed)
            .map(|(n, _)| n as u32)
            .collect();
        self.signals.retain(|s| s.device != removed);
        for s in &mut self.signals {
            if s.device > removed {
                s.device -= 1;
            }
        }
        self.drop_signal_refs(&removed_signals);
    }

    /// Removes edge `index` together with the devices on it. An edge that
    /// continued from the removed one is anchored geographically first, so its
    /// geometry stays where it was; a switch that loses a leg degrades to a
    /// joint. Sections keep their (possibly empty) slot — section ids stay
    /// valid that way.
    pub fn remove_edge(&mut self, index: usize) {
        if index >= self.edges.len() {
            return;
        }
        let removed = index as u32;
        let mut removed_edges = vec![removed];

        // Re-anchor followers while the end pose still exists. A source that
        // does not compile has no pose to give — the followers go as well.
        let followers: Vec<usize> = self
            .edges
            .iter()
            .enumerate()
            .filter(|(_, e)| matches!(e.start, EdgeStart::Continue { edge } if edge == removed))
            .map(|(n, _)| n)
            .collect();
        if !followers.is_empty() {
            match self.compile() {
                Ok(compiled) => {
                    let edge = &compiled.net.edges()[index];
                    let end = edge.end_pose().pos;
                    let heading: f64 = edge.heading0
                        + edge
                            .segments
                            .iter()
                            .map(|s| s.heading_delta(s.len))
                            .sum::<f64>();
                    let (lat, lon, height) = world_coords::geo::from_ecef(end);
                    let point = GeoPoint {
                        lat: lat.to_degrees(),
                        lon: lon.to_degrees(),
                        height: height - self.geoid_offset,
                    };
                    let heading_deg = (90.0 - heading.to_degrees()).rem_euclid(360.0);
                    for n in followers {
                        self.edges[n].start = EdgeStart::Geo { point, heading_deg };
                    }
                }
                Err(_) => {
                    let mut grew = true;
                    while grew {
                        grew = false;
                        for (n, e) in self.edges.iter().enumerate() {
                            if !removed_edges.contains(&(n as u32))
                                && matches!(e.start, EdgeStart::Continue { edge }
                                    if removed_edges.contains(&edge))
                            {
                                removed_edges.push(n as u32);
                                grew = true;
                            }
                        }
                    }
                }
            }
        }
        removed_edges.sort_unstable();
        let edge_map = |old: u32| -> Option<u32> {
            (!removed_edges.contains(&old))
                .then(|| old - removed_edges.iter().filter(|&&r| r < old).count() as u32)
        };

        // Devices on removed edges go, the rest move down — and the signals
        // and routes that referenced them follow.
        let removed_devices: Vec<u32> = self
            .devices
            .iter()
            .enumerate()
            .filter(|(_, d)| edge_map(d.edge).is_none())
            .map(|(n, _)| n as u32)
            .collect();
        self.devices.retain(|d| edge_map(d.edge).is_some());
        for d in &mut self.devices {
            d.edge = edge_map(d.edge).expect("kept devices sit on kept edges");
        }
        let removed_signals: Vec<u32> = self
            .signals
            .iter()
            .enumerate()
            .filter(|(_, s)| removed_devices.contains(&s.device))
            .map(|(n, _)| n as u32)
            .collect();
        self.signals
            .retain(|s| !removed_devices.contains(&s.device));
        for s in &mut self.signals {
            s.device -= removed_devices.iter().filter(|&&r| r < s.device).count() as u32;
        }
        self.drop_signal_refs(&removed_signals);

        for section in &mut self.sections {
            section.edges.retain(|&e| edge_map(e).is_some());
            for e in &mut section.edges {
                *e = edge_map(*e).expect("kept section edges are kept edges");
            }
        }
        for node in &mut self.nodes {
            if let NodeSource::Switch {
                root,
                straight,
                diverging,
                ..
            } = node
            {
                match (
                    edge_map(root.0),
                    edge_map(straight.0),
                    edge_map(diverging.0),
                ) {
                    (Some(r), Some(s), Some(d)) => {
                        root.0 = r;
                        straight.0 = s;
                        diverging.0 = d;
                    }
                    _ => *node = NodeSource::Joint,
                }
            }
        }

        let mut n = 0u32;
        self.edges.retain(|_| {
            let keep = !removed_edges.contains(&n);
            n += 1;
            keep
        });
        for e in &mut self.edges {
            if let EdgeStart::Continue { edge } = &mut e.start {
                *edge = edge_map(*edge).expect("followers were re-anchored or removed");
            }
        }
    }

    /// Removes the given signal indices from every cross reference: routes on
    /// them disappear, `next` links onto them are cleared, the rest are
    /// remapped. `self.signals` itself must already be filtered.
    fn drop_signal_refs(&mut self, removed: &[u32]) {
        if removed.is_empty() {
            return;
        }
        let map = |old: u32| -> Option<u32> {
            (!removed.contains(&old))
                .then(|| old - removed.iter().filter(|&&r| r < old).count() as u32)
        };
        for s in &mut self.signals {
            s.next = s.next.and_then(map);
        }
        self.routes
            .retain(|r| map(r.entry).is_some() && map(r.exit).is_some());
        for r in &mut self.routes {
            r.entry = map(r.entry).expect("kept routes reference kept signals");
            r.exit = map(r.exit).expect("kept routes reference kept signals");
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::musterbahn;

    /// Removing the curve must not move the climb behind it: the follower is
    /// anchored at the exact coordinates the removed edge ended at.
    #[test]
    fn removing_a_middle_edge_keeps_the_follower_in_place() {
        let mut line = musterbahn();
        let before = line.compile().unwrap();
        let expected = before.net.edges()[2].eval(0.0).pos;
        let expected_dir = before.net.edges()[2].eval(0.0).tangent;

        line.remove_edge(1);
        let after = line.compile().expect("still compiles");
        assert_eq!(line.edges.len(), 2);
        assert!(matches!(line.edges[1].start, EdgeStart::Geo { .. }));
        let start = after.net.edges()[1].eval(0.0);
        assert!(
            start.pos.distance(expected) < 0.01,
            "follower moved by {} m",
            start.pos.distance(expected)
        );
        assert!(start.tangent.dot(expected_dir) > 0.999_999);

        // The curve's devices (line conductor, block marker) went with it;
        // everything else moved down one edge index.
        assert_eq!(line.devices.len(), 7);
        assert!(line.devices.iter().all(|d| d.edge <= 1));
        assert_eq!(line.signals.len(), 2);
    }

    /// Devices carry signals, signals carry links — the whole chain follows.
    #[test]
    fn removing_the_first_edge_drops_its_signals() {
        let mut line = musterbahn();
        line.remove_edge(0);
        line.compile().expect("still compiles");
        assert_eq!(line.edges.len(), 2);
        assert!(line.signals.is_empty());
        assert_eq!(line.devices.len(), 4);
        assert!(matches!(line.edges[0].start, EdgeStart::Geo { .. }));
    }

    #[test]
    fn removing_a_device_remaps_the_signal_table() {
        let mut line = musterbahn();
        line.remove_device(0); // the distant signal's device
        line.compile().expect("still compiles");
        assert_eq!(line.signals.len(), 1);
        assert_eq!(line.signals[0].kind, SignalKind::Main);
        assert_eq!(line.signals[0].device, 1);
    }

    /// The distant signal announced the main one; when the main goes, the
    /// `next` link must not dangle.
    #[test]
    fn removing_the_main_signal_clears_the_distant_link() {
        let mut line = musterbahn();
        line.remove_device(2);
        line.compile().expect("still compiles");
        assert_eq!(line.signals.len(), 1);
        assert_eq!(line.signals[0].kind, SignalKind::Distant);
        assert_eq!(line.signals[0].next, None);
    }
}
