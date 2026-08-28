//! Look-ahead on the track graph: speed profile, signals and block boundaries (plan ch. 11).
//!
//! Used by the AI driver and by the LZB centre, which builds its movement authority from it.

use crate::interlock::{BlockMarkerPayload, Interlock};
use crate::shunt::Movement;
use track_model::{DeviceKind, EdgeSide, TrackNetwork, TrackPosition};

/// A speed restriction lying ahead.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Restriction {
    /// Distance from the current position [m].
    pub distance: f64,
    /// Speed permitted from there on [km/h] (0 = stop).
    pub speed: f64,
    /// The signal it comes from, where it comes from one — a speed step of the line
    /// carries none. It is what says *which* signal is holding a movement, which is what
    /// the automatic shunting-route setting asks for
    /// ([`Sim::step`](crate::Sim::step)).
    pub signal: Option<crate::interlock::SignalId>,
}

/// A block boundary lying ahead.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockBoundary {
    /// Distance from the current position [m].
    pub distance: f64,
    /// Track section behind the boundary — its clear detection decides whether a movement
    /// authority may run past it.
    pub section: u32,
}

/// Result of the look-ahead.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lookahead {
    /// Permitted speed at the current position [km/h].
    pub current: f64,
    /// Restrictions lying ahead, sorted by distance.
    pub restrictions: Vec<Restriction>,
    /// Block boundaries lying ahead, sorted by distance.
    pub blocks: Vec<BlockBoundary>,
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
        self.next_stop().map(|r| r.distance)
    }

    /// The next thing that stops the movement, whatever it is.
    pub fn next_stop(&self) -> Option<&Restriction> {
        self.restrictions.iter().find(|r| r.speed <= 0.1)
    }
}

/// Looks `distance` metres ahead and collects the speed profile and signal aspects.
///
/// `movement` decides which signals bind: a train movement reads the main aspects and runs
/// at the line speed, a shunting movement reads Sh 1 and nothing else and is held to
/// shunting speed throughout (`sim_core::shunt::Movement`).
pub fn scan(
    net: &TrackNetwork,
    interlock: &Interlock,
    from: TrackPosition,
    distance: f64,
    movement: Movement,
) -> Lookahead {
    let line = from.speed_limit(net);
    let mut out = Lookahead {
        current: match movement.speed_limit() {
            Some(shunting) => line.min(shunting),
            None => line,
        },
        ..Default::default()
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

        // Speed steps of this edge. A shunting movement is held to shunting speed over
        // all of them — the line speed is a train's, and a shunt is driven on sight.
        let cap = movement.speed_limit().unwrap_or(f64::INFINITY);
        for (s, v) in edge.speed.steps().iter().copied() {
            let v = v.min(cap);
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
                signal: None,
            });
        }

        // Signals and block boundaries on this edge.
        for device in net.devices_on(pos.edge) {
            if device.s < lo || device.s > hi || !device.facing.applies(pos.dir) {
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
            match &device.kind {
                DeviceKind::Signal => {
                    let Some(signal) = interlock.signal_at_device(device.id) else {
                        continue;
                    };
                    if let Some(speed) = interlock.signal_speed(signal.id, movement) {
                        out.restrictions.push(Restriction {
                            distance: travelled + d,
                            speed,
                            signal: Some(signal.id),
                        });
                    }
                }
                DeviceKind::BlockMarker => {
                    let Ok(marker) = ron::from_str::<BlockMarkerPayload>(&device.payload) else {
                        continue;
                    };
                    out.blocks.push(BlockBoundary {
                        distance: travelled + d,
                        section: marker.section,
                    });
                }
                _ => {}
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
                signal: None,
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
    out.blocks.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    out
}
