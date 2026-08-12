//! Look-ahead on the track graph: speed profile and signals (plan ch. 11).

use sim_core::interlock::Interlock;
use track_model::{DeviceKind, EdgeSide, TrackNetwork, TrackPosition};

/// A speed restriction lying ahead.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Restriction {
    /// Distance from the current position [m].
    pub distance: f64,
    /// Speed permitted from there on [km/h] (0 = stop).
    pub speed: f64,
}

/// Result of the look-ahead.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lookahead {
    /// Permitted speed at the current position [km/h].
    pub current: f64,
    /// Restrictions lying ahead, sorted by distance.
    pub restrictions: Vec<Restriction>,
}

impl Lookahead {
    /// Permitted speed [km/h] taking all braking curves into account.
    ///
    /// `decel` is the planned deceleration [m/s²], `margin` a lead distance [m].
    pub fn permitted(&self, decel: f64, margin: f64) -> f64 {
        let mut v = self.current;
        for r in &self.restrictions {
            let d = (r.distance - margin).max(0.0);
            let target = r.speed / 3.6;
            let curve = (target * target + 2.0 * decel * d).sqrt() * 3.6;
            v = v.min(curve);
        }
        v.max(0.0)
    }

    /// Distance to the next stop [m], if one lies ahead.
    pub fn distance_to_stop(&self) -> Option<f64> {
        self.restrictions
            .iter()
            .find(|r| r.speed <= 0.1)
            .map(|r| r.distance)
    }
}

/// Looks `distance` metres ahead and collects the speed profile and signal aspects.
pub fn scan(
    net: &TrackNetwork,
    interlock: &Interlock,
    from: TrackPosition,
    distance: f64,
) -> Lookahead {
    let mut out = Lookahead {
        current: from.speed_limit(net),
        restrictions: Vec::new(),
    };

    let mut pos = from;
    let mut travelled = 0.0;
    // Edge by edge forwards, at most 64 edges (protection against loops).
    for _ in 0..64 {
        let edge = net.edge(pos.edge);
        let (lo, hi) = if pos.dir > 0 {
            (pos.s, edge.length())
        } else {
            (0.0, pos.s)
        };
        let remaining = distance - travelled;
        let span = (hi - lo).min(remaining);

        // Speed steps of this edge.
        for (s, v) in edge.speed.steps().iter().copied() {
            if s < lo || s > hi {
                continue;
            }
            let d = if pos.dir > 0 { s - lo } else { hi - s };
            if d > span {
                continue;
            }
            out.restrictions.push(Restriction {
                distance: travelled + d,
                speed: v,
            });
        }

        // Signals on this edge.
        for device in net.devices_on(pos.edge) {
            if device.kind != DeviceKind::Signal
                || device.s < lo
                || device.s > hi
                || !device.facing.applies(pos.dir)
            {
                continue;
            }
            let d = if pos.dir > 0 {
                device.s - lo
            } else {
                hi - device.s
            };
            if d > span {
                continue;
            }
            let Some(signal) = interlock.signal_at_device(device.id) else {
                continue;
            };
            if let Some(speed) = interlock.signal_speed(signal.id) {
                out.restrictions.push(Restriction {
                    distance: travelled + d,
                    speed,
                });
            }
        }

        travelled += span;
        if travelled >= distance {
            break;
        }

        // Across the node to the next edge.
        let side = if pos.dir > 0 {
            EdgeSide::End
        } else {
            EdgeSide::Start
        };
        let node = if side == EdgeSide::End {
            edge.to
        } else {
            edge.from
        };
        let Ok(next) = net.continuation(node, track_model::EdgeEnd::new(pos.edge, side)) else {
            // Route ends (buffer stop, switch set the wrong way) → stop at this point.
            out.restrictions.push(Restriction {
                distance: travelled,
                speed: 0.0,
            });
            break;
        };
        let next_edge = net.edge(next.edge);
        pos = match next.side {
            EdgeSide::Start => TrackPosition::new(next.edge, 0.0, 1),
            EdgeSide::End => TrackPosition::new(next.edge, next_edge.length(), -1),
        };
    }

    out.restrictions
        .sort_by(|a, b| a.distance.total_cmp(&b.distance));
    out
}
