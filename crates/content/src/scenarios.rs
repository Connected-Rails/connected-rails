//! Example scenarios on the Musterbahn (plan ch. 11.4).

use sim_core::scenario::{Action, Event, Scenario, Trigger};
use sim_core::timetable::{ScheduledStop, Timetable, TimetableKind};
use sim_core::train::RailCondition;
use track_model::EdgeId;

/// Timetable of the scenario "Regionalbahn nach Musterstadt".
pub fn re_4711() -> Timetable {
    Timetable {
        number: "RE 4711".into(),
        category: "RE".into(),
        kind: TimetableKind::Scenario,
        module: None,
        stops: vec![ScheduledStop {
            name: "Musterstadt".into(),
            edge: EdgeId(2),
            s: 2600.0,
            arrival: 420.0,
            departure: 480.0,
            platform: "2".into(),
            module: None,
        }],
    }
}

/// Scenario: departure, the block signal shows stop at first, rain sets in,
/// the goal is the punctual stop at the platform in Musterstadt.
pub fn to_musterstadt() -> Scenario {
    Scenario {
        name: "Regionalbahn nach Musterstadt".into(),
        start: Default::default(),
        description: "RE 4711 von Musterbach nach Musterstadt, 7 km. \
             Das Blocksignal bei km 2,0 zeigt zunächst Halt — der Vorausfahrende räumt gleich."
            .into(),
        player_train: 0,
        events: vec![
            Event {
                name: "abfahrt".into(),
                trigger: Trigger::Time(5.0),
                actions: vec![Action::Announcement(
                    "RE 4711 nach Musterstadt, Abfahrt frei. Zulässig 160 km/h.".into(),
                )],
                once: true,
                module: None,
            },
            Event {
                name: "block_frei".into(),
                trigger: Trigger::TrainPast {
                    train: 0,
                    edge: EdgeId(0),
                    s: 1200.0,
                },
                actions: vec![Action::Message(
                    "Zug 2 hat den Block geräumt — Signal geht auf Fahrt.".into(),
                )],
                once: true,
                module: None,
            },
            Event {
                name: "regen".into(),
                trigger: Trigger::After {
                    event: "block_frei".into(),
                    delay: 30.0,
                },
                actions: vec![
                    Action::SetRail(RailCondition::Wet),
                    Action::Message("Regen setzt ein — Bremswege werden länger.".into()),
                ],
                once: true,
                module: None,
            },
            Event {
                name: "einfahrt".into(),
                trigger: Trigger::TrainPast {
                    train: 0,
                    edge: EdgeId(2),
                    s: 1500.0,
                },
                actions: vec![Action::Announcement(
                    "In Kürze Musterstadt, Bahnsteig 2, Halt an der Haltetafel bei km 6,6.".into(),
                )],
                once: true,
                module: None,
            },
            Event {
                name: "zwangsbremsung".into(),
                trigger: Trigger::ForcedBrake { train: 0 },
                actions: vec![
                    Action::Message(
                        "Zwangsbremsung! Bitte Meldung an den Fahrdienstleiter.".into(),
                    ),
                    Action::Score {
                        points: -50,
                        reason: "Zwangsbremsung im Szenario".into(),
                    },
                ],
                once: true,
                module: None,
            },
            Event {
                name: "ziel".into(),
                trigger: Trigger::TrainStopped {
                    train: 0,
                    edge: EdgeId(2),
                    s: 2600.0,
                    radius: 50.0,
                },
                actions: vec![Action::Finish {
                    success: true,
                    reason: "Musterstadt erreicht".into(),
                }],
                once: true,
                module: None,
            },
            Event {
                name: "vorbeigefahren".into(),
                trigger: Trigger::TrainPast {
                    train: 0,
                    edge: EdgeId(2),
                    s: 2900.0,
                },
                actions: vec![Action::Finish {
                    success: false,
                    reason: "Bahnsteig überfahren".into(),
                }],
                once: true,
                module: None,
            },
        ],
        script: None,
        timetable: None,
        line: None,
        module: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::interlock::SignalId;

    #[test]
    fn scenario_ron_roundtrip() {
        let scenario = to_musterstadt();
        let text = scenario.to_ron();
        let back = Scenario::from_ron(&text).expect("RON readable");
        assert_eq!(back, scenario);
        assert_eq!(back.events.len(), 7);
    }

    #[test]
    fn timetable_ron_roundtrip() {
        let tt = re_4711();
        let back = Timetable::from_ron(&tt.to_ron()).expect("RON readable");
        assert_eq!(back, tt);
    }

    #[test]
    fn signal_trigger_is_expressible() {
        // The trigger exists and is serializable — used by custom scenarios.
        let t = Trigger::SignalStop {
            signal: SignalId(1),
            stop: true,
        };
        let text = ron::to_string(&t).unwrap();
        assert_eq!(ron::from_str::<Trigger>(&text).unwrap(), t);
    }
}
