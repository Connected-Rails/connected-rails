//! Indusi / PZB — intermittent train protection, complete on-board logic (plan 9.3).
//!
//! All German and Austrian intermittent systems descend from the same Indusi principle
//! (1000/500/2000 Hz track magnets, three train categories, acknowledgement within 4 s).
//! They differ only in *which* supervisions are derived from an influence. Therefore one
//! state machine [`Pzb`] plus a per-variant parameter set [`PzbSpec`] covers all of them:
//!
//! | Variant | 1000 Hz | check speeds O/M/U | 500 Hz | Distance sup. | Restrictive | Function test |
//! |---|---|---|---|---|---|---|
//! | [`PzbVariant::I54`]     | time step | 95/90/80 after 20 s | fixed | — | — | — |
//! | [`PzbVariant::I60`]     | time step | 95/75/60 after 20/26/34 s | fixed | — | — | — |
//! | [`PzbVariant::I60M`]    | time step | as I 60 | fixed | — | — | yes |
//! | [`PzbVariant::I60R`]    | ramp | as I 60 | fixed | 1250 m | yes | yes |
//! | [`PzbVariant::Pzb60`]   | time step | as I 60 | fixed | — | — | — |
//! | [`PzbVariant::Pzb90V15`]| ramp | 85/70/55 over 23/29/38 s | ramp 153 m | 1250 m | after a stop | yes |
//! | [`PzbVariant::Pzb90V20`]| ramp | 85/70/55 over 23/29/38 s | ramp 153 m | 1250 m | + 15 s < 10 km/h | yes |
//!
//! On the trackside, 500 Hz, 1000 Hz and 2000 Hz track magnets are effective; their
//! activation depends on the signal aspect and is decided by the interlocking
//! (`TracksideEvent::active`).
//!
//! Every build carries the check speeds of its own time: those of the PZB 90 follow
//! Ril 483.0111, the older ones the Indusi rulebooks they were built to — the harmonisation
//! down to 85/70/55 came with the PZB 90, so an I 60 loco runs 10 km/h faster past a
//! 1000 Hz magnet than the same loco rebuilt. Country packages are compile-time Rust
//! (plan 9.1), so the parameter sets are `const` tables rather than data files.

use crate::cab::{CabInputs, Edge};
use crate::safety::{
    Indicator, LampState, ProtectionAction, ProtectionOutput, SafetyTrainState, SelfTest,
    TracksideEvent, TrainProtectionSystem,
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

/// Build state of the on-board equipment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PzbVariant {
    /// Indusi I 54 — relay technology, pure time supervision, no restrictive mode.
    I54,
    /// Indusi I 60 — the electronic successor, same supervision logic as the I 54.
    I60,
    /// Indusi I 60 Mikrorechner — I 60 logic on a microprocessor, hence with a function test.
    I60M,
    /// Indusi I 60R — I 60 retrofitted with the restrictive programme (interim build
    /// towards the PZB 90): distance supervision and restrictive mode, but the I 60
    /// time supervision of the 1000 Hz influence.
    I60R,
    /// ÖBB PZB 60 — the Austrian Indusi build.
    Pzb60,
    /// PZB 90 version 1.5 — restrictive mode only after a stop.
    Pzb90V15,
    /// PZB 90 version 2.0 — today's standard.
    #[default]
    Pzb90V20,
}

/// Supervision parameters of a train category.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PzbParams {
    /// Start of the 1000 Hz braking curve [km/h] (ramp variants only).
    pub v1000_start: f64,
    /// Check speed of the 1000 Hz influence [km/h].
    pub v1000_end: f64,
    /// Duration of the 1000 Hz supervision [s].
    pub t1000: f64,
    /// 500 Hz check speed at the magnet [km/h].
    pub v500_start: f64,
    /// 500 Hz check speed at the end of the curve [km/h] (ramp variants only).
    pub v500_end: f64,
    /// Indicator lamp label of the train category.
    pub lamp: &'static str,
}

/// How the 1000 Hz influence supervises the speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Curve1000 {
    /// PZB 90: braking curve falling continuously from `v1000_start` to `v1000_end`.
    Ramp,
    /// Indusi I 54/I 60: no supervision while the time runs, the check speed applies from
    /// the moment it has elapsed.
    Step,
}

/// How the 500 Hz influence supervises the speed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Curve500 {
    /// PZB 90: braking curve from `v500_start` to `v500_end` over this distance [m].
    Ramp(f64),
    /// Indusi I 54/I 60: `v500_start` applies immediately and stays.
    Step,
}

/// Parameters of the restrictive supervision.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Restrictive {
    /// Supervision speed of the restrictive mode [km/h].
    pub v: f64,
    /// End of the restrictive 500 Hz curve [km/h].
    pub v_500_end: f64,
    /// The restrictive mode also takes effect after running this long below `v_slow` [s];
    /// `None` = only after a stop (PZB 90 V1.5 and older).
    pub t_slow: Option<f64>,
    /// Speed threshold for `t_slow` [km/h].
    pub v_slow: f64,
}

/// The complete parameter set of one build state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PzbSpec {
    pub o: PzbParams,
    pub m: PzbParams,
    pub u: PzbParams,
    pub curve_1000: Curve1000,
    pub curve_500: Curve500,
    /// The 1000 Hz supervision ends after this distance [m]; `None` = only the release
    /// button ends it (Indusi I 54/I 60).
    pub d_1000: Option<f64>,
    /// Length of the 500 Hz supervision [m].
    pub d_500: f64,
    /// The release button is permitted from this distance after the 1000 Hz magnet [m].
    pub d_exempt: Option<f64>,
    /// Time allowed for the acknowledgement after a 1000 Hz influence [s].
    pub t_acknowledge: f64,
    pub restrictive: Option<Restrictive>,
    /// Speed supervised while the override button is pressed [km/h]; `None` = the older
    /// override button merely suppresses the 2000 Hz influence.
    pub v_override: Option<f64>,
    /// The device runs a function test when it is switched on.
    pub self_test: bool,
}

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
/// Supervision speed of the restrictive supervision [km/h].
pub const V_RESTRICTIVE: f64 = 45.0;
/// Restrictive 500 Hz curve: from 45 down to 25 km/h.
pub const V_RESTRICTIVE_500_END: f64 = 25.0;
/// Supervised speed with the override (Befehl 40) [km/h].
pub const V_OVERRIDE: f64 = 40.0;
/// From this time running below 10 km/h the restrictive supervision takes effect [s].
pub const T_SLOW: f64 = 15.0;
/// Speed threshold for the restrictive supervision [km/h].
pub const V_SLOW: f64 = 10.0;

/// Train categories of the German Indusi builds I 60 … I 60R — the check speeds of the
/// Indusi rulebooks of their own time (95/75/60 after 20/26/34 s), which the PZB 90 only
/// later harmonised down to 85/70/55.
const INDUSI_DE: (PzbParams, PzbParams, PzbParams) = (
    PzbParams {
        v1000_start: 165.0,
        v1000_end: 95.0,
        t1000: 20.0,
        v500_start: 65.0,
        v500_end: 65.0,
        lamp: "95",
    },
    PzbParams {
        v1000_start: 125.0,
        v1000_end: 75.0,
        t1000: 26.0,
        v500_start: 50.0,
        v500_end: 50.0,
        lamp: "75",
    },
    PzbParams {
        v1000_start: 105.0,
        v1000_end: 60.0,
        t1000: 34.0,
        v500_start: 40.0,
        v500_end: 40.0,
        lamp: "60",
    },
);

/// Train categories of the Indusi I 54.
///
/// ponytail: from 1959 the I 54 was set by the vehicle's maximum speed, not by train
/// category — over 120 km/h 95, 100…120 km/h 90, under 100 km/h 80, each supervised 20 s
/// after the influence. The two axes line up closely enough that the train type carries it;
/// a vehicle-speed setting of its own would need the device to be told the vehicle's v-max.
/// The earlier state of the rulebook, with only 95 and 75, is not modelled.
const I54_DE: (PzbParams, PzbParams, PzbParams) = (
    INDUSI_DE.0,
    PzbParams {
        v1000_end: 90.0,
        t1000: 20.0,
        lamp: "90",
        ..INDUSI_DE.1
    },
    PzbParams {
        v1000_end: 80.0,
        t1000: 20.0,
        lamp: "80",
        ..INDUSI_DE.2
    },
);

/// Train categories of the PZB 90 — the harmonised check speeds, and as braking curves.
const PZB90_DE: (PzbParams, PzbParams, PzbParams) = (
    PzbParams {
        v1000_end: 85.0,
        t1000: 23.0,
        v500_end: 45.0,
        lamp: "85",
        ..INDUSI_DE.0
    },
    PzbParams {
        v1000_end: 70.0,
        t1000: 29.0,
        v500_end: 35.0,
        lamp: "70",
        ..INDUSI_DE.1
    },
    PzbParams {
        v1000_end: 55.0,
        t1000: 38.0,
        v500_end: 25.0,
        lamp: "55",
        ..INDUSI_DE.2
    },
);

/// Base of every Indusi build: 1000/500/2000 Hz, acknowledgement within 4 s, release button.
const INDUSI_BASE: PzbSpec = PzbSpec {
    o: INDUSI_DE.0,
    m: INDUSI_DE.1,
    u: INDUSI_DE.2,
    curve_1000: Curve1000::Step,
    curve_500: Curve500::Step,
    d_1000: None,
    d_500: D_500,
    d_exempt: Some(D_EXEMPT),
    t_acknowledge: T_ACKNOWLEDGE,
    restrictive: None,
    v_override: None,
    self_test: false,
};

/// The restrictive supervision as the PZB 90 V2.0 defines it.
const RESTRICTIVE_V20: Restrictive = Restrictive {
    v: V_RESTRICTIVE,
    v_500_end: V_RESTRICTIVE_500_END,
    t_slow: Some(T_SLOW),
    v_slow: V_SLOW,
};

impl PzbVariant {
    pub fn spec(self) -> PzbSpec {
        match self {
            PzbVariant::I54 => PzbSpec {
                o: I54_DE.0,
                m: I54_DE.1,
                u: I54_DE.2,
                ..INDUSI_BASE
            },
            PzbVariant::I60 => INDUSI_BASE,
            PzbVariant::I60M => PzbSpec {
                self_test: true,
                ..INDUSI_BASE
            },
            // The restrictive programme brought the distance supervision and the supervised
            // override with it. Being computer-controlled, the I 60R also replaced the check
            // point of the 1000 Hz influence with a curve — but onto the I 60 check speed,
            // 165 down to 95 km/h over the I 60 supervision time.
            PzbVariant::I60R => PzbSpec {
                curve_1000: Curve1000::Ramp,
                d_1000: Some(D_1000),
                restrictive: Some(Restrictive {
                    t_slow: None,
                    ..RESTRICTIVE_V20
                }),
                v_override: Some(V_OVERRIDE),
                self_test: true,
                ..INDUSI_BASE
            },
            // ponytail: no figures of the ÖBB PZB 60 are published, so it runs on the
            // contemporary German Indusi set — which is what it was built from.
            PzbVariant::Pzb60 => INDUSI_BASE,
            // V1.5: the restrictive mode is only entered after a stop and supervises a
            // constant 45 km/h, the 500 Hz curve included.
            PzbVariant::Pzb90V15 => PzbSpec {
                restrictive: Some(Restrictive {
                    t_slow: None,
                    v_500_end: V_RESTRICTIVE,
                    ..RESTRICTIVE_V20
                }),
                ..PzbVariant::Pzb90V20.spec()
            },
            PzbVariant::Pzb90V20 => PzbSpec {
                o: PZB90_DE.0,
                m: PZB90_DE.1,
                u: PZB90_DE.2,
                curve_1000: Curve1000::Ramp,
                curve_500: Curve500::Ramp(D_500_CURVE),
                d_1000: Some(D_1000),
                restrictive: Some(RESTRICTIVE_V20),
                v_override: Some(V_OVERRIDE),
                self_test: true,
                ..INDUSI_BASE
            },
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            PzbVariant::I54 => "Indusi I 54",
            PzbVariant::I60 => "Indusi I 60",
            PzbVariant::I60M => "Indusi I 60M",
            PzbVariant::I60R => "Indusi I 60R",
            PzbVariant::Pzb60 => "PZB 60 (ÖBB)",
            PzbVariant::Pzb90V15 => "PZB 90 V1.5",
            PzbVariant::Pzb90V20 => "PZB 90 V2.0",
        }
    }
}

impl PzbSpec {
    pub fn params(&self, train_type: TrainType) -> PzbParams {
        match train_type {
            TrainType::O => self.o,
            TrainType::M => self.m,
            TrainType::U => self.u,
        }
    }
}

impl TrainType {
    /// Parameters of the current standard build (PZB 90 V2.0).
    pub fn params(self) -> PzbParams {
        PzbVariant::Pzb90V20.spec().params(self)
    }
}

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

/// The Indusi/PZB on-board equipment.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Pzb {
    pub variant: PzbVariant,
    pub train_type: TrainType,
    isolated: bool,
    /// Function test after switching on.
    test: SelfTest,
    m1000: Option<Monitor1000>,
    m500: Option<Monitor500>,
    /// Restrictive supervision active.
    restrictive: bool,
    /// Override (Befehl 40) button pressed.
    override_40: bool,
    trip: Option<PzbTrip>,
    /// Time below the restrictive threshold within a supervision [s].
    slow_timer: f64,
    acknowledge: Edge,
    exempt: Edge,
    /// Current supervision speed [km/h], as computed last.
    limit: Option<f64>,
}

/// The PZB 90 — kept as a name of its own because most vehicles carry exactly this build.
pub type Pzb90 = Pzb;

impl Pzb {
    /// Standard build (PZB 90 V2.0), already function-tested.
    pub fn new(train_type: TrainType) -> Self {
        Self::with_variant(PzbVariant::Pzb90V20, train_type)
    }

    /// A specific build, already function-tested (vehicle set up before the scenario).
    pub fn with_variant(variant: PzbVariant, train_type: TrainType) -> Self {
        Self {
            variant,
            train_type,
            ..Default::default()
        }
    }

    /// Switches the device on: the function test starts (plan 9.3).
    pub fn power_on(&mut self) {
        if self.spec().self_test {
            self.test.restart();
        }
    }

    pub fn spec(&self) -> PzbSpec {
        self.variant.spec()
    }

    pub fn trip(&self) -> Option<PzbTrip> {
        self.trip
    }

    pub fn is_restrictive(&self) -> bool {
        self.restrictive
    }

    /// State of the function test.
    pub fn self_test(&self) -> SelfTest {
        self.test
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
        let Some(d) = self.spec().d_exempt else {
            return false;
        };
        self.m1000.is_some_and(|m| odometer - m.start_odo >= d) && !self.restrictive
    }

    fn params(&self) -> PzbParams {
        self.spec().params(self.train_type)
    }

    /// Supervision speed derived from all active influences.
    fn compute_limit(&self, odometer: f64) -> Option<f64> {
        let spec = self.spec();
        let p = self.params();
        let mut limit: Option<f64> = None;
        let mut take = |v: f64| {
            limit = Some(limit.map_or(v, |l: f64| l.min(v)));
        };

        if let Some(m) = self.m1000 {
            match (self.restrictive, spec.restrictive) {
                (true, Some(r)) => take(r.v),
                _ => match spec.curve_1000 {
                    Curve1000::Ramp => {
                        let t = (m.elapsed / p.t1000).clamp(0.0, 1.0);
                        take(p.v1000_start + (p.v1000_end - p.v1000_start) * t);
                    }
                    // Older Indusi: the check speed only has to be met once the time has run.
                    Curve1000::Step if m.elapsed >= p.t1000 => take(p.v1000_end),
                    Curve1000::Step => {}
                },
            }
        }
        if let Some(m) = self.m500 {
            let (start, end) = match (self.restrictive, spec.restrictive) {
                (true, Some(r)) => (r.v, r.v_500_end),
                _ => (p.v500_start, p.v500_end),
            };
            match spec.curve_500 {
                Curve500::Ramp(distance) => {
                    let d = ((odometer - m.start_odo) / distance).clamp(0.0, 1.0);
                    take(start + (end - start) * d);
                }
                Curve500::Step => take(start),
            }
        }
        if self.override_40
            && let Some(v) = spec.v_override
        {
            take(v);
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
                // The override (Befehl 40) suppresses the 2000 Hz influence — that is what
                // the button does in every build, supervised speed or not.
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

impl TrainProtectionSystem for Pzb {
    fn update(
        &mut self,
        dt: f64,
        train: &SafetyTrainState,
        cab: &CabInputs,
        events: &[TracksideEvent],
    ) -> ProtectionOutput {
        if self.isolated {
            *self = Self {
                variant: self.variant,
                train_type: self.train_type,
                isolated: true,
                ..Default::default()
            };
            return ProtectionOutput::default();
        }

        self.override_40 = cab.pzb_override;
        let acknowledge = self.acknowledge.rising(cab.pzb_acknowledge);
        let exempt = self.exempt.rising(cab.pzb_exempt);

        // Function test — until it has passed the device holds the forced braking.
        if !self.test.is_passed() {
            self.test.step(dt, train, acknowledge);
            return ProtectionOutput {
                action: ProtectionAction::EmergencyBrake,
                alert: true,
                protection_alert: true,
                ..Default::default()
            };
        }

        let spec = self.spec();

        for e in events {
            self.handle_event(e, train.odometer);
        }

        // Acknowledgement.
        if acknowledge {
            if let Some(m) = &mut self.m1000 {
                m.acknowledged = true;
            }
            if self.trip.is_some() && train.standstill() {
                // Release the forced braking. Builds with a restrictive programme continue
                // under it, older ones simply carry on with the running supervision.
                self.trip = None;
                if spec.restrictive.is_some() {
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
        }

        // Exemption (Frei button).
        if exempt && self.release_allowed(train.odometer) {
            self.m1000 = None;
            self.slow_timer = 0.0;
        }

        // Advance times and distances.
        if let Some(m) = &mut self.m1000 {
            m.elapsed += dt;
            if !m.acknowledged {
                m.ack_timer += dt;
                if m.ack_timer > spec.t_acknowledge {
                    self.trip = Some(PzbTrip::MissingAcknowledge);
                }
            }
            if spec
                .d_1000
                .is_some_and(|d| train.odometer - m.start_odo > d)
            {
                self.m1000 = None;
                self.restrictive = false;
                self.slow_timer = 0.0;
            }
        }
        if let Some(m) = self.m500
            && train.odometer - m.start_odo > spec.d_500
        {
            self.m500 = None;
        }

        // Restrictive supervision: after a stop, and from V2.0 also after running longer
        // than 15 s below 10 km/h within a supervision.
        if let Some(r) = spec.restrictive {
            if self.m1000.is_some() || self.m500.is_some() {
                if train.v_kmh < r.v_slow {
                    self.slow_timer += dt;
                    if train.standstill() || r.t_slow.is_some_and(|t| self.slow_timer > t) {
                        self.restrictive = true;
                    }
                } else {
                    self.slow_timer = 0.0;
                }
            } else {
                self.restrictive = false;
                self.slow_timer = 0.0;
            }
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
            protection_alert: self.m1000.is_some_and(|m| !m.acknowledged) || self.trip.is_some(),
            ..Default::default()
        }
    }

    fn indicators(&self) -> Vec<Indicator> {
        let p = self.params();
        // During the lamp test of the function test every indicator is lit.
        if self.test.lamp_test() {
            return vec![
                Indicator::lamp("pzb_1000hz", true),
                Indicator::lamp("pzb_500hz", true),
                Indicator::lamp("pzb_befehl", true),
                Indicator::lamp(p.lamp, true),
            ];
        }
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
        self.variant.name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safety::SelfTestPhase;

    /// Small test rig: runs the train at constant speed with button presses.
    struct Rig {
        pzb: Pzb,
        state: SafetyTrainState,
        cab: CabInputs,
        out: ProtectionOutput,
    }

    impl Rig {
        fn new(train_type: TrainType, v_kmh: f64) -> Self {
            Self::variant(PzbVariant::Pzb90V20, train_type, v_kmh)
        }

        fn variant(variant: PzbVariant, train_type: TrainType, v_kmh: f64) -> Self {
            Self {
                pzb: Pzb::with_variant(variant, train_type),
                state: SafetyTrainState {
                    v_kmh,
                    line_speed: 160.0,
                    ..Default::default()
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

    // --- Build states -----------------------------------------------------------------

    #[test]
    fn indusi_i54_supervises_the_check_speed_only_after_the_time() {
        // I 54, category O: 20 s of grace, then 95 km/h.
        let mut r = Rig::variant(PzbVariant::I54, TrainType::O, 120.0);
        r.magnet(MagnetFrequency::Hz1000);
        r.acknowledge();
        r.run(19.0);
        assert!(
            r.pzb.supervised_speed().is_none(),
            "no braking curve while the time runs"
        );
        assert!(!r.braking());
        r.run(2.0);
        assert_eq!(r.pzb.supervised_speed(), Some(95.0));
        assert!(r.braking(), "120 km/h after the time has run");
    }

    /// The whole point of the per-build tables: the same loco is supervised differently
    /// depending on which build it carries. The PZB 90 harmonised the check speeds down.
    #[test]
    fn the_older_builds_keep_their_own_check_speeds() {
        let check = |variant, train_type| {
            let mut r = Rig::variant(variant, train_type, 60.0);
            r.magnet(MagnetFrequency::Hz1000);
            r.acknowledge();
            r.run(40.0);
            r.pzb.supervised_speed().expect("supervision running")
        };
        for (train_type, indusi, pzb90) in [
            (TrainType::O, 95.0, 85.0),
            (TrainType::M, 75.0, 70.0),
            (TrainType::U, 60.0, 55.0),
        ] {
            assert_eq!(check(PzbVariant::I60, train_type), indusi);
            assert_eq!(check(PzbVariant::Pzb60, train_type), indusi, "ÖBB PZB 60");
            assert_eq!(check(PzbVariant::Pzb90V20, train_type), pzb90);
        }
        // The I 54 was set by the vehicle's maximum speed: 95/90/80.
        assert_eq!(check(PzbVariant::I54, TrainType::M), 90.0);
        assert_eq!(check(PzbVariant::I54, TrainType::U), 80.0);
    }

    /// The I 60R is computer-controlled: a falling curve instead of a check point, but onto
    /// the I 60 check speed.
    #[test]
    fn indusi_i60r_supervises_a_curve_onto_the_old_check_speed() {
        let mut r = Rig::variant(PzbVariant::I60R, TrainType::O, 150.0);
        r.magnet(MagnetFrequency::Hz1000);
        r.acknowledge();
        r.run(10.0);
        let half = r.pzb.supervised_speed().expect("curve running");
        assert!(
            half > 125.0 && half < 135.0,
            "halfway down from 165 to 95: {half}"
        );
        r.run(11.0);
        assert_eq!(r.pzb.supervised_speed(), Some(95.0));
    }

    #[test]
    fn indusi_i54_five_hundred_hertz_is_a_fixed_check_speed() {
        let mut r = Rig::variant(PzbVariant::I54, TrainType::O, 60.0);
        r.magnet(MagnetFrequency::Hz500);
        assert_eq!(r.pzb.supervised_speed(), Some(65.0));
        r.drive(200.0);
        assert_eq!(
            r.pzb.supervised_speed(),
            Some(65.0),
            "no falling curve, the I 54 holds the check speed"
        );
        assert!(!r.braking());
    }

    #[test]
    fn indusi_i60_has_no_restrictive_mode() {
        let mut r = Rig::variant(PzbVariant::I60, TrainType::O, 80.0);
        r.magnet(MagnetFrequency::Hz1000);
        r.acknowledge();
        r.drive(200.0);
        r.state.v_kmh = 0.0;
        r.run(2.0);
        assert!(
            !r.pzb.is_restrictive(),
            "the I 60 knows no restrictive mode"
        );
        // Pulling away again at 60 km/h stays permitted before the time has run.
        r.state.v_kmh = 60.0;
        r.run(1.0);
        assert!(!r.braking());
    }

    #[test]
    fn indusi_i60_supervision_only_ends_with_the_release_button() {
        let mut r = Rig::variant(PzbVariant::I60, TrainType::O, 80.0);
        r.magnet(MagnetFrequency::Hz1000);
        r.acknowledge();
        r.drive(1400.0);
        assert!(
            r.pzb.monitoring_1000(),
            "no 1250 m distance supervision before the I 60R"
        );
        r.exempt();
        assert!(!r.pzb.monitoring_1000());
    }

    #[test]
    fn indusi_i60r_adds_distance_supervision_and_restrictive_mode() {
        let mut r = Rig::variant(PzbVariant::I60R, TrainType::O, 80.0);
        r.magnet(MagnetFrequency::Hz1000);
        r.acknowledge();
        r.drive(200.0);
        r.state.v_kmh = 0.0;
        r.run(1.0);
        assert!(r.pzb.is_restrictive(), "restrictive programme present");
        assert_eq!(r.pzb.supervised_speed(), Some(V_RESTRICTIVE));

        // …but the 15 s rule below 10 km/h is a PZB 90 V2.0 addition.
        let mut r = Rig::variant(PzbVariant::I60R, TrainType::O, 8.0);
        r.magnet(MagnetFrequency::Hz1000);
        r.acknowledge();
        r.run(20.0);
        assert!(!r.pzb.is_restrictive());

        // The distance supervision ends after 1250 m.
        let mut r = Rig::variant(PzbVariant::I60R, TrainType::O, 80.0);
        r.magnet(MagnetFrequency::Hz1000);
        r.acknowledge();
        r.drive(1300.0);
        assert!(!r.pzb.monitoring_1000());
    }

    #[test]
    fn oebb_pzb60_runs_on_the_contemporary_indusi_set() {
        let mut r = Rig::variant(PzbVariant::Pzb60, TrainType::O, 120.0);
        r.magnet(MagnetFrequency::Hz1000);
        r.acknowledge();
        r.run(19.0);
        assert!(!r.braking(), "20 s of grace");
        r.run(2.0);
        assert_eq!(r.pzb.supervised_speed(), Some(95.0));
        assert!(r.braking());
    }

    #[test]
    fn pzb90_v15_enters_the_restrictive_mode_only_after_a_stop() {
        // Running below 10 km/h does not trigger it …
        let mut r = Rig::variant(PzbVariant::Pzb90V15, TrainType::O, 8.0);
        r.magnet(MagnetFrequency::Hz1000);
        r.acknowledge();
        r.run(20.0);
        assert!(!r.pzb.is_restrictive(), "the 15 s rule came with V2.0");
        // … a stop does.
        r.state.v_kmh = 0.0;
        r.run(1.0);
        assert!(r.pzb.is_restrictive());
    }

    #[test]
    fn pzb90_v15_restrictive_500_hz_stays_at_45() {
        let mut r = Rig::variant(PzbVariant::Pzb90V15, TrainType::O, 40.0);
        r.magnet(MagnetFrequency::Hz1000);
        r.acknowledge();
        r.state.v_kmh = 0.0;
        r.run(1.0);
        assert!(r.pzb.is_restrictive());
        r.state.v_kmh = 40.0;
        r.magnet(MagnetFrequency::Hz500);
        r.drive(200.0);
        assert_eq!(
            r.pzb.supervised_speed(),
            Some(V_RESTRICTIVE),
            "V1.5 holds 45 km/h, V2.0 falls to 25"
        );
        assert!(!r.braking());
    }

    #[test]
    fn pzb90_v20_restrictive_500_hz_falls_to_25() {
        let mut r = Rig::new(TrainType::O, 40.0);
        r.magnet(MagnetFrequency::Hz1000);
        r.acknowledge();
        r.state.v_kmh = 0.0;
        r.run(1.0);
        r.state.v_kmh = 40.0;
        r.magnet(MagnetFrequency::Hz500);
        r.drive(200.0);
        assert!(r.braking(), "the restrictive 500 Hz curve ends at 25 km/h");
    }

    // --- Function test ----------------------------------------------------------------

    #[test]
    fn function_test_holds_the_brake_until_it_is_acknowledged() {
        let mut r = Rig::new(TrainType::O, 0.0);
        r.pzb.power_on();
        assert_eq!(r.pzb.self_test().phase(), SelfTestPhase::Lamps);
        assert!(
            r.pzb.indicators().iter().all(|i| i.lamp == LampState::On),
            "lamp test: every indicator lit"
        );
        r.run(1.0);
        assert!(r.braking(), "the brake stays applied during the test");
        r.run(5.0);
        assert_eq!(r.pzb.self_test().phase(), SelfTestPhase::AwaitAck);
        assert!(r.braking(), "waiting for the acknowledgement");
        r.acknowledge();
        assert!(r.pzb.self_test().is_passed());
        assert!(!r.braking());
    }

    #[test]
    fn function_test_does_not_run_while_the_train_moves() {
        let mut r = Rig::new(TrainType::O, 30.0);
        r.pzb.power_on();
        r.run(20.0);
        assert_eq!(
            r.pzb.self_test().phase(),
            SelfTestPhase::Lamps,
            "the test needs a standstill"
        );
        assert!(r.braking());
    }

    #[test]
    fn older_builds_have_no_function_test() {
        for variant in [PzbVariant::I54, PzbVariant::I60, PzbVariant::Pzb60] {
            let mut r = Rig::variant(variant, TrainType::O, 0.0);
            r.pzb.power_on();
            r.run(0.5);
            assert!(
                !r.braking(),
                "{} switches on without a function test",
                variant.name()
            );
        }
        // The microprocessor builds do run one.
        for variant in [PzbVariant::I60M, PzbVariant::I60R, PzbVariant::Pzb90V20] {
            let mut r = Rig::variant(variant, TrainType::O, 0.0);
            r.pzb.power_on();
            r.run(0.5);
            assert!(r.braking(), "{} runs a function test", variant.name());
        }
    }
}
