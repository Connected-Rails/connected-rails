//! Line source format (RON) and compiler into track network + interlocking (plan ch. 15).

use serde::{Deserialize, Serialize};
use sim_core::interlock::{
    BlockMarkerPayload, Interlock, Route as IlRoute, RouteId, Signal, SignalId, SignalKind,
    SignalSystem,
};
use sim_core::safety::de::{MagnetFrequency, MagnetPayload};
use track_model::{
    DeviceKind, EdgeId, Facing, NodeId, NodeKind, Segment, StepProfile, Switch, SwitchPosition,
    TrackEdge, TrackNetwork, TrackObject, TrackType, TracksideDevice,
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
    /// Track type (`"<mod>:<name>"`, see `track_types/*.ron`) as steps
    /// `(s, name)` — one edge changes its superstructure section by section.
    /// Empty = the default type.
    #[serde(default)]
    pub track_type: Vec<(f64, String)>,
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

/// A scenery object placed relative to the track: a mod's `objects/*.ron`
/// (`"<mod>:<name>"`) at `(edge, s)`. The editor stamps the object's own
/// default offset/rotation/height on placement; the values here are what
/// stands, so a single instance can deviate from its kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjectSource {
    pub object: String,
    pub edge: u32,
    pub s: f64,
    /// Lateral offset [m], positive = right of increasing arc length.
    #[serde(default)]
    pub lateral_offset: f64,
    /// Rotation about the up axis [deg], clockwise seen from above;
    /// 0 = the model's front points along increasing arc length.
    #[serde(default)]
    pub yaw_deg: f64,
    /// Height above the railhead [m].
    #[serde(default)]
    pub height: f64,
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
    /// Scenery objects linked to the track; nothing in the simulation reads
    /// them — they are the line's furniture.
    #[serde(default)]
    pub objects: Vec<ObjectSource>,
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

/// A finding of [`LineSource::check`] — wiring that compiles but fails on the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleIssue {
    /// Device edge index out of range or `s` beyond the edge length.
    DeviceOffEdge { device: u32 },
    /// Magnet payload does not parse or names a signal that does not exist.
    MagnetPayloadInvalid { device: u32 },
    /// Block marker payload does not parse or names a section that does not exist.
    BlockMarkerPayloadInvalid { device: u32 },
    /// Distant (or combination) signal without a 1000 Hz magnet linked to it.
    DistantWithout1000Hz { signal: u32 },
    /// Main (or combination) signal without a 2000 Hz magnet linked to it.
    MainWithout2000Hz { signal: u32 },
    /// Distant signal that announces no signal (`next` missing).
    DistantWithoutNext { signal: u32 },
    /// Signal whose device is missing or not a `Signal` device.
    SignalDeviceMismatch { signal: u32 },
    /// Boundary whose node is missing or not a `Buffer`.
    BoundaryInvalid { boundary: u32 },
    /// Edge names a track type the registry does not know.
    UnknownTrackType { edge: u32 },
    /// Edge uses an LZB track type, but the line places no line conductor.
    LzbTypeWithoutConductor { edge: u32 },
    /// Scenery object outside its track (bad edge index or `s` beyond the length).
    ObjectOffEdge { object: u32 },
    /// Scenery object names an `objects/*.ron` no installed mod has.
    UnknownObject { object: u32 },
}

/// Splits a segment chain at arc length `s`: the segment containing `s` is cut
/// in two (curvature continues through the cut), whole segments stay whole.
fn split_segments(segments: &[Segment], s: f64) -> (Vec<Segment>, Vec<Segment>) {
    let mut first = Vec::new();
    let mut second = Vec::new();
    let mut acc = 0.0;
    for seg in segments {
        if acc >= s {
            second.push(*seg);
        } else if acc + seg.len <= s + 1e-9 {
            first.push(*seg);
        } else {
            let local = s - acc;
            first.push(Segment {
                len: local,
                k0: seg.k0,
                dk: seg.dk,
            });
            second.push(Segment {
                len: seg.len - local,
                k0: seg.k0 + seg.dk * local,
                dk: seg.dk,
            });
        }
        acc += seg.len;
    }
    (first, second)
}

/// Step profile entries of a source edge (`(s, value)`).
type Steps<T> = Vec<(f64, T)>;

/// Splits step profile entries at `s`; the second half starts with the value
/// in force at the cut. Empty stays empty — the edge default applies as before.
fn split_steps<T: Clone>(steps: &[(f64, T)], s: f64) -> (Steps<T>, Steps<T>) {
    if steps.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let value_at = |q: f64| {
        steps
            .iter()
            .filter(|(x, _)| *x <= q)
            .max_by(|a, b| a.0.total_cmp(&b.0))
            // The first entry also applies before its own `s` (StepProfile::new).
            .or_else(|| steps.iter().min_by(|a, b| a.0.total_cmp(&b.0)))
            .map(|(_, v)| v.clone())
            .expect("steps are non-empty")
    };
    let mut first: Vec<(f64, T)> = steps.iter().filter(|(x, _)| *x < s).cloned().collect();
    if first.is_empty() {
        first.push((0.0, value_at(0.0)));
    }
    let mut second = vec![(0.0, value_at(s))];
    second.extend(
        steps
            .iter()
            .filter(|(x, _)| *x > s)
            .map(|(x, v)| (*x - s, v.clone())),
    );
    (first, second)
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
                    let (point, heading_deg) = self.end_anchor(&compiled, index);
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
        // Scenery objects follow their edge; nothing references them by index.
        self.objects.retain(|o| edge_map(o.edge).is_some());
        for o in &mut self.objects {
            o.edge = edge_map(o.edge).expect("kept objects sit on kept edges");
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

    /// Geo anchor (point + compass heading) of the end of compiled edge `index` —
    /// what an edge needs to stand on its own where a `Continue` no longer holds.
    fn end_anchor(&self, compiled: &CompiledLine, index: usize) -> (GeoPoint, f64) {
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
        (point, (90.0 - heading.to_degrees()).rem_euclid(360.0))
    }

    /// Splits edge `index` at arc length `s` into two edges joined by a new
    /// `Joint` node. The second half is appended at the end of the edge list,
    /// so no other edge index moves; devices beyond the cut, step profiles,
    /// switch legs and sections follow, and edges that continued from the old
    /// end are anchored geographically where that end was. Returns
    /// `(joint node, second-half edge index)`.
    ///
    /// Refuses a cut closer than 1 m to either end (a zero-length stub is no
    /// track) and a source that does not compile (nothing to re-anchor against).
    pub fn split_edge(&mut self, index: usize, s: f64) -> Option<(u32, u32)> {
        let length: f64 = self.edges.get(index)?.segments.iter().map(|g| g.len).sum();
        if s < 1.0 || s > length - 1.0 {
            return None;
        }
        // Validity gate only — a source that does not compile cannot re-anchor
        // its followers below.
        self.compile().ok()?;

        let new_index = self.edges.len() as u32;
        let (first, second) = split_segments(&self.edges[index].segments, s);
        let (grade_a, grade_b) = split_steps(&self.edges[index].grade, s);
        let (cant_a, cant_b) = split_steps(&self.edges[index].cant, s);
        let (speed_a, speed_b) = split_steps(&self.edges[index].speed, s);
        let (type_a, type_b) = split_steps(&self.edges[index].track_type, s);

        for d in &mut self.devices {
            if d.edge as usize == index && d.s >= s {
                d.edge = new_index;
                d.s -= s;
            }
        }
        for o in &mut self.objects {
            if o.edge as usize == index && o.s >= s {
                o.edge = new_index;
                o.s -= s;
            }
        }
        // A switch leg attached to the old end now hangs on the second half.
        for node in &mut self.nodes {
            if let NodeSource::Switch {
                root,
                straight,
                diverging,
                ..
            } = node
            {
                for leg in [root, straight, diverging] {
                    if leg.0 as usize == index && leg.1 {
                        leg.0 = new_index;
                    }
                }
            }
        }
        // Both halves stay one occupancy unit — section ids keep their meaning.
        for section in &mut self.sections {
            if section.edges.contains(&(index as u32)) {
                section.edges.push(new_index);
            }
        }

        let joint = self.nodes.len() as u32;
        self.nodes.push(NodeSource::Joint);
        let old_to = self.edges[index].to;
        self.edges[index].to = joint;
        self.edges[index].segments = first;
        self.edges[index].grade = grade_a;
        self.edges[index].cant = cant_a;
        self.edges[index].speed = speed_a;
        self.edges[index].track_type = type_a;
        self.edges.push(EdgeSource {
            from: joint,
            to: old_to,
            start: EdgeStart::Continue { edge: index as u32 },
            segments: second,
            grade: grade_b,
            cant: cant_b,
            speed: speed_b,
            track_type: type_b,
        });

        // Followers continued from the old end, which now belongs to the second
        // half. `Continue { new_index }` would be a forward reference, so they
        // are anchored geographically — at the end the second half has *now*:
        // the cut re-levels the tangent planes, which shifts long edges' ends
        // by the removed curvature-approximation error (sub-metre per km).
        let followers: Vec<usize> = self
            .edges
            .iter()
            .enumerate()
            .filter(|(n, e)| {
                *n != new_index as usize
                    && matches!(e.start, EdgeStart::Continue { edge } if edge as usize == index)
            })
            .map(|(n, _)| n)
            .collect();
        if !followers.is_empty()
            && let Ok(compiled) = self.compile()
        {
            let (point, heading_deg) = self.end_anchor(&compiled, new_index as usize);
            for n in followers {
                self.edges[n].start = EdgeStart::Geo { point, heading_deg };
            }
        }
        Some((joint, new_index))
    }

    /// Rule check of the source file: the wiring mistakes that compile fine
    /// but fail on the line — a distant signal without its 1000 Hz magnet, a
    /// device beyond its track, a boundary on a node that is no buffer, a
    /// track type or scenery object no installed mod has (the registries map
    /// `"<mod>:<name>"` → spec).
    pub fn check(
        &self,
        types: &std::collections::BTreeMap<String, TrackType>,
        objects: &std::collections::BTreeMap<String, TrackObject>,
    ) -> Vec<RuleIssue> {
        let mut issues = Vec::new();

        // Scenery objects: on their track, and of a kind some mod defines.
        let lengths_of = |edge: u32| -> Option<f64> {
            self.edges
                .get(edge as usize)
                .map(|e| e.segments.iter().map(|g| g.len).sum())
        };
        for (i, o) in self.objects.iter().enumerate() {
            let object = i as u32;
            match lengths_of(o.edge) {
                Some(len) if (0.0..=len).contains(&o.s) => {}
                _ => issues.push(RuleIssue::ObjectOffEdge { object }),
            }
            if !objects.contains_key(&o.object) {
                issues.push(RuleIssue::UnknownObject { object });
            }
        }

        // Track types: unknown names, and LZB superstructure whose line
        // conductor was never placed — the type says what belongs on the
        // track, the device is what the LZB actually reads.
        let has_conductor = self
            .devices
            .iter()
            .any(|d| d.kind == DeviceKind::LineConductor);
        for (i, e) in self.edges.iter().enumerate() {
            let edge = i as u32;
            if e.track_type
                .iter()
                .any(|(_, name)| name != "default" && !types.contains_key(name))
            {
                issues.push(RuleIssue::UnknownTrackType { edge });
            }
            if !has_conductor
                && e.track_type
                    .iter()
                    .any(|(_, name)| types.get(name).is_some_and(|t| t.lzb))
            {
                issues.push(RuleIssue::LzbTypeWithoutConductor { edge });
            }
        }
        let lengths: Vec<f64> = self
            .edges
            .iter()
            .map(|e| e.segments.iter().map(|g| g.len).sum())
            .collect();

        let mut magnets: Vec<MagnetPayload> = Vec::new();
        for (i, d) in self.devices.iter().enumerate() {
            let device = i as u32;
            match lengths.get(d.edge as usize) {
                Some(len) if (0.0..=*len).contains(&d.s) => {}
                _ => issues.push(RuleIssue::DeviceOffEdge { device }),
            }
            match d.kind {
                DeviceKind::Magnet => match ron::from_str::<MagnetPayload>(&d.payload) {
                    Ok(p) if p.signal.is_none_or(|g| (g as usize) < self.signals.len()) => {
                        magnets.push(p);
                    }
                    _ => issues.push(RuleIssue::MagnetPayloadInvalid { device }),
                },
                DeviceKind::BlockMarker => match ron::from_str::<BlockMarkerPayload>(&d.payload) {
                    Ok(p) if (p.section as usize) < self.sections.len() => {}
                    _ => issues.push(RuleIssue::BlockMarkerPayloadInvalid { device }),
                },
                _ => {}
            }
        }

        let linked = |signal: u32, frequency: MagnetFrequency| {
            magnets
                .iter()
                .any(|p| p.frequency == frequency && p.signal == Some(signal))
        };
        for (j, sig) in self.signals.iter().enumerate() {
            let signal = j as u32;
            match self.devices.get(sig.device as usize) {
                Some(d) if d.kind == DeviceKind::Signal => {}
                _ => issues.push(RuleIssue::SignalDeviceMismatch { signal }),
            }
            // A Ks combination signal carries both functions, so both magnets.
            if matches!(sig.kind, SignalKind::Distant | SignalKind::Combined)
                && !linked(signal, MagnetFrequency::Hz1000)
            {
                issues.push(RuleIssue::DistantWithout1000Hz { signal });
            }
            if matches!(sig.kind, SignalKind::Main | SignalKind::Combined)
                && !linked(signal, MagnetFrequency::Hz2000)
            {
                issues.push(RuleIssue::MainWithout2000Hz { signal });
            }
            if sig.kind == SignalKind::Distant && sig.next.is_none() {
                issues.push(RuleIssue::DistantWithoutNext { signal });
            }
        }

        for (b, boundary) in self.boundaries.iter().enumerate() {
            match self.nodes.get(boundary.node as usize) {
                Some(NodeSource::Buffer) => {}
                _ => issues.push(RuleIssue::BoundaryInvalid { boundary: b as u32 }),
            }
        }
        issues
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
        // Track-type names are interned per line: index 0 stays the default
        // type — the reserved name `"default"` addresses it, so a section can
        // return to it mid-edge — and the specs behind the other names come
        // from the mod runtime later (`TrackNetwork::apply_track_types`),
        // like signal types.
        let mut type_names: Vec<String> = Vec::new();
        let intern = |names: &mut Vec<String>, name: &str| -> u32 {
            if name == "default" {
                return 0;
            }
            match names.iter().position(|n| n == name) {
                Some(i) => i as u32 + 1,
                None => {
                    names.push(name.to_string());
                    names.len() as u32
                }
            }
        };
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
            if !e.track_type.is_empty() {
                let steps = e
                    .track_type
                    .iter()
                    .map(|(s, name)| (*s, intern(&mut type_names, name)))
                    .collect();
                edge = edge.with_track_type(StepProfile::new(steps));
            }
            edge_ids.push(net.add_edge(edge));
        }
        if !type_names.is_empty() {
            let mut types = vec![TrackType::default()];
            types.extend(type_names.iter().map(|n| TrackType::placeholder(n)));
            net.set_types(types);
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

        // Scenery objects are not compiled into the network — the app places
        // them straight from the source — but a dangling edge index is still
        // a broken file.
        for o in &self.objects {
            if o.edge as usize >= edge_ids.len() {
                return Err(CompileError::UnknownEdge(o.edge));
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

    /// Splitting must be invisible to the geometry: the cut is continuous,
    /// the far end and the follower stay put, devices and sections follow.
    #[test]
    fn splitting_an_edge_keeps_geometry_and_devices() {
        let mut line = musterbahn();
        let before = line.compile().unwrap();
        let cut_pose = before.net.edges()[0].eval(1500.0);
        let end_before = before.net.edges()[0].end_pose().pos;

        let (node, second) = line.split_edge(0, 1500.0).expect("splits");
        assert_eq!(second, 3);
        assert!(matches!(line.nodes[node as usize], NodeSource::Joint));
        let after = line.compile().expect("still compiles");

        let cut = after.net.edges()[3].eval(0.0);
        assert!(cut.pos.distance(cut_pose.pos) < 0.01);
        assert!(cut.tangent.dot(cut_pose.tangent) > 0.999_999);
        // The cut re-levels the tangent planes, so the far end may shift by the
        // curvature-approximation error it removes — sub-metre, nothing more.
        let end_after = after.net.edges()[3].end_pose().pos;
        assert!(end_after.distance(end_before) < 0.5);

        // Devices beyond the cut moved onto the second half, shifted by the cut.
        assert_eq!(line.devices[2].edge, 3, "main signal at km 2.0");
        assert!((line.devices[2].s - 500.0).abs() < 1e-9);
        assert_eq!(line.devices[0].edge, 0, "distant signal at km 1.0 stays");
        // Both halves stay in section 0.
        assert!(line.sections[0].edges.contains(&0) && line.sections[0].edges.contains(&3));
        // The curve continued from edge 0's old end — re-anchored onto the
        // second half's end, so the line stays gapless.
        assert!(matches!(line.edges[1].start, EdgeStart::Geo { .. }));
        assert!(after.net.edges()[1].eval(0.0).pos.distance(end_after) < 0.01);
    }

    /// A cut inside a transition curve keeps the curvature continuous, and the
    /// cant/grade profiles carry the value in force at the cut across it.
    #[test]
    fn splitting_inside_a_clothoid_keeps_curvature_and_profiles() {
        let mut line = musterbahn();
        let before = line.compile().unwrap();
        let cut_pose = before.net.edges()[1].eval(100.0); // mid-transition
        line.split_edge(1, 100.0).expect("splits");
        let after = line.compile().expect("still compiles");
        let cut = after.net.edges()[3].eval(0.0);
        assert!((cut.curvature - cut_pose.curvature).abs() < 1e-12);
        assert!((cut.cant - cut_pose.cant).abs() < 1e-9);
        // The cant ramp's later steps follow, shifted by the cut.
        assert_eq!(line.edges[3].cant[0].0, 0.0);
        assert_eq!(line.edges[3].cant[1], (100.0, 80.0));
        assert_eq!(line.edges[3].cant[2], (700.0, 0.0));
    }

    /// A switch leg that hung on the old edge end follows the second half.
    #[test]
    fn splitting_the_root_edge_rewires_the_switch() {
        let start = GeoPoint {
            lat: 52.0,
            lon: 10.0,
            height: 100.0,
        };
        let mut line = LineSource {
            name: "turnout".into(),
            geoid_offset: 46.0,
            nodes: vec![
                NodeSource::Buffer,
                NodeSource::Switch {
                    root: (0, true),
                    straight: (1, false),
                    diverging: (2, false),
                    throw_time: 6.0,
                },
                NodeSource::Buffer,
                NodeSource::Buffer,
            ],
            edges: vec![
                EdgeSource {
                    from: 0,
                    to: 1,
                    start: EdgeStart::Geo {
                        point: start,
                        heading_deg: 90.0,
                    },
                    segments: vec![Segment::straight(1000.0)],
                    grade: vec![],
                    cant: vec![],
                    speed: vec![],
                    track_type: vec![],
                },
                EdgeSource {
                    from: 1,
                    to: 2,
                    start: EdgeStart::Continue { edge: 0 },
                    segments: vec![Segment::straight(500.0)],
                    grade: vec![],
                    cant: vec![],
                    speed: vec![],
                    track_type: vec![],
                },
                EdgeSource {
                    from: 1,
                    to: 3,
                    start: EdgeStart::Continue { edge: 0 },
                    segments: vec![Segment::arc(300.0, 190.0)],
                    grade: vec![],
                    cant: vec![],
                    speed: vec![],
                    track_type: vec![],
                },
            ],
            devices: vec![],
            objects: vec![],
            sections: vec![],
            signals: vec![],
            routes: vec![],
            boundaries: vec![],
            script: None,
        };
        line.compile().expect("compiles before the split");

        let (_, second) = line.split_edge(0, 400.0).expect("splits");
        assert!(matches!(
            line.nodes[1],
            NodeSource::Switch { root, .. } if root == (second, true)
        ));
        line.compile().expect("still compiles");
    }

    #[test]
    fn split_refuses_the_edge_ends() {
        let mut line = musterbahn();
        assert!(line.split_edge(0, 0.5).is_none());
        assert!(line.split_edge(0, 2999.5).is_none());
        assert!(line.split_edge(7, 10.0).is_none());
    }

    /// The example line is wired correctly; removing its 1000 Hz magnet is the
    /// textbook finding the check exists for.
    #[test]
    fn check_flags_the_missing_1000hz_magnet() {
        let types = std::collections::BTreeMap::new();
        let objects = std::collections::BTreeMap::new();
        let mut line = musterbahn();
        assert!(
            line.check(&types, &objects).is_empty(),
            "{:?}",
            line.check(&types, &objects)
        );
        line.remove_device(1); // the 1000 Hz magnet at the distant signal
        assert_eq!(
            line.check(&types, &objects),
            vec![RuleIssue::DistantWithout1000Hz { signal: 0 }]
        );
    }

    #[test]
    fn check_flags_bad_references() {
        let mut line = musterbahn();
        line.devices[6].s = 9000.0; // platform beyond its edge
        line.devices[1].payload = "(frequency:Hz1000,signal:Some(7))".into();
        line.boundaries.push(BoundarySource {
            name: "mitte".into(),
            node: 1, // a joint, not a buffer
        });
        let issues = line.check(
            &std::collections::BTreeMap::new(),
            &std::collections::BTreeMap::new(),
        );
        assert!(issues.contains(&RuleIssue::DeviceOffEdge { device: 6 }));
        assert!(issues.contains(&RuleIssue::MagnetPayloadInvalid { device: 1 }));
        assert!(issues.contains(&RuleIssue::BoundaryInvalid { boundary: 0 }));
        // The broken magnet no longer counts as the distant signal's 1000 Hz.
        assert!(issues.contains(&RuleIssue::DistantWithout1000Hz { signal: 0 }));
    }

    /// Track types compile into an interned table plus per-edge index
    /// profiles; the specs come from the registry later.
    #[test]
    fn track_types_intern_and_split() {
        let mut line = musterbahn();
        line.edges[0].track_type = vec![(0.0, "ex:hauptbahn".into()), (2500.0, "ex:alt".into())];
        line.edges[2].track_type = vec![(0.0, "ex:hauptbahn".into())];
        let compiled = line.compile().expect("compiles");
        let names: Vec<&str> = compiled
            .net
            .types()
            .iter()
            .map(|t| t.name.as_str())
            .collect();
        assert_eq!(names, ["default", "ex:hauptbahn", "ex:alt"]);
        assert_eq!(compiled.net.edges()[0].track_type.at(0.0), 1);
        assert_eq!(compiled.net.edges()[0].track_type.at(2600.0), 2);
        assert_eq!(compiled.net.edges()[1].track_type.at(0.0), 0);
        assert_eq!(compiled.net.edges()[2].track_type.at(0.0), 1);

        // Splitting carries the type sections across, shifted by the cut.
        line.split_edge(0, 1500.0).expect("splits");
        assert_eq!(line.edges[0].track_type, vec![(0.0, "ex:hauptbahn".into())]);
        assert_eq!(
            line.edges[3].track_type,
            vec![
                (0.0, "ex:hauptbahn".to_string()),
                (1000.0, "ex:alt".to_string())
            ]
        );
    }

    /// Scenery objects follow their edge through splits and removals like
    /// devices do — and the check knows a dangling or unknown one.
    #[test]
    fn objects_follow_split_and_removal() {
        let mast = |edge: u32, s: f64| ObjectSource {
            object: "ex:mast".into(),
            edge,
            s,
            lateral_offset: -3.5,
            yaw_deg: 0.0,
            height: 0.0,
        };
        let mut line = musterbahn();
        line.objects = vec![mast(0, 500.0), mast(0, 2500.0), mast(1, 100.0)];
        line.compile().expect("compiles");

        line.split_edge(0, 1500.0).expect("splits");
        assert_eq!(line.objects[0].edge, 0, "before the cut");
        assert_eq!(line.objects[1].edge, 3, "beyond the cut");
        assert!((line.objects[1].s - 1000.0).abs() < 1e-9);

        // Removing the curve takes its object along; the rest is remapped.
        line.remove_edge(1);
        assert_eq!(line.objects.len(), 2);
        assert_eq!(line.objects[1].edge, 2);
        line.compile().expect("still compiles");

        let types = std::collections::BTreeMap::new();
        let mut objects = std::collections::BTreeMap::new();
        objects.insert(
            "ex:mast".to_string(),
            TrackObject {
                name: "Mast".into(),
                model: "x/assets/mast.gltf".into(),
                lateral_offset: -3.5,
                yaw_deg: 0.0,
                height: 0.0,
            },
        );
        assert!(line.check(&types, &objects).is_empty());
        line.objects[0].s = 99_999.0;
        line.objects[1].object = "ex:fehlt".into();
        let issues = line.check(&types, &objects);
        assert!(issues.contains(&RuleIssue::ObjectOffEdge { object: 0 }));
        assert!(issues.contains(&RuleIssue::UnknownObject { object: 1 }));
    }

    /// The registry-aware rules: unknown names and an LZB superstructure
    /// whose conductor was never placed.
    #[test]
    fn check_flags_track_type_wiring() {
        let mut types = std::collections::BTreeMap::new();
        types.insert(
            "ex:lzb".to_string(),
            TrackType {
                lzb: true,
                ..TrackType::default()
            },
        );
        let objects = std::collections::BTreeMap::new();
        let mut line = musterbahn();
        // The Musterbahn has a line conductor — an LZB type raises nothing.
        line.edges[2].track_type = vec![(0.0, "ex:lzb".into())];
        assert!(line.check(&types, &objects).is_empty());

        line.edges[0].track_type = vec![(0.0, "ex:tippfehler".into())];
        let issues = line.check(&types, &objects);
        assert_eq!(issues, vec![RuleIssue::UnknownTrackType { edge: 0 }]);

        // Without the conductor the LZB type is a promise nothing keeps.
        line.devices.retain(|d| d.kind != DeviceKind::LineConductor);
        let issues = line.check(&types, &objects);
        assert!(issues.contains(&RuleIssue::LzbTypeWithoutConductor { edge: 2 }));
    }
}
