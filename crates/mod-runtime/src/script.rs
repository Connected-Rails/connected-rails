//! Lua host and the behaviour hooks (plan ch. 19).
//!
//! Sandbox: `table`, `string` and `math` only — no `io`, no `os`, no `require`, no
//! filesystem. A script sees a context table of plain numbers and booleans and answers with
//! a table of overrides; it never gets a handle on the simulation itself. That keeps the
//! trust boundary at exactly one place: the value check when the answer is applied.
//!
//! Two hooks exist, because only two things genuinely need behaviour:
//!
//! | file | hook | called for |
//! |---|---|---|
//! | `vehicles/*.ron` → `script` | `update(ctx)` | every train whose leading vehicle names a script |
//! | `signals/*.ron` → `script` | `aspect(ctx)` | every signal of that type, after the rule table |

use crate::Mods;
use mlua::{Function, Lua, LuaOptions, StdLib, Table, Value};
use sim_core::Sim;
use sim_core::interlock::{DistantAspect, MainAspect};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

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
}

impl ModRuntime {
    pub fn load(root: impl AsRef<Path>) -> Self {
        let mods = Mods::load(root);
        let scripts = Scripts::new(&mods.scripts);
        Self { mods, scripts }
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
        self.signal_hooks(sim);
        self.vehicle_hooks(sim, dt);
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
                        .errors
                        .push(format!("{script}: unknown aspect {name:?}")),
                }
            }
            if let Ok(Some(name)) = out.get::<Option<String>>("distant") {
                match distant_aspect(&name) {
                    Some(distant) => signal.aspect.distant = Some(distant),
                    None => self
                        .scripts
                        .errors
                        .push(format!("{script}: unknown aspect {name:?}")),
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
