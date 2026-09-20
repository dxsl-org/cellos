// SPDX-License-Identifier: MIT
//! Lightweight JavaScript runtime for Ocel.

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use crate::js::engine::{JsEngine, JsError, JsValue, NativeFunction};

pub struct SimpleJsRuntime {
    globals: BTreeMap<String, JsValue>,
    functions: BTreeMap<String, NativeFunction>,
}

impl SimpleJsRuntime {
    pub fn new() -> Self {
        let mut runtime = Self {
            globals: BTreeMap::new(),
            functions: BTreeMap::new(),
        };

        // Standard console.log
        runtime.register_function("console.log", |args| {
            let parts: Vec<String> = args.iter().map(|a| a.to_string_repr()).collect();
            let msg = parts.join(" ");
            ostd::io::print("[ocel:js] ");
            ostd::io::println(&msg);
            Ok(JsValue::Undefined)
        });

        // alert
        runtime.register_function("alert", |args| {
            if let Some(first) = args.first() {
                ostd::io::print("[ocel:alert] ");
                ostd::io::println(&first.to_string_repr());
            }
            Ok(JsValue::Undefined)
        });

        runtime
    }

    fn parse_literal(&self, s: &str) -> JsValue {
        let trimmed = s.trim();

        // String literal
        if (trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
        {
            let inner = &trimmed[1..trimmed.len().saturating_sub(1)];
            return JsValue::String(String::from(inner));
        }

        // Boolean
        if trimmed == "true" {
            return JsValue::Bool(true);
        }
        if trimmed == "false" {
            return JsValue::Bool(false);
        }
        if trimmed == "null" {
            return JsValue::Null;
        }
        if trimmed == "undefined" {
            return JsValue::Undefined;
        }

        // Number (integer or float)
        let mut chars = trimmed.chars();
        let first = chars.next();
        if let Some(c) = first {
            if c.is_ascii_digit() || (c == '-' && chars.all(|x| x.is_ascii_digit() || x == '.')) {
                // Parse integer part
                let mut int_val = 0i64;
                let mut frac_val = 0.0f64;
                let mut is_frac = false;
                let mut frac_div = 1.0f64;
                let negative = trimmed.starts_with('-');
                let num_str = trimmed.trim_start_matches('-');

                for ch in num_str.chars() {
                    if ch == '.' {
                        is_frac = true;
                        continue;
                    }
                    if let Some(digit) = ch.to_digit(10) {
                        if is_frac {
                            frac_div *= 10.0;
                            frac_val += digit as f64 / frac_div;
                        } else {
                            int_val = int_val * 10 + digit as i64;
                        }
                    }
                }

                let total = (int_val as f64 + frac_val) * (if negative { -1.0 } else { 1.0 });
                return JsValue::Number(total);
            }
        }

        // Variable lookup
        if let Some(val) = self.globals.get(trimmed) {
            return val.clone();
        }

        JsValue::String(String::from(trimmed))
    }
}

impl JsEngine for SimpleJsRuntime {
    fn eval(&mut self, code: &str) -> Result<JsValue, JsError> {
        let mut last_value = JsValue::Undefined;

        for raw_line in code.lines() {
            let line = raw_line.trim().trim_end_matches(';');
            if line.is_empty() || line.starts_with("//") {
                continue;
            }

            // 1. Variable declaration: var x = ... / let x = ... / const x = ...
            let decl_prefix = line
                .strip_prefix("var ")
                .or_else(|| line.strip_prefix("let "))
                .or_else(|| line.strip_prefix("const "));

            if let Some(decl) = decl_prefix {
                if let Some((name, expr)) = decl.split_once('=') {
                    let val = self.parse_literal(expr);
                    self.set_global(name.trim(), val.clone());
                    last_value = val;
                    continue;
                }
            }

            // 2. Direct assignment: x = ...
            if let Some((lhs, rhs)) = line.split_once('=') {
                let name = lhs.trim();
                // Check if setting document.title
                if name == "document.title" {
                    let val = self.parse_literal(rhs);
                    self.set_global("document.title", val.clone());
                    last_value = val;
                    continue;
                }

                if !name.contains(' ') && !name.is_empty() {
                    let val = self.parse_literal(rhs);
                    self.set_global(name, val.clone());
                    last_value = val;
                    continue;
                }
            }

            // 3. Function call: func(...)
            if let Some((func_name, rest)) = line.split_once('(') {
                let clean_func = func_name.trim();
                if let Some(args_str) = rest.strip_suffix(')') {
                    let mut args = Vec::new();
                    if !args_str.trim().is_empty() {
                        for arg_item in args_str.split(',') {
                            args.push(self.parse_literal(arg_item));
                        }
                    }

                    if let Some(func) = self.functions.get(clean_func) {
                        last_value = func(&args)?;
                        continue;
                    }
                }
            }

            // 4. Expression evaluation fallback
            last_value = self.parse_literal(line);
        }

        Ok(last_value)
    }

    fn set_global(&mut self, name: &str, value: JsValue) {
        self.globals.insert(String::from(name), value);
    }

    fn get_global(&self, name: &str) -> Option<JsValue> {
        self.globals.get(name).cloned()
    }

    fn register_function(&mut self, name: &str, f: NativeFunction) {
        self.functions.insert(String::from(name), f);
    }
}
