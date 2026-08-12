//! PZB 90 (intermittent train protection), complete on-board logic (plan 9.3).
//!
//! On the trackside, 500 Hz, 1000 Hz and 2000 Hz track magnets are effective; their
//! activation depends on the signal aspect and is decided by the interlocking
//! (`TracksideEvent::active`).
//!
//! Numeric values according to Ril 483.0111 (PZB 90, train categories O/M/U).

use crate::cab::{CabInputs, Edge};
use crate::safety::{
    Indicator, LampState, ProtectionAction, ProtectionOutput, SafetyTrainState, TracksideEvent,
    TrainProtectionSystem,
};
use serde::{Deserialize, Serialize};
use track_model::DeviceKind;

/// Frequency of a track magnet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MagnetFrequency {
    Hz500,
    Hz1000,
    Hz2000,
}

/// Payload of a magnet trackside device.
///
/// `signal`/`activation` are read by the interlocking (`interlock::DeviceLink`),
/// `frequency` by the PZB — each side ignores the other's fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MagnetPayload {
    pub frequency: MagnetFrequency,
    /// Associated signal whose aspect decides the activation.
    #[serde(default)]
    pub signal: Option<u32>,
    /// When the magnet is active.
    #[serde(default)]
    pub activation: crate::interlock::Activation,
}

impl MagnetPayload {
    /// 1000 Hz magnet at the distant signal — active with an announced restriction.
    pub fn hz1000(signal: u32) -> Self {
        Self {
            frequency: MagnetFrequency::Hz1000,
            signal: Some(signal),
            activation: crate::interlock::Activation::WhenRestrictive,
        }
    }

    /// 500 Hz magnet ahead of the main signal — active at stop.
    pub fn hz500(signal: u32) -> Self {
        Self {
            frequency: MagnetFrequency::Hz500,
            signal: Some(signal),
            activation: crate::interlock::Activation::WhenStop,
        }
    }

    /// 2000 Hz magnet at the main signal — active at stop.
    pub fn hz2000(signal: u32) -> Self {
        Self {
            frequency: MagnetFrequency::Hz2000,
            signal: Some(signal),
            activation: crate::interlock::Activation::WhenStop,
        }
    }
}

/// Train category by braked weight percentage / maximum speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TrainType {
    /// Upper train category (fast passenger trains).
    #[default]
    O,
    /// Middle train category.
    M,
    /// Lower train category (freight trains).
    U,
}

/// Supervision parameters of a train category.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PzbParams {
    pub v1000_start: f64,
    pub v1000_end: f64,
    /// Duration of the 1000 Hz braking curve [s].
    pub t1000: f64,
    pub v500_start: f64,
    pub v500_end: f64,
    /// Indicator lamp label of the train category.
    pub lamp: &'static str,
}

impl TrainType {
    pub fn params(self) -> PzbParams {
        match self {
            TrainType::O => PzbParams {
                v1000_start: 165.0,
                v1000_end: 85.0,
                t1000: 23.0,
                v500_start: 65.0,
                v500_end: 45.0,
                lamp: "85",
            },
            TrainType::M => PzbParams {
                v1000_start: 125.0,
                v1000_end: 70.0,
                t1000: 29.0,
                v500_start: 50.0,
                v500_end: 35.0,
                lamp: "70",
            },
            TrainType::U => PzbParams {
                v1000_start: 105.0,
                v1000_end: 55.0,
                t1000: 38.0,
                v500_start: 40.0,
                v500_end: 25.0,
                lamp: "55",
            },
        }
    }
}

/// Supervision speed of the restrictive supervision [km/h].
pub const V_RESTRICTIVE: f64 = 45.0;
/// Restrictive 500 Hz curve: from 45 down to 25 km/h.
pub const V_RESTRICTIVE_500_END: f64 = 25.0;
/// Supervised speed with the override (Befehl 40) [km/h].
pub const V_OVERRIDE: f64 = 40.0;
/// Length of the 1000 Hz supervision [m].
pub const D_1000: f64 = 1250.0;
/// Length of the 500 Hz supervision [m].
pub const D_500: f64 = 250.0;
/// Distance of the 500 Hz braking curve [m].
pub const D_500_CURVE: f64 = 153.0;
/// From this distance within the 1000 Hz supervision an exemption is permitted [m].
pub const D_EXEMPT: f64 = 700.0;
/// Time allowed for the acknowledgement after a 1000 Hz influence [s].
pub const T_ACKNOWLEDGE: f64 = 4.0;
/// From this time running below 10 km/h the restrictive supervision takes effect [s].
pub const T_SLOW: f64 = 15.0;
/// Speed threshold for the restrictive supervision [km/h].
pub const V_SLOW: f64 = 10.0;

/// Trigger of a forced braking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PzbTrip {
    /// Acknowledgement not given within 4 s.
    MissingAcknowledge,
    /// Supervision speed exceeded.
    Overspeed,
    /// 2000 Hz influence (signal showing stop).
    Magnet2000,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
struct Monitor1000 {
    start_odo: f64,
    elapsed: f64,
    acknowledged: bool,
    ack_timer: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
struct Monitor500 {
    start_odo: f64,
}

/// The PZB 90 on-board equipment.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Pzb90 {
    pub train_type: TrainType,
    isolated: bool,
    m1000: Option<Monitor1000>,
    m500: Option<Monitor500>,
    /// Restrictive supervision active.
    restrictive: bool,
    /// Override (Befehl 40) button pressed.
    override_40: bool,
    trip: Option<PzbTrip>,
    /// Time below 10 km/h within a supervision [s].
    slow_timer: f64,
    acknowledge: Edge,
    exempt: Edge,
    /// Current supervision speed [km/h], as computed last.
    limit: Option<f64>,
}

impl Pzb90 {
    pub fn new(train_type: TrainType) -> Self {
        Self {
            train_type,
            ..Default::default()
        }
    }

    pub fn trip(&self) -> Option<PzbTrip> {
        self.trip
    }

    pub fn is_restrictive(&self) -> bool {
        self.restrictive
    }

    /// Current supervision speed [km/h], if a supervision is running.
    pub fn supervised_speed(&self) -> Option<f64> {
        self.limit
    }

    pub fn monitoring_1000(&self) -> bool {
        self.m1000.is_some()
    }

    pub fn monitoring_500(&self) -> bool {
        self.m500.is_some()
    }

    /// Is the exemption currently permitted?
    pub fn release_allowed(&self, odometer: f64) -> bool {
        self.m1000
            .is_some_and(|m| odometer - m.start_odo >= D_EXEMPT)
            && !self.restrictive
    }

    fn params(&self) -> PzbParams {
        self.train_type.params()
    }

    /// Supervision speed derived from all active influences.
    fn compute_limit(&self, odometer: f64) -> Option<f64> {
        let p = self.params();
        let mut limit: Option<f64> = None;
        let mut take = |v: f64| {
            limit = Some(limit.map_or(v, |l: f64| l.min(v)));
        };

        if let Some(m) = self.m1000 {
            if self.restrictive {
                take(V_RESTRICTIVE);
            } else {
                let t = (m.elapsed / p.t1000).clamp(0.0, 1.0);
                take(p.v1000_start + (p.v1000_end - p.v1000_start) * t);
            }
        }
        if let Some(m) = self.m500 {
            let d = ((odometer - m.start_odo) / D_500_CURVE).clamp(0.0, 1.0);
            let (start, end) = if self.restrictive {
                (V_RESTRICTIVE, V_RESTRICTIVE_500_END)
            } else {
                (p.v500_start, p.v500_end)
            };
            take(start + (end - start) * d);
        }
        if self.override_40 {
            take(V_OVERRIDE);
        }
        limit
    }

    fn handle_event(&mut self, event: &TracksideEvent, odometer: f64) {
        if event.device != DeviceKind::Magnet || !event.active {
            return;
        }
        let Some(payload) = ron_payload(event) else {
            return;
        };
        // The magnet already lies `s_offset` behind the antenna.
        let start_odo = odometer - event.s_offset;
        match payload.frequency {
            MagnetFrequency::Hz1000 => {
                self.m1000 = Some(Monitor1000 {
                    start_odo,
                    elapsed: 0.0,
                    acknowledged: false,
                    ack_timer: 0.0,
                });
                self.slow_timer = 0.0;
            }
            MagnetFrequency::Hz500 => {
                self.m500 = Some(Monitor500 { start_odo });
            }
            MagnetFrequency::Hz2000 => {
                // The override (Befehl 40) suppresses the 2000 Hz influence.
                if !self.override_40 {
                    self.trip = Some(PzbTrip::Magnet2000);
                }
            }
        }
    }
}

fn ron_payload(event: &TracksideEvent) -> Option<MagnetPayload> {
    ron::from_str::<MagnetPayload>(&event.payload).ok()
}

impl TrainProtectionSystem for Pzb90 {
    fn update(
        &mut self,
        dt: f64,
        train: &SafetyTrainState,
        cab: &CabInputs,
        events: &[TracksideEvent],
    ) -> ProtectionOutput {
        if self.isolated {
            *self = Self {
                train_type: self.train_type,
                isolated: true,
                ..Default::default()
            };
            return ProtectionOutput::default();
        }

        self.override_40 = cab.pzb_override;
        let acknowledge = self.acknowledge.rising(cab.pzb_acknowledge);
        let exempt = self.exempt.rising(cab.pzb_exempt);

        for e in events {
            self.handle_event(e, train.odometer);
        }

        // Acknowledgement.
        if acknowledge {
            if let Some(m) = &mut self.m1000 {
                m.acknowledged = true;
            }
            if self.trip.is_some() && train.standstill() {
                // Release the forced braking — afterwards the restrictive supervision applies.
                self.trip = None;
                self.restrictive = true;
                if self.m1000.is_none() {
                    self.m1000 = Some(Monitor1000 {
                        start_odo: train.odometer,
                        elapsed: 0.0,
                        acknowledged: true,
                        ack_timer: 0.0,
                    });
                }
            }
        }

        // Exemption (Frei button) from 700 m onwards, not in restrictive supervision.
        if exempt && self.release_allowed(train.odometer) {
            self.m1000 = None;
            self.slow_timer = 0.0;
        }

        // Advance times and distances.
        if let Some(m) = &mut self.m1000 {
            m.elapsed += dt;
            if !m.acknowledged {
                m.ack_timer += dt;
                if m.ack_timer > T_ACKNOWLEDGE {
                    self.trip = Some(PzbTrip::MissingAcknowledge);
                }
            }
            if train.odometer - m.start_odo > D_1000 {
                self.m1000 = None;
                self.restrictive = false;
                self.slow_timer = 0.0;
            }
        }
        if let Some(m) = self.m500
            && train.odometer - m.start_odo > D_500
        {
            self.m500 = None;
        }

        // Restrictive supervision: stop, or running longer than 15 s below 10 km/h
        // within a supervision.
        if self.m1000.is_some() || self.m500.is_some() {
            if train.v_kmh < V_SLOW {
                self.slow_timer += dt;
                if train.standstill() || self.slow_timer > T_SLOW {
                    self.restrictive = true;
                }
            } else {
                self.slow_timer = 0.0;
            }
        } else {
            self.restrictive = false;
            self.slow_timer = 0.0;
        }

        // Speed supervision.
        let limit = self.compute_limit(train.odometer);
        self.limit = limit;
        if let Some(l) = limit
            && train.v_kmh > l + 0.5
            && self.trip.is_none()
        {
            self.trip = Some(PzbTrip::Overspeed);
        }

        ProtectionOutput {
            action: if self.trip.is_some() {
                ProtectionAction::EmergencyBrake
            } else {
                ProtectionAction::None
            },
            speed_limit: limit,
            alert: self.m1000.is_some_and(|m| !m.acknowledged) || self.trip.is_some(),
            ..Default::default()
        }
    }

    fn indicators(&self) -> Vec<Indicator> {
        let p = self.params();
        vec![
            Indicator::lamp("pzb_1000hz", self.m1000.is_some()),
            Indicator::lamp("pzb_500hz", self.m500.is_some()),
            Indicator::lamp("pzb_befehl", self.override_40),
            Indicator::state(
                p.lamp,
                if self.restrictive {
                    LampState::Blinking
                } else if self.m1000.is_some_and(|m| m.acknowledged) {
                    LampState::On
                } else {
                    LampState::Off
                },
            ),
        ]
    }

    fn isolate(&mut self, isolated: bool) {
        self.isolated = isolated;
    }

    fn is_isolated(&self) -> bool {
        self.isolated
    }

    fn name(&self) -> &'static str {
        "PZB 90"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Small test rig: runs the train at constant speed with button presses.
    struct Rig {
        pzb: Pzb90,
        state: SafetyTrainState,
        cab: CabInputs,
        out: ProtectionOutput,
    }

    impl Rig {
        fn new(train_type: TrainType, v_kmh: f64) -> Self {
            Self {
                pzb: Pzb90::new(train_type),
                state: SafetyTrainState {
                    v_kmh,
                    odometer: 0.0,
                    line_speed: 160.0,
                    braking: false,
                },
                cab: CabInputs::default(),
                out: ProtectionOutput::default(),
            }
        }

        fn magnet(&mut self, frequency: MagnetFrequency) {
            let payload = ron::to_string(&MagnetPayload {
                frequency,
                signal: None,
                activation: crate::interlock::Activation::Always,
            })
            .unwrap();
            let event = TracksideEvent {
                device: DeviceKind::Magnet,
                payload,
                s_offset: 0.0,
                active: true,
            };
            self.out = self.pzb.update(0.0, &self.state, &self.cab, &[event]);
        }

        /// Runs on for `seconds` seconds (distance derived from the speed).
        fn run(&mut self, seconds: f64) {
            let dt = 0.05;
            for _ in 0..(seconds / dt).round() as u32 {
                self.state.odometer += self.state.v_kmh / 3.6 * dt;
                self.out = self.pzb.update(dt, &self.state, &self.cab, &[]);
            }
        }

        /// Runs on for `meters` metres.
        fn drive(&mut self, meters: f64) {
            if self.state.v_kmh <= 0.0 {
                return;
            }
            self.run(meters / (self.state.v_kmh / 3.6));
        }

        fn press(&mut self, set: impl Fn(&mut CabInputs)) {
            set(&mut self.cab);
            self.run(0.1);
            self.cab = CabInputs {
                afb_target: self.cab.afb_target,
                ..Default::default()
            };
            self.run(0.05);
        }

        fn acknowledge(&mut self) {
            self.press(|c| c.pzb_acknowledge = true);
        }

        fn exempt(&mut self) {
            self.press(|c| c.pzb_exempt = true);
        }

        fn braking(&self) -> bool {
            self.out.action == ProtectionAction::EmergencyBrake
        }
    }

    #[test]
    fn thousand_hertz_without_acknowledge_forces_braking() {
        let mut r = Rig::new(TrainType::O, 120.0);
        r.magnet(MagnetFrequency::Hz1000);
        r.run(3.0);
        assert!(!r.braking(), "no forced braking within 4 s yet");
        r.run(2.0);
        assert!(r.braking());
        assert_eq!(r.pzb.trip(), Some(PzbTrip::MissingAcknowledge));
    }

    #[test]
    fn thousand_hertz_with_acknowledge_supervises_braking_curve() {
        let mut r = Rig::new(TrainType::O, 120.0);
        r.magnet(MagnetFrequency::Hz1000);
        r.acknowledge();
        r.run(3.0);
        assert!(!r.braking());
        // Curve 165 → 85 km/h in 23 s: after 20 s the limit is at ~ 95 km/h.
        let limit = r.pzb.supervised_speed().unwrap();
        assert!(limit > 130.0 && limit < 160.0, "after 3 s: {limit}");
        r.run(17.0);
        let limit = r.pzb.supervised_speed().unwrap();
        assert!(limit > 85.0 && limit < 100.0, "after 20 s: {limit}");
        // At 120 km/h the curve is exceeded as soon as it drops below 120.
        r.run(10.0);
        assert!(r.braking(), "braking curve exceeded");
        assert_eq!(r.pzb.trip(), Some(PzbTrip::Overspeed));
    }

    #[test]
    fn thousand_hertz_exemption_only_from_700_m() {
        let mut r = Rig::new(TrainType::O, 80.0);
        r.magnet(MagnetFrequency::Hz1000);
        r.acknowledge();
        r.drive(600.0);
        r.exempt();
        assert!(r.pzb.monitoring_1000(), "no exemption before 700 m");
        r.drive(150.0);
        r.exempt();
        assert!(!r.pzb.monitoring_1000(), "exemption possible from 700 m");
        assert!(r.pzb.supervised_speed().is_none());
    }

    #[test]
    fn thousand_hertz_supervision_ends_after_1250_m() {
        let mut r = Rig::new(TrainType::O, 80.0);
        r.magnet(MagnetFrequency::Hz1000);
        r.acknowledge();
        r.drive(1300.0);
        assert!(!r.pzb.monitoring_1000());
        assert!(!r.braking());
    }

    #[test]
    fn five_hundred_hertz_supervises_immediately_without_exemption() {
        let mut r = Rig::new(TrainType::O, 60.0);
        r.magnet(MagnetFrequency::Hz500);
        // Immediate supervision: 65 km/h falling, no acknowledgement required.
        assert!(!r.braking());
        r.drive(50.0);
        r.exempt();
        assert!(r.pzb.monitoring_500(), "500 Hz knows no exemption");
        // After 153 m the limit is at 45 km/h → 60 km/h trips it.
        r.drive(120.0);
        assert!(r.braking());
        assert_eq!(r.pzb.trip(), Some(PzbTrip::Overspeed));
    }

    #[test]
    fn two_thousand_hertz_brakes_immediately_override_suppresses() {
        let mut r = Rig::new(TrainType::O, 60.0);
        r.magnet(MagnetFrequency::Hz2000);
        assert!(r.braking());
        assert_eq!(r.pzb.trip(), Some(PzbTrip::Magnet2000));

        // With the override button pressed there is no influence, 40 km/h are supervised.
        let mut r = Rig::new(TrainType::O, 35.0);
        r.cab.pzb_override = true;
        r.run(0.1);
        r.magnet(MagnetFrequency::Hz2000);
        r.run(1.0);
        assert!(!r.braking());
        assert_eq!(r.pzb.supervised_speed(), Some(V_OVERRIDE));
    }

    #[test]
    fn restrictive_supervision_after_stop() {
        let mut r = Rig::new(TrainType::O, 80.0);
        r.magnet(MagnetFrequency::Hz1000);
        r.acknowledge();
        r.drive(200.0);
        r.state.v_kmh = 0.0;
        r.run(1.0);
        assert!(r.pzb.is_restrictive());
        assert_eq!(r.pzb.supervised_speed(), Some(V_RESTRICTIVE));
        // Accelerating to 50 km/h triggers the forced braking.
        r.state.v_kmh = 50.0;
        r.run(0.5);
        assert!(r.braking());
    }

    #[test]
    fn restrictive_supervision_after_15_s_below_10_kmh() {
        let mut r = Rig::new(TrainType::O, 8.0);
        r.magnet(MagnetFrequency::Hz1000);
        r.acknowledge();
        r.run(10.0);
        assert!(!r.pzb.is_restrictive());
        r.run(7.0);
        assert!(r.pzb.is_restrictive());
    }

    #[test]
    fn forced_braking_releases_only_at_standstill() {
        let mut r = Rig::new(TrainType::O, 60.0);
        r.magnet(MagnetFrequency::Hz2000);
        assert!(r.braking());
        r.acknowledge();
        assert!(r.braking(), "no release while running");
        r.state.v_kmh = 0.0;
        r.run(0.5);
        r.acknowledge();
        assert!(
            !r.braking(),
            "releasable at standstill with the Wachsam button"
        );
        assert!(r.pzb.is_restrictive(), "restrictive supervision afterwards");
    }

    #[test]
    fn train_category_u_has_lower_check_speeds() {
        let mut r = Rig::new(TrainType::U, 50.0);
        r.magnet(MagnetFrequency::Hz500);
        r.drive(160.0);
        // U: 40 → 25 km/h; at 50 km/h immediately too fast.
        assert!(r.braking());
    }

    #[test]
    fn inactive_magnet_triggers_nothing() {
        let mut r = Rig::new(TrainType::O, 100.0);
        let payload = ron::to_string(&MagnetPayload::hz2000(0)).unwrap();
        let event = TracksideEvent {
            device: DeviceKind::Magnet,
            payload,
            s_offset: 0.0,
            active: false,
        };
        let out = r.pzb.update(0.1, &r.state, &r.cab, &[event]);
        assert_eq!(out.action, ProtectionAction::None);
    }
}
