// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]

//! Core types for ViOS Cellular SAS architecture.
//!
//! This crate defines fundamental types used across the entire system.

/// Kernel Result Type
pub type HalResult<T> = core::result::Result<T, HalError>;

/// Standard Result type for ViOS APIs.
pub type Result<T, E = ViError> = core::result::Result<T, E>;

/// Kernel/HAL Errors
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HalError {
    GenericError,
    BusError,
    InvalidDevice,
    NotSupported,
    Busy,
    IoError,
    InvalidInput,
}

/// Unique identifier for a Cell.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CellId(pub u64);

/// State of a Cell in its lifecycle.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellState {
    /// Cell is being loaded and linked.
    Loading,
    /// Cell is active and running.
    Active,
    /// Cell is marked for unload but still has references.
    Zombie,
    /// Cell is poisoned and is being recovered.
    Poisoned,
}

/// Semantic versioning for Cells.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SemVer {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl SemVer {
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

/// Physical memory address.
pub type PhysAddr = usize;

/// Virtual memory address (Renamed from VirtAddr for brevity & standardization).
pub type VAddr = usize;

/// Standard Result type for ViOS APIs.
pub type ViResult<T> = core::result::Result<T, ViError>;

/// Common error types.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViError {
    /// Out of memory.
    OutOfMemory,
    /// Invalid argument.
    InvalidArgument,
    /// Resource not found.
    NotFound,
    /// Permission denied.
    PermissionDenied,
    /// Resource already exists.
    AlreadyExists,
    /// Operation would block.
    WouldBlock,
    /// Operation not supported.
    NotSupported,
    /// I/O Error.
    IO,
    /// Invalid input data.
    InvalidInput,
    /// Is a directory.
    IsADirectory,
    /// Not a directory.
    NotADirectory,
    /// Unknown error.
    Unknown,
}

/// File Type Enum
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    File = 0,
    Directory = 1,
    Device = 2,
    Unknown = 255,
}

/// Directory Entry
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DirEntry {
    pub name: [u8; 64], // Fixed size name
    pub file_type: FileType,
    pub size: u64,
}

impl Default for DirEntry {
    fn default() -> Self {
        Self {
            name: [0; 64],
            file_type: FileType::Unknown,
            size: 0,
        }
    }
}
