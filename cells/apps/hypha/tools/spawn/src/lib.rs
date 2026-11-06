//! Host-testable policy for the Hypha `tool-spawn` cell: who may invoke it
//! ([`tool_owner`]) and what it may spawn or kill ([`spawn_policy`]).

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

#[path = "spawn-policy.rs"]
pub mod spawn_policy;
#[path = "../../tool-owner.rs"]
pub mod tool_owner;

#[cfg(test)]
#[path = "policy-tests.rs"]
mod tests;
