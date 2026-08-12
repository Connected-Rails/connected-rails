//! Topologie und Geometrie des Gleisnetzes.

use crate::device::{DeviceId, TracksideDevice};
use crate::geometry::{Segment, eval_chain};
use crate::profile::StepProfile;
use glam::DVec3;
use serde::{Deserialize, Serialize};
use world_coords::{EcefPos, EnuFrame};

macro_rules! id_type {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        pub struct $name(pub u32);

        impl $name {
            pub fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

id_type!(NodeId);
id_type!(EdgeId);

/// Welches Ende einer Kante an einem Knoten hängt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeSide {
    Start,
    End,
}

/// Ein Kantenende (Kante + Seite).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeEnd {
    pub edge: EdgeId,
    pub side: EdgeSide,
}

impl EdgeEnd {
    pub fn new(edge: EdgeId, side: EdgeSide) -> Self {
        Self { edge, side }
    }
}

/// Weichenlage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SwitchPosition {
    /// Stammgleis / Geradeausfahrt.
    Straight,
    /// Zweiggleis / Ablenkung.
    Diverging,
}

/// Weiche: Wurzel + zwei Zweige, mit Umlaufzeit und Verschluss.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Switch {
    pub root: EdgeEnd,
    /// `[Stammgleis, Zweiggleis]`.
    pub branches: [EdgeEnd; 2],
    pub position: SwitchPosition,
    /// Angesteuerte Lage; weicht sie von `position` ab, läuft die Weiche um.
    pub commanded: SwitchPosition,
    /// Umlaufzeit [s].
    pub throw_time: f64,
    /// Restlaufzeit [s].
    pub remaining: f64,
    /// Durch Fahrstraße festgelegt (Verschluss).
    pub locked: bool,
    /// Aufgefahren — bis zur Wiederherstellung nicht befahrbar.
    pub trailed: bool,
}

impl Switch {
    pub fn new(root: EdgeEnd, straight: EdgeEnd, diverging: EdgeEnd) -> Self {
        Self {
            root,
            branches: [straight, diverging],
            position: SwitchPosition::Straight,
            commanded: SwitchPosition::Straight,
            throw_time: 6.0,
            remaining: 0.0,
            locked: false,
            trailed: false,
        }
    }

    pub fn is_moving(&self) -> bool {
        self.remaining > 0.0
    }

    /// Aktuell verbundenes Kantenende (während des Umlaufs: keins).
    pub fn connected(&self) -> Option<EdgeEnd> {
        if self.is_moving() || self.trailed {
            return None;
        }
        Some(match self.position {
            SwitchPosition::Straight => self.branches[0],
            SwitchPosition::Diverging => self.branches[1],
        })
    }

    /// Umstellauftrag. Schlägt fehl, wenn die Weiche verschlossen oder aufgefahren ist.
    pub fn command(&mut self, to: SwitchPosition) -> Result<(), SwitchError> {
        if self.trailed {
            return Err(SwitchError::Trailed);
        }
        if self.locked {
            return Err(SwitchError::Locked);
        }
        if self.position == to && !self.is_moving() {
            return Ok(());
        }
        self.commanded = to;
        self.remaining = self.throw_time;
        Ok(())
    }

    pub fn update(&mut self, dt: f64) {
        if self.remaining <= 0.0 {
            return;
        }
        self.remaining -= dt;
        if self.remaining <= 0.0 {
            self.remaining = 0.0;
            self.position = self.commanded;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchError {
    Locked,
    Trailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeKind {
    /// Stumpfgleisende / Prellbock.
    Buffer,
    /// Durchgehende Verbindung zweier Kantenenden.
    Joint,
    Switch(Switch),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackNode {
    pub id: NodeId,
    pub kind: NodeKind,
    /// Alle hier zusammenlaufenden Kantenenden.
    pub ends: Vec<EdgeEnd>,
}

/// Pose auf dem Gleis, in Weltkoordinaten.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackPose {
    pub pos: EcefPos,
    /// Einheitsvektor in Richtung wachsender Bogenlänge (ECEF).
    pub tangent: DVec3,
    /// „Oben" des Gleises inkl. Überhöhung (ECEF).
    pub up: DVec3,
    /// Krümmung [1/m], positiv = Linksbogen.
    pub curvature: f64,
    /// Längsneigung [‰], positiv = Steigung in Richtung wachsender Bogenlänge.
    pub grade: f64,
    /// Überhöhung [mm].
    pub cant: f64,
}

/// Radaufstandsbreite für die Umrechnung Überhöhung [mm] → Rollwinkel.
pub const TRACK_GAUGE_ROLL: f64 = 1500.0;

/// Eine Gleiskante: Geometrie im lokalen ENU-Frame ihres Anfangspunkts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackEdge {
    pub id: EdgeId,
    pub from: NodeId,
    pub to: NodeId,
    /// Weltposition bei `s = 0`.
    pub anchor: EcefPos,
    /// Anfangsrichtung im lokalen ENU-Frame [rad], 0 = Ost, mathematisch positiv.
    pub heading0: f64,
    pub segments: Vec<Segment>,
    /// Längsneigung [‰] über `s`.
    pub grade: StepProfile<f64>,
    /// Überhöhung [mm] über `s`.
    pub cant: StepProfile<f64>,
    /// Zulässige Geschwindigkeit [km/h] über `s`.
    pub speed: StepProfile<f64>,
    #[serde(skip)]
    frame: Option<EnuFrame>,
    #[serde(skip)]
    length: f64,
}

impl TrackEdge {
    pub fn new(
        id: EdgeId,
        from: NodeId,
        to: NodeId,
        anchor: EcefPos,
        heading0: f64,
        segments: Vec<Segment>,
    ) -> Self {
        let mut e = Self {
            id,
            from,
            to,
            anchor,
            heading0,
            segments,
            grade: StepProfile::constant(0.0),
            cant: StepProfile::constant(0.0),
            speed: StepProfile::constant(160.0),
            frame: None,
            length: 0.0,
        };
        e.finish();
        e
    }

    pub fn with_grade(mut self, grade: StepProfile<f64>) -> Self {
        self.grade = grade;
        self
    }

    pub fn with_cant(mut self, cant: StepProfile<f64>) -> Self {
        self.cant = cant;
        self
    }

    pub fn with_speed(mut self, speed: StepProfile<f64>) -> Self {
        self.speed = speed;
        self
    }

    /// Abgeleitete Daten (Frame, Länge) neu berechnen — nach dem Laden aufrufen.
    pub fn finish(&mut self) {
        self.frame = Some(EnuFrame::at(self.anchor));
        self.length = self.segments.iter().map(|s| s.len).sum();
    }

    pub fn length(&self) -> f64 {
        self.length
    }

    fn frame(&self) -> &EnuFrame {
        self.frame
            .as_ref()
            .expect("TrackEdge::finish() nicht aufgerufen")
    }

    /// Pose bei Bogenlänge `s` (0 = Anfang der Kante).
    pub fn eval(&self, s: f64) -> TrackPose {
        let plan = eval_chain(&self.segments, self.heading0, s);
        let grade = self.grade.at(s);
        let cant = self.cant.at(s);
        let z = self.grade.integrate(s) / 1000.0;
        let frame = self.frame();

        let pos = frame.to_ecef_curved(DVec3::new(plan.pos.x, plan.pos.y, z));
        let (sh, ch) = plan.heading.sin_cos();
        let tangent_local = DVec3::new(ch, sh, grade / 1000.0).normalize();
        let tangent = frame.dir_to_ecef(tangent_local).normalize();

        // Überhöhung: Rollwinkel um die Tangente, Kurvenaußenseite hebt sich.
        let roll = (cant / TRACK_GAUGE_ROLL).asin();
        let up_plain = frame.dir_to_ecef(DVec3::Z);
        let left = tangent.cross(up_plain).normalize() * -1.0; // links der Fahrtrichtung
        let up = (up_plain * roll.cos() + left * roll.sin()).normalize();

        TrackPose {
            pos,
            tangent,
            up,
            curvature: plan.curvature,
            grade,
            cant,
        }
    }

    /// Pose am Kantenende.
    pub fn end_pose(&self) -> TrackPose {
        self.eval(self.length)
    }
}

/// Das gesamte Gleisnetz.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrackNetwork {
    edges: Vec<TrackEdge>,
    nodes: Vec<TrackNode>,
    devices: Vec<TracksideDevice>,
    /// Je Kante die Geräte-IDs, nach `s` sortiert.
    #[serde(skip)]
    devices_by_edge: Vec<Vec<DeviceId>>,
}

impl TrackNetwork {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, kind: NodeKind) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(TrackNode {
            id,
            kind,
            ends: Vec::new(),
        });
        id
    }

    /// Fügt eine Kante hinzu und trägt sie an ihren Knoten ein.
    pub fn add_edge(&mut self, mut edge: TrackEdge) -> EdgeId {
        let id = EdgeId(self.edges.len() as u32);
        edge.id = id;
        edge.finish();
        self.nodes[edge.from.index()]
            .ends
            .push(EdgeEnd::new(id, EdgeSide::Start));
        self.nodes[edge.to.index()]
            .ends
            .push(EdgeEnd::new(id, EdgeSide::End));
        self.edges.push(edge);
        self.devices_by_edge.push(Vec::new());
        id
    }

    pub fn add_device(&mut self, device: TracksideDevice) -> DeviceId {
        let id = DeviceId(self.devices.len() as u32);
        let edge = device.edge.index();
        self.devices.push(TracksideDevice { id, ..device });
        let mut list = std::mem::take(&mut self.devices_by_edge[edge]);
        list.push(id);
        list.sort_by(|a, b| {
            self.devices[a.index()]
                .s
                .total_cmp(&self.devices[b.index()].s)
        });
        self.devices_by_edge[edge] = list;
        id
    }

    pub fn edge(&self, id: EdgeId) -> &TrackEdge {
        &self.edges[id.index()]
    }

    pub fn edges(&self) -> &[TrackEdge] {
        &self.edges
    }

    pub fn node(&self, id: NodeId) -> &TrackNode {
        &self.nodes[id.index()]
    }

    pub fn node_mut(&mut self, id: NodeId) -> &mut TrackNode {
        &mut self.nodes[id.index()]
    }

    pub fn nodes(&self) -> &[TrackNode] {
        &self.nodes
    }

    pub fn device(&self, id: DeviceId) -> &TracksideDevice {
        &self.devices[id.index()]
    }

    pub fn devices(&self) -> &[TracksideDevice] {
        &self.devices
    }

    /// Geräte auf einer Kante, nach `s` aufsteigend.
    pub fn devices_on(&self, edge: EdgeId) -> impl Iterator<Item = &TracksideDevice> + '_ {
        self.devices_by_edge[edge.index()]
            .iter()
            .map(|id| &self.devices[id.index()])
    }

    /// Nach dem Deserialisieren: abgeleitete Daten neu aufbauen.
    pub fn finish(&mut self) {
        for e in &mut self.edges {
            e.finish();
        }
        self.devices_by_edge = vec![Vec::new(); self.edges.len()];
        let mut ids: Vec<DeviceId> = (0..self.devices.len() as u32).map(DeviceId).collect();
        ids.sort_by(|a, b| {
            self.devices[a.index()]
                .s
                .total_cmp(&self.devices[b.index()].s)
        });
        for id in ids {
            let edge = self.devices[id.index()].edge.index();
            self.devices_by_edge[edge].push(id);
        }
    }

    pub fn update_switches(&mut self, dt: f64) {
        for n in &mut self.nodes {
            if let NodeKind::Switch(sw) = &mut n.kind {
                sw.update(dt);
            }
        }
    }

    pub fn switch_mut(&mut self, node: NodeId) -> Option<&mut Switch> {
        match &mut self.nodes[node.index()].kind {
            NodeKind::Switch(sw) => Some(sw),
            _ => None,
        }
    }

    pub fn switch(&self, node: NodeId) -> Option<&Switch> {
        match &self.nodes[node.index()].kind {
            NodeKind::Switch(sw) => Some(sw),
            _ => None,
        }
    }

    /// Fortsetzung hinter einem Knoten: Wo geht es weiter, wenn man über `incoming`
    /// (das Kantenende, mit dem man am Knoten ankommt) einfährt?
    ///
    /// `Err(Blocked)` bei stumpfem Ende, umlaufender/aufgefahrener Weiche oder falscher Lage.
    pub fn continuation(&self, node: NodeId, incoming: EdgeEnd) -> Result<EdgeEnd, Blocked> {
        let node = self.node(node);
        match &node.kind {
            NodeKind::Buffer => Err(Blocked::BufferStop),
            NodeKind::Joint => node
                .ends
                .iter()
                .copied()
                .find(|e| *e != incoming)
                .ok_or(Blocked::BufferStop),
            NodeKind::Switch(sw) => {
                if sw.trailed {
                    return Err(Blocked::Trailed);
                }
                if sw.is_moving() {
                    return Err(Blocked::SwitchMoving);
                }
                let connected = sw.connected().ok_or(Blocked::SwitchMoving)?;
                if incoming == sw.root {
                    // Spitzbefahrung: die eingestellte Lage entscheidet.
                    Ok(connected)
                } else if incoming == connected {
                    // Stumpfbefahrung in richtiger Lage.
                    Ok(sw.root)
                } else if sw.branches.contains(&incoming) {
                    // Stumpfbefahrung gegen die Lage → Auffahren.
                    Err(Blocked::WouldTrail)
                } else {
                    Err(Blocked::BufferStop)
                }
            }
        }
    }
}

/// Warum es hinter einem Knoten nicht weitergeht.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocked {
    BufferStop,
    SwitchMoving,
    Trailed,
    /// Weiche würde aufgefahren (stumpf gegen die Lage).
    WouldTrail,
}

#[cfg(test)]
mod tests {
    use super::*;
    use world_coords::geo::to_ecef_deg;

    fn straight_edge(net: &mut TrackNetwork, a: NodeId, b: NodeId, len: f64) -> EdgeId {
        let anchor = to_ecef_deg(52.0, 10.0, 100.0);
        net.add_edge(TrackEdge::new(
            EdgeId(0),
            a,
            b,
            anchor,
            0.0,
            vec![Segment::straight(len)],
        ))
    }

    #[test]
    fn edge_pose_is_level_and_forward() {
        let mut net = TrackNetwork::new();
        let a = net.add_node(NodeKind::Buffer);
        let b = net.add_node(NodeKind::Buffer);
        let e = straight_edge(&mut net, a, b, 1000.0);
        let edge = net.edge(e);
        let p0 = edge.eval(0.0);
        let p1 = edge.eval(1000.0);
        assert!((p0.pos.distance(p1.pos) - 1000.0).abs() < 1e-3);
        // Ohne Neigung: beide Punkte gleich hoch über dem Ellipsoid.
        let h0 = world_coords::geo::from_ecef(p0.pos).2;
        let h1 = world_coords::geo::from_ecef(p1.pos).2;
        assert!((h0 - h1).abs() < 0.01, "{h0} vs {h1}");
        // Tangente steht senkrecht auf „oben".
        assert!(p0.tangent.dot(p0.up).abs() < 1e-9);
    }

    #[test]
    fn grade_changes_height() {
        let mut net = TrackNetwork::new();
        let a = net.add_node(NodeKind::Buffer);
        let b = net.add_node(NodeKind::Buffer);
        let anchor = to_ecef_deg(52.0, 10.0, 100.0);
        let e = net.add_edge(
            TrackEdge::new(
                EdgeId(0),
                a,
                b,
                anchor,
                0.0,
                vec![Segment::straight(1000.0)],
            )
            .with_grade(StepProfile::constant(10.0)),
        );
        let edge = net.edge(e);
        let h0 = world_coords::geo::from_ecef(edge.eval(0.0).pos).2;
        let h1 = world_coords::geo::from_ecef(edge.eval(1000.0).pos).2;
        assert!((h1 - h0 - 10.0).abs() < 0.01, "{}", h1 - h0);
        assert!(edge.eval(500.0).tangent.dot(edge.eval(0.0).up) > 0.0);
    }

    #[test]
    fn switch_throws_and_blocks_when_locked() {
        let mut sw = Switch::new(
            EdgeEnd::new(EdgeId(0), EdgeSide::End),
            EdgeEnd::new(EdgeId(1), EdgeSide::Start),
            EdgeEnd::new(EdgeId(2), EdgeSide::Start),
        );
        assert_eq!(sw.connected(), Some(sw.branches[0]));
        sw.command(SwitchPosition::Diverging).unwrap();
        assert_eq!(sw.connected(), None, "während Umlauf keine Verbindung");
        sw.update(3.0);
        assert!(sw.is_moving());
        sw.update(3.0);
        assert_eq!(sw.position, SwitchPosition::Diverging);
        assert_eq!(sw.connected(), Some(sw.branches[1]));

        sw.locked = true;
        assert_eq!(
            sw.command(SwitchPosition::Straight),
            Err(SwitchError::Locked)
        );
        assert_eq!(sw.position, SwitchPosition::Diverging);
    }
}
