// SPDX-License-Identifier: MIT
//! Pluggable JavaScript Engine implementation for Tier 2 ocel-js.
//!
//! Provides a DOM-aware evaluation context that records batched DOM mutations,
//! manages synthetic DOM proxy objects (keyed by `NodeId`), and handles DOM event listeners.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use dom_arena::{DomEvent, DomMutation, JsContext, JsEngine, JsError, NodeId};

pub struct OcelJsServiceEngine;

impl Default for OcelJsServiceEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl OcelJsServiceEngine {
    pub fn new() -> Self {
        Self
    }
}

impl JsEngine for OcelJsServiceEngine {
    type Context = OcelJsExecutionContext;

    fn create_context(&mut self) -> Self::Context {
        OcelJsExecutionContext::new()
    }
}

pub struct OcelJsExecutionContext {
    variables: BTreeMap<String, String>,
    listeners: BTreeMap<(NodeId, String), String>, // (target_node, event_kind) -> handler_code
    mutations: Vec<DomMutation>,
}

impl Default for OcelJsExecutionContext {
    fn default() -> Self {
        Self::new()
    }
}

impl OcelJsExecutionContext {
    pub fn new() -> Self {
        let mut variables = BTreeMap::new();
        variables.insert(
            String::from("document.title"),
            String::from("Ocel Document"),
        );
        Self {
            variables,
            listeners: BTreeMap::new(),
            mutations: Vec::new(),
        }
    }

    fn evaluate_expression(&self, expr: &str) -> String {
        let trimmed = expr.trim().trim_matches('"').trim_matches('\'');
        if let Some(val) = self.variables.get(trimmed) {
            val.clone()
        } else {
            String::from(trimmed)
        }
    }
}

impl JsContext for OcelJsExecutionContext {
    fn eval(&mut self, script: &str) -> Result<String, JsError> {
        let mut last_result = String::from("undefined");

        for (line_num, raw_line) in script.lines().enumerate() {
            let line = raw_line.trim().trim_end_matches(';');
            if line.is_empty() || line.starts_with("//") {
                continue;
            }

            // 1. console.log(...)
            if let Some(rest) = line.strip_prefix("console.log(") {
                if let Some(arg) = rest.strip_suffix(')') {
                    let val = self.evaluate_expression(arg);
                    ostd::io::print("[ocel-js:console] ");
                    ostd::io::println(&val);
                    last_result = val;
                    continue;
                }
            }

            // 2. document.title = "..."
            if let Some((lhs, rhs)) = line.split_once('=') {
                let target = lhs.trim();
                let expr_val = self.evaluate_expression(rhs);

                if target == "document.title" {
                    self.variables
                        .insert(String::from("document.title"), expr_val.clone());
                    self.mutations.push(DomMutation::SetDocumentTitle {
                        title: expr_val.clone(),
                    });
                    last_result = expr_val;
                    continue;
                }

                // 3. Node mutation: node_12.textContent = "..."
                if let Some(rest) = target.strip_prefix("node_") {
                    if let Some((id_str, prop)) = rest.split_once('.') {
                        if let Ok(id_num) = id_str.parse::<u32>() {
                            let node_id = NodeId(id_num);
                            if prop == "textContent" || prop == "innerText" {
                                self.mutations.push(DomMutation::SetText {
                                    node: node_id,
                                    text: expr_val.clone(),
                                });
                                last_result = expr_val;
                                continue;
                            }
                        }
                    }
                }

                // 4. Regular variable assignment: var x = ... / let x = ... / x = ...
                let clean_target = target
                    .strip_prefix("var ")
                    .or_else(|| target.strip_prefix("let "))
                    .or_else(|| target.strip_prefix("const "))
                    .unwrap_or(target)
                    .trim();

                self.variables
                    .insert(String::from(clean_target), expr_val.clone());
                last_result = expr_val;
                continue;
            }

            // 5. Method calls on nodes: node_12.setAttribute("class", "active")
            if let Some((call_target, args_rest)) = line.split_once('(') {
                if let Some(args_body) = args_rest.strip_suffix(')') {
                    let args: Vec<String> = args_body
                        .split(',')
                        .map(|a| self.evaluate_expression(a))
                        .collect();

                    if let Some(rest) = call_target.strip_prefix("node_") {
                        if let Some((id_str, method)) = rest.split_once('.') {
                            if let Ok(id_num) = id_str.parse::<u32>() {
                                let node_id = NodeId(id_num);
                                if method == "setAttribute" && args.len() >= 2 {
                                    self.mutations.push(DomMutation::SetAttribute {
                                        node: node_id,
                                        key: args[0].clone(),
                                        val: args[1].clone(),
                                    });
                                    last_result = args[1].clone();
                                    continue;
                                } else if method == "addEventListener" && args.len() >= 2 {
                                    self.listeners
                                        .insert((node_id, args[0].clone()), args[1].clone());
                                    last_result = String::from("listener_attached");
                                    continue;
                                }
                            }
                        }
                    }
                }
            }

            // 6. Simple arithmetic/expression fallback
            if let Some(val) = self.variables.get(line) {
                last_result = val.clone();
            } else {
                last_result = self.evaluate_expression(line);
            }

            if line.contains("throw ") {
                return Err(JsError {
                    message: format!("Uncaught exception at line {}", line_num + 1),
                    line: line_num as u32 + 1,
                });
            }
        }

        Ok(last_result)
    }

    fn dispatch_event(&mut self, event: &DomEvent) -> Result<(), JsError> {
        let kind_str = match event.kind {
            dom_arena::EventKind::Click => "click",
            dom_arena::EventKind::KeyDown => "keydown",
            dom_arena::EventKind::Input => "input",
            dom_arena::EventKind::Submit => "submit",
            dom_arena::EventKind::Change => "change",
        };

        if let Some(handler) = self
            .listeners
            .get(&(event.target, String::from(kind_str)))
            .cloned()
        {
            let _ = self.eval(&handler)?;
        }
        Ok(())
    }

    fn take_mutations(&mut self) -> Vec<DomMutation> {
        core::mem::take(&mut self.mutations)
    }

    fn reset(&mut self) {
        self.variables.clear();
        self.listeners.clear();
        self.mutations.clear();
        self.variables.insert(
            String::from("document.title"),
            String::from("Ocel Document"),
        );
    }
}
