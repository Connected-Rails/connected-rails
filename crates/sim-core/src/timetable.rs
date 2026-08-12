//! Fahrplan-Datenmodell (Plan 11).

use serde::{Deserialize, Serialize};
use track_model::{EdgeId, TrackNetwork, TrackPosition};

/// Ein Fahrplanhalt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScheduledStop {
    /// Betriebsstelle.
    pub name: String,
    /// Haltepunkt (Position der Zugspitze).
    pub edge: EdgeId,
    pub s: f64,
    /// Planmäßige Ankunft [s seit Simulationsbeginn].
    pub arrival: f64,
    /// Planmäßige Abfahrt [s].
    pub departure: f64,
    /// Gleis (nur Anzeige/Fahrstraßenwahl).
    #[serde(default)]
    pub platform: String,
}

impl ScheduledStop {
    /// Entfernung von `from` bis zum Haltepunkt [m], falls er innerhalb `max` voraus liegt.
    ///
    /// ponytail: sucht nur auf dem Fahrweg vorwärts in Meterschritten statt mit einer
    /// Graphsuche — bei 4 km Vorausschau sind das 4000 billige Schritte je Zug und Sekunde.
    /// Durch eine echte Wegsuche ersetzen, wenn Fahrpläne über viele Weichen führen.
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

/// Fahrplan eines Zuges.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Timetable {
    /// Zugnummer (z. B. „RE 4711").
    pub number: String,
    /// Zuggattung.
    #[serde(default)]
    pub category: String,
    pub stops: Vec<ScheduledStop>,
}

impl Timetable {
    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }

    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).expect("serialisierbar")
    }
}
