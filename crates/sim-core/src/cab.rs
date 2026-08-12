//! Cab controls as simulation input (plan ch. 12).

use crate::brakes::DriverBrakeValve;
use serde::{Deserialize, Serialize};

/// Edge detection for push buttons.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    last: bool,
}

impl Edge {
    /// `true` exactly in the step in which the button is pressed.
    pub fn rising(&mut self, now: bool) -> bool {
        let r = now && !self.last;
        self.last = now;
        r
    }

    /// `true` exactly in the step in which the button is released.
    pub fn falling(&mut self, now: bool) -> bool {
        let f = !now && self.last;
        self.last = now;
        f
    }

    pub fn held(&self) -> bool {
        self.last
    }
}

/// All control values of a cab. Buttons are to be read as "currently pressed";
/// the systems evaluate the edges themselves.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CabInputs {
    /// Reverser: −1 backwards, 0 off, +1 forwards.
    pub reverser: i8,
    /// Power controller −1 … +1 (negative = dynamic brake).
    pub throttle: f64,
    pub brake_valve: DriverBrakeValve,
    /// Direct (additional) brake 0 … 1.
    pub direct_brake: f64,
    /// Release button of the loco brake — releases the traction unit's own brake while the
    /// train brake stays applied.
    #[serde(default)]
    pub brake_release: bool,
    /// Parking brake set (spring-applied brake or hand brake).
    #[serde(default)]
    pub parking_brake: bool,
    /// Electrically transmitted, pre-controlled air brake switched on: the whole train
    /// applies at once instead of waiting for the pressure wave.
    #[serde(default)]
    pub ep_brake: bool,
    /// Starter button of the diesel engine.
    #[serde(default)]
    pub engine_start: bool,
    pub sanding: bool,
    /// Sifa pedal/button.
    pub sifa: bool,
    pub pzb_acknowledge: bool,
    pub pzb_exempt: bool,
    pub pzb_override: bool,
    pub lzb_takeover: bool,
    pub lzb_end: bool,
    pub horn: bool,
    /// AFB switched on.
    pub afb: bool,
    /// AFB target speed [km/h].
    pub afb_target: f64,
}

impl Default for CabInputs {
    fn default() -> Self {
        Self {
            reverser: 0,
            throttle: 0.0,
            brake_valve: DriverBrakeValve::Release,
            direct_brake: 0.0,
            brake_release: false,
            parking_brake: false,
            ep_brake: false,
            engine_start: false,
            sanding: false,
            sifa: false,
            pzb_acknowledge: false,
            pzb_exempt: false,
            pzb_override: false,
            lzb_takeover: false,
            lzb_end: false,
            horn: false,
            afb: false,
            afb_target: 0.0,
        }
    }
}
