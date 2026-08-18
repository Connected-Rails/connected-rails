//! Position of a vehicle/wheelset on the track graph and how it is advanced.

use crate::device::DeviceId;
use crate::network::{Blocked, EdgeEnd, EdgeId, EdgeSide, NodeId, TrackNetwork, TrackPose};
use serde::{Deserialize, Serialize};

/// Point on the track network with a direction of travel.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TrackPosition {
    pub edge: EdgeId,
    /// Arc length along the edge [m].
    pub s: f64,
    /// `+1`: direction of travel = increasing `s`, `-1`: decreasing `s`.
    pub dir: i8,
}

/// A device passed during the movement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PassedDevice {
    pub device: DeviceId,
    /// How far behind the new position it lies [m] (0 = right now).
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

    /// Permitted speed at this location [km/h].
    pub fn speed_limit(&self, net: &TrackNetwork) -> f64 {
        net.edge(self.edge).speed.at(self.s)
    }

    /// Moves the position by `distance` [m] in the direction of travel (negative = backwards).
    ///
    /// Collects all passed devices in `passed`. Aborts at nodes that cannot be passed
    /// (buffer stop, moving/trailed switch, trailing move against the position); the
    /// position then stops exactly at the node.
    pub fn advance(
        &mut self,
        net: &TrackNetwork,
        distance: f64,
        passed: &mut Vec<PassedDevice>,
    ) -> Result<(), AdvanceError> {
        let mut remaining = distance;
        // A safety limit against infinite loops with zero-length edges.
        for _ in 0..1024 {
            let edge = net.edge(self.edge);
            let len = edge.length();
            // Movement along the edge direction (sign of s).
            let step = remaining * self.dir as f64;
            let target = self.s + step;

            if target >= 0.0 && target <= len {
                self.collect(net, self.s, target, 0.0, passed);
                self.s = target;
                return Ok(());
            }

            // Leaving the edge: run to the end, carry the remainder across the node.
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
            // Direction on entry: at which end do we enter the next edge?
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

    /// Collect devices between `from` and `to` (in edge coordinates).
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

    /// Signed distance from this position to `other` along the track [m] — positive when
    /// `other` lies ahead in the direction of travel. Searches at most `limit` metres each
    /// way and returns `None` when `other` is not on the path at all; a switch lying the
    /// other way is enough for that.
    ///
    /// This is how a network correction is measured: server and client hold the same train
    /// on the same track, and the only thing that differs is how far along it has come.
    pub fn distance_to(
        &self,
        net: &TrackNetwork,
        other: &TrackPosition,
        limit: f64,
    ) -> Option<f64> {
        for sign in [1.0f64, -1.0] {
            let mut pos = *self;
            let mut travelled = 0.0;
            // A safety limit against infinite loops with zero-length edges, as in `advance`.
            for _ in 0..1024 {
                // Which end of the current edge the search runs towards.
                let towards_end = pos.dir as f64 * sign > 0.0;
                if pos.edge == other.edge {
                    let ahead = (other.s - pos.s) * pos.dir as f64 * sign;
                    if ahead >= -1e-9 && travelled + ahead <= limit {
                        return Some(sign * (travelled + ahead));
                    }
                }
                let edge = net.edge(pos.edge);
                let (boundary, side) = if towards_end {
                    (edge.length(), EdgeSide::End)
                } else {
                    (0.0, EdgeSide::Start)
                };
                travelled += (boundary - pos.s).abs();
                if travelled > limit {
                    break;
                }
                let node = if side == EdgeSide::End {
                    edge.to
                } else {
                    edge.from
                };
                let Ok(next) = net.continuation(node, EdgeEnd::new(pos.edge, side)) else {
                    break;
                };
                // Entering the next edge at its start means running with its `s`; the
                // direction of travel follows from which way the search is going.
                match next.side {
                    EdgeSide::Start => {
                        pos.s = 0.0;
                        pos.dir = if sign > 0.0 { 1 } else { -1 };
                    }
                    EdgeSide::End => {
                        pos.s = net.edge(next.edge).length();
                        pos.dir = if sign > 0.0 { -1 } else { 1 };
                    }
                }
                pos.edge = next.edge;
            }
        }
        None
    }

    /// Position `distance` metres ahead of this one (without modifying it), e.g. for
    /// vehicle ends or the AI look-ahead. `None` if the path is blocked.
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

    /// Small network: A --e0--> B(switch) --e1--> C  and  B --e2--> D
    fn switch_net() -> (TrackNetwork, EdgeId, EdgeId, EdgeId, NodeId) {
        let mut net = TrackNetwork::new();
        let a = net.add_node(NodeKind::Buffer);
        let b = net.add_node(NodeKind::Joint); // turned into a switch below
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

        // Throw the switch and run again.
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
        assert!((pos.s - 500.0).abs() < 1e-9, "stops at the node");

        // Backwards against the buffer stop.
        let mut pos = TrackPosition::new(e0, 10.0, 1);
        let err = pos.advance(&net, -50.0, &mut passed).unwrap_err();
        assert_eq!(err.reason, Blocked::BufferStop);
        assert_eq!(pos.s, 0.0);
    }

    #[test]
    fn trailing_a_switch_is_reported() {
        let (net, _e0, _e1, e2, _b) = switch_net();
        // On the branch track backwards to the root, but the switch is set to straight.
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
        // A backward-facing device must not trigger when travelling forwards.
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
    fn distance_to_measures_over_a_switch_and_backwards() {
        let (net, e0, e1, _e2, _b) = switch_net();
        let here = TrackPosition::new(e0, 100.0, 1);
        // 400 m to the end of e0, then 250 m into e1.
        let ahead = TrackPosition::new(e1, 250.0, 1);
        assert!((here.distance_to(&net, &ahead, 1000.0).unwrap() - 650.0).abs() < 1e-6);
        // The same pair the other way round is the negative distance.
        assert!((ahead.distance_to(&net, &here, 1000.0).unwrap() + 650.0).abs() < 1e-6);
        // On the same edge, behind.
        let behind = TrackPosition::new(e0, 40.0, 1);
        assert!((here.distance_to(&net, &behind, 1000.0).unwrap() + 60.0).abs() < 1e-6);
        // Out of range and off the path stay `None`.
        assert_eq!(here.distance_to(&net, &ahead, 100.0), None);
        let branch = TrackPosition::new(_e2, 50.0, 1);
        assert_eq!(here.distance_to(&net, &branch, 1000.0), None);
    }

    #[test]
    fn oval_loop_closes() {
        // Two semicircles + two straights: a loop that leads back onto itself.
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
        // Edge 3 geometrically ends at the start of edge 0 — the loop is closed.
        let end_pose = TrackPosition::new(edges[3], net.edge(edges[3]).length(), 1).pose(&net);
        assert!(
            end_pose.pos.distance(start_pose.pos) < 0.5,
            "loop open: {} m",
            end_pose.pos.distance(start_pose.pos)
        );
    }
}
