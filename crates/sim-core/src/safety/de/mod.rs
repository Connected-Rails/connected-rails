//! Country package Germany: Sifa, PZB 90, LZB 80 (plan 9.2–9.4).

pub mod lzb;
pub mod pzb;
pub mod sifa;

use crate::cab::CabInputs;
use crate::safety::{
    Indicator, ProtectionOutput, SafetyTrainState, TracksideEvent, TrainProtectionSystem,
};
use serde::{Deserialize, Serialize};

pub use lzb::{Lzb80, LzbMode, LzbTelegram};
pub use pzb::{MagnetFrequency, MagnetPayload, Pzb90, PzbTrip, TrainType};
pub use sifa::Sifa;

/// Train protection equipment of a German vehicle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct DeSafety {
    pub sifa: Option<Sifa>,
    pub pzb: Option<Pzb90>,
    pub lzb: Option<Lzb80>,
}

impl DeSafety {
    /// Usual equipment of a main line loco: Sifa + PZB.
    pub fn pzb(train_type: TrainType) -> Self {
        Self {
            sifa: Some(Sifa::new()),
            pzb: Some(Pzb90::new(train_type)),
            lzb: None,
        }
    }

    /// Equipment with LZB (BR 101/120/ICE).
    pub fn pzb_lzb(train_type: TrainType) -> Self {
        Self {
            sifa: Some(Sifa::new()),
            pzb: Some(Pzb90::new(train_type)),
            lzb: Some(Lzb80::new()),
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

        // Under LZB guidance the PZB magnets are suppressed (plan 9.4).
        let lzb_guiding = self.lzb.is_some_and(|l| l.is_guiding());
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
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safety::ProtectionAction;
    use track_model::DeviceKind;

    #[test]
    fn lzb_guidance_suppresses_pzb_magnets() {
        let mut de = DeSafety::pzb_lzb(TrainType::O);
        let state = SafetyTrainState {
            v_kmh: 120.0,
            ..Default::default()
        };
        let mut cab = CabInputs::default();

        // Pick up the LZB and take over.
        let telegram = LzbTelegram {
            permitted_speed: 160.0,
            target_speed: 160.0,
            target_distance: 20_000.0,
            end_of_authority: false,
            length: 20_000.0,
        };
        let ev = TracksideEvent {
            device: DeviceKind::LineConductor,
            payload: ron::to_string(&telegram).unwrap(),
            s_offset: 0.0,
            active: true,
        };
        de.update(0.1, &state, &cab, &[ev]);
        cab.lzb_takeover = true;
        de.update(0.1, &state, &cab, &[]);
        cab.lzb_takeover = false;
        assert!(de.lzb.unwrap().is_guiding());

        // A 2000 Hz magnet must not trigger anything now.
        let magnet = TracksideEvent {
            device: DeviceKind::Magnet,
            payload: ron::to_string(&MagnetPayload::hz2000(0)).unwrap(),
            s_offset: 0.0,
            active: true,
        };
        let out = de.update(0.1, &state, &cab, &[magnet]);
        assert_eq!(out.action, ProtectionAction::None);
        assert!(de.pzb.unwrap().trip().is_none());
    }
}
