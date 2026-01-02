#![no_std]

//! # ViOS Hardware Abstraction Layer (Core Traits)
//!
//! This crate defines the **Interface** that all hardware platforms must implement.
//! It serves as the "Common Language" between the ViOS Kernel and specific chips (ESP32, ARM, x86).
//!
//! ## Design Philosophy
//! - **Trait-based**: We use Rust traits to enforce interfaces.
//! - **Zero-Cost**: Abstractions should compile down to raw register writes.
//! - **Safe Wrappers**: Unsafe hardware details are hidden behind safe trait methods.

pub mod boot;
pub mod console;
pub mod gpio;
pub mod uart;

/// Common error type for Hardware Operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HalError {
    /// The physical device is not present or not responding.
    DeviceNotPresent,
    /// The operation is not supported by this hardware.
    NotSupported,
    /// Detailed hardware permission error (e.g. MPU violation).
    PermissionDenied,
    /// Generic I/O Error
    IoError,
}

/// Result type alias for HAL operations
pub type HalResult<T> = Result<T, HalError>;
