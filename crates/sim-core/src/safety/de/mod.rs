//! Country package Germany: Sifa, PZB 90, LZB 80, GNT (plan 9.2–9.5).

pub mod gnt;
pub mod lzb;
pub mod pzb;
pub mod sifa;

use crate::cab::CabInputs;
use crate::safety::{
    Indicator, ProtectionOutput, SafetyTrainState, TracksideEvent, TrainProtectionSystem,
};
use serde::{Deserialize, Serialize};

pub use gnt::{Gnt, GntDataPoint, GntMode};
pub use lzb::{Lzb80, LzbBlockMode, LzbMode, LzbSection, LzbTelegram};
pub use pzb::{MagnetFrequency, MagnetPayload, Pzb, Pzb90, PzbTrip, PzbVariant, TrainType};
pub use sifa::{Sifa, SifaKind};

/// Train protection equipment of a German vehicle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct DeSafety {
    pub sifa: Option<Sifa>,
    pub pzb: Option<Pzb>,
    pub lzb: Option<Lzb80>,
    /// Speed supervision for tilting technology — only on units that can tilt.
    #[serde(default)]
    pub gnt: Option<Gnt>,
}

impl DeSafety {
    /// Usual equipment of a main line loco: Sifa + PZB 90 V2.0.
    pub fn pzb(train_type: TrainType) -> Self {
        Self::indusi(PzbVariant::Pzb90V20, train_type)
    }

    /// Sifa + a specific Indusi/PZB build (I 54 … PZB 90 V2.0, ÖBB PZB 60).
    pub fn indusi(variant: PzbVariant, train_type: TrainType) -> Self {
        Self {
            sifa: Some(Sifa::new()),
            pzb: Some(Pzb::with_variant(variant, train_type)),
            lzb: None,
            gnt: None,
        }
    }

    /// Equipment with LZB (BR 101/120/ICE) — LZB/I 80 on top of the PZB 90.
    pub fn pzb_lzb(train_type: TrainType) -> Self {
        Self {
            lzb: Some(Lzb80::new()),
            ..Self::pzb(train_type)
        }
    }

    /// LZB without PZB — a vehicle that may only run under LZB guidance.
    pub fn lzb_only() -> Self {
        Self {
            sifa: Some(Sifa::new()),
            pzb: None,
            lzb: Some(Lzb80::new()),
            gnt: None,
        }
    }

    /// Adds the GNT — the equipment of a tilting unit (BR 611/612 and their kin).
    pub fn with_gnt(mut self) -> Self {
        self.gnt = Some(Gnt::new());
        self
    }

    /// Replaces the Sifa build (time-time, time-distance, RZM).
    pub fn with_sifa(mut self, kind: SifaKind) -> Self {
        self.sifa = Some(Sifa::with_kind(kind));
        self
    }

    /// Switches every system on — the function tests start (plan 9.3/9.4).
    pub fn power_on(&mut self) {
        if let Some(p) = &mut self.pzb {
            p.power_on();
        }
        if let Some(l) = &mut self.lzb {
            l.power_on();
        }
        if let Some(g) = &mut self.gnt {
            g.power_on();
        }
    }

    pub fn update(
        &mut self,
        dt: f64,
        train: &SafetyTrainState,
        cab: &CabInputs,
        events: &[TracksideEvent],
    ) -> ProtectionOutput {
        let mut out = ProtectionOutput::default();

        if let Some(s) = &mut self.sifa {
            out = out.merge(s.update(dt, train, cab, events));
        }

        // Under LZB guidance the PZB magnets are suppressed (plan 9.4) — except in partial
        // block mode, where the lineside signals stay binding and their magnets therefore
        // remain the fallback level.
        let lzb_guiding = self
            .lzb
            .is_some_and(|l| l.is_guiding() && !l.signals_binding());
        let lzb_authority = self.lzb.is_some_and(|l| l.is_guiding());
        if let Some(l) = &mut self.lzb {
            out = out.merge(l.update(dt, train, cab, events));
        }
        if let Some(p) = &mut self.pzb {
            let pzb_events: &[TracksideEvent] = if lzb_guiding { &[] } else { events };
            let pzb_out = p.update(dt, train, cab, pzb_events);
            if !lzb_guiding {
                out = out.merge(pzb_out);
            }
        }

        // The GNT sits above the PZB, not instead of it: it only raises the line speed
        // between two signals, so the magnets stay effective underneath it in every case.
        // Against the LZB it is the other way round — while the LZB guides, its authority
        // already covers the line and the GNT stands down (plan 9.5, `gnt`).
        if let Some(g) = &mut self.gnt {
            g.stand_by(lzb_authority);
            out = out.merge(g.update(dt, train, cab, events));
        }
        out
    }

    pub fn indicators(&self) -> Vec<Indicator> {
        let mut v = Vec::new();
        if let Some(s) = &self.sifa {
            v.extend(s.indicators());
        }
        if let Some(p) = &self.pzb {
            v.extend(p.indicators());
        }
        if let Some(l) = &self.lzb {
            v.extend(l.indicators());
        }
        if let Some(g) = &self.gnt {
            v.extend(g.indicators());
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safety::ProtectionAction;
    use track_model::DeviceKind;

    fn telegram(block_mode: LzbBlockMode) -> TracksideEvent {
        let telegram = LzbTelegram {
            permitted_speed: 160.0,
            target_speed: 160.0,
            target_distance: 20_000.0,
            end_of_authority: false,
            length: 20_000.0,
            block_mode,
            cir_elke: false,
        };
        TracksideEvent {
            device: DeviceKind::LineConductor,
            payload: ron::to_string(&telegram).unwrap(),
            s_offset: 0.0,
            active: true,
        }
    }

    fn magnet_2000() -> TracksideEvent {
        TracksideEvent {
            device: DeviceKind::Magnet,
            payload: ron::to_string(&MagnetPayload::hz2000(0)).unwrap(),
            s_offset: 0.0,
            active: true,
        }
    }

    /// Picks the LZB up and takes over the guidance.
    fn take_over(de: &mut DeSafety, state: &SafetyTrainState, block_mode: LzbBlockMode) {
        let mut cab = CabInputs::default();
        de.update(0.1, state, &cab, &[telegram(block_mode)]);
        cab.lzb_takeover = true;
        de.update(0.1, state, &cab, &[]);
        assert!(de.lzb.unwrap().is_guiding());
    }

    fn running() -> SafetyTrainState {
        SafetyTrainState {
            v_kmh: 120.0,
            train_length: 200.0,
            ..Default::default()
        }
    }

    #[test]
    fn lzb_guidance_suppresses_pzb_magnets() {
        let mut de = DeSafety::pzb_lzb(TrainType::O);
        let state = running();
        take_over(&mut de, &state, LzbBlockMode::Full);

        // A 2000 Hz magnet must not trigger anything now.
        let out = de.update(0.1, &state, &CabInputs::default(), &[magnet_2000()]);
        assert_eq!(out.action, ProtectionAction::None);
        assert!(de.pzb.unwrap().trip().is_none());
    }

    #[test]
    fn partial_block_mode_keeps_the_pzb_magnets_effective() {
        let mut de = DeSafety::pzb_lzb(TrainType::O);
        let state = running();
        take_over(&mut de, &state, LzbBlockMode::Partial);
        assert!(de.lzb.unwrap().signals_binding());

        // The signals stay binding, so the 2000 Hz magnet of a signal at danger works.
        let out = de.update(0.1, &state, &CabInputs::default(), &[magnet_2000()]);
        assert_eq!(out.action, ProtectionAction::EmergencyBrake);
    }

    #[test]
    fn lzb_without_pzb_runs_on_the_guidance_alone() {
        let mut de = DeSafety::lzb_only();
        assert!(de.pzb.is_none());
        let state = running();
        take_over(&mut de, &state, LzbBlockMode::Full);

        // No PZB on board — a track magnet is simply not read.
        let out = de.update(0.1, &state, &CabInputs::default(), &[magnet_2000()]);
        assert_eq!(out.action, ProtectionAction::None);
        assert_eq!(out.speed_limit, Some(160.0), "the LZB supervises");
    }

    fn gnt_point() -> TracksideEvent {
        TracksideEvent {
            device: DeviceKind::Balise,
            payload: ron::to_string(&GntDataPoint::section(160.0, 4000.0)).unwrap(),
            s_offset: 0.0,
            active: true,
        }
    }

    /// The GNT raises the line speed between two signals — it never replaces the signal
    /// protection, so the PZB magnets keep working underneath it.
    #[test]
    fn the_gnt_leaves_the_pzb_magnets_alone() {
        let mut de = DeSafety::pzb(TrainType::M).with_gnt();
        let state = SafetyTrainState {
            v_kmh: 140.0,
            line_speed: 120.0,
            train_length: 100.0,
            ..Default::default()
        };
        let out = de.update(0.1, &state, &CabInputs::default(), &[gnt_point()]);
        assert_eq!(out.speed_limit, Some(160.0), "the GNT profile is released");

        let out = de.update(0.1, &state, &CabInputs::default(), &[magnet_2000()]);
        assert_eq!(out.action, ProtectionAction::EmergencyBrake);
    }

    /// Under LZB guidance the movement authority is the binding one; the GNT stands down
    /// instead of publishing a second, higher supervision next to it.
    #[test]
    fn lzb_guidance_puts_the_gnt_on_standby() {
        let mut de = DeSafety::pzb_lzb(TrainType::O).with_gnt();
        let state = SafetyTrainState {
            v_kmh: 120.0,
            line_speed: 120.0,
            train_length: 200.0,
            ..Default::default()
        };
        de.update(0.1, &state, &CabInputs::default(), &[gnt_point()]);
        assert_eq!(de.gnt.unwrap().mode(), GntMode::Supervising);

        take_over(&mut de, &state, LzbBlockMode::Full);
        let out = de.update(0.1, &state, &CabInputs::default(), &[gnt_point()]);
        assert_eq!(de.gnt.unwrap().mode(), GntMode::Off);
        assert_eq!(out.speed_limit, Some(160.0), "the LZB alone supervises");
    }

    #[test]
    fn power_on_starts_every_function_test() {
        let mut de = DeSafety::pzb_lzb(TrainType::O);
        de.power_on();
        assert!(!de.pzb.unwrap().self_test().is_passed());
        assert!(!de.lzb.unwrap().self_test().is_passed());

        // The PZB holds the brake until its test has been acknowledged.
        let state = SafetyTrainState::default();
        let out = de.update(0.1, &state, &CabInputs::default(), &[]);
        assert_eq!(out.action, ProtectionAction::EmergencyBrake);
    }
}
