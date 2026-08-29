#!/usr/bin/env python3
"""Cuts the BR 101's driving-noise loops out of their published recordings.

The example vehicle's sound table (`mods/example/vehicles/br101_afb.ron`) plays
loops of the real locomotive: the converters' whine of a standing 101, the
GTO converter singing at the start and braking electrically, the DB Makrofon,
the desk's signal buzzer, rolling noise in three speed bands, an air release
and a train compressor. None of that was recorded for this project — it is
cut out of Creative Commons trainspotting videos, a cab ride and Freesound
recordings, listed in `SOURCES` below and credited in THIRD_PARTY_LICENSES.md
(the two CC BY-SA sources bind the files cut from them to that licence).
This script is the cutting table: it downloads the sources and turns them
into the seamless mono Ogg Vorbis loops in `mods/example/assets/sounds/`, so
a cut can be redone or changed without anybody's ears in the loop.

    python3 tools/sounds/br101_sounds.py fetch            # sources → /tmp/br101-sources
    python3 tools/sounds/br101_sounds.py build --check /tmp/br101-check

`fetch` needs `yt-dlp` and `curl`, `build` needs `ffmpeg`; no Python packages.
`--check` writes a spectrogram of every loop repeated three times, which is
where a bad seam or a bystander's voice shows up.

Every loop is normalised to the RMS of the generated `synth:` loops
(`sim_core::synth::LOOP_RMS`), so the volume factors of the table carry over
and two bands that crossfade meet at the same loudness. Peaks are held under
full scale by a limiter run over three repetitions, with the middle one kept,
so the seam sees the same gain on both sides.
"""

from __future__ import annotations

import argparse
import cmath
import math
import operator
import subprocess
import sys
from array import array
from dataclasses import dataclass, field
from pathlib import Path

RATE = 48_000
# `sim_core::synth::LOOP_RMS`.
LOOP_RMS = 0.35
PEAK = 0.95
# Seconds decoded before the cut so that the noise tracker of `afftdn` has
# settled by the time the wanted part begins.
PRE_ROLL = 1.5

REPO = Path(__file__).resolve().parents[2]
OUT_DIR = REPO / "mods" / "example" / "assets" / "sounds"


@dataclass(frozen=True)
class Source:
    """One published recording. `file` is the name it is fetched under."""

    file: str
    url: str
    title: str
    author: str
    licence: str


SOURCES: dict[str, Source] = {
    # YouTube videos published under the Creative Commons Attribution licence
    # (YouTube's "reuse allowed" option, CC BY 3.0). Fetched as audio only.
    "bochum": Source(
        "Eb51FqjK31o.webm",
        "https://www.youtube.com/watch?v=Eb51FqjK31o",
        "BR 101 Abfahrt Bochum Hbf",
        "Zugfan 110",
        "CC BY 3.0",
    ),
    "bochum-makro": Source(
        "KF-2QQvIB4o.webm",
        "https://www.youtube.com/watch?v=KF-2QQvIB4o",
        "Br101 Abfahrt Bochum Hbf (Makro)",
        "Zugfan 110",
        "CC BY 3.0",
    ),
    "duesseldorf": Source(
        "qhlQ0r_I3z0.webm",
        "https://www.youtube.com/watch?v=qhlQ0r_I3z0",
        "BR101 Abfahrt Düsseldorf Hbf",
        "Zugfan 110",
        "CC BY 3.0",
    ),
    "magdeburg": Source(
        "W6yEO0nOb2g.webm",
        "https://www.youtube.com/watch?v=W6yEO0nOb2g",
        "DB 101 070-1 mit IC 2442 nach Hannover Hbf",
        "TheZugBox",
        "CC BY 3.0",
    ),
    "augsburg": Source(
        "7Q374vGrWuo.m4a",
        "https://www.youtube.com/watch?v=7Q374vGrWuo",
        "101 094-1 mit IC 2261 durch A-Oberhausen",
        "AugsburgerTrainspotter",
        "CC BY 3.0",
    ),
    "sfs": Source(
        "bRNnhwc2FZM.webm",
        "https://www.youtube.com/watch?v=bRNnhwc2FZM",
        "Züge am Pfingstbergtunnel (SFS Mannheim Stuttgart) mit ICE 1, 3, 4, Velaro D und ICs BR101",
        "Paul",
        "CC BY 3.0",
    ),
    # Wikimedia Commons, CC BY-SA — the loops cut from these stay CC BY-SA. The
    # cab ride is fetched as the site's 480p transcode: same audio as the 1.9 GB
    # original, a twentieth of the size.
    "cabride": Source(
        "Cab_Ride_Hamburg_Hbf_to_Hamburg_Altona_BR_101_NJ_470.480p.webm",
        "https://upload.wikimedia.org/wikipedia/commons/transcoded/a/a1/Cab_Ride_Hamburg_Hbf_to_Hamburg_Altona_BR_101_NJ_470.webm/Cab_Ride_Hamburg_Hbf_to_Hamburg_Altona_BR_101_NJ_470.webm.480p.vp9.webm",
        "Cab Ride Hamburg Hbf to Hamburg Altona BR 101 NJ 470 (https://commons.wikimedia.org/wiki/File:Cab_Ride_Hamburg_Hbf_to_Hamburg_Altona_BR_101_NJ_470.webm)",
        "IC-Lokführer",
        "CC BY-SA 4.0",
    ),
    "makrofon-628": Source(
        "Makrofon_DB_Baureihe_628.ogg",
        "https://upload.wikimedia.org/wikipedia/commons/e/e9/Makrofon_DB_Baureihe_628.ogg",
        "Makrofon DB Baureihe 628 (https://commons.wikimedia.org/wiki/File:Makrofon_DB_Baureihe_628.ogg)",
        "MdE",
        "CC BY-SA 3.0",
    ),
    # Freesound recordings under CC0 1.0, fetched as the site's preview MP3 —
    # the originals sit behind a login, and the processing below does not keep
    # what the preview loses.
    "air": Source(
        "388568.mp3",
        "https://cdn.freesound.org/previews/388/388568_14360-hq.mp3",
        "Train Air Brake 01.wav (https://freesound.org/s/388568/)",
        "totalcult",
        "CC0 1.0",
    ),
    "compressor": Source(
        "349145.mp3",
        "https://cdn.freesound.org/previews/349/349145_2792951-hq.mp3",
        "Tilt Train Compressor (https://freesound.org/s/349145/)",
        "Yoyodaman234",
        "CC0 1.0",
    ),
    "door-buzzer": Source(
        "332563.mp3",
        "https://cdn.freesound.org/previews/332/332563_5850523-hq.mp3",
        "buzzer.wav (https://freesound.org/s/332563/)",
        "011-_11919_1-1011111",
        "CC0 1.0",
    ),
    "alarm-buzzer": Source(
        "524909.mp3",
        "https://cdn.freesound.org/previews/524/524909_10345385-hq.mp3",
        "Buzzer_Alarm.wav (https://freesound.org/s/524909/)",
        "Engineer_815",
        "CC0 1.0",
    ),
}

# The converters of a 101 pulse at a fixed 500 Hz: a comb of 500 Hz and its
# harmonics on every platform recording, faint while the loco stands with the
# main switch closed (501, 1003, 1504, 2002, 2505 Hz in Magdeburg) and the
# dominant sound of the start, before the pulse pattern begins to climb with
# the motor frequency (504, 1008, 1510, 2002 Hz in Bochum). The standing whine
# is its own entry in the table at a fixed pitch; the start is the traction
# loop, which the table holds at its own pitch up to 20 km/h and pitches up
# from there. The standing loop is resampled by the ratio of the two combs so
# that both play the same 504 Hz and add up instead of beating.
COMB_STANDING_HZ = 501.5
COMB_START_HZ = 504.0
DENOISE = "afftdn=nr={nr}:nf=-50:tn=1"
# The converter's lines sit between 500 Hz and 4 kHz on a platform recording;
# what is outside is wind, the station and the transformer's hum, which does
# not follow the speed and so must not be in a loop whose pitch does (the
# high-pass is doubled to take it out). The station between the lines goes in
# `tonal`, since the loop's volume follows the tractive effort and a noise
# floor that comes and goes with the lever gives the game away.
TRACTION = "highpass=f=300,highpass=f=300,lowpass=f=4500"
# Resampling factors of the upper traction bands. Each band's pitch curve
# runs 0.85 … 1.25 over its window, so the factor is what makes the pitch
# continuous through the middle of the handover to the band below: at 55 km/h
# the low band plays at 1.175, at 155 km/h the mid band at 1.179 × its factor.
MID_RATE = 1.32
HIGH_RATE = 1.74
# The electric brake is recorded inside the cab: the same band limits as the
# traction, plus a shelf against the cab wall.
EBRAKE = "highpass=f=250,highpass=f=250,lowpass=f=4500,highshelf=f=1200:g=4"


@dataclass(frozen=True)
class Cut:
    """One output loop: where it is cut from and how it is treated."""

    name: str
    source: str
    start: float
    length: float
    loop: float
    crossfade: float
    # ffmpeg audio filter chain applied to the decoded window (after the pre-roll).
    filters: str = "highpass=f=40"
    # `linear` holds the level of a tone across the seam, `power` that of noise.
    fade: str = "power"
    # For a tonal loop: how far [samples] the loop length may move either way
    # so the seam falls where the tail continues the head in phase — at least
    # one period of the lowest tone, so every phase is on offer.
    tune: int = 0
    # Keep only the lines of the spectrogram (see `tonal`).
    tonal: bool = False
    # Playback-rate factor applied by resampling: pitch and tempo together, the
    # way the table's pitch curve would do it, only baked into the file.
    rate: float = 1.0
    remark: str = ""


CUTS: list[Cut] = [
    Cut(
        "aux-idle",
        "magdeburg",
        8.5,
        8.0,
        loop=6.0,
        crossfade=0.8,
        filters="highpass=f=60,lowpass=f=4500",
        fade="linear",
        rate=COMB_START_HZ / COMB_STANDING_HZ,
        tune=100,
        tonal=True,
        remark="the loco standing at the platform before the departure",
    ),
    Cut(
        "traction-low",
        "bochum",
        12.0,
        3.5,
        loop=3.0,
        crossfade=0.3,
        filters=TRACTION,
        fade="linear",
        tune=200,
        tonal=True,
        remark="the GTO converters in the first seconds of the start",
    ),
    Cut(
        "traction-mid",
        "bochum",
        12.0,
        3.5,
        loop=2.0,
        crossfade=0.3,
        filters=TRACTION,
        fade="linear",
        rate=MID_RATE,
        tune=140,
        tonal=True,
        remark="the same start, resampled — no free recording of the loco at speed exists",
    ),
    Cut(
        "traction-high",
        "bochum",
        12.0,
        3.5,
        loop=1.4,
        crossfade=0.25,
        filters=TRACTION,
        fade="linear",
        rate=HIGH_RATE,
        tune=100,
        tonal=True,
        remark="the same start, resampled further",
    ),
    Cut(
        "rolling-low",
        "duesseldorf",
        36.0,
        12.0,
        loop=8.0,
        crossfade=1.0,
        remark="the coaches rolling out of the platform behind the loco",
    ),
    Cut(
        "rolling-mid",
        "augsburg",
        17.5,
        6.0,
        loop=5.0,
        crossfade=0.8,
        remark="the IC passing the platform at line speed",
    ),
    Cut(
        "rolling-high",
        "sfs",
        68.0,
        5.0,
        loop=4.0,
        crossfade=0.8,
        # The camera's own 500 Hz tone runs through the whole video.
        filters="highpass=f=40,bandreject=f=500:width_type=h:w=30",
        remark="a pass on the high-speed line, wheel and air at 200 km/h and more",
    ),
    # The 101's own Makrofon on the Bochum video holds a clean 608 Hz for only a
    # fifth of a second before it starts to beat (the uploader calls it "a
    # somewhat broken Makrofon"), and a fifth of a second looped is audibly a
    # loop. The class 628 recording is the same DB signal horn — 621 Hz against
    # the 101's 608 — held cleanly for two seconds; resampled the two per cent
    # down, it is the 101's pitch.
    Cut(
        "horn",
        "makrofon-628",
        0.55,
        1.5,
        loop=1.2,
        crossfade=0.1,
        filters="highpass=f=300",
        fade="linear",
        rate=608.0 / 621.0,
        tune=90,
        remark="the DB Makrofon, blown outside, at the 101's pitch",
    ),
    # No free recording of the PZB's own buzzer exists. The 101's desk buzzer on
    # the cab ride (it sounds at the departure order, 400 + 671 Hz) is a clean
    # two-tone that plays like a telephone key; an electromechanical buzzer is
    # what "Summer" means, and this one rasps for three and a half seconds.
    Cut(
        "pzb-buzzer",
        "door-buzzer",
        0.6,
        3.3,
        loop=2.6,
        crossfade=0.25,
        filters="highpass=f=150,lowpass=f=8000",
        remark="an electromechanical buzzer, the rasp of a real one",
    ),
    # The Sifa's buzzer: a different real buzzer, so the two are told apart by
    # ear — an alarm buzzer, steady, with the station between its lines taken
    # down a little.
    Cut(
        "sifa-buzzer",
        "alarm-buzzer",
        2.0,
        6.0,
        loop=4.0,
        crossfade=0.4,
        filters="highpass=f=150,lowpass=f=8000," + DENOISE.format(nr=12),
        remark="a steady alarm buzzer for the Sifa",
    ),
    # The electric brake: the converters as the loco brakes into Hamburg-Altona,
    # heard from the cab. A shelf lifts the treble the cab wall took, since the
    # loop is placed outside and the simulator's own cab wall muffles it again.
    Cut(
        "ebrake-low",
        "cabride",
        837.5,
        6.0,
        loop=3.0,
        crossfade=0.3,
        filters=EBRAKE,
        fade="linear",
        tune=150,
        tonal=True,
        remark="the converters braking into Hamburg-Altona",
    ),
    Cut(
        "ebrake-mid",
        "cabride",
        837.5,
        6.0,
        loop=2.0,
        crossfade=0.3,
        filters=EBRAKE,
        fade="linear",
        rate=MID_RATE,
        tune=110,
        tonal=True,
        remark="the same braking, resampled",
    ),
    Cut(
        "ebrake-high",
        "cabride",
        837.5,
        6.0,
        loop=1.4,
        crossfade=0.25,
        filters=EBRAKE,
        fade="linear",
        rate=HIGH_RATE,
        tune=90,
        tonal=True,
        remark="the same braking, resampled further",
    ),
    Cut(
        "air",
        "air",
        1.0,
        2.4,
        loop=1.8,
        crossfade=0.3,
        filters="highpass=f=150",
        remark="brake air released at a platform",
    ),
    Cut(
        "compressor",
        "compressor",
        6.0,
        8.0,
        loop=6.0,
        crossfade=0.8,
        filters="highpass=f=50",
        remark="a train's compressor running under the car",
    ),
]


def run(args: list[str], stdin: bytes | None = None) -> bytes:
    result = subprocess.run(args, input=stdin, capture_output=True, check=False)
    if result.returncode != 0:
        sys.exit(f"{args[0]} failed: {result.stderr.decode(errors='replace').strip()}")
    return result.stdout


def fetch(sources: Path) -> None:
    sources.mkdir(parents=True, exist_ok=True)
    for source in SOURCES.values():
        target = sources / source.file
        if target.exists():
            continue
        print(f"fetching {source.title} — {source.author}")
        if "youtube.com" in source.url:
            run(
                [
                    "yt-dlp",
                    "--no-warnings",
                    "-q",
                    "-f",
                    "bestaudio[ext=webm]/bestaudio",
                    "-o",
                    str(sources / "%(id)s.%(ext)s"),
                    "--",
                    source.url,
                ]
            )
        else:
            run(["curl", "-sL", "-o", str(target), source.url])
        if not target.exists():
            sys.exit(f"{source.file} did not arrive under {sources}")


def decode(path: Path, start: float, length: float, filters: str) -> array:
    """Mono float samples of one window, filtered, with the pre-roll cut off."""
    pre = min(PRE_ROLL, start)
    raw = run(
        [
            "ffmpeg",
            "-v",
            "error",
            "-i",
            str(path),
            "-ss",
            f"{start - pre:.3f}",
            "-t",
            f"{length + pre:.3f}",
            "-af",
            filters,
            "-ac",
            "1",
            "-ar",
            str(RATE),
            "-f",
            "f32le",
            "-",
        ]
    )
    samples = array("f")
    samples.frombytes(raw)
    return samples[int(pre * RATE) :]


def resample(samples: array, rate: float) -> array:
    """Plays the samples `rate` times faster: pitch and tempo together."""
    raw = run(
        [
            "ffmpeg",
            "-v",
            "error",
            "-f",
            "f32le",
            "-ar",
            str(RATE),
            "-ac",
            "1",
            "-i",
            "-",
            "-af",
            f"asetrate={RATE * rate},aresample={RATE}",
            "-f",
            "f32le",
            "-",
        ],
        samples.tobytes(),
    )
    out = array("f")
    out.frombytes(raw)
    return out


def fft(x: list[complex]) -> list[complex]:
    n = len(x)
    if n == 1:
        return x
    even, odd = fft(x[0::2]), fft(x[1::2])
    out = [0j] * n
    for k in range(n // 2):
        t = cmath.exp(-2j * math.pi * k / n) * odd[k]
        out[k] = even[k] + t
        out[k + n // 2] = even[k] - t
    return out


def ifft(x: list[complex]) -> list[complex]:
    n = len(x)
    return [v.conjugate() / n for v in fft([v.conjugate() for v in x])]


# Short-time Fourier transform of the tonal extraction: 4096 points at 48 kHz
# are 11.7 Hz a bin, hop a quarter of that.
STFT_N = 4096
STFT_HOP = STFT_N // 4
STFT_WINDOW = [math.sqrt(0.5 - 0.5 * math.cos(2 * math.pi * i / STFT_N)) for i in range(STFT_N)]


def tonal(samples: array, prominence_db: float = 12.0, floor_gain: float = 0.05) -> array:
    """Keeps the lines of a spectrogram and drops what lies between them.

    A converter's whine on a platform recording is a set of steady lines a
    dozen decibels above a broad floor of wind and station; a loop cut from it
    unchanged is mostly that floor, and a floor that swells with the tractive
    effort is what gives a sample away. So every bin that is a local peak in
    its frame — `prominence_db` above the median of its neighbourhood — and
    stays one over neighbouring frames is kept at full gain, its two
    neighbours with it, and everything else is turned down to `floor_gain`.
    Overlap-add with a root-Hann window on both sides puts the frames back.
    """
    n, hop = STFT_N, STFT_HOP
    padded = array("f", [0.0] * n) + samples + array("f", [0.0] * (2 * n))
    frames = (len(padded) - n) // hop
    spectra: list[list[complex]] = []
    masks: list[list[bool]] = []
    half = 24  # bins either side over which the floor is taken
    for f in range(frames):
        start = f * hop
        frame = [complex(padded[start + i] * STFT_WINDOW[i]) for i in range(n)]
        spectrum = fft(frame)
        spectra.append(spectrum)
        mag = [abs(v) for v in spectrum[: n // 2 + 1]]
        mask = [False] * len(mag)
        for k in range(half, len(mag) - half - 1):
            if mag[k] > mag[k - 1] and mag[k] >= mag[k + 1]:
                neighbourhood = sorted(mag[k - half : k + half + 1])
                floor = neighbourhood[half]
                if mag[k] > floor * 10 ** (prominence_db / 20):
                    mask[k] = True
        masks.append(mask)
    out = array("f", [0.0] * len(padded))
    bins = n // 2 + 1
    for f, spectrum in enumerate(spectra):
        gains = [floor_gain] * bins
        for k in range(1, bins - 1):
            # A line persists: this bin (or a neighbour) peaks in at least
            # four of the five frames around this one.
            votes = sum(
                1
                for g in range(max(0, f - 2), min(frames, f + 3))
                if masks[g][k] or masks[g][k - 1] or masks[g][k + 1]
            )
            if votes >= 4:
                gains[k] = 1.0
        shaped = [spectrum[k] * gains[k] for k in range(bins)]
        # Real signal: the upper half mirrors the lower.
        shaped += [shaped[n - k].conjugate() for k in range(bins, n)]
        frame = ifft(shaped)
        start = f * hop
        for i in range(n):
            out[start + i] += frame[i].real * STFT_WINDOW[i]
    # Root-Hann squared sums to 1.5 across the four overlapping frames.
    return array("f", (v / 1.5 for v in out[n : n + len(samples)]))


def correlation(samples: array, lag: int, count: int) -> float:
    head = samples[:count]
    tail = samples[lag : lag + count]
    energy = math.sqrt(sum(s * s for s in head) * sum(s * s for s in tail))
    return sum(map(operator.mul, head, tail)) / energy if energy > 0 else 0.0


def seam(samples: array, loop: float, crossfade: float, fade: str, tune: int) -> array:
    """Folds the tail over the head so the loop point is continuous.

    The output starts with what followed the loop's end, faded into what the
    loop began with, so the wrap from the last sample to the first is the
    recording's own continuation. With `tune`, the loop length is moved to
    where that continuation matches the head best — a tone then wraps in
    phase instead of beating through the crossfade.
    """
    length = int(loop * RATE)
    fade_len = int(crossfade * RATE)
    if length + fade_len + tune > len(samples):
        sys.exit(f"window too short: {len(samples) / RATE:.2f} s for a {loop} s loop")
    if tune:
        length = max(
            range(length - tune, length + tune + 1),
            key=lambda lag: correlation(samples, lag, fade_len),
        )
    out = array("f", samples[:length])
    for i in range(fade_len):
        w = i / max(fade_len - 1, 1)
        if fade == "linear":
            head, tail = w, 1.0 - w
        else:
            head, tail = math.sqrt(w), math.sqrt(1.0 - w)
        out[i] = samples[i] * head + samples[length + i] * tail
    return out


def normalise(samples: array) -> tuple[array, float]:
    rms = math.sqrt(sum(s * s for s in samples) / len(samples))
    gain = LOOP_RMS / rms if rms > 1e-9 else 0.0
    return array("f", (s * gain for s in samples)), gain


def rms(samples: array) -> float:
    return math.sqrt(sum(s * s for s in samples) / len(samples))


def limit(samples: array) -> array:
    """Runs a limiter over three repetitions and keeps the middle one."""
    repeated = samples + samples + samples
    length = len(samples) / RATE
    raw = run(
        [
            "ffmpeg",
            "-v",
            "error",
            "-f",
            "f32le",
            "-ar",
            str(RATE),
            "-ac",
            "1",
            "-i",
            "-",
            "-af",
            f"alimiter=limit={PEAK}:attack=5:release=50:level=0,atrim=start={length:.6f}:end={2 * length:.6f}",
            "-f",
            "f32le",
            "-",
        ],
        repeated.tobytes(),
    )
    limited = array("f")
    limited.frombytes(raw)
    return limited


def limit_and_encode(samples: array, target: Path) -> float:
    """Holds the peaks under full scale and encodes; returns the RMS kept.

    A peaky recording loses level to the limiter, so gain and limiter are
    applied again until the loop sits at [`LOOP_RMS`] — three rounds squash a
    pass-by's crest by a decibel or two, which is what a real mixer would do
    to it as well.
    """
    limited = samples
    for _ in range(3):
        level = rms(limited)
        if level >= 0.97 * LOOP_RMS:
            break
        limited = limit(array("f", (s * LOOP_RMS / level for s in limited)))
    run(
        [
            "ffmpeg",
            "-v",
            "error",
            "-y",
            "-f",
            "f32le",
            "-ar",
            str(RATE),
            "-ac",
            "1",
            "-i",
            "-",
            "-c:a",
            "libvorbis",
            "-q:a",
            "4",
            str(target),
        ],
        limited.tobytes(),
    )
    return math.sqrt(sum(s * s for s in limited) / len(limited))


def spectrogram(path: Path, target: Path) -> None:
    run(
        [
            "ffmpeg",
            "-v",
            "error",
            "-y",
            "-stream_loop",
            "2",
            "-i",
            str(path),
            "-lavfi",
            "showspectrumpic=s=1800x500:mode=combined:color=fire:scale=log:fscale=lin:start=0:stop=6000:legend=1",
            str(target),
        ]
    )


def build(sources: Path, check: Path | None) -> None:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    if check:
        check.mkdir(parents=True, exist_ok=True)
    for cut in CUTS:
        source = SOURCES[cut.source]
        path = sources / source.file
        if not path.exists():
            sys.exit(f"missing {path}: run `fetch` first")
        window = decode(path, cut.start, cut.length, cut.filters)
        if cut.tonal:
            # Before the resampling: the lines are found at the tempo they
            # were recorded at, and are then moved together.
            window = tonal(window)
        if cut.rate != 1.0:
            window = resample(window, cut.rate)
        looped = seam(window, cut.loop, cut.crossfade, cut.fade, cut.tune)
        levelled, gain = normalise(looped)
        target = OUT_DIR / f"{cut.name}.ogg"
        rms = limit_and_encode(levelled, target)
        print(
            f"{cut.name:14} {cut.loop:5.2f} s from {source.file} @ {cut.start:.2f} s"
            f"  gain {20 * math.log10(gain):+5.1f} dB  rms {rms:.3f}"
            + ("" if rms > 0.9 * LOOP_RMS else f"  (factor ×{LOOP_RMS / rms:.2f} to match)")
        )
        if check:
            spectrogram(target, check / f"{cut.name}.png")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("command", choices=["fetch", "build"])
    parser.add_argument("--sources", type=Path, default=Path("/tmp/br101-sources"))
    parser.add_argument("--check", type=Path, help="directory for spectrograms of the result")
    args = parser.parse_args()
    if args.command == "fetch":
        fetch(args.sources)
    else:
        build(args.sources, args.check)


if __name__ == "__main__":
    main()
