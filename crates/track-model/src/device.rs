//! Trackside equipment, country-neutral (plan ch. 5).
//!
//! The device kinds are deliberately only coarsely typed; the domain meaning of the
//! `payload` is known only to the respective country package (`sim-core::safety::de`
//! for Germany).

use crate::network::EdgeId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DeviceId(pub u32);

impl DeviceId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Which direction of travel a device applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Facing {
    /// Only in the direction of increasing arc length.
    #[default]
    Forward,
    /// Only in the direction of decreasing arc length.
    Backward,
    /// Both directions.
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

/// Device kind. The country-specific flavour lives in the `payload`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeviceKind {
    /// Main, distant or shunting signal; `payload` holds signal type and linkage.
    Signal,
    /// Intermittent track magnet (PZB 500/1000/2000 Hz).
    Magnet,
    /// Start/end of a line conductor section (LZB).
    LineConductor,
    /// Balise (ETCS-ready).
    Balise,
    /// Speed board (Lf 1–7).
    SpeedBoard,
    /// Platform with length and name.
    Platform,
    /// Stop board / halt board (Ne 5, H board).
    StopBoard,
    /// Block boundary / axle counting point.
    BlockMarker,
    /// Neutral section (de-energised section).
    NeutralSection,
    /// Anything else, open for country packages.
    Other(String),
}

/// A device at a location `(edge, s)` in the network.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TracksideDevice {
    #[serde(default = "default_device_id")]
    pub id: DeviceId,
    pub kind: DeviceKind,
    pub edge: EdgeId,
    /// Arc length along the edge [m].
    pub s: f64,
    #[serde(default)]
    pub facing: Facing,
    /// Lateral offset for rendering [m], positive = left of the direction of travel.
    #[serde(default)]
    pub lateral_offset: f64,
    /// Country-specific payload data as RON text.
    ///
    /// ponytail: text instead of `ron::Value` — `Value` loses unit enum variants
    /// (`Hz1000` becomes `Unit` when parsed), so no payload type with an enum would
    /// be deserialisable. The text stays hand-editable and only costs one `from_str`
    /// when read; cache the parsed form per device if the load becomes measurable.
    #[serde(default = "unit_payload")]
    pub payload: String,
}

/// Payload of a [`DeviceKind::Platform`]: `(name:"Musterstadt",length:210.0)`.
///
/// The device sits at the start of the platform (its lowest `s`) and `length` runs on
/// from there; which side of the track it is on is the sign of the device's
/// `lateral_offset` (positive = left of increasing arc length). `height` is the
/// platform surface above the railhead, for the people who wait on it — 0.76 m and
/// 0.55 m are the German standards. A line whose platforms are not modelled leaves it
/// at 0, and the crowd stands on the ground.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlatformPayload {
    #[serde(default)]
    pub name: String,
    pub length: f64,
    #[serde(default)]
    pub height: f64,
}

impl PlatformPayload {
    /// The payload of a platform device; `None` for a payload that is not one.
    pub fn parse(payload: &str) -> Option<Self> {
        ron::from_str(payload).ok()
    }
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

    /// Sets the payload from a serialisable structure.
    pub fn with_payload<T: Serialize>(mut self, payload: &T) -> Self {
        self.payload = ron::to_string(payload).expect("payload serialisable");
        self
    }

    /// Reads the payload as the target type.
    pub fn payload_as<T: for<'de> Deserialize<'de>>(&self) -> Option<T> {
        ron::from_str(&self.payload).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The editor's template names only what a platform needs; the height is optional
    /// and anything that is not a platform payload is `None`, not a panic.
    #[test]
    fn platform_payload_parses_with_and_without_height() {
        let plain = PlatformPayload::parse("(name:\"Musterstadt\",length:210.0)").unwrap();
        assert_eq!(plain.name, "Musterstadt");
        assert_eq!(plain.length, 210.0);
        assert_eq!(plain.height, 0.0);
        let built = PlatformPayload::parse("(name:\"\",length:140.0,height:0.76)").unwrap();
        assert_eq!(built.height, 0.76);
        assert!(PlatformPayload::parse("(frequency:Hz1000)").is_none());
    }
}
