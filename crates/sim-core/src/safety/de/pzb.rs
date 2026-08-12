//! PZB 90 (Punktförmige Zugbeeinflussung), vollständige Fahrzeuglogik (Plan 9.3).
//!
//! Streckenseitig wirken 500-Hz-, 1000-Hz- und 2000-Hz-Gleismagnete; ihre Wirksamkeit
//! hängt am Signalbegriff und wird vom Stellwerk entschieden (`TracksideEvent::active`).
//!
//! Zahlenwerte nach Ril 483.0111 (PZB 90, Zugarten O/M/U).

use crate::cab::{CabInputs, Edge};
use crate::safety::{
    Indicator, LampState, ProtectionAction, ProtectionOutput, SafetyTrainState, TracksideEvent,
    TrainProtectionSystem,
};
use serde::{Deserialize, Serialize};
use track_model::DeviceKind;

/// Frequenz eines Gleismagneten.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MagnetFrequency {
    Hz500,
    Hz1000,
    Hz2000,
}

/// Payload eines Magnet-Streckengeräts.
///
/// `signal`/`activation` liest das Stellwerk (`interlock::DeviceLink`), `frequency` die PZB —
/// beide Seiten ignorieren die Felder der jeweils anderen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MagnetPayload {
    pub frequency: MagnetFrequency,
    /// Zugehöriges Signal, dessen Begriff über die Wirksamkeit entscheidet.
    #[serde(default)]
    pub signal: Option<u32>,
    /// Wann der Magnet wirksam ist.
    #[serde(default)]
    pub activation: crate::interlock::Activation,
}

impl MagnetPayload {
    /// 1000-Hz-Magnet am Vorsignal — wirksam bei angekündigter Einschränkung.
    pub fn hz1000(signal: u32) -> Self {
        Self {
            frequency: MagnetFrequency::Hz1000,
            signal: Some(signal),
            activation: crate::interlock::Activation::WhenRestrictive,
        }
    }

    /// 500-Hz-Magnet vor dem Hauptsignal — wirksam bei Halt.
    pub fn hz500(signal: u32) -> Self {
        Self {
            frequency: MagnetFrequency::Hz500,
            signal: Some(signal),
            activation: crate::interlock::Activation::WhenStop,
        }
    }

    /// 2000-Hz-Magnet am Hauptsignal — wirksam bei Halt.
    pub fn hz2000(signal: u32) -> Self {
        Self {
            frequency: MagnetFrequency::Hz2000,
            signal: Some(signal),
            activation: crate::interlock::Activation::WhenStop,
        }
    }
}

/// Zugart nach Bremshundertsteln/Höchstgeschwindigkeit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TrainType {
    /// Obere Zugart (schnellfahrende Reisezüge).
    #[default]
    O,
    /// Mittlere Zugart.
    M,
    /// Untere Zugart (Güterzüge).
    U,
}

/// Überwachungsparameter einer Zugart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PzbParams {
    pub v1000_start: f64,
    pub v1000_end: f64,
    /// Zeit der 1000-Hz-Bremskurve [s].
    pub t1000: f64,
    pub v500_start: f64,
    pub v500_end: f64,
    /// Leuchtmelder-Beschriftung der Zugart.
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

/// Überwachungsgeschwindigkeit der restriktiven Überwachung [km/h].
pub const V_RESTRICTIVE: f64 = 45.0;
/// Restriktive 500-Hz-Kurve: von 45 auf 25 km/h.
pub const V_RESTRICTIVE_500_END: f64 = 25.0;
/// Überwachte Geschwindigkeit bei Befehl 40 [km/h].
pub const V_BEFEHL: f64 = 40.0;
/// Länge der 1000-Hz-Überwachung [m].
pub const D_1000: f64 = 1250.0;
/// Länge der 500-Hz-Überwachung [m].
pub const D_500: f64 = 250.0;
/// Weg der 500-Hz-Bremskurve [m].
pub const D_500_CURVE: f64 = 153.0;
/// Ab diesem Weg innerhalb der 1000-Hz-Überwachung ist Befreiung zulässig [m].
pub const D_FREI: f64 = 700.0;
/// Zeit für die Wachsamkeitsbedienung nach 1000-Hz-Beeinflussung [s].
pub const T_WACHSAM: f64 = 4.0;
/// Ab dieser Langsamfahrzeit unter 10 km/h greift die restriktive Überwachung [s].
pub const T_SLOW: f64 = 15.0;
/// Geschwindigkeitsschwelle für die restriktive Überwachung [km/h].
pub const V_SLOW: f64 = 10.0;

/// Auslöser einer Zwangsbremsung.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PzbTrip {
    /// Wachsamkeit nicht innerhalb 4 s bedient.
    MissingAcknowledge,
    /// Überwachungsgeschwindigkeit überschritten.
    Overspeed,
    /// 2000-Hz-Beeinflussung (Halt zeigendes Signal).
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

/// Die PZB-90-Fahrzeugeinrichtung.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Pzb90 {
    pub train_type: TrainType,
    isolated: bool,
    m1000: Option<Monitor1000>,
    m500: Option<Monitor500>,
    /// Restriktive Überwachung aktiv.
    restrictive: bool,
    /// Befehl-40-Taste gedrückt.
    befehl: bool,
    trip: Option<PzbTrip>,
    /// Zeit unter 10 km/h innerhalb einer Überwachung [s].
    slow_timer: f64,
    wachsam: Edge,
    frei: Edge,
    /// Aktuelle Überwachungsgeschwindigkeit [km/h], zuletzt berechnet.
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

    /// Aktuelle Überwachungsgeschwindigkeit [km/h], falls überwacht wird.
    pub fn supervised_speed(&self) -> Option<f64> {
        self.limit
    }

    pub fn monitoring_1000(&self) -> bool {
        self.m1000.is_some()
    }

    pub fn monitoring_500(&self) -> bool {
        self.m500.is_some()
    }

    /// Ist die Befreiung gerade zulässig?
    pub fn release_allowed(&self, odometer: f64) -> bool {
        self.m1000.is_some_and(|m| odometer - m.start_odo >= D_FREI) && !self.restrictive
    }

    fn params(&self) -> PzbParams {
        self.train_type.params()
    }

    /// Überwachungsgeschwindigkeit aus allen aktiven Beeinflussungen.
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
        if self.befehl {
            take(V_BEFEHL);
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
        // Der Magnet liegt bereits `s_offset` hinter der Antenne.
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
                // Befehl 40 unterdrückt die 2000-Hz-Beeinflussung.
                if !self.befehl {
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

        self.befehl = cab.pzb_befehl;
        let wachsam = self.wachsam.rising(cab.pzb_wachsam);
        let frei = self.frei.rising(cab.pzb_frei);

        for e in events {
            self.handle_event(e, train.odometer);
        }

        // Wachsamkeitsbedienung.
        if wachsam {
            if let Some(m) = &mut self.m1000 {
                m.acknowledged = true;
            }
            if self.trip.is_some() && train.standstill() {
                // Zwangsbremsung freigeben — danach gilt restriktive Überwachung.
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

        // Befreiung (Frei-Taste) ab 700 m, nicht in restriktiver Überwachung.
        if frei && self.release_allowed(train.odometer) {
            self.m1000 = None;
            self.slow_timer = 0.0;
        }

        // Zeiten und Wege fortschreiben.
        if let Some(m) = &mut self.m1000 {
            m.elapsed += dt;
            if !m.acknowledged {
                m.ack_timer += dt;
                if m.ack_timer > T_WACHSAM {
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

        // Restriktive Überwachung: Halt oder länger als 15 s unter 10 km/h
        // innerhalb einer Überwachung.
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

        // Geschwindigkeitsüberwachung.
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
            Indicator::lamp("pzb_befehl", self.befehl),
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

    /// Kleiner Prüfstand: fährt den Zug mit konstanter Geschwindigkeit und Tastendrücken.
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

        /// Fährt `seconds` Sekunden weiter (Weg aus der Geschwindigkeit).
        fn run(&mut self, seconds: f64) {
            let dt = 0.05;
            for _ in 0..(seconds / dt).round() as u32 {
                self.state.odometer += self.state.v_kmh / 3.6 * dt;
                self.out = self.pzb.update(dt, &self.state, &self.cab, &[]);
            }
        }

        /// Fährt `meters` Meter weiter.
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

        fn wachsam(&mut self) {
            self.press(|c| c.pzb_wachsam = true);
        }

        fn frei(&mut self) {
            self.press(|c| c.pzb_frei = true);
        }

        fn braking(&self) -> bool {
            self.out.action == ProtectionAction::EmergencyBrake
        }
    }

    #[test]
    fn tausend_hertz_ohne_wachsam_bremst_zwangs() {
        let mut r = Rig::new(TrainType::O, 120.0);
        r.magnet(MagnetFrequency::Hz1000);
        r.run(3.0);
        assert!(!r.braking(), "innerhalb 4 s noch keine Zwangsbremsung");
        r.run(2.0);
        assert!(r.braking());
        assert_eq!(r.pzb.trip(), Some(PzbTrip::MissingAcknowledge));
    }

    #[test]
    fn tausend_hertz_mit_wachsam_ueberwacht_bremskurve() {
        let mut r = Rig::new(TrainType::O, 120.0);
        r.magnet(MagnetFrequency::Hz1000);
        r.wachsam();
        r.run(3.0);
        assert!(!r.braking());
        // Kurve 165 → 85 km/h in 23 s: nach 20 s liegt die Grenze bei ~ 95 km/h.
        let limit = r.pzb.supervised_speed().unwrap();
        assert!(limit > 130.0 && limit < 160.0, "nach 3 s: {limit}");
        r.run(17.0);
        let limit = r.pzb.supervised_speed().unwrap();
        assert!(limit > 85.0 && limit < 100.0, "nach 20 s: {limit}");
        // Mit 120 km/h wird die Kurve überschritten, sobald sie unter 120 fällt.
        r.run(10.0);
        assert!(r.braking(), "Bremskurve überschritten");
        assert_eq!(r.pzb.trip(), Some(PzbTrip::Overspeed));
    }

    #[test]
    fn tausend_hertz_befreiung_erst_ab_700_m() {
        let mut r = Rig::new(TrainType::O, 80.0);
        r.magnet(MagnetFrequency::Hz1000);
        r.wachsam();
        r.drive(600.0);
        r.frei();
        assert!(r.pzb.monitoring_1000(), "vor 700 m keine Befreiung");
        r.drive(150.0);
        r.frei();
        assert!(!r.pzb.monitoring_1000(), "ab 700 m Befreiung möglich");
        assert!(r.pzb.supervised_speed().is_none());
    }

    #[test]
    fn tausend_hertz_ueberwachung_endet_nach_1250_m() {
        let mut r = Rig::new(TrainType::O, 80.0);
        r.magnet(MagnetFrequency::Hz1000);
        r.wachsam();
        r.drive(1300.0);
        assert!(!r.pzb.monitoring_1000());
        assert!(!r.braking());
    }

    #[test]
    fn fuenfhundert_hertz_ueberwacht_sofort_ohne_befreiung() {
        let mut r = Rig::new(TrainType::O, 60.0);
        r.magnet(MagnetFrequency::Hz500);
        // Sofortige Überwachung: 65 km/h fallend, keine Quittierung nötig.
        assert!(!r.braking());
        r.drive(50.0);
        r.frei();
        assert!(r.pzb.monitoring_500(), "500 Hz kennt keine Befreiung");
        // Nach 153 m liegt die Grenze bei 45 km/h → 60 km/h löst aus.
        r.drive(120.0);
        assert!(r.braking());
        assert_eq!(r.pzb.trip(), Some(PzbTrip::Overspeed));
    }

    #[test]
    fn zweitausend_hertz_bremst_sofort_befehl_unterdrueckt() {
        let mut r = Rig::new(TrainType::O, 60.0);
        r.magnet(MagnetFrequency::Hz2000);
        assert!(r.braking());
        assert_eq!(r.pzb.trip(), Some(PzbTrip::Magnet2000));

        // Mit gedrückter Befehlstaste bleibt die Beeinflussung aus, 40 km/h werden überwacht.
        let mut r = Rig::new(TrainType::O, 35.0);
        r.cab.pzb_befehl = true;
        r.run(0.1);
        r.magnet(MagnetFrequency::Hz2000);
        r.run(1.0);
        assert!(!r.braking());
        assert_eq!(r.pzb.supervised_speed(), Some(V_BEFEHL));
    }

    #[test]
    fn restriktive_ueberwachung_nach_halt() {
        let mut r = Rig::new(TrainType::O, 80.0);
        r.magnet(MagnetFrequency::Hz1000);
        r.wachsam();
        r.drive(200.0);
        r.state.v_kmh = 0.0;
        r.run(1.0);
        assert!(r.pzb.is_restrictive());
        assert_eq!(r.pzb.supervised_speed(), Some(V_RESTRICTIVE));
        // Anfahren auf 50 km/h löst die Zwangsbremsung aus.
        r.state.v_kmh = 50.0;
        r.run(0.5);
        assert!(r.braking());
    }

    #[test]
    fn restriktive_ueberwachung_nach_15_s_unter_10_kmh() {
        let mut r = Rig::new(TrainType::O, 8.0);
        r.magnet(MagnetFrequency::Hz1000);
        r.wachsam();
        r.run(10.0);
        assert!(!r.pzb.is_restrictive());
        r.run(7.0);
        assert!(r.pzb.is_restrictive());
    }

    #[test]
    fn zwangsbremsung_loest_nur_im_stillstand() {
        let mut r = Rig::new(TrainType::O, 60.0);
        r.magnet(MagnetFrequency::Hz2000);
        assert!(r.braking());
        r.wachsam();
        assert!(r.braking(), "in Fahrt keine Freigabe");
        r.state.v_kmh = 0.0;
        r.run(0.5);
        r.wachsam();
        assert!(!r.braking(), "im Stillstand mit Wachsam lösbar");
        assert!(r.pzb.is_restrictive(), "danach restriktive Überwachung");
    }

    #[test]
    fn zugart_u_hat_niedrigere_pruefgeschwindigkeiten() {
        let mut r = Rig::new(TrainType::U, 50.0);
        r.magnet(MagnetFrequency::Hz500);
        r.drive(160.0);
        // U: 40 → 25 km/h; mit 50 km/h sofort zu schnell.
        assert!(r.braking());
    }

    #[test]
    fn unwirksamer_magnet_loest_nichts_aus() {
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
