//! Lua host and the behaviour hooks (plan ch. 19).
//!
//! Sandbox: `table`, `string` and `math` only — no `io`, no `os`, no `require`, no
//! filesystem. A script sees a context table of plain numbers and booleans and answers with
//! a table of overrides; it never gets a handle on the simulation itself. That keeps the
//! trust boundary at exactly one place: the value check when the answer is applied.
//!
//! Four hooks exist, because only these things genuinely need behaviour:
//!
//! | file | hook | called for |
//! |---|---|---|
//! | `vehicles/*.ron` → `script` | `update(ctx)` | every train whose leading vehicle names a script |
//! | `signals/*.ron` → `script` | `aspect(ctx)` | every signal of that type, after the rule table |
//! | `lines/*.ron` → `script` | `on_load(ctx)`, `on_frame(ctx)` | the line that is being driven |
//! | `scenarios/*.ron` → `script` | `on_load(ctx)`, `on_frame(ctx)` | the loaded scenario |
//!
//! Line and scenario hooks decide *when*, not *what*: they fire events of the scenario by
//! name, and the actions of those events stay declarative RON.

use crate::Mods;
use content::route::LineSource;
use mlua::{Function, Lua, LuaOptions, StdLib, Table, Value};
use sim_core::Sim;
use sim_core::interlock::{DistantAspect, MainAspect};
use sim_core::scenario;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use track_model::{NodeId, SwitchPosition};

/// The Lua state with all mod scripts loaded.
pub struct Scripts {
    lua: Lua,
    /// Module table per script, keyed `"<mod>:<file stem>"`.
    modules: BTreeMap<String, Table>,
    /// Scripts that raised an error — skipped from then on, the simulation keeps running
    /// (plan 19.3).
    failed: BTreeSet<String>,
    /// Script errors for the HUD/log.
    pub errors: Vec<String>,
}

impl Scripts {
    /// Loads every script; each one is expected to return its module table.
    pub fn new(sources: &BTreeMap<String, String>) -> Self {
        let lua = Lua::new_with(
            StdLib::TABLE | StdLib::STRING | StdLib::MATH,
            LuaOptions::default(),
        )
        .expect("Lua state with a restricted standard library");
        let mut modules = BTreeMap::new();
        let mut errors = Vec::new();
        for (name, source) in sources {
            match lua.load(source.as_str()).set_name(name.as_str()).eval() {
                Ok(table) => {
                    modules.insert(name.clone(), table);
                }
                Err(e) => errors.push(format!("{name}: {e}")),
            }
        }
        Self {
            lua,
            modules,
            failed: BTreeSet::new(),
            errors,
        }
    }

    /// An empty context table.
    pub fn context(&self) -> Table {
        self.lua.create_table().expect("empty table")
    }

    /// Notes a script error once. Hooks run every frame, so the same complaint would
    /// otherwise fill the log at 60 Hz.
    pub fn error(&mut self, message: String) {
        if !self.errors.contains(&message) {
            self.errors.push(message);
        }
    }

    /// Calls `<script>.<hook>(ctx)`. `None` if the script is unknown, has no such hook,
    /// returned no table, or failed — in the last case it is disabled.
    pub fn call(&mut self, script: &str, hook: &str, ctx: Table) -> Option<Table> {
        if self.failed.contains(script) {
            return None;
        }
        let function: Function = self.modules.get(script)?.get(hook).ok()?;
        match function.call(ctx) {
            Ok(Value::Table(t)) => Some(t),
            Ok(_) => None,
            Err(e) => {
                self.errors.push(format!("{script}.{hook}: {e}"));
                self.failed.insert(script.to_string());
                None
            }
        }
    }
}

/// Loaded mods plus their Lua state — this is what the app holds.
pub struct ModRuntime {
    pub mods: Mods,
    pub scripts: Scripts,
    /// Scripts of the line and the scenario in use, filled by [`ModRuntime::begin`].
    world: Vec<String>,
}

impl ModRuntime {
    pub fn load(root: impl AsRef<Path>) -> Self {
        let mods = Mods::load(root);
        let scripts = Scripts::new(&mods.scripts);
        Self {
            mods,
            scripts,
            world: Vec::new(),
        }
    }

    /// Warnings from loading plus script errors so far.
    pub fn log(&self) -> Vec<String> {
        let mut log = self.mods.warnings.clone();
        log.extend(self.scripts.errors.iter().cloned());
        log
    }

    /// Runs the behaviour hooks. Called once per frame after `Sim::advance`.
    ///
    /// ponytail: per frame, not inside the 200 Hz step — a Lua call per step would be 200
    /// crossings per train per second for behaviour that reacts in tenths of a second.
    /// Move a hook into `Sim::step` if it ever needs to see every step.
    pub fn post_step(&mut self, sim: &mut Sim, dt: f64) {
        self.world_hooks(sim, "on_frame", dt);
        self.signal_hooks(sim);
        self.vehicle_hooks(sim, dt);
    }

    /// Registers the hooks of the line and the scenario in use and calls `on_load` once
    /// (plan 19.7). Call it after the scenario has been set, before the first frame.
    pub fn begin(&mut self, sim: &mut Sim, line: &LineSource) {
        self.world = [line.script.clone(), sim.scenario.scenario.script.clone()]
            .into_iter()
            .flatten()
            .collect();
        self.world_hooks(sim, "on_load", 0.0);
    }

    /// `aspect(ctx)` — may override what the rule table decided.
    fn signal_hooks(&mut self, sim: &mut Sim) {
        for i in 0..sim.interlock.signals.len() {
            let signal = &sim.interlock.signals[i];
            let Some(script) = signal
                .type_index
                .and_then(|t| sim.interlock.types.get(t as usize))
                .and_then(|t| t.script.clone())
            else {
                continue;
            };
            let situation = signal.situation;
            let ctx = self.scripts.context();
            let _ = ctx.set("signal", i as u32);
            let _ = ctx.set("time", sim.time);
            let _ = ctx.set("clear", situation.clear);
            let _ = ctx.set("route", situation.route);
            let _ = ctx.set("diverging", situation.diverging);
            let _ = ctx.set("next_stop", situation.next_stop);
            let _ = ctx.set("next_slow", situation.next_slow);
            // What the rule table produced — the script usually only patches it.
            let _ = ctx.set("main", main_name(signal.aspect.main));
            let _ = ctx.set("distant", distant_name(signal.aspect.distant));
            let _ = ctx.set("speed", signal.aspect.speed);

            let Some(out) = self.scripts.call(&script, "aspect", ctx) else {
                continue;
            };
            let signal = &mut sim.interlock.signals[i];
            if let Ok(Some(name)) = out.get::<Option<String>>("main") {
                match main_aspect(&name) {
                    Some(main) => signal.aspect.main = Some(main),
                    None => self
                        .scripts
                        .error(format!("{script}: unknown aspect {name:?}")),
                }
            }
            if let Ok(Some(name)) = out.get::<Option<String>>("distant") {
                match distant_aspect(&name) {
                    Some(distant) => signal.aspect.distant = Some(distant),
                    None => self
                        .scripts
                        .error(format!("{script}: unknown aspect {name:?}")),
                }
            }
            if let Ok(speed) = out.get::<Option<f64>>("speed") {
                signal.aspect.speed = speed.filter(|s| s.is_finite() && *s >= 0.0);
            }
            if let Ok(Some(lamps)) = out.get::<Option<Vec<String>>>("lamps") {
                signal.lamps = lamps;
            }
        }
    }

    /// `update(ctx)` — writes cab controls, e.g. AFB or tap changer logic.
    fn vehicle_hooks(&mut self, sim: &mut Sim, dt: f64) {
        for train in 0..sim.trains.len() {
            // The script of the leading vehicle operates the desk — that is the cab in use.
            let Some(script) = sim.trains[train]
                .vehicles
                .iter()
                .find_map(|v| v.spec.script.clone())
            else {
                continue;
            };
            let cab = sim.controls[train];
            let head = &sim.trains[train].vehicles[0];
            let ctx = self.scripts.context();
            let _ = ctx.set("train", train as u32);
            let _ = ctx.set("dt", dt);
            let _ = ctx.set("time", sim.time);
            let _ = ctx.set("v_kmh", sim.trains[train].speed_kmh());
            let _ = ctx.set("speed_limit_kmh", head.pos.speed_limit(&sim.net));
            let _ = ctx.set("mass_t", sim.trains[train].mass() / 1000.0);
            let _ = ctx.set("throttle", cab.throttle);
            let _ = ctx.set("reverser", cab.reverser);
            let _ = ctx.set("direct_brake", cab.direct_brake);
            let _ = ctx.set("sanding", cab.sanding);
            let _ = ctx.set("afb", cab.afb);
            let _ = ctx.set("afb_target", cab.afb_target);
            let _ = ctx.set("brake_pipe", head.brake.pipe);
            let _ = ctx.set("notch", head.traction.notch);
            let _ = ctx.set("line_voltage", head.traction.line_voltage);
            let _ = ctx.set("tractive_effort", head.tractive_effort);

            let Some(out) = self.scripts.call(&script, "update", ctx) else {
                continue;
            };
            // Trust boundary: a script may return anything, including NaN.
            let cab = &mut sim.controls[train];
            if let Ok(Some(v)) = out.get::<Option<f64>>("throttle")
                && v.is_finite()
            {
                cab.throttle = v.clamp(-1.0, 1.0);
            }
            if let Ok(Some(v)) = out.get::<Option<f64>>("direct_brake")
                && v.is_finite()
            {
                cab.direct_brake = v.clamp(0.0, 1.0);
            }
            if let Ok(Some(v)) = out.get::<Option<bool>>("sanding") {
                cab.sanding = v;
            }
        }
    }

    /// `display(ctx)` — draw list of one cab screen (plan ch. 12). The script
    /// of the leading vehicle answers for every display of the train; `None`
    /// (no script, no hook, no table) leaves the screen to its widget list.
    ///
    /// `ctx` carries the display's name and size, the eight softkeys
    /// ([`sim_core::cab::CabControl::Display`]) as `buttons[1..8]`, the same
    /// driving values as `update(ctx)`, and two tables `lamp`/`value` with
    /// every indicator of the train protection — everything the MFA shows.
    pub fn vehicle_display(
        &mut self,
        sim: &Sim,
        train: usize,
        display: &sim_core::cab::DisplaySpec,
    ) -> Option<Vec<crate::display::DrawCmd>> {
        let script = sim
            .trains
            .get(train)?
            .vehicles
            .iter()
            .find_map(|v| v.spec.script.clone())?;
        let cab = sim.controls.get(train)?;
        let head = &sim.trains[train].vehicles[0];
        let ctx = self.scripts.context();
        let _ = ctx.set("display", display.name.as_str());
        let _ = ctx.set("width", display.width);
        let _ = ctx.set("height", display.height);
        let _ = ctx.set("time", sim.time);
        let _ = ctx.set("v_kmh", sim.trains[train].speed_kmh());
        let _ = ctx.set("speed_limit_kmh", head.pos.speed_limit(&sim.net));
        let _ = ctx.set("throttle", cab.throttle);
        let _ = ctx.set("reverser", cab.reverser);
        let _ = ctx.set("afb", cab.afb);
        let _ = ctx.set("afb_target", cab.afb_target);
        let _ = ctx.set("brake_pipe", head.brake.pipe);
        let _ = ctx.set("brake_cylinder", head.brake.cylinder);
        let _ = ctx.set("main_reservoir", head.brake.main_reservoir);
        let _ = ctx.set("line_voltage", head.traction.line_voltage);
        let _ = ctx.set("tractive_effort", head.tractive_effort);
        let buttons = self.scripts.context();
        for (i, pressed) in cab.display_buttons.iter().enumerate() {
            let _ = buttons.set(i + 1, *pressed);
        }
        let _ = ctx.set("buttons", buttons);
        // Indicators of the train protection: lamps as booleans, displays as
        // numbers — `value.mfa_v_soll`, `lamp.pzb_1000hz`, …
        let lamp = self.scripts.context();
        let value = self.scripts.context();
        for indicator in head.safety.indicators() {
            match indicator.value {
                Some(v) => {
                    let _ = value.set(indicator.name, v);
                }
                None => {
                    let _ = lamp.set(
                        indicator.name,
                        indicator.lamp != sim_core::safety::LampState::Off,
                    );
                }
            }
        }
        let _ = ctx.set("lamp", lamp);
        let _ = ctx.set("value", value);

        let out = self.scripts.call(&script, "display", ctx)?;
        let (commands, complaint) = crate::display::parse_draw_list(&out);
        if let Some(complaint) = complaint {
            self.scripts.error(format!("{script}.display: {complaint}"));
        }
        Some(commands)
    }

    /// `on_load(ctx)` / `on_frame(ctx)` of the line and the scenario (plan 19.7).
    ///
    /// The script decides *when*, the RON says *what*: `fire` names events of the scenario
    /// and their declarative actions run. `message` and `switch` are there for a line that
    /// carries behaviour of its own without a scenario.
    fn world_hooks(&mut self, sim: &mut Sim, hook: &str, dt: f64) {
        for i in 0..self.world.len() {
            let script = self.world[i].clone();
            let player = sim.scenario.scenario.player_train;
            let head = sim
                .trains
                .get(player)
                .and_then(|t| t.vehicles.first())
                .map(|v| v.pos);
            let ctx = self.scripts.context();
            let _ = ctx.set("dt", dt);
            let _ = ctx.set("time", sim.time);
            let _ = ctx.set("trains", sim.trains.len() as u32);
            let _ = ctx.set("player", player as u32);
            let _ = ctx.set("finished", sim.scenario.is_finished());
            let _ = ctx.set("bonus", sim.scenario.bonus);
            if let Some(train) = sim.trains.get(player) {
                let _ = ctx.set("v_kmh", train.speed_kmh());
            }
            if let Some(pos) = head {
                let _ = ctx.set("edge", pos.edge.0);
                let _ = ctx.set("s", pos.s);
            }
            // Which events have already fired, so a script can chain on them.
            let fired = self.scripts.context();
            for event in &sim.scenario.scenario.events {
                if let Some(t) = sim.scenario.fired_at(&event.name) {
                    let _ = fired.set(event.name.as_str(), t);
                }
            }
            let _ = ctx.set("fired", fired);

            let Some(out) = self.scripts.call(&script, hook, ctx) else {
                continue;
            };
            if let Ok(Some(text)) = out.get::<Option<String>>("message") {
                let announcement = out
                    .get::<Option<bool>>("announcement")
                    .ok()
                    .flatten()
                    .unwrap_or(false);
                sim.scenario.message(sim.time, text, announcement);
            }
            if let Ok(Some(names)) = out.get::<Option<Vec<String>>>("fire") {
                for name in names {
                    if sim.scenario.has_event(&name) {
                        scenario::fire(sim, &name);
                    } else {
                        self.scripts
                            .error(format!("{script}: unknown event {name:?}"));
                    }
                }
            }
            if let Ok(Some(table)) = out.get::<Option<Table>>("switch") {
                self.set_switch(sim, &script, &table);
            }
        }
    }

    /// `switch = { node = 3, position = "diverging" }` — the one thing a line script can do
    /// without a scenario to fire events in.
    fn set_switch(&mut self, sim: &mut Sim, script: &str, table: &Table) {
        let Ok(node) = table.get::<u32>("node") else {
            return;
        };
        let position = match table.get::<String>("position").as_deref() {
            Ok("straight") => SwitchPosition::Straight,
            Ok("diverging") => SwitchPosition::Diverging,
            Ok(other) => {
                self.scripts
                    .error(format!("{script}: unknown switch position {other:?}"));
                return;
            }
            Err(_) => return,
        };
        match sim.net.switch_mut(NodeId(node)) {
            Some(switch) => {
                let _ = switch.command(position);
            }
            None => self
                .scripts
                .error(format!("{script}: no switch at node {node}")),
        }
    }
}

fn main_name(aspect: Option<MainAspect>) -> Option<&'static str> {
    Some(match aspect? {
        MainAspect::Stop => "stop",
        MainAspect::Proceed => "proceed",
        MainAspect::ProceedSlow => "proceed_slow",
        MainAspect::Substitute => "substitute",
        MainAspect::DarkLight => "dark",
    })
}

fn main_aspect(name: &str) -> Option<MainAspect> {
    Some(match name {
        "stop" => MainAspect::Stop,
        "proceed" => MainAspect::Proceed,
        "proceed_slow" => MainAspect::ProceedSlow,
        "substitute" => MainAspect::Substitute,
        "dark" => MainAspect::DarkLight,
        _ => return None,
    })
}

fn distant_name(aspect: Option<DistantAspect>) -> Option<&'static str> {
    Some(match aspect? {
        DistantAspect::ExpectStop => "expect_stop",
        DistantAspect::ExpectProceed => "expect_proceed",
        DistantAspect::ExpectSlow => "expect_slow",
    })
}

fn distant_aspect(name: &str) -> Option<DistantAspect> {
    Some(match name {
        "expect_stop" => DistantAspect::ExpectStop,
        "expect_proceed" => DistantAspect::ExpectProceed,
        "expect_slow" => DistantAspect::ExpectSlow,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime() -> ModRuntime {
        ModRuntime::load(concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods"))
    }

    /// A runtime whose only world script is the given source.
    fn world_runtime(source: &str) -> ModRuntime {
        ModRuntime {
            mods: Mods::default(),
            scripts: Scripts::new(&BTreeMap::from([(
                "test:world".to_string(),
                source.to_string(),
            )])),
            world: vec!["test:world".to_string()],
        }
    }

    /// The example line with a scenario of one script-only event worth 7 points.
    fn world_sim() -> Sim {
        use sim_core::scenario::{Action, Event, Scenario, Trigger};

        let compiled = content::musterbahn().compile().expect("line compiles");
        let mut sim = Sim::new(compiled.net, compiled.interlock, 1);
        sim.set_scenario(
            Scenario {
                name: "test".into(),
                events: vec![Event {
                    name: "boom".into(),
                    trigger: Trigger::Never,
                    actions: vec![Action::Score {
                        points: 7,
                        reason: "script".into(),
                    }],
                    once: true,
                    module: None,
                }],
                ..Default::default()
            },
            sim_core::timetable::Timetable::default(),
        );
        sim
    }

    fn afb(scripts: &mut Scripts, v_kmh: f64, target: f64) -> Option<f64> {
        let ctx = scripts.context();
        for (k, v) in [
            ("v_kmh", v_kmh),
            ("afb_target", target),
            ("speed_limit_kmh", 160.0),
        ] {
            ctx.set(k, v).unwrap();
        }
        ctx.set("afb", true).unwrap();
        ctx.set("reverser", 1).unwrap();
        let out = scripts.call("example:afb", "update", ctx)?;
        out.get::<Option<f64>>("throttle").ok().flatten()
    }

    #[test]
    fn example_scripts_load() {
        let rt = runtime();
        assert!(rt.log().is_empty(), "log: {:?}", rt.log());
    }

    #[test]
    fn afb_accelerates_below_and_brakes_above_the_target() {
        let mut rt = runtime();
        assert!(
            afb(&mut rt.scripts, 60.0, 120.0).unwrap() > 0.5,
            "should pull"
        );
        assert!(
            afb(&mut rt.scripts, 130.0, 120.0).unwrap() < 0.0,
            "should brake"
        );
        // The line speed caps the target speed.
        assert!(
            afb(&mut rt.scripts, 170.0, 200.0).unwrap() < 0.0,
            "line speed wins"
        );
    }

    #[test]
    fn afb_stays_out_of_the_way_when_switched_off() {
        let mut rt = runtime();
        let ctx = rt.scripts.context();
        ctx.set("afb", false).unwrap();
        assert!(rt.scripts.call("example:afb", "update", ctx).is_none());
    }

    #[test]
    fn a_broken_script_is_disabled_instead_of_crashing() {
        let mut scripts = Scripts::new(&BTreeMap::from([(
            "test:boom".to_string(),
            "return { update = function(ctx) error('boom') end }".to_string(),
        )]));
        let ctx = scripts.context();
        assert!(scripts.call("test:boom", "update", ctx).is_none());
        assert_eq!(scripts.errors.len(), 1);
        // Second call: no further error, the script is out.
        let ctx = scripts.context();
        assert!(scripts.call("test:boom", "update", ctx).is_none());
        assert_eq!(scripts.errors.len(), 1);
    }

    /// `Trigger::Never` waits for the script; the script decides the moment, the RON keeps
    /// the actions.
    #[test]
    fn a_scenario_script_fires_an_event_by_name() {
        let mut rt = world_runtime(
            "return { on_frame = function(ctx) \
                 if ctx.time >= 0.0 then return { fire = { 'boom' } } end \
             end }",
        );
        let mut sim = world_sim();

        // On its own the event never comes — that is what `Never` means.
        sim.advance(1.0);
        assert_eq!(sim.scenario.bonus, 0);

        rt.post_step(&mut sim, 0.1);
        assert_eq!(sim.scenario.bonus, 7);
        assert!(sim.scenario.fired_at("boom").is_some());
        // `once`: a second call changes nothing.
        rt.post_step(&mut sim, 0.1);
        assert_eq!(sim.scenario.bonus, 7);
        assert!(rt.log().is_empty(), "log: {:?}", rt.log());
    }

    #[test]
    fn on_load_runs_once_and_can_speak() {
        let mut rt = world_runtime(
            "return { on_load = function(ctx) return { message = 'ready', announcement = true } end }",
        );
        let mut sim = world_sim();
        sim.scenario.scenario.script = Some("test:world".into());
        let line = content::musterbahn();
        rt.begin(&mut sim, &line);
        assert_eq!(sim.scenario.messages.len(), 1);
        assert_eq!(sim.scenario.messages[0].text, "ready");
        assert!(sim.scenario.messages[0].announcement);

        // The script has no `on_frame`, so the per-frame call is a no-op — a script may
        // implement either hook or both.
        rt.post_step(&mut sim, 0.1);
        assert_eq!(sim.scenario.messages.len(), 1);
    }

    /// A typo in an event name is reported — but once, not on every frame.
    #[test]
    fn an_unknown_event_is_reported_a_single_time() {
        let mut rt =
            world_runtime("return { on_frame = function(ctx) return { fire = { 'nope' } } end }");
        let mut sim = world_sim();
        rt.post_step(&mut sim, 0.1);
        rt.post_step(&mut sim, 0.1);
        assert_eq!(rt.scripts.errors.len(), 1, "{:?}", rt.scripts.errors);
        assert!(rt.scripts.errors[0].contains("nope"));
    }

    #[test]
    fn a_switch_command_out_of_a_script_is_checked() {
        let mut sim = world_sim();
        let mut rt = world_runtime(
            "return { on_frame = function(ctx) \
                 return { switch = { node = 1, position = 'sideways' } } \
             end }",
        );
        rt.post_step(&mut sim, 0.1);
        assert!(
            rt.scripts.errors[0].contains("sideways"),
            "{:?}",
            rt.scripts.errors
        );

        // Node 1 of the example line is a joint, not a switch.
        let mut rt = world_runtime(
            "return { on_frame = function(ctx) \
                 return { switch = { node = 1, position = 'diverging' } } \
             end }",
        );
        rt.post_step(&mut sim, 0.1);
        assert!(
            rt.scripts.errors[0].contains("no switch"),
            "{:?}",
            rt.scripts.errors
        );
    }

    /// The scenario of the example mod with its hook — the whole chain from RON to Lua.
    #[test]
    fn the_example_scenario_hook_is_wired_up() {
        let mut rt = runtime();
        let line = rt.mods.lines["example:beispielstrecke"].clone();
        let scenario = rt.mods.scenarios["example:probefahrt"].clone();
        assert_eq!(scenario.script.as_deref(), Some("example:probefahrt"));

        let compiled = line.compile().expect("line compiles");
        let mut sim = Sim::new(compiled.net, compiled.interlock, 1);
        sim.set_scenario(scenario, sim_core::timetable::Timetable::default());
        rt.begin(&mut sim, &line);

        assert!(
            sim.scenario.messages[0].text.contains("Test run loaded"),
            "{:?}",
            sim.scenario.messages
        );
        rt.post_step(&mut sim, 0.1);
        assert!(rt.log().is_empty(), "log: {:?}", rt.log());
    }

    /// The display hook: draw list parsed, junk skipped, caps enforced.
    #[test]
    fn a_display_script_returns_a_draw_list() {
        use crate::display::{DrawCmd, TextAlign};

        let mut scripts = Scripts::new(&BTreeMap::from([(
            "test:screen".to_string(),
            "return { display = function(ctx) return { \
                 { kind = 'clear', color = {0, 0, 0} }, \
                 { kind = 'text', x = 10, y = 4, text = 'ZE ' .. ctx.width, size = 20, align = 'center' }, \
                 { kind = 'rect', x = 0/0, y = 1, w = 2, h = 3 }, \
                 { kind = 'blob' }, \
             } end }"
                .to_string(),
        )]));
        let ctx = scripts.context();
        ctx.set("width", 256).unwrap();
        let out = scripts.call("test:screen", "display", ctx).unwrap();
        let (commands, complaint) = crate::display::parse_draw_list(&out);
        assert_eq!(
            commands,
            vec![
                DrawCmd::Clear {
                    color: [0.0, 0.0, 0.0, 1.0]
                },
                DrawCmd::Text {
                    x: 10.0,
                    y: 4.0,
                    text: "ZE 256".into(),
                    size: 20.0,
                    color: [1.0; 4],
                    align: TextAlign::Center
                },
            ],
            "NaN rect and unknown kind are dropped"
        );
        assert!(complaint.unwrap().contains("blob"));
    }

    #[test]
    fn the_sandbox_has_no_filesystem() {
        let scripts = Scripts::new(&BTreeMap::new());
        assert!(scripts.lua.globals().get::<Value>("io").unwrap().is_nil());
        assert!(scripts.lua.globals().get::<Value>("os").unwrap().is_nil());
        assert!(
            scripts
                .lua
                .globals()
                .get::<Value>("require")
                .unwrap()
                .is_nil()
        );
    }
}
