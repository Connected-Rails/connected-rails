//! Streckenausrüstung („Trackside"), länderneutral (Plan Kap. 5).
//!
//! Die Gerätetypen sind bewusst nur grob typisiert; die fachliche Bedeutung des `payload`
//! kennt allein das jeweilige Länderpaket (`sim-core::safety::de` für Deutschland).

use crate::network::EdgeId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DeviceId(pub u32);

impl DeviceId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Für welche Fahrtrichtung ein Gerät gilt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Facing {
    /// Nur in Richtung wachsender Bogenlänge.
    #[default]
    Forward,
    /// Nur in Richtung fallender Bogenlänge.
    Backward,
    /// Beide Richtungen.
    Both,
}

impl Facing {
    pub fn applies(self, direction: i8) -> bool {
        match self {
            Facing::Both => true,
            Facing::Forward => direction > 0,
            Facing::Backward => direction < 0,
        }
    }
}

/// Gerätetyp. Länderspezifische Ausprägung steckt im `payload`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeviceKind {
    /// Haupt-, Vor- oder Sperrsignal; `payload` enthält Signaltyp und Verknüpfung.
    Signal,
    /// Punktförmiger Gleismagnet (PZB 500/1000/2000 Hz).
    Magnet,
    /// Beginn/Ende eines Linienleiterabschnitts (LZB).
    LineConductor,
    /// Balise (ETCS-ready).
    Balise,
    /// Geschwindigkeitstafel (Lf 1–7).
    SpeedBoard,
    /// Bahnsteig mit Länge und Name.
    Platform,
    /// Halte-/Haltepunkttafel (Ne 5, H-Tafel).
    StopBoard,
    /// Blockgrenze / Achszählpunkt.
    BlockMarker,
    /// Schutzstrecke (spannungsloser Abschnitt).
    NeutralSection,
    /// Sonstiges, für Länderpakete offen.
    Other(String),
}

/// Ein Gerät an einer Stelle `(edge, s)` des Netzes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TracksideDevice {
    #[serde(default = "default_device_id")]
    pub id: DeviceId,
    pub kind: DeviceKind,
    pub edge: EdgeId,
    /// Bogenlänge auf der Kante [m].
    pub s: f64,
    #[serde(default)]
    pub facing: Facing,
    /// Seitlicher Versatz für die Darstellung [m], positiv = links der Fahrtrichtung.
    #[serde(default)]
    pub lateral_offset: f64,
    /// Länderspezifische Nutzdaten als RON-Text.
    ///
    /// ponytail: Text statt `ron::Value` — `Value` verliert Unit-Enum-Varianten
    /// (`Hz1000` wird beim Parsen zu `Unit`), damit wäre kein Payload-Typ mit Enum
    /// deserialisierbar. Der Text bleibt handeditierbar und kostet nur ein `from_str`
    /// beim Auslesen; bei messbarer Last die geparste Form je Gerät cachen.
    #[serde(default = "unit_payload")]
    pub payload: String,
}

fn default_device_id() -> DeviceId {
    DeviceId(u32::MAX)
}

fn unit_payload() -> String {
    "()".to_string()
}

impl TracksideDevice {
    pub fn new(kind: DeviceKind, edge: EdgeId, s: f64) -> Self {
        Self {
            id: default_device_id(),
            kind,
            edge,
            s,
            facing: Facing::Forward,
            lateral_offset: 0.0,
            payload: unit_payload(),
        }
    }

    pub fn with_facing(mut self, facing: Facing) -> Self {
        self.facing = facing;
        self
    }

    /// Setzt das Payload aus einer serialisierbaren Struktur.
    pub fn with_payload<T: Serialize>(mut self, payload: &T) -> Self {
        self.payload = ron::to_string(payload).expect("payload serialisierbar");
        self
    }

    /// Liest das Payload als Zieltyp.
    pub fn payload_as<T: for<'de> Deserialize<'de>>(&self) -> Option<T> {
        ron::from_str(&self.payload).ok()
    }
}
