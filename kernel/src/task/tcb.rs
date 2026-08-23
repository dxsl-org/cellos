use super::pending_mailbox::PendingMailbox;
pub use super::pending_mailbox::PendingMsg;
use crate::hal::arch::Context;
use crate::hal::arch::ViTrapFrame;
use alloc::string::String;
use alloc::vec::Vec;
// use alloc::sync::Arc;
#[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
use alloc::sync::Arc;
use types::*;

use api::fs::{BoxFuture, FileResult};

/// Maximum number of IPC messages buffered for a Frozen cell during hot-swap.
///
/// When a cell is mid-swap (state `Frozen`), incoming `sys_send` calls enqueue
/// here instead of blocking the caller or returning an error.  If the buffer
/// fills, additional senders receive `SyscallError::TryAgain` so they can
/// back off.  Messages are drained to the new cell in Step 5 (UNFREEZE).
pub const HOTSWAP_MSG_QUEUE_DEPTH: usize = 64;

/// Deeper bound for nonblocking input-service events queued to a client.
///
/// Keyboard events use blocking `sys_send`, so they do not rely on this mailbox
/// as a paste reservoir. Nonblocking input-service traffic such as mouse events
/// may use this scheduling cushion while the target is briefly outside `Recv`.
/// Keeping the bound below the 4096-byte UART RX ring limits scheduler critical-
/// section work. All other `sys_try_send` callers keep strict drop-if-not-ready
/// semantics.
pub const INPUT_EVENT_QUEUE_DEPTH: usize = 512;

#[derive(Debug, Clone, PartialEq)]
pub enum TaskState {
    Ready,
    Running,
    /// Sleeping until a specific monotonic time (ticks/ms)
    Sleeping {
        until: usize,
    },
    /// Blocked waiting to send a message to `target_id`.
    /// Stores the message pointer and length temporarily.
    Sending {
        target: usize,
        msg_ptr: VAddr,
        msg_len: usize,
    },
    /// Blocked waiting to receive a message.
    ///
    /// `mask`: sender filter (0 = any sender).
    /// `deadline`: optional monotonic tick count after which the kernel wakes
    ///   this task with `ViError::Timeout`.  `None` = wait indefinitely.
    Recv {
        mask: usize,
        buf_ptr: VAddr,
        buf_len: usize,
        deadline: Option<u64>,
    },
    /// This task has finished running.
    Terminated,
    /// Removed from scheduling while its root generation waits for every hart
    /// to finish switching away. The task record remains dispatch-visible so a
    /// stale remote Context is denied rather than treated as an early-boot
    /// kernel caller.
    Retiring,
    /// Blocked on a Futex wait.
    /// `addr`: The address being waited on.
    FutexWait {
        addr: VAddr,
    },
    /// Waiting for another task to exit (Join).
    Waiting {
        target: usize,
    },
    /// Polling an async future (e.g. syscall)
    Polling,
    /// Blocked in `WaitForEvent(mask, timeout)`.  Woken by `waker::signal_net_rx()`
    /// (or equivalent) when any bit in `mask` fires, or when `deadline` ticks pass.
    /// `deadline = None` means block indefinitely.
    WaitEvent {
        mask: u32,
        deadline: Option<u64>,
    },
    /// Blocked in `WaitCompletion`. Source completions wake through the queue;
    /// a finite TIMER wakes when `deadline` elapses.
    WaitCompletion {
        source: u32,
        deadline: Option<u64>,
    },
    /// Cell is suspended during a hot-swap sequence identified by `swap_id`.
    ///
    /// Invariants while Frozen:
    /// - NOT in any scheduler ready queue.
    /// - Cannot be killed by external signal (returns `ViError::PermissionDenied`).
    /// - Can only be terminated by the hotswap orchestrator via `exit_task` after a
    ///   successful swap, or rolled back to `Ready` on failure.
    /// - Incoming IPC is queued in `crate::cell::hotswap::FROZEN` registry instead
    ///   of being delivered (Phase 02 will drain the queue; P01 stubs this).
    Frozen {
        swap_id: u64,
    },
    /// Blocked in `sys_wait_irq(irq, mmio_base)` until hardware IRQ `irq` fires.
    ///
    /// Woken by the scheduler sweep (NOT from ISR — ISR only sets `IRQ_PENDING[irq]`).
    /// No deadline: IRQ-only wake; Driver Cells must implement their own poll-fallback
    /// if the device can hang without raising an IRQ.
    WaitIrq {
        irq: u8,
    },
}

/// The address-space binding is fixed before a task can enter a ready queue.
///
/// Native domains are deliberately unavailable on every non-RV64 target and
/// default to SAS until a test-only fixture binds a private root.
#[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
#[derive(Clone)]
pub(crate) enum TaskAddressSpace {
    Sas,
    #[cfg_attr(
        not(feature = "test-hooks"),
        expect(
            dead_code,
            reason = "domain admission is intentionally unavailable outside native-domain test hooks"
        )
    )]
    Domain(Arc<crate::memory::address_space::AddressSpace>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LeaseAttributes(pub u32);

impl LeaseAttributes {
    pub const READ: Self = Self(1 << 0);
    pub const WRITE: Self = Self(1 << 1);

    pub fn contains(&self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }
}

#[derive(Debug, Clone)]
pub struct Lease {
    pub id: usize,  // Logic Lease ID (Index)
    pub ptr: VAddr, // Address in Task's space
    pub len: usize, // Length
    pub attributes: LeaseAttributes,
}

#[derive(Debug, Clone)]
pub struct GrantEntry {
    pub ptr: VAddr,
    pub len: usize,
    pub flags: u32,
    pub sender_id: usize,
}

// File Handle for Stateful IO
pub use api::fs::FileHandle;

/// Enum to hold the different types of futures a task might be waiting on.
pub enum SyscallFuture {
    FileRead(usize, BoxFuture<'static, FileResult<usize>>), // fd, future
                                                            // Add other syscall futures here (FileWrite, Connect, etc.)
}

/// Task Control Block (TCB)
#[allow(dead_code)]
pub struct Task {
    pub id: usize,
    pub cell_id: CellId, // OWNER CELL
    /// Immutable root task for this Cell generation. A root starts as its own
    /// endpoint; `spawn_thread` replaces this with the registry-attested root.
    pub root_tid: usize,
    pub name: String,
    pub state: TaskState,
    pub context: Context,
    /// Immutable scheduler dispatch binding; no loader or syscall may replace it.
    #[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
    pub(crate) address_space: TaskAddressSpace,
    pub trap_frame: ViTrapFrame,
    pub allowed_drivers: Vec<usize>,
    // Maps LeaseID -> Lease
    pub leases: alloc::collections::BTreeMap<usize, Lease>,
    // Next available Lease ID
    pub next_lease_id: usize,

    // Grant Table (Zero-Copy IPC)
    // Maps GrantID -> GrantEntry
    pub grant_table: alloc::collections::BTreeMap<usize, GrantEntry>,
    pub next_grant_id: usize,

    // Maps FD -> FileHandle
    pub open_files: alloc::collections::BTreeMap<usize, FileHandle>,
    // The Task ID that this task is currently handling a request FROM (for Reply).
    pub current_caller: Option<usize>,
    /// Owning cell task of `current_caller`, captured when the request is
    /// delivered so service-side lifecycle checks do not depend on the sender
    /// still being alive when they run.
    pub current_caller_cell_id: u64,
    /// Cell generation of `current_caller`, captured with the request.
    pub current_caller_cell_generation: u64,
    /// Monotonic per-task generation for the request currently in
    /// `current_caller`. Lets the kernel reject stale completion paths.
    pub current_caller_request_generation: u64,
    /// Next request generation to assign when this task accepts a sender.
    next_caller_request_generation: u64,
    // Last Reply Value received
    pub reply_value: Option<usize>,
    // Current Working Directory
    pub cwd: String,
    // Stack management
    pub kernel_stack: Option<super::stack::Stack>,
    pub user_stack: Option<super::stack::Stack>,
    /// Frames mapped for this cell's ELF segments, freed when the Task is dropped
    /// (reaped). Without it, a cell's code/data frames leak on every death.
    pub segment_mem: Option<super::stack::CellSegments>,

    /// Bytes of stack charged to `cell_id`'s memory quota on this task's behalf,
    /// `0` if nothing was charged.
    ///
    /// Only thread tasks carry a charge: a cell's own stacks are part of the cost
    /// of admitting the cell, whereas a thread is a cost the cell chose at runtime
    /// and can choose repeatedly, so it has to appear in the same ledger as the
    /// cell's heap or concurrency becomes the one free way to grow a footprint.
    ///
    /// Released exactly once, in `Scheduler::exit_task` — the funnel every death
    /// path passes through — by taking the field, so a double call cannot
    /// double-refund. Reaping is too late: a zombie can sit unreaped indefinitely
    /// and the cell would be billed for a thread that is already gone.
    pub stack_quota_charge: usize,

    // Lifecycle
    pub waiters: Vec<usize>,
    pub exit_code: Option<usize>,
    /// Death-notification queue (NotifyOnExit): tids of watched tasks that died
    /// while this watcher was NOT parked in Recv. Drained by the next `Recv`
    /// regardless of mask, preserving the public NotifyOnExit contract.
    pub pending_deaths: Vec<(usize, usize)>,
    /// VFS root-owner death events, kept separate from generic task watches.
    ///
    /// The opaque watch token never crosses the syscall ABI: it binds this queued
    /// event to the exact subscription so a stale cancellation cannot affect a
    /// successor. These events are delivered only by a wildcard public receive;
    /// a masked backend receive must never consume them.
    pub pending_owner_deaths: Vec<(u64, usize, usize)>,

    /// Exit reason for a generic `NotifyOnExit` wake delivered while this task
    /// was parked in `Recv`. Owner-watch wakes use `pending_owner_deaths` instead.
    pub pending_exit_reason: Option<usize>,

    // Async Kernel Support
    pub pending_future: Option<SyscallFuture>,

    /// Raw block-device access (BlkRead/BlkWrite/BlkFlush).  Granted at spawn for `/bin/vfs`.
    pub block_io_cap: Option<super::cap::BlockIoCap>,
    /// Network transmit/receive (NetTx/NetRx).  Granted at spawn for `/bin/net`.
    pub network_cap: Option<super::cap::NetworkCap>,
    /// Cell spawning + hot-swap (SpawnFromPath/SpawnPinned/HotSwap).
    /// Granted at spawn for `/bin/init` and `/bin/shell`.
    pub spawn_cap: Option<super::cap::SpawnCap>,
    /// RISC-V H-extension CSR access for VMM cells.
    /// Granted when manifest declares `hypervisor = true` AND the firmware reported H-ext.
    pub hypervisor_cap: Option<super::cap::HypervisorCap>,
    /// Supervisor authority: sys_freeze_cell / sys_resume_cell / sys_kill_cell.
    /// Set ONLY by kernel init (direct TCB write). Never propagated through CapSet.
    pub supervisor_cap: Option<super::cap::SupervisorCap>,
    /// PCIe Driver Cell: claim BAR MMIO + authorise DMA via GrantDma.
    /// Granted when manifest declares `pcie_driver = true`.
    pub pcie_driver_cap: Option<super::cap::PcieDriverCap>,
    /// Platform Cell: singleton capability gating `sys_register_pcie_bar`.
    /// Granted by path match `/bin/platform` in loader.rs; at most one holder ever.
    pub platform_cap: Option<super::cap::PlatformCap>,

    /// MMIO device-class capability bitmask (`DEV_*` from
    /// [`crate::resource_registry`]). Set from the ELF manifest's hardware flags.
    /// A cell may `sys_request_mmio` only ranges whose matching device class is
    /// present here. `0` = no MMIO access.
    pub mmio_devices: u8,

    /// Block-I/O partition range grants (Milestone 2.5 P03) — bitmask:
    /// bit 0 = P1 FAT32 (`MANIFEST_FLAG_PART_DATA`), bit 1 = P4 littlefs
    /// (`MANIFEST_FLAG_PART_LFS`). Checked by `check_block_access` for every
    /// raw block syscall, on top of `block_io_cap`. P2 (cell table) and P3
    /// (snapshot) have no bit — those ranges are kernel-only by construction.
    pub block_regions: u8,

    /// x86_64 PKU protection key for this cell's ELF pages (0=trusted, 1=standard, 2=ffi).
    /// On other architectures this field exists but is always 0 and never consulted.
    /// Key assignment: 0 = trusted-core cells (block_io/network/spawn/hypervisor),
    ///                 1 = standard Tier-1 Rust cells,
    ///                 2 = Tier-1b C/FFI cells (mlibc, DOOM).
    pub pku_key: u8,

    /// Precomputed PKRU register value for this cell's key domain.
    /// Written to CPU_LOCAL.pku_value by the scheduler before ring-3 re-entry,
    /// then loaded into PKRU by the asm exit paths (`__trap_exit` / `syscall_entry`).
    /// Always 0 on non-x86_64 targets (and for trusted-core cells on x86_64).
    pub pku_value: u32,

    /// Scheduling priority tier.  Higher value = higher priority.
    /// See `api::TaskPriority` for the three defined levels.
    pub priority: u8,

    /// Cluster participation mode parsed from `__ViCell_cluster` ELF section.
    /// `0` = Isolated (default, no section present).  See `api::cluster::ClusterMode`.
    ///
    /// Invariant: a task with `cluster_mode != 0` MUST have `priority != RealTime (2)`.
    /// The `SpawnPinned` handler enforces this at spawn time.
    pub cluster_mode: u8,

    /// FNV-1a-64 cluster routing identifier from `__ViCell_cluster`.
    /// `0` when `cluster_mode == Isolated`.  NOT a credential — routing only.
    pub cluster_id: u64,

    /// Per-Cell syscall allowlist.  Each bit corresponds to a syscall via
    /// `api::ViSyscall::allowlist_bit()`.  `u64::MAX` = permit all (default,
    /// used when the Cell ELF does not embed a `__ViCell_syscalls` section).
    pub syscall_allowlist: u64,

    /// Watchdog: consecutive 10 ms scheduler ticks this task has been Running
    /// WITHOUT voluntarily blocking. Incremented each tick it is found Running in
    /// `pick_next`, reset to 0 the moment it blocks (Recv/Send/Sleep/etc). A
    /// runaway (infinite loop, never yields) climbs until it crosses the watchdog
    /// budget and is terminated — preventing livelock ("alive but paralyzed").
    pub run_ticks: u32,

    /// Cumulative scheduler ticks charged to this task while it was the running
    /// task at a scheduler accounting point. This is the kernel-exported CPU
    /// sample source for `GetProcs2`; userspace computes percentages from deltas.
    pub cpu_run_ticks: u64,

    /// Cumulative count of `RecvTimeout` deadlines this task has missed (the awaited
    /// message did not arrive in time). For an RT control loop this is its missed-cycle
    /// count. Observability only — surfaced via the audit ring ([`crate::audit`]); the
    /// scheduler does not act on it (RT enforcement is hardware-data-gated).
    pub deadline_misses: u32,

    /// One-shot latch: set when this task has already emitted an `RtCpuOverrun` warning
    /// for the current non-yielding episode, so the early-warning audit fires once per
    /// episode (not every tick). Reset to false whenever the task voluntarily blocks.
    pub rt_overrun_warned: bool,

    /// Liveness-heartbeat deadline (absolute `system_ticks`). `Some(d)` means the cell
    /// opted into heartbeating and must call `Heartbeat` again before tick `d`, else the
    /// scheduler terminates it as HUNG (silent-hang detection — see `pick_next`). `None`
    /// = heartbeat disabled (the default; most cells don't opt in).
    pub heartbeat_deadline: Option<u64>,

    /// Per-cell virtual memory area list for demand-paging.
    ///
    /// Populated by the ELF loader (Phase 04) with one entry per ELF segment.
    /// The #PF handler consults this list when a user-mode page fault occurs to
    /// decide whether to map the page on demand or kill the cell.
    /// Empty until the ELF loader runs; demand-paging is inert on RISC-V/AArch64
    /// (those arches use identity-mapped segments today).
    pub vma: crate::memory::vma::VmaList,

    /// Set by `HotSwapReady` (syscall 401) to signal that the new cell has
    /// finished deserializing state and is ready to receive IPC.
    /// Cleared to `false` at spawn; polled by `wait_for_hotswap_ready`.
    pub hotswap_ready: bool,

    /// Frozen source task this replacement was minted from. Set by the
    /// SpawnReplacement syscall and consumed only by the atomic cutover.
    pub hotswap_source_tid: Option<usize>,

    /// Permanently rejects non-supervisor IPC to an old provider after its
    /// mailbox and service identity have moved to a replacement.
    pub hotswap_ingress_closed: bool,

    /// When `true`, `FreezeCell` and `KillCell` syscalls are rejected with
    /// `PermissionDenied`.  Set at spawn for `init` and kernel-owned cells.
    /// Prevents a compromised Supervisor Cell from disabling the restart tree.
    pub is_critical: bool,

    /// IPC messages buffered while this task is in `TaskState::Frozen` (hot-swap).
    ///
    /// Callers that `sys_send` to a Frozen task have their message copied here
    /// (owned buffer; Law 2) and return immediately with `Ok(0)` — they are NOT
    /// blocked in `Sending` state.  Step 5 of the hotswap orchestrator drains
    /// this queue to the new cell before the old cell is terminated.
    /// Bounded by `HOTSWAP_MSG_QUEUE_DEPTH`; overflow returns `TryAgain`.
    pub pending_msgs: PendingMailbox,

    /// Epoch of the cell this task belongs to, attested to services alongside
    /// `cell_id` (see `api::caller_identity`).
    ///
    /// A service that holds state against a `CellId` — an open handle, a pending
    /// read, a quota ledger row — must not hand that state to a *different* cell
    /// that later shows up under the same id. `CellId` is `CellId(tid)` and the
    /// scheduler's `next_task_id` only ever increments, so ids are not recycled
    /// today; this epoch is what keeps the guarantee if that ever changes,
    /// because it is minted fresh per cell and never reused.
    ///
    /// Threads override the freshly minted value with their cell's (see
    /// `Scheduler::spawn_thread`) so a thread is indistinguishable from its cell
    /// to any check that keys on identity — which is the point of the whole
    /// attestation: `CellId(tid)` was misattributing threads.
    pub cell_generation: u64,

    /// Directory handles this task's spawner named for it, with the spawner's
    /// attested identity (see [`api::dir_handles`]).
    ///
    /// The kernel is a courier here and nothing more. It does not know what any
    /// of these handles refer to, cannot tell a live one from a revoked one, and
    /// must never treat this as a second record of filesystem authority — the
    /// filesystem service owns that, and a second copy would drift in the
    /// direction of silently widening what a cell may reach. The only claim the
    /// kernel makes about this field is provenance: *this* spawner named *these*
    /// values at spawn.
    ///
    /// Bounded inline at [`api::dir_handles::MAX_SPAWN_DIR_HANDLES`], so a
    /// caller-supplied count never sizes an allocation.
    ///
    /// Set once, under the scheduler lock that creates the task, before it can
    /// be scheduled. It is never updated afterwards: authority fixed at creation
    /// is what makes it auditable.
    pub inherited_dirs: api::dir_handles::InheritedDirHandles,

    /// Directory handles this task has named for the next cell it spawns.
    ///
    /// Staged by `SpawnSetDirs` and consumed by the next cell this task creates,
    /// then cleared — including when that spawn fails, so a set can never attach
    /// to a later unrelated child. Empty means "pass nothing on", which is what
    /// every task that never calls `SpawnSetDirs` does.
    pub staged_dirs: api::dir_handles::DirHandleSet,

    /// This cell's completion queue, or `None` until something reserves a slot.
    ///
    /// Kernel-owned heap memory, never a grant: a cell cannot unregister or free
    /// it, so a completion always has somewhere to land and appending needs no
    /// address resolution. Threads of one cell share the handle, and the queue
    /// dies with the last reference to it — see [`crate::task::completion`].
    pub completion: Option<alloc::sync::Arc<crate::task::completion::CompletionQueue>>,
    /// Queue slot held by an in-progress `WaitCompletion` syscall.
    ///
    /// Kept outside `TaskState` because a deferred source wake changes the state
    /// to `Ready` before the syscall has released its reservation. Task exit
    /// takes this field and defers TIMER release outside the scheduler lock.
    pub completion_wait: Option<CompletionWait>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompletionWait {
    pub source: u32,
    pub slot: crate::task::completion::SlotId,
}

/// Source of [`Task::cell_generation`]. Starts at 1 so 0 stays available as
/// "generation not attested on this path".
/// A real spinlock keeps allocation unique across harts, including RV32 where
/// the portable 64-bit atomic fallback only masks local interrupts.
static NEXT_CELL_GENERATION: crate::sync::Spinlock<u64> = crate::sync::Spinlock::new(1);

fn next_cell_generation() -> u64 {
    let mut next = NEXT_CELL_GENERATION.lock();
    let generation = *next;
    *next = generation
        .checked_add(1)
        .expect("cell generation space exhausted");
    generation
}

#[cfg(feature = "test-hooks")]
pub(crate) fn cell_generation_snapshot() -> u64 {
    *NEXT_CELL_GENERATION.lock()
}

#[cfg(feature = "test-hooks")]
pub(crate) fn restore_cell_generation_for_test(next: u64) {
    *NEXT_CELL_GENERATION.lock() = next;
}

impl Task {
    pub fn new(id: usize, cell_id: CellId, name: &str, allowed_drivers: Vec<usize>) -> Self {
        Self {
            id,
            cell_id,
            root_tid: id,
            name: String::from(name),
            state: TaskState::Ready,
            context: Context::default(),
            #[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
            address_space: TaskAddressSpace::Sas,
            trap_frame: ViTrapFrame::default(),
            allowed_drivers,
            leases: alloc::collections::BTreeMap::new(),
            next_lease_id: 1, // Start efficiently
            grant_table: alloc::collections::BTreeMap::new(),
            next_grant_id: 1,
            open_files: alloc::collections::BTreeMap::new(),
            current_caller: None,
            current_caller_cell_id: 0,
            current_caller_cell_generation: 0,
            current_caller_request_generation: 0,
            next_caller_request_generation: 1,
            reply_value: None,
            cwd: String::from("/"),
            kernel_stack: None,
            user_stack: None,
            segment_mem: None,
            stack_quota_charge: 0,
            waiters: Vec::new(),
            exit_code: None,
            pending_deaths: Vec::new(),
            pending_owner_deaths: Vec::new(),
            pending_exit_reason: None,
            pending_future: None,
            block_io_cap: None,
            network_cap: None,
            spawn_cap: None,
            hypervisor_cap: None,
            supervisor_cap: None,
            pcie_driver_cap: None,
            platform_cap: None,
            mmio_devices: 0,
            block_regions: 0,
            pku_key: 0,
            pku_value: 0,
            priority: api::TaskPriority::Normal as u8,
            cluster_mode: 0,
            cluster_id: 0,
            syscall_allowlist: u64::MAX, // permit-all until ELF section is read
            run_ticks: 0,
            cpu_run_ticks: 0,
            deadline_misses: 0,
            rt_overrun_warned: false,
            heartbeat_deadline: None,
            vma: crate::memory::vma::VmaList::new(),
            hotswap_ready: false,
            hotswap_source_tid: None,
            hotswap_ingress_closed: false,
            is_critical: false,
            pending_msgs: PendingMailbox::new(),
            cell_generation: next_cell_generation(),
            inherited_dirs: api::dir_handles::InheritedDirHandles::NONE,
            staged_dirs: api::dir_handles::DirHandleSet::EMPTY,
            completion: None,
            completion_wait: None,
        }
    }

    #[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
    #[cfg(feature = "test-hooks")]
    pub(crate) fn bind_address_space_for_test(
        &mut self,
        address_space: Arc<crate::memory::address_space::AddressSpace>,
    ) {
        assert!(matches!(self.address_space, TaskAddressSpace::Sas));
        self.address_space = TaskAddressSpace::Domain(address_space);
    }

    #[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
    pub(crate) fn address_space_is_live(&self) -> bool {
        match &self.address_space {
            TaskAddressSpace::Sas => true,
            TaskAddressSpace::Domain(space) => space.is_live(),
        }
    }

    pub fn add_lease(&mut self, ptr: VAddr, len: usize, attributes: LeaseAttributes) -> usize {
        let id = self.next_lease_id;
        self.next_lease_id += 1;

        let lease = Lease {
            id,
            ptr,
            len,
            attributes,
        };

        self.leases.insert(id, lease);
        id
    }

    pub fn get_lease(&self, id: usize) -> Option<&Lease> {
        self.leases.get(&id)
    }

    pub fn revoke_lease(&mut self, id: usize) {
        self.leases.remove(&id);
    }

    // --- Grant Table Methods ---
    pub fn add_grant(&mut self, ptr: VAddr, len: usize, flags: u32, sender_id: usize) -> usize {
        let id = self.next_grant_id;
        self.next_grant_id += 1;
        self.grant_table.insert(
            id,
            GrantEntry {
                ptr,
                len,
                flags,
                sender_id,
            },
        );
        id
    }

    pub fn get_grant(&self, id: usize) -> Option<&GrantEntry> {
        self.grant_table.get(&id)
    }

    pub fn remove_grant(&mut self, id: usize) -> Option<GrantEntry> {
        self.grant_table.remove(&id)
    }

    pub fn set_current_caller_context(
        &mut self,
        sender_tid: usize,
        sender_cell_id: u64,
        sender_generation: u64,
    ) {
        self.current_caller = Some(sender_tid);
        self.current_caller_cell_id = sender_cell_id;
        self.current_caller_cell_generation = sender_generation;
        self.current_caller_request_generation = self.next_caller_request_generation;
        self.next_caller_request_generation = self.next_caller_request_generation.saturating_add(1);
    }

    pub fn set_received_caller_context(
        &mut self,
        sender_tid: usize,
        sender_cell_id: u64,
        sender_generation: u64,
    ) {
        // VFS may perform nested IPC while serving a request. Its outer caller
        // remains the authority for grants and owner-death watches until VFS
        // sends that caller's response; a storage reply must not replace it.
        if crate::fast_ipc::is_registered_vfs_cell(self.cell_id.0 as usize)
            && self.current_caller.is_some()
        {
            return;
        }
        self.set_current_caller_context(sender_tid, sender_cell_id, sender_generation);
    }

    /// Drop VFS's public request context and return the exact lease identity
    /// that must be released after the scheduler lock is dropped.
    pub fn begin_receive_context(&mut self, mask: usize) -> Option<(usize, u64)> {
        // VFS uses a wildcard receive only at its public request loop. Masked
        // receives are nested dependency replies and keep the outer authority.
        if mask == 0 && crate::fast_ipc::is_registered_vfs_cell(self.cell_id.0 as usize) {
            let dropped = self
                .current_caller
                .map(|grant_owner| (grant_owner, self.current_caller_request_generation));
            self.clear_current_caller_context();
            return dropped;
        }
        None
    }

    /// Whether a receive mask may dequeue a tokenized VFS owner-death event.
    ///
    /// Owner death is routed on VFS's public plane only. Its root TID and
    /// subscription token identify the event after this predicate succeeds;
    /// neither is a sender match and therefore neither can authorize a masked
    /// backend receive.
    #[inline]
    pub(super) const fn owner_death_matches_receive_mask(mask: usize) -> bool {
        mask == 0
    }

    pub fn clear_current_caller_context(&mut self) {
        self.current_caller = None;
        self.current_caller_cell_id = 0;
        self.current_caller_cell_generation = 0;
        self.current_caller_request_generation = 0;
    }

    pub fn clear_current_caller_context_if(
        &mut self,
        sender_tid: usize,
        request_generation: u64,
    ) -> bool {
        if self.current_caller == Some(sender_tid)
            && self.current_caller_request_generation == request_generation
        {
            self.clear_current_caller_context();
            return true;
        }
        false
    }
}
