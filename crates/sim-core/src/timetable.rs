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
    /// Scheduled arrival [s]; where zero lies is decided by [`TimetableKind`].
    pub arrival: f64,
    /// Scheduled departure [s].
    pub departure: f64,
    /// Platform track (display / route selection only).
    #[serde(default)]
    pub platform: String,
    /// Module whose local `edge` index this stop uses — resolved against the composed
    /// line by the mod runtime, then cleared. `None` falls back to the timetable's
    /// `module`; without either, `edge` is an index of the line itself.
    #[serde(default)]
    pub module: Option<String>,
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

/// How the times of a timetable relate to the simulation clock.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimetableKind {
    /// Times are seconds since the start of the scenario; the timetable runs once.
    #[default]
    Scenario,
    /// Times are seconds since midnight; the timetable wraps around every 24 h.
    Daily,
}

/// Seconds of one day.
pub const DAY: f64 = 86_400.0;

/// Timetable of a train.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Timetable {
    /// Train number (e.g. "RE 4711").
    pub number: String,
    /// Train category.
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub kind: TimetableKind,
    /// Default module for the stops' `edge` indices — see [`ScheduledStop::module`].
    #[serde(default)]
    pub module: Option<String>,
    pub stops: Vec<ScheduledStop>,
}

impl Timetable {
    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }

    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).expect("serializable")
    }

    /// Signed deviation of `time` from `scheduled` [s], positive = late.
    /// A daily timetable measures around the clock: 23:59 against a 0:01 slot is
    /// two minutes early, not a day late.
    pub fn delay(&self, time: f64, scheduled: f64) -> f64 {
        match self.kind {
            TimetableKind::Scenario => time - scheduled,
            TimetableKind::Daily => (time - scheduled + DAY / 2.0).rem_euclid(DAY) - DAY / 2.0,
        }
    }

    /// The next absolute simulation time at or after `time` that `scheduled` refers to.
    pub fn next_occurrence(&self, time: f64, scheduled: f64) -> f64 {
        match self.kind {
            TimetableKind::Scenario => scheduled,
            TimetableKind::Daily => time + (scheduled - time).rem_euclid(DAY),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daily_times_wrap_around_midnight() {
        let daily = Timetable {
            kind: TimetableKind::Daily,
            ..Timetable::default()
        };
        // 23:59 against a 0:01 slot: two minutes early.
        assert_eq!(daily.delay(DAY - 60.0, 60.0), -120.0);
        // 0:01 against a 23:59 slot: two minutes late, on any day.
        assert_eq!(daily.delay(DAY + 60.0, DAY - 60.0), 120.0);
        // The next 0:01 seen from 23:59 of day two lies 120 s ahead.
        assert_eq!(
            daily.next_occurrence(2.0 * DAY - 60.0, 60.0),
            2.0 * DAY + 60.0
        );
        // A slot that is due right now stays put.
        assert_eq!(daily.next_occurrence(DAY + 60.0, 60.0), DAY + 60.0);

        let scenario = Timetable::default();
        assert_eq!(scenario.delay(500.0, 480.0), 20.0);
        assert_eq!(scenario.next_occurrence(100.0, 480.0), 480.0);
    }
}
