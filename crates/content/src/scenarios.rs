//! Example scenarios and the operating day on the Musterbahn (plan ch. 11.4).

use sim_core::consist::{ShuntWay, Spawn};
use sim_core::day::{Date, OperatingDay, Service};
use sim_core::scenario::{Action, Event, Scenario, Trigger};
use sim_core::timetable::{ScheduledStop, Timetable, TimetableKind};
use sim_core::weather::{Preset, WeatherChoice};
use track_model::EdgeId;

/// The two ends of the Musterbahn: the head of a train standing ready to leave.
fn musterbach() -> Spawn {
    Spawn::at(EdgeId(0), 200.0, 1)
}

fn musterstadt() -> Spawn {
    Spawn::at(EdgeId(2), 2_600.0, -1)
}

/// The Musterbahn's operating day: an hourly service each way, around the clock.
///
/// The built-in counterpart of a mod's `days/*.ron` — what the run picker offers when no
/// mod is installed at all. Seven kilometres of single track between two buffer stops, so
/// the plan runs one train at a time: out at twelve past, back at forty-two past, from
/// the first service at 05:12 to the last one before midnight. The times are wall clock
/// and the plan starts over at midnight, which is the whole point of it (plan ch. 11).
pub fn musterbahn_day() -> OperatingDay {
    let mut services = Vec::new();
    for hour in 5..24 {
        let hour_s = f64::from(hour) * 3_600.0;
        // Out at :12, ten minutes for the seven kilometres and the stop at the end.
        services.push(Service {
            number: format!("RB {}", 20_000 + hour * 2),
            category: "RB".into(),
            description: "Stündliche Regionalbahn, ein Halt, sieben Kilometer.".into(),
            vehicle: None,
            cars: 4,
            origin: musterbach(),
            stable_at: None,
            stable_way: ShuntWay::SetBack,
            playable: true,
            module: None,
            stops: vec![
                stop("Musterbach", 0, 200.0, hour_s + 720.0, hour_s + 720.0, "1"),
                stop(
                    "Musterstadt",
                    2,
                    2_600.0,
                    hour_s + 1_320.0,
                    hour_s + 1_320.0,
                    "2",
                ),
            ],
        });
        // … and back at :42, so the two never share the single track.
        services.push(Service {
            number: format!("RB {}", 20_001 + hour * 2),
            category: "RB".into(),
            description: "Die Gegenleistung — dieselbe Einheit, die andere Richtung.".into(),
            vehicle: None,
            cars: 4,
            origin: musterstadt(),
            stable_at: None,
            stable_way: ShuntWay::SetBack,
            playable: true,
            module: None,
            stops: vec![
                stop(
                    "Musterstadt",
                    2,
                    2_600.0,
                    hour_s + 2_520.0,
                    hour_s + 2_520.0,
                    "2",
                ),
                stop(
                    "Musterbach",
                    0,
                    200.0,
                    hour_s + 3_120.0,
                    hour_s + 3_120.0,
                    "1",
                ),
            ],
        });
    }
    OperatingDay {
        name: "Musterbahn".into(),
        description: "Stündlich zwischen Musterbach und Musterstadt, den ganzen Tag.".into(),
        line: None,
        // A late summer day — the season the ground and the trees wear, and where the
        // sun stands at the hour the service leaves.
        date: Date {
            year: 2026,
            month: 8,
            day: 15,
        },
        utc_offset: 2.0,
        weather: WeatherChoice::Dynamic,
        module: None,
        // The Musterbahn's stock is all in traffic: every unit belongs to a working, and
        // the two ends of the line are buffer stops with nowhere to stand anything.
        consists: Vec::new(),
        services,
    }
}

/// One scheduled stop; arrival and departure are seconds since midnight.
fn stop(
    name: &str,
    edge: u32,
    s: f64,
    arrival: f64,
    departure: f64,
    platform: &str,
) -> ScheduledStop {
    ScheduledStop {
        name: name.into(),
        edge: EdgeId(edge),
        s,
        arrival,
        departure,
        platform: platform.into(),
        module: None,
    }
}

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
        weather: Preset::Cloudy,
        description: "RE 4711 von Musterbach nach Musterstadt, 7 km. \
             Das Blocksignal bei km 2,0 zeigt zunächst Halt — der Vorausfahrende räumt gleich."
            .into(),
        consists: Vec::new(),
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
                    Action::SetWeather(Preset::Rain),
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
    fn the_operating_day_runs_one_train_at_a_time() {
        let day = musterbahn_day();
        assert_eq!(day.services.len(), 19 * 2);
        // Single track between two buffers: the plan must never have two of them out.
        for minute in 0..24 * 60 {
            let clock = f64::from(minute) * 60.0;
            let active = day.active(clock);
            assert!(
                active.len() <= 1,
                "{} services at {clock} s: {active:?}",
                active.len()
            );
        }
        // Every service is offered, and the day starts over: the 05:12 is there again
        // twenty-four hours later.
        assert_eq!(day.playable().count(), day.services.len());
        assert!(day.services[0].runs_at(5.0 * 3_600.0 + 720.0));
        assert!(day.services[0].runs_at(5.0 * 3_600.0 + 720.0 + 86_400.0));
    }

    #[test]
    fn the_operating_day_ron_roundtrip() {
        let day = musterbahn_day();
        let back = OperatingDay::from_ron(&day.to_ron()).expect("RON readable");
        assert_eq!(back, day);
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
