//! GNT — speed supervision for tilting technology, on-board side (plan 9.5).
//!
//! A tilting unit may run curves faster than a conventional train, but only for as long as
//! somebody supervises that it really can. That is the GNT: inside a GNT area it supervises a
//! **second, higher speed profile** — the GNT profile — and it only releases that profile
//! while the tilting technology works. The moment the tilt fails, the regular profile is
//! binding again and the GNT leads the train down onto it.
//!
//! The trackside side is a data point: a [`DeviceKind::Balise`] whose payload is a
//! [`GntDataPoint`]. No new device kind was needed — the balise is provided for in the plan
//! and carries whatever payload a country package puts on it. A payload that is not a
//! [`GntDataPoint`] simply does not parse and is ignored, which is how ETCS balises will
//! live on the same kind later.
//!
//! The structure follows the LZB deliberately (`super::lzb`): a telegram-like line datum, a
//! permitted speed, a braking curve down to a target, forced braking when the curve is
//! exceeded. What differs is where the numbers come from — the LZB centre computes its
//! authority from the interlocking every step, whereas the GNT profile is static line data:
//! it says how fast a *curve* may be taken, which no signal aspect changes.
//!
//! **Against the PZB:** nothing. The GNT never replaces signal protection — it only raises
//! the line speed between two signals, so the PZB magnets stay fully effective underneath it.
//! **Against the LZB:** the LZB wins. Its authority already accounts for the line, and the
//! two systems do not overlap on the real network (the GNT exists for conventional lines);
//! while the LZB guides, the GNT stands down and publishes nothing
//! ([`Gnt::stand_by`]).
//!
//! ponytail: what this build leaves out, and how to grow it.
//! * **No train data entry (ZDE).** At the prototype the driver registers with the GNT and
//!   enters the train data; here the registration happens by itself at the first data point,
//!   and the two data that matter are read off the train, exactly as the LZB reads BRH/BRA
//!   instead of having them typed in: tilting capability from the fitment, train length from
//!   [`SafetyTrainState::train_length`]. Upgrade path: a `CabInputs::gnt_register` button
//!   plus a ZDE dialogue on the display — the state machine below already has the
//!   [`GntMode::Ready`] state it would sit in.
//! * **No function test.** The LZB's [`crate::safety::SelfTest`] would drop straight in;
//!   [`Gnt::power_on`] is where it would start, and a GNT test button is what it would need.
//! * **No graduated release of the tilt.** The tilting technology is either healthy or
//!   failed ([`Gnt::set_tilt_fault`]); a real unit can lose single tilt actuators and run on
//!   a reduced surcharge. That would be a percentage on the profile speed rather than a
//!   boolean, and nothing else in here would change.

use crate::cab::{CabInputs, Edge};
use crate::safety::{
    Indicator, ProtectionAction, ProtectionOutput, SafetyTrainState, TracksideEvent,
    TrainProtectionSystem,
};
use serde::{Deserialize, Serialize};
use track_model::DeviceKind;

use super::lzb::{BRAKE_PERCENTAGE_RANGE, REFERENCE_BRAKE_PERCENTAGE};

/// Supervised deceleration of the GNT braking curve at [`REFERENCE_BRAKE_PERCENTAGE`]
/// [m/s²].
///
/// ponytail: estimated. The GNT brake tables are not published either; the figure sits
/// above the LZB's reference value because the GNT exists on tilting multiple units, which
/// are disc-braked throughout and brake harder than the locomotive-hauled trains the LZB
/// curve has to cover as well. Replace it with a table once the figures are on the desk —
/// [`Gnt::deceleration`] is the only place that reads it.
pub const GNT_DECELERATION: f64 = 0.7;

/// How far a data point is good for when it does not say [m].
pub const D_SECTION: f64 = 1000.0;

/// Speed by which the supervised value may be exceeded before the forced braking comes
/// [km/h] — the same tolerance the LZB grants.
pub const V_TOLERANCE: f64 = 3.0;

/// Data point of the GNT (payload of a [`DeviceKind::Balise`]).
///
/// `profile_speed` carries no default on purpose: it is what tells a GNT data point apart
/// from any other balise payload, because a payload without it does not parse.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GntDataPoint {
    /// Speed of the GNT profile from this data point [km/h]. `0` releases nothing: the
    /// section runs on the regular profile, which the GNT then supervises.
    pub profile_speed: f64,
    /// Speed at the end of the section [km/h] — what the braking curve leads down to.
    #[serde(default)]
    pub target_speed: f64,
    /// Distance from the data point to that target [m]. `0` = the data point names no
    /// target, so the profile speed applies over the whole section.
    #[serde(default)]
    pub target_distance: f64,
    /// How far this data point is good for [m]. After it — measured to the rear of the
    /// train — the GNT falls back to the regular profile.
    #[serde(default = "default_section_length")]
    pub length: f64,
    /// This data point ends the GNT area: the on-board unit signs off here.
    #[serde(default)]
    pub end: bool,
}

fn default_section_length() -> f64 {
    D_SECTION
}

impl GntDataPoint {
    /// A data point that opens a section at `profile_speed` and holds it for `length`.
    pub fn section(profile_speed: f64, length: f64) -> Self {
        Self {
            profile_speed,
            target_speed: 0.0,
            target_distance: 0.0,
            length,
            end: false,
        }
    }

    /// The data point that ends a GNT area — the sign-off.
    pub fn end() -> Self {
        Self {
            end: true,
            ..Self::section(0.0, 0.0)
        }
    }
}

/// Operating state of the GNT — what the cab indicators show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GntMode {
    /// Outside a GNT area, or signed off — the GNT supervises nothing.
    #[default]
    Off,
    /// Registered inside a GNT area, but running on the regular profile: the data point
    /// releases no higher speed.
    Ready,
    /// The GNT profile is released and supervised.
    Supervising,
    /// The tilting technology has failed — the regular profile is binding again.
    Fault,
}

/// The GNT on-board equipment.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Gnt {
    isolated: bool,
    /// The LZB is guiding: the GNT stands down behind its movement authority.
    standby: bool,
    /// The tilting technology has failed.
    tilt_fault: bool,
    mode: GntMode,
    /// Data point in force, and the odometer reading it was read at [m].
    point: Option<GntDataPoint>,
    point_odo: f64,
    /// Return run onto the regular profile after the release was lost: the speed it leads
    /// down to [km/h] and the odometer reading from which it is binding [m].
    fallback: Option<(f64, f64)>,
    /// Braked weight percentage the curve is written for — read off the train.
    brake_percentage: f64,
    /// Forced braking active.
    tripped: bool,
    /// Supervised speed [km/h], as computed last.
    limit: Option<f64>,
    acknowledge: Edge,
}

impl Gnt {
    pub fn new() -> Self {
        Self::default()
    }

    /// Switching the vehicle on wipes the running state — a data point read before the
    /// device was live is not line data the GNT may act on.
    pub fn power_on(&mut self) {
        *self = Self {
            isolated: self.isolated,
            ..Self::default()
        };
    }

    /// The LZB has taken the train over (or given it back). While it guides, its authority
    /// is the binding one and the GNT publishes nothing.
    pub fn stand_by(&mut self, standby: bool) {
        self.standby = standby;
    }

    /// Reports the tilting technology as failed or healthy again.
    ///
    /// ponytail: nothing in the simulator writes this yet — the natural source is a tilt
    /// system switch in the cab, and that belongs in `CabInputs` so it travels to the
    /// server like every other lever. Until then it is the hook a scenario or a vehicle
    /// script sets.
    pub fn set_tilt_fault(&mut self, faulty: bool) {
        self.tilt_fault = faulty;
    }

    pub fn tilt_fault(&self) -> bool {
        self.tilt_fault
    }

    pub fn mode(&self) -> GntMode {
        self.mode
    }

    /// Is the higher GNT profile released?
    pub fn released(&self) -> bool {
        self.mode == GntMode::Supervising
    }

    /// Currently supervised speed [km/h], if the GNT supervises at all.
    pub fn supervised_speed(&self) -> Option<f64> {
        self.limit
    }

    pub fn tripped(&self) -> bool {
        self.tripped
    }

    /// Supervised deceleration [m/s²]. As with the LZB it is proportional to the brake
    /// assessment of the train; without reported train data the reference value stands.
    ///
    /// ponytail: no fall-off in the high speed range, unlike the LZB — the GNT lives on
    /// conventional lines at up to 160 km/h, which is below where the brake tables start
    /// falling off at all.
    pub fn deceleration(&self) -> f64 {
        if self.brake_percentage <= 0.0 {
            return GNT_DECELERATION;
        }
        let (lo, hi) = BRAKE_PERCENTAGE_RANGE;
        GNT_DECELERATION * self.brake_percentage.clamp(lo, hi) / REFERENCE_BRAKE_PERCENTAGE
    }

    /// Speed a braking curve permits `distance` metres ahead of a target of `v_target`
    /// [km/h].
    fn curve(&self, v_target: f64, distance: f64) -> f64 {
        let v = v_target / 3.6;
        (v * v + 2.0 * self.deceleration() * distance.max(0.0)).sqrt() * 3.6
    }

    /// Reads the data points out of the trackside events.
    fn collect(&mut self, train: &SafetyTrainState, events: &[TracksideEvent]) {
        for e in events {
            if e.device != DeviceKind::Balise || !e.active {
                continue;
            }
            let Ok(p) = ron::from_str::<GntDataPoint>(&e.payload) else {
                continue;
            };
            if p.end {
                // Sign-off at the end of the area: the supervision is released.
                self.point = None;
                self.fallback = None;
                continue;
            }
            self.point = Some(p);
            self.point_odo = train.odometer - e.s_offset;
        }
    }
}

impl TrainProtectionSystem for Gnt {
    fn update(
        &mut self,
        _dt: f64,
        train: &SafetyTrainState,
        cab: &CabInputs,
        events: &[TracksideEvent],
    ) -> ProtectionOutput {
        // Train datum of the braking curve, read off the train rather than typed in.
        self.brake_percentage = train.brake_percentage;

        if self.isolated || self.standby {
            *self = Self {
                isolated: self.isolated,
                standby: self.standby,
                tilt_fault: self.tilt_fault,
                brake_percentage: self.brake_percentage,
                ..Self::default()
            };
            return ProtectionOutput::default();
        }

        // The GNT shares the acknowledgement button with the PZB — on the units that carry
        // a GNT the two sit in the same device.
        let acknowledge = self.acknowledge.rising(cab.pzb_acknowledge);

        self.collect(train, events);

        // A data point is good for its own length, measured to the rear of the train: the
        // curve is only behind the train once its tail has left it. That is what the train
        // length out of the train data is for.
        if let Some(p) = self.point
            && train.odometer - self.point_odo > p.length + train.train_length
        {
            self.point = None;
        }

        let was_released = self.released();
        let released = !self.tilt_fault && self.point.is_some_and(|p| p.profile_speed > 0.0);

        // Losing the release does not trip the brake: the GNT leads the train down onto the
        // regular profile over the distance its own curve needs for it.
        if was_released && !released {
            let a = self.deceleration();
            let v = (train.v_kmh / 3.6).max(train.line_speed / 3.6);
            let v_reg = train.line_speed / 3.6;
            let distance = ((v * v - v_reg * v_reg) / (2.0 * a)).max(0.0);
            self.fallback = Some((train.line_speed, train.odometer + distance));
        }
        if released {
            self.fallback = None;
        }
        if self.fallback.is_some_and(|(_, at)| train.odometer >= at) {
            self.fallback = None;
        }

        // Supervision. Inside a GNT area the GNT supervises whichever profile is in force —
        // the released one or the regular one; outside it, nothing.
        let limit = self.point.map(|p| {
            let permitted = if released {
                p.profile_speed
            } else {
                train.line_speed
            };
            // During the return run the curve is what is supervised: it starts at the speed
            // the train ran when the release fell away and decays onto the regular profile.
            let mut v = match self.fallback {
                Some((v_reg, at)) => permitted.max(self.curve(v_reg, at - train.odometer)),
                None => permitted,
            };
            if p.target_distance > 0.0 {
                let left = p.target_distance - (train.odometer - self.point_odo);
                v = v.min(self.curve(p.target_speed, left));
            }
            v
        });
        self.limit = limit;

        if limit.is_some_and(|l| train.v_kmh > l + V_TOLERANCE) {
            self.tripped = true;
        }
        // A GNT forced braking runs to a standstill and is released with the
        // acknowledgement button, like the PZB's.
        if self.tripped && train.standstill() && acknowledge {
            self.tripped = false;
        }

        self.mode = if self.tilt_fault {
            GntMode::Fault
        } else if self.point.is_none() {
            GntMode::Off
        } else if released {
            GntMode::Supervising
        } else {
            GntMode::Ready
        };

        ProtectionOutput {
            action: if self.tripped {
                ProtectionAction::EmergencyBrake
            } else {
                ProtectionAction::None
            },
            speed_limit: limit,
            alert: self.tripped || self.mode == GntMode::Fault,
            protection_alert: self.tripped || self.mode == GntMode::Fault,
            ..Default::default()
        }
    }

    fn indicators(&self) -> Vec<Indicator> {
        let mut v = vec![
            Indicator::lamp("gnt_bereit", self.mode == GntMode::Ready),
            Indicator::lamp("gnt_ue", self.mode == GntMode::Supervising),
            Indicator::lamp("gnt_stoerung", self.mode == GntMode::Fault),
            Indicator::lamp("gnt_b", self.tripped),
        ];
        if let Some(l) = self.limit {
            v.push(Indicator::value("gnt_v_soll", l));
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
        "GNT"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_point(p: GntDataPoint, s_offset: f64) -> TracksideEvent {
        TracksideEvent {
            device: DeviceKind::Balise,
            payload: ron::to_string(&p).unwrap(),
            s_offset,
            active: true,
        }
    }

    /// Small test rig: runs the train at constant speed past data points.
    struct Rig {
        gnt: Gnt,
        state: SafetyTrainState,
        cab: CabInputs,
        out: ProtectionOutput,
    }

    impl Rig {
        /// A tilting unit at `v_kmh` on a line whose regular profile is 120 km/h.
        fn new(v_kmh: f64) -> Self {
            Self {
                gnt: Gnt::new(),
                state: SafetyTrainState {
                    v_kmh,
                    line_speed: 120.0,
                    train_length: 50.0,
                    brake_percentage: 130.0,
                    ..Default::default()
                },
                cab: CabInputs::default(),
                out: ProtectionOutput::default(),
            }
        }

        fn pass(&mut self, p: GntDataPoint) {
            self.out = self
                .gnt
                .update(0.0, &self.state, &self.cab, &[data_point(p, 0.0)]);
        }

        fn drive(&mut self, meters: f64) {
            let dt = 0.1;
            let step = (self.state.v_kmh / 3.6 * dt).max(1e-9);
            for _ in 0..(meters / step).round().max(1.0) as u32 {
                self.state.odometer += step;
                self.out = self.gnt.update(dt, &self.state, &self.cab, &[]);
            }
        }

        fn run(&mut self, seconds: f64) {
            let dt = 0.1;
            for _ in 0..(seconds / dt).round() as u32 {
                self.out = self.gnt.update(dt, &self.state, &self.cab, &[]);
            }
        }

        fn press(&mut self, set: impl Fn(&mut CabInputs)) {
            set(&mut self.cab);
            self.run(0.1);
            self.cab = CabInputs::default();
            self.run(0.1);
        }

        fn braking(&self) -> bool {
            self.out.action == ProtectionAction::EmergencyBrake
        }
    }

    #[test]
    fn outside_a_gnt_area_the_gnt_supervises_nothing() {
        let mut r = Rig::new(160.0);
        r.drive(500.0);
        assert_eq!(r.gnt.mode(), GntMode::Off);
        assert_eq!(r.out.speed_limit, None);
        assert!(!r.braking());
    }

    /// The point of the whole system: inside a GNT area a tilting unit may run faster than
    /// the regular profile allows.
    #[test]
    fn a_data_point_releases_the_higher_profile() {
        let mut r = Rig::new(140.0);
        r.pass(GntDataPoint::section(150.0, 2000.0));
        assert_eq!(r.gnt.mode(), GntMode::Supervising);
        assert_eq!(r.out.speed_limit, Some(150.0));
        r.drive(500.0);
        assert!(!r.braking(), "140 km/h is within the GNT profile");
    }

    /// A data point without a profile speed only registers the train; the GNT then
    /// supervises the regular profile, and nothing more.
    #[test]
    fn a_data_point_without_a_profile_speed_supervises_the_regular_one() {
        let mut r = Rig::new(110.0);
        r.pass(GntDataPoint::section(0.0, 2000.0));
        assert_eq!(r.gnt.mode(), GntMode::Ready);
        assert_eq!(r.out.speed_limit, Some(120.0), "the regular profile");
    }

    #[test]
    fn exceeding_the_profile_forces_braking() {
        let mut r = Rig::new(140.0);
        r.pass(GntDataPoint::section(150.0, 2000.0));
        assert!(!r.braking());
        r.state.v_kmh = 158.0;
        r.drive(50.0);
        assert!(r.braking(), "the GNT profile is exceeded");
        assert!(r.gnt.tripped());
    }

    /// The braking curve down to the target of the data point is what makes the GNT more
    /// than a speed board: it supervises the approach, not only the point itself.
    #[test]
    fn the_braking_curve_to_the_target_forces_braking() {
        let mut r = Rig::new(150.0);
        r.pass(GntDataPoint {
            target_speed: 60.0,
            target_distance: 1000.0,
            ..GntDataPoint::section(150.0, 2000.0)
        });
        assert_eq!(
            r.out.speed_limit,
            Some(150.0),
            "far away the profile speed stands"
        );
        r.drive(700.0);
        let v = r.gnt.supervised_speed().unwrap();
        // 300 m left to 60 km/h at 130 % BRH: a = 0.91 m/s²,
        // sqrt(16.7² + 2·0.91·300) = 30.0 m/s = 108 km/h.
        assert!(v > 95.0 && v < 120.0, "supervised speed = {v}");
        assert!(r.braking(), "150 km/h is far above the curve");
    }

    /// The curve belongs to the train: a weakly braked train gets a lower curve at the same
    /// distance, exactly as with the LZB.
    #[test]
    fn the_braking_curve_follows_the_brake_assessment() {
        let curve = |brh: f64| {
            let mut r = Rig::new(100.0);
            r.state.brake_percentage = brh;
            r.pass(GntDataPoint {
                target_speed: 0.0,
                target_distance: 800.0,
                ..GntDataPoint::section(160.0, 2000.0)
            });
            r.gnt.supervised_speed().unwrap()
        };
        assert!(curve(180.0) > curve(65.0) + 20.0);
    }

    /// A failed tilting technology takes the release away — but it does not slam the brake
    /// on: the GNT leads the train down onto the regular profile.
    #[test]
    fn a_tilt_failure_leads_back_to_the_regular_profile() {
        let mut r = Rig::new(150.0);
        r.pass(GntDataPoint::section(150.0, 4000.0));
        assert_eq!(r.out.speed_limit, Some(150.0));

        r.gnt.set_tilt_fault(true);
        r.drive(10.0);
        assert_eq!(r.gnt.mode(), GntMode::Fault);
        assert!(!r.braking(), "the return run is supervised, not braked");
        assert!(
            r.gnt.supervised_speed().unwrap() > 120.0,
            "still above the regular profile right after the fault"
        );

        // Run the return distance out at the regular speed and the regular profile is what
        // is left.
        r.state.v_kmh = 120.0;
        r.drive(600.0);
        assert_eq!(r.gnt.supervised_speed(), Some(120.0));
        assert!(!r.braking());
    }

    /// Whoever does not brake during the return run gets the forced braking.
    #[test]
    fn holding_the_gnt_speed_after_a_tilt_failure_forces_braking() {
        let mut r = Rig::new(150.0);
        r.pass(GntDataPoint::section(150.0, 4000.0));
        r.gnt.set_tilt_fault(true);
        r.drive(1500.0);
        assert!(r.braking());
    }

    /// A forced braking runs to a standstill; the acknowledgement button releases it.
    #[test]
    fn the_forced_braking_is_released_at_a_standstill() {
        let mut r = Rig::new(160.0);
        r.pass(GntDataPoint::section(140.0, 2000.0));
        r.drive(50.0);
        assert!(r.braking());
        r.press(|c| c.pzb_acknowledge = true);
        assert!(r.braking(), "not while the train still runs");
        r.state.v_kmh = 0.0;
        r.press(|c| c.pzb_acknowledge = true);
        assert!(!r.braking());
        assert!(!r.gnt.tripped());
    }

    /// The sign-off at the end of the area releases the supervision.
    #[test]
    fn the_end_data_point_signs_the_train_off() {
        let mut r = Rig::new(140.0);
        r.pass(GntDataPoint::section(150.0, 4000.0));
        assert_eq!(r.gnt.mode(), GntMode::Supervising);
        r.pass(GntDataPoint::end());
        assert_eq!(r.gnt.mode(), GntMode::Off);
        assert_eq!(r.out.speed_limit, None);
        r.state.v_kmh = 160.0;
        r.drive(200.0);
        assert!(!r.braking(), "the GNT no longer supervises");
    }

    /// Without a sign-off the section simply runs out — and only once the rear of the train
    /// has left it, which is what the train length in the train data is for.
    #[test]
    fn the_section_ends_behind_the_rear_of_the_train() {
        let mut r = Rig::new(140.0);
        r.state.train_length = 200.0;
        r.pass(GntDataPoint::section(150.0, 1000.0));
        r.drive(1100.0);
        assert_eq!(
            r.gnt.mode(),
            GntMode::Supervising,
            "the tail is still in the section"
        );
        r.drive(200.0);
        assert_eq!(r.gnt.mode(), GntMode::Off);
    }

    /// While the LZB guides, its authority is the binding one.
    #[test]
    fn the_gnt_stands_down_under_lzb_guidance() {
        let mut r = Rig::new(140.0);
        r.pass(GntDataPoint::section(150.0, 4000.0));
        r.gnt.stand_by(true);
        r.drive(10.0);
        assert_eq!(r.gnt.mode(), GntMode::Off);
        assert_eq!(r.out.speed_limit, None);
    }

    /// A balise that is not a GNT data point is not one — the payload decides, so the same
    /// device kind can carry ETCS telegrams later.
    #[test]
    fn a_foreign_balise_payload_is_ignored() {
        let mut r = Rig::new(140.0);
        r.out = r.gnt.update(
            0.1,
            &r.state,
            &r.cab,
            &[TracksideEvent {
                device: DeviceKind::Balise,
                payload: "(nid_c:1,nid_bg:42)".to_string(),
                s_offset: 0.0,
                active: true,
            }],
        );
        assert_eq!(r.gnt.mode(), GntMode::Off);
    }

    #[test]
    fn the_isolating_switch_silences_the_gnt() {
        let mut r = Rig::new(160.0);
        r.pass(GntDataPoint::section(140.0, 2000.0));
        r.drive(50.0);
        assert!(r.braking());
        r.gnt.isolate(true);
        r.drive(10.0);
        assert!(!r.braking());
        assert!(r.gnt.is_isolated());
    }

    #[test]
    fn switching_on_drops_the_section_data() {
        let mut r = Rig::new(140.0);
        r.pass(GntDataPoint::section(150.0, 4000.0));
        assert_eq!(r.gnt.mode(), GntMode::Supervising);
        r.gnt.power_on();
        r.drive(10.0);
        assert_eq!(r.gnt.mode(), GntMode::Off);
    }
}
