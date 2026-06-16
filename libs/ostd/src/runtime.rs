// SPDX-License-Identifier: MPL-2.0

//! ViCell Cell Runtime — ergonomic entry point and lifecycle management.
//!
//! [`CellRuntime`] is a builder that wires up the heartbeat watchdog and
//! optionally registers the cell as a named service before handing off to the
//! event loop.  It is the recommended way to start a cell.
//!
//! [`app_entry!`] and [`service_entry!`] are declarative macros that generate
//! the `declare_manifest!`, `declare_syscalls!`, and `main()` boilerplate
//! automatically from a capability declaration, so a minimal app is:
//!
//! ```no_run
//! use ostd::app::{AppContext, AppEvent};
//!
//! ostd::app_entry!(handler = my_handler);
//!
//! fn my_handler(ctx: &mut AppContext, ev: AppEvent) {
//!     match ev {
//!         AppEvent::Init     => { /* startup */ }
//!         AppEvent::Shutdown | AppEvent::ShutdownWith { .. } => {
//!             ostd::syscall::sys_exit(0);
//!         }
//!         _ => {}
//!     }
//! }
//! ```

use api::syscall::{SyscallSet, ViSyscall};
use crate::app::{AppContext, AppEvent};

// ── SyscallSet profiles ───────────────────────────────────────────────────────

/// Compute the syscall allowlist mask for a cell with the given capability profile.
///
/// This is a `const fn` so it can be used directly as a `static` initializer —
/// the compiler evaluates it at build time.
///
/// # Base set (all cells)
/// `Send, Recv, TryRecv, Reply, Log, Heartbeat, LookupService, GetTime, RecvTimeout`
///
/// # Capability extras
/// - `block_io=true` → `GrantAlloc/Share/Slice/Free/Register/Unregister`
/// - `network=true`  → `NetTx, NetRx, WaitForEvent, GetRandom`
/// - `spawn=true`    → `SpawnFromPath/Mem/Pinned, Wait, OpenCap, ReadCap, CloseCap,
///                      GetProcs, HotSwap, Snapshot, StateStash/Restore`
pub const fn app_syscall_set(block_io: bool, network: bool, spawn: bool) -> u64 {
    let base = SyscallSet::EMPTY
        .with(ViSyscall::Send)
        .with(ViSyscall::Recv)
        .with(ViSyscall::TryRecv)
        .with(ViSyscall::Reply)
        .with(ViSyscall::Log)
        .with(ViSyscall::Heartbeat)
        .with(ViSyscall::LookupService)
        .with(ViSyscall::GetTime)
        .with(ViSyscall::RecvTimeout);

    let bio_extra = if block_io {
        SyscallSet::EMPTY
            .with(ViSyscall::GrantAlloc)
            .with(ViSyscall::GrantShare)
            .with(ViSyscall::GrantSlice)
            .with(ViSyscall::GrantFree)
            .with(ViSyscall::GrantRegister)
            .with(ViSyscall::GrantUnregister)
            .with(ViSyscall::BlkReadAsync)
    } else { SyscallSet::EMPTY };

    let net_extra = if network {
        SyscallSet::EMPTY
            .with(ViSyscall::NetTx)
            .with(ViSyscall::NetRx)
            .with(ViSyscall::WaitForEvent)
            .with(ViSyscall::GetRandom)
    } else { SyscallSet::EMPTY };

    let spawn_extra = if spawn {
        SyscallSet::EMPTY
            .with(ViSyscall::SpawnFromPath)
            .with(ViSyscall::SpawnFromMem)
            .with(ViSyscall::SpawnPinned)
            .with(ViSyscall::Wait)
            .with(ViSyscall::OpenCap)
            .with(ViSyscall::ReadCap)
            .with(ViSyscall::CloseCap)
            .with(ViSyscall::GetProcs)
            .with(ViSyscall::HotSwap)
            .with(ViSyscall::Snapshot)
            .with(ViSyscall::StateStash)
            .with(ViSyscall::StateRestore)
    } else { SyscallSet::EMPTY };

    SyscallSet(base.0 | bio_extra.0 | net_extra.0 | spawn_extra.0).bits()
}

/// Like [`app_syscall_set`] but includes `WaitForEvent` in the base set for
/// service cells that wake on kernel events (net RX, etc.).
pub const fn service_syscall_set(block_io: bool, network: bool, spawn: bool) -> u64 {
    let app_bits = app_syscall_set(block_io, network, spawn);
    // Add WaitForEvent to the base for services
    let extra = SyscallSet::EMPTY.with(ViSyscall::WaitForEvent);
    SyscallSet(app_bits | extra.0).bits()
}

// ── CellRuntime ───────────────────────────────────────────────────────────────

/// Builder that arms the watchdog heartbeat and runs the cell event loop.
///
/// # Usage
/// ```no_run
/// CellRuntime::new()
///     .heartbeat(500)   // 5-second watchdog (500 × 10 ms ticks)
///     .run(my_handler);
/// ```
pub struct CellRuntime {
    heartbeat_ticks: u64,
}

impl CellRuntime {
    /// Create a builder with a 5-second heartbeat default (500 ticks × 10 ms).
    pub const fn new() -> Self {
        Self { heartbeat_ticks: 500 }
    }

    /// Override the watchdog interval.  `ticks` × 10 ms before the kernel kills
    /// a hung cell.  Pass `0` to disable the heartbeat.
    pub const fn heartbeat(mut self, ticks: u64) -> Self {
        self.heartbeat_ticks = ticks;
        self
    }

    /// Disable the watchdog heartbeat for this cell.
    pub const fn no_heartbeat(mut self) -> Self {
        self.heartbeat_ticks = 0;
        self
    }

    /// Arm the heartbeat, fire [`AppEvent::Init`] once, then run the event loop.
    ///
    /// Never returns.
    pub fn run(self, handler: impl FnMut(&mut AppContext, AppEvent)) -> ! {
        if self.heartbeat_ticks > 0 {
            crate::syscall::sys_heartbeat(self.heartbeat_ticks);
        }
        let mut ctx = AppContext::new();
        ctx.run_with_lifecycle(handler)
    }
}

impl Default for CellRuntime {
    fn default() -> Self { Self::new() }
}

// ── Entry-point macros ────────────────────────────────────────────────────────

/// Ergonomic cell entry point — generates `declare_manifest!`, syscall allowlist,
/// and `main()` from a capability declaration.
///
/// # Forms
/// ```no_run
/// // No caps (minimal app)
/// ostd::app_entry!(handler = my_fn);
///
/// // Single cap shorthands
/// ostd::app_entry!(spawn = true, handler = my_fn);
/// ostd::app_entry!(network = true, handler = my_fn);
/// ostd::app_entry!(block_io = true, handler = my_fn);
///
/// // Full explicit form
/// ostd::app_entry!(block_io = false, network = false, spawn = true, handler = my_fn);
/// ```
///
/// The generated `main()` arms the heartbeat watchdog and fires
/// [`AppEvent::Init`] once before entering the message loop.
#[macro_export]
macro_rules! app_entry {
    // Full explicit 3-cap form
    (
        block_io = $bio:literal,
        network  = $net:literal,
        spawn    = $spawn:literal,
        handler  = $handler:expr $(,)?
    ) => {
        api::declare_manifest!(block_io = $bio, network = $net, spawn = $spawn);

        #[used]
        #[link_section = "__ViCell_syscalls"]
        pub static VICELL_SYSCALLS: u64 =
            $crate::runtime::app_syscall_set($bio, $net, $spawn);

        #[no_mangle]
        pub fn main() {
            $crate::runtime::CellRuntime::new().run($handler);
        }
    };
    // Shorthand — only spawn
    (spawn = $spawn:literal, handler = $handler:expr $(,)?) => {
        $crate::app_entry!(block_io = false, network = false, spawn = $spawn, handler = $handler);
    };
    // Shorthand — only network
    (network = $net:literal, handler = $handler:expr $(,)?) => {
        $crate::app_entry!(block_io = false, network = $net, spawn = false, handler = $handler);
    };
    // Shorthand — only block_io
    (block_io = $bio:literal, handler = $handler:expr $(,)?) => {
        $crate::app_entry!(block_io = $bio, network = false, spawn = false, handler = $handler);
    };
    // No caps
    (handler = $handler:expr $(,)?) => {
        $crate::app_entry!(block_io = false, network = false, spawn = false, handler = $handler);
    };
}

/// Ergonomic service cell entry point — like [`app_entry!`] but uses the service
/// syscall profile (includes `WaitForEvent` in the base set).
///
/// # Forms
/// ```no_run
/// ostd::service_entry!(handler = my_fn);
/// ostd::service_entry!(network = true, handler = my_fn);
/// ostd::service_entry!(block_io = true, network = true, spawn = false, handler = my_fn);
/// ```
#[macro_export]
macro_rules! service_entry {
    // Full explicit 3-cap form
    (
        block_io = $bio:literal,
        network  = $net:literal,
        spawn    = $spawn:literal,
        handler  = $handler:expr $(,)?
    ) => {
        api::declare_manifest!(block_io = $bio, network = $net, spawn = $spawn);

        #[used]
        #[link_section = "__ViCell_syscalls"]
        pub static VICELL_SYSCALLS: u64 =
            $crate::runtime::service_syscall_set($bio, $net, $spawn);

        #[no_mangle]
        pub fn main() {
            $crate::runtime::CellRuntime::new().run($handler);
        }
    };
    // Shorthand — only network (most common for network services)
    (network = $net:literal, handler = $handler:expr $(,)?) => {
        $crate::service_entry!(block_io = false, network = $net, spawn = false, handler = $handler);
    };
    // Shorthand — only block_io (storage services)
    (block_io = $bio:literal, handler = $handler:expr $(,)?) => {
        $crate::service_entry!(block_io = $bio, network = false, spawn = false, handler = $handler);
    };
    // No extra caps
    (handler = $handler:expr $(,)?) => {
        $crate::service_entry!(block_io = false, network = false, spawn = false, handler = $handler);
    };
}
