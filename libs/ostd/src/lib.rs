// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![feature(alloc_error_handler)]

extern crate alloc;

// ostd - ViCell Standard Library
//
// Replacement for Rust's std library for ViCell Cells.
// INTERFACE ONLY - NO IMPLEMENTATION YET.

pub use api::*;

// Re-export alloc types
pub use alloc::boxed;
pub use alloc::string;
pub use alloc::vec;

/// Result type used throughout ViCell.
pub type Result<T, E = ViError> = core::result::Result<T, E>;

pub mod fast_ipc;
/// Typed linear Grant handles for zero-copy shared memory (Singularity exchange-heap pattern).
pub mod grant;
pub mod mmio;
pub mod startup;
pub mod sync;
pub mod syscall;

/// Allocator hooks (to be implemented).
pub mod heap;

/// I/O traits and functions.
pub mod io;

/// Filesystem.
pub mod fs;

/// Shared readline / REPL state machine (used by Shell).
pub mod repl;

pub mod prelude;

/// Executor
pub mod executor;

/// TLS 1.3 client helpers for app cells.
pub mod tls;

/// Platform mtime frequency: ticks per millisecond at the assumed 10 MHz mtime clock.
///
/// Matches `hal::arch::riscv::common::timer::TICKS_PER_10MS / 10`.
/// Override at build time for boards with a different mtime frequency.
pub const MTIME_TICKS_PER_MS: u64 = 10_000;

/// App-side display helpers (ViSurface, wait_for_compositor).
pub mod display;

/// Bitmap font renderer — `draw_text` for ASCII output on any pixel buffer.
pub mod font;

/// Scalable glyph atlas backed by fontdue (no_std + hashbrown feature).
pub mod font_atlas;

/// Task spawning.
pub mod task {
    use crate::*;

    /// Yield current task.
    pub fn yield_now() {
        syscall::sys_yield();
    }
}
