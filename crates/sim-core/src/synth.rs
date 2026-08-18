//! Generated sound sources (plan ch. 13) — what `synth:<name>` in a sound table resolves to.
//!
//! The repository carries no recorded samples, so the sources every vehicle falls back on
//! are computed: oscillator stacks and filtered noise in a mono buffer at [`RATE`]. A mod
//! that brings real recordings takes exactly the same path — only the `file` of the entry
//! changes, and the app loads it from disk instead of calling in here. This lives in
//! `sim-core` next to the table that names the sources, so the simulator and the vehicle
//! editor's preview hear the same thing.
//!
//! Two properties of the loops matter more than the waveforms:
//!
//! - **Length.** [`LOOP_SECONDS`] is the period at which a listener stops hearing a machine
//!   and starts hearing a buffer. Every partial is a whole multiple of `1 / LOOP_SECONDS`, so
//!   the seam is silent, and [`wander`] drifts the level over the whole buffer so the
//!   repetition does not lock into the ear.
//! - **Bands.** `rolling-*` and `traction-*` come in three each. The table crossfades them
//!   over speed or engine speed ([`Curve::window`](crate::sound::Curve::window)), so no layer
//!   is ever resampled far. One loop stretched from 0.7 to 1.7 drags its formants along and
//!   arrives at the top of the range as a toy train.
//!
//! Levels are not hand-tuned: every loop is normalised to [`LOOP_RMS`] and every one-shot to
//! full scale. Two layers that meet in a crossfade therefore meet at the same loudness, which
//! is the one thing a listener notices immediately if it is wrong.

use std::f32::consts::TAU;

/// Sample rate of the generated sources [Hz]. CD rate rather than the 22 kHz of the first
/// draft: rail noise and brake squeal carry well above the 11 kHz Nyquist that cost.
pub const RATE: u32 = 44_100;

/// Length of a generated loop [s]. Every partial has to be a whole multiple of
/// `1 / LOOP_SECONDS` for the seam to stay silent — with 4 s that is a 0.25 Hz grid, which
/// every frequency below happens to sit on.
pub const LOOP_SECONDS: f32 = 4.0;

/// Target RMS of a normalised loop. Leaves head-room for the sum of a dozen entries before
/// the master limiter has to work.
const LOOP_RMS: f32 = 0.35;

/// How much of the loop's head the tail is faded over [s] — long enough to hide the step in
/// a noise stream, short enough that nothing is audibly doubled.
const CROSSFADE_SECONDS: f32 = 0.03;

/// Every generated source. The vehicle editor offers this list; the tests walk it.
pub const NAMES: [&str; 15] = [
    "rolling",
    "rolling-low",
    "rolling-mid",
    "rolling-high",
    "traction",
    "traction-low",
    "traction-mid",
    "traction-high",
    "air",
    "compressor",
    "horn",
    "buzzer",
    "squeal",
    "joint",
    "contactor",
];

/// The mono samples behind `synth:<name>`, or `None` for a name nothing generates.
///
/// `rolling` and `traction` without a band suffix are the middle band. They are what the
/// tables named before there were bands, and a mod file that still says so keeps working.
pub fn synth(name: &str) -> Option<Vec<f32>> {
    // Resolve the alias before the seed is taken, or `rolling` and `rolling-mid` would
    // come out as two different noise streams under one description.
    let name = match name {
        "rolling" => "rolling-mid",
        "traction" => "traction-mid",
        other => other,
    };
    let seed = seed(name);
    let source = match name {
        // Rolling noise in three bands. `low` is the rumble of the running gear at
        // shunting speed, `high` the hiss of the wheel on the rail at line speed.
        "rolling-low" => rolling(seed, 0.0, 0.03, 25.0, 0.30),
        "rolling-mid" => rolling(seed, 0.0, 0.09, 50.0, 0.15),
        "rolling-high" => rolling(seed, 0.02, 0.45, 100.0, 0.06),
        // Traction: converter whine. Each band a fundamental with harmonics, and a
        // partial one hertz off the fundamental so the note beats instead of standing
        // still — a perfectly steady tone is the giveaway of a synthesised loop.
        "traction-low" => tone_loop(&[(150.0, 0.5), (151.0, 0.2), (300.0, 0.25), (450.0, 0.12)]),
        "traction-mid" => tone_loop(&[(200.0, 0.5), (201.0, 0.2), (400.0, 0.3), (800.0, 0.12)]),
        "traction-high" => tone_loop(&[
            (600.0, 0.45),
            (601.0, 0.18),
            (1200.0, 0.25),
            (1800.0, 0.12),
            (2400.0, 0.06),
        ]),
        // Air: white noise with the bass taken out, so it hisses instead of rumbling.
        "air" => {
            let mut low = 0.0;
            let mut hiss = noise(seed);
            loop_buffer(move |t| {
                let white = hiss();
                low += (white - low) * 0.35;
                ((white - low) + (t * TAU * 120.0).sin() * 0.05) * wander(t)
            })
        }
        // Compressor: a low hum, chugging six times a second.
        "compressor" => loop_buffer(|t| {
            tone(t, &[(80.0, 0.6), (160.0, 0.2)]) * (0.6 + 0.4 * (t * TAU * 6.0).sin())
        }),
        // Horn: the two-tone of a Makrofon, both notes with their octave.
        "horn" => tone_loop(&[(370.0, 0.4), (440.0, 0.4), (740.0, 0.1), (880.0, 0.1)]),
        // Buzzer: 800 Hz with odd harmonics — that is what makes it nag rather than sing.
        "buzzer" => tone_loop(&[(800.0, 0.5), (2400.0, 0.17), (4000.0, 0.1)]),
        // Brake squeal: a high note that wanders, the way a block does on the tread.
        "squeal" => loop_buffer(|t| {
            let wobble = 1.0 + 0.02 * (t * TAU * 7.0).sin();
            tone(t, &[(2100.0 * wobble, 0.35), (4200.0 * wobble, 0.12)])
        }),
        // Rail joint: a noise burst that decays — the wheel dropping into the gap.
        "joint" => {
            let mut burst = noise(seed);
            one_shot(0.14, move |t| {
                (burst() * 0.7 + (t * TAU * 90.0).sin() * 0.5) * decay(t, 22.0)
            })
        }
        // Contactor: the same shape, shorter and metallic — a tap changer notch.
        "contactor" => {
            let mut click = noise(seed);
            one_shot(0.09, move |t| {
                (click() * 0.5 + tone(t, &[(1300.0, 0.4), (2600.0, 0.2)])) * decay(t, 45.0)
            })
        }
        _ => return None,
    };
    Some(source)
}

/// Rolling noise of one band: white noise between two one-pole edges, plus the hum of the
/// running gear. `low` is the coefficient of the edge that is subtracted again — 0 leaves a
/// plain lowpass and therefore a rumble, a larger `high` opens the band towards a hiss.
fn rolling(seed: u64, low: f32, high: f32, hum: f32, hum_level: f32) -> Vec<f32> {
    let (mut upper, mut lower) = (0.0, 0.0);
    let mut white = noise(seed);
    loop_buffer(move |t| {
        let x = white();
        upper += (x - upper) * high;
        lower += (x - lower) * low;
        ((upper - lower) + (t * TAU * hum).sin() * hum_level) * wander(t)
    })
}

/// A loop of nothing but partials, with the slow level drift on top.
fn tone_loop(partials: &[(f32, f32)]) -> Vec<f32> {
    let partials = partials.to_vec();
    loop_buffer(move |t| tone(t, &partials) * wander(t))
}

/// Slow level drift over the buffer, 1.0 on average. Both rates are whole multiples of
/// `1 / LOOP_SECONDS`, so the drift is seamless across the loop point like everything else.
fn wander(t: f32) -> f32 {
    1.0 + 0.12 * (t * TAU * 0.25).sin() + 0.07 * (t * TAU * 0.75).sin()
}

/// Exponential envelope of a one-shot: 1 at the start, silent at the end.
fn decay(t: f32, rate: f32) -> f32 {
    (-t * rate).exp()
}

/// Sum of sine partials: `(frequency [Hz], amplitude)`.
fn tone(t: f32, partials: &[(f32, f32)]) -> f32 {
    partials
        .iter()
        .map(|(frequency, amplitude)| (t * frequency * TAU).sin() * amplitude)
        .sum()
}

/// A [`LOOP_SECONDS`] buffer, seam crossfaded and normalised to [`LOOP_RMS`].
///
/// The crossfade is what makes a noise loop usable at all. Partials are periodic over the
/// buffer by construction, so their ends meet; noise is not, and the step from the last
/// sample back to the first is a click every four seconds for the whole run. So the
/// generator is run [`CROSSFADE_SECONDS`] past the end and that tail is faded over the head.
/// For the tonal part that is a no-op — `tone(t + LOOP_SECONDS) == tone(t)` exactly — and
/// for the noise it is the join.
fn loop_buffer(sample: impl FnMut(f32) -> f32) -> Vec<f32> {
    let mut buffer = generate(LOOP_SECONDS + CROSSFADE_SECONDS, sample);
    let length = (RATE as f32 * LOOP_SECONDS) as usize;
    let fade = (RATE as f32 * CROSSFADE_SECONDS) as usize;
    for i in 0..fade {
        let head = i as f32 / fade as f32;
        buffer[i] = buffer[i] * head + buffer[length + i] * (1.0 - head);
    }
    buffer.truncate(length);
    let rms = (buffer.iter().map(|s| s * s).sum::<f32>() / buffer.len().max(1) as f32).sqrt();
    // A silent buffer would divide by zero; nothing here produces one, but a mod-facing
    // generator has no business trusting that.
    let gain = if rms > 1e-6 { LOOP_RMS / rms } else { 0.0 };
    scale(&mut buffer, gain);
    buffer
}

/// A short buffer, normalised to full scale — a transient is judged by its peak, not its
/// average.
fn one_shot(seconds: f32, sample: impl FnMut(f32) -> f32) -> Vec<f32> {
    let mut buffer = generate(seconds, sample);
    let peak = buffer.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let gain = if peak > 1e-6 { 0.95 / peak } else { 0.0 };
    scale(&mut buffer, gain);
    buffer
}

/// `seconds` of mono samples.
fn generate(seconds: f32, mut sample: impl FnMut(f32) -> f32) -> Vec<f32> {
    let count = (RATE as f32 * seconds) as usize;
    (0..count).map(|i| sample(i as f32 / RATE as f32)).collect()
}

/// Applies a gain and clips — normalisation can push a peaky buffer over full scale.
fn scale(buffer: &mut [f32], gain: f32) {
    for sample in buffer.iter_mut() {
        *sample = (*sample * gain).clamp(-1.0, 1.0);
    }
}

/// White noise from the given seed — the buffer is the same on every start, which keeps the
/// app as deterministic as the simulation, but every source gets its own stream so two
/// bands do not correlate into one voice.
fn noise(seed: u64) -> impl FnMut() -> f32 {
    let mut state = seed | 1;
    move || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 40) as f32 / 8_388_608.0 - 1.0
    }
}

/// FNV-1a over the source name — the seed of its noise stream.
fn seed(name: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in name.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_name_generates_and_nothing_else_does() {
        for name in NAMES {
            let buffer = synth(name).unwrap_or_else(|| panic!("{name}"));
            assert!(!buffer.is_empty(), "{name}");
            assert!(buffer.iter().all(|s| (-1.0..=1.0).contains(s)), "{name}");
        }
        assert!(synth("nonexistent").is_none());
    }

    /// Every loop is [`LOOP_SECONDS`] long and every one-shot shorter — the table treats
    /// them differently, and a loop that came out short would stutter.
    #[test]
    fn loops_are_full_length_and_one_shots_are_not() {
        let full = (RATE as f32 * LOOP_SECONDS) as usize;
        for name in NAMES {
            let len = synth(name).unwrap().len();
            match name {
                "joint" | "contactor" => assert!(len < full / 4, "{name}: {len}"),
                _ => assert_eq!(len, full, "{name}"),
            }
        }
    }

    /// The point of the normalisation: two bands that crossfade must not step in level.
    #[test]
    fn the_bands_match_in_loudness() {
        let rms = |name: &str| {
            let buffer = synth(name).unwrap();
            (buffer.iter().map(|s| s * s).sum::<f32>() / buffer.len() as f32).sqrt()
        };
        for name in ["rolling-low", "rolling-mid", "rolling-high"] {
            assert!((rms(name) - LOOP_RMS).abs() < 0.02, "{name}: {}", rms(name));
        }
        for name in ["traction-low", "traction-mid", "traction-high"] {
            assert!((rms(name) - LOOP_RMS).abs() < 0.02, "{name}: {}", rms(name));
        }
        // The names without a suffix are the middle band, byte for byte.
        assert_eq!(synth("rolling"), synth("rolling-mid"));
        assert_eq!(synth("traction"), synth("traction-mid"));
    }

    /// A loop is glued to itself every [`LOOP_SECONDS`]. If the step across that seam is
    /// bigger than the steps the waveform takes anyway, it clicks — audibly, every four
    /// seconds, for the whole run.
    #[test]
    fn the_loop_seam_does_not_click() {
        for name in NAMES {
            if matches!(name, "joint" | "contactor") {
                continue;
            }
            let buffer = synth(name).unwrap();
            let largest = buffer
                .windows(2)
                .map(|w| (w[1] - w[0]).abs())
                .fold(0.0f32, f32::max);
            let seam = (buffer[0] - buffer[buffer.len() - 1]).abs();
            assert!(
                seam <= largest,
                "{name}: seam {seam}, largest step {largest}"
            );
        }
    }

    /// Two bands must not be the same noise at a different filter setting — that would fold
    /// them into one louder voice in the crossfade instead of two.
    #[test]
    fn each_source_gets_its_own_noise() {
        assert_ne!(seed("rolling-low"), seed("rolling-high"));
        let mut a = noise(seed("rolling-low"));
        let mut b = noise(seed("rolling-high"));
        assert!((0..1000).any(|_| a() != b()));
    }

    #[test]
    fn noise_stays_in_range_and_repeats() {
        let mut first = noise(seed("air"));
        let mut second = noise(seed("air"));
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

    /// The drift has to average out, or the loop would be quieter or louder than the
    /// normalisation says.
    #[test]
    fn the_wander_averages_to_one() {
        let samples = 10_000;
        let mean = (0..samples)
            .map(|i| wander(i as f32 / samples as f32 * LOOP_SECONDS))
            .sum::<f32>()
            / samples as f32;
        assert!((mean - 1.0).abs() < 0.01, "{mean}");
    }
}
