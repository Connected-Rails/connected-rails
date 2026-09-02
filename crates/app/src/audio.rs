//! Sound (plan ch. 13) — the vehicle's sound table put to work on kira's mixer.
//!
//! Nothing here decides *what* a train sounds like. That is written in the vehicle file
//! ([`sim_core::sound`]): which sample follows which quantity, under which conditions,
//! started by which trigger. This module only does what needs an audio device — it turns
//! the entries into playing sounds, follows their curves every frame and detects the edges
//! that a trigger fires on.
//!
//! Two kinds of entry come out of the same table:
//!
//! - **without a trigger** — a loop whose volume and pitch are modulated (rolling noise,
//!   traction, air).
//! - **with a trigger** — a one-shot, played in the frame the edge is crossed (rail joints,
//!   tap changer contactors).
//!
//! A vehicle without a table of its own runs on [`sim_core::sound::default_table`], and its
//! samples are **generated** ([`sim_core::synth`]). So the repository carries no binary
//! samples, and a mod that brings its own files takes exactly the same path — only the
//! `file` of the entry changes.
//!
//! ## Why kira rather than Bevy's own audio
//!
//! Bevy's audio is a set of sinks on one bus: no filter graph, no sends, no effects. That
//! was enough for "a loop whose volume follows the speed" and nothing beyond it. The mixer
//! here is what the rest costs:
//!
//! ```text
//!   main ── compressor (limiter)
//!    ├── cab                       desk sounds, no distance, no wall  ─┐
//!    └── emitter[train, vehicle]   spatial, one filter each           ─┤
//!                                                                      └→ reverb (send)
//! ```
//!
//! - **One spatial track per vehicle**, so distance attenuation and stereo placement come
//!   out of its position and every sound of that vehicle shares one filter.
//! - **That filter is the cab wall and the air in one.** Its cutoff falls with distance
//!   (air absorbs treble long before bass) and drops to [`CAB_CUTOFF`] while the camera sits
//!   inside. Bevy's version was a hand-written one-pole in a decoder, switched by a global
//!   atomic, the same for every emitter.
//! - **Doppler** is computed here, from the sim's own velocities, and multiplied into the
//!   playback rate — no audio engine does it for you. It is why a wayside camera hears a
//!   train pass rather than approach and stop.
//! - **Reverb** is a send track whose level follows `TrackType::reverb` under the player:
//!   0 on the open line, 1 in a tunnel, in between for a station hall.
//! - **A compressor on the main track** catches the sum. A dozen entries at their own
//!   volumes have no shared head-room otherwise.

use crate::render::VehicleView;
use crate::{PlayerTrain, SimResource, settings, ui};
use bevy::prelude::*;
use kira::effect::compressor::CompressorBuilder;
use kira::effect::filter::{FilterBuilder, FilterHandle};
use kira::effect::reverb::ReverbBuilder;
use kira::listener::ListenerHandle;
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
use kira::track::{
    MainTrackBuilder, SendTrackBuilder, SendTrackHandle, SpatialTrackBuilder, SpatialTrackHandle,
    TrackBuilder, TrackHandle,
};
use kira::{AudioManager, AudioManagerSettings, Capacities, Decibels, DefaultBackend, Mix, Tween};
use sim_core::sound::{SoundSpec, SoundState};
use sim_core::train::VehicleSpec;
use sim_core::{sound, synth};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

/// Cutoff of the cab wall [Hz] while the camera sits inside.
///
/// ponytail: one figure for every cab; a per-vehicle insulation value moves into
/// `VehicleSpec` when someone records real cabs and can hear the difference.
const CAB_CUTOFF: f64 = 800.0;

/// Cutoff of an unobstructed emitter right next to the listener [Hz] — above hearing, so
/// the filter is out of the way until distance or the cab wall brings it down.
const OPEN_CUTOFF: f64 = 20_000.0;

/// Distance over which air absorption takes the cutoff down by a factor of e [m]. Not a
/// measured figure — the shape is right (treble goes first) and the number is what makes a
/// train two hundred metres away sound like one.
const ABSORPTION: f64 = 220.0;

/// Distances at which an emitter is at full volume and at which it is inaudible [m].
const DISTANCES: (f32, f32) = (4.0, 700.0);

/// Speed of sound [m/s] — the denominator of the Doppler shift.
const SOUND_SPEED: f32 = 343.0;

/// Bounds of the Doppler factor. A train cannot legitimately leave them; a rebase or a
/// teleporting scenario event can, and an unbounded factor turns that into a screech.
const DOPPLER: (f64, f64) = (0.8, 1.3);

/// How much of a track's signal is sent to the reverb when the surroundings ring fully.
const REVERB_SEND: Decibels = Decibels(-3.0);

/// The cab is inside the vehicle: it hears the same room, but far less of it.
const CAB_REVERB_SEND: Decibels = Decibels(-15.0);

/// Time constant of every volume and pitch move. A level that jumps clicks, and a playback
/// rate that jumps chirps.
const FADE: Tween = Tween {
    start_time: kira::StartTime::Immediate,
    duration: Duration::from_millis(80),
    easing: kira::Easing::Linear,
};

/// How the voice a retriggered one-shot replaces is faded out — short enough to stay a
/// retrigger, long enough not to be a click.
const STEAL: Tween = Tween {
    start_time: kira::StartTime::Immediate,
    duration: Duration::from_millis(15),
    easing: kira::Easing::Linear,
};

/// A running loop and where in the table it came from.
struct Loop {
    train: usize,
    vehicle: usize,
    entry: usize,
    handle: StaticSoundHandle,
}

/// The mixer track of one vehicle: everything placed on it is heard from its position,
/// through one filter.
struct Emitter {
    track: SpatialTrackHandle,
    filter: FilterHandle,
}

/// The mixer, the sources and everything currently playing.
#[derive(Resource)]
pub struct Audio {
    manager: AudioManager,
    listener: ListenerHandle,
    /// Wet return of the room. Its volume is the environment: silent on the open line.
    reverb: SendTrackHandle,
    /// Heard at the driver's desk — no distance, no cab wall.
    cab: TrackHandle,
    emitters: HashMap<(usize, usize), Emitter>,
    /// The sources behind the tables, by the `file` of the entry.
    sources: HashMap<String, StaticSoundData>,
    /// The table a vehicle without one of its own runs on.
    default: Vec<SoundSpec>,
    loops: Vec<Loop>,
    /// The state of the previous frame: triggers need an edge, air a difference.
    previous: HashMap<(usize, usize), SoundState>,
    /// The last one-shot per source. A key pressed twice in a row replaces its own click
    /// instead of stacking a second voice on it — ten presses were ten times the level.
    shots: HashMap<(usize, usize, String), StaticSoundHandle>,
}

impl Audio {
    /// Opens the audio device and builds the mixer. `None` means the machine has no output
    /// — a headless CI run, a container — and the simulator runs on without sound.
    fn new(master: f32) -> Option<Self> {
        let settings = AudioManagerSettings {
            capacities: Capacities {
                // One track per vehicle that makes a noise; a long consist of powered
                // units is the case the default of 128 would not survive.
                sub_track_capacity: 512,
                ..Capacities::default()
            },
            // The limiter, so a dozen entries at their own volumes have shared head-room.
            // Everything is mixed well below full scale, so this only works on peaks.
            main_track_builder: MainTrackBuilder::new()
                .volume(decibels(master))
                .with_effect(
                    CompressorBuilder::new()
                        .threshold(-12.0)
                        .ratio(6.0)
                        .attack_duration(Duration::from_millis(5))
                        .release_duration(Duration::from_millis(150)),
                ),
            ..AudioManagerSettings::default()
        };
        let mut manager = match AudioManager::<DefaultBackend>::new(settings) {
            Ok(manager) => manager,
            Err(error) => {
                warn!("sound: no audio device ({error}) — running silent");
                return None;
            }
        };
        // The listener is placed at the camera every frame; the identity pose only has to
        // be valid until then.
        let listener = manager
            .add_listener([0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0])
            .ok()?;
        // Fully wet: how much room there is comes out of the send track's own volume, so
        // one call switches the whole environment instead of one per emitter.
        let reverb = manager
            .add_send_track(
                SendTrackBuilder::new()
                    .volume(Decibels::SILENCE)
                    .with_effect(
                        ReverbBuilder::new()
                            .feedback(0.88)
                            .damping(0.35)
                            .stereo_width(1.0)
                            .mix(Mix(1.0)),
                    ),
            )
            .ok()?;
        let cab = manager
            .add_sub_track(TrackBuilder::new().with_send(&reverb, CAB_REVERB_SEND))
            .ok()?;
        Some(Self {
            manager,
            listener,
            reverb,
            cab,
            emitters: HashMap::new(),
            sources: HashMap::new(),
            default: sound::default_table(),
            loops: Vec::new(),
            previous: HashMap::new(),
            shots: HashMap::new(),
        })
    }

    /// The sound table of a vehicle.
    ///
    /// ponytail: a hauled vehicle without a table of its own stays silent instead of
    /// inheriting the default — that one carries a compressor and a horn, which a coach has
    /// no business making. A coach that is to roll audibly writes its own two entries.
    fn table<'a>(&'a self, spec: &'a VehicleSpec) -> &'a [SoundSpec] {
        if !spec.sounds.is_empty() {
            &spec.sounds
        } else if spec.powered() {
            &self.default
        } else {
            &[]
        }
    }

    /// Master volume, straight from the settings page.
    pub fn set_master(&mut self, master: f32) {
        self.manager
            .main_track()
            .set_volume(decibels(master), Tween::default());
    }

    /// Stops everything the run had playing. Dropping a track handle takes the track and
    /// the loops on it with it — the same tear-down `setup_audio` does before it builds
    /// the next run's, only this time nothing is built afterwards.
    pub fn silence(&mut self) {
        // Dropping a handle does not stop a sound — kira plays it to its end, and a loop
        // has none. Every voice has to be told, and the cab track is not tied to any
        // entity that a teardown could take with it.
        for running in &mut self.loops {
            running.handle.stop(STEAL);
        }
        for shot in self.shots.values_mut() {
            shot.stop(STEAL);
        }
        self.loops.clear();
        self.emitters.clear();
        self.previous.clear();
        self.shots.clear();
    }
}

/// Opens the output device and inserts the mixer.
///
/// Runs while the app is **built**, not in `Startup`: the initial state transition fires
/// `OnEnter(Driving)` before any startup schedule, so `setup_audio` would find no mixer and
/// the whole run would come out silent. Without a device the resource stays absent and
/// every system below is a no-op — a headless CI run is not a reason to fail.
///
/// Add it after `settings::plugin`, whose `Audio` resource carries the stored volume.
pub fn plugin(app: &mut App) {
    let master = app.world().resource::<settings::Audio>().master;
    if let Some(audio) = Audio::new(master) {
        app.insert_resource(audio);
    }
}

/// Loads the sources of the loaded vehicles, gives every one of them a mixer track and
/// starts its loops silently.
///
/// Runs on entering the drive: the trains and their view entities are created by `setup`,
/// and the commands of that system are only applied afterwards. Starting a second scenario
/// runs it again, so everything the previous one built is dropped first.
pub fn setup_audio(
    audio: Option<ResMut<Audio>>,
    sim: Res<SimResource>,
    player: Res<PlayerTrain>,
    views: Query<&VehicleView>,
) {
    let Some(mut audio) = audio else {
        return;
    };
    // Dropping a track handle removes the track and everything playing on it — that is the
    // whole tear-down of the previous run.
    audio.loops.clear();
    audio.emitters.clear();
    audio.previous.clear();
    audio.shots.clear();

    // Which files the loaded vehicles actually ask for — a mod's table may name any of them.
    let mut wanted: Vec<String> = Vec::new();
    for train in &sim.0.trains {
        for vehicle in &train.vehicles {
            for entry in audio.table(&vehicle.spec) {
                if !wanted.contains(&entry.file) && !audio.sources.contains_key(&entry.file) {
                    wanted.push(entry.file.clone());
                }
            }
        }
    }
    for file in wanted {
        // `synth:<name>` is generated, everything else is a sample out of a mod's directory,
        // read from where `mods://` is rooted.
        let source = match file.strip_prefix("synth:") {
            Some(name) => match synth::synth(name) {
                Some(samples) => looping(samples, synth::RATE),
                None => {
                    warn!("sound: unknown generated source {file}");
                    continue;
                }
            },
            None => match StaticSoundData::from_file(Path::new("mods").join(&file)) {
                Ok(data) => data,
                Err(error) => {
                    warn!("sound: cannot read mods/{file}: {error}");
                    continue;
                }
            },
        };
        audio.sources.insert(file, source);
    }

    // A mixer track for every vehicle that has a view entity and something to say. Placed
    // at the origin for now — `update_audio` moves it in the same frame.
    let voiced: Vec<(usize, usize)> = views
        .iter()
        .map(|view| (view.train, view.vehicle))
        .filter(|&(t, v)| {
            sim.0
                .trains
                .get(t)
                .and_then(|train| train.vehicles.get(v))
                .is_some_and(|vehicle| !audio.table(&vehicle.spec).is_empty())
        })
        .collect();
    for key in voiced {
        let Audio {
            manager,
            listener,
            reverb,
            ..
        } = &mut *audio;
        let mut builder = SpatialTrackBuilder::new()
            .distances(DISTANCES)
            .with_send(&*reverb, REVERB_SEND);
        let filter = builder.add_effect(FilterBuilder::new().cutoff(OPEN_CUTOFF));
        match manager.add_spatial_sub_track(&*listener, [0.0, 0.0, 0.0], builder) {
            Ok(track) => {
                audio.emitters.insert(key, Emitter { track, filter });
            }
            Err(error) => warn!("sound: no mixer track for vehicle {key:?}: {error}"),
        }
    }

    let (mut started, mut triggered) = (0, 0);
    for (t, train) in sim.0.trains.iter().enumerate() {
        for (v, vehicle) in train.vehicles.iter().enumerate() {
            for i in 0..audio.table(&vehicle.spec).len() {
                let entry = &audio.table(&vehicle.spec)[i];
                if !entry.is_loop() {
                    triggered += 1;
                    continue;
                }
                let (file, positional) = (entry.file.clone(), entry.positional);
                let Some(source) = audio.sources.get(&file).cloned() else {
                    continue;
                };
                // Starts silent; the curves bring it up in the first frame. A loop entry
                // repeats its sample for as long as it plays — the recorded ones out of a
                // mod have no loop region of their own, and the generated ones keep theirs.
                let source = source.volume(Decibels::SILENCE).loop_region(0.0..);
                let handle = match (positional, audio.emitters.get_mut(&(t, v))) {
                    // Placed in the world: the track rides along on the vehicle, so
                    // distance, stereo placement and the wall are its business.
                    (true, Some(emitter)) => emitter.track.play(source),
                    // Not placed: a cab sound. Only the train being driven has a cab that
                    // anyone is sitting in.
                    (false, _) if t == player.0 => audio.cab.play(source),
                    _ => continue,
                };
                match handle {
                    Ok(handle) => {
                        started += 1;
                        audio.loops.push(Loop {
                            train: t,
                            vehicle: v,
                            entry: i,
                            handle,
                        });
                    }
                    Err(error) => warn!("sound: cannot start {file}: {error}"),
                }
            }
        }
    }
    info!(
        "Sound: {started} loops and {triggered} triggered entries from {} sources",
        audio.sources.len()
    );
}

/// Follows the curves of every loop, moves the mixer with the train and fires the triggered
/// entries.
// A Bevy system takes its resources as parameters — the argument count says nothing here.
#[allow(clippy::too_many_arguments)]
pub fn update_audio(
    audio: Option<ResMut<Audio>>,
    sim: Res<SimResource>,
    player: Res<PlayerTrain>,
    time: Res<Time>,
    camera_state: Res<ui::CameraState>,
    walker: Res<crate::walk::Walker>,
    camera: Query<&GlobalTransform, With<ui::CabCamera>>,
    views: Query<(&VehicleView, &GlobalTransform)>,
) {
    let Some(mut audio) = audio else {
        return;
    };
    let dt = time.delta_secs_f64().clamp(1e-3, 0.25);
    let sim = &sim.0;

    // The listener sits at the camera. In the cab and on the outside view it rides with the
    // train, so only a wayside camera hears a pass-by — which is exactly the Doppler case.
    let Ok(&view) = camera.single() else {
        return;
    };
    let (_, rotation, translation) = view.to_scale_rotation_translation();
    audio.listener.set_position(translation.to_array(), FADE);
    audio.listener.set_orientation(
        [rotation.x, rotation.y, rotation.z, rotation.w],
        Tween::default(),
    );
    let in_cab = inside_a_vehicle(camera_state.mode, walker.place);
    // The cab track carries what is heard *at the desk* and has no place in the world, so
    // distance cannot quieten it: it is faded out by hand when the listener leaves.
    audio.cab.set_volume(
        if in_cab {
            Decibels::IDENTITY
        } else {
            Decibels::SILENCE
        },
        FADE,
    );
    let listener_velocity = match camera_state.mode {
        // The wayside camera and the free one stand still in the world: a train running
        // past either of them is a pass-by and has to sound like one, which it cannot if
        // the listener is credited with the train's own speed.
        ui::CameraMode::Wayside | ui::CameraMode::Fly => Vec3::ZERO,
        // On foot on the ground the listener stands still like a wayside camera does: the
        // train running past him is a pass-by and has to sound like one, which it cannot
        // if he is credited with its own speed.
        ui::CameraMode::Walk if !in_cab => Vec3::ZERO,
        // Cab and orbit both ride on the player train, so it shifts nothing against itself.
        _ => train_velocity(sim, player.0, &views),
    };

    // One reading per vehicle — every entry of that vehicle is evaluated against it.
    let mut states: HashMap<(usize, usize), SoundState> = HashMap::new();
    for (t, train) in sim.trains.iter().enumerate() {
        let cab = sim.controls[t];
        let protection = &sim.runtime[t].protection;
        for (v, vehicle) in train.vehicles.iter().enumerate() {
            if audio.table(&vehicle.spec).is_empty() {
                continue;
            }
            let mut state =
                SoundState::sample(vehicle, &cab, protection, audio.previous.get(&(t, v)), dt);
            // The sampler deliberately sees no track and no weather — both are filled in
            // here, where net and world state live.
            // Track without a type is track nobody has laid a superstructure
            // on — it cannot happen on a compiled line, and what rolls over it
            // rolls as smoothly as welded main line.
            state.roughness = sim
                .net
                .track_type_at(vehicle.pos.edge, vehicle.pos.s)
                .map_or(1.0, |ty| ty.roughness);
            // The rain quantity is how hard it falls, not whether it does: a
            // drizzle is not a downpour with the volume turned down.
            let weather = sim.weather.now;
            state.rain = if weather.precip.is_liquid() {
                f64::from((weather.rate / 6.0).min(1.0))
            } else {
                0.0
            };
            // The clap arrives `distance / 343 m/s` after the flash, and rolls
            // for longer the further away it struck.
            state.thunder = sim
                .weather
                .lightning(sim.time)
                .map_or(0.0, |strike| f64::from(strike.thunder(sim.time, dt)));
            states.insert((t, v), state);
        }
    }

    // Where every vehicle is, how fast, and hence its Doppler factor and how muffled it is.
    let mut doppler: HashMap<(usize, usize), f64> = HashMap::new();
    for (view, transform) in views.iter() {
        let key = (view.train, view.vehicle);
        let Some(vehicle) = sim
            .trains
            .get(key.0)
            .and_then(|train| train.vehicles.get(key.1))
        else {
            continue;
        };
        let position = transform.translation();
        // The vehicle's own axis: `look_rotation` puts the track tangent on -Z, so this is
        // the direction it is travelling in at a positive `v`.
        let velocity = transform.forward() * vehicle.v as f32;
        let distance = position.distance(translation);
        doppler.insert(
            key,
            doppler_factor(position - translation, velocity - listener_velocity),
        );
        if let Some(emitter) = audio.emitters.get_mut(&key) {
            emitter.track.set_position(position.to_array(), FADE);
            emitter.filter.set_cutoff(cutoff(distance, in_cab), FADE);
        }
    }

    // The room: what the track under the player says, so a tunnel wall arrives with the
    // train rather than with the camera.
    let ringing = sim
        .trains
        .get(player.0)
        .and_then(|train| train.vehicles.first())
        .and_then(|vehicle| {
            sim.net
                .track_type_at(vehicle.pos.edge, vehicle.pos.s)
                .map(|ty| ty.reverb)
        })
        .unwrap_or(0.0);
    audio
        .reverb
        .set_volume(decibels(ringing.clamp(0.0, 1.0) as f32), FADE);

    // Loops: volume and pitch, tweened rather than set hard.
    for index in 0..audio.loops.len() {
        let Loop {
            train,
            vehicle,
            entry,
            ..
        } = audio.loops[index];
        let level = states.get(&(train, vehicle)).and_then(|state| {
            let spec = sim.trains.get(train)?.vehicles.get(vehicle)?;
            Some(audio.table(&spec.spec).get(entry)?.level(state))
        });
        let Some((volume, pitch)) = level else {
            continue;
        };
        let shift = doppler.get(&(train, vehicle)).copied().unwrap_or(1.0);
        let handle = &mut audio.loops[index].handle;
        handle.set_volume(decibels(volume as f32), FADE);
        handle.set_playback_rate(pitch * shift, FADE);
    }

    // Triggered entries: one-shots, played in the frame the edge is crossed.
    for (&(t, v), state) in states.iter() {
        // The first frame has no edge — everything would fire at once.
        let Some(before) = audio.previous.get(&(t, v)).copied() else {
            continue;
        };
        let shift = doppler.get(&(t, v)).copied().unwrap_or(1.0);
        let fired: Vec<(String, bool, f64, f64)> = audio
            .table(&sim.trains[t].vehicles[v].spec)
            .iter()
            .filter(|entry| !entry.is_loop() && entry.fires(state, &before))
            .map(|entry| {
                let (volume, pitch) = entry.level(state);
                (entry.file.clone(), entry.positional, volume, pitch)
            })
            .collect();
        for (file, positional, volume, pitch) in fired {
            let Some(source) = audio.sources.get(&file).cloned() else {
                continue;
            };
            let source = source
                .volume(decibels(volume as f32))
                .playback_rate(pitch * shift)
                // A one-shot is one shot: the generated sources carry a loop region, which
                // a joint or a click must not keep.
                .loop_region(Option::<kira::sound::Region>::None);
            let played = match (positional, audio.emitters.get_mut(&(t, v))) {
                (true, Some(emitter)) => emitter.track.play(source),
                _ => audio.cab.play(source),
            };
            match played {
                // The previous voice of the same source goes out over a few milliseconds —
                // stopping it outright would put a click of its own into the sample.
                Ok(handle) => {
                    if let Some(mut old) = audio.shots.insert((t, v, file), handle) {
                        old.stop(STEAL);
                    }
                }
                Err(error) => warn!("sound: cannot play {file}: {error}"),
            }
        }
    }

    audio.previous = states;
}

/// Is the listener inside a vehicle?
///
/// Not the same question as "is the camera the cab camera": the driver who has got up and
/// walked out of the door is standing on the ballast with the cab behind him, and the walk
/// is the same camera either way. Two things hang on the answer — the wall the spatial
/// emitters are heard through, and whether the cab track, which has no place in the world
/// and so cannot be quietened by distance, is heard at all.
fn inside_a_vehicle(mode: ui::CameraMode, place: Option<crate::walk::Place>) -> bool {
    match mode {
        ui::CameraMode::Cab => true,
        // `None` is the seat itself: he never got up.
        ui::CameraMode::Walk => !matches!(place, Some(crate::walk::Place::Outside { .. })),
        ui::CameraMode::Outside | ui::CameraMode::Wayside | ui::CameraMode::Fly => false,
    }
}

/// A source that repeats for as long as it plays.
fn looping(samples: Vec<f32>, rate: u32) -> StaticSoundData {
    StaticSoundData {
        sample_rate: rate,
        frames: samples.into_iter().map(kira::Frame::from_mono).collect(),
        settings: kira::sound::static_sound::StaticSoundSettings::default(),
        slice: None,
    }
    .loop_region(0.0..)
}

/// Volume in decibels for a linear 0 … 1 factor. 0 is silence, not −∞ dB — kira treats
/// anything at or below [`Decibels::SILENCE`] as exactly zero amplitude.
fn decibels(linear: f32) -> Decibels {
    if linear <= 1e-3 {
        Decibels::SILENCE
    } else {
        Decibels(20.0 * linear.log10())
    }
}

/// Cutoff of an emitter's filter [Hz]: air takes the treble out with distance, the cab wall
/// takes what is left of it.
///
/// Both are one figure per emitter rather than a ray cast against the world. Occlusion by
/// terrain and buildings is the next step and needs the collision geometry the app does not
/// have yet.
fn cutoff(distance: f32, in_cab: bool) -> f64 {
    let open = OPEN_CUTOFF * (-f64::from(distance.max(0.0)) / ABSORPTION).exp();
    // The wall of the cab the listener is sitting in muffles everything outside it — its
    // own vehicle no more and no less than the train on the next track.
    let wall = if in_cab { CAB_CUTOFF } else { f64::INFINITY };
    // Below about 60 Hz there is nothing left to hear; a cutoff walking towards zero only
    // costs precision in the filter.
    open.min(wall).max(60.0)
}

/// Doppler factor for an emitter `offset` metres from the listener, closing at `velocity`.
///
/// `f' = f · c / (c + v_r)` with `v_r` the rate at which the two are separating: away from
/// the listener lowers the pitch, towards it raises it. The listener's own velocity is
/// already subtracted from `velocity` by the caller, which is why a cab camera hears no
/// shift from its own train and a wayside camera hears the pass-by.
fn doppler_factor(offset: Vec3, velocity: Vec3) -> f64 {
    let Some(direction) = offset.try_normalize() else {
        // Sitting exactly on the emitter — no line of sight to project onto.
        return 1.0;
    };
    // Clamp the closing rate, not just the result: past the speed of sound the denominator
    // changes sign and the formula comes back inside out — a huge approach speed would read
    // as a huge *fall* in pitch. No train gets there, a teleporting scenario event does.
    let separating = velocity
        .dot(direction)
        .clamp(-0.5 * SOUND_SPEED, 2.0 * SOUND_SPEED);
    let factor = f64::from(SOUND_SPEED / (SOUND_SPEED + separating));
    factor.clamp(DOPPLER.0, DOPPLER.1)
}

/// Velocity of a train [m/s in render space], read off its first vehicle.
///
/// The whole consist moves at one speed, so any vehicle would do; the direction has to come
/// out of a transform because the sim only knows the signed speed along the track.
fn train_velocity(
    sim: &sim_core::Sim,
    train: usize,
    views: &Query<(&VehicleView, &GlobalTransform)>,
) -> Vec3 {
    views
        .iter()
        .find(|(view, _)| view.train == train)
        .and_then(|(view, transform)| {
            let vehicle = sim.trains.get(train)?.vehicles.get(view.vehicle)?;
            Some(transform.forward() * vehicle.v as f32)
        })
        .unwrap_or(Vec3::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walk::Place;

    /// A linear factor of 1 must not change the volume, and 0 must be silent rather than
    /// merely quiet — a muted loop that still leaks is audible under a dozen others.
    #[test]
    fn the_volume_curve_maps_onto_decibels() {
        assert_eq!(decibels(1.0), Decibels::IDENTITY);
        assert_eq!(decibels(0.0).as_amplitude(), 0.0);
        assert_eq!(decibels(-1.0).as_amplitude(), 0.0);
        // Half the amplitude is about −6 dB, and the round trip lands back where it was.
        assert!((decibels(0.5).0 + 6.0).abs() < 0.05);
        assert!((decibels(0.5).as_amplitude() - 0.5).abs() < 1e-3);
    }

    /// The filter: open next to the emitter, closing with distance, and shut down to the
    /// cab wall the moment the camera moves inside.
    #[test]
    fn the_cutoff_falls_with_distance_and_with_the_cab_wall() {
        assert!(cutoff(0.0, false) > 19_000.0);
        assert!(cutoff(200.0, false) < cutoff(50.0, false));
        assert!(cutoff(2_000.0, false) >= 60.0, "never walks to zero");
        // In the cab the wall rules until distance takes it below the wall by itself.
        assert_eq!(cutoff(0.0, true), CAB_CUTOFF);
        assert!(cutoff(1_000.0, true) < CAB_CUTOFF);
    }

    /// The sign of the shift is the one thing worth a test: approaching raises the pitch,
    /// receding lowers it, and passing broadside does neither.
    #[test]
    fn doppler_rises_on_approach_and_falls_on_departure() {
        let ahead = Vec3::new(0.0, 0.0, -100.0);
        // Coming at the listener: the emitter is ahead and moving back towards it.
        assert!(doppler_factor(ahead, Vec3::new(0.0, 0.0, 30.0)) > 1.0);
        // Going away.
        assert!(doppler_factor(ahead, Vec3::new(0.0, 0.0, -30.0)) < 1.0);
        // Straight across the line of sight — no radial component, no shift.
        assert_eq!(doppler_factor(ahead, Vec3::new(30.0, 0.0, 0.0)), 1.0);
        // A standing emitter and a standing listener are the common case.
        assert_eq!(doppler_factor(ahead, Vec3::ZERO), 1.0);
        // And nothing a runaway state can produce leaves the range.
        assert_eq!(
            doppler_factor(ahead, Vec3::new(0.0, 0.0, 100_000.0)),
            DOPPLER.1
        );
        assert_eq!(
            doppler_factor(ahead, Vec3::new(0.0, 0.0, -100_000.0)),
            DOPPLER.0
        );
        // Degenerate: listener sitting on the emitter.
        assert_eq!(doppler_factor(Vec3::ZERO, Vec3::new(0.0, 0.0, 30.0)), 1.0);
    }

    /// The generated sources have to survive the trip into a kira buffer — a rate or a
    /// frame count that came out wrong would play at the wrong pitch or not at all.
    #[test]
    fn a_generated_source_becomes_a_looping_buffer() {
        let samples = synth::synth("rolling-mid").expect("generated");
        let data = looping(samples.clone(), synth::RATE);
        assert_eq!(data.sample_rate, synth::RATE);
        assert_eq!(data.frames.len(), samples.len());
        assert!(data.settings.loop_region.is_some(), "loops");
        // Mono into both channels, or the source would only come out of one ear.
        assert_eq!(data.frames[0].left, data.frames[0].right);
    }

    /// Whether a sound repeats comes from the entry, not the source: a recording has no
    /// loop region of its own and has to be looped when its entry is a loop, and a
    /// generated click keeps a loop region that a one-shot has to drop.
    #[test]
    fn loops_repeat_and_one_shots_end() {
        use kira::backend::mock::MockBackendSettings;
        use kira::sound::PlaybackState;
        let mut manager =
            AudioManager::<kira::backend::mock::MockBackend>::new(AudioManagerSettings {
                backend_settings: MockBackendSettings {
                    sample_rate: synth::RATE,
                },
                ..AudioManagerSettings::default()
            })
            .expect("manager");
        // A tenth of a second of tone; one backend pass is 128 frames of it.
        let data = looping(vec![0.25; (synth::RATE / 10) as usize], synth::RATE);

        let mut loop_handle = manager.play(data.clone()).expect("play");
        for _ in 0..64 {
            // The mock backend runs the renderer by hand, like the cpal backend does.
            manager.backend_mut().on_start_processing();
            manager.backend_mut().process();
        }
        assert_eq!(loop_handle.state(), PlaybackState::Playing);

        let shot = manager
            .play(data.loop_region(Option::<kira::sound::Region>::None))
            .expect("play");
        for _ in 0..64 {
            manager.backend_mut().on_start_processing();
            manager.backend_mut().process();
        }
        assert_eq!(shot.state(), PlaybackState::Stopped);
        let _ = &mut loop_handle;
    }

    /// What the driver hears follows where the driver *is*, not which camera is drawing.
    /// Getting this wrong is what made the cab sounds follow a player who had walked out
    /// onto the platform: the cab track has no position in the world, so nothing else
    /// quietens it.
    #[test]
    fn the_cab_is_only_heard_from_inside_the_vehicle() {
        let aboard = Some(Place::Aboard {
            vehicle: 0,
            eye: Vec3::ZERO,
        });
        let outside = Some(Place::Outside {
            eye: world_coords::EcefPos::default(),
        });

        // At the desk, and on the way down the aisle.
        assert!(inside_a_vehicle(ui::CameraMode::Cab, None));
        assert!(inside_a_vehicle(ui::CameraMode::Walk, None));
        assert!(inside_a_vehicle(ui::CameraMode::Walk, aboard));
        // Out of the door, and on the cameras that never were inside.
        assert!(!inside_a_vehicle(ui::CameraMode::Walk, outside));
        assert!(!inside_a_vehicle(ui::CameraMode::Outside, aboard));
        assert!(!inside_a_vehicle(ui::CameraMode::Wayside, None));
    }
}
