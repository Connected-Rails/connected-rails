//! Basic sounds (plan ch. 13).
//!
//! Every sound is one looping sink whose volume and playback speed follow the simulation
//! state of the player's train — rolling noise, traction, air, compressor, horn and the
//! train protection buzzer. The loops are **generated**, not shipped: a handful of
//! oscillators and a noise generator written into a WAV buffer at startup.
//!
//! ponytail: continuous modulation only, no event queue. Discrete noises (rail joints,
//! tap changer contactors, brake squeal), sounds per vehicle file and positional audio of
//! other trains belong to the full audio of M6 — that is when `sim-core` gets an event
//! stream and `VehicleSpec` a sound table.

use crate::{PlayerTrain, SimResource};
use bevy::audio::Volume;
use bevy::prelude::*;
use std::f32::consts::TAU;

/// Sample rate of the generated loops [Hz]. Enough for engine hum and hiss, and every
/// buffer stays under 50 kB.
const RATE: u32 = 22_050;

/// The sounds of the cab. Each one is an entity with an `AudioPlayer` that never stops —
/// only its volume changes.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Channel {
    /// Wheels on the rail, rises with speed.
    Rolling,
    /// Traction: converter whine or diesel engine.
    Traction,
    /// Air flowing through the brake valves.
    Air,
    /// Main reservoir compressor.
    Compressor,
    /// Horn (Makrofon), two-tone.
    Horn,
    /// Sifa/PZB buzzer.
    Buzzer,
}

/// Creates the loops and starts them silently.
pub fn setup_audio(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    // Rolling: low-passed noise plus the rumble of the running gear.
    let mut low = 0.0;
    let mut rumble = noise();
    let rolling = generate(move |t| {
        low += (rumble() - low) * 0.08;
        low * 3.0 + (t * TAU * 50.0).sin() * 0.15
    });
    // Traction: converter whine — a fundamental with two harmonics.
    let traction = generate(|t| tone(t, &[(200.0, 0.5), (400.0, 0.3), (800.0, 0.12)]));
    // Air: white noise, high-passed so it hisses instead of rumbling.
    let mut low = 0.0;
    let mut hiss = noise();
    let air = generate(move |t| {
        let white = hiss();
        low += (white - low) * 0.35;
        (white - low) * 0.8 + (t * TAU * 120.0).sin() * 0.05
    });
    // Compressor: a low hum, chugging six times a second.
    let compressor =
        generate(|t| tone(t, &[(80.0, 0.6), (160.0, 0.2)]) * (0.6 + 0.4 * (t * TAU * 6.0).sin()));
    // Horn: the two-tone of a Makrofon, both notes with their octave.
    let horn = generate(|t| tone(t, &[(370.0, 0.4), (440.0, 0.4), (740.0, 0.1), (880.0, 0.1)]));
    // Buzzer: 800 Hz with odd harmonics — that is what makes it nag rather than sing.
    let buzzer = generate(|t| tone(t, &[(800.0, 0.5), (2400.0, 0.17), (4000.0, 0.1)]));

    for (channel, source) in [
        (Channel::Rolling, rolling),
        (Channel::Traction, traction),
        (Channel::Air, air),
        (Channel::Compressor, compressor),
        (Channel::Horn, horn),
        (Channel::Buzzer, buzzer),
    ] {
        commands.spawn((
            AudioPlayer::new(sources.add(source)),
            PlaybackSettings::LOOP.with_volume(Volume::SILENT),
            channel,
        ));
    }
}

/// What the cab hears, read off the simulation once per frame.
#[derive(Clone, Copy, Default, Debug)]
struct Cues {
    /// Driving speed [km/h], always positive.
    speed: f32,
    /// Tractive effort as a share of the reference force.
    effort: f32,
    /// Playback speed of the traction loop.
    traction_pitch: f32,
    /// Pressure change in the brake system [bar/s].
    air: f32,
    compressor: bool,
    horn: bool,
    /// The train protection demands an operation.
    alert: bool,
}

/// Volume and playback speed of one channel.
fn level(channel: Channel, cues: &Cues) -> (f32, f32) {
    match channel {
        Channel::Rolling => (
            (cues.speed / 60.0).min(1.0) * 0.55,
            0.7 + cues.speed / 200.0,
        ),
        Channel::Traction => (cues.effort * 0.45, cues.traction_pitch),
        Channel::Air => ((cues.air * 3.0).min(1.0) * 0.5, 1.0),
        Channel::Compressor => (if cues.compressor { 0.3 } else { 0.0 }, 1.0),
        Channel::Horn => (if cues.horn { 0.7 } else { 0.0 }, 1.0),
        Channel::Buzzer => (if cues.alert { 0.35 } else { 0.0 }, 1.0),
    }
}

/// Volume and pitch of every channel, from the state of the player's train.
pub fn update_audio(
    sim: Res<SimResource>,
    player: Res<PlayerTrain>,
    time: Res<Time>,
    // Brake pipe and cylinder of the previous frame — air is heard when it moves.
    mut last: Local<Option<(f64, f64)>>,
    mut sinks: Query<(&Channel, &mut AudioSink)>,
) {
    let dt = time.delta_secs_f64().clamp(1e-3, 0.25);
    let sim = &sim.0;
    let Some(train) = sim.trains.get(player.0) else {
        return;
    };
    let loco = &train.vehicles[0];
    let cab = &sim.controls[player.0];
    let speed = train.speed_kmh().abs() as f32;

    let (pipe, cylinder) = (loco.brake.pipe, loco.brake.cylinder);
    let (prev_pipe, prev_cylinder) = last.unwrap_or((pipe, cylinder));
    *last = Some((pipe, cylinder));
    // Pressure change at the traction unit in bar/s: venting and filling both hiss.
    let air = ((pipe - prev_pipe).abs() + (cylinder - prev_cylinder).abs()) / dt;

    // Diesel engines are heard by their speed, electrics by the driving speed —
    // the converter whine follows the motor, and that follows the wheel.
    let rpm = loco.traction.engine_rpm as f32;
    let traction_pitch = if rpm > 0.0 {
        (rpm / 900.0).clamp(0.4, 2.5)
    } else {
        0.5 + speed / 150.0
    };
    let cues = Cues {
        speed,
        // ponytail: 250 kN as the reference instead of the vehicle's own maximum — no drive
        // model states one, and the loudness of a loco does not follow its data sheet anyway.
        effort: (loco.tractive_effort.abs() as f32 / 250_000.0).min(1.0),
        traction_pitch,
        air: air as f32,
        compressor: loco.brake.compressor_running && loco.traction.compressor,
        horn: cab.horn,
        alert: sim.runtime[player.0].protection.alert,
    };

    for (channel, mut sink) in sinks.iter_mut() {
        let (volume, pitch) = level(*channel, &cues);
        // Fade instead of jumping: a volume set hard clicks in the speaker.
        let fade = (dt as f32 * 12.0).min(1.0);
        let faded = sink.volume().fade_towards(Volume::Linear(volume), fade);
        sink.set_volume(faded);
        sink.set_speed(pitch);
    }
}

/// One second of mono 16-bit PCM in a WAV container.
///
/// Generating the loops saves the repository a set of binary samples, and every whole
/// frequency runs through a whole number of periods in a second — so the loop has no seam.
fn generate(mut sample: impl FnMut(f32) -> f32) -> AudioSource {
    let count = RATE as usize;
    let data = count as u32 * 2;
    let mut bytes = Vec::with_capacity(44 + data as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes()); // size of the format chunk
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM, uncompressed
    bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
    bytes.extend_from_slice(&RATE.to_le_bytes());
    bytes.extend_from_slice(&(RATE * 2).to_le_bytes()); // bytes per second
    bytes.extend_from_slice(&2u16.to_le_bytes()); // bytes per frame
    bytes.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    for i in 0..count {
        let t = i as f32 / RATE as f32;
        let value = (sample(t).clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    AudioSource {
        bytes: bytes.into(),
    }
}

/// Sum of sine partials: `(frequency [Hz], amplitude)`.
fn tone(t: f32, partials: &[(f32, f32)]) -> f32 {
    partials
        .iter()
        .map(|(frequency, amplitude)| (t * frequency * TAU).sin() * amplitude)
        .sum()
}

/// White noise from a fixed seed — the buffer is the same on every start, which keeps the
/// app as deterministic as the simulation.
fn noise() -> impl FnMut() -> f32 {
    let mut state = 0x2545_f491_4f6c_dd1du64;
    move || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 40) as f32 / 8_388_608.0 - 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::audio::{Decodable, Source};

    /// The buffer has to survive rodio — without the `wav` feature the decoder panics,
    /// and the app would go silent without a word.
    #[test]
    fn the_loops_decode() {
        let source = generate(|t| (t * TAU * 440.0).sin());
        let decoder = source.decoder();
        assert_eq!(decoder.sample_rate().get(), RATE);
        assert_eq!(decoder.channels().get(), 1);
        assert_eq!(decoder.count(), RATE as usize);
    }

    #[test]
    fn wav_buffer_is_well_formed() {
        let source = generate(|t| (t * TAU * 440.0).sin());
        let bytes = &source.bytes;
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[36..40], b"data");
        // Header plus one second of 16-bit mono.
        assert_eq!(bytes.len(), 44 + RATE as usize * 2);
        let data = u32::from_le_bytes(bytes[40..44].try_into().unwrap());
        assert_eq!(data as usize, bytes.len() - 44);
        assert_eq!(
            u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize,
            bytes.len() - 8
        );
    }

    /// A standing, unmanned vehicle is silent, and each channel answers to its own cue.
    #[test]
    fn every_channel_follows_its_cue() {
        let quiet = Cues {
            traction_pitch: 1.0,
            ..default()
        };
        for channel in [
            Channel::Rolling,
            Channel::Traction,
            Channel::Air,
            Channel::Compressor,
            Channel::Horn,
            Channel::Buzzer,
        ] {
            assert_eq!(level(channel, &quiet).0, 0.0, "{channel:?}");
        }

        // Rolling noise rises with speed and stops rising at the top.
        let slow = level(
            Channel::Rolling,
            &Cues {
                speed: 30.0,
                ..quiet
            },
        );
        let fast = level(
            Channel::Rolling,
            &Cues {
                speed: 90.0,
                ..quiet
            },
        );
        assert!(slow.0 > 0.0 && slow.0 < fast.0);
        assert!(fast.1 > slow.1, "faster also means higher pitched");
        let faster = level(
            Channel::Rolling,
            &Cues {
                speed: 250.0,
                ..quiet
            },
        );
        assert_eq!(fast.0, faster.0, "the volume is capped");

        // Buttons and lamps are on or off, nothing in between.
        assert!(
            level(
                Channel::Horn,
                &Cues {
                    horn: true,
                    ..quiet
                }
            )
            .0 > 0.0
        );
        assert!(
            level(
                Channel::Buzzer,
                &Cues {
                    alert: true,
                    ..quiet
                }
            )
            .0 > 0.0
        );
        assert!(
            level(
                Channel::Compressor,
                &Cues {
                    compressor: true,
                    ..quiet
                }
            )
            .0 > 0.0
        );
        // Air hisses at any pressure change, in either direction.
        assert!(level(Channel::Air, &Cues { air: 0.2, ..quiet }).0 > 0.0);
    }

    #[test]
    fn noise_stays_in_range_and_repeats() {
        let mut first = noise();
        let mut second = noise();
        let mut sum = 0.0;
        for _ in 0..10_000 {
            let value = first();
            assert!((-1.0..=1.0).contains(&value), "{value}");
            assert_eq!(value, second());
            sum += value;
        }
        // No DC offset — otherwise the loop would thump at every seam.
        assert!(sum.abs() / 10_000.0 < 0.05, "{sum}");
    }
}
