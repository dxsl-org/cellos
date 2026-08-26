//! Host-testable protocol helpers for the Hypha LLM gateway.

#![no_std]

extern crate alloc;

pub mod http;
#[path = "json-validation.rs"]
mod json_validation;
