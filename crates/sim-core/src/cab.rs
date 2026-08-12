//! Bedienelemente des Führerstands als Sim-Eingang (Plan Kap. 12).

use crate::brakes::DriverBrakeValve;
use serde::{Deserialize, Serialize};

/// Flankenerkennung für Taster.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    last: bool,
}

impl Edge {
    /// `true` genau in dem Schritt, in dem der Taster gedrückt wird.
    pub fn rising(&mut self, now: bool) -> bool {
        let r = now && !self.last;
        self.last = now;
        r
    }

    /// `true` genau in dem Schritt, in dem der Taster losgelassen wird.
    pub fn falling(&mut self, now: bool) -> bool {
        let f = !now && self.last;
        self.last = now;
        f
    }

    pub fn held(&self) -> bool {
        self.last
    }
}

/// Alle Stellwerte eines Führerstands. Taster sind als „gerade gedrückt" zu verstehen;
/// die Systeme werten Flanken selbst aus.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CabInputs {
    /// Richtungswender: −1 rückwärts, 0 aus, +1 vorwärts.
    pub reverser: i8,
    /// Fahrschalter −1 … +1 (negativ = elektrische Bremse).
    pub throttle: f64,
    pub brake_valve: DriverBrakeValve,
    /// Zusatzbremse 0 … 1.
    pub direct_brake: f64,
    pub sanding: bool,
    /// Sifa-Pedal/-Taster.
    pub sifa: bool,
    pub pzb_wachsam: bool,
    pub pzb_frei: bool,
    pub pzb_befehl: bool,
    pub lzb_uebernahme: bool,
    pub lzb_ende: bool,
    pub horn: bool,
    /// AFB eingeschaltet.
    pub afb: bool,
    /// AFB-Sollgeschwindigkeit [km/h].
    pub afb_target: f64,
}

impl Default for CabInputs {
    fn default() -> Self {
        Self {
            reverser: 0,
            throttle: 0.0,
            brake_valve: DriverBrakeValve::Release,
            direct_brake: 0.0,
            sanding: false,
            sifa: false,
            pzb_wachsam: false,
            pzb_frei: false,
            pzb_befehl: false,
            lzb_uebernahme: false,
            lzb_ende: false,
            horn: false,
            afb: false,
            afb_target: 0.0,
        }
    }
}
