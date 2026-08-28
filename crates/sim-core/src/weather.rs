//! Weather (plan 14.1) — one state for the whole world, and what it leaves behind.
//!
//! Two types: [`Weather`] is what the sky looks like at one moment (cover, what is
//! falling and how hard, wind, sight, temperature), and [`Timeline`] is that value
//! over time — the keyframe the run came from, the one it is moving to, and the
//! water and snow the weather has left on the ground since.
//!
//! **Multiplayer.** Nothing here is replicated. Between two scenario actions the
//! weather is a pure function of the simulation clock, and the accumulations run in
//! the fixed 200 Hz step, so every peer that has seen the same
//! [`SetWeather`](crate::scenario::Action::SetWeather) is in the same rain. A
//! scenario action is what travels, and it travels already.

use crate::train::RailCondition;
use serde::{Deserialize, Serialize};

/// What is falling out of the sky.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Precip {
    #[default]
    None,
    Rain,
    /// Rain and snow together, around freezing — the worst thing for adhesion.
    Sleet,
    Snow,
    Hail,
}

impl Precip {
    /// Whether this falls as a streak (rain) rather than a flake (snow).
    pub fn is_liquid(self) -> bool {
        matches!(self, Precip::Rain | Precip::Sleet | Precip::Hail)
    }
}

/// The weather at one moment.
///
/// Every field is a physical quantity rather than a switch, because the renderer,
/// the physics and the sound all want a different part of it: the sky reads `cover`,
/// the particle field reads `precip` and `rate`, the atmosphere reads `visibility`,
/// and the rail reads what the rate has added up to (see [`Timeline`]).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Weather {
    /// Cloud cover, 0 = clear … 1 = closed deck.
    pub cover: f32,
    /// Height of the cloud base above the ground \[m\].
    pub base: f32,
    pub precip: Precip,
    /// Precipitation rate \[mm/h\]: 0.5 drizzle, 4 rain, 20 downpour.
    pub rate: f32,
    /// Wind speed \[m/s\].
    pub wind: f32,
    /// Direction the wind blows *from* \[rad\], 0 = north, clockwise — the
    /// meteorological convention, so a "westerly" is π/2 × 3.
    pub bearing: f32,
    /// Gustiness, 0 = steady … 1 = squally.
    pub gust: f32,
    /// Meteorological visibility \[m\].
    pub visibility: f32,
    /// Depth of the ground fog layer \[m\]; 0 = no layer, only haze.
    pub fog_depth: f32,
    /// Air temperature \[°C\]. Decides whether water freezes and snow melts.
    pub temperature: f32,
    /// Lightning strikes per minute within earshot; 0 = no thunderstorm.
    pub thunder: f32,
}

impl Default for Weather {
    fn default() -> Self {
        Preset::Clear.weather()
    }
}

impl Weather {
    /// The weather `t` of the way from `a` to `b`, `t` in 0 … 1.
    ///
    /// Everything continuous is interpolated; what is falling cannot be half a
    /// snowflake, so the kind switches at the halfway point and the rate fades
    /// through zero around it — rain stops, then snow starts, which is what a
    /// change of air mass does anyway.
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let mix = |x: f32, y: f32| x + (y - x) * t;
        let (precip, rate) = if a.precip == b.precip {
            (a.precip, mix(a.rate, b.rate))
        } else if t < 0.5 {
            (a.precip, a.rate * (1.0 - 2.0 * t))
        } else {
            (b.precip, b.rate * (2.0 * t - 1.0))
        };
        Self {
            cover: mix(a.cover, b.cover),
            base: mix(a.base, b.base),
            precip,
            rate,
            wind: mix(a.wind, b.wind),
            // Shortest way round the compass, so a north-easterly backing to
            // north-westerly does not sweep through south.
            bearing: a.bearing + shortest_angle(a.bearing, b.bearing) * t,
            gust: mix(a.gust, b.gust),
            visibility: mix(a.visibility, b.visibility),
            fog_depth: mix(a.fog_depth, b.fog_depth),
            temperature: mix(a.temperature, b.temperature),
            thunder: mix(a.thunder, b.thunder),
        }
    }
}

/// Signed difference `b - a` of two bearings, wrapped to ±π.
fn shortest_angle(a: f32, b: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let d = (b - a).rem_euclid(tau);
    if d > tau / 2.0 { d - tau } else { d }
}

/// The named weathers a scenario picks from
/// ([`SetWeather`](crate::scenario::Action::SetWeather)).
///
/// A preset is only a set of numbers with a name on it — everything downstream
/// reads the [`Weather`] it produces, never the name, so a line may set its own
/// values and a mod may add a weather this list has never heard of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Preset {
    #[default]
    Clear,
    Cloudy,
    Overcast,
    Fog,
    Drizzle,
    Rain,
    /// Heavy rain and a strong wind — what a warm front's cold sector brings.
    Storm,
    Thunderstorm,
    Sleet,
    Snow,
    Blizzard,
    Hail,
    /// Clear, still and below freezing: nothing falls, but the rail is glazed.
    Frost,
}

impl Preset {
    /// Every named weather, in the order a picker should offer them: from a clear
    /// day through what falls out of a warm front to what freezes on the rail.
    pub const ALL: [Preset; 13] = [
        Preset::Clear,
        Preset::Cloudy,
        Preset::Overcast,
        Preset::Fog,
        Preset::Drizzle,
        Preset::Rain,
        Preset::Storm,
        Preset::Thunderstorm,
        Preset::Sleet,
        Preset::Snow,
        Preset::Blizzard,
        Preset::Hail,
        Preset::Frost,
    ];

    /// The preset a weather came out of, if it is still exactly one of them.
    pub fn of(weather: Weather) -> Option<Preset> {
        Preset::ALL.into_iter().find(|p| p.weather() == weather)
    }

    /// The numbers behind the name. Central European lowland values — a summer
    /// noon at 18 °C, a snowfall at −3 °C.
    pub fn weather(self) -> Weather {
        // Everything not named is the clear-day value.
        let clear = Weather {
            cover: 0.05,
            base: 2_000.0,
            precip: Precip::None,
            rate: 0.0,
            wind: 2.0,
            bearing: std::f32::consts::FRAC_PI_2 * 3.0,
            gust: 0.1,
            visibility: 40_000.0,
            fog_depth: 0.0,
            temperature: 18.0,
            thunder: 0.0,
        };
        match self {
            Preset::Clear => clear,
            Preset::Cloudy => Weather {
                cover: 0.45,
                base: 1_800.0,
                visibility: 30_000.0,
                ..clear
            },
            Preset::Overcast => Weather {
                cover: 0.95,
                base: 900.0,
                wind: 4.0,
                visibility: 20_000.0,
                temperature: 12.0,
                ..clear
            },
            Preset::Fog => Weather {
                cover: 0.6,
                base: 300.0,
                wind: 0.5,
                gust: 0.0,
                visibility: 300.0,
                fog_depth: 60.0,
                temperature: 6.0,
                ..clear
            },
            Preset::Drizzle => Weather {
                cover: 0.9,
                base: 700.0,
                precip: Precip::Rain,
                rate: 0.5,
                wind: 3.0,
                visibility: 8_000.0,
                temperature: 10.0,
                ..clear
            },
            Preset::Rain => Weather {
                cover: 0.85,
                base: 800.0,
                precip: Precip::Rain,
                rate: 4.0,
                wind: 5.0,
                gust: 0.3,
                visibility: 4_000.0,
                temperature: 11.0,
                ..clear
            },
            Preset::Storm => Weather {
                cover: 1.0,
                base: 500.0,
                precip: Precip::Rain,
                rate: 12.0,
                wind: 18.0,
                gust: 0.8,
                visibility: 2_000.0,
                temperature: 13.0,
                ..clear
            },
            Preset::Thunderstorm => Weather {
                cover: 1.0,
                // A cumulonimbus stands on a low base and reaches to the tropopause.
                base: 600.0,
                precip: Precip::Rain,
                rate: 20.0,
                wind: 14.0,
                gust: 0.9,
                visibility: 1_500.0,
                temperature: 19.0,
                thunder: 2.0,
                ..clear
            },
            Preset::Sleet => Weather {
                cover: 1.0,
                base: 600.0,
                precip: Precip::Sleet,
                rate: 3.0,
                wind: 6.0,
                gust: 0.4,
                visibility: 2_500.0,
                temperature: 1.0,
                ..clear
            },
            Preset::Snow => Weather {
                cover: 0.95,
                base: 700.0,
                precip: Precip::Snow,
                rate: 2.0,
                wind: 3.0,
                visibility: 1_500.0,
                temperature: -3.0,
                ..clear
            },
            Preset::Blizzard => Weather {
                cover: 1.0,
                base: 400.0,
                precip: Precip::Snow,
                rate: 6.0,
                wind: 20.0,
                gust: 1.0,
                visibility: 300.0,
                temperature: -8.0,
                ..clear
            },
            Preset::Hail => Weather {
                cover: 1.0,
                base: 700.0,
                precip: Precip::Hail,
                rate: 15.0,
                wind: 10.0,
                gust: 0.7,
                visibility: 2_000.0,
                temperature: 16.0,
                thunder: 1.0,
                ..clear
            },
            Preset::Frost => Weather {
                cover: 0.1,
                wind: 1.0,
                gust: 0.0,
                visibility: 25_000.0,
                temperature: -5.0,
                ..clear
            },
        }
    }
}

/// How a run's weather is decided (plan 14.1).
///
/// A scenario brings its own sky and does not ask. A timetable run does ask: it is the
/// same line at the same hour every time it is driven, so either the day makes its own
/// weather or the player names one and gets exactly that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum WeatherChoice {
    /// The sky makes itself out of the run's seed and the clock — fronts move through,
    /// and no two days on the same service look alike.
    #[default]
    Dynamic,
    /// One named weather, placed at the start of the run and left where it is.
    Fixed(Preset),
}

/// Step of the slow octave \[s\] — the day's own trend, the front that takes half a day
/// to cross the country.
const SLOW: f64 = 5.0 * 3_600.0;
/// … and of the fast one: the shower that crosses within the hour.
const FAST: f64 = 3_600.0;

/// Weather that makes itself out of the clock (plan 14.1).
///
/// Two octaves of value noise give a *severity* between 0 and 1, and that severity is
/// read off a ladder of presets: the sky walks from clear through cloudy and overcast
/// into what falls out of it, and back down again. Which ladder it walks is the month's
/// decision — the front that rains in June snows in January.
///
/// It is a pure function of `(seed, clock)`: no state to carry, nothing to replicate, and
/// two peers that agree on the day agree on the sky forever without a message
/// (CLAUDE.md, ch. 20). That is also why the run's seed has to come out of the *content*
/// (the day plan and the date) rather than out of a clock reading at start-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dynamic {
    /// What makes this day's weather this day's.
    pub seed: u64,
    /// Month the run plays in, 1–12.
    pub month: u32,
}

impl Dynamic {
    /// The ladder this month walks: fair weather at the foot, the worst of it at the top.
    /// Clear and cloudy stand twice down there because most days are one of the two, and
    /// a ladder is walked at an even pace.
    pub fn ladder(self) -> [Preset; 8] {
        // November to March a front arrives as sleet and snow, and a cold night leaves
        // fog in the valley; the rest of the year it arrives as rain, with the summer
        // heat putting a thunderstorm at the top.
        if self.month >= 11 || self.month <= 3 {
            [
                Preset::Clear,
                Preset::Cloudy,
                Preset::Cloudy,
                Preset::Overcast,
                Preset::Fog,
                Preset::Drizzle,
                Preset::Sleet,
                Preset::Snow,
            ]
        } else {
            [
                Preset::Clear,
                Preset::Clear,
                Preset::Cloudy,
                Preset::Overcast,
                Preset::Drizzle,
                Preset::Rain,
                Preset::Storm,
                Preset::Thunderstorm,
            ]
        }
    }

    /// How bad it is at `clock`, 0 = the foot of the ladder … 1 = its top.
    pub fn severity(self, clock: f64) -> f32 {
        let mix =
            0.62 * noise(self.seed, 1, clock / SLOW) + 0.38 * noise(self.seed, 2, clock / FAST);
        // Most days are fair. Without the curve the sky would sit in the middle of the
        // ladder for good — permanently overcast, which is not a climate but an average.
        mix.clamp(0.0, 1.0).powf(1.9)
    }

    /// The weather at `clock` \[s since local midnight of the run's first day\].
    pub fn at(self, clock: f64) -> Weather {
        self.rung(self.severity(clock))
    }

    /// The weather at a severity of 0 … 1 — the ladder read at that height.
    ///
    /// Between two rungs the two presets are simply interpolated, so the day has no steps
    /// in it: the cover thickens, the base comes down, and the rain starts when the rung
    /// that has rain in it is halfway reached.
    pub fn rung(self, severity: f32) -> Weather {
        let ladder = self.ladder();
        let top = (ladder.len() - 1) as f32;
        let height = (severity * top).clamp(0.0, top);
        let i = (height.floor() as usize).min(ladder.len() - 2);
        Weather::lerp(
            ladder[i].weather(),
            ladder[i + 1].weather(),
            height - i as f32,
        )
    }
}

/// One octave of value noise: smooth between two hashed lattice points, in 0 … 1.
fn noise(seed: u64, salt: u64, x: f64) -> f32 {
    let cell = x.floor();
    let f = (x - cell) as f32;
    let cell = cell as i64;
    let (a, b) = (hash01(seed, salt, cell), hash01(seed, salt, cell + 1));
    // Smoothstep, so there is no corner in the sky where one hour hands over to the next.
    a + (b - a) * f * f * (3.0 - 2.0 * f)
}

/// A number in 0 … 1 out of a seed, a salt and a lattice index.
fn hash01(seed: u64, salt: u64, index: i64) -> f32 {
    let mut h = seed
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add((index as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F))
        .wrapping_add(salt.wrapping_mul(0x1656_67B1_9E37_79F9));
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 29;
    (h >> 40) as f32 / (1 << 24) as f32
}

/// How long a change of weather takes \[s\]. Weather moves in over a front, it does
/// not switch; five minutes is about what a shower needs to cross a valley.
pub const TRANSITION: f64 = 300.0;

/// Rain that has fallen for this many seconds at 4 mm/h leaves a fully wet surface.
const WET_TIME: f32 = 180.0;

/// A wet surface takes this long to dry out again \[s\] — half an hour, which is
/// the order a ballast bed and a roof need on a mild day.
const DRY_TIME: f32 = 1_800.0;

/// Snow at 2 mm/h needs this long \[s\] for a closed cover.
const SNOW_TIME: f32 = 1_200.0;

/// Seconds per °C above freezing to melt a full cover.
const MELT_TIME: f32 = 900.0;

/// How long a flash lights the sky \[s\].
const FLASH: f64 = 0.35;

/// Speed of sound \[m/s\] at 15 °C — what puts the thunder after the flash.
pub const SOUND_SPEED: f64 = 343.0;

/// A lightning strike: when it lit, how far away it stands and in which
/// direction. Everything about it is a function of the scenario clock, so the
/// same flash lights the same field on every machine (plan 14.1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Strike {
    /// Simulation time the channel lit \[s\].
    pub at: f64,
    /// Distance \[m\]. The thunder follows `distance / SOUND_SPEED` later.
    pub distance: f32,
    /// Direction it stands in \[rad\], 0 = north, clockwise.
    pub bearing: f32,
}

impl Strike {
    /// How brightly this strike is lighting the sky at `time`, 1 … 0.
    ///
    /// A channel flickers rather than fading: the return strokes follow each
    /// other in tens of milliseconds, which is what the eye remembers.
    pub fn brightness(&self, time: f64) -> f32 {
        let age = time - self.at;
        if !(0.0..FLASH).contains(&age) {
            return 0.0;
        }
        let decay = 1.0 - (age / FLASH) as f32;
        let flicker = 0.65 + 0.35 * ((age * 47.0) as f32).sin().abs();
        decay * flicker
    }

    /// Whether the thunder of this strike reaches the observer at `time`, within
    /// one step of `dt` — a level for the sound table, 1 the moment it arrives.
    pub fn thunder(&self, time: f64, dt: f64) -> f32 {
        let arrives = self.at + f64::from(self.distance) / SOUND_SPEED;
        // A near strike cracks, a distant one rolls for seconds.
        let roll = 0.5 + f64::from(self.distance) / 2_000.0;
        let age = time - arrives;
        if age < 0.0 || age > roll + dt {
            return 0.0;
        }
        (1.0 - (age / roll) as f32).clamp(0.0, 1.0)
    }
}

/// The weather over time: where it came from, where it is going, and what it has
/// left on the ground.
///
/// [`Sim::weather`](crate::Sim::weather) holds one of these. Everything that draws
/// or drives off the weather reads [`now`](Self::now); the accumulations
/// [`wetness`](Self::wetness) and [`snow`](Self::snow) are the slow part that the
/// rail condition and the material shaders hang off.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Timeline {
    /// The weather of this moment — interpolated in [`step`](Self::step).
    pub now: Weather,
    from: Weather,
    to: Weather,
    /// Simulation time the running change started at \[s\].
    t0: f64,
    span: f64,
    /// Surface water, 0 = bone dry … 1 = running wet.
    pub wetness: f32,
    /// Lying snow, 0 = bare … 1 = closed cover.
    pub snow: f32,
    /// Rail condition set by hand ([`SetRail`](crate::scenario::Action::SetRail)) —
    /// leaves, sanded rail, a scenario that wants one specific problem. It survives
    /// until the next change of weather.
    pub rail_override: Option<RailCondition>,
    /// The sky making itself out of the clock — what a timetable run set to
    /// [`WeatherChoice::Dynamic`] is driven under. `None` = the weather only moves where
    /// something moves it, which is what a scenario wants.
    #[serde(default)]
    pub dynamic: Option<Dynamic>,
}

impl Default for Timeline {
    fn default() -> Self {
        let clear = Weather::default();
        Self {
            now: clear,
            from: clear,
            to: clear,
            t0: 0.0,
            span: TRANSITION,
            wetness: 0.0,
            snow: 0.0,
            rail_override: None,
            dynamic: None,
        }
    }
}

impl Timeline {
    /// Starts a change to `to` at simulation time `time`, taking [`TRANSITION`].
    /// A change of weather clears a hand-set rail condition — the sky has taken
    /// the question over again.
    ///
    /// It also switches a running [`Dynamic`] off: a scenario that says it starts to rain
    /// means it, and a generator that carried on underneath would wash the front away
    /// again a few minutes later.
    pub fn set(&mut self, to: Weather, time: f64) {
        self.from = self.now;
        self.to = to;
        self.t0 = time;
        self.span = TRANSITION;
        self.rail_override = None;
        self.dynamic = None;
    }

    /// Places `weather` immediately, with no transition — what the start of a run
    /// and the editor's preview do.
    pub fn place(&mut self, weather: Weather, time: f64) {
        self.now = weather;
        self.from = weather;
        self.to = weather;
        self.t0 = time;
        // The ground starts in the state the weather implies, so a scenario that
        // starts in the rain does not start on a dry rail.
        self.wetness = if weather.precip.is_liquid() { 1.0 } else { 0.0 };
        self.snow = f32::from(u8::from(weather.precip == Precip::Snow));
        self.rail_override = None;
        self.dynamic = None;
    }

    /// Hands the sky to a generator: the weather at `clock` is placed, and from there on
    /// the day makes its own (see [`Dynamic`]).
    ///
    /// `clock` is seconds since local midnight of the run's first day — the same reading
    /// [`Sim::clock`](crate::Sim::clock) gives, so a run that starts at eight in the
    /// morning starts in that morning's weather rather than in the day's first hour.
    pub fn generate(&mut self, dynamic: Dynamic, clock: f64) {
        self.place(dynamic.at(clock), 0.0);
        self.dynamic = Some(dynamic);
    }

    /// The keyframed weather at simulation time `time` — where the run came from and
    /// where it is going.
    ///
    /// A [`Dynamic`] day does not come through here: it hangs off the wall clock rather
    /// than the run's own time, and [`step`](Self::step) reads it straight off
    /// [`Dynamic::at`].
    pub fn at(&self, time: f64) -> Weather {
        let t = if self.span > 0.0 {
            ((time - self.t0) / self.span).clamp(0.0, 1.0) as f32
        } else {
            1.0
        };
        Weather::lerp(self.from, self.to, t)
    }

    /// One simulation step: interpolates the sky and integrates what falls out of it.
    ///
    /// `time` is seconds since the start of the run and `clock` seconds since local
    /// midnight — the keyframes hang off the first, a [`Dynamic`] day off the second,
    /// because a front at three in the afternoon has to be there whichever run drove into
    /// it.
    pub fn step(&mut self, time: f64, clock: f64, dt: f64) {
        self.now = match self.dynamic {
            Some(dynamic) => dynamic.at(clock),
            None => self.at(time),
        };
        let dt = dt as f32;
        let w = self.now;
        let falling = if w.precip == Precip::None {
            0.0
        } else {
            w.rate
        };

        // Liquid water wets, snow only wets once it is warm enough to melt.
        let rain = if w.precip.is_liquid() { falling } else { 0.0 };
        if rain > 0.0 {
            self.wetness += (rain / 4.0).min(2.0) * dt / WET_TIME;
        } else {
            self.wetness -= dt / DRY_TIME;
        }

        if w.precip == Precip::Snow && w.temperature <= 1.0 {
            self.snow += (falling / 2.0).min(3.0) * dt / SNOW_TIME;
        }
        if w.temperature > 0.0 && self.snow > 0.0 {
            let melted = (w.temperature * dt / MELT_TIME).min(self.snow);
            self.snow -= melted;
            // Meltwater runs off the same surfaces the rain wets.
            self.wetness += melted;
        }

        self.wetness = self.wetness.clamp(0.0, 1.0);
        self.snow = self.snow.clamp(0.0, 1.0);
    }

    /// The strike burning at `time`, if there is one.
    ///
    /// Strikes are not rolled and stored, they are *read off the clock*: the
    /// thunderstorm's rate divides the run into windows, and one hash per window
    /// says where in it the channel lit and where it stands. No state, no seed to
    /// replicate, and a client that joins in the middle of the storm sees the
    /// same sky as everyone else.
    pub fn lightning(&self, time: f64) -> Option<Strike> {
        let rate = self.now.thunder;
        if rate <= 0.0 || time < 0.0 {
            return None;
        }
        let window = 60.0 / f64::from(rate);
        // The window this time falls in, and the one before it — a strike near
        // the start of a window is still burning from the previous one.
        (0..2).find_map(|back| {
            let index = (time / window).floor() as i64 - back;
            if index < 0 {
                return None;
            }
            let hash = |salt: u64| {
                let mut h = (index as u64)
                    .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                    .wrapping_add(salt.wrapping_mul(0xC2B2_AE3D_27D4_EB4F));
                h ^= h >> 33;
                h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
                h ^= h >> 29;
                (h >> 40) as f32 / (1 << 24) as f32
            };
            let strike = Strike {
                at: (index as f64 + f64::from(hash(1))) * window,
                // Most of a storm's strikes are the far ones — the near ones are
                // rare, which is exactly why they are worth something.
                distance: 300.0 + 11_000.0 * hash(2) * hash(2),
                bearing: hash(3) * std::f32::consts::TAU,
            };
            (strike.brightness(time) > 0.0 || strike.thunder(time, 1.0) > 0.0).then_some(strike)
        })
    }

    /// What the weather has left on the rail head.
    ///
    /// The order matters: snow and ice beat water, and the *first* rain on a rail
    /// that has been dry for hours is worse than the downpour after it — it lifts
    /// the oil and brake dust into a film instead of washing it off.
    pub fn rail(&self) -> RailCondition {
        if let Some(rail) = self.rail_override {
            return rail;
        }
        let w = self.now;
        // Snow and sleet on the head, ice on a wet rail below freezing, and the
        // first rain on a rail that has been dry for hours: three different films,
        // one effect.
        let greasy = self.snow > 0.02
            || w.precip == Precip::Sleet
            || (w.temperature <= 0.0 && self.wetness > 0.05)
            || (w.precip.is_liquid() && self.wetness < 0.3);
        if greasy {
            RailCondition::Slippery
        } else if self.wetness > 0.05 {
            RailCondition::Wet
        } else {
            RailCondition::Dry
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_of_weather_arrives_over_the_transition() {
        let mut line = Timeline::default();
        line.set(Preset::Rain.weather(), 0.0);
        assert_eq!(line.at(0.0).cover, Preset::Clear.weather().cover);
        assert_eq!(line.at(TRANSITION).cover, Preset::Rain.weather().cover);
        let half = line.at(TRANSITION / 2.0);
        assert!(
            half.cover > 0.05 && half.cover < 0.85,
            "cover {}",
            half.cover
        );
        // Beyond the transition it stays put.
        assert_eq!(line.at(TRANSITION * 10.0), line.at(TRANSITION));
    }

    #[test]
    fn rain_gives_way_to_snow_through_a_dry_moment() {
        let mut line = Timeline::default();
        line.place(Preset::Rain.weather(), 0.0);
        line.set(Preset::Snow.weather(), 0.0);
        assert_eq!(line.at(TRANSITION * 0.25).precip, Precip::Rain);
        assert_eq!(line.at(TRANSITION * 0.75).precip, Precip::Snow);
        // The changeover itself has next to nothing falling.
        assert!(line.at(TRANSITION * 0.49).rate < 0.2);
    }

    #[test]
    fn the_bearing_takes_the_short_way_round() {
        let a = Weather {
            bearing: 0.2,
            ..Preset::Clear.weather()
        };
        let b = Weather {
            bearing: std::f32::consts::TAU - 0.2,
            ..Preset::Clear.weather()
        };
        // Backing from north-by-east to north-by-west passes north, not south.
        let mid = Weather::lerp(a, b, 0.5).bearing;
        assert!(mid.abs() < 0.05 || (mid - std::f32::consts::TAU).abs() < 0.05);
    }

    #[test]
    fn rain_wets_and_the_first_of_it_is_the_worst() {
        let mut line = Timeline::default();
        line.set(Preset::Rain.weather(), 0.0);
        let step_to = |line: &mut Timeline, end: f64, from: f64| {
            let mut t = from;
            while t < end {
                line.step(t, t, 0.05);
                t += 0.05;
            }
        };
        // The first drops on a rail that has been dry for hours leave a film.
        step_to(&mut line, TRANSITION * 0.6, 0.0);
        assert!(line.wetness < 0.3, "wetness {}", line.wetness);
        assert_eq!(line.rail(), RailCondition::Slippery);
        // Half an hour later it is washed off and merely wet.
        step_to(&mut line, TRANSITION + 1_800.0, TRANSITION * 0.6);
        assert!(line.wetness > 0.9, "wetness {}", line.wetness);
        assert_eq!(line.rail(), RailCondition::Wet);
    }

    #[test]
    fn snow_lies_while_it_freezes_and_melts_when_it_thaws() {
        let mut line = Timeline::default();
        line.place(Preset::Snow.weather(), 0.0);
        let mut t = 0.0;
        while t < 1_800.0 {
            line.step(t, t, 0.1);
            t += 0.1;
        }
        assert!(line.snow > 0.5, "snow {}", line.snow);
        assert_eq!(line.rail(), RailCondition::Slippery);

        line.set(Preset::Cloudy.weather(), t);
        let end = t + 8.0 * 3_600.0;
        while t < end {
            line.step(t, t, 0.5);
            t += 0.5;
        }
        assert_eq!(line.snow, 0.0, "a mild day clears it");
    }

    #[test]
    fn a_thunderstorm_strikes_and_the_thunder_follows() {
        let mut line = Timeline::default();
        line.place(Preset::Thunderstorm.weather(), 0.0);
        let mut strikes = Vec::new();
        let mut thunder = 0.0f32;
        let mut t = 0.0;
        while t < 600.0 {
            line.step(t, t, 0.05);
            if let Some(strike) = line.lightning(t) {
                if strike.brightness(t) > 0.0 && !strikes.contains(&strike.at) {
                    strikes.push(strike.at);
                    // The thunder cannot have arrived before the flash.
                    assert!(strike.thunder(t, 0.05) == 0.0, "thunder before light");
                }
                thunder = thunder.max(strike.thunder(t, 0.05));
            }
            t += 0.05;
        }
        // Two a minute over ten minutes, give or take the window boundaries.
        assert!(
            (15..=25).contains(&strikes.len()),
            "strikes: {}",
            strikes.len()
        );
        assert!(thunder > 0.0, "and every one of them is heard");
    }

    #[test]
    fn lightning_is_the_same_on_every_machine() {
        let mut line = Timeline::default();
        line.place(Preset::Thunderstorm.weather(), 0.0);
        // Same clock, same strike — no state, no seed, nothing to replicate.
        for t in [12.3, 45.6, 300.0] {
            assert_eq!(line.lightning(t), line.lightning(t));
        }
        // And a clear sky has none.
        let mut clear = Timeline::default();
        clear.place(Preset::Clear.weather(), 0.0);
        assert_eq!(clear.lightning(100.0), None);
    }

    #[test]
    fn a_generated_day_is_the_same_day_on_every_machine() {
        let dynamic = Dynamic {
            seed: 0x5eed_1234,
            month: 7,
        };
        // No state, no seed to replicate: the clock alone says what the sky does.
        for clock in [0.0, 12_345.0, 43_200.0, 86_399.0] {
            assert_eq!(dynamic.at(clock), dynamic.at(clock));
        }
        // And it does not stand still — a day has weather in it, not one weather.
        let over_the_day: Vec<f32> = (0..24)
            .map(|h| dynamic.severity(f64::from(h) * 3_600.0))
            .collect();
        let low = over_the_day.iter().copied().fold(f32::MAX, f32::min);
        let high = over_the_day.iter().copied().fold(0.0f32, f32::max);
        assert!(high - low > 0.1, "flat day: {low} … {high}");
    }

    #[test]
    fn a_generated_day_has_no_steps_in_it() {
        let dynamic = Dynamic { seed: 7, month: 10 };
        let mut previous = dynamic.at(0.0);
        let mut clock = 0.0;
        while clock < 86_400.0 {
            clock += 10.0;
            let now = dynamic.at(clock);
            assert!(
                (now.cover - previous.cover).abs() < 0.05,
                "the sky jumped at {clock} s: {} to {}",
                previous.cover,
                now.cover
            );
            previous = now;
        }
    }

    #[test]
    fn the_same_front_rains_in_june_and_snows_in_january() {
        let summer = Dynamic { seed: 42, month: 6 };
        let winter = Dynamic { seed: 42, month: 1 };
        // The foot of both ladders is the same fair day …
        assert_eq!(summer.rung(0.0).precip, Precip::None);
        assert_eq!(winter.rung(0.0).precip, Precip::None);
        // … and the top of them is the same front in two air masses.
        assert_eq!(summer.rung(1.0).precip, Precip::Rain);
        assert_eq!(winter.rung(1.0).precip, Precip::Snow);
        assert!(winter.rung(1.0).temperature < 0.0);
        assert!(summer.rung(1.0).temperature > 10.0);
    }

    #[test]
    fn a_generated_day_starts_the_run_in_the_weather_of_its_hour() {
        let dynamic = Dynamic { seed: 99, month: 5 };
        let mut line = Timeline::default();
        // Eight in the morning, not the first hour of the plan.
        let clock = 8.0 * 3_600.0;
        line.generate(dynamic, clock);
        assert_eq!(line.now, dynamic.at(clock));
        // And it carries on off the clock rather than off the run's own time.
        line.step(30.0, clock + 30.0, 0.05);
        assert_eq!(line.now, dynamic.at(clock + 30.0));
    }

    #[test]
    fn a_scenario_takes_the_sky_over_from_the_generator() {
        let mut line = Timeline::default();
        line.generate(Dynamic { seed: 3, month: 6 }, 8.0 * 3_600.0);
        assert!(line.dynamic.is_some());
        line.set(Preset::Thunderstorm.weather(), 0.0);
        assert!(line.dynamic.is_none(), "the action owns the sky now");
        line.step(TRANSITION, 8.0 * 3_600.0 + TRANSITION, 0.05);
        assert_eq!(line.now.thunder, Preset::Thunderstorm.weather().thunder);
    }

    #[test]
    fn a_hand_set_rail_survives_until_the_weather_changes() {
        let mut line = Timeline {
            rail_override: Some(RailCondition::Slippery),
            ..Default::default()
        };
        assert_eq!(line.rail(), RailCondition::Slippery);
        line.set(Preset::Clear.weather(), 0.0);
        assert_eq!(line.rail(), RailCondition::Dry);
    }
}
