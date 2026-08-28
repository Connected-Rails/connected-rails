//! Signals, routes and block protection (plan ch. 10).
//!
//! Country-neutral: a signal is a state machine with aspects; which lamp images an aspect
//! has in the Ks or H/V system is decided by the presentation in the country package.

use crate::shunt::{Movement, SHUNTING_SPEED_KMH};
use serde::{Deserialize, Serialize};
use track_model::{DeviceKind, EdgeId, NodeId, SwitchPosition, TrackNetwork, TracksideDevice};

macro_rules! id_type {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        pub struct $name(pub u32);
        impl $name {
            pub fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

id_type!(SignalId);
id_type!(RouteId);
id_type!(SectionId);

/// Signal system (determines only the presentation, not the logic).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignalSystem {
    /// Main/distant signal system (H/V).
    HV,
    /// Combination signal system (Ks).
    Ks,
    /// Hl system (eastern network) — v2.
    Hl,
}

/// Type of the signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignalKind {
    /// Main signal.
    Main,
    /// Distant signal (only announces the aspect of the associated main signal).
    Distant,
    /// Combination signal (Ks): main and distant signal function on one screen.
    Combined,
    /// Shunting signal (Sh1/Ra12).
    Shunting,
    /// Track lock (Gleissperre, Sh 2/Wn 7): laid on it derails a vehicle
    /// running onto it, laid off the track is free.
    ///
    /// It is a signal here because everything it needs is what a signal has:
    /// two states, an aspect the interlocking sets, a lamp image the mod's
    /// signal type names and a 3D model that moves with it (`motions` in the
    /// signal model swings the shoe). Stop means laid on — the state it rests
    /// in, and the one flank protection holds it in.
    TrackLock,
}

impl SignalKind {
    /// Can a route end at this signal? A distant signal announces, a track
    /// lock secures — neither is a place a train move is authorised to.
    pub fn ends_a_route(self) -> bool {
        matches!(self, Self::Main | Self::Combined | Self::Shunting)
    }

    /// Can it hold a movement off a route as flank protection? Everything
    /// that can show stop by itself — the track lock most of all, which is
    /// what it exists for.
    pub fn holds_a_flank(self) -> bool {
        !matches!(self, Self::Distant)
    }
}

/// Main signal aspect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MainAspect {
    /// Hp0 / Ks stop.
    #[default]
    Stop,
    /// Hp1 / Ks1 — proceed.
    Proceed,
    /// Hp2 / Ks2 with Zs3 — slow speed (diverging route).
    ProceedSlow,
    /// Zs1/Zs7 — substitute signal.
    Substitute,
    /// Marker light — signal invalid.
    DarkLight,
}

impl MainAspect {
    pub fn is_stop(self) -> bool {
        matches!(self, MainAspect::Stop | MainAspect::DarkLight)
    }
}

/// Distant signal aspect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DistantAspect {
    /// Vr0 / Ks2 — expect stop.
    #[default]
    ExpectStop,
    /// Vr1 / Ks1 — expect proceed.
    ExpectProceed,
    /// Vr2 — expect slow speed.
    ExpectSlow,
}

impl DistantAspect {
    /// Is the 1000 Hz magnet active? (With Vr0 and Vr2, not with Vr1.)
    pub fn is_restrictive(self) -> bool {
        !matches!(self, DistantAspect::ExpectProceed)
    }
}

/// How long a shunting route stands locked without its movement running over it before it
/// is given back \[s\] — the Zeitverschluss of a Rangierfahrstraße.
///
/// Long enough that a driver who has drawn up and is looking at the road is not taken by
/// surprise, short enough that a route set for a movement that thought better of it does
/// not hold the points for the rest of the day.
pub const SHUNT_HOLD: f64 = 300.0;

/// Shunting signal aspect (Ril 301).
///
/// What a Sperrsignal shows, and what a main signal shows *in addition* to its own aspect
/// while a shunting route is set over it. It is the only thing that lets a shunting
/// movement past a signal: Hp 1 authorises trains and says nothing to a shunt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ShuntAspect {
    /// Sh 0 — Halt! Fahrverbot. At a Sperrsignal it forbids every movement, a train
    /// movement included.
    #[default]
    Stop,
    /// Sh 1 — Fahrverbot aufgehoben: a shunting movement may pass, on sight.
    Proceed,
}

/// Complete signal aspect.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Aspect {
    #[serde(default)]
    pub main: Option<MainAspect>,
    #[serde(default)]
    pub distant: Option<DistantAspect>,
    /// Sh 0 / Sh 1. `None` = the signal carries no shunting signal at all, which stops a
    /// shunting movement just as Sh 0 does — a shunt does not pass a signal that has
    /// nothing to say to it.
    #[serde(default)]
    pub shunt: Option<ShuntAspect>,
    /// Zs3/Zs3v speed indicator [km/h].
    #[serde(default)]
    pub speed: Option<f64>,
}

impl Aspect {
    pub fn stop() -> Self {
        Self {
            main: Some(MainAspect::Stop),
            distant: None,
            shunt: None,
            speed: None,
        }
    }

    /// Does it show Sh 1?
    pub fn permits_shunting(&self) -> bool {
        self.shunt == Some(ShuntAspect::Proceed)
    }

    pub fn is_stop(&self) -> bool {
        self.main.is_some_and(MainAspect::is_stop)
    }

    /// Does the signal announce a restriction? (Basis of the 1000 Hz activation.)
    pub fn announces_restriction(&self) -> bool {
        self.distant.is_some_and(DistantAspect::is_restrictive)
    }
}

/// What the interlocking knows about a signal in this step.
///
/// Input of the declarative signal type and of the optional script hook — the signal system
/// stays data, the interlocking only supplies the situation (plan ch. 19).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Situation {
    /// All guarded sections are clear.
    pub clear: bool,
    /// A route is locked at this signal.
    pub route: bool,
    /// The locked route leads over a diverging path.
    pub diverging: bool,
    /// The following main signal shows stop.
    pub next_stop: bool,
    /// The following main signal shows slow speed.
    pub next_slow: bool,
    /// A **shunting** route is locked at this signal — the signal shows Sh 1, and a rule
    /// table names the lamp image that goes with it.
    #[serde(default)]
    pub shunting: bool,
}

/// Condition of an aspect rule — only the fields that are set have to match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Condition {
    #[serde(default)]
    pub clear: Option<bool>,
    #[serde(default)]
    pub route: Option<bool>,
    #[serde(default)]
    pub diverging: Option<bool>,
    #[serde(default)]
    pub next_stop: Option<bool>,
    #[serde(default)]
    pub next_slow: Option<bool>,
    #[serde(default)]
    pub shunting: Option<bool>,
}

impl Condition {
    pub fn matches(&self, s: &Situation) -> bool {
        let ok = |c: Option<bool>, v: bool| c.is_none_or(|c| c == v);
        ok(self.clear, s.clear)
            && ok(self.route, s.route)
            && ok(self.diverging, s.diverging)
            && ok(self.next_stop, s.next_stop)
            && ok(self.next_slow, s.next_slow)
            && ok(self.shunting, s.shunting)
    }
}

/// One row of the signal state machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AspectRule {
    #[serde(default)]
    pub when: Condition,
    pub show: Aspect,
    /// Lamp image for the presentation, e.g. `["red"]` or `["green", "yellow"]`.
    #[serde(default)]
    pub lamps: Vec<String>,
}

/// A signal type as data: which aspect belongs to which situation.
///
/// Comes from a mod (`signals/*.ron`) and replaces the built-in aspect logic for the
/// signals that reference it. Behaviour that a table cannot express — substitute signal,
/// counting down a timer, hand-operated signals — goes into the optional `script`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignalType {
    #[serde(default = "default_system")]
    pub system: SignalSystem,
    /// The first matching rule wins; without a match the signal shows stop.
    pub rules: Vec<AspectRule>,
    /// Optional Lua hook `"<mod>:<script>"` — runs after the rules and may override
    /// the aspect. Evaluated by the mod runtime, not by `sim-core`.
    #[serde(default)]
    pub script: Option<String>,
    /// Default 3D model, `"<mod>:<name>"` below `signal_models/`. A placement may
    /// override it per signal (`SignalSource::model`).
    #[serde(default)]
    pub model: Option<String>,
    /// Free-form tags the mod author gives the entry, for finding it again in
    /// a catalogue of thousands: `["mast", "catenary", "epoch-4"]`. Lower-case
    /// kebab by convention — the editors normalise what is typed, and the
    /// content drawer lower-cases when it groups, so a hand-written `Mast`
    /// still lands on the same tag.
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_system() -> SignalSystem {
    SignalSystem::Ks
}

/// Modular 3D model of a signal: glTF parts chained by mount-point nodes, plus the
/// binding of lamp-image strings to nodes (the vehicle path of ch. 15.3, applied to
/// signals — after the Zusi pattern, where screens, masts and indicators are shared
/// files linked together).
///
/// Comes from a mod (`signal_models/*.ron`). The renderer shows a bound node while
/// its string is in the signal's current lamp image and hides it otherwise.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SignalModel {
    /// A part without a `mount` stands at the device position itself; every other
    /// part hangs off a named node of an earlier part.
    pub parts: Vec<SignalPart>,
    /// Which glTF node is which lamp-image string.
    #[serde(default)]
    pub lamps: Vec<LampBinding>,
    /// Moving nodes — semaphore arms and the like.
    #[serde(default)]
    pub motions: Vec<MotionBinding>,
    /// Levels of detail over all parts, coarsest last — nodes named
    /// `<name>_LOD<level>` switch by camera distance, beyond the last distance
    /// they disappear. Empty = the whole assembly at every distance.
    #[serde(default)]
    pub lods: Vec<crate::train::Lod>,
    /// Free-form tags the mod author gives the entry, for finding it again in
    /// a catalogue of thousands: `["mast", "catenary", "epoch-4"]`. Lower-case
    /// kebab by convention — the editors normalise what is typed, and the
    /// content drawer lower-cases when it groups, so a hand-written `Mast`
    /// still lands on the same tag.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// One glTF file of a signal assembly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignalPart {
    /// glTF below `mods/`, e.g. `"example/assets/sig_schirm_ks.gltf"`.
    pub file: String,
    /// `(part, node)`: the mount-point node of another part this one hangs off.
    /// `None` puts the part at the device position.
    #[serde(default)]
    pub mount: Option<(u32, String)>,
}

/// Binds one lamp-image string to one glTF node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LampBinding {
    /// Lamp-image string of the aspect rules, e.g. `"red"` or `"zs3_4"`.
    pub lamp: String,
    /// Part the node lives in.
    #[serde(default)]
    pub part: u32,
    /// glTF node — visible while its string is in the signal's lamp image.
    pub node: String,
}

/// Binds one lamp-image string to a moving node — a semaphore arm is a motion
/// bound to its own string (`"fluegel1"`), which the aspect rules put into the
/// lamp image like any lamp.
///
/// The string is the *target*: while it is in the current lamp image the node
/// travels to 1, without it back to 0, at the pace the travel time sets — so a
/// quick aspect change swings the arm through its real intermediate positions.
/// One binding per node; an aspect that moves two arms simply lists two strings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotionBinding {
    /// Lamp-image string that drives the node to full travel.
    pub lamp: String,
    /// Part the node lives in.
    #[serde(default)]
    pub part: u32,
    /// glTF node the motion moves.
    pub node: String,
    /// How the node moves over the travel 0 … 1.
    pub motion: crate::train::Motion,
    /// Travel time of the full swing [s]; 0 switches instantly.
    #[serde(default = "default_motion_seconds")]
    pub seconds: f64,
}

fn default_motion_seconds() -> f64 {
    1.5
}

/// Track clear detection section (axle counter section).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackSection {
    pub id: SectionId,
    pub edges: Vec<EdgeId>,
    pub occupied: bool,
    /// Which trains are standing on it. `occupied` is only whether this list is empty;
    /// *who* is on it is what tells a movement's own road apart from one that was
    /// occupied before it started (see [`Route::owner`]).
    #[serde(default)]
    pub trains: Vec<usize>,
    /// Locked by a route.
    pub locked_by: Option<RouteId>,
}

/// A signal of the line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signal {
    pub id: SignalId,
    pub system: SignalSystem,
    pub kind: SignalKind,
    /// Associated trackside device (position in the track network).
    pub device: track_model::DeviceId,
    /// Following main signal — for the distant signalling.
    pub next: Option<SignalId>,
    /// Sections that must be clear for the signal to be allowed to show proceed.
    pub guarded: Vec<SectionId>,
    /// The signal shows proceed only with a set route (interlocking signal);
    /// otherwise it is an automatic block signal.
    pub requires_route: bool,
    /// Speed for a diverging move [km/h] (Zs3).
    pub diverging_speed: Option<f64>,
    /// Current aspect.
    pub aspect: Aspect,
    /// Cleared route.
    pub route: Option<RouteId>,
    /// Signal type from a mod (index into `Interlock::types`). With it the aspect comes
    /// from the rule table instead of the built-in logic.
    #[serde(default)]
    pub type_index: Option<u32>,
    /// Situation of the last update — input of rule table and script hook.
    #[serde(default)]
    pub situation: Situation,
    /// Lamp image of the current aspect (only with a signal type).
    #[serde(default)]
    pub lamps: Vec<String>,
    /// How many set routes hold this signal at stop as their flank protection.
    /// A counter, not a flag: two routes may lean on the same signal, and the
    /// second one to be released must not clear it for the first.
    #[serde(default)]
    pub flank_locked: u32,
}

impl Signal {
    pub fn new(id: SignalId, kind: SignalKind, device: track_model::DeviceId) -> Self {
        Self {
            id,
            system: SignalSystem::Ks,
            kind,
            device,
            next: None,
            guarded: Vec::new(),
            requires_route: false,
            diverging_speed: None,
            // Default position: main signals show stop, distant signals "expect stop",
            // and a Sperrsignal shows Sh 0 and no main aspect at all.
            aspect: match kind {
                SignalKind::Distant => Aspect {
                    main: None,
                    distant: Some(DistantAspect::ExpectStop),
                    shunt: None,
                    speed: None,
                },
                SignalKind::Shunting => Aspect {
                    main: None,
                    distant: None,
                    shunt: Some(ShuntAspect::Stop),
                    speed: None,
                },
                _ => Aspect::stop(),
            },
            route: None,
            type_index: None,
            situation: Situation::default(),
            lamps: Vec::new(),
            flank_locked: 0,
        }
    }
}

/// State of a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RouteState {
    #[default]
    Free,
    /// Requested — switches are moving.
    Requested,
    /// Locked (switches locked, the signal may show proceed).
    Locked,
    /// Train inside the route.
    Occupied,
}

/// What a route authorises (Ril 408 / 301).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RouteKind {
    /// A **Zugfahrstraße**: the path is proved clear before the entry signal clears, and
    /// what it clears is the main aspect.
    #[default]
    Train,
    /// A **Rangierfahrstraße**: it locks the points and clears **Sh 1** at the entry
    /// signal, and it may be set into an *occupied* track — which is what shunting is
    /// for. It has no overlap: a shunting movement is driven on sight, so there is
    /// nothing to run past.
    Shunt,
}

/// A route from the entry to the exit signal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    pub id: RouteId,
    pub entry: SignalId,
    pub exit: SignalId,
    /// Train route or shunting route.
    #[serde(default)]
    pub kind: RouteKind,
    /// Required positions of the switches in the path.
    pub switches: Vec<(NodeId, SwitchPosition)>,
    /// Sections of the path, in the direction of travel.
    pub sections: Vec<SectionId>,
    /// Overlap behind the exit signal.
    pub overlap: Vec<SectionId>,
    /// Flank protection: what keeps a vehicle off the path from the side.
    #[serde(default)]
    pub flank: Vec<FlankGuard>,
    /// The route leads over a diverging path (slow speed).
    pub diverging: bool,
    pub state: RouteState,
    /// The movement this route belongs to, once one has passed its entry signal
    /// ([`Interlock::entered`]).
    ///
    /// A **train** route needs none: the track clear detection proves the whole path
    /// empty behind the train, whoever it was. A **shunting** route does: it may have been
    /// set into an occupied road, so the only thing that says whether *its* movement has
    /// run over it and cleared it is which train is where. Without this the first thing to
    /// pass the signal released the route — right where one shunt works alone, wrong the
    /// moment a station has two.
    #[serde(default)]
    pub owner: Option<usize>,
    /// How long it has stood locked without its movement running over it \[s\].
    #[serde(default)]
    pub waiting: f64,
}

impl Route {
    pub fn new(id: RouteId, entry: SignalId, exit: SignalId) -> Self {
        Self {
            id,
            entry,
            exit,
            kind: RouteKind::Train,
            switches: Vec::new(),
            sections: Vec::new(),
            overlap: Vec::new(),
            flank: Vec::new(),
            diverging: false,
            state: RouteState::Free,
            owner: None,
            waiting: 0.0,
        }
    }

    /// The same route as a shunting route.
    pub fn shunting(mut self) -> Self {
        self.kind = RouteKind::Shunt;
        self
    }
}

/// One flank protection measure of a route: what keeps a vehicle from running
/// into its path from the side, where a track joins it.
///
/// The two the interlocking can enforce itself. A track that ends in a buffer
/// stop needs none, and the turnout a route runs into facing protects the
/// route by lying in the position the route needs anyway.
///
/// A **track lock** is the signal case: it is a [`SignalKind::TrackLock`],
/// so holding it at stop is holding it laid on — which is exactly what flank
/// protection by a Gleissperre is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlankGuard {
    /// Protecting turnout (Schutzweiche): held in the position that leads a
    /// flank movement away from the route, and locked with the route.
    Switch(NodeId, SwitchPosition),
    /// Protecting signal (Schutzsignal): held at stop while the route is set,
    /// so nothing can be cleared onto the route from the side.
    Signal(SignalId),
}

/// Activation condition of a signal-dependent trackside device.
///
/// Country-neutral: the device itself states when it is active; the interlocking only knows
/// the signal aspect. That keeps PZB magnets a German matter while the link stays generic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Activation {
    /// Always active.
    #[default]
    Always,
    /// Active when the associated signal shows stop (500/2000 Hz).
    WhenStop,
    /// Active when the associated signal announces a restriction (1000 Hz).
    WhenRestrictive,
}

/// Neutral part of a device payload: signal reference and activation.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct DeviceLink {
    #[serde(default)]
    pub signal: Option<u32>,
    #[serde(default)]
    pub activation: Activation,
}

/// Payload of a `DeviceKind::BlockMarker` device — a block boundary in the line data. Which
/// train protection makes use of it is its own business: the LZB ends a movement authority
/// here, the AI driver brakes for it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct BlockMarkerPayload {
    /// Track section behind the marker.
    pub section: u32,
}

/// The interlocking.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Interlock {
    pub signals: Vec<Signal>,
    pub sections: Vec<TrackSection>,
    pub routes: Vec<Route>,
    /// Signal types supplied by mods; `Signal::type_index` points into this.
    #[serde(default)]
    pub types: Vec<SignalType>,
}

impl Interlock {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_section(&mut self, edges: Vec<EdgeId>) -> SectionId {
        let id = SectionId(self.sections.len() as u32);
        self.sections.push(TrackSection {
            id,
            edges,
            occupied: false,
            trains: Vec::new(),
            locked_by: None,
        });
        id
    }

    pub fn add_signal(&mut self, mut signal: Signal) -> SignalId {
        let id = SignalId(self.signals.len() as u32);
        signal.id = id;
        self.signals.push(signal);
        id
    }

    pub fn add_route(&mut self, mut route: Route) -> RouteId {
        let id = RouteId(self.routes.len() as u32);
        route.id = id;
        self.routes.push(route);
        id
    }

    pub fn signal(&self, id: SignalId) -> &Signal {
        &self.signals[id.index()]
    }

    pub fn section(&self, id: SectionId) -> &TrackSection {
        &self.sections[id.index()]
    }

    pub fn route(&self, id: RouteId) -> &Route {
        &self.routes[id.index()]
    }

    /// Track clear detection: which sections are occupied by vehicles?
    /// Track clear detection: which sections are occupied, and by whom.
    ///
    /// *By whom* is what a **shunting** route needs. It may have been set into a road that
    /// was occupied to begin with, so "the section shows occupied" says nothing about
    /// whether the movement it was set for has run over it yet — only whose wheels are on
    /// it does (see [`Route::owner`]).
    pub fn update_occupancy(&mut self, occupied: &[(usize, EdgeId)]) {
        for s in &mut self.sections {
            s.trains.clear();
            for (train, edge) in occupied {
                if s.edges.contains(edge) && !s.trains.contains(train) {
                    s.trains.push(*train);
                }
            }
            s.occupied = !s.trains.is_empty();
        }
    }

    /// Request a route (automatic route setting or dispatcher).
    pub fn request_route(&mut self, id: RouteId, net: &mut TrackNetwork) -> bool {
        let route = &self.routes[id.index()];
        if route.state != RouteState::Free {
            return route.state == RouteState::Locked;
        }
        // A signal another route holds at stop as its flank protection cannot
        // clear a route of its own — that is what the protection is for.
        if self.signals[route.entry.index()].flank_locked > 0 {
            return false;
        }
        // No section may be locked by something else — and, for a train route, none may
        // be occupied either. A **shunting** route is deliberately allowed into an
        // occupied track: setting back onto a rake that is standing there is the move it
        // exists for, and the movement is driven on sight (Ril 408).
        let kind = route.kind;
        let blocked = route.sections.iter().chain(route.overlap.iter()).any(|s| {
            let sec = &self.sections[s.index()];
            (sec.occupied && kind == RouteKind::Train) || sec.locked_by.is_some_and(|r| r != id)
        });
        if blocked {
            return false;
        }
        // A protecting signal has to be free to be held: a route already set
        // there runs where this one wants protection.
        let flank = route.flank.clone();
        for guard in &flank {
            if let FlankGuard::Signal(signal) = guard
                && self
                    .routes
                    .iter()
                    .any(|r| r.entry == *signal && r.state != RouteState::Free)
            {
                return false;
            }
        }
        // Move the switches of the path and of the flank protection alike —
        // a protecting turnout is set and locked exactly like one in the path.
        let switches: Vec<(NodeId, SwitchPosition)> = self.routes[id.index()]
            .switches
            .iter()
            .copied()
            .chain(flank.iter().filter_map(|g| match g {
                FlankGuard::Switch(node, position) => Some((*node, *position)),
                FlankGuard::Signal(_) => None,
            }))
            .collect();
        for (node, pos) in &switches {
            if let Some(sw) = net.switch_mut(*node) {
                if sw.locked {
                    return false;
                }
                if sw.command(*pos).is_err() {
                    return false;
                }
            }
        }
        for guard in &flank {
            if let FlankGuard::Signal(signal) = guard {
                self.signals[signal.index()].flank_locked += 1;
            }
        }
        let sections: Vec<SectionId> = route
            .sections
            .iter()
            .chain(route.overlap.iter())
            .copied()
            .collect();
        for s in sections {
            self.sections[s.index()].locked_by = Some(id);
        }
        self.routes[id.index()].state = RouteState::Requested;
        true
    }

    /// Is `train` standing anywhere on route `i`?
    fn on_route(&self, i: usize, train: Option<usize>) -> bool {
        let Some(train) = train else { return false };
        self.routes[i]
            .sections
            .iter()
            .any(|s| self.sections[s.index()].trains.contains(&train))
    }

    /// A movement has passed `signal` under Sh 1: the shunting route locked there is
    /// **its** route from here on.
    ///
    /// This is where a route learns whose it is. Until now it was set for whoever was
    /// standing in front of the signal, which is a guess; from here it belongs to this
    /// movement, and only this movement clearing it releases it again — a second shunt
    /// running past the same signal takes nothing away from the first. The signal itself
    /// falls back to Sh 0 as the movement passes it, which is what the first axle does to
    /// a Sperrsignal.
    pub fn entered(&mut self, signal: SignalId, train: usize) {
        let Some(id) = self
            .signals
            .get(signal.index())
            .and_then(|sig| sig.route)
            .filter(|id| self.routes[id.index()].kind == RouteKind::Shunt)
        else {
            return;
        };
        self.routes[id.index()].owner = Some(train);
        self.routes[id.index()].waiting = 0.0;
        self.signals[signal.index()].route = None;
    }

    /// Sets the first free **shunting** route out of `entry`, if there is one.
    ///
    /// The signalman's answer to a shunting movement that has drawn up to a signal and is
    /// waiting: it is asked for by name of the signal rather than of the route, because
    /// that is all the movement standing in front of it knows.
    pub fn request_shunt_route(&mut self, entry: SignalId, net: &mut TrackNetwork) -> bool {
        let Some(id) = self
            .routes
            .iter()
            .find(|r| r.entry == entry && r.kind == RouteKind::Shunt && r.state == RouteState::Free)
            .map(|r| r.id)
        else {
            return false;
        };
        self.request_route(id, net)
    }

    /// Release a route (after the train has passed or on cancellation).
    pub fn release_route(&mut self, id: RouteId, net: &mut TrackNetwork) {
        self.routes[id.index()].owner = None;
        self.routes[id.index()].waiting = 0.0;
        let route = &self.routes[id.index()];
        let flank = route.flank.clone();
        let switches: Vec<NodeId> = route
            .switches
            .iter()
            .map(|(node, _)| *node)
            .chain(flank.iter().filter_map(|g| match g {
                FlankGuard::Switch(node, _) => Some(*node),
                FlankGuard::Signal(_) => None,
            }))
            .collect();
        let sections: Vec<SectionId> = route
            .sections
            .iter()
            .chain(route.overlap.iter())
            .copied()
            .collect();
        for node in switches {
            if let Some(sw) = net.switch_mut(node) {
                sw.locked = false;
            }
        }
        for guard in &flank {
            if let FlankGuard::Signal(signal) = guard {
                let held = &mut self.signals[signal.index()].flank_locked;
                *held = held.saturating_sub(1);
            }
        }
        for s in sections {
            let sec = &mut self.sections[s.index()];
            if sec.locked_by == Some(id) {
                sec.locked_by = None;
            }
        }
        self.routes[id.index()].state = RouteState::Free;
        let entry = self.routes[id.index()].entry;
        self.signals[entry.index()].route = None;
    }

    /// One step of the interlocking logic: lock/release routes, set signals.
    pub fn update(&mut self, net: &mut TrackNetwork, dt: f64) {
        self.update_routes(net, dt);
        self.update_signals();
    }

    fn update_routes(&mut self, net: &mut TrackNetwork, dt: f64) {
        for i in 0..self.routes.len() {
            match self.routes[i].state {
                RouteState::Requested => {
                    // Lock as soon as every switch is in position — those of
                    // the path and those that give flank protection alike.
                    let switches: Vec<(NodeId, SwitchPosition)> = self.routes[i]
                        .switches
                        .iter()
                        .copied()
                        .chain(self.routes[i].flank.iter().filter_map(|g| match g {
                            FlankGuard::Switch(node, position) => Some((*node, *position)),
                            FlankGuard::Signal(_) => None,
                        }))
                        .collect();
                    let ready = switches.iter().all(|(node, pos)| {
                        net.switch(*node)
                            .is_none_or(|sw| !sw.is_moving() && sw.position == *pos && !sw.trailed)
                    });
                    if ready {
                        for (node, _) in switches {
                            if let Some(sw) = net.switch_mut(node) {
                                sw.locked = true;
                            }
                        }
                        self.routes[i].state = RouteState::Locked;
                        let entry = self.routes[i].entry;
                        let id = self.routes[i].id;
                        self.signals[entry.index()].route = Some(id);
                    }
                }
                RouteState::Locked => {
                    // Has the movement entered the route? A **train** route asks the track
                    // clear detection: anything on the path is the train it was set for,
                    // because nothing else was let on. A **shunting** route cannot ask that
                    // — it may have been set into an occupied road in the first place — so
                    // it asks whether *its own* movement is on it, which it knows since
                    // that movement passed its signal (`Interlock::entered`).
                    let entered = match self.routes[i].kind {
                        RouteKind::Train => self.routes[i]
                            .sections
                            .iter()
                            .any(|s| self.sections[s.index()].occupied),
                        RouteKind::Shunt => self.on_route(i, self.routes[i].owner),
                    };
                    if entered {
                        self.routes[i].state = RouteState::Occupied;
                        self.routes[i].waiting = 0.0;
                        // The signal drops to stop behind the train.
                        let entry = self.routes[i].entry;
                        self.signals[entry.index()].route = None;
                    } else if self.routes[i].kind == RouteKind::Shunt {
                        // Zeitverschluss: a shunting route set for a movement that never
                        // came is given back, or the points under it would stay locked for
                        // the rest of the day. Whoever is still standing at the signal asks
                        // for it again a second later (`Sim::set_shunt_routes`).
                        self.routes[i].waiting += dt;
                        if self.routes[i].waiting > SHUNT_HOLD {
                            let (id, entry) = (self.routes[i].id, self.routes[i].entry);
                            self.signals[entry.index()].route = None;
                            self.release_route(id, net);
                        }
                    }
                }
                RouteState::Occupied => {
                    // Release once the movement has completely cleared the path — the
                    // whole path for a train, *its own* wheels for a shunt.
                    let cleared = match self.routes[i].kind {
                        RouteKind::Train => self.routes[i]
                            .sections
                            .iter()
                            .all(|s| !self.sections[s.index()].occupied),
                        RouteKind::Shunt => !self.on_route(i, self.routes[i].owner),
                    };
                    if cleared {
                        let id = self.routes[i].id;
                        self.release_route(id, net);
                    }
                }
                RouteState::Free => {}
            }
        }
    }

    fn update_signals(&mut self) {
        // 1. Main signal aspects.
        for i in 0..self.signals.len() {
            let sig = &self.signals[i];
            if sig.kind == SignalKind::Distant {
                continue;
            }
            // A signal held as another route's flank protection counts as
            // "not clear": it shows stop, and so does a mod's rule table,
            // which reads the same situation.
            let free = sig.flank_locked == 0
                && sig
                    .guarded
                    .iter()
                    .all(|s| !self.sections[s.index()].occupied);
            // Which kind of route is set here decides what the signal clears: a train
            // route clears the main aspect, a shunting route clears Sh 1 and leaves the
            // main signal at stop (Ril 301 — Hp 1 is never shown to a shunting movement).
            let kind = sig.route.map(|r| self.routes[r.index()].kind);
            let train_route = kind == Some(RouteKind::Train);
            let shunting = kind == Some(RouteKind::Shunt) && sig.flank_locked == 0;
            let route_ok = !sig.requires_route || train_route;
            let diverging = sig
                .route
                .map(|r| self.routes[r.index()].diverging)
                .unwrap_or(false);
            let main = if free && route_ok {
                if diverging {
                    MainAspect::ProceedSlow
                } else {
                    MainAspect::Proceed
                }
            } else {
                MainAspect::Stop
            };
            let speed = if main == MainAspect::ProceedSlow {
                self.signals[i].diverging_speed
            } else {
                None
            };
            let sperrsignal = sig.kind == SignalKind::Shunting;
            let sig = &mut self.signals[i];
            // A Sperrsignal has no main aspect: Sh 0 and Sh 1 are all it shows, and Sh 0
            // is "Halt! Fahrverbot" for every movement.
            sig.aspect.main = (!sperrsignal).then_some(main);
            sig.aspect.speed = speed;
            sig.aspect.shunt = match (sperrsignal, shunting) {
                (_, true) => Some(ShuntAspect::Proceed),
                (true, false) => Some(ShuntAspect::Stop),
                // A main signal with no shunting route set over it says nothing to a
                // shunt, which is itself an answer: it stops there and waits for an order.
                (false, false) => None,
            };
            sig.situation.clear = free;
            sig.situation.route = sig.route.is_some();
            sig.situation.diverging = diverging;
            sig.situation.shunting = shunting;
        }

        // 2. Distant signalling and the mod rule tables, in signalling order: a signal is
        // evaluated after its `next`, so `situation.next_*` and the distant aspect see the
        // following signal's *final* aspect — rule table included — from the same update.
        // A `next` cycle (ring line) is cut where the walk started; within the ring the
        // announcement is then one step late, which is where every signal stood before.
        for i in self.signalling_order() {
            if let Some(next) = self.signals[i].next {
                let next_main = self.signals[next.index()]
                    .aspect
                    .main
                    .unwrap_or(MainAspect::Stop);
                let distant = match next_main {
                    MainAspect::Proceed | MainAspect::Substitute => DistantAspect::ExpectProceed,
                    MainAspect::ProceedSlow => DistantAspect::ExpectSlow,
                    _ => DistantAspect::ExpectStop,
                };
                self.signals[i].aspect.distant = Some(distant);
                self.signals[i].situation.next_stop = next_main.is_stop();
                self.signals[i].situation.next_slow = next_main == MainAspect::ProceedSlow;
            } else if self.signals[i].kind != SignalKind::Main {
                self.signals[i].aspect.distant = None;
            }

            // Signal types from mods: the rule table replaces the built-in aspect.
            let Some(ty) = self.signals[i]
                .type_index
                .and_then(|t| self.types.get(t as usize))
            else {
                continue;
            };
            // Fallback per plan 19.3: a type without a matching rule shows stop.
            let (aspect, lamps) = match ty
                .rules
                .iter()
                .find(|r| r.when.matches(&self.signals[i].situation))
            {
                Some(rule) => (rule.show, rule.lamps.clone()),
                None => (Aspect::stop(), Vec::new()),
            };
            self.signals[i].aspect = aspect;
            self.signals[i].lamps = lamps;
        }
    }

    /// Signal indices ordered so that a signal comes after its `next` — the order the
    /// aspects propagate against the direction of travel.
    fn signalling_order(&self) -> Vec<usize> {
        let n = self.signals.len();
        let mut order = Vec::with_capacity(n);
        // 0 = unvisited, 1 = on the current chain, 2 = ordered.
        let mut state = vec![0u8; n];
        for start in 0..n {
            let mut chain = Vec::new();
            let mut i = start;
            while state[i] == 0 {
                state[i] = 1;
                chain.push(i);
                match self.signals[i].next {
                    Some(next) if state[next.index()] == 0 => i = next.index(),
                    _ => break,
                }
            }
            for &j in chain.iter().rev() {
                state[j] = 2;
                order.push(j);
            }
        }
        order
    }

    /// Registers a signal type and returns its index (for the mod runtime).
    pub fn add_type(&mut self, ty: SignalType) -> u32 {
        self.types.push(ty);
        self.types.len() as u32 - 1
    }

    /// Signal belonging to a trackside device (if it is one).
    pub fn signal_at_device(&self, device: track_model::DeviceId) -> Option<&Signal> {
        self.signals.iter().find(|s| s.device == device)
    }

    /// Is a signal-dependent trackside device currently active?
    ///
    /// Basis of the PZB magnet activation: 1000 Hz with an announced restriction,
    /// 500/2000 Hz with a signal showing stop.
    pub fn device_active(&self, device: &TracksideDevice) -> bool {
        let link: DeviceLink = ron::from_str(&device.payload).unwrap_or_default();
        match link.activation {
            Activation::Always => true,
            Activation::WhenStop | Activation::WhenRestrictive => {
                let Some(signal) = link.signal.map(SignalId) else {
                    return true;
                };
                let Some(sig) = self.signals.get(signal.index()) else {
                    return true;
                };
                match link.activation {
                    // Sh 1 switches the magnet off: a shunting movement is meant to pass
                    // this signal, and a 2000 Hz magnet under it would trip every one of
                    // them (Ril 301).
                    Activation::WhenStop => sig.aspect.is_stop() && !sig.aspect.permits_shunting(),
                    Activation::WhenRestrictive => sig.aspect.announces_restriction(),
                    Activation::Always => true,
                }
            }
        }
    }

    /// Aspect of a signal as a speed requirement [km/h], if it is restrictive.
    pub fn signal_speed(&self, id: SignalId, movement: Movement) -> Option<f64> {
        let s = self.signal(id);
        // A shunting movement is let past by Sh 1 and by nothing else — a main signal
        // showing Hp 1 has said nothing to it — and where it is let past it runs on sight
        // at shunting speed.
        let shunting = |s: &Signal| {
            Some(if s.aspect.permits_shunting() {
                SHUNTING_SPEED_KMH
            } else {
                0.0
            })
        };
        match s.kind {
            // A distant signal announces; it stops nothing.
            SignalKind::Distant => None,
            // Laid on, it derails whatever runs onto it, train or shunt alike.
            SignalKind::TrackLock => s.aspect.is_stop().then_some(0.0),
            // Sh 0 at a Sperrsignal is "Halt! Fahrverbot" — it forbids every movement,
            // and a train movement past it is not a thing either.
            SignalKind::Shunting => shunting(s),
            SignalKind::Main | SignalKind::Combined => match movement {
                Movement::Shunt => shunting(s),
                Movement::Train => match s.aspect.main? {
                    MainAspect::Stop | MainAspect::DarkLight => Some(0.0),
                    MainAspect::ProceedSlow => s.aspect.speed.or(Some(40.0)),
                    MainAspect::Substitute => Some(40.0),
                    MainAspect::Proceed => None,
                },
            },
        }
    }
}

/// Helper for content: check the payload of a signal-dependent device.
pub fn is_signal_device(kind: &DeviceKind) -> bool {
    // The line conductor is not one of them: its telegram comes from the LZB centre, which
    // reads the interlocking itself.
    matches!(kind, DeviceKind::Magnet | DeviceKind::Signal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use track_model::{
        DeviceId, EdgeId, NodeKind, Segment, Switch, SwitchPosition, TrackEdge, TrackNetwork,
    };
    use world_coords::geo::to_ecef_deg;

    fn net_with_switch() -> (TrackNetwork, NodeId, EdgeId, EdgeId, EdgeId) {
        let mut net = TrackNetwork::new();
        let a = net.add_node(NodeKind::Buffer);
        let b = net.add_node(NodeKind::Joint);
        let c = net.add_node(NodeKind::Buffer);
        let d = net.add_node(NodeKind::Buffer);
        let anchor = to_ecef_deg(52.0, 10.0, 100.0);
        let e0 = net.add_edge(TrackEdge::new(
            EdgeId(0),
            a,
            b,
            anchor,
            0.0,
            vec![Segment::straight(500.0)],
        ));
        let p = net.edge(e0).end_pose().pos;
        let e1 = net.add_edge(TrackEdge::new(
            EdgeId(0),
            b,
            c,
            p,
            0.0,
            vec![Segment::straight(500.0)],
        ));
        let e2 = net.add_edge(TrackEdge::new(
            EdgeId(0),
            b,
            d,
            p,
            0.0,
            vec![Segment::arc(200.0, -400.0)],
        ));
        net.node_mut(b).kind = NodeKind::Switch(Switch::new(
            track_model::EdgeEnd::new(e0, track_model::EdgeSide::End),
            track_model::EdgeEnd::new(e1, track_model::EdgeSide::Start),
            track_model::EdgeEnd::new(e2, track_model::EdgeSide::Start),
        ));
        (net, b, e0, e1, e2)
    }

    #[test]
    fn automatic_block_drops_to_stop_behind_train() {
        let mut net = TrackNetwork::new();
        let a = net.add_node(NodeKind::Buffer);
        let b = net.add_node(NodeKind::Buffer);
        let e = net.add_edge(TrackEdge::new(
            EdgeId(0),
            a,
            b,
            to_ecef_deg(52.0, 10.0, 100.0),
            0.0,
            vec![Segment::straight(1000.0)],
        ));
        let mut il = Interlock::new();
        let sec = il.add_section(vec![e]);
        let mut sig = Signal::new(SignalId(0), SignalKind::Main, DeviceId(0));
        sig.guarded = vec![sec];
        let sid = il.add_signal(sig);

        il.update_occupancy(&[]);
        il.update(&mut net, 0.05);
        assert_eq!(il.signal(sid).aspect.main, Some(MainAspect::Proceed));

        il.update_occupancy(&[(0, e)]);
        il.update(&mut net, 0.05);
        assert_eq!(il.signal(sid).aspect.main, Some(MainAspect::Stop));
    }

    /// A line with one section and one signal on it, for the shunting rules below.
    fn one_section() -> (TrackNetwork, Interlock, EdgeId, SectionId) {
        let mut net = TrackNetwork::new();
        let a = net.add_node(NodeKind::Buffer);
        let b = net.add_node(NodeKind::Buffer);
        let e = net.add_edge(TrackEdge::new(
            EdgeId(0),
            a,
            b,
            to_ecef_deg(52.0, 10.0, 100.0),
            0.0,
            vec![Segment::straight(1000.0)],
        ));
        let mut il = Interlock::new();
        let sec = il.add_section(vec![e]);
        (net, il, e, sec)
    }

    /// Ril 301: a shunting route clears **Sh 1** at the entry signal and leaves the main
    /// signal at stop. Hp 1 is never shown to a shunting movement, and Sh 1 says nothing
    /// to a train.
    #[test]
    fn a_shunting_route_clears_sh1_and_leaves_the_main_signal_at_stop() {
        let (mut net, mut il, _, sec) = one_section();
        let mut sig = Signal::new(SignalId(0), SignalKind::Main, DeviceId(0));
        sig.guarded = vec![sec];
        sig.requires_route = true;
        let entry = il.add_signal(sig);
        let exit = il.add_signal(Signal::new(SignalId(0), SignalKind::Main, DeviceId(1)));
        let mut route = Route::new(RouteId(0), entry, exit).shunting();
        route.sections = vec![sec];
        let route = il.add_route(route);

        il.update_occupancy(&[]);
        il.update(&mut net, 0.05);
        // Nothing set: stop for a train, and nothing said to a shunt — which stops it too.
        assert_eq!(il.signal_speed(entry, Movement::Train), Some(0.0));
        assert_eq!(il.signal_speed(entry, Movement::Shunt), Some(0.0));

        assert!(il.request_route(route, &mut net));
        il.update(&mut net, 0.05);
        assert_eq!(
            il.signal(entry).aspect.main,
            Some(MainAspect::Stop),
            "a shunting route never clears the main signal"
        );
        assert!(il.signal(entry).aspect.permits_shunting(), "Sh 1");
        assert_eq!(
            il.signal_speed(entry, Movement::Shunt),
            Some(SHUNTING_SPEED_KMH),
            "and the shunt is let past, on sight"
        );
        assert_eq!(
            il.signal_speed(entry, Movement::Train),
            Some(0.0),
            "the train still waits for Hp 1"
        );
        // A mod's rule table is told about it, so it can name the Sh 1 lamp image.
        assert!(il.signal(entry).situation.shunting);
    }

    /// A shunting route may be set into an occupied track — that is what shunting is for.
    /// A train route may not.
    #[test]
    fn only_a_shunting_route_is_set_into_an_occupied_track() {
        let (mut net, mut il, edge, sec) = one_section();
        let entry = il.add_signal(Signal::new(SignalId(0), SignalKind::Main, DeviceId(0)));
        let exit = il.add_signal(Signal::new(SignalId(0), SignalKind::Main, DeviceId(1)));
        let mut train = Route::new(RouteId(0), entry, exit);
        train.sections = vec![sec];
        let train = il.add_route(train);
        let mut shunt = Route::new(RouteId(0), entry, exit).shunting();
        shunt.sections = vec![sec];
        let shunt = il.add_route(shunt);

        // Something is standing on it.
        il.update_occupancy(&[(0, edge)]);
        assert!(
            !il.request_route(train, &mut net),
            "a train is not let onto occupied track"
        );
        assert!(
            il.request_route(shunt, &mut net),
            "a shunt is — setting back onto a rake is the move it exists for"
        );
    }

    /// A shunting route belongs to the movement that ran over it.
    ///
    /// A second shunt passing the same signal takes nothing away from the first, and the
    /// route is given back when *its* movement has cleared it — not when the road happens
    /// to be empty, which on a road that was occupied to begin with it may never be.
    #[test]
    fn a_shunting_route_is_released_by_the_movement_it_belongs_to() {
        let (mut net, mut il, edge, sec) = one_section();
        let mut sig = Signal::new(SignalId(0), SignalKind::Shunting, DeviceId(0));
        sig.requires_route = true;
        let entry = il.add_signal(sig);
        let exit = il.add_signal(Signal::new(SignalId(0), SignalKind::Main, DeviceId(1)));
        let mut route = Route::new(RouteId(0), entry, exit).shunting();
        route.sections = vec![sec];
        let route = il.add_route(route);

        // A rake is standing on the road all along — which is why a shunting route may be
        // set into it at all, and why "the section is clear" can never release it.
        let rake = 7;
        il.update_occupancy(&[(rake, edge)]);
        assert!(il.request_route(route, &mut net));
        il.update(&mut net, 0.05);
        assert_eq!(il.routes[route.index()].state, RouteState::Locked);
        assert!(il.signal(entry).aspect.permits_shunting(), "Sh 1");

        // The shunt passes the signal: the route is its own from here.
        let shunt = 3;
        il.entered(entry, shunt);
        assert_eq!(il.routes[route.index()].owner, Some(shunt));
        assert!(
            !il.signal(entry).aspect.permits_shunting() || il.signal(entry).route.is_none(),
            "and the signal falls back behind it"
        );

        // It runs over the road, beside the rake.
        il.update_occupancy(&[(rake, edge), (shunt, edge)]);
        il.update(&mut net, 0.05);
        assert_eq!(il.routes[route.index()].state, RouteState::Occupied);

        // Another movement runs past the same signal — and takes nothing.
        il.entered(entry, 5);
        assert_eq!(
            il.routes[route.index()].owner,
            Some(shunt),
            "the route is still the first one's"
        );

        // Still not released while its own movement is on it, rake or no rake.
        il.update(&mut net, 0.05);
        assert_eq!(il.routes[route.index()].state, RouteState::Occupied);

        // And released when *it* has cleared, though the rake is standing there yet.
        il.update_occupancy(&[(rake, edge)]);
        il.update(&mut net, 0.05);
        assert_eq!(il.routes[route.index()].state, RouteState::Free);
        assert_eq!(il.routes[route.index()].owner, None);
    }

    /// Zeitverschluss: a shunting route set for a movement that never came is given back,
    /// or the points under it would stay locked for the rest of the day.
    #[test]
    fn a_shunting_route_nobody_takes_is_given_back() {
        let (mut net, mut il, _, sec) = one_section();
        let mut sig = Signal::new(SignalId(0), SignalKind::Shunting, DeviceId(0));
        sig.requires_route = true;
        let entry = il.add_signal(sig);
        let exit = il.add_signal(Signal::new(SignalId(0), SignalKind::Main, DeviceId(1)));
        let mut route = Route::new(RouteId(0), entry, exit).shunting();
        route.sections = vec![sec];
        let route = il.add_route(route);

        il.update_occupancy(&[]);
        assert!(il.request_route(route, &mut net));
        il.update(&mut net, 0.05);
        assert_eq!(il.routes[route.index()].state, RouteState::Locked);

        // Nobody comes.
        let mut waited = 0.0;
        while waited < SHUNT_HOLD + 1.0 {
            il.update(&mut net, 0.5);
            waited += 0.5;
        }
        assert_eq!(il.routes[route.index()].state, RouteState::Free);
        assert!(
            !il.signal(entry).aspect.permits_shunting(),
            "and the signal is back at Sh 0"
        );
    }

    /// Sh 0 at a Sperrsignal is "Halt! Fahrverbot" — it stops a train movement as dead as
    /// a shunting one, and it shows no main aspect at all.
    #[test]
    fn a_sperrsignal_at_sh0_forbids_every_movement() {
        let (mut net, mut il, _, _) = one_section();
        let mut sig = Signal::new(SignalId(0), SignalKind::Shunting, DeviceId(0));
        sig.requires_route = true;
        let sid = il.add_signal(sig);
        il.update_occupancy(&[]);
        il.update(&mut net, 0.05);
        assert_eq!(il.signal(sid).aspect.main, None);
        assert_eq!(il.signal(sid).aspect.shunt, Some(ShuntAspect::Stop));
        assert_eq!(il.signal_speed(sid, Movement::Train), Some(0.0));
        assert_eq!(il.signal_speed(sid, Movement::Shunt), Some(0.0));
    }

    /// The 2000 Hz magnet under a signal is switched off while it shows Sh 1: a shunting
    /// movement is *meant* to pass it, and the magnet would trip every one of them.
    #[test]
    fn sh1_switches_the_2000_hz_magnet_off() {
        let (mut net, mut il, _, sec) = one_section();
        let mut sig = Signal::new(SignalId(0), SignalKind::Main, DeviceId(0));
        sig.guarded = vec![sec];
        sig.requires_route = true;
        let entry = il.add_signal(sig);
        let exit = il.add_signal(Signal::new(SignalId(0), SignalKind::Main, DeviceId(1)));
        let mut route = Route::new(RouteId(0), entry, exit).shunting();
        route.sections = vec![sec];
        let route = il.add_route(route);
        let magnet = track_model::TracksideDevice {
            id: DeviceId(1),
            kind: DeviceKind::Magnet,
            edge: EdgeId(0),
            s: 500.0,
            facing: track_model::Facing::Forward,
            lateral_offset: 0.0,
            payload: "(frequency:Hz2000,signal:Some(0),activation:WhenStop)".into(),
        };

        il.update_occupancy(&[]);
        il.update(&mut net, 0.05);
        assert!(il.device_active(&magnet), "the signal is at stop");

        il.request_route(route, &mut net);
        il.update(&mut net, 0.05);
        assert!(il.signal(entry).aspect.is_stop(), "still Hp 0");
        assert!(
            !il.device_active(&magnet),
            "but Sh 1 has switched the magnet off"
        );
    }

    /// A signal type from a mod decides the aspect instead of the built-in logic.
    #[test]
    fn signal_type_rules_replace_the_builtin_aspect() {
        let mut net = TrackNetwork::new();
        let a = net.add_node(NodeKind::Buffer);
        let b = net.add_node(NodeKind::Buffer);
        let e = net.add_edge(TrackEdge::new(
            EdgeId(0),
            a,
            b,
            to_ecef_deg(52.0, 10.0, 100.0),
            0.0,
            vec![Segment::straight(1000.0)],
        ));
        let mut il = Interlock::new();
        let sec = il.add_section(vec![e]);
        let ty = il.add_type(SignalType {
            system: SignalSystem::Ks,
            rules: vec![
                AspectRule {
                    when: Condition {
                        clear: Some(true),
                        ..Condition::default()
                    },
                    show: Aspect {
                        main: Some(MainAspect::ProceedSlow),
                        distant: None,
                        speed: Some(60.0),
                        shunt: None,
                    },
                    lamps: vec!["yellow".into(), "zs3_6".into()],
                },
                // No rule for an occupied section — the fallback is stop.
            ],
            script: None,
            model: None,
            tags: Vec::new(),
        });
        let mut sig = Signal::new(SignalId(0), SignalKind::Main, DeviceId(0));
        sig.guarded = vec![sec];
        sig.type_index = Some(ty);
        let sid = il.add_signal(sig);

        il.update_occupancy(&[]);
        il.update(&mut net, 0.05);
        assert_eq!(il.signal(sid).aspect.main, Some(MainAspect::ProceedSlow));
        assert_eq!(il.signal(sid).aspect.speed, Some(60.0));
        assert_eq!(il.signal(sid).lamps, ["yellow", "zs3_6"]);

        il.update_occupancy(&[(0, e)]);
        il.update(&mut net, 0.05);
        assert_eq!(il.signal(sid).aspect.main, Some(MainAspect::Stop));
        assert!(il.signal(sid).lamps.is_empty());
    }

    /// Two typed signals in a row announce each other within the same update: the
    /// predecessor's rule table sees the follower's rule-table aspect, not its built-in one.
    #[test]
    fn chained_typed_signals_announce_in_the_same_update() {
        let mut net = TrackNetwork::new();
        let a = net.add_node(NodeKind::Buffer);
        let b = net.add_node(NodeKind::Buffer);
        let e = net.add_edge(TrackEdge::new(
            EdgeId(0),
            a,
            b,
            to_ecef_deg(52.0, 10.0, 100.0),
            0.0,
            vec![Segment::straight(1000.0)],
        ));
        let mut il = Interlock::new();
        let sec = il.add_section(vec![e]);
        // The follower's type demands a locked route although the built-in logic does not:
        // built-in says proceed, the rule table says stop.
        let ty_route = il.add_type(SignalType {
            system: SignalSystem::Ks,
            rules: vec![AspectRule {
                when: Condition {
                    route: Some(true),
                    ..Condition::default()
                },
                show: Aspect {
                    main: Some(MainAspect::Proceed),
                    distant: None,
                    speed: None,
                    shunt: None,
                },
                lamps: vec!["green".into()],
            }],
            script: None,
            model: None,
            tags: Vec::new(),
        });
        let ty_announce = il.add_type(SignalType {
            system: SignalSystem::Ks,
            rules: vec![
                AspectRule {
                    when: Condition {
                        next_stop: Some(true),
                        ..Condition::default()
                    },
                    show: Aspect {
                        main: Some(MainAspect::Proceed),
                        distant: Some(DistantAspect::ExpectStop),
                        speed: None,
                        shunt: None,
                    },
                    lamps: vec!["yellow".into()],
                },
                AspectRule {
                    when: Condition::default(),
                    show: Aspect {
                        main: Some(MainAspect::Proceed),
                        distant: Some(DistantAspect::ExpectProceed),
                        speed: None,
                        shunt: None,
                    },
                    lamps: vec!["green".into()],
                },
            ],
            script: None,
            model: None,
            tags: Vec::new(),
        });
        // The announcing signal is stored *before* its follower, so storage order
        // would evaluate it first — the signalling order must not.
        let mut first = Signal::new(SignalId(0), SignalKind::Main, DeviceId(0));
        first.type_index = Some(ty_announce);
        first.next = Some(SignalId(1));
        let first_id = il.add_signal(first);
        let mut follower = Signal::new(SignalId(0), SignalKind::Main, DeviceId(1));
        follower.guarded = vec![sec];
        follower.type_index = Some(ty_route);
        let follower_id = il.add_signal(follower);

        il.update_occupancy(&[]);
        il.update(&mut net, 0.05);
        // No route locked: the follower's table shows stop (fallback) although its
        // built-in aspect would be proceed — and the first signal already announces it.
        assert_eq!(il.signal(follower_id).aspect.main, Some(MainAspect::Stop));
        assert_eq!(il.signal(first_id).lamps, ["yellow"]);
        assert_eq!(
            il.signal(first_id).aspect.distant,
            Some(DistantAspect::ExpectStop)
        );
    }

    #[test]
    fn distant_signal_follows_main_signal() {
        let mut net = TrackNetwork::new();
        let a = net.add_node(NodeKind::Buffer);
        let b = net.add_node(NodeKind::Buffer);
        let e = net.add_edge(TrackEdge::new(
            EdgeId(0),
            a,
            b,
            to_ecef_deg(52.0, 10.0, 100.0),
            0.0,
            vec![Segment::straight(1000.0)],
        ));
        let mut il = Interlock::new();
        let sec = il.add_section(vec![e]);
        let mut main = Signal::new(SignalId(0), SignalKind::Main, DeviceId(0));
        main.guarded = vec![sec];
        let main_id = il.add_signal(main);
        let mut distant = Signal::new(SignalId(0), SignalKind::Distant, DeviceId(1));
        distant.next = Some(main_id);
        let distant_id = il.add_signal(distant);

        il.update_occupancy(&[(0, e)]);
        il.update(&mut net, 0.05);
        assert_eq!(
            il.signal(distant_id).aspect.distant,
            Some(DistantAspect::ExpectStop)
        );
        assert!(il.signal(distant_id).aspect.announces_restriction());

        il.update_occupancy(&[]);
        il.update(&mut net, 0.05);
        assert_eq!(
            il.signal(distant_id).aspect.distant,
            Some(DistantAspect::ExpectProceed)
        );
        assert!(!il.signal(distant_id).aspect.announces_restriction());
    }

    #[test]
    fn route_sets_switch_locks_it_and_releases() {
        let (mut net, node, e0, _e1, e2) = net_with_switch();
        let mut il = Interlock::new();
        let s_entry = il.add_section(vec![e0]);
        let s_exit = il.add_section(vec![e2]);
        let mut sig = Signal::new(SignalId(0), SignalKind::Main, DeviceId(0));
        sig.requires_route = true;
        sig.guarded = vec![s_exit];
        sig.diverging_speed = Some(40.0);
        let sid = il.add_signal(sig);
        let exit_sig = il.add_signal(Signal::new(SignalId(0), SignalKind::Main, DeviceId(1)));
        let mut route = Route::new(RouteId(0), sid, exit_sig);
        route.switches = vec![(node, SwitchPosition::Diverging)];
        route.sections = vec![s_exit];
        route.diverging = true;
        let rid = il.add_route(route);

        // Without a route: stop.
        il.update(&mut net, 0.05);
        assert_eq!(il.signal(sid).aspect.main, Some(MainAspect::Stop));

        assert!(il.request_route(rid, &mut net));
        il.update(&mut net, 0.05);
        assert_eq!(
            il.route(rid).state,
            RouteState::Requested,
            "switch is moving"
        );
        net.update_switches(10.0);
        il.update(&mut net, 0.05);
        assert_eq!(il.route(rid).state, RouteState::Locked);
        assert!(net.switch(node).unwrap().locked, "switch locked");
        assert_eq!(il.signal(sid).aspect.main, Some(MainAspect::ProceedSlow));
        assert_eq!(il.signal(sid).aspect.speed, Some(40.0));

        // Train enters → signal to stop, route occupied.
        il.update_occupancy(&[(0, e2)]);
        il.update(&mut net, 0.05);
        assert_eq!(il.route(rid).state, RouteState::Occupied);
        assert_eq!(il.signal(sid).aspect.main, Some(MainAspect::Stop));

        // Train clears → release, switch free again.
        il.update_occupancy(&[]);
        il.update(&mut net, 0.05);
        assert_eq!(il.route(rid).state, RouteState::Free);
        assert!(!net.switch(node).unwrap().locked);
        let _ = s_entry;
    }

    #[test]
    fn occupied_path_prevents_route() {
        let (mut net, node, _e0, _e1, e2) = net_with_switch();
        let mut il = Interlock::new();
        let s_exit = il.add_section(vec![e2]);
        let sid = il.add_signal(Signal::new(SignalId(0), SignalKind::Main, DeviceId(0)));
        let exit = il.add_signal(Signal::new(SignalId(0), SignalKind::Main, DeviceId(1)));
        let mut route = Route::new(RouteId(0), sid, exit);
        route.switches = vec![(node, SwitchPosition::Diverging)];
        route.sections = vec![s_exit];
        let rid = il.add_route(route);

        il.update_occupancy(&[(0, e2)]);
        assert!(!il.request_route(rid, &mut net));
        assert_eq!(il.route(rid).state, RouteState::Free);
    }

    /// Flank protection: the route through the straight leg holds the turnout
    /// in the position that leads a flank movement away, and locks it — the
    /// same turnout, set for protection rather than for the path.
    #[test]
    fn a_protecting_turnout_is_set_and_locked_with_the_route() {
        let (mut net, node, e0, e1, _e2) = net_with_switch();
        let mut il = Interlock::new();
        let s_exit = il.add_section(vec![e1]);
        let entry = il.add_signal(Signal::new(SignalId(0), SignalKind::Main, DeviceId(0)));
        let exit = il.add_signal(Signal::new(SignalId(0), SignalKind::Main, DeviceId(1)));
        let mut route = Route::new(RouteId(0), entry, exit);
        route.sections = vec![s_exit];
        route.flank = vec![FlankGuard::Switch(node, SwitchPosition::Diverging)];
        let rid = il.add_route(route);

        // The switch starts straight, so protection has to move it.
        net.switch_mut(node).unwrap().position = SwitchPosition::Straight;
        assert!(il.request_route(rid, &mut net));
        il.update(&mut net, 0.05);
        assert_eq!(il.route(rid).state, RouteState::Requested, "switch moving");
        net.update_switches(10.0);
        il.update(&mut net, 0.05);
        assert_eq!(il.route(rid).state, RouteState::Locked);
        let sw = net.switch(node).unwrap();
        assert_eq!(sw.position, SwitchPosition::Diverging, "leads flank away");
        assert!(sw.locked, "and is locked with the route");

        il.release_route(rid, &mut net);
        assert!(!net.switch(node).unwrap().locked);
        let _ = e0;
    }

    /// A protecting signal stays at stop while the route is set, and no route
    /// can be cleared from it — which is the whole point of holding it.
    #[test]
    fn a_protecting_signal_is_held_at_stop() {
        let (mut net, node, _e0, e1, e2) = net_with_switch();
        let mut il = Interlock::new();
        let main_section = il.add_section(vec![e1]);
        let side_section = il.add_section(vec![e2]);
        let entry = il.add_signal(Signal::new(SignalId(0), SignalKind::Main, DeviceId(0)));
        let exit = il.add_signal(Signal::new(SignalId(0), SignalKind::Main, DeviceId(1)));
        // The signal on the side track, which the route wants held at stop.
        let mut guard = Signal::new(SignalId(0), SignalKind::Main, DeviceId(2));
        guard.guarded = vec![side_section];
        let guard_id = il.add_signal(guard);

        let mut route = Route::new(RouteId(0), entry, exit);
        route.sections = vec![main_section];
        route.flank = vec![FlankGuard::Signal(guard_id)];
        let rid = il.add_route(route);
        // A route of its own from the protecting signal.
        let mut side = Route::new(RouteId(0), guard_id, exit);
        side.sections = vec![side_section];
        let side_id = il.add_route(side);

        // On its own the side signal clears — its section is free.
        il.update(&mut net, 0.05);
        assert_eq!(il.signal(guard_id).aspect.main, Some(MainAspect::Proceed));

        assert!(il.request_route(rid, &mut net));
        il.update(&mut net, 0.05);
        assert_eq!(il.signal(guard_id).aspect.main, Some(MainAspect::Stop));
        assert!(
            !il.signal(guard_id).situation.clear,
            "the rule table sees it"
        );
        assert!(
            !il.request_route(side_id, &mut net),
            "a held signal clears no route"
        );

        // Released, the protection goes with it.
        il.release_route(rid, &mut net);
        il.update(&mut net, 0.05);
        assert_eq!(il.signal(guard_id).flank_locked, 0);
        assert_eq!(il.signal(guard_id).aspect.main, Some(MainAspect::Proceed));
        assert!(il.request_route(side_id, &mut net));
        let _ = node;
    }

    /// The other way round: a signal that another route already runs from
    /// cannot be taken as flank protection — it is showing proceed.
    #[test]
    fn a_signal_with_a_route_cannot_be_taken_as_protection() {
        let (mut net, _node, _e0, e1, e2) = net_with_switch();
        let mut il = Interlock::new();
        let main_section = il.add_section(vec![e1]);
        let side_section = il.add_section(vec![e2]);
        let entry = il.add_signal(Signal::new(SignalId(0), SignalKind::Main, DeviceId(0)));
        let exit = il.add_signal(Signal::new(SignalId(0), SignalKind::Main, DeviceId(1)));
        let guard_id = il.add_signal(Signal::new(SignalId(0), SignalKind::Main, DeviceId(2)));

        let mut side = Route::new(RouteId(0), guard_id, exit);
        side.sections = vec![side_section];
        let side_id = il.add_route(side);
        let mut route = Route::new(RouteId(0), entry, exit);
        route.sections = vec![main_section];
        route.flank = vec![FlankGuard::Signal(guard_id)];
        let rid = il.add_route(route);

        assert!(il.request_route(side_id, &mut net));
        assert!(!il.request_route(rid, &mut net), "protection not available");
        il.release_route(side_id, &mut net);
        assert!(il.request_route(rid, &mut net));
    }

    #[test]
    fn magnet_activation_depends_on_signal_aspect() {
        let mut net = TrackNetwork::new();
        let a = net.add_node(NodeKind::Buffer);
        let b = net.add_node(NodeKind::Buffer);
        let e = net.add_edge(TrackEdge::new(
            EdgeId(0),
            a,
            b,
            to_ecef_deg(52.0, 10.0, 100.0),
            0.0,
            vec![Segment::straight(1000.0)],
        ));
        let mut il = Interlock::new();
        let sec = il.add_section(vec![e]);
        let mut main = Signal::new(SignalId(0), SignalKind::Main, DeviceId(0));
        main.guarded = vec![sec];
        let main_id = il.add_signal(main);
        let mut distant = Signal::new(SignalId(0), SignalKind::Distant, DeviceId(1));
        distant.next = Some(main_id);
        let distant_id = il.add_signal(distant);

        use crate::safety::de::MagnetPayload;
        let magnet_1000 = TracksideDevice::new(DeviceKind::Magnet, e, 100.0)
            .with_payload(&MagnetPayload::hz1000(distant_id.0));
        let magnet_2000 = TracksideDevice::new(DeviceKind::Magnet, e, 900.0)
            .with_payload(&MagnetPayload::hz2000(main_id.0));

        il.update_occupancy(&[(0, e)]);
        il.update(&mut net, 0.05);
        assert!(il.device_active(&magnet_1000), "Vr0 → 1000 Hz active");
        assert!(il.device_active(&magnet_2000), "Hp0 → 2000 Hz active");

        il.update_occupancy(&[]);
        il.update(&mut net, 0.05);
        assert!(!il.device_active(&magnet_1000), "Vr1 → 1000 Hz inactive");
        assert!(!il.device_active(&magnet_2000), "Hp1 → 2000 Hz inactive");
    }
}
