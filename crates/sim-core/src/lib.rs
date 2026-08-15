//! Core of the train simulation: fixed time step, deterministic, without Bevy (plan ch. 3.1).
//!
//! Order per step (plan 3.2): electrics → traction/brake → longitudinal dynamics →
//! position on the track graph → train protection → interlocking → AI.

pub mod brakes;
pub mod cab;
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
pub mod sound;
pub mod timetable;
pub mod train;

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
}

/// The whole simulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sim {
    pub net: TrackNetwork,
    pub interlock: Interlock,
    pub trains: Vec<Train>,
    pub runtime: Vec<TrainRuntime>,
    /// Cabs: one set of inputs per train (AI or player write into it).
    pub controls: Vec<CabInputs>,
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
    /// Weather — set by scenario actions, rendered by the app (plan ch. 14).
    #[serde(default)]
    pub weather: train::Weather,
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
            time: 0.0,
            start: scenario::StartTime::default(),
            rng: rng::Rng::new(seed),
            scenario: scenario::ScenarioRuntime::default(),
            score: score::ScoreKeeper::default(),
            weather: train::Weather::default(),
            accumulator: 0.0,
        }
    }

    /// Load a scenario; the player train is scored as well.
    pub fn set_scenario(&mut self, scenario: scenario::Scenario, timetable: timetable::Timetable) {
        self.start = scenario.start;
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
        for i in 0..self.trains.len() {
            self.step_train(i, dt);
        }

        // Interlocking: track clear detection, routes, signals, switch movements.
        let occupied = self.occupied_edges();
        self.interlock.update_occupancy(&occupied);
        self.interlock.update(&mut self.net);
        self.net.update_switches(dt);

        // Scoring and scenario last — they see the finished state of the step.
        let mut score = std::mem::take(&mut self.score);
        score.update(self, dt);
        self.score = score;
        scenario::step(self);

        self.time += dt;
    }

    fn occupied_edges(&self) -> Vec<EdgeId> {
        let mut edges = Vec::new();
        for t in &self.trains {
            for v in &t.vehicles {
                if !edges.contains(&v.pos.edge) {
                    edges.push(v.pos.edge);
                }
            }
        }
        edges
    }

    fn step_train(&mut self, index: usize, dt: f64) {
        let mut cab = self.controls[index];
        let action = self.runtime[index].protection.action;

        // AFB (plan 9.4): the controller replaces power controller and brake valve for
        // this step; the driver's levers themselves stay where they were left.
        if let Some(afb) = cab::afb_control(&self.trains[index], &cab) {
            cab.throttle = afb.throttle;
            cab.brake_valve = afb.valve;
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
            let train = &mut self.trains[index];
            let neutral = self.runtime[index].neutral_section_left > 0.0;
            for veh in &mut train.vehicles {
                let Some(spec) = veh.spec.traction.clone() else {
                    continue;
                };
                veh.traction.line_voltage = if neutral {
                    0.0
                } else {
                    electric::NOMINAL_LINE_VOLTAGE
                };
                veh.traction.notch = if traction_allowed {
                    cab.throttle * cab.reverser.max(0) as f64
                } else {
                    // On forced braking: traction off, dynamic brake stays allowed.
                    cab.throttle.min(0.0)
                };
                veh.sanding = cab.sanding;
                if cab.engine_start {
                    electric::start_engine(&mut veh.traction, &spec);
                }
                electric::step(&mut veh.traction, &spec, veh.v, dt);
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

    /// Builds the events for the train protection from the devices that were passed.
    fn collect_events(
        &mut self,
        index: usize,
        report: &physics::StepReport,
    ) -> Vec<TracksideEvent> {
        let mut events = Vec::new();
        for (vehicle, passed) in &report.passed {
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
        {
            let head = self.trains[index].head_position();
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

/// Shorthand for assembling a train at a track position.
pub fn spawn(vehicles: Vec<Vehicle>, at: TrackPosition, net: &TrackNetwork) -> Train {
    Train::assemble(vehicles, at, net)
}
