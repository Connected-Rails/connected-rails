//! Elektrik, Antrieb und Aufrüstprozedur (Plan Kap. 8).
//!
//! Kein SPICE: ein gerichteter Zustandsgraph aus Schaltern und Verbrauchern. Was zählt,
//! ist die Reihenfolge (Batterie → Stromabnehmer → Hauptschalter → Hilfsbetriebe) und die
//! Kennlinie des Traktionsstrangs.

use crate::brakes::approach;
use serde::{Deserialize, Serialize};

/// Nennspannung des deutschen Bahnstromnetzes [V].
pub const NOMINAL_LINE_VOLTAGE: f64 = 15_000.0;
/// Ab dieser Fahrdrahtspannung darf der Hauptschalter einschalten [V].
pub const MIN_LINE_VOLTAGE: f64 = 10_000.0;

/// Traktionsstrang eines Triebfahrzeugs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TractionSpec {
    /// Trafo mit Schaltwerk (Altbau-E-Lok, z. B. BR 110/140).
    TapChanger {
        /// Anzahl Fahrstufen.
        steps: u32,
        /// Anfahrzugkraft [N].
        max_force: f64,
        /// Dauerleistung am Rad [W].
        max_power: f64,
        /// Höchstgeschwindigkeit [km/h].
        v_max: f64,
        /// Zeit je Schaltstufe [s].
        step_time: f64,
    },
    /// Drehstromantrieb mit Umrichter (BR 101/185/423, ICE).
    Converter {
        max_force: f64,
        max_power: f64,
        v_max: f64,
        /// Höchste elektrische Bremskraft [N].
        brake_force: f64,
        /// Leistung der elektrischen Bremse [W].
        brake_power: f64,
        /// Anstiegszeit von 0 auf volle Kraft [s].
        ramp_time: f64,
    },
    /// Dieselantrieb (BR 218 hydraulisch, BR 648).
    Diesel {
        max_force: f64,
        max_power: f64,
        v_max: f64,
        /// Zeit vom Leerlauf auf Volllast [s].
        ramp_time: f64,
        /// Anlasszeit des Motors [s].
        start_time: f64,
    },
}

impl TractionSpec {
    pub fn v_max(&self) -> f64 {
        match self {
            TractionSpec::TapChanger { v_max, .. }
            | TractionSpec::Converter { v_max, .. }
            | TractionSpec::Diesel { v_max, .. } => *v_max,
        }
    }

    /// Verfügbare Zugkraft bei Geschwindigkeit `v` [m/s] — Zugkraft-Hyperbel.
    pub fn available_force(&self, v: f64) -> f64 {
        let (max_force, max_power, v_max) = match self {
            TractionSpec::TapChanger {
                max_force,
                max_power,
                v_max,
                ..
            }
            | TractionSpec::Converter {
                max_force,
                max_power,
                v_max,
                ..
            }
            | TractionSpec::Diesel {
                max_force,
                max_power,
                v_max,
                ..
            } => (*max_force, *max_power, *v_max),
        };
        let av = v.abs();
        if av > v_max / 3.6 {
            return 0.0;
        }
        // Unterhalb der Übergangsgeschwindigkeit konstante Kraft, darüber konstante Leistung.
        max_force.min(max_power / av.max(0.5))
    }

    /// Verfügbare elektrische Bremskraft bei `v` [m/s].
    pub fn available_brake_force(&self, v: f64) -> f64 {
        match self {
            TractionSpec::Converter {
                brake_force,
                brake_power,
                ..
            } => brake_force.min(brake_power / v.abs().max(0.5)),
            // Schaltwerkloks der Baureihe 110 haben keine E-Bremse, Diesel v1 auch nicht.
            _ => 0.0,
        }
    }
}

/// Zustand von Bordnetz und Antrieb eines Fahrzeugs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TractionState {
    pub battery: bool,
    /// Befehl „Stromabnehmer heben".
    pub pantograph_command: bool,
    /// Hubzustand des Stromabnehmers 0…1 (Laufzeit ~ 5 s).
    pub pantograph: f64,
    /// Befehl „Hauptschalter ein".
    pub main_switch_command: bool,
    pub main_switch: bool,
    /// Fahrdrahtspannung am Stromabnehmer [V] — von der Strecke gesetzt
    /// (0 in Schutzstrecken oder ohne Oberleitung).
    pub line_voltage: f64,
    /// Fahrschalter: −1 … +1 (negativ = elektrische Bremse).
    pub notch: f64,
    /// Aktuelle Schaltwerkstufe (nur `TapChanger`).
    pub step: f64,
    /// Dieselmotor läuft.
    pub engine_running: bool,
    /// Anlasser-Restzeit [s].
    pub start_timer: f64,
    /// Luftpresser eingeschaltet.
    pub compressor: bool,
    /// Zugsammelschiene (Heizung) eingeschaltet.
    pub train_line: bool,
    /// Aktuelle Zugkraft [N], positiv = Traktion, negativ = elektrische Bremse.
    pub force: f64,
}

impl Default for TractionState {
    fn default() -> Self {
        Self {
            battery: false,
            pantograph_command: false,
            pantograph: 0.0,
            main_switch_command: false,
            main_switch: false,
            line_voltage: 0.0,
            notch: 0.0,
            step: 0.0,
            engine_running: false,
            start_timer: 0.0,
            compressor: false,
            train_line: false,
            force: 0.0,
        }
    }
}

impl TractionState {
    /// Aufgerüstet und fahrbereit?
    pub fn ready(&self) -> bool {
        self.battery && (self.main_switch || self.engine_running)
    }
}

/// Ein Simulationsschritt für Bordnetz und Antrieb eines Fahrzeugs.
pub fn step(state: &mut TractionState, spec: &TractionSpec, v: f64, dt: f64) {
    update_power(state, spec, dt);

    let electric_ok = state.main_switch && state.line_voltage >= MIN_LINE_VOLTAGE;
    let powered = match spec {
        TractionSpec::Diesel { .. } => state.engine_running,
        _ => electric_ok,
    };

    if !powered {
        // Kraft fällt ab, Schaltwerk läuft in die Nullstellung zurück.
        approach(&mut state.force, 0.0, 1.0e6, dt);
        approach(&mut state.step, 0.0, 5.0, dt);
        return;
    }

    let notch = state.notch.clamp(-1.0, 1.0);
    match spec {
        TractionSpec::TapChanger {
            steps, step_time, ..
        } => {
            // Das Schaltwerk läuft Stufe für Stufe — der Fahrschalter gibt nur das Ziel vor.
            let target = notch.max(0.0) * *steps as f64;
            approach(&mut state.step, target, 1.0 / step_time.max(0.01), dt);
            state.force = state.step / *steps as f64 * spec.available_force(v);
        }
        TractionSpec::Converter { ramp_time, .. } => {
            let target = if notch >= 0.0 {
                notch * spec.available_force(v)
            } else {
                notch * spec.available_brake_force(v)
            };
            let rate = spec.available_force(v).max(1.0) / ramp_time.max(0.1);
            approach(&mut state.force, target, rate, dt);
        }
        TractionSpec::Diesel { ramp_time, .. } => {
            let target = notch.max(0.0) * spec.available_force(v);
            let rate = spec.available_force(v).max(1.0) / ramp_time.max(0.1);
            approach(&mut state.force, target, rate, dt);
        }
    }
}

/// Aufrüstkette: Batterie → Stromabnehmer → Hauptschalter (Plan 8, Aufrüstprozedur).
fn update_power(state: &mut TractionState, spec: &TractionSpec, dt: f64) {
    if !state.battery {
        state.pantograph_command = false;
        state.main_switch_command = false;
        state.compressor = false;
    }

    // Stromabnehmer braucht ~ 5 s zum Heben und ~ 3 s zum Senken.
    let target = if state.pantograph_command && state.battery {
        1.0
    } else {
        0.0
    };
    let rate = if target > state.pantograph {
        1.0 / 5.0
    } else {
        1.0 / 3.0
    };
    approach(&mut state.pantograph, target, rate, dt);

    // Hauptschalter: nur mit anliegender Fahrdrahtspannung, fällt bei Spannungsverlust ab
    // (Schutzstrecke!).
    let contact = state.pantograph > 0.98;
    let voltage_ok = contact && state.line_voltage >= MIN_LINE_VOLTAGE;
    state.main_switch = state.main_switch_command && state.battery && voltage_ok;

    if let TractionSpec::Diesel { start_time, .. } = spec {
        if state.start_timer > 0.0 {
            state.start_timer -= dt;
            if state.start_timer <= 0.0 {
                state.engine_running = true;
                state.start_timer = 0.0;
            }
        }
        let _ = start_time;
    }
}

/// Dieselmotor anlassen (braucht Batterie).
pub fn start_engine(state: &mut TractionState, spec: &TractionSpec) {
    if let TractionSpec::Diesel { start_time, .. } = spec
        && state.battery
        && !state.engine_running
        && state.start_timer <= 0.0
    {
        state.start_timer = *start_time;
    }
}
