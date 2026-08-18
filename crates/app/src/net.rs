//! Multiplayer over dedicated servers (plan ch. 20), on
//! [lightyear](https://github.com/cBournhonesque/lightyear).
//!
//! Without `--connect` and `--dedicated` nothing in here runs: the simulator is exactly
//! the single player it was, and the run does not even open a socket.
//!
//! What goes over the wire is deliberately not the world:
//!
//! * **Positions on the track, never transforms.** A train moves in one dimension, so its
//!   state is `(edge, s, dir, v, a)` — about 17 bytes. The client rebuilds the full pose,
//!   cant included, from the spline itself. A replicated quaternion would not only cost
//!   more, it would shake the vehicle sideways off the rail, which the eye reads as broken
//!   because it is physically impossible.
//! * **Setpoints, not results.** The driver's levers ([`CabInputs`]) travel as events on a
//!   reliable channel and every peer runs the same deterministic physics on them. Positions
//!   only follow as an occasional correction.
//! * **Corrections are speed, not teleportation.** An error is worked off by running a
//!   fraction of a percent fast or slow for a moment ([`Train::nudge`]); nothing is ever set
//!   to the server's position while it is anywhere near. That is where rubber banding comes
//!   from, and this is how it is avoided.
//! * **One train, one packet.** The vehicles behind the leader follow from the couplers
//!   along the spline, so a thirty-wagon freight costs the same as a single railcar and
//!   cannot drift apart on packet loss.
//! * **Interest management.** A line runs for hundreds of kilometres; only trains within a
//!   few of them are corrected at the full rate.

use crate::menu::Selection;
use crate::{AiDrivers, GameState, PlayerTrain, SimResource};
use bevy::app::ScheduleRunnerPlugin;
use bevy::log::LogPlugin;
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use core::net::{IpAddr, Ipv4Addr, SocketAddr};
use core::time::Duration;
use lightyear::netcode::Key;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::*;
use mod_runtime::ModRuntime;
use serde::{Deserialize, Serialize};
use sim_core::Sim;
use sim_core::cab::CabInputs;
use track_model::{EdgeId, TrackPosition};
use world_coords::EcefPos;

/// Port the dedicated server listens on when the address names none.
pub const DEFAULT_PORT: u16 = 27_015;

/// Netcode protocol id. Bump it whenever the wire format changes — an old client is then
/// turned away at the door instead of desynchronising ten minutes into the run.
const PROTOCOL_ID: u64 = 0x7261_696c_0001;

/// How often the server sends corrections [s].
const SYNC_INTERVAL: f64 = 0.1;
/// Trains this close to a client are corrected at the full rate [m].
const NEAR_RADIUS: f64 = 3_000.0;
/// … and out to here at a tenth of it. Beyond, nothing is sent at all: the client keeps
/// simulating them off the last setpoints it heard, which for a train is enough.
const FAR_RADIUS: f64 = 20_000.0;
/// One in this many syncs serves the far ring.
const FAR_DIVISOR: u32 = 10;

/// Time constant the longitudinal error is worked off with [s].
const CORRECTION_TAU: f64 = 1.5;
/// A correction may not run faster than this share of the train's own speed …
const CORRECTION_FRACTION: f64 = 0.02;
/// … and never slower than this [m/s], so a standing train still creeps into place.
const CORRECTION_FLOOR: f64 = 0.2;
/// The furthest an error is smoothed away [m]. Past it the train is somewhere else
/// entirely — a client that just joined, a switch taken the other way, a stall long enough
/// to lose it — and there is nothing to smooth, so it gets placed.
const RESYNC_LIMIT: f64 = 50.0;
/// A received state is never extrapolated further than this [s]. Half a second is worth
/// centimetres on a train, which is why latency can be made invisible here at all.
const MAX_EXTRAPOLATION: f64 = 0.5;
/// Difference to the server's simulation clock that is taken over rather than lived with [s].
const CLOCK_LIMIT: f64 = 0.25;
/// Time constant a speed difference to the server is taken over with [s]. Without it a
/// train that once ran a fraction slow keeps its distance error forever, because the nudge
/// below can only ever hold it, not close it.
const SPEED_TAU: f64 = 1.0;
/// … applied at no more than this [m/s²] — a tenth of what a brake application feels like,
/// and the same for every vehicle, so no coupler notices.
const SPEED_RATE: f64 = 0.3;

// ---------------------------------------------------------------------------- protocol

/// Track-relative state of one train — everything a correction is made of.
#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct TrainSync {
    pub train: u16,
    /// Edge of the leading vehicle's centre.
    pub edge: u32,
    /// Arc length on that edge [m].
    pub s: f32,
    pub dir: i8,
    /// Speed [m/s] and acceleration [m/s²] to extrapolate the transit time with.
    pub v: f32,
    pub a: f32,
    /// First state of this train the client is being sent — on joining, or after the train
    /// came back into range. It is not late, it is from a world the client never saw, so it
    /// gets placed rather than smoothed towards.
    pub fresh: bool,
}

/// All the corrections of one sync tick in one packet, with the simulation time they were
/// taken at — a client that joined late, or stalled, has to know how far behind it is
/// before any of the positions make sense.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SyncBatch {
    pub time: f64,
    pub trains: Vec<TrainSync>,
}

/// Client → server on connecting: the train the scenario put this player in, and the
/// fingerprint of the world it built.
#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct Join {
    pub train: u16,
    pub world: u64,
}

/// Server → client: the train it may drive. The wish from [`Join`] is granted while the
/// train is still free, so a single player joining his own scenario keeps his cab.
#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct Welcome {
    pub train: u16,
    pub world: u64,
}

/// Server → client: where one train's levers stand. Sent when they move, not on a rate.
#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct Setpoints {
    pub train: u16,
    pub cab: CabInputs,
}

/// Client → server: where the player's own levers stand.
#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct DriverInput(pub CabInputs);

/// Ordered and reliable: everything that is an event and must not be lost — a lever
/// movement, a joining client.
struct Control;

/// Sequenced and unreliable: the position corrections. A late one is worthless, because
/// the next one is already better, so an old packet is dropped rather than applied.
struct Corrections;

/// The types that go over the wire. Has to be registered after the lightyear plugins and
/// before any client or server entity is spawned.
fn protocol(app: &mut App) {
    app.register_message::<Join>()
        .add_direction(NetworkDirection::ClientToServer);
    app.register_message::<DriverInput>()
        .add_direction(NetworkDirection::ClientToServer);
    app.register_message::<Welcome>()
        .add_direction(NetworkDirection::ServerToClient);
    app.register_message::<Setpoints>()
        .add_direction(NetworkDirection::ServerToClient);
    app.register_message::<SyncBatch>()
        .add_direction(NetworkDirection::ServerToClient);

    app.add_channel::<Control>(ChannelSettings {
        mode: ChannelMode::OrderedReliable(ReliableSettings::default()),
        ..default()
    })
    .add_direction(NetworkDirection::Bidirectional);
    app.add_channel::<Corrections>(ChannelSettings {
        mode: ChannelMode::SequencedUnreliable,
        ..default()
    })
    .add_direction(NetworkDirection::ServerToClient);
}

/// Which side of the connection this process is. Absent = single player.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Client,
    Server,
}

/// Fingerprint of the world both sides built ([`crate::world::fingerprint`]).
#[derive(Resource, Clone, Copy)]
pub struct WorldId(pub u64);

/// `host:port`, `host` (default port) or `:port` / `port` (listen on everything).
fn parse_addr(text: &str, listen: bool) -> Option<SocketAddr> {
    if let Ok(addr) = text.parse::<SocketAddr>() {
        return Some(addr);
    }
    if let Ok(port) = text.trim_start_matches(':').parse::<u16>() {
        let host = if listen {
            Ipv4Addr::UNSPECIFIED
        } else {
            Ipv4Addr::LOCALHOST
        };
        return Some(SocketAddr::new(IpAddr::V4(host), port));
    }
    let host: Ipv4Addr = text.parse().ok()?;
    Some(SocketAddr::new(IpAddr::V4(host), DEFAULT_PORT))
}

// ------------------------------------------------------------------------------ client

/// Address of the server this process talks to.
#[derive(Resource, Clone, Copy)]
struct ServerAddress(SocketAddr);

/// What one train still owes the server: a distance and a speed, both worked off gently
/// rather than applied.
#[derive(Clone, Copy, Default)]
struct Correction {
    /// Longitudinal error [m] — positive means the server has the train further ahead.
    distance: f64,
    /// Speed difference [m/s], same sign convention.
    speed: f64,
}

/// What the client keeps between frames.
#[derive(Resource, Default)]
pub struct Session {
    /// What each train still owes.
    error: Vec<Correction>,
    /// Levers last sent — only a movement goes on the wire.
    sent: Option<CabInputs>,
    /// The join request is out.
    asked: bool,
    /// The server has assigned a train.
    pub joined: bool,
    /// Round trip time as the link measures it [s] — read by the HUD.
    pub rtt: f64,
    /// Trains to place rather than smooth on their next state. Set when the clock jumped:
    /// the positions we hold are then not late, they are from another moment altogether.
    resync: Vec<bool>,
}

impl Session {
    /// Correction still pending for a train [m] — what the HUD shows to make the
    /// smoothing visible while it happens.
    pub fn correction(&self, train: usize) -> f64 {
        self.error.get(train).map_or(0.0, |c| c.distance)
    }
}

/// Turns the app into a multiplayer client when `--connect <host:port>` is given.
///
/// Called from `main` while the app is built, so nothing is added at all in single player.
pub fn plugin(app: &mut App) {
    let Some(addr) = crate::arg("--connect").and_then(|a| parse_addr(&a, false)) else {
        return;
    };
    app.add_plugins(client::ClientPlugins {
        tick_duration: Duration::from_secs_f64(SYNC_INTERVAL),
    });
    protocol(app);
    app.insert_resource(Role::Client)
        .insert_resource(ServerAddress(addr))
        .init_resource::<Session>()
        .add_systems(OnEnter(GameState::Driving), connect)
        .add_systems(
            Update,
            (
                client_receive.before(crate::step_simulation),
                client_send
                    .after(crate::ui::player_input)
                    .before(crate::step_simulation),
                client_correct.after(crate::step_simulation),
            )
                .run_if(in_state(GameState::Driving)),
        );
}

/// Opens the link. The world is already built at this point — the fingerprint in [`Join`]
/// is what tells the two sides whether they built the same one.
fn connect(
    mut commands: Commands,
    address: Res<ServerAddress>,
    existing: Query<Entity, With<Client>>,
) -> Result {
    if !existing.is_empty() {
        return Ok(());
    }
    let auth = Authentication::Manual {
        server_addr: address.0,
        // The client id only has to be unique per server; the process id is.
        client_id: u64::from(std::process::id()),
        private_key: Key::default(),
        protocol_id: PROTOCOL_ID,
    };
    let client = commands
        .spawn((
            Client,
            Name::new("Server link"),
            // Port 0: the operating system picks a free one.
            LocalAddr(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)),
            PeerAddr(address.0),
            Link::default(),
            client::NetcodeClient::new(auth, client::NetcodeConfig::default())?,
            // Ping and pong, and with them the round trip time the corrections are
            // extrapolated over. On the server side it comes with every connection.
            PingManager::default(),
            UdpIo::default(),
        ))
        .id();
    commands.trigger(Connect { entity: client });
    info!("connecting to {}", address.0);
    Ok(())
}

/// Asks for a train once, then sends the levers whenever they move.
fn client_send(
    sim: Res<SimResource>,
    player: Res<PlayerTrain>,
    world: Res<WorldId>,
    mut session: ResMut<Session>,
    mut join: Query<&mut MessageSender<Join>, (With<Client>, With<Connected>)>,
    mut input: Query<&mut MessageSender<DriverInput>, (With<Client>, With<Connected>)>,
) {
    // Not connected, or not any more: ask again when the link comes back.
    if join.is_empty() {
        session.asked = false;
        session.joined = false;
        session.sent = None;
        return;
    }
    if !session.asked {
        for mut tx in &mut join {
            tx.send::<Control>(Join {
                train: player.0 as u16,
                world: world.0,
            });
            session.asked = true;
        }
    }
    if !session.joined {
        return;
    }
    let Some(cab) = sim.0.controls.get(player.0) else {
        return;
    };
    if session.sent == Some(*cab) {
        return;
    }
    for mut tx in &mut input {
        tx.send::<Control>(DriverInput(*cab));
        session.sent = Some(*cab);
    }
}

/// Applies what the server said: the train we drive, everyone's levers, and the position
/// corrections — the last of which only ever become a pending error, never a jump.
// A Bevy system takes its resources as parameters — the argument count says nothing here.
#[allow(clippy::too_many_arguments)]
fn client_receive(
    mut sim: ResMut<SimResource>,
    mut session: ResMut<Session>,
    mut player: ResMut<PlayerTrain>,
    mut drivers: ResMut<AiDrivers>,
    world: Res<WorldId>,
    link: Query<&Link, With<Client>>,
    mut welcome: Query<&mut MessageReceiver<Welcome>, With<Client>>,
    mut setpoints: Query<&mut MessageReceiver<Setpoints>, With<Client>>,
    mut batches: Query<&mut MessageReceiver<SyncBatch>, With<Client>>,
) {
    for mut rx in &mut welcome {
        for greeting in rx.receive() {
            if greeting.world != world.0 {
                error!(
                    "the server runs a different world ({:016x} against our {:016x}) — \
                     start both sides with the same --line/--scenario",
                    greeting.world, world.0
                );
            }
            if greeting.train as usize != player.0 {
                warn!(
                    "the server put us in train {} instead of {}",
                    greeting.train, player.0
                );
            }
            player.0 = greeting.train as usize;
            // Every other train is driven by the server from here on; a second AI running
            // locally would fight the setpoints coming in.
            drivers.0.clear();
            session.joined = true;
            info!("joined as train {}", greeting.train);
        }
    }

    let sim = &mut sim.0;
    for mut rx in &mut setpoints {
        for update in rx.receive() {
            if let Some(cab) = sim.controls.get_mut(update.train as usize) {
                *cab = update.cab;
            }
        }
    }

    // One-way transit time: what the state we are being told has aged by on the way here.
    session.rtt = link
        .iter()
        .next()
        .map_or(0.0, |link| link.stats.rtt.as_secs_f64());
    let latency = (session.rtt / 2.0).min(MAX_EXTRAPOLATION);
    session
        .error
        .resize(sim.trains.len(), Correction::default());
    session.resync.resize(sim.trains.len(), false);
    for mut rx in &mut batches {
        for batch in rx.receive() {
            // The clock the timetable, the scenario and the signals run on. Milliseconds of
            // drift are left alone — the fixed step swallows them — but a client that joined
            // mid-run or lost a second to a stall takes the server's time over.
            let server_now = batch.time + latency;
            if (sim.time - server_now).abs() > CLOCK_LIMIT {
                info!(
                    "clock: {:+.1} s off, taking the server's",
                    server_now - sim.time
                );
                sim.time = server_now;
                session.resync.fill(true);
            }
            for state in batch.trains {
                let index = state.train as usize;
                let resync = state.fresh
                    || core::mem::replace(
                        session.resync.get_mut(index).unwrap_or(&mut false),
                        false,
                    );
                accept(sim, &mut session.error, state, latency, resync);
            }
        }
    }
}

/// Turns one received state into a pending correction.
fn accept(sim: &mut Sim, error: &mut [Correction], state: TrainSync, latency: f64, resync: bool) {
    let index = state.train as usize;
    let Sim { trains, net, .. } = sim;
    let (Some(train), Some(error)) = (trains.get_mut(index), error.get_mut(index)) else {
        return;
    };
    let Some(front) = train.vehicles.first() else {
        return;
    };
    // Where the server's train stands by now, and how fast it is going. A train never
    // changes direction abruptly and its acceleration only steps at a lever movement, so
    // half a second of this is good to centimetres.
    let reported = TrackPosition::new(EdgeId(state.edge), f64::from(state.s), state.dir);
    let ahead = f64::from(state.v) * latency + 0.5 * f64::from(state.a) * latency * latency;
    let speed = f64::from(state.v) + f64::from(state.a) * latency;
    let target = reported.offset_by(net, ahead).unwrap_or(reported);

    if let Some(distance) = (!resync)
        .then(|| front.pos.distance_to(net, &target, RESYNC_LIMIT))
        .flatten()
    {
        debug!(
            "train {index}: {distance:+.3} m off, v {:.2} against {:.2}",
            front.v, speed
        );
        error.distance = distance;
        error.speed = speed - front.v;
        return;
    }

    // Too far off to work off, or not even on our path any more: a client that has just
    // joined, a switch taken the other way, a stall long enough to lose the train. There is
    // nothing to smooth here, so the consist is placed — and given the speed it was found
    // at, because starting it again from the wrong one would only lose it a second time.
    let half = front.spec.length / 2.0;
    let head = target.offset_by(net, half).unwrap_or(target);
    warn!("train {index}: lost, placing it back where the server has it");
    train.place_head_at(head, net);
    for vehicle in &mut train.vehicles {
        vehicle.v = speed;
        vehicle.a = f64::from(state.a);
    }
    *error = Correction::default();
}

/// Works the pending correction off through the speed, which is the only way a train may
/// be corrected: a jump of 30 cm looks broken, two seconds at 0.3 % off is invisible.
///
/// Two terms, and both are needed. The speed difference is taken over first — every vehicle
/// by the same amount, so the couplers stay where they were — because a train that runs a
/// fraction slow builds its distance error again as fast as it is worked off. The distance
/// itself then follows as a moment of running slightly fast or slow.
fn client_correct(mut sim: ResMut<SimResource>, mut session: ResMut<Session>, time: Res<Time>) {
    let dt = time.delta_secs_f64().min(0.25);
    let Sim { trains, net, .. } = &mut sim.0;
    for (index, error) in session.error.iter_mut().enumerate() {
        let Some(train) = trains.get_mut(index) else {
            continue;
        };
        if error.speed.abs() > 1e-4 {
            let step = (error.speed * dt / SPEED_TAU).clamp(-SPEED_RATE * dt, SPEED_RATE * dt);
            for vehicle in &mut train.vehicles {
                vehicle.v += step;
            }
            error.speed -= step;
        }
        if error.distance.abs() > 1e-3 {
            let rate = (train.speed().abs() * CORRECTION_FRACTION).max(CORRECTION_FLOOR);
            let step = (error.distance * dt / CORRECTION_TAU).clamp(-rate * dt, rate * dt);
            train.nudge(net, step);
            error.distance -= step;
        } else {
            error.distance = 0.0;
        }
    }
}

// ------------------------------------------------------------------------------ server

/// What the dedicated server keeps between frames.
#[derive(Resource, Default)]
pub struct Host {
    /// The train each connected client drives.
    assigned: HashMap<Entity, usize>,
    /// Which trains a client has already been told about.
    known: HashMap<Entity, HashSet<usize>>,
    /// Levers as they were last broadcast — a resend only goes out on a change.
    last: Vec<CabInputs>,
    accumulator: f64,
    tick: u32,
}

impl Host {
    /// A train a client has taken over is no longer driven by the AI.
    pub fn is_player_driven(&self, train: usize) -> bool {
        self.assigned.values().any(|assigned| *assigned == train)
    }
}

/// Runs the dedicated server: the same simulation, without a window, a renderer or a
/// sound card. Never returns.
pub fn run_dedicated(address: &str) {
    let Some(address) = parse_addr(address, true) else {
        eprintln!("--dedicated: {address} is not an address");
        return;
    };
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
            1.0 / 60.0,
        ))),
        LogPlugin::default(),
    ));
    app.add_plugins(server::ServerPlugins {
        tick_duration: Duration::from_secs_f64(SYNC_INTERVAL),
    });
    protocol(&mut app);

    let mut mods = ModRuntime::load("mods");
    for warning in mods.log() {
        warn!("mod: {warning}");
    }
    let world = crate::world::build(&mut mods, &Selection::default());
    let id = crate::world::fingerprint(&world.line.name, &world.sim);
    info!(
        "world {id:016x}: {} trains on {}",
        world.sim.trains.len(),
        world.line.name
    );

    app.insert_resource(Role::Server)
        .insert_resource(WorldId(id))
        .insert_resource(PlayerTrain(world.player))
        .insert_resource(AiDrivers(world.drivers))
        .insert_resource(crate::Mods(mods))
        .insert_resource(SimResource(world.sim))
        .init_resource::<Host>()
        .add_systems(Startup, move |mut commands: Commands| {
            let server = commands
                .spawn((
                    server::NetcodeServer::new(server::NetcodeConfig {
                        protocol_id: PROTOCOL_ID,
                        ..default()
                    }),
                    Name::new("Dedicated server"),
                    LocalAddr(address),
                    server::ServerUdpIo::default(),
                ))
                .id();
            commands.trigger(server::Start { entity: server });
            info!("listening on {address}");
        })
        .add_systems(
            Update,
            (
                server_join,
                server_receive,
                crate::drive_ai,
                crate::step_simulation,
                crate::run_mod_scripts,
                server_broadcast,
            )
                .chain(),
        );
    app.run();
}

/// The connections to a server, with whatever of each one a system needs.
type Connections<'w, 's, D> = Query<'w, 's, (Entity, D), With<ClientOf>>;

/// Hands a train to every client that asked for one, and forgets the ones that left.
fn server_join(
    mut host: ResMut<Host>,
    sim: Res<SimResource>,
    world: Res<WorldId>,
    mut clients: Connections<(
        &'static mut MessageReceiver<Join>,
        &'static mut MessageSender<Welcome>,
    )>,
) {
    let mut live = HashSet::new();
    for (entity, (mut rx, mut tx)) in &mut clients {
        live.insert(entity);
        for request in rx.receive() {
            if request.world != world.0 {
                warn!(
                    "client {entity} built a different world ({:016x} against {:016x})",
                    request.world, world.0
                );
            }
            // The wish is granted while the train is free; otherwise the first one that is.
            let wanted = request.train as usize;
            let train = if wanted < sim.0.trains.len() && !host.is_player_driven(wanted) {
                wanted
            } else {
                (0..sim.0.trains.len())
                    .find(|t| !host.is_player_driven(*t))
                    .unwrap_or(0)
            };
            host.assigned.insert(entity, train);
            tx.send::<Control>(Welcome {
                train: train as u16,
                world: world.0,
            });
            info!("client {entity} drives train {train}");
        }
    }
    // A client that dropped gives its train back to the AI.
    host.assigned.retain(|entity, _| live.contains(entity));
    host.known.retain(|entity, _| live.contains(entity));
}

/// Takes the driver's levers of every client into the simulation.
fn server_receive(
    mut sim: ResMut<SimResource>,
    host: Res<Host>,
    mut clients: Connections<&'static mut MessageReceiver<DriverInput>>,
) {
    for (entity, mut rx) in &mut clients {
        let Some(&train) = host.assigned.get(&entity) else {
            continue;
        };
        for input in rx.receive() {
            if let Some(cab) = sim.0.controls.get_mut(train) {
                *cab = input.0;
            }
        }
    }
}

/// Sends the levers that moved and, at [`SYNC_INTERVAL`], the corrections — each client
/// only for the trains near enough to be worth the bandwidth.
fn server_broadcast(
    time: Res<Time>,
    sim: Res<SimResource>,
    mut host: ResMut<Host>,
    mut clients: Connections<(
        &'static mut MessageSender<Setpoints>,
        &'static mut MessageSender<SyncBatch>,
    )>,
) {
    host.accumulator += time.delta_secs_f64();
    if host.accumulator < SYNC_INTERVAL {
        return;
    }
    host.accumulator = 0.0;
    host.tick = host.tick.wrapping_add(1);

    let sim = &sim.0;
    host.last.resize(sim.trains.len(), CabInputs::default());

    // One sample per train: the leading vehicle, and where in the world it is — the
    // vehicles behind it follow from the couplers on every client by themselves.
    let states: Vec<Option<(TrainSync, EcefPos)>> = sim
        .trains
        .iter()
        .enumerate()
        .map(|(index, train)| {
            let front = train.vehicles.first()?;
            Some((
                TrainSync {
                    train: index as u16,
                    edge: front.pos.edge.0,
                    s: front.pos.s as f32,
                    dir: front.pos.dir,
                    v: front.v as f32,
                    a: front.a as f32,
                    fresh: false,
                },
                front.pos.pose(&sim.net).pos,
            ))
        })
        .collect();
    let moved: Vec<bool> = sim
        .controls
        .iter()
        .zip(&host.last)
        .map(|(now, before)| now != before)
        .collect();

    let Host {
        assigned,
        known,
        tick,
        ..
    } = &mut *host;
    for (entity, (mut setpoints, mut syncs)) in &mut clients {
        let own = assigned.get(&entity).copied().unwrap_or(0);
        let Some(Some((_, eye))) = states.get(own) else {
            continue;
        };
        let known = known.entry(entity).or_default();
        let mut batch = Vec::new();
        for (index, state) in states.iter().enumerate() {
            let Some((sync, position)) = state else {
                continue;
            };
            let distance = if index == own {
                0.0
            } else {
                eye.distance(*position)
            };
            let divisor = if distance <= NEAR_RADIUS {
                1
            } else if distance <= FAR_RADIUS {
                FAR_DIVISOR
            } else {
                known.remove(&index);
                continue;
            };
            // A train the client has not heard of yet gets its levers before its position,
            // so its physics runs on the right ones from the first correction — and it gets
            // that position straight away, whatever the rate of its ring says.
            let fresh = known.insert(index);
            if fresh || moved.get(index).copied().unwrap_or(false) {
                setpoints.send::<Control>(Setpoints {
                    train: index as u16,
                    cab: sim.controls[index],
                });
            }
            if fresh || *tick % divisor == 0 {
                batch.push(TrainSync { fresh, ..*sync });
            }
        }
        if !batch.is_empty() {
            syncs.send::<Corrections>(SyncBatch {
                time: sim.time,
                trains: batch,
            });
        }
    }
    host.last.copy_from_slice(&sim.controls);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_parse_in_both_roles() {
        assert_eq!(
            parse_addr("127.0.0.1:5000", false).unwrap().port(),
            5_000,
            "host:port"
        );
        assert!(parse_addr(":27015", true).unwrap().ip().is_unspecified());
        assert!(parse_addr("27015", false).unwrap().ip().is_loopback());
        assert_eq!(parse_addr("10.0.0.5", false).unwrap().port(), DEFAULT_PORT);
        assert!(parse_addr("nonsense", false).is_none());
    }

    /// A correction is worked off through the speed, and slowly enough to stay invisible.
    #[test]
    fn a_correction_never_arrives_in_one_step() {
        let mut error = 5.0f64;
        let dt = 1.0 / 60.0;
        let speed = 30.0;
        let rate = (speed * CORRECTION_FRACTION).max(CORRECTION_FLOOR);
        let mut steps = 0;
        while error.abs() > 1e-3 && steps < 10_000 {
            let step = (error * dt / CORRECTION_TAU).clamp(-rate * dt, rate * dt);
            assert!(
                step.abs() <= rate * dt,
                "faster than the speed budget allows"
            );
            error -= step;
            steps += 1;
        }
        assert!(error.abs() <= 1e-3, "the error is worked off");
        assert!(steps > 60, "and not inside a single frame: {steps} frames");
    }
}
