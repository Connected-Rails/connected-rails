//! Vorausschau auf dem Gleisgraph: Geschwindigkeitsprofil und Signale (Plan Kap. 11).

use sim_core::interlock::Interlock;
use track_model::{DeviceKind, EdgeSide, TrackNetwork, TrackPosition};

/// Eine vorausliegende Geschwindigkeitsvorgabe.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Restriction {
    /// Entfernung ab der aktuellen Position [m].
    pub distance: f64,
    /// Ab dort zulässige Geschwindigkeit [km/h] (0 = Halt).
    pub speed: f64,
}

/// Ergebnis der Vorausschau.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lookahead {
    /// Zulässige Geschwindigkeit an der aktuellen Position [km/h].
    pub current: f64,
    /// Vorausliegende Einschränkungen, nach Entfernung sortiert.
    pub restrictions: Vec<Restriction>,
}

impl Lookahead {
    /// Erlaubte Geschwindigkeit [km/h] unter Berücksichtigung aller Bremskurven.
    ///
    /// `decel` ist die geplante Verzögerung [m/s²], `margin` ein Vorhalteweg [m].
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

    /// Entfernung bis zum nächsten Halt [m], falls einer voraus liegt.
    pub fn distance_to_stop(&self) -> Option<f64> {
        self.restrictions
            .iter()
            .find(|r| r.speed <= 0.1)
            .map(|r| r.distance)
    }
}

/// Schaut `distance` Meter voraus und sammelt Geschwindigkeitsprofil und Signalbegriffe.
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
    // Kantenweise vorwärts, maximal 64 Kanten (Schutz gegen Ringschluss).
    for _ in 0..64 {
        let edge = net.edge(pos.edge);
        let (lo, hi) = if pos.dir > 0 {
            (pos.s, edge.length())
        } else {
            (0.0, pos.s)
        };
        let remaining = distance - travelled;
        let span = (hi - lo).min(remaining);

        // Geschwindigkeitsstufen dieser Kante.
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

        // Signale auf dieser Kante.
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

        // Über den Knoten zur nächsten Kante.
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
            // Fahrweg endet (Prellbock, Weiche liegt falsch) → Halt an dieser Stelle.
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
