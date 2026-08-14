//! boa 0.20 interop: one `Context` per gauge, the script API from the crate
//! doc, and the tick loop that decides when to repaint.
//!
//! State sharing between native functions and the [`Runtime`] uses a
//! thread-local registry keyed by a `u64` token: the runtime owns the token
//! and removes its entry on drop; native closures capture only the `Copy`
//! token (plus a node index for element objects), which keeps them inside
//! `NativeFunction::from_copy_closure`'s safe API — nothing traced by the
//! GC is captured. Borrows of the registry are always short and never held
//! across a call into the engine, so re-entrant natives cannot collide.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use boa_engine::object::builtins::JsFunction;
use boa_engine::object::{FunctionObjectBuilder, ObjectInitializer};
use boa_engine::property::Attribute;
use boa_engine::{
    Context, JsArgs, JsNativeError, JsObject, JsResult, JsString, JsValue, NativeFunction, Source,
    js_string,
};

use crate::dom::{self, Document};
use crate::style::{self, ComputedStyle, Stylesheet};
use crate::{PaintCmd, SimFrame, layout, paint};

/// A registered `onFrame`/`onButton` callback. A handler that throws is
/// disabled for good; the error is reported once through `take_errors`.
struct Handler {
    func: JsFunction,
    disabled: bool,
}

#[derive(Clone, Copy)]
enum HandlerKind {
    Frame,
    Button,
}

impl HandlerKind {
    fn label(self) -> &'static str {
        match self {
            Self::Frame => "onFrame",
            Self::Button => "onButton",
        }
    }
}

/// Everything the native functions need to reach: the mutable DOM, the
/// parsed stylesheet, callback lists, the element object cache and the held
/// state of the softkeys for `sim.button(n)`.
struct GaugeState {
    doc: Document,
    sheet: Stylesheet,
    on_frame: Vec<Handler>,
    on_button: Vec<Handler>,
    elements: HashMap<usize, JsObject>,
    buttons: [bool; 8],
}

impl GaugeState {
    fn handlers_mut(&mut self, kind: HandlerKind) -> &mut Vec<Handler> {
        match kind {
            HandlerKind::Frame => &mut self.on_frame,
            HandlerKind::Button => &mut self.on_button,
        }
    }
}

thread_local! {
    static STATES: RefCell<HashMap<u64, GaugeState>> = RefCell::new(HashMap::new());
}

static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);

/// Short, non-reentrant access to one gauge's state. Returns `None` if the
/// gauge is gone (cannot happen while its `Runtime` is alive).
fn with_state<R>(token: u64, f: impl FnOnce(&mut GaugeState) -> R) -> Option<R> {
    STATES.with(|s| s.borrow_mut().get_mut(&token).map(f))
}

fn remove_state(token: u64) {
    // `try_with` so a drop during thread teardown stays silent.
    let _ = STATES.try_with(|s| s.borrow_mut().remove(&token));
}

pub struct Runtime {
    token: u64,
    context: Context,
    width: f32,
    height: f32,
    /// The global `sim` object, kept for the per-tick property updates.
    sim: JsObject,
    /// Interned property keys so a steady tick allocates no new strings.
    number_keys: HashMap<String, JsString>,
    lamp_keys: HashMap<String, JsString>,
    prev_buttons: [bool; 8],
    errors: Vec<String>,
    styles_scratch: Vec<ComputedStyle>,
}

impl Runtime {
    pub fn new(source: &str, width: f32, height: f32) -> Result<Self, String> {
        let doc = dom::parse(source)?;
        let sheet = style::parse_stylesheet(&doc.css);
        let script = doc.script.clone();
        let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
        STATES.with(|s| {
            s.borrow_mut().insert(token, GaugeState {
                doc,
                sheet,
                on_frame: Vec::new(),
                on_button: Vec::new(),
                elements: HashMap::new(),
                buttons: [false; 8],
            })
        });
        let mut context = Context::default();
        let sim = match install_globals(&mut context, token) {
            Ok(sim) => sim,
            Err(e) => {
                remove_state(token);
                return Err(format!("script setup failed: {e}"));
            }
        };
        if let Some(src) = script
            && let Err(e) = context.eval(Source::from_bytes(src.as_bytes()))
        {
            remove_state(token);
            return Err(format!("script error: {e}"));
        }
        Ok(Self {
            token,
            context,
            width,
            height,
            sim,
            number_keys: HashMap::new(),
            lamp_keys: HashMap::new(),
            prev_buttons: [false; 8],
            errors: Vec::new(),
            styles_scratch: Vec::new(),
        })
    }

    pub fn tick(&mut self, frame: &SimFrame) -> Option<Vec<PaintCmd>> {
        let _ = with_state(self.token, |st| st.buttons = frame.buttons);
        self.update_sim(frame);
        let _ = with_state(self.token, |st| dom::apply_bindings(&mut st.doc, frame));
        let prev = self.prev_buttons;
        self.prev_buttons = frame.buttons;
        for (i, (&now, &before)) in frame.buttons.iter().zip(prev.iter()).enumerate() {
            if now != before {
                let args = [JsValue::from(i as i32 + 1), JsValue::from(now)];
                self.fire(HandlerKind::Button, &args);
            }
        }
        self.fire(HandlerKind::Frame, &[]);
        self.render()
    }

    pub fn take_errors(&mut self) -> Vec<String> {
        std::mem::take(&mut self.errors)
    }

    /// Mirrors the frame into the global `sim` object: `time`, every number
    /// under its flat name (dotted names are plain property keys), lamps as
    /// booleans under `lamp.<name>`.
    fn update_sim(&mut self, frame: &SimFrame) {
        let key = self.number_key("time");
        self.set_sim(key, JsValue::from(frame.time));
        for (name, value) in &frame.numbers {
            let key = self.number_key(name);
            self.set_sim(key, JsValue::from(*value));
        }
        for (name, lit) in &frame.lamps {
            let key = self.lamp_key(name);
            self.set_sim(key, JsValue::from(*lit));
        }
    }

    fn set_sim(&mut self, key: JsString, value: JsValue) {
        if let Err(e) = self.sim.set(key, value, false, &mut self.context) {
            self.errors.push(format!("sim update failed: {e}"));
        }
    }

    fn number_key(&mut self, name: &str) -> JsString {
        if let Some(k) = self.number_keys.get(name) {
            return k.clone();
        }
        let k = JsString::from(name);
        self.number_keys.insert(name.to_owned(), k.clone());
        k
    }

    fn lamp_key(&mut self, name: &str) -> JsString {
        if let Some(k) = self.lamp_keys.get(name) {
            return k.clone();
        }
        let k = JsString::from(format!("lamp.{name}").as_str());
        self.lamp_keys.insert(name.to_owned(), k.clone());
        k
    }

    /// Calls every enabled handler of one kind. The list is only ever
    /// appended to, so a snapshot of the count keeps handlers registered
    /// mid-callback out of this tick.
    fn fire(&mut self, kind: HandlerKind, args: &[JsValue]) {
        let count = with_state(self.token, |st| st.handlers_mut(kind).len()).unwrap_or(0);
        for i in 0..count {
            let func = with_state(self.token, |st| {
                let h = &st.handlers_mut(kind)[i];
                (!h.disabled).then(|| h.func.clone())
            })
            .flatten();
            let Some(func) = func else { continue };
            if let Err(e) = func.call(&JsValue::undefined(), args, &mut self.context) {
                self.errors
                    .push(format!("{} handler error: {e}", kind.label()));
                let _ = with_state(self.token, |st| {
                    if let Some(h) = st.handlers_mut(kind).get_mut(i) {
                        h.disabled = true;
                    }
                });
            }
        }
    }

    /// Style + layout + paint, but only when a mutation marked the document
    /// dirty — an unchanged tick does no layout work at all.
    fn render(&mut self) -> Option<Vec<PaintCmd>> {
        enum Outcome {
            Clean,
            Painted(Vec<PaintCmd>),
            Failed(String),
        }
        let mut outcome = Outcome::Clean;
        let (width, height) = (self.width, self.height);
        let styles = &mut self.styles_scratch;
        let _ = with_state(self.token, |st| {
            if !st.doc.dirty {
                return;
            }
            st.doc.dirty = false;
            style::compute_into(&st.doc, &st.sheet, styles);
            match layout::solve(&st.doc, styles, width, height) {
                Ok(solved) => {
                    let mut cmds = Vec::new();
                    paint::paint(&st.doc, styles, &solved, &mut cmds);
                    outcome = Outcome::Painted(cmds);
                }
                Err(e) => outcome = Outcome::Failed(e),
            }
        });
        match outcome {
            Outcome::Clean => None,
            Outcome::Painted(cmds) => Some(cmds),
            Outcome::Failed(e) => {
                self.errors.push(e);
                None
            }
        }
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        remove_state(self.token);
    }
}

/// Installs `document`, `sim`, `onFrame` and `onButton` — the whole script
/// surface. Returns the `sim` object for the per-tick updates.
fn install_globals(ctx: &mut Context, token: u64) -> JsResult<JsObject> {
    let get_by_id = NativeFunction::from_copy_closure(move |_this, args, ctx| {
        let id = args.get_or_undefined(0).to_string(ctx)?.to_std_string_lossy();
        let Some(idx) = with_state(token, |st| st.doc.find_by_id(&id)).flatten() else {
            return Ok(JsValue::null());
        };
        if let Some(obj) = with_state(token, |st| st.elements.get(&idx).cloned()).flatten() {
            return Ok(obj.into());
        }
        let obj = build_element_object(ctx, token, idx);
        let _ = with_state(token, |st| st.elements.insert(idx, obj.clone()));
        Ok(obj.into())
    });
    let document = ObjectInitializer::new(ctx)
        .function(get_by_id, js_string!("getElementById"), 1)
        .build();
    ctx.register_global_property(js_string!("document"), document, Attribute::all())?;

    let button = NativeFunction::from_copy_closure(move |_this, args, ctx| {
        let n = args.get_or_undefined(0).to_number(ctx)?;
        let pressed = if n.is_finite() && (1.0..=8.0).contains(&n) {
            with_state(token, |st| st.buttons[n as usize - 1]).unwrap_or(false)
        } else {
            false
        };
        Ok(JsValue::from(pressed))
    });
    let sim = ObjectInitializer::new(ctx)
        .function(button, js_string!("button"), 1)
        .build();
    ctx.register_global_property(js_string!("sim"), sim.clone(), Attribute::all())?;

    register_handler_global(ctx, token, "onFrame", HandlerKind::Frame)?;
    register_handler_global(ctx, token, "onButton", HandlerKind::Button)?;
    Ok(sim)
}

fn register_handler_global(
    ctx: &mut Context,
    token: u64,
    name: &str,
    kind: HandlerKind,
) -> JsResult<()> {
    let nf = NativeFunction::from_copy_closure(move |_this, args, _ctx| {
        let func = args
            .get_or_undefined(0)
            .as_object()
            .and_then(|o| JsFunction::from_object(o.clone()));
        let Some(func) = func else {
            return Err(JsNativeError::typ()
                .with_message("expected a function argument")
                .into());
        };
        let _ = with_state(token, |st| {
            st.handlers_mut(kind).push(Handler {
                func,
                disabled: false,
            });
        });
        Ok(JsValue::undefined())
    });
    ctx.register_global_callable(JsString::from(name), 1, nf)
}

/// Builds the JS face of one DOM element. Every closure captures only
/// `(token, idx)`, both `Copy`; the object is cached per node in the
/// registry, so scripts may call `getElementById` every frame for free.
fn build_element_object(ctx: &mut Context, token: u64, idx: usize) -> JsObject {
    let text_get = accessor_fn(
        ctx,
        NativeFunction::from_copy_closure(move |_this, _args, _ctx| {
            let text = with_state(token, |st| st.doc.text_content(idx)).unwrap_or_default();
            Ok(JsString::from(text.as_str()).into())
        }),
    );
    let text_set = accessor_fn(
        ctx,
        NativeFunction::from_copy_closure(move |_this, args, ctx| {
            let text = args.get_or_undefined(0).to_string(ctx)?.to_std_string_lossy();
            let _ = with_state(token, |st| st.doc.set_text_content(idx, &text));
            Ok(JsValue::undefined())
        }),
    );
    let hidden_get = accessor_fn(
        ctx,
        NativeFunction::from_copy_closure(move |_this, _args, _ctx| {
            let hidden = with_state(token, |st| st.doc.element(idx).is_some_and(|el| el.hidden))
                .unwrap_or(false);
            Ok(JsValue::from(hidden))
        }),
    );
    let hidden_set = accessor_fn(
        ctx,
        NativeFunction::from_copy_closure(move |_this, args, _ctx| {
            let hidden = args.get_or_undefined(0).to_boolean();
            let _ = with_state(token, |st| st.doc.set_hidden(idx, hidden));
            Ok(JsValue::undefined())
        }),
    );

    let get_attribute = NativeFunction::from_copy_closure(move |_this, args, ctx| {
        let name = attr_name(args.get_or_undefined(0), ctx)?;
        match with_state(token, |st| st.doc.get_attribute(idx, &name)).flatten() {
            Some(v) => Ok(JsString::from(v.as_str()).into()),
            None => Ok(JsValue::null()),
        }
    });
    let set_attribute = NativeFunction::from_copy_closure(move |_this, args, ctx| {
        let name = attr_name(args.get_or_undefined(0), ctx)?;
        let value = args.get_or_undefined(1).to_string(ctx)?.to_std_string_lossy();
        let _ = with_state(token, |st| st.doc.set_attribute(idx, &name, &value));
        Ok(JsValue::undefined())
    });

    let class_add = NativeFunction::from_copy_closure(move |_this, args, ctx| {
        for arg in args {
            let class = arg.to_string(ctx)?.to_std_string_lossy();
            let _ = with_state(token, |st| st.doc.class_add(idx, &class));
        }
        Ok(JsValue::undefined())
    });
    let class_remove = NativeFunction::from_copy_closure(move |_this, args, ctx| {
        for arg in args {
            let class = arg.to_string(ctx)?.to_std_string_lossy();
            let _ = with_state(token, |st| st.doc.class_remove(idx, &class));
        }
        Ok(JsValue::undefined())
    });
    let class_toggle = NativeFunction::from_copy_closure(move |_this, args, ctx| {
        let class = args.get_or_undefined(0).to_string(ctx)?.to_std_string_lossy();
        let present =
            with_state(token, |st| st.doc.class_toggle(idx, &class)).unwrap_or(false);
        Ok(JsValue::from(present))
    });
    let class_contains = NativeFunction::from_copy_closure(move |_this, args, ctx| {
        let class = args.get_or_undefined(0).to_string(ctx)?.to_std_string_lossy();
        let present =
            with_state(token, |st| st.doc.class_contains(idx, &class)).unwrap_or(false);
        Ok(JsValue::from(present))
    });
    let class_list = ObjectInitializer::new(ctx)
        .function(class_add, js_string!("add"), 1)
        .function(class_remove, js_string!("remove"), 1)
        .function(class_toggle, js_string!("toggle"), 1)
        .function(class_contains, js_string!("contains"), 1)
        .build();

    let set_property = NativeFunction::from_copy_closure(move |_this, args, ctx| {
        let name = args.get_or_undefined(0).to_string(ctx)?.to_std_string_lossy();
        let value = args.get_or_undefined(1).to_string(ctx)?.to_std_string_lossy();
        let _ = with_state(token, |st| st.doc.style_set_property(idx, &name, &value));
        Ok(JsValue::undefined())
    });
    let style_obj = ObjectInitializer::new(ctx)
        .function(set_property, js_string!("setProperty"), 2)
        .build();

    ObjectInitializer::new(ctx)
        .accessor(
            js_string!("textContent"),
            Some(text_get),
            Some(text_set),
            Attribute::all(),
        )
        .accessor(
            js_string!("hidden"),
            Some(hidden_get),
            Some(hidden_set),
            Attribute::all(),
        )
        .function(get_attribute, js_string!("getAttribute"), 1)
        .function(set_attribute, js_string!("setAttribute"), 2)
        .property(js_string!("classList"), class_list, Attribute::all())
        .property(js_string!("style"), style_obj, Attribute::all())
        .build()
}

fn accessor_fn(ctx: &mut Context, nf: NativeFunction) -> JsFunction {
    FunctionObjectBuilder::new(ctx.realm(), nf).build()
}

fn attr_name(value: &JsValue, ctx: &mut Context) -> JsResult<String> {
    Ok(value.to_string(ctx)?.to_std_string_lossy().to_ascii_lowercase())
}
