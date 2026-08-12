//! Signale, Fahrstraßen und Blocksicherung (Plan Kap. 10).
//!
//! Länderneutral: ein Signal ist ein Zustandsautomat mit Begriffen; welche Lampenbilder
//! ein Begriff im Ks- oder H/V-System hat, entscheidet die Darstellung im Länderpaket.

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

/// Signalsystem (bestimmt nur die Darstellung, nicht die Logik).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignalSystem {
    /// Haupt-/Vorsignalsystem (H/V).
    HV,
    /// Kombinationssignalsystem (Ks).
    Ks,
    /// Hl-System (Ostnetz) — v2.
    Hl,
}

/// Bauart des Signals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignalKind {
    /// Hauptsignal.
    Main,
    /// Vorsignal (zeigt nur den Begriff des zugehörigen Hauptsignals an).
    Distant,
    /// Kombinationssignal (Ks): Haupt- und Vorsignalfunktion in einem Schirm.
    Combined,
    /// Sperrsignal (Sh1/Ra12).
    Shunting,
}

/// Hauptsignalbegriff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MainAspect {
    /// Hp0 / Ks-Halt.
    #[default]
    Stop,
    /// Hp1 / Ks1 — Fahrt.
    Proceed,
    /// Hp2 / Ks2 mit Zs3 — Langsamfahrt (Ablenkung).
    ProceedSlow,
    /// Zs1/Zs7 — Ersatzsignal.
    Substitute,
    /// Kennlicht — Signal ungültig.
    DarkLight,
}

impl MainAspect {
    pub fn is_stop(self) -> bool {
        matches!(self, MainAspect::Stop | MainAspect::DarkLight)
    }
}

/// Vorsignalbegriff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DistantAspect {
    /// Vr0 / Ks2 — Halt erwarten.
    #[default]
    ExpectStop,
    /// Vr1 / Ks1 — Fahrt erwarten.
    ExpectProceed,
    /// Vr2 — Langsamfahrt erwarten.
    ExpectSlow,
}

impl DistantAspect {
    /// Wirkt der 1000-Hz-Magnet? (Bei Vr0 und Vr2, nicht bei Vr1.)
    pub fn is_restrictive(self) -> bool {
        !matches!(self, DistantAspect::ExpectProceed)
    }
}

/// Vollständiger Signalbegriff.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Aspect {
    pub main: Option<MainAspect>,
    pub distant: Option<DistantAspect>,
    /// Zs3/Zs3v-Geschwindigkeitsanzeiger [km/h].
    pub speed: Option<f64>,
}

impl Aspect {
    pub fn stop() -> Self {
        Self {
            main: Some(MainAspect::Stop),
            distant: None,
            speed: None,
        }
    }

    pub fn is_stop(&self) -> bool {
        self.main.is_some_and(MainAspect::is_stop)
    }

    /// Kündigt das Signal eine Einschränkung an? (Grundlage der 1000-Hz-Wirksamkeit.)
    pub fn announces_restriction(&self) -> bool {
        self.distant.is_some_and(DistantAspect::is_restrictive)
    }
}

/// Gleisfreimeldeabschnitt (Achszählerabschnitt).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackSection {
    pub id: SectionId,
    pub edges: Vec<EdgeId>,
    pub occupied: bool,
    /// Durch eine Fahrstraße festgelegt.
    pub locked_by: Option<RouteId>,
}

/// Ein Signal der Strecke.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signal {
    pub id: SignalId,
    pub system: SignalSystem,
    pub kind: SignalKind,
    /// Zugehöriges Streckengerät (Position im Gleisnetz).
    pub device: track_model::DeviceId,
    /// Folgendes Hauptsignal — für die Vorsignalisierung.
    pub next: Option<SignalId>,
    /// Abschnitte, die frei sein müssen, damit das Signal Fahrt zeigen darf.
    pub guarded: Vec<SectionId>,
    /// Signal zeigt nur mit gestellter Fahrstraße Fahrt (Stellwerkssignal);
    /// sonst Selbstblocksignal.
    pub requires_route: bool,
    /// Geschwindigkeit bei abzweigender Fahrt [km/h] (Zs3).
    pub diverging_speed: Option<f64>,
    /// Aktueller Begriff.
    pub aspect: Aspect,
    /// Zugelassene Fahrstraße.
    pub route: Option<RouteId>,
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
            // Grundstellung: Hauptsignale zeigen Halt, Vorsignale „Halt erwarten".
            aspect: match kind {
                SignalKind::Distant => Aspect {
                    main: None,
                    distant: Some(DistantAspect::ExpectStop),
                    speed: None,
                },
                _ => Aspect::stop(),
            },
            route: None,
        }
    }
}

/// Zustand einer Fahrstraße.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RouteState {
    #[default]
    Free,
    /// Angefordert — Weichen laufen um.
    Requested,
    /// Festgelegt (Weichen verschlossen, Signal darf Fahrt zeigen).
    Locked,
    /// Zug in der Fahrstraße.
    Occupied,
}

/// Eine Fahrstraße von Start- zu Zielsignal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    pub id: RouteId,
    pub entry: SignalId,
    pub exit: SignalId,
    /// Sollagen der Weichen im Fahrweg.
    pub switches: Vec<(NodeId, SwitchPosition)>,
    /// Abschnitte des Fahrwegs, in Fahrtrichtung.
    pub sections: Vec<SectionId>,
    /// Durchrutschweg hinter dem Zielsignal.
    pub overlap: Vec<SectionId>,
    /// Fahrstraße führt über einen abzweigenden Weg (Langsamfahrt).
    pub diverging: bool,
    pub state: RouteState,
}

impl Route {
    pub fn new(id: RouteId, entry: SignalId, exit: SignalId) -> Self {
        Self {
            id,
            entry,
            exit,
            switches: Vec::new(),
            sections: Vec::new(),
            overlap: Vec::new(),
            diverging: false,
            state: RouteState::Free,
        }
    }
}

/// Wirksamkeitsbedingung eines signalabhängigen Streckengeräts.
///
/// Länderneutral: das Gerät sagt selbst, wann es wirkt; das Stellwerk kennt nur den
/// Signalbegriff. So bleiben PZB-Magnete DE-Sache, die Verknüpfung aber allgemein.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Activation {
    /// Immer wirksam.
    #[default]
    Always,
    /// Wirksam, wenn das zugehörige Signal Halt zeigt (500/2000 Hz).
    WhenStop,
    /// Wirksam, wenn das zugehörige Signal eine Einschränkung ankündigt (1000 Hz).
    WhenRestrictive,
}

/// Neutraler Teil eines Geräte-Payloads: Signalbezug und Wirksamkeit.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct DeviceLink {
    #[serde(default)]
    pub signal: Option<u32>,
    #[serde(default)]
    pub activation: Activation,
}

/// Das Stellwerk.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Interlock {
    pub signals: Vec<Signal>,
    pub sections: Vec<TrackSection>,
    pub routes: Vec<Route>,
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

    /// Gleisfreimeldung: welche Abschnitte sind von Fahrzeugen besetzt?
    pub fn update_occupancy(&mut self, occupied_edges: &[EdgeId]) {
        for s in &mut self.sections {
            s.occupied = s.edges.iter().any(|e| occupied_edges.contains(e));
        }
    }

    /// Fahrstraße anfordern (Zuglenkung oder Fdl).
    pub fn request_route(&mut self, id: RouteId, net: &mut TrackNetwork) -> bool {
        let route = &self.routes[id.index()];
        if route.state != RouteState::Free {
            return route.state == RouteState::Locked;
        }
        // Kein Abschnitt darf besetzt oder anderweitig festgelegt sein.
        let blocked = route.sections.iter().chain(route.overlap.iter()).any(|s| {
            let sec = &self.sections[s.index()];
            sec.occupied || sec.locked_by.is_some_and(|r| r != id)
        });
        if blocked {
            return false;
        }
        // Weichen umstellen.
        let switches = route.switches.clone();
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

    /// Fahrstraße auflösen (nach Zugfahrt oder Rücknahme).
    pub fn release_route(&mut self, id: RouteId, net: &mut TrackNetwork) {
        let route = &self.routes[id.index()];
        let switches = route.switches.clone();
        let sections: Vec<SectionId> = route
            .sections
            .iter()
            .chain(route.overlap.iter())
            .copied()
            .collect();
        for (node, _) in switches {
            if let Some(sw) = net.switch_mut(node) {
                sw.locked = false;
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

    /// Ein Schritt der Stellwerkslogik: Fahrstraßen festlegen/auflösen, Signale stellen.
    pub fn update(&mut self, net: &mut TrackNetwork) {
        self.update_routes(net);
        self.update_signals();
    }

    fn update_routes(&mut self, net: &mut TrackNetwork) {
        for i in 0..self.routes.len() {
            match self.routes[i].state {
                RouteState::Requested => {
                    // Festlegen, sobald alle Weichen in Lage sind.
                    let ready = self.routes[i].switches.iter().all(|(node, pos)| {
                        net.switch(*node)
                            .is_none_or(|sw| !sw.is_moving() && sw.position == *pos && !sw.trailed)
                    });
                    if ready {
                        let switches = self.routes[i].switches.clone();
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
                    // Zug hat die Fahrstraße befahren?
                    if self.routes[i]
                        .sections
                        .iter()
                        .any(|s| self.sections[s.index()].occupied)
                    {
                        self.routes[i].state = RouteState::Occupied;
                        // Signal fällt hinter dem Zug auf Halt.
                        let entry = self.routes[i].entry;
                        self.signals[entry.index()].route = None;
                    }
                }
                RouteState::Occupied => {
                    // Auflösen, wenn der Zug den Fahrweg vollständig geräumt hat.
                    let cleared = self.routes[i]
                        .sections
                        .iter()
                        .all(|s| !self.sections[s.index()].occupied);
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
        // 1. Hauptsignalbegriffe.
        for i in 0..self.signals.len() {
            let sig = &self.signals[i];
            if sig.kind == SignalKind::Distant {
                continue;
            }
            let free = sig
                .guarded
                .iter()
                .all(|s| !self.sections[s.index()].occupied);
            let route_ok = !sig.requires_route || sig.route.is_some();
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
            let sig = &mut self.signals[i];
            sig.aspect.main = Some(main);
            sig.aspect.speed = speed;
        }

        // 2. Vorsignalisierung aus dem folgenden Hauptsignal.
        for i in 0..self.signals.len() {
            let Some(next) = self.signals[i].next else {
                if self.signals[i].kind != SignalKind::Main {
                    self.signals[i].aspect.distant = None;
                }
                continue;
            };
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
        }
    }

    /// Signal zu einem Streckengerät (falls es eins ist).
    pub fn signal_at_device(&self, device: track_model::DeviceId) -> Option<&Signal> {
        self.signals.iter().find(|s| s.device == device)
    }

    /// Ist ein signalabhängiges Streckengerät gerade wirksam?
    ///
    /// Grundlage der PZB-Magnetwirksamkeit: 1000 Hz bei angekündigter Einschränkung,
    /// 500/2000 Hz bei Halt zeigendem Signal.
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
                    Activation::WhenStop => sig.aspect.is_stop(),
                    Activation::WhenRestrictive => sig.aspect.announces_restriction(),
                    Activation::Always => true,
                }
            }
        }
    }

    /// Signalbegriff eines Signals als Geschwindigkeitsvorgabe [km/h], falls einschränkend.
    pub fn signal_speed(&self, id: SignalId) -> Option<f64> {
        let s = self.signal(id);
        match s.aspect.main? {
            MainAspect::Stop | MainAspect::DarkLight => Some(0.0),
            MainAspect::ProceedSlow => s.aspect.speed.or(Some(40.0)),
            MainAspect::Substitute => Some(40.0),
            MainAspect::Proceed => None,
        }
    }
}

/// Hilfsfunktion für Content: Payload eines signalabhängigen Geräts prüfen.
pub fn is_signal_device(kind: &DeviceKind) -> bool {
    matches!(
        kind,
        DeviceKind::Magnet | DeviceKind::Signal | DeviceKind::LineConductor
    )
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
    fn selbstblock_faellt_hinter_zug_auf_halt() {
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
        il.update(&mut net);
        assert_eq!(il.signal(sid).aspect.main, Some(MainAspect::Proceed));

        il.update_occupancy(&[e]);
        il.update(&mut net);
        assert_eq!(il.signal(sid).aspect.main, Some(MainAspect::Stop));
    }

    #[test]
    fn vorsignal_folgt_hauptsignal() {
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

        il.update_occupancy(&[e]);
        il.update(&mut net);
        assert_eq!(
            il.signal(distant_id).aspect.distant,
            Some(DistantAspect::ExpectStop)
        );
        assert!(il.signal(distant_id).aspect.announces_restriction());

        il.update_occupancy(&[]);
        il.update(&mut net);
        assert_eq!(
            il.signal(distant_id).aspect.distant,
            Some(DistantAspect::ExpectProceed)
        );
        assert!(!il.signal(distant_id).aspect.announces_restriction());
    }

    #[test]
    fn fahrstrasse_stellt_weiche_verschliesst_und_loest_auf() {
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

        // Ohne Fahrstraße: Halt.
        il.update(&mut net);
        assert_eq!(il.signal(sid).aspect.main, Some(MainAspect::Stop));

        assert!(il.request_route(rid, &mut net));
        il.update(&mut net);
        assert_eq!(
            il.route(rid).state,
            RouteState::Requested,
            "Weiche läuft um"
        );
        net.update_switches(10.0);
        il.update(&mut net);
        assert_eq!(il.route(rid).state, RouteState::Locked);
        assert!(net.switch(node).unwrap().locked, "Weiche verschlossen");
        assert_eq!(il.signal(sid).aspect.main, Some(MainAspect::ProceedSlow));
        assert_eq!(il.signal(sid).aspect.speed, Some(40.0));

        // Zug fährt ein → Signal auf Halt, Fahrstraße besetzt.
        il.update_occupancy(&[e2]);
        il.update(&mut net);
        assert_eq!(il.route(rid).state, RouteState::Occupied);
        assert_eq!(il.signal(sid).aspect.main, Some(MainAspect::Stop));

        // Zug räumt → Auflösung, Weiche wieder frei.
        il.update_occupancy(&[]);
        il.update(&mut net);
        assert_eq!(il.route(rid).state, RouteState::Free);
        assert!(!net.switch(node).unwrap().locked);
        let _ = s_entry;
    }

    #[test]
    fn belegter_fahrweg_verhindert_fahrstrasse() {
        let (mut net, node, _e0, _e1, e2) = net_with_switch();
        let mut il = Interlock::new();
        let s_exit = il.add_section(vec![e2]);
        let sid = il.add_signal(Signal::new(SignalId(0), SignalKind::Main, DeviceId(0)));
        let exit = il.add_signal(Signal::new(SignalId(0), SignalKind::Main, DeviceId(1)));
        let mut route = Route::new(RouteId(0), sid, exit);
        route.switches = vec![(node, SwitchPosition::Diverging)];
        route.sections = vec![s_exit];
        let rid = il.add_route(route);

        il.update_occupancy(&[e2]);
        assert!(!il.request_route(rid, &mut net));
        assert_eq!(il.route(rid).state, RouteState::Free);
    }

    #[test]
    fn magnetwirksamkeit_haengt_am_signalbegriff() {
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

        il.update_occupancy(&[e]);
        il.update(&mut net);
        assert!(il.device_active(&magnet_1000), "Vr0 → 1000 Hz wirksam");
        assert!(il.device_active(&magnet_2000), "Hp0 → 2000 Hz wirksam");

        il.update_occupancy(&[]);
        il.update(&mut net);
        assert!(!il.device_active(&magnet_1000), "Vr1 → 1000 Hz unwirksam");
        assert!(!il.device_active(&magnet_2000), "Hp1 → 2000 Hz unwirksam");
    }
}
