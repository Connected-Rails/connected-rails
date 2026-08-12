//! KI-Triebfahrzeugführer und Fahrplan (Plan Kap. 11).
//!
//! Die KI fährt dieselbe Fahrzeugsimulation wie der Spieler — kein Cheat: sie stellt
//! nur [`CabInputs`], sonst nichts.

pub mod lookahead;
pub mod timetable;

use lookahead::Lookahead;
use serde::{Deserialize, Serialize};
use sim_core::Sim;
use sim_core::brakes::DriverBrakeValve;
use sim_core::cab::CabInputs;
pub use timetable::{ScheduledStop, Timetable};

/// Fahrverhalten des KI-Fahrers.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DrivingStyle {
    /// Geplante Bremsverzögerung [m/s²].
    pub decel: f64,
    /// Sicherheitsabstand vor Zielpunkten [m].
    pub margin: f64,
    /// Zielgeschwindigkeit unterhalb der zulässigen [km/h].
    pub speed_reserve: f64,
    /// Vorausschauweite [m].
    pub lookahead: f64,
    /// Ansprechzeit der Bremse plus Reaktionszeit [s] — der Weg, den der Zug noch
    /// ungebremst zurücklegt, bevor die Verzögerung wirkt.
    pub reaction_time: f64,
}

impl Default for DrivingStyle {
    fn default() -> Self {
        Self {
            decel: 0.5,
            margin: 30.0,
            speed_reserve: 2.0,
            lookahead: 4_000.0,
            reaction_time: 5.0,
        }
    }
}

/// Was der Fahrer gerade tut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DriverState {
    /// Fährt nach Fahrplan.
    #[default]
    Driving,
    /// Hält am Bahnsteig und wartet auf die Abfahrtszeit.
    Dwelling,
    /// Steht vor Halt zeigendem Signal.
    WaitingAtSignal,
    /// Fahrt beendet (Endbahnhof erreicht).
    Finished,
}

/// Ein KI-Triebfahrzeugführer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiDriver {
    pub timetable: Timetable,
    pub style: DrivingStyle,
    pub state: DriverState,
    /// Index des nächsten Halts im Fahrplan.
    pub next_stop: usize,
    /// Sifa-Pedal-Rhythmus [s].
    sifa_timer: f64,
    sifa_pressed: bool,
    /// Restzeit der PZB-Wachsamkeitsbedienung [s].
    wachsam_timer: f64,
    /// Zeitpunkt, ab dem wieder abgefahren werden darf [s].
    depart_at: f64,
}

impl AiDriver {
    pub fn new(timetable: Timetable) -> Self {
        Self {
            timetable,
            style: DrivingStyle::default(),
            state: DriverState::Driving,
            next_stop: 0,
            sifa_timer: 0.0,
            sifa_pressed: false,
            wachsam_timer: 0.0,
            depart_at: 0.0,
        }
    }

    /// Ein Fahrerschritt: liest die Simulation und schreibt die Führerstandseingaben.
    pub fn drive(&mut self, sim: &mut Sim, train: usize, dt: f64) {
        let head = sim.trains[train].vehicles[0].pos;
        let v_kmh = sim.trains[train].speed_kmh();
        let view = lookahead::scan(&sim.net, &sim.interlock, head, self.style.lookahead);

        let mut target = self.target_speed(sim, &view, head, v_kmh);

        // Zugsicherung: Überwachungsgeschwindigkeit nie überschreiten.
        if let Some(limit) = sim.runtime[train].protection.speed_limit {
            target = target.min(limit - 1.0);
        }
        target = target.max(0.0);

        let cab = &mut sim.controls[train];
        cab.reverser = 1;
        Self::apply_speed_control(cab, v_kmh, target);

        // Sifa im 20-s-Rhythmus bedienen (kurzer Pedaldruck).
        self.sifa_timer += dt;
        if self.sifa_timer > 20.0 {
            self.sifa_pressed = true;
            if self.sifa_timer > 20.5 {
                self.sifa_pressed = false;
                self.sifa_timer = 0.0;
            }
        }
        cab.sifa = self.sifa_pressed;

        // PZB-Wachsamkeit quittieren, sobald die Zugsicherung sie verlangt.
        if sim.runtime[train].protection.alert {
            self.wachsam_timer = 0.6;
        }
        if self.wachsam_timer > 0.0 {
            self.wachsam_timer -= dt;
            cab.pzb_wachsam = self.wachsam_timer > 0.3;
        } else {
            cab.pzb_wachsam = false;
        }

        // LZB-Übernahme immer bestätigen.
        cab.lzb_uebernahme = sim.runtime[train].protection.target_distance.is_some();
        self.update_state(sim, train, v_kmh, &view);
    }

    /// Sollgeschwindigkeit aus Streckenprofil, Signalen und Fahrplanhalt.
    fn target_speed(
        &self,
        sim: &Sim,
        view: &Lookahead,
        head: track_model::TrackPosition,
        v_kmh: f64,
    ) -> f64 {
        // Vorhalteweg: fester Abstand + der Weg während Reaktions- und Ansprechzeit.
        let margin = self.style.margin + v_kmh / 3.6 * self.style.reaction_time;
        let mut target = view.permitted(self.style.decel, margin) - self.style.speed_reserve;

        // Fahrplanhalt: Bremskurve auf den Haltepunkt.
        if let Some(stop) = self.timetable.stops.get(self.next_stop)
            && let Some(d) = stop.distance_from(&sim.net, head, self.style.lookahead)
        {
            let curve = (2.0 * self.style.decel * (d - margin).max(0.0)).sqrt() * 3.6;
            target = target.min(curve);
        }
        if self.state == DriverState::Dwelling && sim.time < self.depart_at {
            target = 0.0;
        }
        target
    }

    /// Einfacher Geschwindigkeitsregler auf Fahrschalter und Führerbremsventil.
    fn apply_speed_control(cab: &mut CabInputs, v_kmh: f64, target: f64) {
        let err = target - v_kmh;
        if err > 2.0 {
            cab.throttle = (err / 15.0).clamp(0.1, 1.0);
            cab.brake_valve = DriverBrakeValve::Release;
        } else if err < -1.0 {
            cab.throttle = 0.0;
            // Je größer die Überschreitung, desto stärker die Betriebsbremsung.
            let drop = (-err / 20.0).clamp(0.4, 1.5);
            cab.brake_valve = DriverBrakeValve::Service(drop);
        } else {
            cab.throttle = 0.0;
            cab.brake_valve = if v_kmh < 0.5 {
                DriverBrakeValve::Service(0.8)
            } else {
                DriverBrakeValve::Lap
            };
        }
    }

    fn update_state(&mut self, sim: &Sim, train: usize, v_kmh: f64, view: &Lookahead) {
        match self.state {
            DriverState::Driving => {
                if v_kmh < 0.5 {
                    if let Some(stop) = self.timetable.stops.get(self.next_stop) {
                        let head = sim.trains[train].vehicles[0].pos;
                        if stop
                            .distance_from(&sim.net, head, 200.0)
                            .is_some_and(|d| d < 50.0)
                        {
                            self.state = DriverState::Dwelling;
                            self.depart_at = stop.departure.max(sim.time + 20.0);
                            return;
                        }
                    }
                    if view.distance_to_stop().is_some_and(|d| d < 100.0) {
                        self.state = DriverState::WaitingAtSignal;
                    }
                }
            }
            DriverState::Dwelling => {
                if sim.time >= self.depart_at {
                    self.next_stop += 1;
                    self.state = if self.next_stop >= self.timetable.stops.len() {
                        DriverState::Finished
                    } else {
                        DriverState::Driving
                    };
                }
            }
            DriverState::WaitingAtSignal => {
                if view.distance_to_stop().is_none_or(|d| d > 150.0) {
                    self.state = DriverState::Driving;
                }
            }
            DriverState::Finished => {}
        }
    }
}
