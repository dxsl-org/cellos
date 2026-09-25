//! Safe Rust host services for C ports.
//!
//! This crate owns Cellos service clients. C ABI glue belongs in the porting
//! cell, where raw-pointer validation can be reviewed with the application ABI.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::vec::Vec;
use api::display::PixelFormat;
use ostd::{
    clients::{net::SocketId, NetClient, VfsClient},
    display::{wait_for_compositor, ViSurface},
    input::{self, InputEvent},
    syscall::sys_get_time,
    task::yield_now,
    ViResult,
};

/// Capability-scoped host services supplied to one C application port.
pub struct PlatformHost {
    vfs: VfsClient,
    net: NetClient,
}

impl PlatformHost {
    /// Creates lazy VFS and network clients; no service is contacted yet.
    pub fn new() -> Self {
        Self {
            vfs: VfsClient::new(),
            net: NetClient::new(),
        }
    }

    /// Returns the Cellos monotonic clock in milliseconds.
    pub fn time_ms(&self) -> u64 {
        sys_get_time() / ostd::MTIME_TICKS_PER_MS
    }

    /// Cooperatively waits for at least `ms` milliseconds.
    pub fn sleep_ms(&self, ms: u32) {
        let deadline = self.time_ms().saturating_add(ms as u64);
        while self.time_ms() < deadline {
            yield_now();
        }
    }

    /// Creates an interactive BGRA compositor surface owned by this Cell.
    pub fn create_surface(&self, width: u32, height: u32) -> ViResult<ViSurface> {
        ViSurface::create(wait_for_compositor(), width, height, PixelFormat::Bgra8888)
    }

    /// Requests keyboard/pointer focus. `false` means the input service is unavailable.
    pub fn request_input_focus(&self) -> bool {
        input::request_focus()
    }

    /// Drains at most `max` decoded input events without blocking.
    pub fn poll_input(&self, max: usize) -> Vec<InputEvent> {
        input::poll_events(max)
    }

    /// Reads a bounded file through the Cell's VFS authority.
    pub fn read_file(&mut self, path: &str, max_bytes: usize) -> ViResult<Vec<u8>> {
        self.vfs.read_file_bounded(path, max_bytes)
    }

    /// Writes one bounded VFS request through the Cell's declared authority.
    pub fn write_file(&mut self, path: &str, content: &[u8]) -> ViResult<()> {
        self.vfs.write_file(path, content)
    }

    /// Opens a TCP connection through the Cell's Net-service authority.
    pub fn tcp_connect(&mut self, address: [u8; 4], port: u16) -> ViResult<SocketId> {
        self.net.tcp_connect(address, port)
    }
}

impl Default for PlatformHost {
    fn default() -> Self {
        Self::new()
    }
}
