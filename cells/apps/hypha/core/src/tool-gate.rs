//! Operator gate between LLM output and tool cells.
//!
//! WHY: a `TOOL_CALL:` reply is whatever the model endpoint returned — on the
//! QEMU dev transport that is a plaintext HTTP peer, and even over TLS it is
//! model output steerable by anything in the prompt. Calls with a side effect
//! (start or stop a cell, change a file) are therefore shown to the operator
//! verbatim and run only after an explicit `y` typed on the hypha console.
//! Read-only calls run unprompted; `tool-fs` confines them to its workspace.
//!
//! The console line is the operator's only view of the call, so model-chosen
//! text must not be able to act on the terminal (clear it, move the cursor,
//! overwrite the line, reorder it with bidi controls): [`display_escape`].

use alloc::string::String;
use core::fmt::Write;

/// Tools whose effect outlives the turn and therefore need operator approval.
pub fn needs_confirmation(name: &str) -> bool {
    matches!(name, "spawn_cell" | "kill_cell" | "write_file")
}

/// The exact call, terminal-safe, followed by the question.
pub fn confirmation_prompt(name: &str, args_json: &str) -> String {
    alloc::format!(
        "[hypha] model requests tool call: {} {}\n[hypha] allow? [y/N] ",
        display_escape(name),
        display_escape(args_json)
    )
}

/// Only an explicit `y` approves; anything else, including an empty line or a
/// read failure upstream, denies.
pub fn is_confirmed(answer: &str) -> bool {
    matches!(answer.trim(), "y" | "Y")
}

/// Render untrusted text for the console: every control or invisible
/// formatting character becomes a visible `\u{…}` escape.
pub fn display_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_control() || is_invisible_format(c) {
            let _ = write!(out, "\\u{{{:x}}}", c as u32);
        } else {
            out.push(c);
        }
    }
    out
}

/// Zero-width and bidi-control characters (Unicode `Cf` ranges that reorder or
/// hide text on a terminal).
fn is_invisible_format(c: char) -> bool {
    matches!(
        c,
        '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2069}' | '\u{feff}'
    )
}
