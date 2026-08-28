//! Where a train comes from and what it is made of (plan ch. 11).
//!
//! Two things a scenario and an operating day both need, and neither could say before:
//!
//! * **A spawn point.** [`Spawn`] is where a train is put on the line — a place on the
//!   track graph, or a road of the line by name. Naming a **portal** is what makes a
//!   train come out of the part of the railway that was never built: the stock appears
//!   there, runs in, and at the end of its working runs off the same way and is gone
//!   ([`crate::yard`]).
//! * **A consist.** [`ConsistSource`] is a train that stands on the line before anything
//!   moves: what it is made of, head first, where it stands, and the timetable the AI
//!   drives it to. A scenario's list is what its events address by index, and
//!   `player_train` picks which of them is the player's — before this, a scenario could
//!   name a train in an event but not put one there.
//!
//! **Multiplayer.** All of it is content both peers load, and building a world out of it
//! is a pure function of that content: the same file gives the same trains at the same
//! indices, which is what the world fingerprint checks on joining.

use crate::Sim;
use serde::{Deserialize, Serialize};
use track_model::{EdgeId, TrackPosition};

/// Where a train is put on the line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Spawn {
    /// A point on the track graph: the head of the train, and which way it faces.
    At {
        edge: EdgeId,
        /// Arc length along the edge \[m\].
        s: f64,
        /// `+1`: the train leaves towards rising `s`, `-1`: the other way.
        #[serde(default = "forwards")]
        dir: i8,
        /// Module whose local `edge` index this uses — resolved against the composed line
        /// by the mod runtime, then cleared, like every other index in a mod's files.
        #[serde(default)]
        module: Option<String>,
    },
    /// A road of the line by name ([`Sim::yard`]) — a stabling road, or a portal, which
    /// is where a train comes from when the railway beyond it was never built.
    Yard(String),
}

fn forwards() -> i8 {
    1
}

impl Spawn {
    /// A place on the graph, for the common case of writing one out in code.
    pub fn at(edge: EdgeId, s: f64, dir: i8) -> Self {
        Spawn::At {
            edge,
            s,
            dir,
            module: None,
        }
    }

    /// Where it puts the head of the train, as far as this run knows. `None` for a road
    /// the line does not have — a mod's mistake, and a warning rather than a crash.
    pub fn position(&self, sim: &Sim) -> Option<TrackPosition> {
        match self {
            Spawn::At { edge, s, dir, .. } => sim
                .net
                .edges()
                .get(edge.index())
                .filter(|track| *s <= track.length())
                .map(|_| TrackPosition::new(*edge, *s, *dir)),
            Spawn::Yard(name) => sim.yard(name).map(|yard| yard.at),
        }
    }

    /// The module a local edge index of this spawn belongs to, if it names one.
    pub fn module(&self) -> Option<&str> {
        match self {
            Spawn::At { module, .. } => module.as_deref(),
            Spawn::Yard(_) => None,
        }
    }

    /// Shifts a module-local edge index by that module's offset and forgets the module —
    /// the mod runtime's half of the `module` field. Taking the name is what makes a
    /// second pass a no-op; a road is named, not indexed, and needs no shifting at all.
    pub fn shift(&mut self, offset: u32) {
        if let Spawn::At { edge, module, .. } = self
            && module.take().is_some()
        {
            edge.0 += offset;
        }
    }

    /// The road this names, if it names one.
    pub fn yard(&self) -> Option<&str> {
        match self {
            Spawn::Yard(name) => Some(name),
            Spawn::At { .. } => None,
        }
    }

    /// Whether it is a portal — the edge of the modelled railway, where a train comes
    /// from nowhere and goes to nowhere.
    pub fn is_portal(&self, sim: &Sim) -> bool {
        self.yard()
            .and_then(|name| sim.yard(name))
            .is_some_and(|yard| yard.kind == crate::yard::YardKind::Portal)
    }
}

/// Which way a unit is taken to the road it is left on.
///
/// Getting it wrong costs only the look of the thing: the move is best effort, and the
/// unit is *placed* on the road when its working's window closes whatever the driver
/// managed (see `app::services`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ShuntWay {
    /// Set back onto it — the road takes the rear of the train first, which is what a
    /// unit terminating at a platform does with the siding behind it.
    #[default]
    SetBack,
    /// Draw forward onto it, for a road that lies ahead of where the working ends.
    DrawUp,
}

/// A stretch of one kind of vehicle in a consist.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Formation {
    /// The vehicle, `"<mod>:<file stem>"`.
    pub vehicle: String,
    /// How many of it, one behind the other.
    #[serde(default = "one")]
    pub count: usize,
    /// Which of the vehicle's liveries they run in.
    #[serde(default)]
    pub variant: Option<usize>,
}

fn one() -> usize {
    1
}

impl Formation {
    /// One vehicle of that kind.
    pub fn single(vehicle: &str) -> Self {
        Self {
            vehicle: vehicle.into(),
            count: 1,
            variant: None,
        }
    }

    /// `count` of them.
    pub fn several(vehicle: &str, count: usize) -> Self {
        Self {
            vehicle: vehicle.into(),
            count,
            variant: None,
        }
    }
}

/// A train that stands on the line before anything moves.
///
/// The order of a scenario's or a day's list is the order the trains are built in, so it
/// is also the order their indices run in: `player_train` and an event's `train:` address
/// this list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsistSource {
    /// Train number for the radio and the HUD — "RE 4711", "Gz 51230".
    #[serde(default)]
    pub number: String,
    /// What it is made of, head first.
    pub vehicles: Vec<Formation>,
    /// Where it stands.
    pub at: Spawn,
    /// Ready to move: battery on, pantograph up, main switch in. `false` is a cold engine
    /// the driver has to wake up, which is a scenario of its own.
    #[serde(default = "yes")]
    pub prepared: bool,
    /// The timetable the AI drives it to, `"<mod>:<file stem>"`. Without one it stands
    /// where it was put — which is what stock stabled in a siding does all day.
    #[serde(default)]
    pub timetable: Option<String>,
    /// A shunt job it works — draw up, set back, couple, uncouple, stand
    /// ([`ShuntJob`](crate::shunt::ShuntJob)).
    ///
    /// It stands entirely on its own: a consist with a job and no timetable is a shunting
    /// movement and nothing else, which is what a pilot working a yard is. With both, the
    /// job is worked once the last stop of the timetable has been made — the unit runs its
    /// service and is then put away.
    #[serde(default)]
    pub shunt: Option<crate::shunt::ShuntJob>,
    /// Module whose local indices this consist uses; the spawn's own `module` wins.
    #[serde(default)]
    pub module: Option<String>,
}

fn yes() -> bool {
    true
}

impl ConsistSource {
    /// How many vehicles it comes to.
    pub fn length(&self) -> usize {
        self.vehicles.iter().map(|part| part.count).sum()
    }

    /// Every vehicle id it names, head first, one entry per vehicle.
    pub fn each_vehicle(&self) -> impl Iterator<Item = (&str, Option<usize>)> {
        self.vehicles
            .iter()
            .flat_map(|part| std::iter::repeat_n((part.vehicle.as_str(), part.variant), part.count))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_consist_is_read_head_first() {
        let consist = ConsistSource {
            number: "RE 4711".into(),
            vehicles: vec![
                Formation::single("example:br101_afb"),
                Formation::several("example:bmz", 3),
            ],
            at: Spawn::at(EdgeId(0), 200.0, 1),
            prepared: true,
            timetable: None,
            shunt: None,
            module: None,
        };
        assert_eq!(consist.length(), 4);
        let ids: Vec<&str> = consist.each_vehicle().map(|(id, _)| id).collect();
        assert_eq!(
            ids,
            [
                "example:br101_afb",
                "example:bmz",
                "example:bmz",
                "example:bmz"
            ]
        );
    }

    #[test]
    fn a_module_local_spawn_is_shifted_once_and_a_road_never() {
        let mut local = Spawn::At {
            edge: EdgeId(1),
            s: 100.0,
            dir: 1,
            module: Some("ost".into()),
        };
        local.shift(7);
        assert_eq!(local, Spawn::at(EdgeId(8), 100.0, 1));
        // Applying it twice must not shift twice — the name is taken with the offset.
        local.shift(7);
        assert_eq!(local, Spawn::at(EdgeId(8), 100.0, 1));

        // A road is addressed by name, so a composition leaves it exactly as it is.
        let mut road = Spawn::Yard("Portal Ost".into());
        road.shift(7);
        assert_eq!(road, Spawn::Yard("Portal Ost".into()));
        assert_eq!(road.yard(), Some("Portal Ost"));
    }

    #[test]
    fn ron_roundtrip() {
        for spawn in [
            Spawn::at(EdgeId(2), 2_600.0, -1),
            Spawn::Yard("Abstellgleis 1".into()),
        ] {
            let text = ron::to_string(&spawn).expect("serializable");
            assert_eq!(ron::from_str::<Spawn>(&text).expect("readable"), spawn);
        }
    }
}
