//! Host-testable protocol helpers for the Hypha LLM gateway.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod http;
#[path = "json-validation.rs"]
mod json_validation;
