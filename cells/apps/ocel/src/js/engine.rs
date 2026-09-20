// SPDX-License-Identifier: MIT
//! JavaScript Engine Trait and Core Value Model for Ocel.

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum JsValue {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<JsValue>),
    Object(BTreeMap<String, JsValue>),
}

impl JsValue {
    pub fn to_string_repr(&self) -> String {
        match self {
            Self::Undefined => String::from("undefined"),
            Self::Null => String::from("null"),
            Self::Bool(b) => String::from(if *b { "true" } else { "false" }),
            Self::Number(n) => format!("{:.2}", n),
            Self::String(s) => s.clone(),
            Self::Array(arr) => {
                let items: Vec<String> = arr.iter().map(|v| v.to_string_repr()).collect();
                format!("[{}]", items.join(", "))
            }
            Self::Object(map) => {
                let pairs: Vec<String> = map
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, v.to_string_repr()))
                    .collect();
                format!("{{{}}}", pairs.join(", "))
            }
        }
    }
}
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct JsError {
    pub message: String,
    pub line: u32,
}

pub type NativeFunction = fn(args: &[JsValue]) -> Result<JsValue, JsError>;

pub trait JsEngine {
    fn eval(&mut self, code: &str) -> Result<JsValue, JsError>;
    fn set_global(&mut self, name: &str, value: JsValue);
    fn get_global(&self, name: &str) -> Option<JsValue>;
    fn register_function(&mut self, name: &str, f: NativeFunction);
}
