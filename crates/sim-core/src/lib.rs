//! Core of the train simulation: fixed time step, deterministic, without Bevy (plan ch. 3.1).
//!
//! Order per step (plan 3.2): electrics → traction/brake → longitudinal dynamics →
//! position on the track graph → train protection → interlocking → AI.

pub mod blocks;
pub mod brakes;
pub mod cab;
pub mod consist;
pub mod day;
pub mod doors;
pub mod drive;
pub mod electric;
pub mod interlock;
pub mod lookahead;
pub mod physics;
pub mod rng;
pub mod safety;
pub mod scenario;
pub mod score;
pub mod shunt;
pub mod signal;
pub mod sound;
pub mod steam;
pub mod synth;
pub mod timetable;
pub mod train;
pub mod weather;
pub mod yard;

/// Gravitational acceleration [m/s²].
pub const G: f64 = 9.806_65;

use brakes::DriverBrakeValve;
use cab::CabInputs;
use interlock::Interlock;
use safety::de::LzbSection;
use safety::{ProtectionAction, ProtectionOutput, SafetyTrainState, TracksideEvent};
use serde::{Deserialize, Serialize};
use track_model::{DeviceKind, EdgeId, TrackNetwork, TrackPosition};
use train::{Train, Vehicle};

/// Runtime state of a train that does not belong in the vehicle model.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrainRuntime {
    /// Distance travelled [m], monotonic — reference for all distance supervisions
    /// of the train protection.
    pub odometer: f64,
    /// Last output of the train protection.
    #[serde(skip)]
    pub protection: ProtectionOutput,
    /// Last output of the door control.
    #[serde(skip)]
    pub doors: doors::DoorOutput,
    /// Remaining distance without contact wire voltage (neutral section) [m].
    pub neutral_section_left: f64,
    /// Train is stopped because of a node that cannot be passed.
    pub blocked: bool,
    /// Line conductor section the train is running in, and how far it transmits.
    pub lzb_section: Option<safety::de::LzbSection>,
    pub lzb_until_odo: f64,
    /// The shunting order the driver has given and how long it is still tried for
    /// (plan ch. 11). Derived from `CabInputs::shunt`, which is what travels.
    #[serde(default)]
    pub shunt_request: shunt::ShuntRequest,
    /// What became of it — the shunter's answer, for the HUD. Local, like every other
    /// result in here.
    #[serde(default = "shunt::default_report")]
    pub shunt: shunt::ShuntReport,
}

/// How far ahead a shunting movement looks for the signal that is holding it, when it asks
/// for a route out of it [m] — far enough to have the points move while it is still
/// rolling up, short enough that it is asking about the signal it is actually at.
const SHUNT_ROUTE_REACH: f64 = 400.0;

/// The whole simulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sim {
    pub net: TrackNetwork,
    pub interlock: Interlock,
    pub trains: Vec<Train>,
    pub runtime: Vec<TrainRuntime>,
    /// Cabs: one set of inputs per train (AI or player write into it).
    pub controls: Vec<CabInputs>,
    /// Stabling roads and portals of the line — where stock may be put and where trains
    /// appear and disappear (plan ch. 11). Line content, filled in when the world is
    /// built; a run without them simply has none.
    #[serde(default)]
    pub yards: Vec<yard::Yard>,
    /// Simulation time [s since the start of the run].
    pub time: f64,
    /// Wall clock at `time == 0` — date and time of day (plan ch. 14).
    #[serde(default)]
    pub start: scenario::StartTime,
    pub rng: rng::Rng,
    /// Scenario with events and messages (plan 11.4).
    #[serde(default)]
    pub scenario: scenario::ScenarioRuntime,
    /// Scoring of the player train's run (plan 11).
    #[serde(default)]
    pub score: score::ScoreKeeper,
    /// Weather and what it has left on the ground (plan 14.1) — moved by scenario
    /// actions, read by the renderer, the sound and the rail condition.
    #[serde(default)]
    pub weather: weather::Timeline,
    /// Fixed steps taken since the start of the run — what the once-a-second jobs of
    /// [`step`](Self::step) are paced by. A count rather than the clock, because the clock
    /// is a sum of floats and a modulo of it drifts.
    #[serde(default)]
    steps: u64,
    accumulator: f64,
}

impl Sim {
    /// Fixed physics time step [s] (200 Hz, plan 3.1).
    pub const DT: f64 = 1.0 / 200.0;
    /// No more than this is caught up per frame (spiral-of-death protection).
    pub const MAX_CATCHUP: f64 = 0.25;

    pub fn new(net: TrackNetwork, interlock: Interlock, seed: u64) -> Self {
        Self {
            net,
            interlock,
            trains: Vec::new(),
            runtime: Vec::new(),
            controls: Vec::new(),
            yards: Vec::new(),
            time: 0.0,
            start: scenario::StartTime::default(),
            rng: rng::Rng::new(seed),
            scenario: scenario::ScenarioRuntime::default(),
            score: score::ScoreKeeper::default(),
            weather: weather::Timeline::default(),
            steps: 0,
            accumulator: 0.0,
        }
    }

    /// Load a scenario; the player train is scored as well.
    pub fn set_scenario(&mut self, scenario: scenario::Scenario, timetable: timetable::Timetable) {
        self.start = scenario.start;
        self.weather.place(scenario.weather.weather(), 0.0);
        self.score = score::ScoreKeeper::new(scenario.player_train, timetable);
        self.scenario = scenario::ScenarioRuntime::new(scenario);
    }

    /// Wall clock [s since local midnight of the start day]; keeps growing past
    /// [`timetable::DAY`] on multi-day runs.
    pub fn clock(&self) -> f64 {
        self.start.seconds() + self.time
    }

    pub fn add_train(&mut self, train: Train) -> usize {
        self.trains.push(train);
        self.runtime.push(TrainRuntime::default());
        self.controls.push(CabInputs::default());
        self.trains.len() - 1
    }

    /// Advances the simulation by `dt` seconds of real time (fixed time step internally).
    pub fn advance(&mut self, dt: f64) {
        self.accumulator = (self.accumulator + dt).min(Self::MAX_CATCHUP);
        while self.accumulator >= Self::DT {
            self.step(Self::DT);
            self.accumulator -= Self::DT;
        }
    }

    /// One fixed simulation step.
    pub fn step(&mut self, dt: f64) {
        // The weather first: it moves the sky on and decides what the wheels find
        // on the rail this step (plan 14.1).
        self.weather.step(self.time, self.clock(), dt);
        let rail = self.weather.rail();
        for train in &mut self.trains {
            train.rail = rail;
        }

        for i in 0..self.trains.len() {
            // A stabled train is out of service: no physics, and no place on the line
            // (see `Train::stabled`).
            if self.trains[i].stabled {
                continue;
            }
            // The shunter first: a coupling made this step is one consist by the time the
            // couplers are worked out below (plan ch. 11). The loop's bound was taken
            // before it started, so the rear part of an uncoupling is not stepped until
            // the next one — five milliseconds at a stand, which is what an uncoupling is.
            shunt::step(self, i, dt);
            if self.trains[i].stabled || self.trains[i].vehicles.is_empty() {
                continue;
            }
            self.step_train(i, dt);
        }

        // Interlocking: track clear detection, routes, signals, switch movements.
        let occupied = self.occupied_edges();
        self.interlock.update_occupancy(&occupied);
        self.set_shunt_routes();
        self.interlock.update(&mut self.net, dt);
        self.net.update_switches(dt);

        // Scoring and scenario last — they see the finished state of the step.
        let mut score = std::mem::take(&mut self.score);
        score.update(self, dt);
        self.score = score;
        scenario::step(self);

        self.time += dt;
    }

    /// Automatic shunting-route setting (plan ch. 10).
    ///
    /// A shunting movement that has drawn up to a signal showing Sh 0 is given the first
    /// shunting route out of that signal whose path is free — which is what the signalman
    /// does when a shunt is standing in front of him waiting. Train routes are not set
    /// this way: a train movement is timetabled, and which route it takes is a decision,
    /// not a reflex.
    ///
    /// It is a pure function of the world and runs inside the fixed step, so every peer
    /// sets the same route in the same step without a message about it (CLAUDE.md ch. 20).
    fn set_shunt_routes(&mut self) {
        // Once a second, and only for what is standing: a movement that is rolling is not
        // waiting in front of a signal, and looking 400 m up the track two hundred times a
        // second for every train on the line is a scan nobody asked for.
        self.steps = self.steps.wrapping_add(1);
        if !self.steps.is_multiple_of((1.0 / Self::DT) as u64) {
            return;
        }
        for i in 0..self.trains.len() {
            let train = &self.trains[i];
            if train.stabled || train.vehicles.is_empty() || train.speed().abs() > 1.0 {
                continue;
            }
            // The end that leads: a move setting back is stopped by the signal behind it.
            let end = if self.controls[i].reverser < 0 {
                train::ConsistEnd::Tail
            } else {
                train::ConsistEnd::Head
            };
            let scanning = train.movement;
            let Some(mut from) = train.end_position(&self.net, end) else {
                continue;
            };
            if end == train::ConsistEnd::Tail {
                from.dir = -from.dir;
            }
            let view = lookahead::scan(
                &self.net,
                &self.interlock,
                from,
                SHUNT_ROUTE_REACH,
                scanning,
            );
            let Some(entry) = view.next_stop().and_then(|stop| stop.signal) else {
                continue;
            };
            // Who is asked for: a movement that is already shunting, and anything at all
            // that is held by a **Sperrsignal** — a signal that authorises nothing but
            // shunting, so whatever is standing in front of it is about to shunt, whether
            // it has been told so yet or not. That is how a unit stabled in a siding gets
            // out of it: it is let past by Sh 1, and passing Sh 1 is what makes it a
            // shunting movement in the first place.
            let sperrsignal = self.interlock.signal(entry).kind == interlock::SignalKind::Shunting;
            if self.trains[i].movement != shunt::Movement::Shunt && !sperrsignal {
                continue;
            }
            let mut interlock = std::mem::take(&mut self.interlock);
            interlock.request_shunt_route(entry, &mut self.net);
            self.interlock = interlock;
        }
    }

    /// What the track clear detection sees: every edge a train stands on, and which train
    /// it is. *Which* matters to a shunting route, which may run over a road that was
    /// occupied before it started (`interlock::Route::owner`).
    fn occupied_edges(&self) -> Vec<(usize, EdgeId)> {
        let mut edges = Vec::new();
        for (index, t) in self.trains.iter().enumerate() {
            if t.stabled {
                continue;
            }
            for v in &t.vehicles {
                if !edges.contains(&(index, v.pos.edge)) {
                    edges.push((index, v.pos.edge));
                }
            }
        }
        edges
    }

    fn step_train(&mut self, index: usize, dt: f64) {
        // An empty consist has nothing to drive: a train that was coupled away keeps its
        // slot (see `crate::shunt`), and everything below reads its leading vehicle.
        if self.trains[index].vehicles.is_empty() {
            return;
        }
        let mut cab = self.controls[index];
        let action = self.runtime[index].protection.action;

        // AFB (plan 9.4): the controller replaces power controller and brake valve for
        // this step; the driver's levers themselves stay where they were left.
        if let Some(afb) = cab::afb_control(&self.trains[index], &cab) {
            cab.throttle = afb.throttle;
            cab.brake_valve = afb.valve;
        }

        // 0a. Signal graph — the control logic the vehicle's diagram was built out of. It
        // runs before everything else, so what it commands takes hold in this same step.
        let extra_brake = self.step_signals(index, &cab, dt);
        if let Some(throttle) = self.trains[index]
            .vehicles
            .get(
                self.trains[index]
                    .cab
                    .min(self.trains[index].vehicles.len().saturating_sub(1)),
            )
            .and_then(|v| v.signal_out.throttle)
        {
            cab.throttle = throttle;
        }
        if extra_brake > 0.0
            && let Some(target) = signal_brake_valve(cab.brake_valve, extra_brake)
        {
            cab.brake_valve = target;
        }

        // 0. Doors — traction interlock and door loop act on this step already.
        let doors = doors::step(&mut self.trains[index], &cab, dt);
        self.runtime[index].doors = doors;

        // Forced braking overrides the driver's brake valve and the traction.
        let valve = match action {
            _ if doors.emergency => DriverBrakeValve::Emergency,
            ProtectionAction::EmergencyBrake => DriverBrakeValve::Emergency,
            ProtectionAction::ForcedServiceBrake => DriverBrakeValve::Service(1.5),
            _ => cab.brake_valve,
        };
        let traction_allowed = action == ProtectionAction::None && !doors.traction_lock;

        // 1. Electrics and drive.
        {
            let net = &self.net;
            let train = &mut self.trains[index];
            let neutral = self.runtime[index].neutral_section_left > 0.0;
            for veh in &mut train.vehicles {
                if veh.spec.drives.is_empty() {
                    continue;
                }
                // What is over the vehicle is a property of the track, read at the
                // pantograph — a section with no wire, or one carrying a system this
                // vehicle was not built for, leaves its main switch open.
                let system = if neutral {
                    None
                } else {
                    net.electrification_at(veh.pos.edge, veh.pos.s)
                };
                veh.traction.line_system = system;
                veh.traction.line_voltage = system.map_or(0.0, |s| s.voltage());
                // The power controller says how hard the machine pulls, the reverser
                // which way — in neutral it does not pull at all. A dynamic brake works
                // on the direction of travel, so in back gear it is the air brake's job
                // and the notch never goes negative there.
                veh.traction.notch = if traction_allowed {
                    let notch = if cab.reverser == 0 { 0.0 } else { cab.throttle };
                    if cab.reverser < 0 {
                        notch.max(0.0)
                    } else {
                        notch
                    }
                } else {
                    // On forced braking: traction off, dynamic brake stays allowed.
                    cab.throttle.min(0.0)
                };
                veh.traction.back_gear = cab.reverser < 0;
                veh.sanding = cab.sanding;
                // The range selector goes to the drive, which only lets it take at a stand.
                veh.traction.road_gear = cab.road_gear;
                // Steam: the fireman's and the driver's hands go straight through.
                veh.traction.steam_controls = cab.steam;
                if cab.shovel > 0.0
                    && let Some(boiler) = veh.traction.drives[0].steam.as_mut()
                    && let Some(drive::TractionSpec::Steam { loco, .. }) =
                        veh.spec.drives.first().map(|d| &d.traction)
                {
                    steam::fire(boiler, loco, cab.shovel);
                }
                if cab.engine_start {
                    // Cranking is what empties a battery; a flat one will not turn the
                    // engine over at all.
                    let battery = veh.traction.battery
                        && electric::crank_battery(&mut veh.traction, &veh.spec.supply, dt);
                    for (i, drive) in veh.spec.drives.iter().enumerate() {
                        electric::start_engine(
                            &mut veh.traction.drives[i],
                            &drive.traction,
                            battery,
                        );
                    }
                }
                electric::step(
                    &mut veh.traction,
                    &veh.spec.drives,
                    &veh.spec.supply,
                    veh.v,
                    dt,
                );
            }
        }

        // 2. Brake — one control valve per vehicle, plus main reservoir and compressor.
        brakes::step(&mut self.trains[index], &cab, valve, dt);

        // 3./4. Longitudinal dynamics and position on the track graph.
        let report = physics::step(&mut self.trains[index], &self.net, dt);
        self.runtime[index].blocked = report.blocked.is_some();

        let dx = self.trains[index].speed().abs() * dt;
        self.runtime[index].odometer += dx;
        if self.runtime[index].neutral_section_left > 0.0 {
            self.runtime[index].neutral_section_left -= dx;
        }

        // 5. Train protection: evaluate trackside devices.
        let events = self.collect_events(index, &report);
        let state = SafetyTrainState {
            v_kmh: self.trains[index].speed_kmh().abs(),
            odometer: self.runtime[index].odometer,
            line_speed: self.trains[index].vehicles[0].pos.speed_limit(&self.net),
            braking: !self.trains[index].vehicles[0].brake.released(),
            train_length: self.trains[index].length(),
            brake_percentage: self.trains[index].brake_percentage(),
            brake_apply_time: self.trains[index].brake_apply_time(),
        };
        let mut out = ProtectionOutput::default();
        let train = &mut self.trains[index];
        for veh in &mut train.vehicles {
            out = out.merge(veh.safety.update(dt, &state, &cab, &events));
        }
        self.runtime[index].protection = out;
    }

    /// Runs every vehicle's signal graph. Returns the largest extra brake demand any of
    /// them asked for — the brake is a train-wide thing, so the strongest wins.
    fn step_signals(&mut self, index: usize, cab: &CabInputs, dt: f64) -> f64 {
        let train = &mut self.trains[index];
        let mut extra_brake: f64 = 0.0;
        for veh in &mut train.vehicles {
            if veh.spec.signal.is_empty() {
                continue;
            }
            let drive = veh.traction.drives[0];
            let readings = signal::SignalReadings {
                throttle: cab.throttle,
                brake_demand: cab.brake_valve.demand(),
                direct_brake: cab.direct_brake,
                speed: veh.v,
                target_speed_kmh: cab.afb_target,
                cylinder: veh.brake.applied_cylinder(),
                pipe: veh.brake.pipe,
                main_reservoir: veh.brake.main_reservoir,
                motor_current: drive.motor_current,
                engine_rpm: drive.engine_rpm,
                temperature: drive.peak_temp(),
                tractive_effort: veh.tractive_effort,
                reverser: f64::from(cab.reverser),
                sanding: veh.sanding,
            };
            let out = signal::step(&veh.spec.signal, &mut veh.signal, &readings, dt);
            veh.signal_out = out;
            extra_brake = extra_brake.max(out.brake_demand);
            if out.sanding {
                veh.sanding = true;
            }
            if let Some(blower) = out.blower {
                for chain in &mut veh.traction.drives {
                    chain.blower = blower;
                }
            }
        }
        extra_brake
    }

    /// Builds the events for the train protection from the devices that were passed.
    fn collect_events(
        &mut self,
        index: usize,
        report: &physics::StepReport,
    ) -> Vec<TracksideEvent> {
        let mut events = Vec::new();
        let dir = self.trains[index].vehicles[0].pos.dir;
        for (vehicle, passed) in &report.passed {
            // Passing a signal says what kind of movement this is from here on: Sh 1
            // makes it a shunting movement, a main proceed aspect makes it a train. That
            // is how a shunt draws up to the starting signal, is given a train route, and
            // leaves as a train (`shunt::Movement`). It is read before the antenna check —
            // a movement is a movement whether the vehicle carries train protection or not.
            let (kind, facing, device_id) = {
                let device = self.net.device(passed.device);
                (device.kind.clone(), device.facing, device.id)
            };
            if kind == DeviceKind::Signal
                && facing.applies(dir)
                && let Some(signal) = self.interlock.signal_at_device(device_id)
            {
                let (aspect, id) = (signal.aspect, signal.id);
                if aspect.permits_shunting() {
                    self.trains[index].movement = shunt::Movement::Shunt;
                } else if aspect.main.is_some_and(|main| !main.is_stop()) {
                    self.trains[index].movement = shunt::Movement::Train;
                }
                // And the shunting route it was let past by is *this* movement's from
                // here on: it is released when this train has cleared it, not when the
                // next thing runs past the signal (`Interlock::entered`).
                if aspect.permits_shunting() {
                    self.interlock.entered(id, index);
                }
            }
            // Only vehicles carrying an antenna read trackside devices.
            if matches!(
                self.trains[index].vehicles[*vehicle].safety,
                safety::SafetySystems::None
            ) {
                continue;
            }
            let device = self.net.device(passed.device);
            if device.kind == DeviceKind::NeutralSection {
                #[derive(serde::Deserialize, Default)]
                struct Neutral {
                    #[serde(default)]
                    length: f64,
                }
                let n: Neutral = ron::from_str(&device.payload).unwrap_or_default();
                self.runtime[index].neutral_section_left = n.length.max(1.0);
            }
            if device.kind == DeviceKind::LineConductor {
                // The cable itself is only line data; the authority comes from the centre
                // below, freshly for every step.
                let section: LzbSection = ron::from_str(&device.payload).unwrap_or_default();
                self.runtime[index].lzb_section = Some(section);
                self.runtime[index].lzb_until_odo =
                    self.runtime[index].odometer + section.length.max(1.0);
                continue;
            }
            events.push(TracksideEvent {
                device: device.kind.clone(),
                payload: device.payload.clone(),
                s_offset: passed.distance_behind,
                active: self.interlock.device_active(device),
            });
        }

        // The loop cable transmits continuously: the LZB centre builds the movement authority
        // out of the line's block division and the current state of the interlocking, so a
        // signal going to stop ahead of the train shortens the authority at once.
        let rt = &self.runtime[index];
        if rt.odometer < rt.lzb_until_odo
            && let Some(section) = rt.lzb_section
            && let Some(head) = self.trains[index].head()
        {
            let telegram = safety::de::lzb::authority(&self.net, &self.interlock, head, &section);
            events.push(TracksideEvent {
                device: DeviceKind::LineConductor,
                payload: ron::to_string(&telegram).unwrap_or_default(),
                s_offset: 0.0,
                active: true,
            });
        }
        events
    }

    /// State hash for determinism and regression tests (plan 16.1/18).
    pub fn state_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |x: u64| {
            h ^= x;
            h = h.wrapping_mul(0x100_0000_01b3);
        };
        for t in &self.trains {
            for v in &t.vehicles {
                mix(v.x.to_bits());
                mix(v.v.to_bits());
                mix(v.brake.pipe.to_bits());
                mix(v.brake.cylinder.to_bits());
                mix(v.pos.s.to_bits());
                mix(v.pos.edge.0 as u64);
            }
        }
        for r in &self.runtime {
            mix(r.odometer.to_bits());
        }
        mix(self.time.to_bits());
        h
    }
}

/// The brake valve position a signal graph's extra demand asks for — never less than what
/// the driver already has on, because a controller may add brake but must not take it off.
fn signal_brake_valve(current: DriverBrakeValve, demand: f64) -> Option<DriverBrakeValve> {
    let wanted = demand.clamp(0.0, 1.0) * brakes::FULL_SERVICE_DROP;
    match current {
        DriverBrakeValve::Emergency => None,
        DriverBrakeValve::Service(drop) if drop >= wanted => None,
        _ => Some(DriverBrakeValve::Service(wanted)),
    }
}

/// Shorthand for assembling a train at a track position.
pub fn spawn(vehicles: Vec<Vehicle>, at: TrackPosition, net: &TrackNetwork) -> Train {
    Train::assemble(vehicles, at, net)
}
