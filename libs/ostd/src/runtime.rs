// SPDX-License-Identifier: MPL-2.0

//! ViCell Cell Runtime — ergonomic entry point and lifecycle management.
//!
//! [`CellRuntime`] is a builder that wires up the heartbeat watchdog and
//! optionally registers the cell as a named service before handing off to the
//! event loop.  It is the recommended way to start a cell.
//!
//! [`app_entry!`] is a declarative macro that generates
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

use crate::app::{AppContext, AppEvent};
use abi::syscall::{SyscallSet, ViSyscall};

// ── SyscallSet profiles ───────────────────────────────────────────────────────

/// Compute the syscall allowlist mask for a cell with the given capability profile.
///
/// This is a `const fn` so it can be used directly as a `static` initializer —
/// the compiler evaluates it at build time.
///
/// # Base set (all cells)
/// `Send, SendGather, Recv, TryRecv, Log, Heartbeat, LookupServiceBound, GetTime,
/// RecvTimeout` (`SendGather` backs the non-parking `post` and replies)
///
/// # Capability extras
/// - `block_io=true` → `GrantAlloc/Share/Slice/Free/Register/Unregister`
/// - `network=true`  → `WaitForEvent, GetRandom`
/// - `spawn=true`    → `SpawnFromPath/Mem/Pinned, Wait, OpenCap, ReadCap, CloseCap,
///                      GetProcs, Snapshot, StateStash/Restore`
pub const fn app_syscall_set(block_io: bool, network: bool, spawn: bool) -> u64 {
    let base = SyscallSet::EMPTY
        .with(ViSyscall::Send)
        .with(ViSyscall::SendGather)
        .with(ViSyscall::Recv)
        .with(ViSyscall::TryRecv)
        .with(ViSyscall::Log)
        .with(ViSyscall::Heartbeat)
        .with(ViSyscall::LookupServiceBound)
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
    } else {
        SyscallSet::EMPTY
    };

    let net_extra = if network {
        SyscallSet::EMPTY
            .with(ViSyscall::WaitForEvent)
            .with(ViSyscall::GetRandom)
    } else {
        SyscallSet::EMPTY
    };

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
            .with(ViSyscall::Snapshot)
            .with(ViSyscall::StateStash)
            .with(ViSyscall::StateRestore)
    } else {
        SyscallSet::EMPTY
    };

    SyscallSet(base.0 | bio_extra.0 | net_extra.0 | spawn_extra.0).bits()
}

/// Rich process telemetry stays opt-in.
///
/// `spawn = true` grants the legacy `GetProcs` (id/state/name only). `GetProcs2`
/// additionally exposes per-task CPU ticks and owned-memory footprint, which is
/// a side channel between Cells — it must be declared explicitly through
/// `declare_syscalls!`, never inherited from a coarse capability flag. A
/// compile-time assertion so no future edit to the sets above can regress it.
const _: () = {
    let bit = match ViSyscall::GetProcs2.allowlist_bit() {
        Some(bit) => bit,
        None => panic!("GetProcs2 must own an allowlist bit"),
    };
    assert!(
        app_syscall_set(true, true, true) & (1u64 << bit) == 0,
        "GetProcs2 must never be granted implicitly by app_syscall_set"
    );
};

/// Global allocator telemetry stays opt-in because it exposes cross-cell activity.
const _: () = {
    let bit = match ViSyscall::MemInfo.allowlist_bit() {
        Some(bit) => bit,
        None => panic!("MemInfo must own an allowlist bit"),
    };
    assert!(
        app_syscall_set(true, true, true) & (1u64 << bit) == 0,
        "MemInfo must never be granted implicitly by app_syscall_set"
    );
};

// ── CellRuntime ───────────────────────────────────────────────────────────────

/// Builder that arms the watchdog heartbeat and runs the cell event loop.
///
/// # Usage
/// ```no_run
/// use ostd::app::{AppContext, AppEvent};
/// use ostd::runtime::CellRuntime;
///
/// fn my_handler(_context: &mut AppContext, _event: AppEvent) {}
///
/// CellRuntime::new()
///     .heartbeat(500)             // 5-second watchdog (500 × 10 ms ticks)
///     .help("Usage: foo <file>")  // auto-handle -h / --help
///     .run(my_handler);
/// ```
pub struct CellRuntime {
    heartbeat_ticks: u64,
    help_text: Option<&'static str>,
}

impl CellRuntime {
    /// Create a builder with a 5-second heartbeat default (500 ticks × 10 ms).
    pub const fn new() -> Self {
        Self {
            heartbeat_ticks: 500,
            help_text: None,
        }
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

    /// Register a usage string for automatic `-h` / `--help` handling.
    ///
    /// If the cell is spawned with `-h` or `--help` in its args, `usage` is
    /// printed to the console and the cell exits before entering the event loop.
    /// Convenience alternative to calling [`ostd::args::check_help`] manually in
    /// the `AppEvent::Init` arm.
    pub const fn help(mut self, usage: &'static str) -> Self {
        self.help_text = Some(usage);
        self
    }

    /// Arm the heartbeat, check for `--help`, fire [`AppEvent::Init`] once, then
    /// run the event loop.
    ///
    /// Never returns.
    pub fn run(self, handler: impl FnMut(&mut AppContext, AppEvent)) -> ! {
        if let Some(usage) = self.help_text {
            crate::args::check_help(usage);
        }
        let mut ctx = AppContext::new();
        if self.heartbeat_ticks > 0 {
            // The event loop re-arms it on every iteration (see AppContext::run).
            ctx.arm_heartbeat(self.heartbeat_ticks);
        }
        ctx.run_with_lifecycle(handler)
    }
}

impl Default for CellRuntime {
    fn default() -> Self {
        Self::new()
    }
}

// ── Entry-point macros ────────────────────────────────────────────────────────

/// Ergonomic cell entry point — generates `declare_manifest!`, syscall allowlist,
/// and `main()` from a capability declaration.
///
/// # Forms
///
/// The capability-free form accepts `handler` with optional `help`. Capability
/// shorthands add one of `block_io`, `network`, or `spawn`. The full form
/// accepts all three capability booleans, with optional `help`.
///
/// # Example
/// ```no_run
/// use ostd::app::{AppContext, AppEvent};
///
/// fn my_handler(_context: &mut AppContext, _event: AppEvent) {}
///
/// ostd::app_entry!(network = true, handler = my_handler);
/// ```
///
/// The generated `main()` arms the heartbeat watchdog, checks for `-h`/`--help`
/// when a `help` string is provided, and fires [`AppEvent::Init`] once before
/// entering the message loop.
#[macro_export]
macro_rules! app_entry {
    // Full explicit 3-cap form
    (
        block_io = $bio:literal,
        network  = $net:literal,
        spawn    = $spawn:literal,
        handler  = $handler:expr $(,)?
    ) => {
        $crate::__declare_manifest!(block_io = $bio, network = $net, spawn = $spawn);

        #[used]
        #[link_section = "__ViCell_syscalls"]
        #[export_name = "__vicell_syscall_allowlist"]
        pub static VICELL_SYSCALLS: u64 = $crate::runtime::app_syscall_set($bio, $net, $spawn);

        // The attribute belongs to the bare-metal target only: a hosted test
        // build must get libtest's entry, not this one (see ostd::cell_main!).
        #[cfg_attr(not(any(test, unix, windows, target_family = "wasm")), no_mangle)]
        pub fn main() {
            $crate::runtime::CellRuntime::new().run($handler);
        }
    };
    // Full explicit 3-cap form with help
    (
        block_io = $bio:literal,
        network  = $net:literal,
        spawn    = $spawn:literal,
        help     = $help:literal,
        handler  = $handler:expr $(,)?
    ) => {
        $crate::__declare_manifest!(block_io = $bio, network = $net, spawn = $spawn);

        #[used]
        #[link_section = "__ViCell_syscalls"]
        #[export_name = "__vicell_syscall_allowlist"]
        pub static VICELL_SYSCALLS: u64 = $crate::runtime::app_syscall_set($bio, $net, $spawn);

        // The attribute belongs to the bare-metal target only: a hosted test
        // build must get libtest's entry, not this one (see ostd::cell_main!).
        #[cfg_attr(not(any(test, unix, windows, target_family = "wasm")), no_mangle)]
        pub fn main() {
            $crate::runtime::CellRuntime::new()
                .help($help)
                .run($handler);
        }
    };
    // Full explicit 3-cap form + explicit protection class (Manifest v2 opt-in)
    // — e.g. an FFI cell requesting
    // `tier = abi::manifest::PROTECTION_CLASS_FFI` for PKU key 2. Additive:
    // every pre-existing form keeps the default PROTECTION_CLASS_LEGACY.
    (
        block_io = $bio:literal,
        network  = $net:literal,
        spawn    = $spawn:literal,
        tier     = $tier:expr,
        handler  = $handler:expr $(,)?
    ) => {
        $crate::__declare_manifest!(
            block_io = $bio,
            network = $net,
            spawn = $spawn,
            tier = $tier
        );

        #[used]
        #[link_section = "__ViCell_syscalls"]
        #[export_name = "__vicell_syscall_allowlist"]
        pub static VICELL_SYSCALLS: u64 = $crate::runtime::app_syscall_set($bio, $net, $spawn);

        // The attribute belongs to the bare-metal target only: a hosted test
        // build must get libtest's entry, not this one (see ostd::cell_main!).
        #[cfg_attr(not(any(test, unix, windows, target_family = "wasm")), no_mangle)]
        pub fn main() {
            $crate::runtime::CellRuntime::new().run($handler);
        }
    };
    // Shorthand — only spawn
    (spawn = $spawn:literal, handler = $handler:expr $(,)?) => {
        $crate::app_entry!(
            block_io = false,
            network = false,
            spawn = $spawn,
            handler = $handler
        );
    };
    // Shorthand — only spawn with help
    (spawn = $spawn:literal, help = $help:literal, handler = $handler:expr $(,)?) => {
        $crate::app_entry!(
            block_io = false,
            network = false,
            spawn = $spawn,
            help = $help,
            handler = $handler
        );
    };
    // Shorthand — only network
    (network = $net:literal, handler = $handler:expr $(,)?) => {
        $crate::app_entry!(
            block_io = false,
            network = $net,
            spawn = false,
            handler = $handler
        );
    };
    // Shorthand — only network with help
    (network = $net:literal, help = $help:literal, handler = $handler:expr $(,)?) => {
        $crate::app_entry!(
            block_io = false,
            network = $net,
            spawn = false,
            help = $help,
            handler = $handler
        );
    };
    // Shorthand — only block_io
    (block_io = $bio:literal, handler = $handler:expr $(,)?) => {
        $crate::app_entry!(
            block_io = $bio,
            network = false,
            spawn = false,
            handler = $handler
        );
    };
    // Shorthand — only block_io with help
    (block_io = $bio:literal, help = $help:literal, handler = $handler:expr $(,)?) => {
        $crate::app_entry!(
            block_io = $bio,
            network = false,
            spawn = false,
            help = $help,
            handler = $handler
        );
    };
    // No caps with help
    (help = $help:literal, handler = $handler:expr $(,)?) => {
        $crate::app_entry!(
            block_io = false,
            network = false,
            spawn = false,
            help = $help,
            handler = $handler
        );
    };
    // No caps
    (handler = $handler:expr $(,)?) => {
        $crate::app_entry!(
            block_io = false,
            network = false,
            spawn = false,
            handler = $handler
        );
    };
}
