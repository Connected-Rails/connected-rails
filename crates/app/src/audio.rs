//! Sound (plan ch. 13) — the vehicle's sound table put to work.
//!
//! Nothing here decides *what* a train sounds like. That is written in the vehicle file
//! ([`sim_core::sound`]): which sample follows which quantity, under which conditions,
//! started by which trigger. This module only does what needs an audio device — it turns
//! the entries into sinks, follows their curves every frame and detects the edges that a
//! trigger fires on.
//!
//! Two kinds of entry come out of the same table:
//!
//! - **without a trigger** — a loop whose volume and pitch are modulated (rolling noise,
//!   traction, air).
//! - **with a trigger** — a one-shot, spawned in the frame the edge is crossed (rail joints,
//!   tap changer contactors).
//!
//! A vehicle without a table of its own runs on [`sim_core::sound::default_table`], and its
//! samples are **generated**: a handful of oscillators and a noise generator written into a
//! WAV buffer at start-up, addressed as `synth:<name>`. So the repository carries no binary
//! samples, and a mod that brings its own files takes exactly the same path — only the
//! `file` of the entry changes.

use crate::render::VehicleView;
use crate::{PlayerTrain, SimResource, models, ui};
use bevy::audio::{AudioSinkPlayback, SpatialAudioSink, SpatialListener, Volume};
use bevy::prelude::*;
use sim_core::sound::{SoundSpec, SoundState, default_table};
use sim_core::train::VehicleSpec;
use std::collections::HashMap;
use std::f32::consts::TAU;

/// Sample rate of the generated sources [Hz]. Enough for engine hum and hiss, and every
/// buffer stays under 50 kB.
const RATE: u32 = 22_050;

/// Gap between the ears of the listener [m] — the stereo base of the cab.
const EAR_GAP: f32 = 0.3;

/// Which entry of which vehicle a sink plays.
#[derive(Component, Clone, Copy, Debug)]
pub struct Sound {
    train: usize,
    vehicle: usize,
    /// Index into the vehicle's sound table.
    entry: usize,
}

/// The sources behind the tables, by the `file` of the entry.
#[derive(Resource)]
pub struct Sounds {
    sources: HashMap<String, Handle<AudioSource>>,
    /// The table a vehicle without one of its own runs on.
    default: Vec<SoundSpec>,
    /// View entity of every vehicle — what a placed sound hangs off. Vehicles are spawned
    /// once at start-up, so this is looked up here instead of re-queried every frame.
    emitters: HashMap<(usize, usize), Entity>,
}

impl Sounds {
    /// The sound table of a vehicle.
    ///
    /// ponytail: a hauled vehicle without a table of its own stays silent instead of
    /// inheriting the default — that one carries a compressor and a horn, which a coach has
    /// no business making. A coach that is to roll audibly writes its own two entries.
    fn table<'a>(&'a self, spec: &'a VehicleSpec) -> &'a [SoundSpec] {
        if !spec.sounds.is_empty() {
            &spec.sounds
        } else if spec.traction.is_some() {
            &self.default
        } else {
            &[]
        }
    }
}

/// Creates the sources, places the listener and starts every loop silently.
///
/// Runs in `PostStartup`: the trains and their view entities are created by `setup`, and the
/// commands of a `Startup` system are only applied afterwards.
pub fn setup_audio(
    mut commands: Commands,
    mut assets: ResMut<Assets<AudioSource>>,
    server: Res<AssetServer>,
    sim: Res<SimResource>,
    player: Res<PlayerTrain>,
    views: Query<(Entity, &VehicleView)>,
    camera: Query<Entity, With<ui::CabCamera>>,
) {
    // The listener sits at the camera — the cab, or the outside view.
    if let Ok(camera) = camera.single() {
        commands
            .entity(camera)
            .insert(SpatialListener::new(EAR_GAP));
    }

    let mut bank = Sounds {
        sources: HashMap::new(),
        default: default_table(),
        emitters: views
            .iter()
            .map(|(entity, view)| ((view.train, view.vehicle), entity))
            .collect(),
    };

    // Which files the loaded vehicles actually ask for — a mod's table may name any of them.
    let mut wanted: Vec<String> = Vec::new();
    for train in &sim.0.trains {
        for vehicle in &train.vehicles {
            for entry in bank.table(&vehicle.spec) {
                if !wanted.contains(&entry.file) {
                    wanted.push(entry.file.clone());
                }
            }
        }
    }
    for file in wanted {
        // `synth:<name>` is generated here, everything else is a sample out of a mod.
        let handle = match file.strip_prefix("synth:") {
            Some(name) => match synth(name) {
                Some(source) => assets.add(source),
                None => {
                    warn!("sound: unknown generated source {file}");
                    continue;
                }
            },
            None => server.load(models::asset_path(&file)),
        };
        bank.sources.insert(file, handle);
    }

    let (mut loops, mut triggered) = (0, 0);
    for (t, train) in sim.0.trains.iter().enumerate() {
        for (v, vehicle) in train.vehicles.iter().enumerate() {
            for (i, entry) in bank.table(&vehicle.spec).iter().enumerate() {
                if !entry.is_loop() {
                    triggered += 1;
                    continue;
                }
                loops += 1;
                let Some(handle) = bank.sources.get(&entry.file) else {
                    continue;
                };
                let marker = Sound {
                    train: t,
                    vehicle: v,
                    entry: i,
                };
                let settings = PlaybackSettings {
                    spatial: entry.positional,
                    ..PlaybackSettings::LOOP
                }
                .with_volume(Volume::SILENT);
                let bundle = (AudioPlayer::new(handle.clone()), settings, marker);
                match bank.emitters.get(&(t, v)) {
                    // Placed in the world: the sink rides along on the vehicle, so distance
                    // attenuation and Doppler fall out of its transform.
                    Some(parent) if entry.positional => {
                        commands
                            .entity(*parent)
                            .with_child((bundle, Transform::default()));
                    }
                    // Not placed: a cab sound. Only the train being driven has a cab that
                    // anyone is sitting in.
                    _ if t == player.0 && !entry.positional => {
                        commands.spawn(bundle);
                    }
                    _ => {}
                }
            }
        }
    }
    info!(
        "Sound: {loops} loops and {triggered} triggered entries from {} sources",
        bank.sources.len()
    );
    commands.insert_resource(bank);
}

/// Follows the curves of every loop and fires the triggered entries.
pub fn update_audio(
    mut commands: Commands,
    sim: Res<SimResource>,
    bank: Res<Sounds>,
    time: Res<Time>,
    // The state of the previous frame: triggers need an edge, air a difference.
    mut previous: Local<HashMap<(usize, usize), SoundState>>,
    mut plain: Query<(&Sound, &mut AudioSink)>,
    mut spatial: Query<(&Sound, &mut SpatialAudioSink)>,
) {
    let dt = time.delta_secs_f64().clamp(1e-3, 0.25);
    let sim = &sim.0;

    // One reading per vehicle — every entry of that vehicle is evaluated against it.
    let mut states: HashMap<(usize, usize), SoundState> = HashMap::new();
    for (t, train) in sim.trains.iter().enumerate() {
        let cab = sim.controls[t];
        let alert = sim.runtime[t].protection.alert;
        for (v, vehicle) in train.vehicles.iter().enumerate() {
            if bank.table(&vehicle.spec).is_empty() {
                continue;
            }
            let state = SoundState::sample(vehicle, &cab, alert, previous.get(&(t, v)), dt);
            states.insert((t, v), state);
        }
    }

    // Loops: volume and pitch, faded rather than set hard — a volume that jumps clicks.
    let fade = (dt as f32 * 12.0).min(1.0);
    let level = |sound: &Sound| -> Option<(f32, f32)> {
        let state = states.get(&(sound.train, sound.vehicle))?;
        let spec = sim.trains.get(sound.train)?.vehicles.get(sound.vehicle)?;
        let entry = bank.table(&spec.spec).get(sound.entry)?;
        let (volume, pitch) = entry.level(state);
        Some((volume as f32, pitch as f32))
    };
    for (sound, mut sink) in plain.iter_mut() {
        if let Some((volume, pitch)) = level(sound) {
            apply(&mut *sink, volume, pitch, fade);
        }
    }
    for (sound, mut sink) in spatial.iter_mut() {
        if let Some((volume, pitch)) = level(sound) {
            apply(&mut *sink, volume, pitch, fade);
        }
    }

    // Triggered entries: one-shots, spawned in the frame the edge is crossed.
    for (&(t, v), state) in states.iter() {
        let Some(before) = previous.get(&(t, v)) else {
            // The first frame has no edge — everything would fire at once.
            continue;
        };
        let spec = &sim.trains[t].vehicles[v].spec;
        for entry in bank.table(spec).iter().filter(|e| !e.is_loop()) {
            if !entry.fires(state, before) {
                continue;
            }
            let Some(handle) = bank.sources.get(&entry.file) else {
                continue;
            };
            let (volume, pitch) = entry.level(state);
            let settings = PlaybackSettings {
                spatial: entry.positional,
                ..PlaybackSettings::DESPAWN
            }
            .with_volume(Volume::Linear(volume as f32))
            .with_speed(pitch as f32);
            let bundle = (AudioPlayer::new(handle.clone()), settings);
            match bank.emitters.get(&(t, v)) {
                Some(parent) if entry.positional => {
                    commands
                        .entity(*parent)
                        .with_child((bundle, Transform::default()));
                }
                _ => {
                    commands.spawn(bundle);
                }
            }
        }
    }

    *previous = states;
}

/// Volume (faded) and playback speed of one sink.
fn apply(sink: &mut impl AudioSinkPlayback, volume: f32, pitch: f32, fade: f32) {
    let faded = sink.volume().fade_towards(Volume::Linear(volume), fade);
    sink.set_volume(faded);
    sink.set_speed(pitch);
}

/// The generated sources, addressed as `synth:<name>` from a sound table.
///
/// Loops are one second long so that every whole frequency runs through a whole number of
/// periods and the loop has no seam; the one-shots are as short as the noise they stand for.
fn synth(name: &str) -> Option<AudioSource> {
    let source = match name {
        // Rolling: low-passed noise plus the rumble of the running gear.
        "rolling" => {
            let mut low = 0.0;
            let mut rumble = noise();
            generate(1.0, move |t| {
                low += (rumble() - low) * 0.08;
                low * 3.0 + (t * TAU * 50.0).sin() * 0.15
            })
        }
        // Traction: converter whine — a fundamental with two harmonics.
        "traction" => generate(1.0, |t| {
            tone(t, &[(200.0, 0.5), (400.0, 0.3), (800.0, 0.12)])
        }),
        // Air: white noise, high-passed so it hisses instead of rumbling.
        "air" => {
            let mut low = 0.0;
            let mut hiss = noise();
            generate(1.0, move |t| {
                let white = hiss();
                low += (white - low) * 0.35;
                (white - low) * 0.8 + (t * TAU * 120.0).sin() * 0.05
            })
        }
        // Compressor: a low hum, chugging six times a second.
        "compressor" => generate(1.0, |t| {
            tone(t, &[(80.0, 0.6), (160.0, 0.2)]) * (0.6 + 0.4 * (t * TAU * 6.0).sin())
        }),
        // Horn: the two-tone of a Makrofon, both notes with their octave.
        "horn" => generate(1.0, |t| {
            tone(t, &[(370.0, 0.4), (440.0, 0.4), (740.0, 0.1), (880.0, 0.1)])
        }),
        // Buzzer: 800 Hz with odd harmonics — that is what makes it nag rather than sing.
        "buzzer" => generate(1.0, |t| {
            tone(t, &[(800.0, 0.5), (2400.0, 0.17), (4000.0, 0.1)])
        }),
        // Brake squeal: a high note that wanders, the way a block does on the tread.
        "squeal" => generate(1.0, |t| {
            let wobble = 1.0 + 0.02 * (t * TAU * 7.0).sin();
            tone(t, &[(2100.0 * wobble, 0.35), (4200.0 * wobble, 0.12)])
        }),
        // Rail joint: a noise burst that decays — the wheel dropping into the gap.
        "joint" => {
            let mut burst = noise();
            generate(0.14, move |t| {
                (burst() * 0.7 + (t * TAU * 90.0).sin() * 0.5) * decay(t, 22.0)
            })
        }
        // Contactor: the same shape, shorter and metallic — a tap changer notch.
        "contactor" => {
            let mut click = noise();
            generate(0.09, move |t| {
                (click() * 0.5 + tone(t, &[(1300.0, 0.4), (2600.0, 0.2)])) * decay(t, 45.0)
            })
        }
        _ => return None,
    };
    Some(source)
}

/// Exponential envelope of a one-shot: 1 at the start, silent at the end.
fn decay(t: f32, rate: f32) -> f32 {
    (-t * rate).exp()
}

/// `seconds` of mono 16-bit PCM in a WAV container.
fn generate(seconds: f32, mut sample: impl FnMut(f32) -> f32) -> AudioSource {
    let count = (RATE as f32 * seconds) as usize;
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
    use sim_core::sound::default_table;

    /// The buffer has to survive rodio — without the `wav` feature the decoder panics,
    /// and the app would go silent without a word.
    #[test]
    fn the_loops_decode() {
        let source = generate(1.0, |t| (t * TAU * 440.0).sin());
        let decoder = source.decoder();
        assert_eq!(decoder.sample_rate().get(), RATE);
        assert_eq!(decoder.channels().get(), 1);
        assert_eq!(decoder.count(), RATE as usize);
    }

    #[test]
    fn wav_buffer_is_well_formed() {
        let source = generate(1.0, |t| (t * TAU * 440.0).sin());
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
        // A one-shot is short, and short enough to still be a valid buffer.
        let click = generate(0.09, |t| decay(t, 45.0));
        assert_eq!(click.bytes.len(), 44 + (RATE as f32 * 0.09) as usize * 2);
    }

    /// Every `synth:` name the default table asks for has to exist — a typo would leave the
    /// simulator silent with nothing but a warning in the log.
    #[test]
    fn the_default_table_finds_all_its_sources() {
        for entry in default_table() {
            let name = entry
                .file
                .strip_prefix("synth:")
                .unwrap_or_else(|| panic!("{} is not generated", entry.file));
            assert!(synth(name).is_some(), "{name}");
        }
        assert!(synth("nonexistent").is_none());
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

    /// The envelope of a one-shot has to be gone by the end of the buffer, otherwise the
    /// sound is cut off with a click.
    #[test]
    fn one_shots_decay_to_silence() {
        assert_eq!(decay(0.0, 22.0), 1.0);
        assert!(decay(0.14, 22.0) < 0.05);
        assert!(decay(0.09, 45.0) < 0.02);
    }
}
