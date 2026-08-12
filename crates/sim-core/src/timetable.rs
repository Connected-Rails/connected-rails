//! Timetable data model (plan 11).

use serde::{Deserialize, Serialize};
use track_model::{EdgeId, TrackNetwork, TrackPosition};

/// A scheduled stop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScheduledStop {
    /// Operating point.
    pub name: String,
    /// Stopping point (position of the head of the train).
    pub edge: EdgeId,
    pub s: f64,
    /// Scheduled arrival [s since the start of the simulation].
    pub arrival: f64,
    /// Scheduled departure [s].
    pub departure: f64,
    /// Platform track (display / route selection only).
    #[serde(default)]
    pub platform: String,
}

impl ScheduledStop {
    /// Distance from `from` to the stopping point [m], if it lies ahead within `max`.
    ///
    /// ponytail: searches only forwards along the path in metre steps instead of using a
    /// graph search — with 4 km of look-ahead that is 4000 cheap steps per train and second.
    /// Replace it with a real path search once timetables lead over many switches.
    pub fn distance_from(&self, net: &TrackNetwork, from: TrackPosition, max: f64) -> Option<f64> {
        if from.edge == self.edge {
            let d = (self.s - from.s) * from.dir as f64;
            return (d >= -20.0 && d <= max).then_some(d.max(0.0));
        }
        let mut pos = from;
        let mut scratch = Vec::new();
        let mut travelled = 0.0;
        while travelled < max {
            let step = 25.0;
            if pos.advance(net, step, &mut scratch).is_err() {
                return None;
            }
            travelled += step;
            if pos.edge == self.edge {
                let d = (self.s - pos.s) * pos.dir as f64;
                if (-25.0..=max).contains(&d) {
                    return Some((travelled + d).max(0.0));
                }
            }
        }
        None
    }
}

/// Timetable of a train.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Timetable {
    /// Train number (e.g. "RE 4711").
    pub number: String,
    /// Train category.
    #[serde(default)]
    pub category: String,
    pub stops: Vec<ScheduledStop>,
}

impl Timetable {
    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }

    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).expect("serializable")
    }
}
