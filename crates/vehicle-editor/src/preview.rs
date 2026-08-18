//! Sound preview (plan ch. 13) — hearing a table entry without starting the simulator.
//!
//! A sound table is written blind otherwise. The curves have sparklines, but a sparkline
//! does not say whether the rolling noise at 40 km/h is a train or a hairdryer, whether two
//! layers hand over without a step, or whether a pitch ramp pulls a sample apart at the top
//! of its range. So this plays the entry and hands the author the quantities it depends on
//! as sliders: move the speed and the loop follows exactly as it would while driving,
//! because it is [`SoundSpec::level`] doing the work in both places.
//!
//! Deliberately a plain stereo bus — no spatial track, no cab wall, no reverb. Those belong
//! to a place in the world, and the editor has no world. What is being judged here is the
//! entry, not where it is standing.

use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
use kira::{AudioManager, AudioManagerSettings, Decibels, DefaultBackend, Tween};
use sim_core::sound::{SoundSpec, SoundState};
use sim_core::synth;
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

/// Time constant of a volume or pitch move, as in the simulator — a level that jumps clicks.
const FADE: Tween = Tween {
    start_time: kira::StartTime::Immediate,
    duration: Duration::from_millis(80),
    easing: kira::Easing::Linear,
};

/// What is currently playing.
struct Playing {
    /// Index into the vehicle's sound table.
    entry: usize,
    handle: StaticSoundHandle,
}

/// The preview: an output device, the sources it has decoded so far, and the state the
/// sliders scrub.
#[derive(bevy::prelude::Resource, Default)]
pub struct Preview {
    /// `None` on a machine without an output device — the editor still runs, the play
    /// button says why it does not.
    manager: Option<AudioManager>,
    sources: HashMap<String, StaticSoundData>,
    playing: Option<Playing>,
    /// Shared by every entry on purpose: setting the speed for one layer and then
    /// auditioning the neighbouring one is the whole point of a crossfade preview.
    pub state: SoundState,
    /// Last error, shown next to the button — a missing file is the common case and has to
    /// say so rather than play silence.
    pub error: Option<String>,
}

impl Preview {
    /// Opens the output device. Called once while the editor starts; failure is not fatal.
    pub fn open() -> Self {
        let manager = AudioManager::<DefaultBackend>::new(AudioManagerSettings::default()).ok();
        Self {
            manager,
            state: SoundState::default(),
            ..Self::default()
        }
    }

    /// `false` when there is no output device, so the editor can say so instead of
    /// offering a button that does nothing.
    pub fn available(&self) -> bool {
        self.manager.is_some()
    }

    /// Is this entry the one currently playing?
    pub fn is_playing(&self, entry: usize) -> bool {
        self.playing.as_ref().is_some_and(|p| p.entry == entry)
    }

    /// Starts this entry, or stops it if it is the one already running.
    ///
    /// One entry at a time: the point is to judge a single sound, and two of them at once
    /// is the mix, which is what the simulator is for.
    pub fn toggle(&mut self, entry: usize, spec: &SoundSpec) {
        if self.is_playing(entry) {
            self.stop();
            return;
        }
        self.stop();
        self.error = None;
        let Some(manager) = self.manager.as_mut() else {
            return;
        };
        let source = match Self::source(&mut self.sources, &spec.file) {
            Ok(source) => source,
            Err(message) => {
                self.error = Some(message);
                return;
            }
        };
        let (volume, pitch) = spec.level(&self.state);
        // A loop repeats until it is stopped; a triggered entry is one shot per press,
        // exactly as it is one shot per edge while driving.
        let source = source
            .volume(decibels(volume as f32))
            .playback_rate(pitch)
            .loop_region(spec.is_loop().then(|| kira::sound::Region::from(0.0..)));
        match manager.play(source) {
            Ok(handle) => self.playing = Some(Playing { entry, handle }),
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    /// Stops whatever is running.
    pub fn stop(&mut self) {
        if let Some(mut playing) = self.playing.take() {
            playing.handle.stop(FADE);
        }
    }

    /// Applies the entry's curves at the scrubbed state — called every frame a slider may
    /// have moved.
    pub fn refresh(&mut self, spec: &SoundSpec) {
        let state = self.state;
        let Some(playing) = self.playing.as_mut() else {
            return;
        };
        let (volume, pitch) = spec.level(&state);
        playing.handle.set_volume(decibels(volume as f32), FADE);
        playing.handle.set_playback_rate(pitch, FADE);
    }

    /// The volume and playback rate the entry would have right now — the numbers under the
    /// sliders, so a muting condition is visible and not just inaudible.
    pub fn level(&self, spec: &SoundSpec) -> (f64, f64) {
        spec.level(&self.state)
    }

    /// Decodes a source once and keeps it: `synth:<name>` is generated, everything else is
    /// a file below `mods/`, the same path the simulator reads it from.
    fn source(
        cache: &mut HashMap<String, StaticSoundData>,
        file: &str,
    ) -> Result<StaticSoundData, String> {
        if let Some(source) = cache.get(file) {
            return Ok(source.clone());
        }
        if file.is_empty() {
            return Err(String::new());
        }
        let source = match file.strip_prefix("synth:") {
            Some(name) => {
                let samples = synth::synth(name).ok_or_else(|| file.to_string())?;
                StaticSoundData {
                    sample_rate: synth::RATE,
                    frames: samples.into_iter().map(kira::Frame::from_mono).collect(),
                    settings: kira::sound::static_sound::StaticSoundSettings::default(),
                    slice: None,
                }
            }
            None => StaticSoundData::from_file(Path::new(crate::MOD_SOURCE).join(file))
                .map_err(|error| error.to_string())?,
        };
        cache.insert(file.to_string(), source.clone());
        Ok(source)
    }
}

/// Volume in decibels for a linear 0 … 1 factor; 0 is silence, not −∞ dB.
fn decibels(linear: f32) -> Decibels {
    if linear <= 1e-3 {
        Decibels::SILENCE
    } else {
        Decibels(20.0 * linear.log10())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::sound::{Curve, Quantity};

    /// The generated sources reach the preview by the same name the table writes.
    #[test]
    fn a_generated_source_is_decoded_and_cached() {
        let mut cache = HashMap::new();
        let source = Preview::source(&mut cache, "synth:rolling-mid").expect("generated");
        assert_eq!(source.sample_rate, synth::RATE);
        assert_eq!(cache.len(), 1);
        // Second ask comes out of the cache — decoding a mod's sample per frame would
        // stall the editor.
        Preview::source(&mut cache, "synth:rolling-mid").expect("cached");
        assert_eq!(cache.len(), 1);
        assert!(Preview::source(&mut cache, "synth:nonexistent").is_err());
        assert!(Preview::source(&mut cache, "").is_err());
    }

    /// What the sliders drive: the same `level` the simulator calls, so the preview cannot
    /// drift away from what is heard while driving.
    #[test]
    fn the_scrubbed_state_drives_the_entrys_own_curves() {
        let mut preview = Preview::default();
        let spec = SoundSpec {
            name: "rolling".into(),
            file: "synth:rolling-mid".into(),
            trigger: sim_core::sound::Trigger::Loop,
            conditions: Vec::new(),
            volume: Some(Curve::ramp(Quantity::Speed, 0.0, 0.0, 100.0, 1.0)),
            factors: Vec::new(),
            pitch: None,
            positional: true,
        };
        assert_eq!(preview.level(&spec).0, 0.0);
        assert!(preview.state.set(Quantity::Speed, 50.0));
        assert!((preview.level(&spec).0 - 0.5).abs() < 1e-9);
        // A cab input needs a whole train to scale against — the editor has to know.
        assert!(
            !preview
                .state
                .set(Quantity::Control(sim_core::cab::CabControl::Throttle), 1.0)
        );
    }
}
