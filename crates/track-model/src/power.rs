//! Electrification of the line (plan ch. 15): what hangs over the track.
//!
//! A supply system is line data as much as vehicle data — the wire carries what it
//! carries, and a locomotive is built for one or more of those systems. Keeping the enum
//! here rather than in the simulation is what lets both sides name the same thing:
//! [`crate::TrackNetwork`] states what a section is electrified with, and the vehicle
//! states what it can work under.

use serde::{Deserialize, Serialize};

/// A railway supply system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PowerSystem {
    /// 15 kV 16.7 Hz — Germany, Austria, Switzerland, Scandinavia.
    #[default]
    Ac15kv,
    /// 25 kV 50 Hz — France, Britain, most new construction.
    Ac25kv,
    /// 3 kV DC — Italy, Poland, Belgium.
    Dc3kv,
    /// 1.5 kV DC — Netherlands, southern France.
    Dc1500v,
    /// Third rail, 750 V DC — collected by a shoe from the side, not from above.
    ThirdRail,
}

impl PowerSystem {
    /// Nominal voltage [V].
    pub fn voltage(self) -> f64 {
        match self {
            PowerSystem::Ac15kv => 15_000.0,
            PowerSystem::Ac25kv => 25_000.0,
            PowerSystem::Dc3kv => 3_000.0,
            PowerSystem::Dc1500v => 1_500.0,
            PowerSystem::ThirdRail => 750.0,
        }
    }

    /// Lowest voltage a main switch built for this system may still close at [V].
    pub fn minimum(self) -> f64 {
        self.voltage() * 2.0 / 3.0
    }

    /// A shoe neither rises nor falls; everything else hangs on a pantograph.
    pub fn is_third_rail(self) -> bool {
        matches!(self, PowerSystem::ThirdRail)
    }

    /// Stable id used in line and vehicle files and in the editors.
    pub fn id(self) -> &'static str {
        match self {
            PowerSystem::Ac15kv => "ac-15kv",
            PowerSystem::Ac25kv => "ac-25kv",
            PowerSystem::Dc3kv => "dc-3kv",
            PowerSystem::Dc1500v => "dc-1.5kv",
            PowerSystem::ThirdRail => "third-rail",
        }
    }

    pub const ALL: [PowerSystem; 5] = [
        PowerSystem::Ac15kv,
        PowerSystem::Ac25kv,
        PowerSystem::Dc3kv,
        PowerSystem::Dc1500v,
        PowerSystem::ThirdRail,
    ];

    /// Reads an id back; an unknown one is not a system.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }
}

/// What a section of track is electrified with. `None` is a section under no wire at all —
/// a branch line, a siding, or the gap either side of a system boundary.
pub type Electrification = Option<PowerSystem>;

/// The id an [`Electrification`] is written as, `"none"` where there is no wire.
pub fn electrification_id(value: Electrification) -> &'static str {
    match value {
        Some(system) => system.id(),
        None => "none",
    }
}

/// Reads one back.
pub fn electrification_from_id(id: &str) -> Electrification {
    PowerSystem::from_id(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_and_unknown_is_no_wire() {
        for system in PowerSystem::ALL {
            assert_eq!(PowerSystem::from_id(system.id()), Some(system));
            assert_eq!(electrification_id(Some(system)), system.id());
        }
        assert_eq!(electrification_from_id("none"), None);
        assert_eq!(electrification_from_id("ac-50kv"), None);
        assert_eq!(electrification_id(None), "none");
    }

    #[test]
    fn the_minimum_is_below_the_nominal_voltage_of_every_system() {
        for system in PowerSystem::ALL {
            assert!(system.minimum() < system.voltage());
            assert!(system.minimum() > 0.0);
        }
        // A 15 kV switch must not close on 1.5 kV, and a 1.5 kV one must not on 750 V.
        assert!(PowerSystem::Dc1500v.voltage() < PowerSystem::Ac15kv.minimum());
        assert!(PowerSystem::ThirdRail.voltage() < PowerSystem::Dc1500v.minimum());
    }
}
