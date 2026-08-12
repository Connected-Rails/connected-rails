//! Szenario- und Ereignissystem (Plan Kap. 11.4).
//!
//! Ein Szenario ist eine RON-Datei aus Ereignissen: jedes hat einen Auslöser (Zeit,
//! Zugposition, Zustand) und Aktionen (Weiche/Signal stellen, Ansage, Wertung, Ende).
//! Ausgewertet wird in jedem Simulationsschritt, nach Physik und Stellwerk.

use crate::Sim;
use crate::interlock::{RouteId, SignalId};
use crate::train::RailCondition;
use serde::{Deserialize, Serialize};
use track_model::{EdgeId, NodeId, SwitchPosition};

/// Auslöser eines Ereignisses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Trigger {
    /// Ab dieser Simulationszeit [s].
    Time(f64),
    /// Zugspitze hat die Stelle `(edge, s)` erreicht oder überfahren.
    TrainPast { train: usize, edge: EdgeId, s: f64 },
    /// Zug steht innerhalb `radius` um `(edge, s)`.
    TrainStopped {
        train: usize,
        edge: EdgeId,
        s: f64,
        radius: f64,
    },
    /// Geschwindigkeit über dem Schwellwert [km/h].
    SpeedAbove { train: usize, kmh: f64 },
    /// Geschwindigkeit unter dem Schwellwert [km/h].
    SpeedBelow { train: usize, kmh: f64 },
    /// Signal zeigt Halt (`stop = true`) bzw. Fahrt (`stop = false`).
    SignalStop { signal: SignalId, stop: bool },
    /// Die Zugsicherung hat eingegriffen.
    ForcedBrake { train: usize },
    /// `delay` Sekunden nachdem das Ereignis `event` ausgelöst hat.
    After { event: String, delay: f64 },
    /// Alle Teilbedingungen erfüllt.
    All(Vec<Trigger>),
    /// Mindestens eine Teilbedingung erfüllt.
    Any(Vec<Trigger>),
}

/// Was ein Ereignis auslöst.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Action {
    /// Meldung an den Spieler (Fahrplanhinweis, Fdl-Anweisung).
    Message(String),
    /// Ansage über Zugfunk/Lautsprecher (v1: Text, Audio folgt mit Kap. 13).
    Announcement(String),
    SetSwitch {
        node: NodeId,
        position: SwitchPosition,
    },
    RequestRoute(RouteId),
    ReleaseRoute(RouteId),
    /// Wetterwechsel — wirkt über den Kraftschluss auf die Fahrdynamik.
    SetRail(RailCondition),
    /// Punkte gutschreiben oder abziehen.
    Score {
        points: i32,
        reason: String,
    },
    /// Szenario beenden.
    Finish {
        success: bool,
        reason: String,
    },
}

/// Ein Ereignis des Szenarios.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// Eindeutiger Name — Bezug für [`Trigger::After`].
    pub name: String,
    pub trigger: Trigger,
    pub actions: Vec<Action>,
    /// Nur einmal auslösen (Regelfall).
    #[serde(default = "yes")]
    pub once: bool,
}

fn yes() -> bool {
    true
}

/// Ein vollständiges Szenario.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Scenario {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Zug, den der Spieler fährt.
    #[serde(default)]
    pub player_train: usize,
    #[serde(default)]
    pub events: Vec<Event>,
}

impl Scenario {
    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }

    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).expect("serialisierbar")
    }
}

/// Eine Meldung an den Spieler.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub time: f64,
    pub text: String,
    /// Ansage statt Textmeldung.
    pub announcement: bool,
}

/// Ausgang eines Szenarios.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    pub success: bool,
    pub reason: String,
    pub time: f64,
}

/// Laufzeitzustand des Szenarios.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScenarioRuntime {
    pub scenario: Scenario,
    /// Auslösezeit je Ereignis (`None` = noch nicht ausgelöst).
    fired_at: Vec<Option<f64>>,
    pub messages: Vec<Message>,
    pub outcome: Option<Outcome>,
    /// Punkte aus [`Action::Score`].
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

    /// Zuletzt ausgegebene Meldungen (für HUD).
    pub fn recent_messages(&self, count: usize) -> &[Message] {
        let start = self.messages.len().saturating_sub(count);
        &self.messages[start..]
    }

    /// Hat das Ereignis `name` bereits ausgelöst?
    pub fn fired_at(&self, name: &str) -> Option<f64> {
        let index = self.scenario.events.iter().position(|e| e.name == name)?;
        self.fired_at.get(index).copied().flatten()
    }
}

/// Ein Auswertungsschritt: prüft alle Auslöser und führt fällige Aktionen aus.
///
/// Steht außerhalb von [`ScenarioRuntime`], weil die Aktionen auf die ganze Simulation
/// wirken (Weichen, Fahrstraßen, Wetter).
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

/// Prüft einen Auslöser gegen den aktuellen Simulationszustand.
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
    }
}

/// Führt eine Aktion aus.
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
        Action::SetRail(condition) => {
            for train in &mut sim.trains {
                train.rail = *condition;
            }
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
