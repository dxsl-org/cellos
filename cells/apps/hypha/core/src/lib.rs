//! Host-testable policy for the Hypha `core` cell: the operator confirmation
//! gate ([`tool_gate`]), the model preamble ([`preamble`]) and the tool owner
//! binding it publishes ([`tool_owner`]).

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod preamble;
#[path = "tool-gate.rs"]
pub mod tool_gate;
#[path = "../../tools/tool-owner.rs"]
pub mod tool_owner;

#[cfg(test)]
#[path = "policy-tests.rs"]
mod tests;
