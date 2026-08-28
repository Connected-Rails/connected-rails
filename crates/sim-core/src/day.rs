//! The operating day (plan ch. 11): a whole day of services on one line, looping every
//! 24 hours.
//!
//! A scenario is one prepared run — a start time, a task, and events that fire while it
//! is driven. An operating day is the other way a line is used: the timetable of a whole
//! day, every service in it, and the player picks one of them and drives it. Its times
//! are wall clock, so a service is at the same hour whichever evening it is taken, and
//! the plan starts over at midnight — the 23:40 out of Musterbach arrives at 00:12 by
//! wrapping, without the plan knowing what a day is.
//!
//! What the player still gets to say is [`RunSetup`]: the **date** the day plays on — the
//! plan's own by default, which is what puts the run in the right season — and the
//! **weather**, either generated for that day
//! ([`WeatherChoice::Dynamic`](crate::weather::WeatherChoice::Dynamic)) or one named
//! preset held throughout.
//!
//! **Multiplayer.** Every value here is content both peers load, and which services are
//! under way is a pure function of the clock ([`Service::runs_at`]) — so a client and the
//! server put the same trains on the line in the same order without a message about it.
//! The AI that drives them stays the server's, as ever, and so does the right to say no.

use crate::consist::{ConsistSource, ShuntWay, Spawn};
use crate::scenario::StartTime;
use crate::timetable::{DAY, ScheduledStop, Timetable, TimetableKind};
use crate::weather::{Dynamic, WeatherChoice};
use serde::{Deserialize, Serialize};

/// How long before its departure a service takes hold of a train \[s\] — it has to stand
/// at the platform before it leaves, not appear on the minute.
pub const LEAD: f64 = 300.0;

/// … and how long after its last arrival it gives the train up again \[s\].
pub const TAIL: f64 = 180.0;

/// How early the player is put in the cab before the service departs \[s\]. Long enough
/// to look at the road ahead, short enough not to be a wait.
pub const PREPARE: f64 = 120.0;

/// A calendar date — the day a run plays on.
///
/// Its own type rather than the date half of [`StartTime`] because the player dials it in
/// the menu and a dialled date has to roll over month and year ends properly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Date {
    pub year: i32,
    /// 1–12.
    pub month: u32,
    /// 1–31.
    pub day: u32,
}

impl Default for Date {
    /// Midsummer, like [`StartTime`]'s — the same day the simulator's fixed lighting had.
    fn default() -> Self {
        Self {
            year: 2026,
            month: 6,
            day: 21,
        }
    }
}

impl Date {
    /// Days since 1970-01-01, proleptic Gregorian (Howard Hinnant's civil algorithm).
    /// Only ever used to step a date by whole days, which is the one thing the menu does
    /// with it.
    pub fn day_number(self) -> i64 {
        let (y, m, d) = (
            i64::from(self.year),
            i64::from(self.month),
            i64::from(self.day),
        );
        // March is the first month of the era's year, so the leap day lands at its end.
        let y = y - i64::from(m <= 2);
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// The date `days` days after 1970-01-01 — the inverse of [`day_number`](Self::day_number).
    pub fn from_day_number(days: i64) -> Self {
        let z = days + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = mp + if mp < 10 { 3 } else { -9 };
        Self {
            year: (y + i64::from(m <= 2)) as i32,
            month: m as u32,
            day: d as u32,
        }
    }

    /// This date moved by `days`, rolling over month and year ends.
    pub fn shifted(self, days: i64) -> Self {
        Self::from_day_number(self.day_number() + days)
    }

    /// Whether the numbers name a day that exists — a mod may write 31 February.
    pub fn is_valid(self) -> bool {
        (1..=12).contains(&self.month)
            && self.day >= 1
            && Self::from_day_number(self.day_number()) == self
    }
}

/// One train's run within the day.
///
/// The stops carry seconds since local midnight, exactly as a `Daily`
/// [`Timetable`] does — [`timetable`](Self::timetable) is that timetable, and it is what
/// the AI drives to and what the player is scored against.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Service {
    /// Train number, e.g. "RE 4711".
    pub number: String,
    /// Train category — "RE", "RB", "ICE", "Gz".
    #[serde(default)]
    pub category: String,
    /// One sentence for the run picker.
    #[serde(default)]
    pub description: String,
    /// The vehicle at the head, `"<mod>:<file stem>"`. `None` = the one the player picked
    /// in the menu, which is also what an AI service falls back to.
    #[serde(default)]
    pub vehicle: Option<String>,
    /// Vehicles behind it.
    #[serde(default)]
    pub cars: usize,
    /// Where its stock stands before it leaves — a place on the line, or a road by name.
    /// A **portal** is what a service coming from a part of the railway that was never
    /// built starts at: its stock appears there and runs in ([`crate::consist::Spawn`]).
    pub origin: Spawn,
    /// The road its stock is put on when the working is over — a
    /// [`Yard`](crate::yard::Yard) of the line, by name. A stabling road holds the unit
    /// where it can be seen and where it occupies the track like any other train; a
    /// portal is the edge of the module and swallows it altogether.
    ///
    /// `None` leaves the unit standing at its terminus and takes it out of service on the
    /// spot, which is what a plan that says nothing means.
    #[serde(default)]
    pub stable_at: Option<String>,
    /// Which way the stock is taken there. Only the look of it hangs on this — the unit is
    /// placed on the road when the working's window closes, however far the driver got.
    #[serde(default)]
    pub stable_way: ShuntWay,
    /// Whether the player may take this service. A shunt move or a freight through the
    /// night can run for the look of the thing without being offered.
    #[serde(default = "yes")]
    pub playable: bool,
    /// Module whose local edge indices this service uses — resolved against the composed
    /// line by the mod runtime, then cleared. `None` falls back to the day's `module`.
    #[serde(default)]
    pub module: Option<String>,
    /// The stops, in order; times are seconds since local midnight.
    pub stops: Vec<ScheduledStop>,
}

/// How long a service holds its train after the last arrival \[s\].
///
/// A working that has a road to put its stock on needs longer than one that simply ends
/// at the platform: the unit still has to be driven there, and the window has to be open
/// while it is (see [`Service::tail`]).
pub const SHUNT_TAIL: f64 = 600.0;

fn yes() -> bool {
    true
}

impl Service {
    /// When it leaves its origin \[s since local midnight\].
    pub fn departure(&self) -> f64 {
        self.stops.first().map_or(0.0, |stop| stop.departure)
    }

    /// When it reaches its last stop \[s since local midnight\]; may read past
    /// [`DAY`] where the service runs over midnight.
    pub fn arrival(&self) -> f64 {
        self.stops.last().map_or(0.0, |stop| stop.arrival)
    }

    /// How long it is under way \[s\]. Taken around the clock, so a service that leaves
    /// at 23:40 and arrives at 00:12 lasts half an hour rather than minus a day.
    pub fn duration(&self) -> f64 {
        if self.stops.len() < 2 {
            return 0.0;
        }
        (self.arrival() - self.departure()).rem_euclid(DAY)
    }

    /// How long it holds its train after the last arrival \[s\] — [`TAIL`], or
    /// [`SHUNT_TAIL`] where there is a road to put the stock on and a move to drive.
    pub fn tail(&self) -> f64 {
        if self.stable_at.is_some() {
            SHUNT_TAIL
        } else {
            TAIL
        }
    }

    /// Whether the service holds a train at `clock` \[s since local midnight; a reading
    /// past a day is brought back into one\].
    ///
    /// It takes the train [`LEAD`] before departure and gives it up [`tail`](Self::tail)
    /// after the last arrival. A pure function of the clock — that is what lets two peers
    /// dispatch the same trains without agreeing on anything first.
    pub fn runs_at(&self, clock: f64) -> bool {
        let span = self.duration() + LEAD + self.tail();
        if span >= DAY {
            return true;
        }
        let from = (self.departure() - LEAD).rem_euclid(DAY);
        (clock - from).rem_euclid(DAY) < span
    }

    /// The timetable the AI drives to and the player is scored against: the same stops,
    /// read around the clock.
    pub fn timetable(&self) -> Timetable {
        Timetable {
            number: self.number.clone(),
            category: self.category.clone(),
            kind: TimetableKind::Daily,
            module: None,
            stops: self.stops.clone(),
        }
    }

    /// Origin and destination, for the run picker: "Musterbach → Musterstadt".
    pub fn route(&self) -> (String, String) {
        let first = self
            .stops
            .first()
            .map(|s| s.name.clone())
            .unwrap_or_default();
        let last = self
            .stops
            .last()
            .map(|s| s.name.clone())
            .unwrap_or_default();
        (first, last)
    }
}

/// A whole operating day on one line.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OperatingDay {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// The line it runs on, `"<mod>:<file stem>"` — a plain line or a composition.
    /// `--line` on the command line wins, as everywhere.
    #[serde(default)]
    pub line: Option<String>,
    /// The date the day plays on unless the player moves it. It decides the season and
    /// where the sun stands, so a winter timetable should carry a winter date.
    #[serde(default)]
    pub date: Date,
    /// Local clock ahead of UT \[h\] — Germany: 1 in winter, 2 in summer.
    #[serde(default = "central_european_summer")]
    pub utc_offset: f64,
    /// How the weather is decided unless the player overrides it.
    #[serde(default)]
    pub weather: WeatherChoice,
    /// Default module for the services' edge indices — see [`Service::module`].
    #[serde(default)]
    pub module: Option<String>,
    /// Stock that stands on the line from the first minute, whatever the plan does later:
    /// units in the sidings, a rake waiting to be collected, a light engine on shed. A
    /// consist that names a timetable is driven to it (plan ch. 11).
    #[serde(default)]
    pub consists: Vec<ConsistSource>,
    pub services: Vec<Service>,
}

fn central_european_summer() -> f64 {
    2.0
}

impl OperatingDay {
    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }

    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).expect("serializable")
    }

    /// What the run picker starts with: the plan's own date and weather.
    pub fn setup(&self) -> RunSetup {
        RunSetup {
            date: self.date,
            weather: self.weather,
        }
    }

    /// The services the player may take, with their index in [`services`](Self::services).
    pub fn playable(&self) -> impl Iterator<Item = (usize, &Service)> {
        self.services
            .iter()
            .enumerate()
            .filter(|(_, service)| service.playable)
    }

    /// Indices of the services under way at `clock`, in the order they left — which is
    /// the order both peers have to claim trains in for their worlds to match.
    pub fn active(&self, clock: f64) -> Vec<usize> {
        let mut active: Vec<usize> = (0..self.services.len())
            .filter(|&i| self.services[i].runs_at(clock))
            .collect();
        active.sort_by(|&a, &b| {
            let key = |i: usize| self.services[i].departure();
            key(a)
                .partial_cmp(&key(b))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.cmp(&b))
        });
        active
    }

    /// The wall clock the run begins at when `index` is the service taken: [`PREPARE`]
    /// before it departs, on `date` — or on the day before, where that lead-in crosses
    /// midnight.
    pub fn start_time(&self, index: usize, date: Date) -> StartTime {
        let departure = self
            .services
            .get(index)
            .map_or(0.0, |service| service.departure());
        let begin = departure - PREPARE;
        // `StartTime` counts in whole minutes, so the lead-in is taken to the minute
        // below: a service at 08:12 starts the run at 08:10, never at 08:10:37.
        let minute = (begin / 60.0).floor();
        let date = if begin < 0.0 { date.shifted(-1) } else { date };
        let minutes = (minute as i64).rem_euclid((DAY / 60.0) as i64);
        StartTime {
            year: date.year,
            month: date.month,
            day: date.day,
            hour: (minutes / 60) as u32,
            minute: (minutes % 60) as u32,
            utc_offset: self.utc_offset,
        }
    }
}

/// What the player set for a timetable run before it started (plan ch. 11).
///
/// The defaults come out of the plan ([`OperatingDay::setup`]); the run picker is where
/// they are moved. It travels to the dedicated server as part of the run's description,
/// because both sides have to stand in the same weather on the same date.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct RunSetup {
    pub date: Date,
    pub weather: WeatherChoice,
}

impl RunSetup {
    /// The seed a generated day of weather is made from.
    ///
    /// It comes out of the content — the plan's name and the date — and never out of a
    /// clock reading at start-up: the same service on the same date has to bring the same
    /// weather on every machine, or two players are driving through different rain
    /// (CLAUDE.md, ch. 20).
    pub fn seed(&self, day: &str) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |x: u64| {
            h ^= x;
            h = h.wrapping_mul(0x100_0000_01b3);
        };
        for byte in day.as_bytes() {
            mix(u64::from(*byte));
        }
        mix(self.date.day_number() as u64);
        h
    }

    /// The generator this setup asks for, if it asks for one.
    pub fn dynamic(&self, day: &str) -> Option<Dynamic> {
        match self.weather {
            WeatherChoice::Dynamic => Some(Dynamic {
                seed: self.seed(day),
                month: self.date.month,
            }),
            WeatherChoice::Fixed(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weather::Preset;
    use track_model::EdgeId;

    fn stop(name: &str, edge: u32, s: f64, arrival: f64, departure: f64) -> ScheduledStop {
        ScheduledStop {
            name: name.into(),
            edge: EdgeId(edge),
            s,
            arrival,
            departure,
            platform: "1".into(),
            module: None,
        }
    }

    fn service(number: &str, from: f64, to: f64) -> Service {
        Service {
            number: number.into(),
            category: "RE".into(),
            description: String::new(),
            vehicle: None,
            cars: 3,
            origin: Spawn::at(EdgeId(0), 200.0, 1),
            stable_at: None,
            stable_way: ShuntWay::default(),
            playable: true,
            module: None,
            stops: vec![
                stop("Musterbach", 0, 200.0, from, from),
                stop("Musterstadt", 2, 2_600.0, to, to + 60.0),
            ],
        }
    }

    #[test]
    fn a_date_rolls_over_month_and_year_ends() {
        let end = Date {
            year: 2026,
            month: 12,
            day: 31,
        };
        assert_eq!(
            end.shifted(1),
            Date {
                year: 2027,
                month: 1,
                day: 1
            }
        );
        assert_eq!(end.shifted(-365).year, 2025);
        // A leap year has the 29th and the year after it does not.
        let leap = Date {
            year: 2028,
            month: 2,
            day: 28,
        };
        assert_eq!(leap.shifted(1).day, 29);
        assert!(
            !Date {
                year: 2027,
                month: 2,
                day: 29
            }
            .is_valid()
        );
        assert!(leap.shifted(1).is_valid());
    }

    #[test]
    fn a_service_holds_its_train_around_its_own_hours() {
        let day = OperatingDay {
            services: vec![service("RE 4711", 8.0 * 3_600.0, 8.0 * 3_600.0 + 1_800.0)],
            ..Default::default()
        };
        let s = &day.services[0];
        assert!(!s.runs_at(7.0 * 3_600.0), "an hour before it, nothing");
        assert!(s.runs_at(8.0 * 3_600.0 - LEAD + 1.0), "it stands ready");
        assert!(s.runs_at(8.0 * 3_600.0 + 900.0), "it is under way");
        assert!(!s.runs_at(9.0 * 3_600.0), "and gone again");
        // The day loops: the same service is there again 24 h later.
        assert!(s.runs_at(8.0 * 3_600.0 + 900.0 + DAY));
    }

    #[test]
    fn a_service_over_midnight_keeps_its_train() {
        // 23:40 to 00:12 — half an hour, not minus a day.
        let night = service("RB 20", 23.0 * 3_600.0 + 2_400.0, 12.0 * 60.0);
        assert_eq!(night.duration(), 32.0 * 60.0);
        assert!(night.runs_at(23.0 * 3_600.0 + 3_000.0));
        assert!(night.runs_at(5.0 * 60.0), "still running after midnight");
        assert!(!night.runs_at(12.0 * 3_600.0), "but not at noon");
    }

    #[test]
    fn the_run_starts_shortly_before_the_service_leaves() {
        let day = OperatingDay {
            date: Date {
                year: 2026,
                month: 8,
                day: 15,
            },
            services: vec![
                service("RE 4711", 8.0 * 3_600.0 + 12.0 * 60.0, 9.0 * 3_600.0),
                // One minute past midnight: the lead-in falls on the day before.
                service("RB 20", 60.0, 3_600.0),
            ],
            ..Default::default()
        };
        let start = day.start_time(0, day.date);
        assert_eq!((start.hour, start.minute), (8, 10));
        assert_eq!(start.day, 15);

        let over = day.start_time(1, day.date);
        assert_eq!((over.hour, over.minute), (23, 59));
        assert_eq!(over.day, 14, "the run begins on the evening before");
    }

    #[test]
    fn active_services_are_the_same_list_on_every_machine() {
        let day = OperatingDay {
            services: vec![
                service("RE 4711", 8.0 * 3_600.0, 8.0 * 3_600.0 + 1_800.0),
                service("RB 20", 8.0 * 3_600.0 + 600.0, 8.0 * 3_600.0 + 2_400.0),
                service("RE 4713", 20.0 * 3_600.0, 20.0 * 3_600.0 + 1_800.0),
            ],
            ..Default::default()
        };
        // Sorted by departure, so two peers claim trains in the same order.
        assert_eq!(day.active(8.0 * 3_600.0 + 700.0), vec![0, 1]);
        assert_eq!(day.active(12.0 * 3_600.0), Vec::<usize>::new());
        assert_eq!(day.active(20.0 * 3_600.0 + 60.0), vec![2]);
    }

    #[test]
    fn the_weather_seed_is_content_and_nothing_else() {
        let setup = RunSetup {
            date: Date {
                year: 2026,
                month: 1,
                day: 9,
            },
            weather: WeatherChoice::Dynamic,
        };
        assert_eq!(setup.seed("example:tag"), setup.seed("example:tag"));
        assert_ne!(setup.seed("example:tag"), setup.seed("example:anderer"));
        // Another date is another day of weather.
        let next = RunSetup {
            date: setup.date.shifted(1),
            ..setup
        };
        assert_ne!(setup.seed("example:tag"), next.seed("example:tag"));
        // A January day generates off the cold ladder.
        let dynamic = setup.dynamic("example:tag").expect("dynamic");
        assert_eq!(dynamic.month, 1);
        assert!(dynamic.ladder().contains(&Preset::Snow));
        // And an overridden weather asks for no generator at all.
        assert!(
            RunSetup {
                weather: WeatherChoice::Fixed(Preset::Rain),
                ..setup
            }
            .dynamic("example:tag")
            .is_none()
        );
    }

    #[test]
    fn ron_roundtrip() {
        let day = OperatingDay {
            name: "Beispieltag".into(),
            description: "A day on the example line".into(),
            line: Some("example:beispielstrecke".into()),
            services: vec![service("RE 4711", 8.0 * 3_600.0, 9.0 * 3_600.0)],
            ..Default::default()
        };
        let back = OperatingDay::from_ron(&day.to_ron()).expect("RON readable");
        assert_eq!(back, day);
    }
}
