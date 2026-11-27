//! Sandboxed interactive visual programs and the native trees they produce.
//!
//! Programs are JavaScript function bodies that return a synchronous render
//! function. The only application capability is the frozen `visual` component
//! API; there is no filesystem, network, shell, DOM, or module loader.
mod model;
pub use model::*;

use rquickjs::{Context, Function, Object, Persistent, Runtime};
use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

/// Conservative per-interpreter bound for transcript cache accounting.
pub const MEMORY_LIMIT_BYTES: usize = 16 * 1024 * 1024;
const TREE_BYTES: usize = 1024 * 1024;
const CALL_DEADLINE: Duration = Duration::from_millis(60);

/// An isolated program. Keep it alive to preserve state between native events.
/// Field order ensures all persistent JS values are dropped before their VM.
pub struct VisualRuntime {
    controller: Persistent<Object<'static>>,
    context: Context,
    _runtime: Runtime,
    deadline: Rc<Cell<Instant>>,
    interrupted: Rc<Cell<bool>>,
    stopped: bool,
    current: Option<Node>,
}

impl VisualRuntime {
    pub fn new(source: &str) -> Result<Self, String> {
        validate_source(source)?;
        let runtime = Runtime::new().map_err(|e| format!("Could not start visual runtime: {e}"))?;
        runtime.set_memory_limit(MEMORY_LIMIT_BYTES);
        runtime.set_max_stack_size(256 * 1024);
        let deadline = Rc::new(Cell::new(Instant::now() + CALL_DEADLINE));
        let interrupted = Rc::new(Cell::new(false));
        let interrupt_deadline = deadline.clone();
        let interrupt_flag = interrupted.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || {
            let expired = Instant::now() >= interrupt_deadline.get();
            if expired {
                interrupt_flag.set(true);
            }
            expired
        })));
        let context = Context::full(&runtime)
            .map_err(|e| format!("Could not initialize visual context: {e}"))?;
        let controller = context.with(|ctx| {
            let result = (|| -> rquickjs::Result<Persistent<Object<'static>>> {
                let controller: Object = ctx.eval(include_str!("visual/sdk.js"))?;
                let init: Function = controller.get("init")?;
                init.call::<_, ()>((source,))?;
                Ok(Persistent::save(&ctx, controller))
            })();
            result.map_err(|e| execution_error(&ctx, e, &interrupted))
        })?;
        Ok(Self {
            controller,
            context,
            _runtime: runtime,
            deadline,
            interrupted,
            stopped: false,
            current: None,
        })
    }

    pub fn render(&mut self) -> Result<Node, String> {
        self.call("render", None)
    }

    /// Dispatch only the current render's handler. Values are checked against the
    /// original control before script code sees them (slider bounds, tab index,
    /// boolean toggle). Stale callbacks cannot mutate a replacement render.
    pub fn dispatch(&mut self, handler: &str, value: serde_json::Value) -> Result<Node, String> {
        if handler.len() > 64 {
            return Err("Invalid visual event handler".into());
        }
        self.current
            .as_ref()
            .and_then(|node| validate_event(node, handler, &value))
            .unwrap_or_else(|| Err("This visual control is stale; use the current view".into()))?;
        let payload = serde_json::to_string(&value).map_err(|e| e.to_string())?;
        if payload.len() > 32 * 1024 {
            return Err("Visual event payload is too large".into());
        }
        self.call("dispatch", Some((handler, &payload)))
    }

    fn call(&mut self, method: &str, event: Option<(&str, &str)>) -> Result<Node, String> {
        if self.stopped {
            return Err("Visual stopped after an execution error; reload it to continue".into());
        }
        self.deadline.set(Instant::now() + CALL_DEADLINE);
        self.interrupted.set(false);
        let result = self.context.with(|ctx| {
            let result = (|| -> rquickjs::Result<String> {
                let controller = self.controller.clone().restore(&ctx)?;
                let function: Function = controller.get(method)?;
                match event {
                    Some((handler, payload)) => function.call((handler, payload)),
                    None => function.call(()),
                }
            })();
            result.map_err(|e| execution_error(&ctx, e, &self.interrupted))
        });
        let result = result.and_then(|json| {
            if json.len() > TREE_BYTES {
                return Err("Visual component tree exceeds 1 MiB".into());
            }
            let node: Node = serde_json::from_str(&json)
                .map_err(|e| format!("Invalid visual component tree: {e}"))?;
            node.validate()?;
            Ok(node)
        });
        match &result {
            Ok(node) => self.current = Some(node.clone()),
            Err(_) => self.stopped = true,
        }
        result
    }
}

fn validate_event(node: &Node, id: &str, value: &serde_json::Value) -> Option<Result<(), String>> {
    let accepted = match node {
        Node::Column { children, .. }
        | Node::Row { children, .. }
        | Node::Grid { children, .. }
        | Node::Card { children, .. } => {
            return children
                .iter()
                .find_map(|child| validate_event(child, id, value));
        }
        Node::Button {
            handler, disabled, ..
        } if handler == id => {
            if *disabled {
                return Some(Err("This visual control is disabled".into()));
            }
            value.is_null()
        }
        Node::Toggle { handler, .. } if handler == id => value.is_boolean(),
        Node::Input { handler, .. } if handler == id => {
            value.as_str().is_some_and(|s| s.len() <= 4096)
        }
        Node::Select {
            handler, options, ..
        } if handler == id => value.as_u64().is_some_and(|v| v < options.len() as u64),
        Node::Tabs {
            handler, labels, ..
        } if handler == id => value.as_u64().is_some_and(|v| v < labels.len() as u64),
        Node::Slider {
            handler, min, max, ..
        } if handler == id => value
            .as_f64()
            .is_some_and(|v| v.is_finite() && v >= *min && v <= *max),
        _ => return None,
    };
    Some(if accepted {
        Ok(())
    } else {
        Err("Invalid value for this visual control".into())
    })
}

/// Validate the initial view and a bounded sample of its interactive controls.
/// Each event runs in a fresh VM so checking never changes the published state.
pub fn check(source: &str) -> Result<Node, String> {
    let initial = VisualRuntime::new(source)?.render()?;
    let mut probes = Vec::new();
    collect_probes(&initial, &mut probes);
    for (handler, value) in probes.into_iter().take(8) {
        let mut probe = VisualRuntime::new(source)?;
        probe.render()?;
        probe
            .dispatch(&handler, value)
            .map_err(|e| format!("Visual control check failed ({handler}): {e}"))?;
    }
    Ok(initial)
}

fn collect_probes(node: &Node, probes: &mut Vec<(String, serde_json::Value)>) {
    use serde_json::json;
    if probes.len() >= 8 {
        return;
    }
    match node {
        Node::Column { children, .. }
        | Node::Row { children, .. }
        | Node::Grid { children, .. }
        | Node::Card { children, .. } => {
            for child in children {
                collect_probes(child, probes);
            }
        }
        Node::Button {
            handler,
            disabled: false,
            ..
        } => probes.push((handler.clone(), json!(null))),
        Node::Toggle { handler, value, .. } => probes.push((handler.clone(), json!(!value))),
        Node::Input { handler, value, .. } => probes.push((handler.clone(), json!(value))),
        Node::Select {
            handler,
            options,
            selected,
            ..
        } => probes.push((handler.clone(), json!((selected + 1) % options.len()))),
        Node::Tabs {
            handler,
            labels,
            selected,
            ..
        } => probes.push((handler.clone(), json!((selected + 1) % labels.len()))),
        Node::Slider {
            handler, min, max, ..
        } => {
            probes.push((handler.clone(), json!(min)));
            probes.push((handler.clone(), json!(max)));
        }
        _ => {}
    }
}

fn execution_error(
    ctx: &rquickjs::Ctx<'_>,
    error: rquickjs::Error,
    interrupted: &Cell<bool>,
) -> String {
    if interrupted.get() {
        // Clear the exception without invoking an untrusted exception accessor.
        let _ = ctx.catch();
        return "Visual execution exceeded its 60 ms time budget".into();
    }
    let message = if error.is_exception() {
        let exception = ctx.catch();
        exception
            .as_exception()
            .and_then(|e| e.message())
            .unwrap_or_else(|| "Script threw an exception".into())
    } else {
        error.to_string()
    };
    // Reading an Error.message getter can execute script too; it runs under
    // the same interrupt deadline as the operation that threw the exception.
    if interrupted.get() {
        let _ = ctx.catch();
        return "Visual execution exceeded its 60 ms time budget".into();
    }
    format!(
        "Visual execution failed: {}",
        message.chars().take(2048).collect::<String>()
    )
}

#[cfg(test)]
mod tests;
