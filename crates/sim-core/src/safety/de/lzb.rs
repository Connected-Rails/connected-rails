//! LZB 80/I 80 — continuous train protection, on-board side (plan 9.4).
//!
//! The trackside (LZB centre in the interlocking) supplies movement authorities as
//! [`LzbTelegram`] over the loop cable sections. From these the vehicle derives v-Soll,
//! v-Ziel and the distance to target and supervises the braking curve.
//!
//! The block division is line data, not a setting: it follows from where the line carries
//! block markers (`DeviceKind::BlockMarker`). [`authority`] — the centre — looks ahead of the
//! train and ends the movement authority at the first block boundary that is not clear. The
//! block mode of the telegram is what falls out of that look-ahead (plan 9.4):
//!
//! * [`LzbBlockMode::Full`] — the line has LZB block markers of its own, so they divide the
//!   movement authority and replace the lineside signals. The PZB magnets are suppressed
//!   under guidance.
//! * [`LzbBlockMode::Partial`] — no LZB block markers: the only boundaries left are the main
//!   signals, so every authority ends at one and the signals stay binding.
//!
//! [`LzbTelegram::cir_elke`] switches the CIR-ELKE build on: shorter blocks with a higher
//! supervised deceleration, finer speed steps, and speed rises that take effect at the head
//! of the train instead of at its rear.
//!
//! The braking curve belongs to the train, not to the line: [`Lzb80::deceleration`] derives
//! it from the brake assessment (BRH), the brake position (BRA) and the initial braking
//! speed, the same three inputs the driver enters at the prototype. The curve only
//! supervises — the braking itself is the physical brake, so a train that is braked too
//! weakly for its movement authority cannot hold the curve.

use crate::cab::{CabInputs, Edge};
use crate::interlock::Interlock;
use crate::lookahead::{self, Restriction};
use crate::safety::{
    Indicator, LampState, ProtectionAction, ProtectionOutput, SafetyTrainState, SelfTest,
    TracksideEvent, TrainProtectionSystem,
};
use serde::{Deserialize, Serialize};
use track_model::{DeviceKind, TrackNetwork, TrackPosition};

/// Block division the LZB works with — a result of the line data, see [`authority`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LzbBlockMode {
    /// LZB block markers instead of signals — the LZB alone gives the movement authority.
    #[default]
    Full,
    /// The LZB is laid over the signal block division; the signals stay binding.
    Partial,
}

/// Line data of a line conductor section: what the cable is, not what it transmits. The
/// payload of a `DeviceKind::LineConductor` device.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct LzbSection {
    /// Length of the section the cable transmits over [m].
    pub length: f64,
    /// CIR-ELKE line.
    #[serde(default)]
    pub cir_elke: bool,
    /// The LZB area ends with this section — the end procedure runs here.
    #[serde(default)]
    pub end: bool,
}

/// Telegram of the LZB centre (payload of a loop cable section).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LzbTelegram {
    /// Permitted speed in the section [km/h].
    pub permitted_speed: f64,
    /// Target speed [km/h] (0 = stop).
    pub target_speed: f64,
    /// Distance to the target from the telegram location [m].
    pub target_distance: f64,
    /// This telegram announces the end of the LZB.
    #[serde(default)]
    pub end_of_authority: bool,
    /// Length of the loop cable section over which this telegram is transmitted [m].
    /// The loop cable transmits continuously — the simulation repeats the telegram
    /// as long as the train is inside the section.
    #[serde(default = "default_conductor_length")]
    pub length: f64,
    /// Block division of this section.
    #[serde(default)]
    pub block_mode: LzbBlockMode,
    /// CIR-ELKE section.
    #[serde(default)]
    pub cir_elke: bool,
}

fn default_conductor_length() -> f64 {
    1000.0
}

/// Deceleration of the LZB braking curve at [`REFERENCE_BRAKE_PERCENTAGE`] and an initial
/// braking speed up to [`V_FULL_DECELERATION`] [m/s²].
///
/// ponytail: the deceleration steps of the real brake tables sit in DB Netz specifications
/// that are not published, so the curve is a straight line through them: proportional to the
/// braked weight percentage, falling off in the upper speed range. Replace the two factors
/// with a table once the figures are on the desk — [`Lzb80::deceleration`] is the only place
/// that reads them.
pub const LZB_DECELERATION: f64 = 0.6;
/// The same for CIR-ELKE [m/s²] — a curve model of its own, not a surcharge: the tighter
/// block division only works because the curve is allowed to be steeper.
pub const LZB_DECELERATION_CIRELKE: f64 = 0.85;
/// Braked weight percentage the two decelerations are stated for [%].
pub const REFERENCE_BRAKE_PERCENTAGE: f64 = 100.0;
/// The brake assessment is held between these two figures [%] — a train outside the range
/// runs on the nearest end of it rather than on an absurd curve.
pub const BRAKE_PERCENTAGE_RANGE: (f64, f64) = (30.0, 225.0);
/// Up to this initial braking speed the brake tables state a constant deceleration [km/h].
pub const V_FULL_DECELERATION: f64 = 150.0;
/// Above it the deceleration falls off linearly with the declining wheel/rail adhesion,
/// down to [`DECELERATION_FADE`] of its value at this speed [km/h].
pub const V_FADED_DECELERATION: f64 = 300.0;
/// Share of the deceleration that is left at [`V_FADED_DECELERATION`].
pub const DECELERATION_FADE: f64 = 0.6;
/// Speed step of the LZB displays [km/h].
pub const V_STEP: f64 = 10.0;
/// Speed step under CIR-ELKE [km/h].
pub const V_STEP_CIRELKE: f64 = 5.0;
/// Without a telegram over this distance the LZB counts as failed [m].
pub const D_LOSS: f64 = 300.0;
/// How far ahead the centre grants a movement authority [m].
pub const AUTHORITY_RANGE: f64 = 12_000.0;
/// Supervision speed in the failure/end procedure ("V40") [km/h].
pub const V_END: f64 = 40.0;

/// LZB centre: the movement authority for a train whose head stands at `head`, on the line
/// conductor section `section`.
///
/// The block division is not a setting of the telegram but the line's own: the authority
/// ends at the first boundary that is not clear — a block marker whose section is occupied,
/// or a main signal at stop. Whether the line carries block markers at all is what decides
/// the block mode, and with it whether the signals stay binding.
///
/// v-target is the most restrictive point ahead, a speed restriction of the line as much as
/// a stop: of all the points ahead the one whose braking curve cuts deepest into the
/// permitted speed.
pub fn authority(
    net: &TrackNetwork,
    interlock: &Interlock,
    head: TrackPosition,
    section: &LzbSection,
) -> LzbTelegram {
    // The LZB supervises train movements; a shunting movement is not in its area at all.
    let ahead = lookahead::scan(
        net,
        interlock,
        head,
        AUTHORITY_RANGE,
        crate::shunt::Movement::Train,
    );

    // A block boundary whose section is occupied ends the authority. A marker naming a
    // section that does not exist counts as occupied.
    let end_of_movement = ahead
        .blocks
        .iter()
        .find(|b| {
            interlock
                .sections
                .get(b.section as usize)
                .is_none_or(|s| s.occupied)
        })
        .map(|b| Restriction {
            distance: b.distance,
            speed: 0.0,
            signal: None,
        });

    // ponytail: the centre picks the governing target with the reference deceleration — the
    // train-specific curve is the vehicle's business, and it only ever gets one target.
    // Two targets whose curves cross within the authority would need the vehicle to be given
    // both, which the display does not have room for anyway.
    let curve = |r: &Restriction| {
        let v = r.speed / 3.6;
        (v * v + 2.0 * LZB_DECELERATION * r.distance.max(0.0)).sqrt() * 3.6
    };
    let permitted = ahead.current;
    let target = ahead
        .restrictions
        .iter()
        .copied()
        .chain(end_of_movement)
        // A speed rise ahead is not a target; the LZB announces the restrictive point even
        // while its curve still lies above the permitted speed.
        .filter(|r| r.speed < permitted)
        .min_by(|a, b| curve(a).total_cmp(&curve(b)));

    LzbTelegram {
        permitted_speed: permitted,
        target_speed: target.map_or(permitted, |r| r.speed),
        target_distance: target.map_or(AUTHORITY_RANGE, |r| r.distance.max(0.0)),
        end_of_authority: section.end,
        length: section.length,
        block_mode: if ahead.blocks.is_empty() {
            LzbBlockMode::Partial
        } else {
            LzbBlockMode::Full
        },
        cir_elke: section.cir_elke,
    }
}

/// Operating state of the LZB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LzbMode {
    /// No loop cable — the PZB is in charge.
    #[default]
    Off,
    /// Pick-up running, takeover by the driver still pending ("Ü" blinking).
    Acceptance,
    /// LZB guidance active.
    Guiding,
    /// End of the LZB announced ("ENDE" blinking).
    Ending,
    /// LZB failed — failure procedure.
    Failure,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Lzb80 {
    pub mode: LzbMode,
    isolated: bool,
    /// Function test after switching on.
    test: SelfTest,
    /// Last received telegram.
    telegram: Option<LzbTelegram>,
    /// Odometer reading the telegram's distance to target refers to [m].
    telegram_odo: f64,
    /// Odometer reading of the last reception — for the failure detection [m].
    last_contact_odo: f64,
    /// Distance to target, current [m].
    target_distance: f64,
    /// Permitted speed currently in force [km/h]. Outside CIR-ELKE a rise only applies
    /// once the rear of the train has passed the point of change.
    permitted: f64,
    /// Pending speed rise: (speed [km/h], odometer from which it applies [m]).
    pending_rise: Option<(f64, f64)>,
    /// Forced braking active.
    tripped: bool,
    /// Train data the braking curve is written for: braked weight percentage (BRH) and the
    /// filling time of the slowest brake (BRA). At the prototype the driver enters them; here
    /// they come from the train itself, so they cannot be entered wrongly.
    brake_percentage: f64,
    brake_apply_time: f64,
    takeover: Edge,
    end: Edge,
    lzb_test: Edge,
}

impl Lzb80 {
    /// An LZB that has already been function-tested.
    pub fn new() -> Self {
        Self::default()
    }

    /// Switches the device on: the function test starts (plan 9.4).
    pub fn power_on(&mut self) {
        self.test.restart();
    }

    /// State of the function test.
    pub fn self_test(&self) -> SelfTest {
        self.test
    }

    pub fn is_guiding(&self) -> bool {
        matches!(self.mode, LzbMode::Guiding | LzbMode::Ending)
    }

    /// Block mode of the section currently being run through.
    pub fn block_mode(&self) -> LzbBlockMode {
        self.telegram.map(|t| t.block_mode).unwrap_or_default()
    }

    /// Are the lineside signals binding? In partial block mode they are, so their PZB
    /// magnets stay effective as the fallback level.
    ///
    /// ponytail: the model reduces the difference between the block modes to "signals
    /// binding yes/no". The full picture also covers the block division of the movement
    /// authority itself — that follows once the LZB centre in the interlocking generates
    /// its own block markers.
    pub fn signals_binding(&self) -> bool {
        self.is_guiding() && self.block_mode() == LzbBlockMode::Partial
    }

    /// CIR-ELKE section?
    pub fn is_cir_elke(&self) -> bool {
        self.telegram.is_some_and(|t| t.cir_elke)
    }

    /// Supervised deceleration [m/s²] — derived from the train data, not a fixed value.
    ///
    /// Two things decide it, as they do at the prototype: the brake assessment of the train
    /// (BRH), which the deceleration is proportional to, and the initial braking speed. Up to
    /// [`V_FULL_DECELERATION`] the brake tables state constant values; above it they fall off
    /// linearly, because the adhesion between wheel and rail does. The initial braking speed
    /// is the permitted speed in force — that is what the train brakes from, and it is capped
    /// by the maximum speed the LZB was given for this train.
    pub fn deceleration(&self) -> f64 {
        let base = if self.is_cir_elke() {
            LZB_DECELERATION_CIRELKE
        } else {
            LZB_DECELERATION
        };
        base * self.brake_assessment() * self.adhesion_fade(self.permitted)
    }

    /// Brake assessment as a factor on the reference curve — 1.0 at
    /// [`REFERENCE_BRAKE_PERCENTAGE`]. Without reported train data the LZB stays on the
    /// reference value.
    fn brake_assessment(&self) -> f64 {
        if self.brake_percentage <= 0.0 {
            return 1.0;
        }
        let (lo, hi) = BRAKE_PERCENTAGE_RANGE;
        self.brake_percentage.clamp(lo, hi) / REFERENCE_BRAKE_PERCENTAGE
    }

    /// Factor the deceleration falls off by above [`V_FULL_DECELERATION`].
    fn adhesion_fade(&self, v_kmh: f64) -> f64 {
        let over = (v_kmh - V_FULL_DECELERATION) / (V_FADED_DECELERATION - V_FULL_DECELERATION);
        1.0 - (1.0 - DECELERATION_FADE) * over.clamp(0.0, 1.0)
    }

    /// Time the brake needs to take hold, as far as the curve allows for it [s]: half the
    /// filling time is the usual equivalent-time substitute for the pressure ramp. It is what
    /// separates a freight train in G from a passenger train in P.
    fn build_up_time(&self) -> f64 {
        self.brake_apply_time / 2.0
    }

    /// Speed step of the displays [km/h].
    pub fn speed_step(&self) -> f64 {
        if self.is_cir_elke() {
            V_STEP_CIRELKE
        } else {
            V_STEP
        }
    }

    /// The LZB transmits speeds in steps; rounding down keeps it on the safe side.
    fn quantise(&self, v: f64) -> f64 {
        let step = self.speed_step();
        (v / step).floor() * step
    }

    /// v-Soll [km/h] — the guidance's target speed (also the AFB input).
    pub fn permitted_speed(&self) -> Option<f64> {
        if !self.is_guiding() {
            return None;
        }
        let t = self.telegram?;
        // Braking curve to the target, with the build-up time of the brake as lost distance:
        // v² + 2·a·t_b·v = v_target² + 2·a·s, solved for v.
        let a = self.deceleration();
        let loss = a * self.build_up_time();
        let v_target = t.target_speed / 3.6;
        let reach = v_target * v_target + 2.0 * a * self.target_distance.max(0.0);
        let curve = ((loss * loss + reach).sqrt() - loss) * 3.6;
        Some(self.quantise(self.permitted.min(curve)))
    }

    /// v-Ziel [km/h].
    pub fn target_speed(&self) -> Option<f64> {
        self.telegram
            .map(|t| self.quantise(t.target_speed))
            .filter(|_| self.is_guiding())
    }

    /// Distance to target [m].
    pub fn target_distance(&self) -> Option<f64> {
        self.is_guiding().then_some(self.target_distance.max(0.0))
    }

    pub fn tripped(&self) -> bool {
        self.tripped
    }

    /// Takes a new telegram over. A speed rise only applies once the whole train has
    /// passed the point of change — under CIR-ELKE already at its head.
    fn accept(&mut self, t: LzbTelegram, train: &SafetyTrainState, s_offset: f64) {
        let at = train.odometer - s_offset;
        if t.permitted_speed > self.permitted && self.is_guiding() && !t.cir_elke {
            // The centre repeats the authority every step, so a rise that is already waiting
            // must not be pushed ahead of the train with it.
            if self.pending_rise.is_none_or(|(v, _)| v < t.permitted_speed) {
                self.pending_rise = Some((t.permitted_speed, at + train.train_length));
            }
        } else {
            self.permitted = t.permitted_speed;
            self.pending_rise = None;
        }
        self.telegram = Some(t);
        self.telegram_odo = at;
    }
}

impl TrainProtectionSystem for Lzb80 {
    fn update(
        &mut self,
        dt: f64,
        train: &SafetyTrainState,
        cab: &CabInputs,
        events: &[TracksideEvent],
    ) -> ProtectionOutput {
        // Train data of the braking curve — at the prototype an entry by the driver, here
        // read off the train, and kept up to date because the load may change at a station.
        self.brake_percentage = train.brake_percentage;
        self.brake_apply_time = train.brake_apply_time;

        if self.isolated {
            self.mode = LzbMode::Off;
            return ProtectionOutput::default();
        }

        let takeover = self.takeover.rising(cab.lzb_takeover);
        let end = self.end.rising(cab.lzb_end);
        let test_ack = self.lzb_test.rising(cab.lzb_test);

        // Function test — until it has passed the LZB is not available; the PZB remains
        // in charge, so unlike the PZB test this one does not hold the brake.
        if !self.test.is_passed() {
            self.test.step(dt, train, test_ack);
            self.mode = LzbMode::Off;
            self.telegram = None;
            return ProtectionOutput::default();
        }

        // Pick up telegrams from the loop cable.
        for e in events {
            if e.device != DeviceKind::LineConductor || !e.active {
                continue;
            }
            let Ok(t) = ron::from_str::<LzbTelegram>(&e.payload) else {
                continue;
            };
            // The loop cable transmits continuously. An unchanged telegram is only a sign
            // of life and must not reset the distance to target.
            self.last_contact_odo = train.odometer;
            if self.telegram == Some(t) {
                continue;
            }
            self.accept(t, train, e.s_offset);
            self.mode = match self.mode {
                LzbMode::Off | LzbMode::Failure => LzbMode::Acceptance,
                LzbMode::Ending if !t.end_of_authority => LzbMode::Guiding,
                m => m,
            };
            if t.end_of_authority && self.is_guiding() {
                self.mode = LzbMode::Ending;
            }
        }

        // A pending speed rise takes effect once the rear of the train has passed.
        if let Some((v, at)) = self.pending_rise
            && train.odometer >= at
        {
            self.permitted = v;
            self.pending_rise = None;
        }

        // Reduce the distance to target with the distance travelled.
        if let Some(t) = self.telegram {
            self.target_distance = t.target_distance - (train.odometer - self.telegram_odo);
        }

        // Takeover by the driver.
        if self.mode == LzbMode::Acceptance && takeover {
            self.mode = LzbMode::Guiding;
        }

        // Loss of telegram → failure procedure.
        if matches!(self.mode, LzbMode::Guiding | LzbMode::Acceptance)
            && train.odometer - self.last_contact_odo > D_LOSS
        {
            self.mode = LzbMode::Failure;
        }

        // Acknowledge the end procedure → back to the PZB.
        if self.mode == LzbMode::Ending && end {
            self.mode = LzbMode::Off;
            self.telegram = None;
        }
        if self.mode == LzbMode::Failure && end {
            self.mode = LzbMode::Off;
            self.telegram = None;
        }

        // Supervision.
        let limit = match self.mode {
            LzbMode::Guiding | LzbMode::Ending => self.permitted_speed(),
            LzbMode::Failure => Some(V_END),
            _ => None,
        };
        if let Some(l) = limit {
            if train.v_kmh > l + 3.0 {
                self.tripped = true;
            } else if train.v_kmh <= l {
                self.tripped = false;
            }
        } else {
            self.tripped = false;
        }

        ProtectionOutput {
            action: if self.tripped {
                // The LZB brakes with a service application first, not an emergency one.
                ProtectionAction::ForcedServiceBrake
            } else {
                ProtectionAction::None
            },
            speed_limit: limit,
            target_speed: self.target_speed(),
            target_distance: self.target_distance(),
            alert: self.mode == LzbMode::Acceptance || self.mode == LzbMode::Ending,
            vigilance_alert: false,
            protection_alert: self.mode == LzbMode::Acceptance || self.mode == LzbMode::Ending,
        }
    }

    fn indicators(&self) -> Vec<Indicator> {
        // Lamp test of the function test: everything lit.
        if self.test.lamp_test() {
            return [
                "lzb_ue",
                "lzb_g",
                "lzb_ende",
                "lzb_stoerung",
                "lzb_b",
                "lzb_v40",
            ]
            .into_iter()
            .map(|n| Indicator::lamp(n, true))
            .collect();
        }
        let mut v = vec![
            Indicator::state(
                "lzb_ue",
                match self.mode {
                    LzbMode::Acceptance => LampState::Blinking,
                    LzbMode::Guiding | LzbMode::Ending => LampState::On,
                    _ => LampState::Off,
                },
            ),
            // "G" — the LZB is guiding the train.
            Indicator::lamp("lzb_g", self.is_guiding()),
            Indicator::state(
                "lzb_ende",
                match self.mode {
                    LzbMode::Ending => LampState::Blinking,
                    _ => LampState::Off,
                },
            ),
            Indicator::lamp("lzb_stoerung", self.mode == LzbMode::Failure),
            Indicator::lamp("lzb_b", self.tripped),
            Indicator::lamp("lzb_v40", self.mode == LzbMode::Failure),
        ];
        if let Some(s) = self.permitted_speed() {
            v.push(Indicator::value("mfa_v_soll", s));
        }
        if let Some(s) = self.target_speed() {
            v.push(Indicator::value("mfa_v_ziel", s));
        }
        if let Some(d) = self.target_distance() {
            v.push(Indicator::value("mfa_zielentfernung", d));
        }
        v
    }

    fn isolate(&mut self, isolated: bool) {
        self.isolated = isolated;
    }

    fn is_isolated(&self) -> bool {
        self.isolated
    }

    fn name(&self) -> &'static str {
        "LZB 80"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safety::SelfTestPhase;

    fn telegram_event(t: LzbTelegram, s_offset: f64) -> TracksideEvent {
        TracksideEvent {
            device: DeviceKind::LineConductor,
            payload: ron::to_string(&t).unwrap(),
            s_offset,
            active: true,
        }
    }

    /// A telegram with the usual defaults; the tests override what they care about.
    fn telegram(permitted_speed: f64, target_speed: f64, target_distance: f64) -> LzbTelegram {
        LzbTelegram {
            permitted_speed,
            target_speed,
            target_distance,
            end_of_authority: false,
            length: 1000.0,
            block_mode: LzbBlockMode::Full,
            cir_elke: false,
        }
    }

    struct Rig {
        lzb: Lzb80,
        state: SafetyTrainState,
        cab: CabInputs,
        out: ProtectionOutput,
        /// What the loop cable is currently transmitting (None = no cable/failure).
        telegram: Option<LzbTelegram>,
    }

    impl Rig {
        fn new(v_kmh: f64) -> Self {
            Self {
                lzb: Lzb80::new(),
                state: SafetyTrainState {
                    v_kmh,
                    train_length: 200.0,
                    ..Default::default()
                },
                cab: CabInputs::default(),
                out: ProtectionOutput::default(),
                telegram: None,
            }
        }
        fn events(&self) -> Vec<TracksideEvent> {
            self.telegram
                .map(|t| vec![telegram_event(t, 0.0)])
                .unwrap_or_default()
        }
        fn send(&mut self, t: LzbTelegram) {
            self.telegram = Some(t);
            self.out = self.lzb.update(0.0, &self.state, &self.cab, &self.events());
        }
        fn drive(&mut self, meters: f64) {
            let dt = 0.1;
            let step = self.state.v_kmh / 3.6 * dt;
            let n = if step > 0.0 {
                (meters / step).round() as u32
            } else {
                (meters as u32).max(1)
            };
            for _ in 0..n {
                self.state.odometer += step;
                let ev = self.events();
                self.out = self.lzb.update(dt, &self.state, &self.cab, &ev);
            }
        }
        /// Lets `seconds` pass on the spot (for the function test).
        fn run(&mut self, seconds: f64) {
            let dt = 0.1;
            for _ in 0..(seconds / dt).round() as u32 {
                let ev = self.events();
                self.out = self.lzb.update(dt, &self.state, &self.cab, &ev);
            }
        }
        fn press(&mut self, set: impl Fn(&mut CabInputs)) {
            set(&mut self.cab);
            let ev = self.events();
            self.out = self.lzb.update(0.05, &self.state, &self.cab, &ev);
            self.cab = CabInputs::default();
            let ev = self.events();
            self.out = self.lzb.update(0.05, &self.state, &self.cab, &ev);
        }
        fn takeover(&mut self) {
            self.press(|c| c.lzb_takeover = true);
        }
    }

    #[test]
    fn acceptance_only_after_takeover() {
        let mut r = Rig::new(120.0);
        r.send(telegram(160.0, 0.0, 5000.0));
        assert_eq!(r.lzb.mode, LzbMode::Acceptance);
        assert!(!r.lzb.is_guiding());
        r.takeover();
        assert_eq!(r.lzb.mode, LzbMode::Guiding);
        assert!(r.lzb.permitted_speed().is_some());
    }

    #[test]
    fn braking_curve_to_stop_lowers_permitted_speed() {
        let mut r = Rig::new(160.0);
        r.send(telegram(160.0, 0.0, 6000.0));
        r.takeover();
        assert!(
            r.lzb.permitted_speed().unwrap() >= 160.0,
            "far away: full permitted speed"
        );
        r.drive(5000.0);
        let v = r.lzb.permitted_speed().unwrap();
        // 1000 m left until the stop, reference curve at 160 km/h: a = 0.58 m/s²,
        // sqrt(2·0.58·1000) = 34.2 m/s = 123 km/h.
        assert!(v > 110.0 && v < 135.0, "permitted speed = {v}");
        assert!(r.lzb.target_distance().unwrap() < 1100.0);
        r.drive(900.0);
        assert!(r.lzb.permitted_speed().unwrap() < 45.0);
        assert_eq!(
            r.out.action,
            ProtectionAction::ForcedServiceBrake,
            "160 km/h is too fast"
        );
    }

    /// The curve is the train's, not the line's: brake assessment and brake position both
    /// move it. An ICE at 180 BRH in R may run where a freight train at 65 BRH in G may not.
    #[test]
    fn the_braking_curve_follows_the_train_data() {
        let curve = |brh: f64, apply_time: f64| {
            let mut r = Rig::new(120.0);
            r.state.brake_percentage = brh;
            r.state.brake_apply_time = apply_time;
            r.send(telegram(160.0, 0.0, 1000.0));
            r.takeover();
            r.lzb.permitted_speed().unwrap()
        };
        assert!(
            curve(180.0, 4.0) > curve(65.0, 22.0) + 50.0,
            "an ICE keeps a far higher speed than a freight train at the same target"
        );
        assert!(
            curve(100.0, 4.0) > curve(100.0, 22.0),
            "the G position costs braking distance and therefore speed"
        );
    }

    /// Above 150 km/h the brake tables let the deceleration fall off — the fixed value used
    /// to be too optimistic in exactly the range the LZB exists for.
    #[test]
    fn the_deceleration_falls_off_in_the_high_speed_range() {
        let decel = |permitted: f64| {
            let mut r = Rig::new(100.0);
            r.state.brake_percentage = 130.0;
            r.send(telegram(permitted, 0.0, 20_000.0));
            r.takeover();
            r.lzb.deceleration()
        };
        assert!(
            (decel(150.0) - decel(100.0)).abs() < 1e-9,
            "constant up to 150 km/h"
        );
        let faded = decel(300.0) / decel(150.0);
        assert!(
            (faded - DECELERATION_FADE).abs() < 1e-9,
            "fade at 300 km/h = {faded}"
        );
    }

    #[test]
    fn end_procedure_hands_over_to_pzb() {
        let mut r = Rig::new(100.0);
        r.send(telegram(160.0, 100.0, 2000.0));
        r.takeover();
        r.send(LzbTelegram {
            end_of_authority: true,
            ..telegram(100.0, 100.0, 1000.0)
        });
        assert_eq!(r.lzb.mode, LzbMode::Ending);
        r.telegram = None; // loop cable ends
        r.press(|c| c.lzb_end = true);
        assert_eq!(r.lzb.mode, LzbMode::Off);
        assert!(!r.lzb.is_guiding(), "PZB takes over again");
    }

    #[test]
    fn telegram_loss_leads_to_failure_procedure() {
        let mut r = Rig::new(100.0);
        r.send(telegram(160.0, 160.0, 9000.0));
        r.takeover();
        r.telegram = None; // loop cable ends without warning
        r.drive(400.0);
        assert_eq!(r.lzb.mode, LzbMode::Failure);
        assert_eq!(r.out.speed_limit, Some(V_END));
    }

    // --- Block modes ------------------------------------------------------------------

    #[test]
    fn full_block_mode_replaces_the_signals() {
        let mut r = Rig::new(100.0);
        r.send(telegram(160.0, 160.0, 9000.0));
        r.takeover();
        assert_eq!(r.lzb.block_mode(), LzbBlockMode::Full);
        assert!(
            !r.lzb.signals_binding(),
            "full block mode: LZB block markers instead of signals"
        );
    }

    #[test]
    fn partial_block_mode_keeps_the_signals_binding() {
        let mut r = Rig::new(100.0);
        r.send(LzbTelegram {
            block_mode: LzbBlockMode::Partial,
            ..telegram(160.0, 0.0, 3000.0)
        });
        r.takeover();
        assert_eq!(r.lzb.block_mode(), LzbBlockMode::Partial);
        assert!(r.lzb.signals_binding());
        assert!(r.lzb.is_guiding(), "guidance runs all the same");
    }

    // --- CIR-ELKE ---------------------------------------------------------------------

    #[test]
    fn cir_elke_supervises_a_steeper_braking_curve() {
        let mut plain = Rig::new(160.0);
        plain.send(telegram(300.0, 0.0, 3000.0));
        plain.takeover();

        let mut cir = Rig::new(160.0);
        cir.send(LzbTelegram {
            cir_elke: true,
            ..telegram(300.0, 0.0, 3000.0)
        });
        cir.takeover();

        assert!(cir.lzb.deceleration() > plain.lzb.deceleration());
        assert!(
            cir.lzb.permitted_speed().unwrap() > plain.lzb.permitted_speed().unwrap(),
            "the steeper curve permits more speed at the same distance"
        );
    }

    #[test]
    fn cir_elke_displays_five_kmh_steps() {
        let mut r = Rig::new(100.0);
        r.send(telegram(160.0, 85.0, 9000.0));
        r.takeover();
        assert_eq!(r.lzb.target_speed(), Some(80.0), "10 km/h steps");

        let mut r = Rig::new(100.0);
        r.send(LzbTelegram {
            cir_elke: true,
            ..telegram(160.0, 85.0, 9000.0)
        });
        r.takeover();
        assert_eq!(r.lzb.target_speed(), Some(85.0), "5 km/h steps");
    }

    #[test]
    fn speed_rise_waits_for_the_rear_of_the_train() {
        let mut r = Rig::new(80.0);
        r.state.train_length = 400.0;
        r.send(telegram(100.0, 100.0, 9000.0));
        r.takeover();
        r.send(telegram(160.0, 160.0, 8000.0));
        assert_eq!(
            r.lzb.permitted_speed(),
            Some(100.0),
            "the rise only applies once the train has passed"
        );
        r.drive(200.0);
        assert_eq!(r.lzb.permitted_speed(), Some(100.0));
        r.drive(250.0);
        assert_eq!(r.lzb.permitted_speed(), Some(160.0));
    }

    #[test]
    fn cir_elke_raises_the_speed_at_the_head_of_the_train() {
        let mut r = Rig::new(80.0);
        r.state.train_length = 400.0;
        r.send(LzbTelegram {
            cir_elke: true,
            ..telegram(100.0, 100.0, 9000.0)
        });
        r.takeover();
        r.send(LzbTelegram {
            cir_elke: true,
            ..telegram(160.0, 160.0, 8000.0)
        });
        assert_eq!(r.lzb.permitted_speed(), Some(160.0), "effective at once");
    }

    #[test]
    fn a_speed_reduction_always_applies_at_once() {
        let mut r = Rig::new(120.0);
        r.state.train_length = 400.0;
        r.send(telegram(160.0, 160.0, 9000.0));
        r.takeover();
        r.send(telegram(100.0, 100.0, 8000.0));
        assert_eq!(r.lzb.permitted_speed(), Some(100.0));
    }

    // --- Function test ----------------------------------------------------------------

    #[test]
    fn function_test_blocks_the_guidance_until_it_is_acknowledged() {
        let mut r = Rig::new(0.0);
        r.lzb.power_on();
        assert_eq!(r.lzb.self_test().phase(), SelfTestPhase::Lamps);
        assert!(r.lzb.indicators().iter().all(|i| i.lamp == LampState::On));
        r.send(telegram(160.0, 160.0, 9000.0));
        r.run(6.0);
        assert_eq!(r.lzb.self_test().phase(), SelfTestPhase::AwaitAck);
        assert_eq!(r.lzb.mode, LzbMode::Off, "no pick-up during the test");
        r.press(|c| c.lzb_test = true);
        assert!(r.lzb.self_test().is_passed());
        r.run(0.2);
        assert_eq!(r.lzb.mode, LzbMode::Acceptance, "picks up after the test");
    }
}
