//! The QuickJS-backed script engine.
//!
//! The engine object is deliberately thin: QuickJS owns JavaScript state, and
//! the DOM surface Ocel talks to (`document.title`, `node_<id>.textContent`,
//! `node_<id>.setAttribute`, `node_<id>.addEventListener`, `console.log`,
//! event dispatch) is defined by a **JavaScript prelude** evaluated once per
//! context. Rust never builds JS objects through the C API: it evaluates the
//! prelude, evaluates the document's script, and then reads the mutation queue
//! the prelude recorded.
//!
//! That keeps the C surface to the handful of entry points in [`super::ffi`]
//! and makes the DOM dialect a reviewed, readable JS file instead of a wall of
//! `JS_NewCFunction` calls.
//!
//! Mutation wire format (single string, records separated by `\n`, fields by
//! `\t`): `T<title>`, `X<node>\t<text>`, `A<node>\t<key>\t<val>`,
//! `R<node>\t<key>` (removeAttribute), `P<parent>\t<child>`,
//! `D<parent>\t<child>` (removeChild), `G<message>` (console.log).

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use dom_arena::{DomEvent, DomMutation, JsContext, JsEngine, JsError, NodeId};

use super::ffi;

/// DOM globals the prelude materialises: `node_0` … `node_N`.
///
/// Ocel indexes the arena's nodes by id, and the prelude records mutations
/// against those same ids. A fixed window keeps the prelude from needing lazy
/// global creation (JavaScript has no global getter), and 256 covers the
/// layouts Ocel produces today.
const NODE_WINDOW: usize = 256;

/// Recursion budget handed to the engine, in bytes of cell stack.
///
/// Cells get a 256 KiB stack (`kernel/src/task.rs`, `STACK_PAGES`). The engine
/// checks this limit before recursing into the interpreter or the parser, so a
/// pathological script fails as a JS `RangeError` instead of faulting the cell.
const MAX_STACK_BYTES: usize = 192 * 1024;

/// Heap ceiling for the engine's own accounting. The allocation still comes
/// from the cell heap (see the cell's `declare_custom_heap!`); this only makes
/// the engine's allocator fail the allocation instead of the cell.
const MAX_HEAP_BYTES: usize = 3 * 1024 * 1024;

const PRELUDE: &str = include_str!("prelude.js");

pub struct QuickJsEngine;

impl Default for QuickJsEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl QuickJsEngine {
    pub fn new() -> Self {
        Self
    }
}

impl JsEngine for QuickJsEngine {
    type Context = QuickJsContext;

    fn create_context(&mut self) -> Self::Context {
        QuickJsContext::new()
    }
}

pub struct QuickJsContext {
    rt: *mut ffi::JSRuntime,
    ctx: *mut ffi::JSContext,
    mutations: Vec<DomMutation>,
    /// Non-fatal engine diagnostics (script `console.log`, runtime warnings)
    /// surfaced to the cell log rather than into the mutation stream.
    logs: Vec<String>,
}

impl Drop for QuickJsContext {
    fn drop(&mut self) {
        unsafe {
            if !self.ctx.is_null() {
                ffi::JS_FreeContext(self.ctx);
            }
            if !self.rt.is_null() {
                ffi::JS_FreeRuntime(self.rt);
            }
        }
    }
}

impl QuickJsContext {
    fn new() -> Self {
        let mut out = Self {
            rt: core::ptr::null_mut(),
            ctx: core::ptr::null_mut(),
            mutations: Vec::new(),
            logs: Vec::new(),
        };
        unsafe {
            let rt = ffi::JS_NewRuntime();
            if rt.is_null() {
                out.logs.push(String::from(
                    "[ocel-quickjs] JS_NewRuntime failed (cell heap exhausted?)",
                ));
                return out;
            }
            out.rt = rt;
            ffi::JS_SetMaxStackSize(rt, MAX_STACK_BYTES);
            ffi::JS_SetMemoryLimit(rt, MAX_HEAP_BYTES);
            ffi::JS_SetGCThreshold(rt, 256 * 1024);
            let ctx = ffi::JS_NewContext(rt);
            if ctx.is_null() {
                out.logs.push(String::from(
                    "[ocel-quickjs] JS_NewContext failed; engine disabled",
                ));
                return out;
            }
            out.ctx = ctx;
        }
        if let Err(e) = out.eval_raw(&prelude_source(), "<ocel-prelude>") {
            // A prelude failure is fatal for the DOM dialect: report it through
            // the log channel so the cell's start-up line can say so.
            out.logs.push(alloc::format!(
                "[ocel-quickjs] DOM prelude failed: {}",
                e.message
            ));
        }
        out
    }

    fn ready(&self) -> bool {
        !self.ctx.is_null()
    }

    /// Evaluate `src`, collecting anything the prelude recorded.
    ///
    /// Returns `Ok(result_repr)` for a normal completion and `Err(JsError)`
    /// for a JS exception (the message comes from the engine's own exception
    /// object, so a script bug reports as the script author's error).
    fn eval_raw(&mut self, src: &str, filename: &str) -> Result<String, JsError> {
        if !self.ready() {
            return Err(JsError {
                message: String::from("engine unavailable"),
                line: 0,
            });
        }
        let code = alloc::ffi::CString::new(src).map_err(|_| JsError {
            message: String::from("script contains an interior NUL byte"),
            line: 0,
        })?;
        let name = alloc::ffi::CString::new(filename)
            .unwrap_or_else(|_| alloc::ffi::CString::new("<script>").expect("literal has no NUL"));
        unsafe {
            let value = ffi::JS_Eval(
                self.ctx,
                code.as_ptr(),
                src.len(),
                name.as_ptr(),
                ffi::JS_EVAL_TYPE_GLOBAL,
            );
            if ffi::vios_js_is_exception(value) != 0 {
                let message = self.take_exception_message();
                ffi::vios_js_free(self.ctx, value);
                return Err(JsError { message, line: 0 });
            }
            let repr = self.value_to_string(value);
            ffi::vios_js_free(self.ctx, value);
            self.collect_mutations();
            Ok(repr)
        }
    }

    /// Read the pending exception's `message`/`stack` and clear it.
    unsafe fn take_exception_message(&mut self) -> String {
        let exc = ffi::JS_GetException(self.ctx);
        let mut message = self.value_to_string(exc);
        ffi::vios_js_free(self.ctx, exc);
        if message.is_empty() {
            message = String::from("uncaught JavaScript exception");
        }
        message
    }

    /// `String(value)`-style rendering, using the engine's own conversion so
    /// numbers keep the engine's formatting.
    unsafe fn value_to_string(&mut self, value: ffi::JSValue) -> String {
        if ffi::vios_js_is_undefined(value) != 0 {
            return String::from("undefined");
        }
        if ffi::vios_js_is_null(value) != 0 {
            return String::from("null");
        }
        if ffi::vios_js_is_number(value) != 0 {
            if ffi::vios_js_is_float(value) != 0 {
                return format_float(ffi::vios_js_as_float(value));
            }
            let mut d = 0.0f64;
            if ffi::JS_ToFloat64(self.ctx, &mut d, value) == 0 {
                return format_float(d);
            }
        }
        if ffi::vios_js_is_bool(value) != 0 {
            return if ffi::JS_ToBool(self.ctx, value) != 0 {
                String::from("true")
            } else {
                String::from("false")
            };
        }
        let mut len = 0usize;
        let ptr = ffi::JS_ToCStringLen2(self.ctx, &mut len, value, 0);
        if ptr.is_null() {
            return String::from("[unprintable]");
        }
        // `.cast()` rather than `as *const u8`: `c_char` is `u8` on
        // riscv64/aarch64 and `i8` on x86_64.
        let out = core::str::from_utf8(core::slice::from_raw_parts(ptr.cast::<u8>(), len))
            .unwrap_or("[invalid utf-8]")
            .to_string();
        ffi::JS_FreeCString(self.ctx, ptr);
        out
    }

    /// Drain the prelude's queues into Rust-side mutations and logs.
    fn collect_mutations(&mut self) {
        let drained = self.eval_queue("__vios_takeMutations()");
        for record in drained.lines() {
            if record.is_empty() {
                continue;
            }
            let mut fields = record.split('\t');
            let kind = fields.next().unwrap_or("");
            match kind {
                "T" => {
                    if let Some(title) = fields.next() {
                        self.mutations.push(DomMutation::SetDocumentTitle {
                            title: title.to_string(),
                        });
                    }
                }
                "X" => {
                    let (Some(node), Some(text)) = (fields.next(), fields.next()) else {
                        continue;
                    };
                    if let Ok(id) = node.parse::<u32>() {
                        self.mutations.push(DomMutation::SetText {
                            node: NodeId(id),
                            text: text.to_string(),
                        });
                    }
                }
                "A" => {
                    let (Some(node), Some(key), Some(val)) =
                        (fields.next(), fields.next(), fields.next())
                    else {
                        continue;
                    };
                    if let Ok(id) = node.parse::<u32>() {
                        self.mutations.push(DomMutation::SetAttribute {
                            node: NodeId(id),
                            key: key.to_string(),
                            val: val.to_string(),
                        });
                    }
                }
                "R" => {
                    let (Some(node), Some(key)) = (fields.next(), fields.next()) else {
                        continue;
                    };
                    if let Ok(id) = node.parse::<u32>() {
                        self.mutations.push(DomMutation::RemoveAttribute {
                            node: NodeId(id),
                            key: key.to_string(),
                        });
                    }
                }
                "P" => {
                    let (Some(parent), Some(child)) = (fields.next(), fields.next()) else {
                        continue;
                    };
                    if let (Ok(p), Ok(c)) = (parent.parse::<u32>(), child.parse::<u32>()) {
                        self.mutations.push(DomMutation::AppendChild {
                            parent: NodeId(p),
                            child: NodeId(c),
                        });
                    }
                }
                "D" => {
                    let (Some(parent), Some(child)) = (fields.next(), fields.next()) else {
                        continue;
                    };
                    if let (Ok(p), Ok(c)) = (parent.parse::<u32>(), child.parse::<u32>()) {
                        self.mutations.push(DomMutation::RemoveChild {
                            parent: NodeId(p),
                            child: NodeId(c),
                        });
                    }
                }
                _ => {}
            }
        }

        let logged = self.eval_queue("__vios_takeLogs()");
        for line in logged.lines() {
            if !line.is_empty() {
                self.logs.push(line.to_string());
            }
        }
    }

    /// Evaluate a prelude helper that returns a string, without recursing into
    /// [`Self::collect_mutations`].
    fn eval_queue(&mut self, expr: &str) -> String {
        if !self.ready() {
            return String::new();
        }
        let Ok(code) = alloc::ffi::CString::new(expr) else {
            return String::new();
        };
        let name = c"<ocel-queue>".as_ptr();
        unsafe {
            let value = ffi::JS_Eval(
                self.ctx,
                code.as_ptr(),
                expr.len(),
                name,
                ffi::JS_EVAL_TYPE_GLOBAL,
            );
            if ffi::vios_js_is_exception(value) != 0 {
                let message = self.take_exception_message();
                self.logs.push(alloc::format!(
                    "[ocel-quickjs] queue eval failed: {message}"
                ));
                ffi::vios_js_free(self.ctx, value);
                return String::new();
            }
            let out = self.value_to_string(value);
            ffi::vios_js_free(self.ctx, value);
            out
        }
    }
}

impl JsContext for QuickJsContext {
    fn eval(&mut self, script: &str) -> Result<String, JsError> {
        self.eval_raw(script, "<script>")
    }

    fn dispatch_event(&mut self, event: &DomEvent) -> Result<(), JsError> {
        let kind = match event.kind {
            dom_arena::EventKind::Click => "click",
            dom_arena::EventKind::KeyDown => "keydown",
            dom_arena::EventKind::Input => "input",
            dom_arena::EventKind::Submit => "submit",
            dom_arena::EventKind::Change => "change",
        };
        if !self.ready() {
            return Ok(());
        }
        let call = alloc::format!("__vios_dispatch({}, \"{}\")", event.target.0, kind);
        self.eval_raw(&call, "<dispatch>").map(|_| ())
    }

    fn take_mutations(&mut self) -> Vec<DomMutation> {
        core::mem::take(&mut self.mutations)
    }

    fn reset(&mut self) {
        // Recreating the context is the only reset that cannot leak a global:
        // `eval('...')`-based resets always leave the prelude's own state behind.
        *self = Self::new();
    }
}

impl QuickJsContext {
    /// Drain and return the engine diagnostics collected so far.
    pub fn take_logs(&mut self) -> Vec<String> {
        core::mem::take(&mut self.logs)
    }
}

/// Minimal `Number`-to-text for the value repr: integers print without a
/// trailing `.0`, everything else uses Rust's shortest round-trip. The engine's
/// own `js_dtoa` does the same job in C for `String(n)`; this only affects the
/// protocol's informational `result_repr`.
fn format_float(d: f64) -> String {
    let integral = d.is_finite() && libm::trunc(d) == d;
    if integral && libm::fabs(d) < 9.007_199_254_740_992e15 {
        alloc::format!("{}", d as i64)
    } else if d.is_nan() {
        String::from("NaN")
    } else if d.is_infinite() {
        String::from(if d > 0.0 { "Infinity" } else { "-Infinity" })
    } else {
        alloc::format!("{d}")
    }
}

/// The prelude with the node window the Rust side expects.
///
/// `NODE_WINDOW` is a Rust constant because the engine driver owns the node id
/// space; injecting it keeps one number instead of two that could drift.
fn prelude_source() -> String {
    alloc::format!("var NODE_WINDOW = {};\n{}", NODE_WINDOW, PRELUDE)
}

/// Marker used by the cell's start-up line so a lane can tell which engine
/// answered without guessing from behaviour.
pub fn engine_identity() -> String {
    alloc::format!("quickjs {}", env!("QJS_VENDOR_VERSION"))
}
