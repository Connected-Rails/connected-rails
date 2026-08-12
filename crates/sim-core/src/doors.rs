//! Passenger door control (plan ch. 9, sibling of the train protection).
//!
//! Three builds, all sharing the same two interlocks — no traction while a door is not
//! closed and locked, and an unlocked door above walking pace applies the emergency brake:
//!
//! * [`DoorSystem::Tb0`] — doors blocked from 0 km/h. The driver releases them at a
//!   standstill and closes them himself; nothing closes by itself.
//! * [`DoorSystem::Tav`] — like TB0 plus automatic closing: the doors close on their own
//!   once [`DoorParams::auto_close`] has run out (driver-only dispatch).
//! * [`DoorSystem::UicWtb`] — TAV over the UIC 556 / IEC 61375 train bus. After the
//!   consist changes the bus has to be inaugurated before commands are accepted, and a
//!   command reaches vehicle *n* only after *n* bus cycles.

use crate::cab::{CabInputs, Edge};
use crate::train::Train;
use serde::{Deserialize, Serialize};

/// Door control system of a train.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DoorSystem {
    /// No passenger door control (freight train, light engine).
    #[default]
    None,
    Tb0,
    Tav,
    UicWtb,
}

impl DoorSystem {
    /// Does the release end by itself after the boarding time?
    fn closes_automatically(self) -> bool {
        matches!(self, DoorSystem::Tav | DoorSystem::UicWtb)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DoorParams {
    /// Travel time of a door leaf [s].
    pub travel: f64,
    /// Time from the release until a passenger presses the door button [s].
    pub boarding: f64,
    /// Time an open door stays open before TAV/WTB closes it [s].
    pub auto_close: f64,
    /// A release is only accepted below this speed [km/h].
    pub release_below: f64,
    /// Above this speed an unlocked door applies the emergency brake [km/h].
    pub brake_above: f64,
    /// Time a WTB command needs per vehicle [s] (process data period).
    pub bus_cycle: f64,
    /// Duration of the WTB inauguration after a consist change [s].
    pub inauguration: f64,
}

impl Default for DoorParams {
    fn default() -> Self {
        Self {
            travel: 2.0,
            boarding: 1.0,
            auto_close: 20.0,
            release_below: 0.5,
            brake_above: 5.0,
            bus_cycle: 0.025,
            inauguration: 3.0,
        }
    }
}

/// Phase of one side of a vehicle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DoorPhase {
    /// Closed and locked — only in this phase is traction permitted.
    #[default]
    Locked,
    /// Released and still closed: the passenger may press the door button.
    Released,
    Opening,
    Open,
    Closing,
}

impl DoorPhase {
    pub fn is_locked(self) -> bool {
        self == DoorPhase::Locked
    }
}

/// One side of one vehicle.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct DoorSide {
    pub phase: DoorPhase,
    /// Leaf position 0 (closed) … 1 (open) — for the model animation.
    pub travel: f64,
    /// Time in the current phase [s].
    timer: f64,
}

impl DoorSide {
    fn enter(&mut self, phase: DoorPhase) {
        self.phase = phase;
        self.timer = 0.0;
    }

    fn step(&mut self, dt: f64, p: &DoorParams, auto: bool, moving: bool) {
        self.timer += dt;
        match self.phase {
            // TB0 blocks from 0 km/h: a released but still closed door locks again as
            // soon as the train moves.
            DoorPhase::Released if moving => self.enter(DoorPhase::Locked),
            DoorPhase::Released if self.timer >= p.boarding => self.enter(DoorPhase::Opening),
            DoorPhase::Opening => {
                self.travel = (self.travel + dt / p.travel).min(1.0);
                if self.travel >= 1.0 {
                    self.enter(DoorPhase::Open);
                }
            }
            DoorPhase::Open if auto && self.timer >= p.auto_close => self.enter(DoorPhase::Closing),
            DoorPhase::Closing => {
                self.travel = (self.travel - dt / p.travel).max(0.0);
                if self.travel <= 0.0 {
                    self.enter(DoorPhase::Locked);
                }
            }
            _ => {}
        }
    }
}

/// Doors of one vehicle.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct VehicleDoors {
    pub left: DoorSide,
    pub right: DoorSide,
}

impl VehicleDoors {
    pub fn closed_and_locked(&self) -> bool {
        self.left.phase.is_locked() && self.right.phase.is_locked()
    }
}

/// A command travelling through the train.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum Command {
    Release { left: bool, right: bool },
    Close,
}

/// Door control of a train — the driver's side of the system.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct DoorControl {
    pub system: DoorSystem,
    pub params: DoorParams,
    /// Command on the bus and how long it has been travelling [s].
    command: Option<(Command, f64)>,
    /// Number of vehicles the bus was inaugurated for.
    nodes: usize,
    /// Remaining time of the inauguration [s].
    inauguration: f64,
    release_left: Edge,
    release_right: Edge,
    close: Edge,
}

/// What the door control tells the rest of the simulation.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DoorOutput {
    /// Traction interlock: no traction while a door is not closed and locked.
    pub traction_lock: bool,
    /// Door loop broken while running — emergency brake.
    pub emergency: bool,
    /// "Closed and locked" indication in the cab.
    pub closed_and_locked: bool,
    /// The train bus is being inaugurated; commands are not accepted yet.
    pub inaugurating: bool,
}

impl DoorControl {
    pub fn new(system: DoorSystem) -> Self {
        Self {
            system,
            ..Self::default()
        }
    }
}

/// One step of the door control. Runs before traction and brake so that both see the
/// current interlock.
pub fn step(train: &mut Train, cab: &CabInputs, dt: f64) -> DoorOutput {
    let v_kmh = train.speed_kmh().abs();
    let mut ctl = train.doors;
    let out = update(&mut ctl, &mut train.vehicles, cab, v_kmh, dt);
    train.doors = ctl;
    out
}

fn update(
    ctl: &mut DoorControl,
    vehicles: &mut [crate::train::Vehicle],
    cab: &CabInputs,
    v_kmh: f64,
    dt: f64,
) -> DoorOutput {
    // The edges have to be read in every step, otherwise a button held down while the
    // system is off would fire on the next release.
    let release_left = ctl.release_left.rising(cab.door_release_left);
    let release_right = ctl.release_right.rising(cab.door_release_right);
    let close = ctl.close.rising(cab.door_close);

    if ctl.system == DoorSystem::None {
        return DoorOutput {
            closed_and_locked: true,
            ..DoorOutput::default()
        };
    }

    // WTB: a changed consist re-inaugurates the bus.
    let wtb = ctl.system == DoorSystem::UicWtb;
    if wtb && ctl.nodes != vehicles.len() {
        ctl.nodes = vehicles.len();
        ctl.inauguration = ctl.params.inauguration;
    }
    ctl.inauguration = (ctl.inauguration - dt).max(0.0);
    let bus_ready = !wtb || ctl.inauguration <= 0.0;

    // New command from the cab. A release is only accepted at a standstill.
    if bus_ready {
        if close {
            ctl.command = Some((Command::Close, 0.0));
        } else if (release_left || release_right) && v_kmh <= ctl.params.release_below {
            ctl.command = Some((
                Command::Release {
                    left: release_left,
                    right: release_right,
                },
                0.0,
            ));
        }
    }

    let moving = v_kmh > ctl.params.release_below;
    let auto = ctl.system.closes_automatically();
    // A command reaches vehicle i after i bus cycles; without a bus it acts at once.
    let delay_of = |i: usize| {
        if wtb {
            i as f64 * ctl.params.bus_cycle
        } else {
            0.0
        }
    };
    for (i, vehicle) in vehicles.iter_mut().enumerate() {
        if let Some((command, age)) = ctl.command {
            let delay = delay_of(i);
            if age >= delay && age - dt < delay && vehicle.spec.passenger_doors {
                apply(command, &mut vehicle.doors);
            }
        }
        vehicle.doors.left.step(dt, &ctl.params, auto, moving);
        vehicle.doors.right.step(dt, &ctl.params, auto, moving);
    }

    // Retire the command once the last vehicle has seen it.
    let last = delay_of(vehicles.len().saturating_sub(1));
    if let Some((_, age)) = &mut ctl.command {
        *age += dt;
        // The last vehicle applies the command in the step in which `age` passes its
        // delay, so the command may only be dropped one step later.
        if *age >= last + dt {
            ctl.command = None;
        }
    }

    let locked = vehicles.iter().all(|v| v.doors.closed_and_locked());
    DoorOutput {
        traction_lock: !locked,
        emergency: !locked && v_kmh > ctl.params.brake_above,
        closed_and_locked: locked,
        inaugurating: !bus_ready,
    }
}

fn apply(command: Command, doors: &mut VehicleDoors) {
    match command {
        Command::Release { left, right } => {
            for (side, wanted) in [(&mut doors.left, left), (&mut doors.right, right)] {
                if wanted && side.phase.is_locked() {
                    side.enter(DoorPhase::Released);
                }
            }
        }
        Command::Close => {
            for side in [&mut doors.left, &mut doors.right] {
                match side.phase {
                    // Released but never opened: locking again is immediate.
                    DoorPhase::Released => side.enter(DoorPhase::Locked),
                    DoorPhase::Opening | DoorPhase::Open => side.enter(DoorPhase::Closing),
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::train::{Vehicle, VehicleSpec};
    use track_model::{EdgeId, TrackPosition};

    fn coaches(system: DoorSystem, n: usize) -> (DoorControl, Vec<Vehicle>) {
        let spec = VehicleSpec {
            passenger_doors: true,
            ..VehicleSpec::default()
        };
        let vehicles = (0..n)
            .map(|_| Vehicle::new(spec.clone(), TrackPosition::new(EdgeId(0), 0.0, 1)))
            .collect();
        (DoorControl::new(system), vehicles)
    }

    fn run(
        ctl: &mut DoorControl,
        vehicles: &mut [Vehicle],
        cab: &CabInputs,
        v_kmh: f64,
        seconds: f64,
    ) -> DoorOutput {
        let dt = 0.05;
        let mut out = DoorOutput::default();
        for _ in 0..(seconds / dt) as usize {
            out = update(ctl, vehicles, cab, v_kmh, dt);
        }
        out
    }

    fn press_release_left(ctl: &mut DoorControl, vehicles: &mut [Vehicle]) {
        let cab = CabInputs {
            door_release_left: true,
            ..CabInputs::default()
        };
        update(ctl, vehicles, &cab, 0.0, 0.05);
    }

    #[test]
    fn release_opens_the_doors_and_blocks_traction() {
        let (mut ctl, mut vehicles) = coaches(DoorSystem::Tb0, 2);
        let out = run(&mut ctl, &mut vehicles, &CabInputs::default(), 0.0, 1.0);
        assert!(out.closed_and_locked && !out.traction_lock);

        press_release_left(&mut ctl, &mut vehicles);
        let out = run(&mut ctl, &mut vehicles, &CabInputs::default(), 0.0, 5.0);
        assert_eq!(vehicles[0].doors.left.phase, DoorPhase::Open);
        assert_eq!(vehicles[0].doors.right.phase, DoorPhase::Locked, "one side");
        assert!(out.traction_lock, "no traction with an open door");

        // TB0 does not close by itself.
        let out = run(&mut ctl, &mut vehicles, &CabInputs::default(), 0.0, 60.0);
        assert_eq!(vehicles[0].doors.left.phase, DoorPhase::Open);
        assert!(out.traction_lock);

        let cab = CabInputs {
            door_close: true,
            ..CabInputs::default()
        };
        update(&mut ctl, &mut vehicles, &cab, 0.0, 0.05);
        let out = run(&mut ctl, &mut vehicles, &CabInputs::default(), 0.0, 5.0);
        assert!(out.closed_and_locked && !out.traction_lock);
    }

    #[test]
    fn tav_closes_by_itself_tb0_does_not() {
        let (mut ctl, mut vehicles) = coaches(DoorSystem::Tav, 1);
        press_release_left(&mut ctl, &mut vehicles);
        run(&mut ctl, &mut vehicles, &CabInputs::default(), 0.0, 5.0);
        assert_eq!(vehicles[0].doors.left.phase, DoorPhase::Open);
        let out = run(&mut ctl, &mut vehicles, &CabInputs::default(), 0.0, 30.0);
        assert!(out.closed_and_locked, "TAV closes after auto_close");
    }

    #[test]
    fn no_release_while_the_train_rolls() {
        let (mut ctl, mut vehicles) = coaches(DoorSystem::Tb0, 1);
        let cab = CabInputs {
            door_release_left: true,
            ..CabInputs::default()
        };
        update(&mut ctl, &mut vehicles, &cab, 20.0, 0.05);
        run(&mut ctl, &mut vehicles, &CabInputs::default(), 20.0, 5.0);
        assert_eq!(vehicles[0].doors.left.phase, DoorPhase::Locked);
    }

    #[test]
    fn open_door_at_speed_triggers_the_emergency_brake() {
        let (mut ctl, mut vehicles) = coaches(DoorSystem::Tb0, 1);
        press_release_left(&mut ctl, &mut vehicles);
        let out = run(&mut ctl, &mut vehicles, &CabInputs::default(), 0.0, 5.0);
        assert!(!out.emergency, "at a standstill it is only the interlock");
        let out = update(&mut ctl, &mut vehicles, &CabInputs::default(), 30.0, 0.05);
        assert!(out.emergency);
    }

    #[test]
    fn wtb_inaugurates_before_it_accepts_commands() {
        let (mut ctl, mut vehicles) = coaches(DoorSystem::UicWtb, 4);
        let cab = CabInputs {
            door_release_left: true,
            ..CabInputs::default()
        };
        let out = update(&mut ctl, &mut vehicles, &cab, 0.0, 0.05);
        assert!(out.inaugurating);
        run(&mut ctl, &mut vehicles, &CabInputs::default(), 0.0, 4.0);

        press_release_left(&mut ctl, &mut vehicles);
        // The command needs one bus cycle per vehicle: the head has it, the tail not yet.
        assert_eq!(vehicles[0].doors.left.phase, DoorPhase::Released);
        assert_eq!(vehicles[3].doors.left.phase, DoorPhase::Locked);
        run(&mut ctl, &mut vehicles, &CabInputs::default(), 0.0, 5.0);
        assert!(
            vehicles
                .iter()
                .all(|v| v.doors.left.phase == DoorPhase::Open)
        );
    }
}
