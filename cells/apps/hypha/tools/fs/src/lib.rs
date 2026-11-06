//! Host-testable policy for the Hypha `tool-fs` cell: who may invoke it
//! ([`tool_owner`]) and which paths it may touch ([`workspace`]).

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

#[path = "../../tool-owner.rs"]
pub mod tool_owner;
pub mod workspace;

#[cfg(test)]
#[path = "policy-tests.rs"]
mod tests;
