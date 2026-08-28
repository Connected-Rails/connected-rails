//! Stabling roads and portals — where stock lives when it is not on a working
//! (plan ch. 11, "v1 trains spawn/despawn at fiddle yards").
//!
//! Shunting needs somewhere to shunt *to*. A line therefore names the places a train may
//! be put: a **stabling road** is a siding on the modelled line, and a **portal** is the
//! edge of it — the fiddle yard beyond the last signal, where a train that runs off the
//! module goes and where the stock of a working that starts elsewhere comes from. Both are
//! line content ([`LineSource::yards`](../../content/route/struct.LineSource.html)), placed
//! by the route editor next to the devices and objects, and qualified against module
//! offsets like everything else that names an edge.
//!
//! **Trains appear and disappear at portals, not anywhere.** [`Sim::place_at`] refuses a
//! road that is too short or already occupied, and [`Sim::withdraw`] refuses to take a
//! train off the line anywhere but at a portal it is actually standing at. A consist taken
//! off keeps its slot in [`Sim::trains`] and becomes `stabled` — the indices are what
//! everything from the AI driver to the network protocol addresses trains by, so nothing
//! is ever removed (see [`crate::shunt`]).
//!
//! **Multiplayer:** a yard is line data, so every peer has the same list at the same
//! indices; putting a train on a road is a deterministic function of the world and needs
//! no message of its own. Who is allowed to *ask* for it is the server's business, exactly
//! as it is for a route request.

use crate::Sim;
use serde::{Deserialize, Serialize};
use track_model::TrackPosition;

/// What a place for stock is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum YardKind {
    /// A siding on the modelled line: a train left here stands where it can be seen, and
    /// occupies its road like any other train.
    #[default]
    Stabling,
    /// The edge of the line — a fiddle yard, a junction beyond the last signal, the
    /// neighbouring railway. Trains appear and disappear here and nowhere else.
    Portal,
}

impl YardKind {
    /// Message key of the kind's name.
    pub fn key(self) -> &'static str {
        match self {
            YardKind::Stabling => "yard-kind-stabling",
            YardKind::Portal => "yard-kind-portal",
        }
    }
}

/// A place for stock, resolved onto the track graph.
///
/// `at` is where the **head** of a standing train comes to, and its direction is the one
/// the train faces — a road is not just a place, it is a place with a way round.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Yard {
    /// What a timetable, an operating day or a shunt job addresses it by.
    pub name: String,
    pub kind: YardKind,
    pub at: TrackPosition,
    /// Usable length [m]; `0` = not stated, and then nothing is refused for being long.
    pub length: f64,
}

impl Yard {
    /// Whether a consist of `length` metres fits.
    pub fn fits(&self, length: f64) -> bool {
        self.length <= 0.0 || length <= self.length
    }
}

/// Why a train could not be put on a road, or taken off the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum YardError {
    /// The line has no yard of that name.
    NoYard,
    /// No such train.
    NoTrain,
    /// The consist is longer than the road.
    TooLong,
    /// Something else is standing on the road.
    Occupied,
    /// The road runs off the end of the graph — the consist does not fit on the track
    /// behind the mark.
    OffTheGraph,
    /// Only a portal may swallow a train.
    NotAPortal,
    /// The train is not standing at that portal.
    NotThere,
    /// It is still moving.
    Moving,
}

impl YardError {
    /// Message key of the refusal.
    pub fn key(self) -> &'static str {
        match self {
            YardError::NoYard => "yard-refused-no-yard",
            YardError::NoTrain => "yard-refused-no-train",
            YardError::TooLong => "yard-refused-too-long",
            YardError::Occupied => "yard-refused-occupied",
            YardError::OffTheGraph => "yard-refused-off-the-graph",
            YardError::NotAPortal => "yard-refused-not-a-portal",
            YardError::NotThere => "yard-refused-not-there",
            YardError::Moving => "yard-refused-moving",
        }
    }
}

/// How near the mark a train has to stand for a portal to swallow it [m].
///
/// A portal is a place, not a point: the train has run off the end of the modelled line,
/// and half a coach either way is nobody's business.
pub const PORTAL_REACH: f64 = 30.0;

impl Sim {
    /// The yard of that name, if the line has one.
    pub fn yard(&self, name: &str) -> Option<&Yard> {
        self.yards.iter().find(|y| y.name == name)
    }

    /// Puts `train` on the road `yard` and brings it into service.
    ///
    /// The consist is lined up with its head on the mark, facing the way the road faces,
    /// and its `stabled` flag comes off — that is how a train appears at a portal, and how
    /// a unit is put away on a siding at the end of its day.
    ///
    /// Refuses when the road is too short for the consist, when another train is standing
    /// on it, or when the track behind the mark runs out before the consist does.
    pub fn place_at(&mut self, train: usize, yard: &str) -> Result<(), YardError> {
        let mark = self.yard(yard).ok_or(YardError::NoYard)?.clone();
        let consist = self.trains.get(train).ok_or(YardError::NoTrain)?;
        if consist.vehicles.is_empty() {
            return Err(YardError::NoTrain);
        }
        let length = consist.length();
        if !mark.fits(length) {
            return Err(YardError::TooLong);
        }
        // The road has to be free — a train placed into another one is exactly the
        // surprise this refuses.
        if !self.road_is_clear(&mark, length, train) {
            return Err(YardError::Occupied);
        }
        // And the track behind the mark has to be there at all.
        if mark.at.offset_by(&self.net, -length).is_none() {
            return Err(YardError::OffTheGraph);
        }

        let net = std::mem::replace(&mut self.net, track_model::TrackNetwork::new());
        let consist = &mut self.trains[train];
        consist.place_head_at(mark.at, &net);
        consist.stabled = false;
        for vehicle in &mut consist.vehicles {
            vehicle.v = 0.0;
            vehicle.a = 0.0;
        }
        self.net = net;
        Ok(())
    }

    /// Takes `train` off the line at the portal `yard`.
    ///
    /// The consist keeps its slot and becomes an empty-of-duty `stabled` train: not
    /// driven, not drawn, and no longer occupying the track. It keeps its vehicles, so the
    /// same unit can be put back on the line later with [`Sim::place_at`].
    ///
    /// Refuses anywhere but at a portal, and only for a train standing within
    /// [`PORTAL_REACH`] of the mark.
    pub fn withdraw(&mut self, train: usize, yard: &str) -> Result<(), YardError> {
        let mark = self.yard(yard).ok_or(YardError::NoYard)?.clone();
        if mark.kind != YardKind::Portal {
            return Err(YardError::NotAPortal);
        }
        let consist = self.trains.get(train).ok_or(YardError::NoTrain)?;
        let head = consist.head().ok_or(YardError::NoTrain)?;
        if consist.speed().abs() > crate::shunt::STANDSTILL {
            return Err(YardError::Moving);
        }
        if head
            .distance_to(&self.net, &mark.at, PORTAL_REACH)
            .is_none_or(|d| d.abs() > PORTAL_REACH)
        {
            return Err(YardError::NotThere);
        }
        self.trains[train].stabled = true;
        Ok(())
    }

    /// Is the stretch of `length` metres behind `mark` free of other trains?
    fn road_is_clear(&self, mark: &Yard, length: f64, except: usize) -> bool {
        for (index, train) in self.trains.iter().enumerate() {
            if index == except || train.stabled {
                continue;
            }
            for vehicle in &train.vehicles {
                // Measured against the road's own direction: a vehicle between the mark
                // and `length` behind it stands where the consist would go.
                let Some(d) = mark.at.distance_to(&self.net, &vehicle.pos, length) else {
                    continue;
                };
                if (-length..=0.0).contains(&d) {
                    return false;
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brakes::{BrakeKind, BrakeSpec};
    use crate::train::{Train, Vehicle, VehicleSpec};
    use track_model::{EdgeId, NodeKind, Segment, TrackEdge, TrackNetwork};
    use world_coords::geo::to_ecef_deg;

    fn line() -> Sim {
        let mut net = TrackNetwork::new();
        let a = net.add_node(NodeKind::Buffer);
        let b = net.add_node(NodeKind::Buffer);
        net.add_edge(TrackEdge::new(
            EdgeId(0),
            a,
            b,
            to_ecef_deg(52.0, 10.0, 100.0),
            0.0,
            vec![Segment::straight(2000.0)],
        ));
        net.finish();
        let mut sim = Sim::new(net, crate::interlock::Interlock::default(), 1);
        sim.yards = vec![
            Yard {
                name: "Abstellgleis 1".into(),
                kind: YardKind::Stabling,
                at: TrackPosition::new(EdgeId(0), 1000.0, 1),
                length: 100.0,
            },
            Yard {
                name: "Portal West".into(),
                kind: YardKind::Portal,
                at: TrackPosition::new(EdgeId(0), 60.0, -1),
                length: 0.0,
            },
        ];
        sim
    }

    fn rake(count: usize) -> Vec<VehicleSpec> {
        (0..count)
            .map(|i| VehicleSpec {
                name: format!("w{i}"),
                length: 20.0,
                mass_empty: 40_000.0,
                brake: BrakeSpec::from_brake_weight(30.0, BrakeKind::Block),
                ..VehicleSpec::default()
            })
            .collect()
    }

    fn add(sim: &mut Sim, specs: Vec<VehicleSpec>, head: TrackPosition) -> usize {
        let vehicles = specs
            .into_iter()
            .map(|s| Vehicle::new(s, head))
            .collect::<Vec<_>>();
        let train = Train::assemble(vehicles, head, &sim.net);
        sim.add_train(train)
    }

    /// A stabled unit put on a road stands on it, facing the way the road faces, and is
    /// back in service.
    #[test]
    fn a_unit_put_on_a_stabling_road_stands_on_it() {
        let mut sim = line();
        let unit = add(&mut sim, rake(3), TrackPosition::new(EdgeId(0), 1900.0, 1));
        sim.trains[unit].stabled = true;
        sim.place_at(unit, "Abstellgleis 1").expect("fits");
        assert!(!sim.trains[unit].stabled);
        let head = sim.trains[unit].head().expect("has a head");
        assert!((head.s - 1000.0).abs() < 1e-6);
        assert_eq!(head.dir, 1);
        assert!((sim.trains[unit].vehicles[2].pos.s - 950.0).abs() < 1.0);
    }

    /// The refusals: too long for the road, somebody already on it, no such name.
    #[test]
    fn a_road_that_will_not_take_the_train_says_so() {
        let mut sim = line();
        let long = add(&mut sim, rake(6), TrackPosition::new(EdgeId(0), 1900.0, 1));
        assert_eq!(
            sim.place_at(long, "Abstellgleis 1"),
            Err(YardError::TooLong)
        );
        assert_eq!(sim.place_at(long, "gibt es nicht"), Err(YardError::NoYard));
        assert_eq!(sim.place_at(99, "Abstellgleis 1"), Err(YardError::NoTrain));

        let mut sim = line();
        let sitting = add(&mut sim, rake(3), TrackPosition::new(EdgeId(0), 1000.0, 1));
        assert!(!sim.trains[sitting].stabled);
        let other = add(&mut sim, rake(2), TrackPosition::new(EdgeId(0), 1900.0, 1));
        assert_eq!(
            sim.place_at(other, "Abstellgleis 1"),
            Err(YardError::Occupied)
        );
        // With the road cleared it goes on.
        sim.trains[sitting].stabled = true;
        assert!(sim.place_at(other, "Abstellgleis 1").is_ok());
    }

    /// Trains only disappear at portals, and only where the portal is.
    #[test]
    fn a_train_only_disappears_at_a_portal_it_stands_at() {
        let mut sim = line();
        let unit = add(&mut sim, rake(2), TrackPosition::new(EdgeId(0), 1000.0, 1));
        assert_eq!(
            sim.withdraw(unit, "Abstellgleis 1"),
            Err(YardError::NotAPortal)
        );
        assert_eq!(sim.withdraw(unit, "Portal West"), Err(YardError::NotThere));

        sim.place_at(unit, "Portal West")
            .expect("the portal takes it");
        assert_eq!(sim.trains[unit].head().expect("head").dir, -1);
        // Rolling, it is not taken off.
        sim.trains[unit].vehicles[0].v = 3.0;
        assert_eq!(sim.withdraw(unit, "Portal West"), Err(YardError::Moving));
        sim.trains[unit].vehicles[0].v = 0.0;
        sim.withdraw(unit, "Portal West").expect("swallows it");
        assert!(sim.trains[unit].stabled);
        // It kept its slot and its vehicles, so it can come back out.
        assert_eq!(sim.trains.len(), 1);
        assert_eq!(sim.trains[unit].vehicles.len(), 2);
        assert!(sim.place_at(unit, "Portal West").is_ok());
        assert!(!sim.trains[unit].stabled);
    }
}
