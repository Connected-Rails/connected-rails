//! Scenario and event system (plan ch. 11.4).
//!
//! A scenario is a RON file of events: each has a trigger (time, train position, state)
//! and actions (set switch/signal, announcement, scoring, end).
//! It is evaluated in every simulation step, after physics and interlocking.

use crate::Sim;
use crate::interlock::{RouteId, SignalId};
use crate::train::RailCondition;
use crate::weather::Preset;
use serde::{Deserialize, Serialize};
use track_model::{EdgeId, NodeId, SwitchPosition};

/// Trigger of an event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Trigger {
    /// From this simulation time onwards [s].
    Time(f64),
    /// The head of the train has reached or passed the point `(edge, s)`.
    TrainPast { train: usize, edge: EdgeId, s: f64 },
    /// The train is standing within `radius` of `(edge, s)`.
    TrainStopped {
        train: usize,
        edge: EdgeId,
        s: f64,
        radius: f64,
    },
    /// Speed above the threshold [km/h].
    SpeedAbove { train: usize, kmh: f64 },
    /// Speed below the threshold [km/h].
    SpeedBelow { train: usize, kmh: f64 },
    /// The signal shows stop (`stop = true`) or proceed (`stop = false`).
    SignalStop { signal: SignalId, stop: bool },
    /// The train protection has intervened.
    ForcedBrake { train: usize },
    /// `delay` seconds after the event `event` has fired.
    After { event: String, delay: f64 },
    /// All sub-conditions fulfilled.
    All(Vec<Trigger>),
    /// At least one sub-condition fulfilled.
    Any(Vec<Trigger>),
    /// Never on its own — the event waits for a script to [`fire`] it (plan 19.7).
    Never,
}

/// What an event triggers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Action {
    /// Message to the player (timetable note, dispatcher instruction).
    Message(String),
    /// Announcement over train radio/loudspeaker (v1: text, audio follows with ch. 13).
    Announcement(String),
    SetSwitch {
        node: NodeId,
        position: SwitchPosition,
    },
    RequestRoute(RouteId),
    ReleaseRoute(RouteId),
    /// Change of weather — the sky moves to this over
    /// [`weather::TRANSITION`](crate::weather::TRANSITION), and the rail follows what
    /// falls out of it.
    SetWeather(Preset),
    /// Rail condition alone (leaves, sanded rail) — the sky stays as it is, and the
    /// setting holds until the next change of weather.
    SetRail(RailCondition),
    /// Award or deduct points.
    Score {
        points: i32,
        reason: String,
    },
    /// Finish the scenario.
    Finish {
        success: bool,
        reason: String,
    },
}

/// An event of the scenario.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// Unique name — reference for [`Trigger::After`].
    pub name: String,
    pub trigger: Trigger,
    pub actions: Vec<Action>,
    /// Fire only once (the normal case).
    #[serde(default = "yes")]
    pub once: bool,
    /// Module whose local indices this event's trigger and actions use — resolved
    /// against the composed line by the mod runtime, then cleared. `None` falls back
    /// to the scenario's `module`; without either, indices are those of the line.
    #[serde(default)]
    pub module: Option<String>,
}

fn yes() -> bool {
    true
}

/// Wall-clock date and time at the start of the run (`sim.time == 0`), plan ch. 14.
///
/// Drives the sun and moon and anchors `Daily` timetables; `Scenario` timetables
/// and event triggers stay relative to the start of the run.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StartTime {
    pub year: i32,
    /// 1–12.
    pub month: u32,
    /// 1–31.
    pub day: u32,
    /// 0–23, local time.
    pub hour: u32,
    pub minute: u32,
    /// Local clock ahead of UT [h] — Germany: 1 in winter, 2 in summer.
    pub utc_offset: f64,
}

impl Default for StartTime {
    /// Midsummer noon — matches the fixed lighting the simulator had before.
    fn default() -> Self {
        Self {
            year: 2026,
            month: 6,
            day: 21,
            hour: 12,
            minute: 0,
            utc_offset: 2.0,
        }
    }
}

impl StartTime {
    /// Seconds since local midnight.
    pub fn seconds(&self) -> f64 {
        f64::from(self.hour * 3600 + self.minute * 60)
    }

    /// Seconds since **UT** midnight of the start day — what astronomy wants.
    pub fn seconds_ut(&self) -> f64 {
        self.seconds() - self.utc_offset * 3600.0
    }
}

/// A complete scenario.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Scenario {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Wall-clock date and time the run begins at.
    #[serde(default)]
    pub start: StartTime,
    /// The weather the run starts in — placed, not moved to, so a scenario that
    /// begins in the rain begins on a wet rail (plan 14.1).
    #[serde(default)]
    pub weather: Preset,
    /// The train the player drives.
    #[serde(default)]
    pub player_train: usize,
    #[serde(default)]
    pub events: Vec<Event>,
    /// Optional Lua script hook (plan 19.7), named `"<mod>:<file stem>"`.
    #[serde(default)]
    pub script: Option<String>,
    /// Optional timetable for the player train, named `"<mod>:<file stem>"` —
    /// without one the scoring counts scenario points only.
    #[serde(default)]
    pub timetable: Option<String>,
    /// Optional line the scenario runs on, named `"<mod>:<file stem>"` — a plain line
    /// or a composition of modules. `--line` on the command line wins.
    #[serde(default)]
    pub line: Option<String>,
    /// Default module for the events' indices — see [`Event::module`].
    #[serde(default)]
    pub module: Option<String>,
}

impl Scenario {
    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }

    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).expect("serializable")
    }
}

/// A message to the player.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub time: f64,
    pub text: String,
    /// Announcement instead of a text message.
    pub announcement: bool,
}

/// Outcome of a scenario.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    pub success: bool,
    pub reason: String,
    pub time: f64,
}

/// Runtime state of the scenario.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScenarioRuntime {
    pub scenario: Scenario,
    /// Firing time per event (`None` = not fired yet).
    fired_at: Vec<Option<f64>>,
    pub messages: Vec<Message>,
    pub outcome: Option<Outcome>,
    /// Points from [`Action::Score`].
    pub bonus: i32,
}

impl ScenarioRuntime {
    pub fn new(scenario: Scenario) -> Self {
        Self {
            fired_at: vec![None; scenario.events.len()],
            scenario,
            messages: Vec::new(),
            outcome: None,
            bonus: 0,
        }
    }

    pub fn is_finished(&self) -> bool {
        self.outcome.is_some()
    }

    /// The most recently issued messages (for the HUD).
    pub fn recent_messages(&self, count: usize) -> &[Message] {
        let start = self.messages.len().saturating_sub(count);
        &self.messages[start..]
    }

    /// Has the event `name` already fired?
    pub fn fired_at(&self, name: &str) -> Option<f64> {
        let index = self.scenario.events.iter().position(|e| e.name == name)?;
        self.fired_at.get(index).copied().flatten()
    }

    /// Is there an event of that name?
    pub fn has_event(&self, name: &str) -> bool {
        self.scenario.events.iter().any(|e| e.name == name)
    }

    /// Adds a message (script hooks and the dispatcher use this).
    pub fn message(&mut self, time: f64, text: String, announcement: bool) {
        self.messages.push(Message {
            time,
            text,
            announcement,
        });
    }
}

/// One evaluation step: checks all triggers and executes the due actions.
///
/// It lives outside [`ScenarioRuntime`] because the actions act on the whole simulation
/// (switches, routes, weather).
pub fn step(sim: &mut Sim) {
    if sim.scenario.is_finished() || sim.scenario.scenario.events.is_empty() {
        return;
    }
    let time = sim.time;
    let count = sim.scenario.scenario.events.len();

    for i in 0..count {
        let event = sim.scenario.scenario.events[i].clone();
        if event.once && sim.scenario.fired_at[i].is_some() {
            continue;
        }
        if !evaluate(&event.trigger, sim) {
            continue;
        }
        sim.scenario.fired_at[i] = Some(time);
        for action in &event.actions {
            apply(action, sim);
        }
        if sim.scenario.is_finished() {
            return;
        }
    }
}

/// Fires the event `name` no matter what its trigger says — this is how a scenario or line
/// script decides the moment itself (plan 19.7). The actions stay declarative; only the
/// *when* comes from the script.
///
/// `false` if there is no such event or it has already fired and is `once`.
pub fn fire(sim: &mut Sim, name: &str) -> bool {
    let Some(i) = sim
        .scenario
        .scenario
        .events
        .iter()
        .position(|e| e.name == name)
    else {
        return false;
    };
    let event = sim.scenario.scenario.events[i].clone();
    if event.once && sim.scenario.fired_at[i].is_some() {
        return false;
    }
    sim.scenario.fired_at[i] = Some(sim.time);
    for action in &event.actions {
        apply(action, sim);
    }
    true
}

/// Checks a trigger against the current simulation state.
fn evaluate(trigger: &Trigger, sim: &Sim) -> bool {
    match trigger {
        Trigger::Time(t) => sim.time >= *t,
        Trigger::TrainPast { train, edge, s } => sim
            .trains
            .get(*train)
            .map(|t| t.vehicles[0].pos)
            .is_some_and(|p| p.edge == *edge && (p.s - *s) * p.dir as f64 >= 0.0),
        Trigger::TrainStopped {
            train,
            edge,
            s,
            radius,
        } => sim.trains.get(*train).is_some_and(|t| {
            let p = t.vehicles[0].pos;
            t.speed_kmh().abs() < 0.5 && p.edge == *edge && (p.s - *s).abs() <= *radius
        }),
        Trigger::SpeedAbove { train, kmh } => sim
            .trains
            .get(*train)
            .is_some_and(|t| t.speed_kmh().abs() > *kmh),
        Trigger::SpeedBelow { train, kmh } => sim
            .trains
            .get(*train)
            .is_some_and(|t| t.speed_kmh().abs() < *kmh),
        Trigger::SignalStop { signal, stop } => sim
            .interlock
            .signals
            .get(signal.index())
            .is_some_and(|s| s.aspect.is_stop() == *stop),
        Trigger::ForcedBrake { train } => sim
            .runtime
            .get(*train)
            .is_some_and(|r| r.protection.action != crate::safety::ProtectionAction::None),
        Trigger::After { event, delay } => sim
            .scenario
            .fired_at(event)
            .is_some_and(|t| sim.time >= t + *delay),
        Trigger::All(list) => list.iter().all(|t| evaluate(t, sim)),
        Trigger::Any(list) => list.iter().any(|t| evaluate(t, sim)),
        Trigger::Never => false,
    }
}

/// Executes an action.
fn apply(action: &Action, sim: &mut Sim) {
    let time = sim.time;
    match action {
        Action::Message(text) | Action::Announcement(text) => {
            sim.scenario.messages.push(Message {
                time,
                text: text.clone(),
                announcement: matches!(action, Action::Announcement(_)),
            });
        }
        Action::SetSwitch { node, position } => {
            if let Some(sw) = sim.net.switch_mut(*node) {
                let _ = sw.command(*position);
            }
        }
        Action::RequestRoute(route) => {
            if route.index() < sim.interlock.routes.len() {
                let mut interlock = std::mem::take(&mut sim.interlock);
                interlock.request_route(*route, &mut sim.net);
                sim.interlock = interlock;
            }
        }
        Action::ReleaseRoute(route) => {
            if route.index() < sim.interlock.routes.len() {
                let mut interlock = std::mem::take(&mut sim.interlock);
                interlock.release_route(*route, &mut sim.net);
                sim.interlock = interlock;
            }
        }
        Action::SetWeather(preset) => {
            sim.weather.set(preset.weather(), time);
        }
        Action::SetRail(condition) => {
            sim.weather.rail_override = Some(*condition);
        }
        Action::Score { points, reason } => {
            sim.scenario.bonus += points;
            sim.scenario.messages.push(Message {
                time,
                text: format!("{reason} ({points:+})"),
                announcement: false,
            });
        }
        Action::Finish { success, reason } => {
            sim.scenario.outcome = Some(Outcome {
                success: *success,
                reason: reason.clone(),
                time,
            });
        }
    }
}
