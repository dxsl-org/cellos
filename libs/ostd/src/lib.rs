#![no_std]
// #![deny(unsafe_code)]
// #![deny(unsafe_code)]

//! # ViOS Safe Standard Library (ostd)
//!
//! This library provides the "Safe Rust" surface for all Drivers and Applications.
//! It abstracts away the raw System Calls and Unsafe bindings.
//!
//! ## Philosophy (Asterinas Model)
//! - **No Unsafe**: Users of this crate cannot write `unsafe` code.
//! - **Abstractions**: We provide high-level wrappers for Kernel capabilities.

extern crate alloc;

pub mod console;
pub mod io;
pub mod memory;
pub mod prelude;
pub mod syscall;
pub mod executor;
pub mod ipc;
pub mod task;
pub mod sync;

/// A marker trait for Drivers that are "Safe"
pub trait SafeDriver {
    fn name(&self) -> &str;
    fn init(&mut self) -> Result<(), &'static str>;
}
