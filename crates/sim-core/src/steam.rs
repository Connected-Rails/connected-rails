//! Steam locomotive (plan ch. 8, steam).
//!
//! Three stores that feed each other and nothing else: coal on the grate, water in the
//! boiler, steam above it. Everything a steam driver does is moving something between them
//! — fire the grate, open the damper, put an injector on, crack the regulator, wind the
//! cutoff back — and everything that goes wrong is one of them running out.
//!
//! That is why the model is a mass and energy balance and not a tractive effort curve: a
//! curve cannot run out of steam, and running out of steam is the whole point of the type.
//!
//! ```text
//!   tender coal ─► grate ─(draught)─► heat ─► boiler ─► steam ─► cylinders ─► wheels
//!   tender water ─(injector)─► boiler water                └─(exhaust)─► draught
//! ```
//!
//! The exhaust closing the loop back onto the draught is what makes a steam locomotive
//! self-regulating while it is working and helpless when it is not — hence the blower.

use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

/// Atmospheric pressure [bar] — the boiler gauge reads gauge pressure, the physics needs
/// absolute.
pub const ATMOSPHERIC: f64 = 1.013;

/// Heat of combustion of locomotive coal [J/kg].
pub const COAL_ENERGY: f64 = 29.0e6;

/// Heat needed to turn a kilogram of feed water into steam at working pressure [J/kg] —
/// heating up and evaporating together.
pub const EVAPORATION_HEAT: f64 = 2.6e6;

/// Density of saturated steam per bar of absolute pressure [kg/(m³·bar)].
///
/// ponytail: the steam table is nearly a straight line through the range a locomotive
/// boiler works in (5…20 bar), and this is its slope. Upgrade path is a real table, which
/// only matters for a boiler run far outside that range.
pub const STEAM_DENSITY_PER_BAR: f64 = 0.51;

/// Steam locomotive: boiler, firebox, cylinders and what feeds them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SteamLoco {
    // --- Boiler -----------------------------------------------------------
    /// Water space of the boiler when full [l].
    pub boiler_water: f64,
    /// Steam space above the water [l].
    pub boiler_steam: f64,
    /// Working pressure [bar gauge].
    pub working_pressure: f64,
    /// Pressure at which the safety valves lift [bar gauge].
    pub safety_valve: f64,
    /// Evaporative heating surface [m²] — how much of the fire's heat reaches the water.
    pub heating_surface: f64,
    /// Superheater fitted: the steam leaves the boiler dry and hot, which is worth about a
    /// fifth of the consumption for the same work.
    #[serde(default)]
    pub superheater: bool,

    // --- Firebox ----------------------------------------------------------
    /// Grate area [m²].
    pub grate_area: f64,
    /// Coal the grate holds when fully made up [kg].
    pub grate_capacity: f64,
    /// Coal burnt per square metre of grate and second at full draught [kg/(m²·s)].
    pub burn_rate: f64,
    /// Share of the draught the blower alone can make.
    pub blower_draught: f64,

    // --- Cylinders --------------------------------------------------------
    /// Number of cylinders (double acting).
    pub cylinders: u32,
    /// Cylinder bore [m].
    pub bore: f64,
    /// Piston stroke [m].
    pub stroke: f64,
    /// Driving wheel diameter [m].
    pub wheel_diameter: f64,
    /// Longest cutoff the reverser reaches (0…1).
    pub max_cutoff: f64,
    /// Back pressure in the exhaust [bar absolute].
    pub back_pressure: f64,
    /// Mechanical efficiency of motion and axleboxes.
    pub efficiency: f64,

    // --- Supplies ---------------------------------------------------------
    /// Water an injector puts into the boiler [l/s].
    pub injector_rate: f64,
    /// Water in the tender [l].
    pub tender_water: f64,
    /// Coal in the tender [kg].
    pub tender_coal: f64,
    /// Coal one shovelful moves [kg].
    pub shovel_mass: f64,
}

impl Default for SteamLoco {
    fn default() -> Self {
        // Roughly a DR class 52 — a boiler nobody has to look up to recognise.
        Self {
            boiler_water: 8_800.0,
            boiler_steam: 3_200.0,
            working_pressure: 16.0,
            safety_valve: 16.5,
            heating_surface: 177.0,
            superheater: true,
            grate_area: 3.9,
            grate_capacity: 260.0,
            burn_rate: 0.055,
            blower_draught: 0.35,
            cylinders: 2,
            bore: 0.6,
            stroke: 0.66,
            wheel_diameter: 1.4,
            max_cutoff: 0.75,
            back_pressure: 1.3,
            efficiency: 0.82,
            injector_rate: 3.2,
            tender_water: 30_000.0,
            tender_coal: 10_000.0,
            shovel_mass: 6.0,
        }
    }
}

impl SteamLoco {
    /// Swept volume of one cylinder [m³].
    pub fn swept_volume(&self) -> f64 {
        PI / 4.0 * self.bore * self.bore * self.stroke
    }

    /// Mean effective pressure [Pa] at boiler pressure `p` [bar gauge], regulator `reg`
    /// (0…1) and cutoff `cutoff` (0…1).
    ///
    /// The steam does work twice: pushing while the valve is open, then expanding after it
    /// shuts. `c·(1 + ln(1/c))` is that expansion — which is exactly why winding the cutoff
    /// back costs less effort than it saves steam, and why a driver does it.
    pub fn mean_effective_pressure(&self, p: f64, reg: f64, cutoff: f64) -> f64 {
        let cutoff = cutoff.clamp(0.02, self.max_cutoff.clamp(0.05, 1.0));
        let admission = (p + ATMOSPHERIC) * reg.clamp(0.0, 1.0);
        let expansion = cutoff * (1.0 + (1.0_f64 / cutoff).ln());
        // A superheater keeps the steam dry through the expansion; wet steam condenses on
        // the cylinder walls and loses part of it.
        let quality = if self.superheater { 0.94 } else { 0.85 };
        ((admission * expansion * quality) - self.back_pressure).max(0.0) * 1.0e5
    }

    /// Tractive effort at the rim [N] at boiler pressure `p`, regulator `reg` and cutoff.
    ///
    /// Work per revolution is `n · mep · A · s · 2` (double acting), and one revolution
    /// carries the loco `π·D` — that quotient is the effort, no fudge factor needed.
    pub fn tractive_effort(&self, p: f64, reg: f64, cutoff: f64) -> f64 {
        let area = PI / 4.0 * self.bore * self.bore;
        let work = self.cylinders.max(1) as f64
            * self.mean_effective_pressure(p, reg, cutoff)
            * area
            * self.stroke
            * 2.0;
        work / (PI * self.wheel_diameter.max(0.1)) * self.efficiency.clamp(0.1, 1.0)
    }

    /// Steam the cylinders swallow [kg/s] at speed `v` [m/s].
    pub fn steam_demand(&self, p: f64, reg: f64, cutoff: f64, v: f64) -> f64 {
        let cutoff = cutoff.clamp(0.02, self.max_cutoff.clamp(0.05, 1.0));
        let revs = v.abs() / (PI * self.wheel_diameter.max(0.1));
        let volume = self.swept_volume() * cutoff * 2.0 * self.cylinders.max(1) as f64 * revs;
        let density = (p + ATMOSPHERIC) * STEAM_DENSITY_PER_BAR;
        volume * density * reg.clamp(0.0, 1.0)
    }

    /// Steam the boiler can raise at full fire [kg/s] — the figure a data sheet states as
    /// the evaporation rate, and the ceiling every other number runs into.
    pub fn max_evaporation(&self) -> f64 {
        self.grate_area * self.burn_rate * COAL_ENERGY * self.boiler_efficiency() / EVAPORATION_HEAT
    }

    /// How much of the fire's heat the water actually gets. Grows with the heating surface
    /// per square metre of grate and stops well short of one — the rest goes up the chimney.
    pub fn boiler_efficiency(&self) -> f64 {
        let ratio = self.heating_surface / self.grate_area.max(0.1);
        (0.30 + 0.008 * ratio).clamp(0.3, 0.72)
    }

    /// Steam pressure [bar gauge] a mass of steam `mass` [kg] makes in the steam space.
    fn pressure_of(&self, mass: f64, water: f64) -> f64 {
        // The steam space is whatever the water has left of the boiler.
        let space = ((self.boiler_water + self.boiler_steam - water) / 1000.0).max(0.2);
        (mass / space / STEAM_DENSITY_PER_BAR - ATMOSPHERIC).max(0.0)
    }

    /// Mass of steam [kg] at pressure `p` [bar gauge] with `water` [l] in the boiler.
    fn mass_of(&self, p: f64, water: f64) -> f64 {
        let space = ((self.boiler_water + self.boiler_steam - water) / 1000.0).max(0.2);
        (p + ATMOSPHERIC) * STEAM_DENSITY_PER_BAR * space
    }

    /// Water level as a share of the glass: 0 is the bottom nut, 1 the top.
    pub fn glass(&self, water: f64) -> f64 {
        let full = self.boiler_water.max(1.0);
        ((water / full) - 0.7) / 0.25
    }
}

/// What the fireman and the driver have their hands on.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct SteamControls {
    /// Regulator 0…1.
    pub regulator: f64,
    /// Cutoff 0…1 as a share of `SteamLoco::max_cutoff`; the sign is the reverser.
    pub cutoff: f64,
    /// Blower 0…1.
    pub blower: f64,
    /// Damper 0…1.
    pub damper: f64,
    /// Firehole door 0…1 — open it and cold air kills the fire, which is what makes
    /// firing on the move a skill.
    pub firehole: f64,
    /// Left injector 0…1.
    pub injector_left: f64,
    /// Right injector 0…1.
    pub injector_right: f64,
}

/// Running state of the boiler and the fire.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SteamState {
    /// Boiler pressure [bar gauge].
    pub pressure: f64,
    /// Water in the boiler [l].
    pub water: f64,
    /// Coal on the grate [kg].
    pub fire_mass: f64,
    /// How well the fire is burning, 0…1 — it takes time to come round after firing.
    pub fire_intensity: f64,
    /// Water left in the tender [l].
    pub tender_water: f64,
    /// Coal left in the tender [kg].
    pub tender_coal: f64,
    /// Safety valves blowing off.
    pub blowing_off: bool,
    /// Steam the cylinders are using [kg/s] — the exhaust that makes the draught.
    pub steam_use: f64,
    /// Draught through the fire, 0…1.
    pub draught: f64,
    /// Tractive effort [N].
    pub force: f64,
    /// Boiler priming: the water is so high it is being carried over into the cylinders.
    pub priming: bool,
}

impl SteamState {
    /// A locomotive in steam and ready to go: boiler at working pressure, glass half full,
    /// a made-up fire.
    pub fn new(loco: &SteamLoco) -> Self {
        Self {
            pressure: loco.working_pressure,
            water: loco.boiler_water * 0.82,
            fire_mass: loco.grate_capacity * 0.6,
            fire_intensity: 0.6,
            tender_water: loco.tender_water,
            tender_coal: loco.tender_coal,
            blowing_off: false,
            steam_use: 0.0,
            draught: 0.0,
            force: 0.0,
            priming: false,
        }
    }

    /// A cold locomotive — no pressure, no fire.
    pub fn cold(loco: &SteamLoco) -> Self {
        Self {
            pressure: 0.0,
            fire_mass: 0.0,
            fire_intensity: 0.0,
            ..Self::new(loco)
        }
    }

    /// Water level in the glass, 0…1. Below 0 the crown sheet is uncovered, which is how
    /// boilers are destroyed.
    pub fn glass(&self, loco: &SteamLoco) -> f64 {
        loco.glass(self.water)
    }

    /// Is the crown sheet uncovered? A real one drops its fusible plug; here it is a state
    /// the scoring and the sound can react to.
    pub fn low_water(&self, loco: &SteamLoco) -> bool {
        self.glass(loco) < 0.0
    }
}

/// Puts one shovelful on the grate; returns what actually went on.
pub fn fire(state: &mut SteamState, loco: &SteamLoco, shovels: f64) -> f64 {
    let want = (loco.shovel_mass * shovels.max(0.0)).min(state.tender_coal);
    let room = (loco.grate_capacity - state.fire_mass).max(0.0);
    let put = want.min(room);
    state.fire_mass += put;
    state.tender_coal -= put;
    // Fresh coal smothers the fire until it has caught.
    if loco.grate_capacity > 0.0 {
        state.fire_intensity =
            (state.fire_intensity - put / loco.grate_capacity * 0.8).clamp(0.0, 1.0);
    }
    put
}

/// One simulation step of the boiler, the fire and the cylinders.
///
/// `v` is the road speed [m/s]. Returns the tractive effort [N].
pub fn step(
    loco: &SteamLoco,
    state: &mut SteamState,
    controls: &SteamControls,
    v: f64,
    dt: f64,
) -> f64 {
    // --- Draught ---------------------------------------------------------
    // The exhaust up the chimney pulls the fire; the blower stands in for it while the
    // regulator is shut. That coupling is the whole character of the type.
    let exhaust = (state.steam_use / loco.max_evaporation().max(1e-6)).clamp(0.0, 1.4);
    let blower = controls.blower.clamp(0.0, 1.0) * loco.blower_draught;
    let damper = controls.damper.clamp(0.0, 1.0);
    // An open firehole door lets cold air in over the fire instead of through it.
    let door_loss = 1.0 - 0.5 * controls.firehole.clamp(0.0, 1.0);
    state.draught = ((exhaust + blower) * damper * door_loss).clamp(0.0, 1.4);

    // --- Fire ------------------------------------------------------------
    let has_fire = state.fire_mass > 0.1;
    // The fire comes round towards what the draught can support, in its own time.
    let target = if has_fire {
        (state.draught / 1.0).clamp(0.0, 1.0)
            * (state.fire_mass / loco.grate_capacity.max(1.0))
                .min(1.0)
                .powf(0.4)
    } else {
        0.0
    };
    let rate = if target > state.fire_intensity {
        // Coming round takes about a minute of hard blowing.
        1.0 / 45.0
    } else {
        1.0 / 20.0
    };
    let delta = (target - state.fire_intensity).clamp(-rate * dt, rate * dt);
    state.fire_intensity += delta;

    let burn = loco.grate_area * loco.burn_rate * state.fire_intensity.clamp(0.0, 1.0);
    let burn = burn.min(state.fire_mass / dt.max(1e-6));
    state.fire_mass = (state.fire_mass - burn * dt).max(0.0);
    let heat = burn * COAL_ENERGY * loco.boiler_efficiency();

    // --- Water -----------------------------------------------------------
    let injectors = (controls.injector_left.clamp(0.0, 1.0)
        + controls.injector_right.clamp(0.0, 1.0))
        * loco.injector_rate;
    let feed = injectors.min(state.tender_water / dt.max(1e-6));
    let feed = feed.min((loco.boiler_water - state.water).max(0.0) / dt.max(1e-6));
    state.water += feed * dt;
    state.tender_water = (state.tender_water - feed * dt).max(0.0);

    // --- Steam -----------------------------------------------------------
    let mut mass = loco.mass_of(state.pressure, state.water);
    // Cold feed water takes heat out of the boiler before it can become steam. That is why
    // an injector drops the pressure, and why a fireman puts one on before the safety
    // valves lift rather than after.
    let feed_heat = feed * 4_186.0 * 80.0;
    let evaporated = ((heat - feed_heat) / EVAPORATION_HEAT).max(0.0);
    mass += evaporated * dt;
    state.water = (state.water - evaporated * dt).max(0.0);

    // --- Cylinders -------------------------------------------------------
    let cutoff = controls.cutoff.abs().clamp(0.0, 1.0) * loco.max_cutoff;
    let regulator = controls.regulator.clamp(0.0, 1.0);
    // Priming: with the water over the top of the glass it is carried into the cylinders
    // with the steam — the regulator makes effort but the boiler loses water fast.
    state.priming = loco.glass(state.water) > 1.05 && regulator > 0.3;
    let used = loco.steam_demand(state.pressure, regulator, cutoff, v);
    let used = used.min(mass / dt.max(1e-6));
    mass -= used * dt;
    state.steam_use = used;
    if state.priming {
        // Water going out with the steam, at roughly the steam's own mass again.
        state.water = (state.water - used * dt).max(0.0);
    }

    // --- Safety valves ---------------------------------------------------
    state.pressure = loco.pressure_of(mass, state.water);
    if state.pressure > loco.safety_valve {
        // The valves lift and hold the pressure at their setting; the steam is gone.
        let allowed = loco.mass_of(loco.safety_valve, state.water);
        mass = allowed;
        state.pressure = loco.safety_valve;
        state.blowing_off = true;
    } else if state.pressure < loco.safety_valve - 0.3 {
        state.blowing_off = false;
    }
    let _ = mass;

    // --- Effort ----------------------------------------------------------
    // No steam left is no effort, whatever the regulator says — the boiler is the limit.
    let force = loco.tractive_effort(state.pressure, regulator, cutoff);
    let sign = if controls.cutoff < 0.0 { -1.0 } else { 1.0 };
    state.force = force * sign;
    state.force
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loco() -> SteamLoco {
        SteamLoco::default()
    }

    #[test]
    fn a_class_52_pulls_what_the_data_sheet_says() {
        let loco = loco();
        // 52 at full regulator and long cutoff: around 240 kN starting effort.
        let f = loco.tractive_effort(16.0, 1.0, 0.75);
        assert!((180_000.0..300_000.0).contains(&f), "{f:.0} N");
        // Winding the cutoff back costs effort.
        assert!(loco.tractive_effort(16.0, 1.0, 0.25) < f);
        // …but costs less than it saves in steam, which is why a driver does it.
        let long = loco.steam_demand(16.0, 1.0, 0.75, 20.0);
        let short = loco.steam_demand(16.0, 1.0, 0.25, 20.0);
        let effort_ratio = loco.tractive_effort(16.0, 1.0, 0.25) / f;
        assert!(
            short / long < effort_ratio,
            "{} vs {}",
            short / long,
            effort_ratio
        );
    }

    #[test]
    fn the_evaporation_rate_is_of_the_right_order() {
        // A 52 makes something like 3 kg of steam a second flat out (≈ 11 t/h).
        let e = loco().max_evaporation();
        assert!((1.5..5.0).contains(&e), "{e:.2} kg/s");
    }

    #[test]
    fn working_hard_without_firing_runs_the_boiler_down() {
        let loco = loco();
        let mut state = SteamState::new(&loco);
        let controls = SteamControls {
            regulator: 1.0,
            cutoff: 1.0,
            damper: 1.0,
            ..Default::default()
        };
        let start = state.pressure;
        for _ in 0..(200 * 240) {
            step(&loco, &mut state, &controls, 80.0 / 3.6, 1.0 / 200.0);
        }
        assert!(
            state.pressure < start,
            "{:.1} → {:.1} bar",
            start,
            state.pressure
        );
        assert!(state.fire_mass < loco.grate_capacity * 0.6);
        assert!(state.water < loco.boiler_water * 0.82);
    }

    #[test]
    fn firing_and_a_blower_bring_the_pressure_back() {
        let loco = loco();
        let mut state = SteamState::new(&loco);
        state.pressure = 8.0;
        state.fire_intensity = 0.2;
        let controls = SteamControls {
            blower: 1.0,
            damper: 1.0,
            ..Default::default()
        };
        for i in 0..(200 * 600) {
            if i % (200 * 30) == 0 {
                fire(&mut state, &loco, 4.0);
            }
            step(&loco, &mut state, &controls, 0.0, 1.0 / 200.0);
        }
        assert!(state.pressure > 12.0, "{:.1} bar", state.pressure);
    }

    #[test]
    fn the_safety_valves_hold_the_pressure_at_their_setting() {
        let loco = loco();
        let mut state = SteamState::new(&loco);
        state.fire_mass = loco.grate_capacity;
        state.fire_intensity = 1.0;
        let controls = SteamControls {
            damper: 1.0,
            blower: 1.0,
            ..Default::default()
        };
        for _ in 0..(200 * 600) {
            step(&loco, &mut state, &controls, 0.0, 1.0 / 200.0);
        }
        assert!(state.pressure <= loco.safety_valve + 1e-6);
        assert!(state.blowing_off);
    }

    #[test]
    fn an_injector_fills_the_boiler_and_costs_pressure() {
        let loco = loco();
        let mut state = SteamState::new(&loco);
        state.water = loco.boiler_water * 0.75;
        state.fire_intensity = 0.0;
        state.fire_mass = 0.0;
        let before = (state.water, state.pressure);
        let controls = SteamControls {
            injector_left: 1.0,
            ..Default::default()
        };
        for _ in 0..(200 * 60) {
            step(&loco, &mut state, &controls, 0.0, 1.0 / 200.0);
        }
        assert!(state.water > before.0);
        assert!(state.tender_water < loco.tender_water);
        assert!(state.pressure <= before.1);
    }

    #[test]
    fn a_shut_regulator_without_a_blower_lets_the_fire_die_down() {
        let loco = loco();
        let mut state = SteamState::new(&loco);
        let controls = SteamControls {
            damper: 1.0,
            ..Default::default()
        };
        for _ in 0..(200 * 120) {
            step(&loco, &mut state, &controls, 0.0, 1.0 / 200.0);
        }
        assert!(state.fire_intensity < 0.1, "{}", state.fire_intensity);
    }

    #[test]
    fn the_glass_reads_the_way_round_it_should() {
        let loco = loco();
        assert!(loco.glass(loco.boiler_water * 0.95) > loco.glass(loco.boiler_water * 0.75));
        let mut state = SteamState::new(&loco);
        state.water = loco.boiler_water * 0.6;
        assert!(state.low_water(&loco));
    }
}
