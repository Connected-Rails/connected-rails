//! Composing georeferenced line modules into one line (plan ch. 15, after the Zusi 3
//! module model).
//!
//! A **module** is an ordinary [`LineSource`] with named [`boundaries`] — `Buffer` nodes at
//! the open ends where another module may attach. A **composition** lists modules by
//! reference and merges them into one line: every index space (nodes, edges, devices,
//! sections, signals, routes — including the indices inside magnet, signal and block
//! marker payloads) is shifted by the module's offset, and connected boundary nodes are
//! fused into one `Joint`.
//!
//! **Connections come from the georeference.** Two boundaries that lie at the same
//! position (within [`SNAP_DISTANCE`]) connect by themselves — a module builder places
//! the module edge at the agreed coordinates and is done. `connections` states a pairing
//! explicitly where that is not wanted or not possible.
//!
//! Several versions of a module — other epochs, other equipment — are simply several
//! module files; the composition picks one by name.
//!
//! [`boundaries`]: LineSource::boundaries

use crate::route::{EdgeStart, LineSource, NodeSource};
use serde::{Deserialize, Serialize};
use sim_core::interlock::{Activation, BlockMarkerPayload};
use sim_core::safety::de::MagnetPayload;
use std::collections::BTreeMap;
use track_model::{DeviceKind, EdgeId};

/// How close two module boundaries have to lie to snap together [m].
///
/// ponytail: a constant — module edges are placed at agreed coordinates, centimetres
/// apart. A per-composition tolerance field steps in when real data needs one.
pub const SNAP_DISTANCE: f64 = 1.0;

/// A line composed of georeferenced modules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Composition {
    pub name: String,
    /// Module references (`"<mod>:<line>"`), each once.
    pub modules: Vec<String>,
    /// Explicit connections `((module, boundary), (module, boundary))` on top of the
    /// automatic pairing by position.
    #[serde(default)]
    pub connections: Vec<((String, String), (String, String))>,
    /// Cross-module distant signalling `((module, signal), (module, following signal))`:
    /// sets the first signal's `next` to the second — what a module cannot state itself,
    /// because its `next` is a module-local index.
    #[serde(default)]
    pub signal_links: Vec<((String, u32), (String, u32))>,
    /// Optional Lua script hook of the composed line, named `"<mod>:<file stem>"`.
    #[serde(default)]
    pub script: Option<String>,
}

/// Result of composing: the merged line plus the per-module index offsets that
/// module-qualified content (timetables, scenario events) is resolved against.
#[derive(Debug, Clone)]
pub struct Composed {
    pub line: LineSource,
    pub offsets: BTreeMap<String, ModuleOffsets>,
    /// Worth logging: per-module offsets, boundaries that stayed open, dropped scripts.
    pub notes: Vec<String>,
}

/// Per-module index offsets in the composed line — what a timetable or scenario written
/// against the composition has to add to a module-local index.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModuleOffsets {
    pub nodes: u32,
    pub edges: u32,
    pub devices: u32,
    pub sections: u32,
    pub signals: u32,
    pub routes: u32,
}

/// The `signal`/`activation` payload of a `DeviceKind::Signal` device.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct SignalDevicePayload {
    #[serde(default)]
    signal: Option<u32>,
    #[serde(default)]
    activation: Activation,
}

impl Composition {
    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }

    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).expect("serializable")
    }

    /// Merges the referenced modules into one line.
    ///
    /// A missing module, a module that does not compile, or a broken link is an error —
    /// half a composition is no line.
    pub fn compose(&self, lines: &BTreeMap<String, LineSource>) -> Result<Composed, String> {
        let mut notes = Vec::new();

        // Resolve the modules; each may appear once (the same module twice would mean
        // the same place twice — a copy under another name is the way to reuse one).
        let mut modules: Vec<(&str, &LineSource)> = Vec::new();
        for name in &self.modules {
            if modules.iter().any(|(n, _)| n == name) {
                return Err(format!("module {name} listed twice"));
            }
            let line = lines
                .get(name)
                .ok_or_else(|| format!("module {name} not found"))?;
            modules.push((name, line));
        }
        if modules.is_empty() {
            return Err("composition without modules".into());
        }

        // Boundary positions come from the compiled geometry of each module on its own.
        let mut boundaries: Vec<Boundary> = Vec::new();
        let mut offsets: BTreeMap<String, ModuleOffsets> = BTreeMap::new();
        let mut signal_counts: BTreeMap<String, u32> = BTreeMap::new();
        let mut merged = LineSource {
            name: self.name.clone(),
            geoid_offset: modules[0].1.geoid_offset,
            nodes: Vec::new(),
            edges: Vec::new(),
            devices: Vec::new(),
            sections: Vec::new(),
            signals: Vec::new(),
            routes: Vec::new(),
            boundaries: Vec::new(),
            script: self.script.clone(),
        };

        for (index, (name, module)) in modules.iter().enumerate() {
            let compiled = module
                .compile()
                .map_err(|e| format!("module {name}: {e:?}"))?;
            let off = ModuleOffsets {
                nodes: merged.nodes.len() as u32,
                edges: merged.edges.len() as u32,
                devices: merged.devices.len() as u32,
                sections: merged.sections.len() as u32,
                signals: merged.signals.len() as u32,
                routes: merged.routes.len() as u32,
            };
            notes.push(format!(
                "module {name}: nodes +{}, edges +{}, devices +{}, sections +{}, signals +{}, routes +{}",
                off.nodes, off.edges, off.devices, off.sections, off.signals, off.routes
            ));
            offsets.insert(name.to_string(), off);
            signal_counts.insert(name.to_string(), module.signals.len() as u32);

            if module.geoid_offset != merged.geoid_offset {
                notes.push(format!(
                    "module {name}: geoid offset {} differs from {} of the first module",
                    module.geoid_offset, merged.geoid_offset
                ));
            }
            if let Some(script) = &module.script {
                if merged.script.is_none() {
                    merged.script = Some(script.clone());
                } else {
                    notes.push(format!(
                        "module {name}: script {script} dropped — the composed line already has one"
                    ));
                }
            }

            for b in &module.boundaries {
                match boundary_position(module, &compiled, b.node) {
                    Some(pos) => boundaries.push(Boundary {
                        module: index,
                        name: b.name.clone(),
                        node: off.nodes + b.node,
                        pos,
                        connected: false,
                    }),
                    None => notes.push(format!(
                        "module {name}: boundary {:?} has no edge — ignored",
                        b.name
                    )),
                }
            }

            merge_module(&mut merged, module, off);
        }

        // Explicit connections first, then everything that lies on the same spot.
        let mut joins: Vec<(u32, u32)> = Vec::new();
        for ((mod_a, bnd_a), (mod_b, bnd_b)) in &self.connections {
            let a = find_boundary(&boundaries, &self.modules, mod_a, bnd_a)?;
            let b = find_boundary(&boundaries, &self.modules, mod_b, bnd_b)?;
            if boundaries[a].connected || boundaries[b].connected || a == b {
                return Err(format!(
                    "connection {mod_a}:{bnd_a} — {mod_b}:{bnd_b} reuses a boundary"
                ));
            }
            boundaries[a].connected = true;
            boundaries[b].connected = true;
            joins.push((boundaries[a].node, boundaries[b].node));
        }
        for a in 0..boundaries.len() {
            if boundaries[a].connected {
                continue;
            }
            let near = (a + 1..boundaries.len()).find(|&b| {
                !boundaries[b].connected
                    && boundaries[b].module != boundaries[a].module
                    && boundaries[a].pos.distance(boundaries[b].pos) <= SNAP_DISTANCE
            });
            if let Some(b) = near {
                boundaries[a].connected = true;
                boundaries[b].connected = true;
                joins.push((boundaries[a].node, boundaries[b].node));
            }
        }
        for b in boundaries.iter().filter(|b| !b.connected) {
            notes.push(format!(
                "boundary {}:{} stays open",
                self.modules[b.module], b.name
            ));
        }
        if joins.is_empty() && modules.len() > 1 {
            notes.push("no module connections — the modules stand apart".into());
        }

        // Fuse each pair: the second node's edges move over, the first becomes a joint.
        // The second node stays behind as an unused buffer — an index shift would touch
        // every reference for no gain.
        for (keep, drop) in joins {
            for e in &mut merged.edges {
                if e.from == drop {
                    e.from = keep;
                }
                if e.to == drop {
                    e.to = keep;
                }
            }
            merged.nodes[keep as usize] = NodeSource::Joint;
        }

        // Cross-module distant signalling: the composition sets `next` where a module
        // cannot — evaluated in signalling order like every other chain.
        for ((mod_a, sig_a), (mod_b, sig_b)) in &self.signal_links {
            let a = global_signal(&offsets, &signal_counts, mod_a, *sig_a)?;
            let b = global_signal(&offsets, &signal_counts, mod_b, *sig_b)?;
            merged.signals[a as usize].next = Some(b);
        }

        Ok(Composed {
            line: merged,
            offsets,
            notes,
        })
    }
}

/// Global signal index of `(module, local signal index)`.
fn global_signal(
    offsets: &BTreeMap<String, ModuleOffsets>,
    signal_counts: &BTreeMap<String, u32>,
    module: &str,
    signal: u32,
) -> Result<u32, String> {
    let off = offsets
        .get(module)
        .ok_or_else(|| format!("signal link names unknown module {module}"))?;
    if signal >= signal_counts[module] {
        return Err(format!(
            "signal link: module {module} has no signal {signal}"
        ));
    }
    Ok(off.signals + signal)
}

struct Boundary {
    module: usize,
    name: String,
    /// Node index in the composed line.
    node: u32,
    pos: glam::DVec3,
    connected: bool,
}

fn find_boundary(
    boundaries: &[Boundary],
    modules: &[String],
    module: &str,
    name: &str,
) -> Result<usize, String> {
    boundaries
        .iter()
        .position(|b| modules[b.module] == module && b.name == name)
        .ok_or_else(|| format!("connection names unknown boundary {module}:{name}"))
}

/// World position of a boundary node, from the module compiled on its own.
fn boundary_position(
    module: &LineSource,
    compiled: &crate::route::CompiledLine,
    node: u32,
) -> Option<glam::DVec3> {
    for (i, e) in module.edges.iter().enumerate() {
        if e.from == node {
            return Some(compiled.net.edge(EdgeId(i as u32)).anchor.0);
        }
        if e.to == node {
            return Some(compiled.net.edge(EdgeId(i as u32)).end_pose().pos.0);
        }
    }
    None
}

/// Appends `module` to `merged` with every index shifted by `off`.
fn merge_module(merged: &mut LineSource, module: &LineSource, off: ModuleOffsets) {
    for n in &module.nodes {
        merged.nodes.push(match n {
            NodeSource::Switch {
                root,
                straight,
                diverging,
                throw_time,
            } => NodeSource::Switch {
                root: (root.0 + off.edges, root.1),
                straight: (straight.0 + off.edges, straight.1),
                diverging: (diverging.0 + off.edges, diverging.1),
                throw_time: *throw_time,
            },
            other => other.clone(),
        });
    }
    for e in &module.edges {
        let mut e = e.clone();
        e.from += off.nodes;
        e.to += off.nodes;
        if let EdgeStart::Continue { edge } = &mut e.start {
            *edge += off.edges;
        }
        merged.edges.push(e);
    }
    for d in &module.devices {
        let mut d = d.clone();
        d.edge += off.edges;
        d.payload = shift_payload(&d.kind, &d.payload, off);
        merged.devices.push(d);
    }
    for s in &module.sections {
        let mut s = s.clone();
        for e in &mut s.edges {
            *e += off.edges;
        }
        merged.sections.push(s);
    }
    for s in &module.signals {
        let mut s = s.clone();
        s.device += off.devices;
        if let Some(next) = &mut s.next {
            *next += off.signals;
        }
        for g in &mut s.guarded {
            *g += off.sections;
        }
        merged.signals.push(s);
    }
    for r in &module.routes {
        let mut r = r.clone();
        r.entry += off.signals;
        r.exit += off.signals;
        for (n, _) in &mut r.switches {
            *n += off.nodes;
        }
        for s in &mut r.sections {
            *s += off.sections;
        }
        for s in &mut r.overlap {
            *s += off.sections;
        }
        merged.routes.push(r);
    }
}

/// Shifts the indices inside a device payload. Payload kinds without indices, empty
/// payloads and text that does not parse pass through unchanged — the compile of the
/// module already ran, so unparsable text is the module's own business.
fn shift_payload(kind: &DeviceKind, payload: &str, off: ModuleOffsets) -> String {
    if payload.is_empty() {
        return payload.into();
    }
    match kind {
        DeviceKind::Magnet => match ron::from_str::<MagnetPayload>(payload) {
            Ok(mut p) => {
                p.signal = p.signal.map(|s| s + off.signals);
                ron::to_string(&p).expect("serializable")
            }
            Err(_) => payload.into(),
        },
        DeviceKind::Signal => match ron::from_str::<SignalDevicePayload>(payload) {
            Ok(mut p) => {
                p.signal = p.signal.map(|s| s + off.signals);
                ron::to_string(&p).expect("serializable")
            }
            Err(_) => payload.into(),
        },
        DeviceKind::BlockMarker => match ron::from_str::<BlockMarkerPayload>(payload) {
            Ok(mut p) => {
                p.section += off.sections;
                ron::to_string(&p).expect("serializable")
            }
            Err(_) => payload.into(),
        },
        _ => payload.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::{
        BoundarySource, DeviceSource, EdgeSource, GeoPoint, SectionSource, SignalSource,
    };
    use sim_core::interlock::{SignalKind, SignalSystem};
    use track_model::{Segment, TrackPosition};
    use world_coords::geo::{from_ecef, from_utm, to_utm};

    /// A 2 km straight module from `start`, with one signal at 1.8 km, its 2000 Hz magnet,
    /// one section and boundaries at both ends.
    fn module(start: GeoPoint, heading_deg: f64, geoid: f64) -> LineSource {
        LineSource {
            name: "module".into(),
            geoid_offset: geoid,
            nodes: vec![NodeSource::Buffer, NodeSource::Buffer],
            edges: vec![EdgeSource {
                from: 0,
                to: 1,
                start: EdgeStart::Geo {
                    point: start,
                    heading_deg,
                },
                segments: vec![Segment::straight(2000.0)],
                grade: vec![],
                cant: vec![],
                speed: vec![(0.0, 120.0)],
            }],
            devices: vec![
                DeviceSource {
                    kind: DeviceKind::Signal,
                    edge: 0,
                    s: 1800.0,
                    facing: Default::default(),
                    lateral_offset: 3.0,
                    payload: "(signal:Some(0))".into(),
                },
                DeviceSource {
                    kind: DeviceKind::Magnet,
                    edge: 0,
                    s: 1800.0,
                    facing: Default::default(),
                    lateral_offset: 0.0,
                    payload: ron::to_string(&MagnetPayload::hz2000(0)).unwrap(),
                },
            ],
            sections: vec![SectionSource { edges: vec![0] }],
            signals: vec![SignalSource {
                kind: SignalKind::Main,
                system: SignalSystem::Ks,
                device: 0,
                next: None,
                guarded: vec![0],
                requires_route: false,
                diverging_speed: None,
                signal_type: None,
                model: None,
            }],
            routes: vec![],
            boundaries: vec![
                BoundarySource {
                    name: "start".into(),
                    node: 0,
                },
                BoundarySource {
                    name: "end".into(),
                    node: 1,
                },
            ],
            script: None,
        }
    }

    /// Geo point of the far end of `module`, exactly as the compiler sees it.
    fn end_point(module: &LineSource) -> GeoPoint {
        let compiled = module.compile().expect("module compiles");
        let end = compiled.net.edge(EdgeId(0)).end_pose().pos;
        let (lat, lon, h) = from_ecef(end);
        GeoPoint {
            lat: lat.to_degrees(),
            lon: lon.to_degrees(),
            height: h - module.geoid_offset,
        }
    }

    fn two_modules() -> (Composition, BTreeMap<String, LineSource>) {
        let west = module(
            GeoPoint {
                lat: 52.0,
                lon: 10.0,
                height: 100.0,
            },
            90.0,
            46.0,
        );
        let ost = module(end_point(&west), 90.0, 46.0);
        let mut lines = BTreeMap::new();
        lines.insert("test:west".to_string(), west);
        lines.insert("test:ost".to_string(), ost);
        let composition = Composition {
            name: "Gesamt".into(),
            modules: vec!["test:west".into(), "test:ost".into()],
            connections: vec![],
            signal_links: vec![],
            script: None,
        };
        (composition, lines)
    }

    #[test]
    fn boundaries_snap_by_position_and_fuse() {
        let (composition, lines) = two_modules();
        let Composed {
            line: merged,
            notes,
            ..
        } = composition.compose(&lines).expect("composes");

        // West's end node and ost's start node are one joint now.
        assert_eq!(merged.edges[1].from, merged.edges[0].to);
        assert_eq!(merged.nodes[merged.edges[0].to as usize], NodeSource::Joint);
        assert!(notes.iter().any(|n| n.contains("test:ost: nodes +2")));
        // The outer ends stay open.
        assert_eq!(notes.iter().filter(|n| n.contains("stays open")).count(), 2);

        // A train runs across the seam: the composed line is one network.
        let compiled = merged.compile().expect("composed line compiles");
        let mut pos = TrackPosition::new(EdgeId(0), 1990.0, 1);
        let mut scratch = Vec::new();
        pos.advance(&compiled.net, 20.0, &mut scratch)
            .expect("crosses the boundary");
        assert_eq!(pos.edge, EdgeId(1));
        assert!((pos.s - 10.0).abs() < 1e-6);

        // And the seam is geometrically tight.
        let gap = compiled
            .net
            .edge(EdgeId(0))
            .end_pose()
            .pos
            .0
            .distance(compiled.net.edge(EdgeId(1)).anchor.0);
        assert!(gap < 0.01, "gap {gap} m");
    }

    #[test]
    fn indices_and_payloads_shift_with_the_module() {
        let (composition, lines) = two_modules();
        let Composed { line: merged, .. } = composition.compose(&lines).expect("composes");

        // The second module's signal points at its own device and section.
        assert_eq!(merged.signals[1].device, 2);
        assert_eq!(merged.signals[1].guarded, vec![1]);
        assert_eq!(merged.sections[1].edges, vec![1]);
        assert_eq!(merged.devices[2].edge, 1);
        // Its magnet payload now names the shifted signal index.
        let magnet: MagnetPayload = ron::from_str(&merged.devices[3].payload).unwrap();
        assert_eq!(magnet.signal, Some(1));
    }

    #[test]
    fn distant_modules_stay_apart_with_a_note() {
        let west = module(
            GeoPoint {
                lat: 52.0,
                lon: 10.0,
                height: 100.0,
            },
            90.0,
            46.0,
        );
        // 1 km further north — nothing snaps.
        let far = module(
            GeoPoint {
                lat: 52.01,
                lon: 10.0,
                height: 100.0,
            },
            90.0,
            46.0,
        );
        let mut lines = BTreeMap::new();
        lines.insert("test:west".to_string(), west);
        lines.insert("test:far".to_string(), far);
        let composition = Composition {
            name: "Getrennt".into(),
            modules: vec!["test:west".into(), "test:far".into()],
            connections: vec![],
            signal_links: vec![],
            script: None,
        };
        let Composed {
            line: merged,
            notes,
            ..
        } = composition.compose(&lines).expect("composes");
        assert!(notes.iter().any(|n| n.contains("stand apart")));
        assert!(merged.compile().is_ok());
    }

    #[test]
    fn explicit_connection_bridges_a_gap() {
        let west = module(
            GeoPoint {
                lat: 52.0,
                lon: 10.0,
                height: 100.0,
            },
            90.0,
            46.0,
        );
        let far = module(
            GeoPoint {
                lat: 52.01,
                lon: 10.0,
                height: 100.0,
            },
            90.0,
            46.0,
        );
        let mut lines = BTreeMap::new();
        lines.insert("test:west".to_string(), west);
        lines.insert("test:far".to_string(), far);
        let composition = Composition {
            name: "Verbunden".into(),
            modules: vec!["test:west".into(), "test:far".into()],
            connections: vec![(
                ("test:west".into(), "end".into()),
                ("test:far".into(), "start".into()),
            )],
            signal_links: vec![],
            script: None,
        };
        let Composed { line: merged, .. } = composition.compose(&lines).expect("composes");
        assert_eq!(merged.edges[1].from, merged.edges[0].to);
    }

    /// The Zusi failure mode that must not exist here: modules whose source data was
    /// prepared in different UTM zones shift by metres at the transition, because their
    /// coordinates are planar per zone. Our module anchors are geodetic and the world is
    /// ECEF — which zone a module's data went through must not matter at all.
    #[test]
    fn modules_from_different_utm_zones_meet_exactly() {
        // The shared boundary point sits exactly on the 32/33 zone boundary (12° E).
        // Each module gets it through its own zone's projection and back — the numeric
        // path module data from that zone would take.
        let lat = 52.0f64.to_radians();
        let lon = 12.0f64.to_radians();
        let via_zone = |zone: u8| {
            let (e, n) = to_utm(lat, lon, zone);
            let (lat2, lon2) = from_utm(e, n, zone);
            GeoPoint {
                lat: lat2.to_degrees(),
                lon: lon2.to_degrees(),
                height: 100.0,
            }
        };
        // West extends into zone 32, ost into zone 33; both start at the boundary.
        let west = module(via_zone(32), 270.0, 46.0);
        let ost = module(via_zone(33), 90.0, 46.0);
        let mut lines = BTreeMap::new();
        lines.insert("test:west".to_string(), west);
        lines.insert("test:ost".to_string(), ost);
        let composition = Composition {
            name: "Zonengrenze".into(),
            modules: vec!["test:west".into(), "test:ost".into()],
            connections: vec![],
            signal_links: vec![],
            script: None,
        };

        let Composed {
            line: merged,
            notes,
            ..
        } = composition.compose(&lines).expect("composes");
        // The two start boundaries snapped into one node…
        assert_eq!(merged.edges[1].from, merged.edges[0].from);
        assert!(!notes.iter().any(|n| n.contains("stand apart")));
        // …and the seam is tight to the millimetre, not off by metres.
        let compiled = merged.compile().expect("compiles");
        let gap = compiled
            .net
            .edge(EdgeId(0))
            .anchor
            .0
            .distance(compiled.net.edge(EdgeId(1)).anchor.0);
        assert!(gap < 0.001, "zone seam gap {gap} m");
    }

    /// A signal link makes the last signal of one module announce the first signal of
    /// the next — across the boundary, within the same update.
    #[test]
    fn signal_links_announce_across_the_boundary() {
        let (mut composition, lines) = two_modules();
        composition.signal_links = vec![(("test:west".into(), 0), ("test:ost".into(), 0))];
        let composed = composition.compose(&lines).expect("composes");
        assert_eq!(composed.line.signals[0].next, Some(1));

        // Occupying the second module's section turns its signal to stop — and the
        // first module's signal announces it.
        let compiled = composed.line.compile().expect("compiles");
        let mut il = compiled.interlock;
        let mut net = compiled.net;
        il.update_occupancy(&[EdgeId(1)]);
        il.update(&mut net);
        assert_eq!(
            il.signals[0].aspect.distant,
            Some(sim_core::interlock::DistantAspect::ExpectStop)
        );
        assert!(il.signals[0].situation.next_stop);
    }

    #[test]
    fn a_broken_signal_link_is_an_error() {
        let (mut composition, lines) = two_modules();
        composition.signal_links = vec![(("test:west".into(), 0), ("test:nirgends".into(), 0))];
        assert!(composition.compose(&lines).is_err());
        composition.signal_links = vec![(("test:west".into(), 0), ("test:ost".into(), 7))];
        assert!(composition.compose(&lines).is_err());
    }

    #[test]
    fn missing_module_is_an_error() {
        let composition = Composition {
            name: "Kaputt".into(),
            modules: vec!["test:fehlt".into()],
            connections: vec![],
            signal_links: vec![],
            script: None,
        };
        assert!(composition.compose(&BTreeMap::new()).is_err());
    }
}
