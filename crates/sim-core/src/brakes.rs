//! Druckluftbremse (Plan Kap. 7).
//!
//! Modelliert wird echte Pneumatik, kein Bremskraft-Slider:
//! Hauptluftleitung als Knotenkette entlang des Zuges, je Fahrzeug ein KE-Steuerventil
//! (Dreidrucksystem) mit Steuerkammer, Vorratsluftbehälter und Bremszylinder.

use crate::G;
use crate::train::Train;
use serde::{Deserialize, Serialize};

/// Regelbetriebsdruck der Hauptluftleitung [bar].
pub const PIPE_NOMINAL: f64 = 5.0;
/// Fülldruck beim Füllstoß [bar].
pub const PIPE_OVERCHARGE: f64 = 5.4;
/// Ansprechdruckabsenkung des Steuerventils [bar].
pub const RESPONSE_DROP: f64 = 0.3;
/// Absenkung für Vollbremsung [bar].
pub const FULL_SERVICE_DROP: f64 = 1.5;

/// Bremsstellung (Umstellgriff am Fahrzeug).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BrakePosition {
    /// Güterzug, lange Übergangszeiten.
    G,
    /// Personenzug.
    #[default]
    P,
    /// Schnellbremsstellung, höhere Bremskraft im oberen Geschwindigkeitsbereich.
    R,
    /// R mit Magnetschienenbremse.
    RMg,
}

impl BrakePosition {
    /// Füllzeit des Bremszylinders (0 → 95 %) [s].
    pub fn apply_time(self) -> f64 {
        match self {
            BrakePosition::G => 22.0,
            _ => 4.0,
        }
    }

    /// Lösezeit des Bremszylinders [s].
    pub fn release_time(self) -> f64 {
        match self {
            BrakePosition::G => 50.0,
            _ => 17.0,
        }
    }

    pub fn has_mg(self) -> bool {
        matches!(self, BrakePosition::RMg)
    }

    /// Kraftzuschlag im R-Bereich oberhalb 60 km/h.
    pub fn high_speed_factor(self, v_kmh: f64) -> f64 {
        match self {
            BrakePosition::R | BrakePosition::RMg if v_kmh > 60.0 => 1.35,
            _ => 1.0,
        }
    }
}

/// Bremsbauart — bestimmt den Reibwertverlauf über der Geschwindigkeit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BrakeKind {
    /// Klotzbremse (Grauguss): Reibwert fällt stark mit der Geschwindigkeit.
    #[default]
    Block,
    /// Scheibenbremse: nahezu konstanter Reibwert.
    Disc,
}

impl BrakeKind {
    /// Reibwertfaktor bezogen auf Stillstand.
    /// ponytail: zwei glatte Kurven statt Karwatzki-Tabellen — genügt für Bremswege
    /// im Prozentbereich; echte Belagkennfelder je Bauart nachrüsten, wenn
    /// Bremstafel-Feinabgleich ansteht.
    pub fn friction_factor(self, v_kmh: f64) -> f64 {
        let v = v_kmh.abs();
        match self {
            BrakeKind::Block => 1.0 / (1.0 + 0.011 * v),
            BrakeKind::Disc => 1.0 / (1.0 + 0.003 * v),
        }
    }
}

/// Bremsausrüstung eines Fahrzeugs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrakeSpec {
    pub kind: BrakeKind,
    pub position: BrakePosition,
    /// Bremsgewicht [t] — Grundlage der Bremshundertstel.
    pub brake_weight: f64,
    /// Bremskraft bei vollem Bremszylinderdruck und Stillstand [N].
    pub max_force: f64,
    /// Höchster Bremszylinderdruck [bar].
    pub max_cylinder: f64,
    /// Volumenverhältnis Bremszylinder / Vorratsluftbehälter (Erschöpfbarkeit).
    pub cylinder_to_reservoir: f64,
    /// Magnetschienenbremse vorhanden.
    #[serde(default)]
    pub has_mg: bool,
    /// Kraft der Magnetschienenbremse [N].
    #[serde(default)]
    pub mg_force: f64,
    /// Zusatzbremse (direkte Bremse) vorhanden — nur Triebfahrzeuge.
    #[serde(default)]
    pub has_direct: bool,
    /// Federspeicher-/Handbremskraft [N].
    #[serde(default)]
    pub parking_force: f64,
}

impl BrakeSpec {
    /// Bremsausrüstung aus dem Bremsgewicht ableiten.
    ///
    /// Der Faktor ist gegen die Bremstafel kalibriert: ein Zug mit 100 Bremshundertsteln
    /// kommt aus 100 km/h mit Schnellbremsung in der Größenordnung 500 m zum Stehen
    /// (siehe Test `schnellbremsung_aus_100_kmh`).
    pub fn from_brake_weight(brake_weight_t: f64, kind: BrakeKind) -> Self {
        Self {
            kind,
            position: BrakePosition::P,
            brake_weight: brake_weight_t,
            max_force: brake_weight_t * 1000.0 * G * 0.145,
            max_cylinder: 3.8,
            cylinder_to_reservoir: 0.35,
            has_mg: false,
            mg_force: 0.0,
            has_direct: false,
            parking_force: 0.0,
        }
    }

    pub fn with_position(mut self, position: BrakePosition) -> Self {
        self.position = position;
        self
    }

    pub fn with_direct_brake(mut self) -> Self {
        self.has_direct = true;
        self
    }

    pub fn with_mg(mut self, force: f64) -> Self {
        self.has_mg = true;
        self.mg_force = force;
        self.position = BrakePosition::RMg;
        self
    }
}

/// Laufzeitzustand der Bremse eines Fahrzeugs. Alle Drücke in bar (Überdruck).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BrakeState {
    /// Hauptluftleitung an diesem Fahrzeug.
    pub pipe: f64,
    /// Steuerkammer (Referenzdruck des Steuerventils).
    pub control_reservoir: f64,
    /// Vorratsluftbehälter (R-Behälter).
    pub aux_reservoir: f64,
    /// Bremszylinderdruck aus der selbsttätigen Bremse.
    pub cylinder: f64,
    /// Bremszylinderdruck aus der Zusatzbremse (direkte Bremse).
    pub direct_cylinder: f64,
    /// Hauptluftbehälter (nur Triebfahrzeuge).
    pub main_reservoir: f64,
    /// Magnetschienenbremse angelegt.
    pub mg_applied: bool,
    /// Federspeicher-/Handbremse angelegt.
    pub parking_applied: bool,
    /// Aktuelle Bremskraft [N] (Ausgabe an die Längsdynamik).
    pub force: f64,
}

impl BrakeState {
    pub fn new(spec: &BrakeSpec) -> Self {
        let _ = spec;
        Self {
            pipe: PIPE_NOMINAL,
            control_reservoir: PIPE_NOMINAL,
            aux_reservoir: PIPE_NOMINAL,
            cylinder: 0.0,
            direct_cylinder: 0.0,
            main_reservoir: 9.0,
            mg_applied: false,
            parking_applied: false,
            force: 0.0,
        }
    }

    /// Gelöst?
    pub fn released(&self) -> bool {
        self.cylinder < 0.15 && self.direct_cylinder < 0.15
    }
}

/// Stellung des Führerbremsventils.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum DriverBrakeValve {
    /// Füllstellung (Füllstoß, HL über Regeldruck).
    Fill,
    /// Fahrtstellung — HL auf Regeldruck, angeglichen.
    Release,
    /// Abschlussstellung — keine Verbindung, Druck bleibt stehen.
    Lap,
    /// Betriebsbremsung mit Druckabsenkung [bar] gegenüber Regeldruck.
    Service(f64),
    /// Schnellbremsung.
    Emergency,
}

impl DriverBrakeValve {
    /// Solldruck der Hauptluftleitung am Führerbremsventil [bar].
    pub fn target_pressure(self) -> Option<f64> {
        match self {
            DriverBrakeValve::Fill => Some(PIPE_OVERCHARGE),
            DriverBrakeValve::Release => Some(PIPE_NOMINAL),
            DriverBrakeValve::Lap => None,
            DriverBrakeValve::Service(drop) => {
                Some((PIPE_NOMINAL - drop.clamp(0.0, FULL_SERVICE_DROP)).max(3.4))
            }
            DriverBrakeValve::Emergency => Some(0.0),
        }
    }

    /// Fluss zum Sollwert [bar/s]: Füllen langsamer als Entlüften, Schnellbremsung sehr schnell.
    pub fn flow_rate(self) -> f64 {
        match self {
            DriverBrakeValve::Fill => 1.2,
            DriverBrakeValve::Release => 0.5,
            DriverBrakeValve::Lap => 0.0,
            DriverBrakeValve::Service(_) => 0.6,
            DriverBrakeValve::Emergency => 6.0,
        }
    }
}

/// Leitwert zwischen zwei benachbarten Fahrzeugen [1/s].
///
/// ponytail: Knotenmodell statt Rohr-PDE (Plan 7). Die Druckabsenkung läuft dadurch
/// diffusiv statt als Welle nach hinten — Reihenfolge und Verzögerung stimmen
/// qualitativ (langer Güterzug bremst hinten später), die exakte
/// Durchschlagsgeschwindigkeit nicht. Upgrade-Pfad: Charakteristikenverfahren je Rohrabschnitt.
pub const PIPE_CONDUCTANCE: f64 = 6.0;

/// Ein Simulationsschritt der gesamten Bremsanlage eines Zuges.
pub fn step(train: &mut Train, valve: DriverBrakeValve, direct: f64, dt: f64) {
    update_pipe(train, valve, dt);
    let cab = train.cab;
    let v_kmh = train.speed_kmh().abs();
    for (i, veh) in train.vehicles.iter_mut().enumerate() {
        update_control_valve(&mut veh.brake, &veh.spec.brake, dt);
        if veh.spec.brake.has_direct && i == cab {
            let target = direct.clamp(0.0, 1.0) * veh.spec.brake.max_cylinder;
            approach(&mut veh.brake.direct_cylinder, target, 2.0, dt);
        }
        veh.brake.mg_applied = veh.spec.brake.has_mg
            && veh.spec.brake.position.has_mg()
            && v_kmh > 50.0
            && veh.brake.pipe < PIPE_NOMINAL - 1.0;
        veh.brake.force = brake_force(&veh.spec.brake, &veh.brake, v_kmh);
    }
}

/// Druckausgleich in der Hauptluftleitung inklusive Führerbremsventil.
fn update_pipe(train: &mut Train, valve: DriverBrakeValve, dt: f64) {
    let n = train.vehicles.len();
    let pressures: Vec<f64> = train.vehicles.iter().map(|v| v.brake.pipe).collect();
    for i in 0..n {
        let mut flow = 0.0;
        if i > 0 {
            flow += PIPE_CONDUCTANCE * (pressures[i - 1] - pressures[i]);
        }
        if i + 1 < n {
            flow += PIPE_CONDUCTANCE * (pressures[i + 1] - pressures[i]);
        }
        // Verbrauch durch das Steuerventil beim Nachspeisen des Vorratsbehälters.
        let veh = &train.vehicles[i];
        if veh.brake.aux_reservoir < veh.brake.pipe {
            flow -= 0.15 * (veh.brake.pipe - veh.brake.aux_reservoir);
        }
        let p = &mut train.vehicles[i].brake.pipe;
        *p = (*p + flow * dt).clamp(0.0, PIPE_OVERCHARGE);
    }
    // Führerbremsventil wirkt am besetzten Führerstand.
    if let Some(target) = valve.target_pressure() {
        let cab = train.cab.min(n.saturating_sub(1));
        let p = &mut train.vehicles[cab].brake.pipe;
        approach(p, target, valve.flow_rate(), dt);
    }
}

/// KE-Steuerventil: Dreidrucksystem mit Steuerkammer, Vorratsbehälter und Bremszylinder.
fn update_control_valve(state: &mut BrakeState, spec: &BrakeSpec, dt: f64) {
    // Steuerkammer folgt der HL nur beim Lösen/Füllen (und nie über Regeldruck hinaus,
    // sonst würde der Füllstoß die Bremse „überladen").
    if state.pipe >= state.control_reservoir {
        approach(
            &mut state.control_reservoir,
            state.pipe.min(PIPE_NOMINAL),
            0.35,
            dt,
        );
    }
    // Vorratsbehälter wird aus der HL nachgespeist.
    if state.pipe > state.aux_reservoir {
        approach(&mut state.aux_reservoir, state.pipe, 0.15, dt);
    }

    let drop = state.control_reservoir - state.pipe;
    let target = if drop <= RESPONSE_DROP {
        0.0
    } else {
        // Voller Zylinderdruck bei Vollbremsungsabsenkung.
        let ratio = spec.max_cylinder / (FULL_SERVICE_DROP - RESPONSE_DROP);
        ((drop - RESPONSE_DROP) * ratio).min(spec.max_cylinder)
    };
    // Erschöpfbarkeit: der Zylinder kann nie über den Vorratsbehälter hinaus gefüllt werden.
    let target = target.min(state.aux_reservoir);

    let rate = if target > state.cylinder {
        // 0 → 95 % in apply_time.
        spec.max_cylinder / spec.position.apply_time() * 3.0
    } else {
        spec.max_cylinder / spec.position.release_time() * 3.0
    };
    let before = state.cylinder;
    approach(&mut state.cylinder, target, rate, dt);
    // Luftverbrauch aus dem Vorratsbehälter.
    let delta = state.cylinder - before;
    if delta > 0.0 {
        state.aux_reservoir = (state.aux_reservoir - delta * spec.cylinder_to_reservoir).max(0.0);
    }
}

/// Bremskraft eines Fahrzeugs [N].
fn brake_force(spec: &BrakeSpec, state: &BrakeState, v_kmh: f64) -> f64 {
    let cylinder = state.cylinder.max(state.direct_cylinder);
    let mut f = cylinder / spec.max_cylinder
        * spec.max_force
        * spec.kind.friction_factor(v_kmh)
        * spec.position.high_speed_factor(v_kmh);
    if state.mg_applied {
        f += spec.mg_force;
    }
    if state.parking_applied {
        f += spec.parking_force;
    }
    f.max(0.0)
}

/// Bewegt `value` mit maximaler Rate `rate` [Einheit/s] auf `target` zu.
pub(crate) fn approach(value: &mut f64, target: f64, rate: f64, dt: f64) {
    let max_step = rate * dt;
    let diff = target - *value;
    *value += diff.clamp(-max_step, max_step);
}
