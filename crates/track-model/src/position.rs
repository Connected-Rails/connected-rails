//! Position eines Fahrzeugs/Radsatzes auf dem Gleisgraph und deren Fortschreibung.

use crate::device::DeviceId;
use crate::network::{Blocked, EdgeEnd, EdgeId, EdgeSide, NodeId, TrackNetwork, TrackPose};
use serde::{Deserialize, Serialize};

/// Punkt auf dem Gleisnetz mit Fahrtrichtung.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TrackPosition {
    pub edge: EdgeId,
    /// Bogenlänge auf der Kante [m].
    pub s: f64,
    /// `+1`: Fahrtrichtung = wachsendes `s`, `-1`: fallendes `s`.
    pub dir: i8,
}

/// Ein während der Bewegung überfahrenes Gerät.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PassedDevice {
    pub device: DeviceId,
    /// Wie weit hinter der neuen Position es liegt [m] (0 = gerade jetzt).
    pub distance_behind: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdvanceError {
    pub node: NodeId,
    pub reason: Blocked,
}

impl TrackPosition {
    pub fn new(edge: EdgeId, s: f64, dir: i8) -> Self {
        Self { edge, s, dir }
    }

    pub fn pose(&self, net: &TrackNetwork) -> TrackPose {
        let mut p = net.edge(self.edge).eval(self.s);
        if self.dir < 0 {
            p.tangent = -p.tangent;
            p.curvature = -p.curvature;
            p.grade = -p.grade;
            p.cant = -p.cant;
        }
        p
    }

    /// Zulässige Geschwindigkeit an dieser Stelle [km/h].
    pub fn speed_limit(&self, net: &TrackNetwork) -> f64 {
        net.edge(self.edge).speed.at(self.s)
    }

    /// Bewegt die Position um `distance` [m] in Fahrtrichtung (negativ = rückwärts).
    ///
    /// Sammelt dabei alle überfahrenen Geräte in `passed`. Bricht an Knoten ab, die
    /// nicht befahrbar sind (Prellbock, umlaufende/aufgefahrene Weiche, Auffahren);
    /// die Position bleibt dann exakt am Knoten stehen.
    pub fn advance(
        &mut self,
        net: &TrackNetwork,
        distance: f64,
        passed: &mut Vec<PassedDevice>,
    ) -> Result<(), AdvanceError> {
        let mut remaining = distance;
        // Ein Sicherheitslimit gegen Endlosschleifen bei Null-Länge-Kanten.
        for _ in 0..1024 {
            let edge = net.edge(self.edge);
            let len = edge.length();
            // Bewegung in Kantenrichtung (Vorzeichen von s).
            let step = remaining * self.dir as f64;
            let target = self.s + step;

            if target >= 0.0 && target <= len {
                self.collect(net, self.s, target, 0.0, passed);
                self.s = target;
                return Ok(());
            }

            // Kante verlassen: bis zum Ende laufen, Rest über den Knoten tragen.
            let (boundary, side) = if target > len {
                (len, EdgeSide::End)
            } else {
                (0.0, EdgeSide::Start)
            };
            let used = (boundary - self.s).abs();
            let rest_abs = remaining.abs() - used;
            self.collect(net, self.s, boundary, rest_abs, passed);
            self.s = boundary;

            let node = if side == EdgeSide::End {
                edge.to
            } else {
                edge.from
            };
            let incoming = EdgeEnd::new(self.edge, side);
            let next = net
                .continuation(node, incoming)
                .map_err(|reason| AdvanceError { node, reason })?;

            let next_edge = net.edge(next.edge);
            // Richtung beim Eintritt: an welchem Ende betreten wir die nächste Kante?
            let forward = remaining > 0.0;
            match next.side {
                EdgeSide::Start => {
                    self.s = 0.0;
                    self.dir = if forward { 1 } else { -1 };
                }
                EdgeSide::End => {
                    self.s = next_edge.length();
                    self.dir = if forward { -1 } else { 1 };
                }
            }
            self.edge = next.edge;
            remaining = if forward { rest_abs } else { -rest_abs };
            if rest_abs <= 0.0 {
                return Ok(());
            }
        }
        Ok(())
    }

    /// Geräte zwischen `from` und `to` (in Kantenkoordinaten) einsammeln.
    fn collect(
        &self,
        net: &TrackNetwork,
        from: f64,
        to: f64,
        distance_after: f64,
        passed: &mut Vec<PassedDevice>,
    ) {
        if from == to {
            return;
        }
        let (lo, hi) = if from < to { (from, to) } else { (to, from) };
        for d in net.devices_on(self.edge) {
            if d.s < lo || d.s > hi || !d.facing.applies(self.dir) {
                continue;
            }
            let behind = (to - d.s).abs() + distance_after;
            passed.push(PassedDevice {
                device: d.id,
                distance_behind: behind,
            });
        }
        passed.sort_by(|a, b| b.distance_behind.total_cmp(&a.distance_behind));
    }

    /// Position `distance` Meter vor dieser (ohne sie zu verändern), z. B. für
    /// Fahrzeugenden oder die Vorausschau der KI. `None`, wenn der Weg blockiert ist.
    pub fn offset_by(&self, net: &TrackNetwork, distance: f64) -> Option<TrackPosition> {
        let mut p = *self;
        let mut scratch = Vec::new();
        p.advance(net, distance, &mut scratch).ok()?;
        Some(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::{DeviceKind, TracksideDevice};
    use crate::geometry::Segment;
    use crate::network::{NodeKind, Switch, SwitchPosition, TrackEdge};
    use world_coords::geo::to_ecef_deg;

    /// Kleines Netz: A --e0--> B(Weiche) --e1--> C  und  B --e2--> D
    fn switch_net() -> (TrackNetwork, EdgeId, EdgeId, EdgeId, NodeId) {
        let mut net = TrackNetwork::new();
        let a = net.add_node(NodeKind::Buffer);
        let b = net.add_node(NodeKind::Joint); // wird gleich zur Weiche
        let c = net.add_node(NodeKind::Buffer);
        let d = net.add_node(NodeKind::Buffer);
        let anchor = to_ecef_deg(52.0, 10.0, 100.0);
        let e0 = net.add_edge(TrackEdge::new(
            EdgeId(0),
            a,
            b,
            anchor,
            0.0,
            vec![Segment::straight(500.0)],
        ));
        let p = net.edge(e0).end_pose();
        let e1 = net.add_edge(TrackEdge::new(
            EdgeId(0),
            b,
            c,
            p.pos,
            0.0,
            vec![Segment::straight(500.0)],
        ));
        let e2 = net.add_edge(TrackEdge::new(
            EdgeId(0),
            b,
            d,
            p.pos,
            0.0,
            vec![
                Segment::transition(60.0, 0.0, -1.0 / 300.0),
                Segment::arc(100.0, -300.0),
            ],
        ));
        let sw = Switch::new(
            EdgeEnd::new(e0, EdgeSide::End),
            EdgeEnd::new(e1, EdgeSide::Start),
            EdgeEnd::new(e2, EdgeSide::Start),
        );
        net.node_mut(b).kind = NodeKind::Switch(sw);
        (net, e0, e1, e2, b)
    }

    #[test]
    fn advances_over_switch_in_commanded_position() {
        let (mut net, e0, e1, e2, b) = switch_net();
        let mut pos = TrackPosition::new(e0, 0.0, 1);
        let mut passed = Vec::new();
        pos.advance(&net, 600.0, &mut passed).unwrap();
        assert_eq!(pos.edge, e1);
        assert!((pos.s - 100.0).abs() < 1e-9);

        // Umstellen und erneut fahren.
        let sw = net.switch_mut(b).unwrap();
        sw.command(SwitchPosition::Diverging).unwrap();
        net.update_switches(10.0);
        let mut pos = TrackPosition::new(e0, 0.0, 1);
        pos.advance(&net, 600.0, &mut passed).unwrap();
        assert_eq!(pos.edge, e2);
    }

    #[test]
    fn blocked_while_switch_moves_and_at_buffer() {
        let (mut net, e0, _e1, _e2, b) = switch_net();
        net.switch_mut(b)
            .unwrap()
            .command(SwitchPosition::Diverging)
            .unwrap();
        let mut pos = TrackPosition::new(e0, 0.0, 1);
        let mut passed = Vec::new();
        let err = pos.advance(&net, 600.0, &mut passed).unwrap_err();
        assert_eq!(err.reason, Blocked::SwitchMoving);
        assert!((pos.s - 500.0).abs() < 1e-9, "hält am Knoten");

        // Rückwärts gegen den Prellbock.
        let mut pos = TrackPosition::new(e0, 10.0, 1);
        let err = pos.advance(&net, -50.0, &mut passed).unwrap_err();
        assert_eq!(err.reason, Blocked::BufferStop);
        assert_eq!(pos.s, 0.0);
    }

    #[test]
    fn trailing_a_switch_is_reported() {
        let (net, _e0, _e1, e2, _b) = switch_net();
        // Auf dem Zweiggleis rückwärts zur Wurzel, Weiche liegt aber auf Stamm.
        let mut pos = TrackPosition::new(e2, 50.0, 1);
        let mut passed = Vec::new();
        let err = pos.advance(&net, -60.0, &mut passed).unwrap_err();
        assert_eq!(err.reason, Blocked::WouldTrail);
    }

    #[test]
    fn devices_are_collected_in_order_with_distance() {
        let (mut net, e0, _e1, _e2, _b) = switch_net();
        net.add_device(TracksideDevice::new(DeviceKind::Magnet, e0, 100.0));
        net.add_device(TracksideDevice::new(DeviceKind::Signal, e0, 300.0));
        // Rückwärtsgerichtetes Gerät darf bei Vorwärtsfahrt nicht auslösen.
        net.add_device(
            TracksideDevice::new(DeviceKind::Magnet, e0, 200.0)
                .with_facing(crate::device::Facing::Backward),
        );

        let mut pos = TrackPosition::new(e0, 0.0, 1);
        let mut passed = Vec::new();
        pos.advance(&net, 400.0, &mut passed).unwrap();
        assert_eq!(passed.len(), 2);
        assert_eq!(net.device(passed[0].device).kind, DeviceKind::Magnet);
        assert!((passed[0].distance_behind - 300.0).abs() < 1e-9);
        assert!((passed[1].distance_behind - 100.0).abs() < 1e-9);
    }

    #[test]
    fn oval_loop_closes() {
        // Zwei Halbkreise + zwei Geraden: Rundkurs, der auf sich selbst zurückführt.
        let mut net = TrackNetwork::new();
        let n: Vec<NodeId> = (0..4).map(|_| net.add_node(NodeKind::Joint)).collect();
        let r = 400.0;
        let half = std::f64::consts::PI * r;
        let mut anchor = to_ecef_deg(52.0, 10.0, 100.0);
        let mut heading = 0.0;
        let mut edges = Vec::new();
        for i in 0..4 {
            let segs = if i % 2 == 0 {
                vec![Segment::straight(1000.0)]
            } else {
                vec![Segment::arc(half, r)]
            };
            let e = net.add_edge(TrackEdge::new(
                EdgeId(0),
                n[i],
                n[(i + 1) % 4],
                anchor,
                heading,
                segs,
            ));
            let end = net.edge(e).end_pose();
            anchor = end.pos;
            heading += net
                .edge(e)
                .segments
                .iter()
                .map(|s| s.heading_delta(s.len))
                .sum::<f64>();
            edges.push(e);
        }
        let total: f64 = edges.iter().map(|e| net.edge(*e).length()).sum();
        let start = TrackPosition::new(edges[0], 0.0, 1);
        let start_pose = start.pose(&net);
        let mut pos = start;
        let mut passed = Vec::new();
        pos.advance(&net, total, &mut passed).unwrap();
        assert_eq!(pos.edge, edges[0]);
        // Die Kante 3 endet geometrisch am Anfang von Kante 0 — Rundkurs geschlossen.
        let end_pose = TrackPosition::new(edges[3], net.edge(edges[3]).length(), 1).pose(&net);
        assert!(
            end_pose.pos.distance(start_pose.pos) < 0.5,
            "Rundkurs offen: {} m",
            end_pose.pos.distance(start_pose.pos)
        );
    }
}
