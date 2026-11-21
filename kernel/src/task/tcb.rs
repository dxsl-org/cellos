use crate::hal::arch::Context;
use crate::hal::arch::ViTrapFrame;
use crate::ipc::pending_mailbox::PendingMailbox;
pub use crate::ipc::pending_mailbox::PendingMsg;
use alloc::string::String;
use alloc::vec::Vec;
// use alloc::sync::Arc;
#[cfg(all(feature = "native-domains", cellos_wide))]
use alloc::sync::Arc;
use types::*;

/// Maximum number of IPC messages buffered for a Frozen cell during hot-swap.
///
/// When a cell is mid-swap (state `Frozen`), incoming `sys_send` calls enqueue
/// here instead of blocking the caller or returning an error.  If the buffer
/// fills, additional senders receive `SyscallError::TryAgain` so they can
/// back off.  Messages are drained to the new cell in Step 5 (UNFREEZE).
pub const HOTSWAP_MSG_QUEUE_DEPTH: usize = 64;

/// Most messages one Cell may have queued in one target's mailbox by `post`
/// (`SendGather`): a quarter of [`HOTSWAP_MSG_QUEUE_DEPTH`], so one busy
/// poster leaves room for other clients' requests and events.
pub const POST_QUOTA_PER_SENDER: usize = 16;

/// Deeper bound for nonblocking input-service events queued to a client.
///
/// Keyboard events use blocking `sys_send`, so they do not rely on this mailbox
/// as a paste reservoir. Nonblocking input-service traffic such as mouse events
/// may use this scheduling cushion while the target is briefly outside `Recv`.
/// Keeping the bound below the 4096-byte UART RX ring limits scheduler critical-
/// section work. All other `sys_try_send` callers keep strict drop-if-not-ready
/// semantics.
pub const INPUT_EVENT_QUEUE_DEPTH: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Ready,
    Running,
    /// Parked until `wait` is satisfied or, when set, `deadline` (absolute
    /// `system_ticks`) passes. Every parked shape is one of these: the timer
    /// sweep keys on `deadline` and the event sweep on [`WaitKind::Completion`].
    Blocked {
        wait: WaitKind,
        deadline: Option<u64>,
    },
    /// This task has finished running.
    Terminated,
    /// Removed from scheduling while its root generation waits for every hart
    /// to finish switching away. The task record remains dispatch-visible so a
    /// stale remote Context is denied rather than treated as an early-boot
    /// kernel caller.
    Retiring,
    /// Cell is suspended during a hot-swap sequence identified by `swap_id`.
    ///
    /// Invariants while Frozen:
    /// - NOT in any scheduler ready queue.
    /// - Cannot be killed by external signal (returns `ViError::PermissionDenied`).
    /// - Can only be terminated by the hotswap orchestrator via `exit_task` after a
    ///   successful swap, or rolled back to `Ready` on failure.
    /// - Accepted incoming IPC is retained in this task's pending mailbox and
    ///   transferred only by the live cutover barrier to the replacement.
    Frozen {
        swap_id: u64,
    },
}

/// What a [`TaskState::Blocked`] task waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitKind {
    /// Only the deadline (`Sleep`).
    Sleep,
    /// The receiver to consume the message published under `delivery_id`. The
    /// payload lives in the receiver's kernel-owned mailbox; this holds only
    /// the delivery token.
    Send { target: usize, delivery_id: u64 },
    /// A message. `mask`: sender filter (0 = any sender). On the deadline the
    /// kernel wakes the task with a 0 sender.
    Recv {
        mask: usize,
        buf_ptr: VAddr,
        buf_len: usize,
    },
    /// A pipe read (ring empty, a writer end still open).
    PipeRead {
        handle: crate::ipc::pipe::PipeHandle,
    },
    /// A pipe write (ring full).
    PipeWrite {
        handle: crate::ipc::pipe::PipeHandle,
    },
    /// A futex wake. `key`: `(address-space identity, generation, word
    /// address)` — see [`crate::ipc::futex`] for why the identity is part of it.
    Futex { key: crate::ipc::futex::FutexKey },
    /// Another task's exit (Join).
    Join { target: usize },
    /// A completion source: `source` is a non-empty subset of
    /// `NET_RX | TIMER | IPC | IRQ`. IPC terminals wake it through
    /// `async_ipc::wake`, a pending IRQ line through the event sweep, and the
    /// deadline through the timer sweep. `delivery` says how the wait reports
    /// what woke it; the legacy `WaitForEvent`, `IpcWait` and `WaitIrq` calls
    /// are this wait with their own delivery (CP1 D2/D3).
    Completion { source: u32, delivery: Delivery },
}

/// How a [`WaitKind::Completion`] wait reports its result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// `WaitCompletion`: a record in the reserved slot of the cell's queue.
    /// NET_RX completes that reservation; IRQ lines come from
    /// [`CompletionWait::irq_lines`].
    Queue,
    /// `WaitForEvent(mask)`: the event sweep consumes the fired `mask` bits of
    /// the NET_RX level flag into the return register (no reservation).
    EventMask(u32),
    /// `IpcWait`: the caller checks for a terminal operation after the wake.
    IpcTerminal,
    /// `WaitIrq(line)`: the event sweep consumes `line` once it is pending,
    /// clears its waiter slot and readies the task. No deadline.
    IrqLine(u8),
}

impl TaskState {
    /// Absolute `system_ticks` at which the timer sweep must look at a task
    /// parked in this state, if it has a deadline.
    pub(crate) fn wake_deadline(&self) -> Option<u64> {
        match self {
            TaskState::Blocked { deadline, .. } => *deadline,
            _ => None,
        }
    }

    /// Whether the sweep must poll a task parked in this state for an event
    /// (an IRQ, NET_RX, or a completion source) on every pass.
    ///
    /// `IpcWait` is woken by the IPC terminal itself and needs no polling.
    pub(crate) fn waits_for_events(&self) -> bool {
        matches!(
            self,
            TaskState::Blocked {
                wait: WaitKind::Completion { delivery, .. },
                ..
            } if *delivery != Delivery::IpcTerminal
        )
    }
}

/// Short name for a task state, for admission-failure diagnostics.
///
/// The scheduler already has `TaskState` as a value everywhere an admission is
/// decided; a name is what a log line needs, and `Debug` would print the whole
/// payload (buffer pointers, deadlines) into a console line.
pub fn task_state_name(state: &TaskState) -> &'static str {
    match state {
        TaskState::Ready => "ready",
        TaskState::Running => "running",
        TaskState::Blocked { wait, .. } => match wait {
            WaitKind::Sleep => "sleeping",
            WaitKind::Send { .. } => "sending",
            WaitKind::Recv { .. } => "recv",
            WaitKind::PipeRead { .. } => "pipe-read",
            WaitKind::PipeWrite { .. } => "pipe-write",
            WaitKind::Futex { .. } => "futex-wait",
            WaitKind::Join { .. } => "waiting",
            WaitKind::Completion { delivery, .. } => match delivery {
                Delivery::Queue => "wait-completion",
                Delivery::EventMask(_) => "wait-event",
                Delivery::IpcTerminal => "wait-ipc",
                Delivery::IrqLine(_) => "wait-irq",
            },
        },
        TaskState::Terminated => "terminated",
        TaskState::Retiring => "retiring",
        TaskState::Frozen { .. } => "frozen",
    }
}

/// The address-space binding is fixed before a task can enter a ready queue.
///
/// Native domains are deliberately unavailable on every non-RV64 target and
/// default to SAS until a test-only fixture binds a private root.
#[cfg(all(feature = "native-domains", cellos_wide))]
#[allow(dead_code)]
#[derive(Clone)]
pub(crate) enum TaskAddressSpace {
    Sas,
    Domain(Arc<crate::memory::address_space::AddressSpace>),
}

// File Handle for Stateful IO
pub use abi::fs::FileHandle;

/// The request a service task is currently serving (Task field group).
pub struct CallerSlot {
    /// The Task ID that this task is currently handling a request FROM (for Reply).
    pub current: Option<usize>,
    /// Owning cell task of `current`, captured when the request is delivered so
    /// service-side lifecycle checks do not depend on the sender still being
    /// alive when they run.
    pub cell_id: u64,
    /// Cell generation of `current`, captured with the request.
    pub cell_generation: u64,
    /// Monotonic per-task generation for the request currently in `current`.
    /// Lets the kernel reject stale completion paths.
    pub request_generation: u64,
    /// Next request generation to assign when this task accepts a sender.
    next_request_generation: u64,
}

impl CallerSlot {
    const fn new() -> Self {
        Self {
            current: None,
            cell_id: 0,
            cell_generation: 0,
            request_generation: 0,
            next_request_generation: 1,
        }
    }
}

/// Scheduler accounting kept per task (Task field group).
#[derive(Default)]
pub struct SchedStats {
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
    /// Exact on-CPU nanoseconds on the `GetTime(0)` monotonic clock (Phase 06
    /// S6-1, K1). Charged at every scheduler pass on this task's hart for the
    /// interval since that hart's previous pass; never reset within the task's
    /// lifetime. `TelemetrySnapshot` adds the still-running interval in its row.
    pub cpu_ns: u64,
    /// Cumulative count of `RecvTimeout` deadlines this task has missed (the awaited
    /// message did not arrive in time). For an RT control loop this is its missed-cycle
    /// count. Observability only — surfaced via the audit ring ([`crate::audit`]); the
    /// scheduler does not act on it (RT enforcement is hardware-data-gated).
    pub deadline_misses: u32,
    /// One-shot latch: set when this task has already emitted an `RtCpuOverrun` warning
    /// for the current non-yielding episode, so the early-warning audit fires once per
    /// episode (not every tick). Reset to false whenever the task voluntarily blocks.
    pub rt_overrun_warned: bool,
}

/// Hot-swap replacement state (Task field group).
#[derive(Default)]
pub struct HotswapState {
    /// Set by `HotSwapReady` (syscall 401) to signal that the new cell has
    /// finished deserializing state and is ready to receive IPC.
    /// Cleared to `false` at spawn; polled by `wait_for_hotswap_ready`.
    pub ready: bool,
    /// Frozen source task this replacement was minted from. Set by the
    /// SpawnReplacement syscall and consumed only by the atomic cutover.
    pub source_tid: Option<usize>,
    /// Permanently rejects non-supervisor IPC to an old provider after its
    /// mailbox and service identity have moved to a replacement.
    pub ingress_closed: bool,
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
    #[cfg(all(feature = "native-domains", cellos_wide))]
    pub(crate) address_space: TaskAddressSpace,
    /// Whether this worker's stacks were dynamically inserted into its domain.
    #[cfg(all(feature = "native-domains", cellos_wide))]
    pub(crate) has_dynamic_stack_mapping: bool,
    pub trap_frame: ViTrapFrame,
    /// Pipe endpoints this task owns. A handle is usable by exactly the task that
    /// holds it; `PipeShare` moves a copy into another task's table.
    pub pipe_handles: alloc::collections::BTreeSet<crate::ipc::pipe::PipeHandle>,
    /// User thread pointer (TLS base) for this task. Opaque to the kernel: the cell
    /// allocates its own TLS blocks and sets the value with `SetTlsBase`. See
    /// [`tls`](super::tls) for which register carries it per architecture.
    pub tls_base: usize,
    /// Command line this task staged for its own next launch. Task-local by
    /// construction: it is this task's own state, so no other task's syscall —
    /// and no cleanup for another task's failed launch — can address it.
    pub staged_argv: Option<Vec<u8>>,
    /// Command line this task's spawner handed it before it became runnable.
    /// Read once by `StateRestore` on the reserved argv key; dropped with the
    /// task record, exactly like any other terminal-state resource.
    pub inherited_argv: Option<Vec<u8>>,
    /// Measured identity of the image this root was admitted from. `None` for
    /// threads and kernel-built tasks. Private so the only writer is
    /// [`Task::bind_image`], which refuses a second binding: the record is the
    /// image of this incarnation for as long as the root lives.
    image: Option<abi::cell_image::ImageIdentity>,
    /// The root was launched by the kernel from a catalogued install path, so
    /// its signed image name is its launch identity (`caller_launch_state`).
    /// False for caller-supplied ELF/Mem bytes, sandbox paths and aliases.
    pub(crate) launch_named: bool,
    /// `(cell_id, generation)` of the Cell whose governed launch published this
    /// root, fixed at publication; `None` for kernel/boot spawns and threads.
    /// Keys the narrow "a launcher registers its own child" rules
    /// (`MACHINE_PIN` from trusted init, `OCEL_JS`/`OCEL_PDF` from `activator`).
    pub(crate) spawner: Option<(u64, u64)>,
    /// Canonical context retained for this committed root lifetime.
    pub(crate) child_context: Option<Vec<u8>>,
    pub(crate) child_stdio: [abi::cell_session::ObjectRefV1; 4],
    pub allowed_drivers: Vec<usize>,

    // Maps FD -> FileHandle
    pub open_files: alloc::collections::BTreeMap<usize, FileHandle>,
    /// The request this task is currently serving, for `Reply`.
    pub caller: CallerSlot,
    /// Exact request token last received, zero for a legacy request.
    pub current_async_op: usize,
    /// Kernel-owned bounded async RPC storage; no borrowed user pointers.
    pub async_operations: super::async_ipc::Operations,
    // Last Reply Value received
    pub reply_value: Option<usize>,
    // Current Working Directory
    pub cwd: String,
    // Stack management. A thread's stacks are billed to its incarnation's
    // memory account until their frames are freed: a thread is a cost the cell
    // chose at runtime and can choose repeatedly. A cell's own stacks are part
    // of admitting it and are billed to nobody.
    pub kernel_stack: Option<super::stack::Stack>,
    pub user_stack: Option<super::stack::Stack>,
    /// Frames mapped for this cell's ELF segments, freed when the Task is dropped
    /// (reaped). Without it, a cell's code/data frames leak on every death.
    pub segment_mem: Option<crate::memory::cell_segments::CellSegments>,

    // Lifecycle
    pub waiters: Vec<usize>,
    pub exit_code: Option<usize>,
    /// Death-notification queue (NotifyOnExit): tids of watched tasks that died
    /// while this watcher was NOT parked in Recv. Drained by the next `Recv`
    /// regardless of mask, preserving the public NotifyOnExit contract.
    pub pending_deaths: Vec<(usize, usize)>,
    /// Root-owner death events of registered watchers (Active providers of
    /// `OWNER_WATCH` registry rows), kept separate from generic task watches.
    ///
    /// The opaque watch token never crosses the syscall ABI: it binds this queued
    /// event to the exact subscription so a stale cancellation cannot affect a
    /// successor. These events are delivered only by a wildcard public receive
    /// (`Recv`, or `TryRecv` with mask 0); a masked backend receive must never
    /// consume them.
    pub pending_owner_deaths: Vec<(u64, usize, usize)>,

    /// Exit reason for a generic `NotifyOnExit` wake delivered while this task
    /// was parked in `Recv`. Owner-watch wakes use `pending_owner_deaths` instead.
    pub pending_exit_reason: Option<usize>,

    /// Test-hooks-only, exact-path authority to self-register `service::SILO`.
    /// A launch-route token on the root task, not a Cell right (not in `CapSet`).
    #[cfg(feature = "test-hooks")]
    pub development_silo_registration_cap: Option<super::cap::DevelopmentSiloRegistrationCap>,

    /// The Cell incarnation's authority record (architecture review H-1): every
    /// kernel cap, service right, MMIO class and block region the Cell holds.
    /// Read only on the Cell's **root** task, through `Scheduler::cell_rights`,
    /// so every thread of the incarnation sees one set of rights and a revoke
    /// aimed at any of them applies to all. A thread's own record stays `EMPTY`.
    pub(crate) rights: super::cap::CapSet,
    /// `CapRevoke` mask whose teardown is in flight on this root. Gates already
    /// see it withdrawn (`Scheduler::cell_rights`), so no task of the Cell can
    /// re-claim a resource while the revoke tears its holders down; the commit
    /// narrows `rights` and clears it, a failed teardown clears it.
    pub(crate) revoke_pending: u32,

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

    /// x86_64 `IA32_U_CET` image for this task: 0 unless the Cell's signed
    /// image opts into CET-IBT and the CPU enables it (`loader::gnu_property`),
    /// in which case it starts as `hal::cet::USER_CET_IBT`. The scheduler loads
    /// it on switch-in and saves the hardware value (tracker state included)
    /// back on switch-out (`hal::cet::switch_user_cet`); threads inherit it.
    /// Always 0 on other targets.
    pub user_cet: u64,

    /// Scheduling priority tier.  Higher value = higher priority.
    /// See `abi::TaskPriority` for the three defined levels.
    pub priority: u8,
    /// Hart this task last ran on (set at dispatch); RT placement prefers it
    /// (Phase 10 step 7).
    pub last_hart: Option<usize>,

    /// Per-Cell syscall allowlist.  Each bit corresponds to a syscall via
    /// `abi::ViSyscall::allowlist_bit()`.  `u64::MAX` = permit all (default,
    /// used when the Cell ELF does not embed a `__ViCell_syscalls` section).
    pub syscall_allowlist: u64,

    /// Scheduler accounting: watchdog, CPU telemetry and RT observability.
    pub sched_stats: SchedStats,

    /// Liveness-heartbeat deadline (absolute `system_ticks`). `Some(d)` means the cell
    /// opted into heartbeating and must call `Heartbeat` again before tick `d`, else the
    /// scheduler terminates it as HUNG (silent-hang detection — see `pick_next`). `None`
    /// = heartbeat disabled (the default; most cells don't opt in).
    pub heartbeat_deadline: Option<u64>,

    /// Hot-swap replacement state.
    pub hotswap: HotswapState,

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
    /// `cell_id` (see `abi::caller_identity`).
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
    /// attested identity (see [`abi::dir_handles`]).
    ///
    /// The kernel is a courier here and nothing more. It does not know what any
    /// of these handles refer to, cannot tell a live one from a revoked one, and
    /// must never treat this as a second record of filesystem authority — the
    /// filesystem service owns that, and a second copy would drift in the
    /// direction of silently widening what a cell may reach. The only claim the
    /// kernel makes about this field is provenance: *this* spawner named *these*
    /// values at spawn.
    ///
    /// Bounded inline at [`abi::dir_handles::MAX_SPAWN_DIR_HANDLES`], so a
    /// caller-supplied count never sizes an allocation.
    ///
    /// Set once, under the scheduler lock that creates the task, before it can
    /// be scheduled. It is never updated afterwards: authority fixed at creation
    /// is what makes it auditable.
    pub inherited_dirs: abi::dir_handles::InheritedDirHandles,

    /// Directory handles this task has named for the next cell it spawns.
    ///
    /// Staged by `SpawnSetDirs` and consumed by the next cell this task creates,
    /// then cleared — including when that spawn fails, so a set can never attach
    /// to a later unrelated child. Empty means "pass nothing on", which is what
    /// every task that never calls `SpawnSetDirs` does.
    pub staged_dirs: abi::dir_handles::DirHandleSet,

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
    /// Last source a multi-source completion wait delivered, as an index into
    /// `completion_wait::SELECTION_ORDER`; rotation starts after it so one ready
    /// source cannot starve another. One byte, no allocation.
    pub completion_rotor: u8,
    /// Last IRQ line a `WaitCompletion(IRQ)` delivered; the next choice starts
    /// after it so one busy line cannot starve the task's other lines.
    pub completion_irq_cursor: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompletionWait {
    /// Who owns `slot`, as a source class: `NET_RX` when the NET_RX waker holds
    /// the reservation (it is displaced later as abandoned), otherwise `TIMER`,
    /// meaning only this waiter holds it and task exit must release it. A
    /// multi-source mask without NET_RX is recorded as `TIMER` for that reason.
    pub source: u32,
    pub slot: crate::task::completion::SlotId,
    /// Lines this wait covers when its mask names `IRQ`: every `AllocPcieIrq`
    /// line the task held at entry (refreshed when a take finds one revoked).
    /// Empty for every other mask. The scheduler sweep reads it under
    /// `SCHEDULER` without taking the allocation lock.
    pub irq_lines: crate::dev::irq_wait::IrqLineSet,
}

/// Source of [`Task::cell_generation`]. Starts at 1 so 0 stays available as
/// "generation not attested on this path".
/// A real spinlock keeps allocation unique across harts, including RV32 where
/// the portable 64-bit atomic fallback only masks local interrupts.
static NEXT_CELL_GENERATION: crate::sync::Spinlock<u64> =
    crate::sync::Spinlock::ranked(1, crate::sync::rank::NEXT_CELL_GENERATION);

/// Mint a fresh incarnation generation. A launch mints it before reserving the
/// quota row, so the row and the task carry the same generation.
pub(crate) fn next_cell_generation() -> u64 {
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
    /// A task of a fresh incarnation of `cell_id`: mints its generation. A
    /// thread of a live Cell is built with [`Self::of_incarnation`] instead.
    pub fn new(id: usize, cell_id: CellId, name: &str, allowed_drivers: Vec<usize>) -> Self {
        Self::of_incarnation(id, cell_id, next_cell_generation(), name, allowed_drivers)
    }

    /// A task of incarnation `generation` of `cell_id`: one the caller minted
    /// with [`next_cell_generation`], or the owning Cell's for a thread.
    pub(crate) fn of_incarnation(
        id: usize,
        cell_id: CellId,
        generation: u64,
        name: &str,
        allowed_drivers: Vec<usize>,
    ) -> Self {
        Self {
            id,
            cell_id,
            root_tid: id,
            name: String::from(name),
            state: TaskState::Ready,
            context: Context::default(),
            #[cfg(all(feature = "native-domains", cellos_wide))]
            address_space: TaskAddressSpace::Sas,
            #[cfg(all(feature = "native-domains", cellos_wide))]
            has_dynamic_stack_mapping: false,
            trap_frame: ViTrapFrame::default(),
            pipe_handles: alloc::collections::BTreeSet::new(),
            tls_base: 0,
            staged_argv: None,
            inherited_argv: None,
            image: None,
            launch_named: false,
            spawner: None,
            child_context: None,
            child_stdio: [abi::cell_session::ObjectRefV1::ZERO; 4],
            allowed_drivers,
            open_files: alloc::collections::BTreeMap::new(),
            caller: CallerSlot::new(),
            current_async_op: 0,
            async_operations: super::async_ipc::Operations::new(),
            reply_value: None,
            cwd: String::from("/"),
            kernel_stack: None,
            user_stack: None,
            segment_mem: None,
            waiters: Vec::new(),
            exit_code: None,
            pending_deaths: Vec::new(),
            pending_owner_deaths: Vec::new(),
            pending_exit_reason: None,
            #[cfg(feature = "test-hooks")]
            development_silo_registration_cap: None,
            rights: super::cap::CapSet::EMPTY,
            revoke_pending: 0,
            pku_key: 0,
            pku_value: 0,
            user_cet: 0,
            priority: abi::TaskPriority::Normal as u8,
            last_hart: None,
            syscall_allowlist: u64::MAX, // permit-all until ELF section is read
            sched_stats: SchedStats::default(),
            heartbeat_deadline: None,
            hotswap: HotswapState::default(),
            is_critical: false,
            pending_msgs: PendingMailbox::new(),
            cell_generation: generation,
            inherited_dirs: abi::dir_handles::InheritedDirHandles::NONE,
            staged_dirs: abi::dir_handles::DirHandleSet::EMPTY,
            completion: None,
            completion_wait: None,
            completion_rotor: super::completion_wait::INITIAL_ROTOR,
            completion_irq_cursor: u8::MAX,
        }
    }

    /// Record the measured image identity before the task is published.
    /// Returns `false`, changing nothing, when an identity is already bound:
    /// an incarnation is admitted from exactly one image.
    pub(crate) fn bind_image(&mut self, image: abi::cell_image::ImageIdentity) -> bool {
        if self.image.is_some() {
            return false;
        }
        self.image = Some(image);
        true
    }

    /// The image identity bound by [`Self::bind_image`], if any.
    pub(crate) fn image(&self) -> Option<abi::cell_image::ImageIdentity> {
        self.image
    }

    #[cfg(all(feature = "native-domains", cellos_wide))]
    #[allow(dead_code)]
    pub(crate) fn bind_address_space(
        &mut self,
        address_space: Arc<crate::memory::address_space::AddressSpace>,
    ) {
        assert!(matches!(self.address_space, TaskAddressSpace::Sas));
        self.address_space = TaskAddressSpace::Domain(address_space);
    }

    #[cfg(all(feature = "native-domains", cellos_wide))]
    #[cfg(all(feature = "test-hooks", cellos_wide))]
    pub(crate) fn bind_address_space_for_test(
        &mut self,
        address_space: Arc<crate::memory::address_space::AddressSpace>,
    ) {
        self.bind_address_space(address_space);
    }

    /// Acquire the fail-closed execution pin for this task's root. Sas tasks
    /// carry no private root and pass trivially. Must run under `SCHEDULER`
    /// before the task is dequeued or attributed: a Dying root rejects here,
    /// while retirement after a successful pin is legal and drain-safe.
    #[cfg(all(feature = "native-domains", cellos_wide))]
    #[allow(dead_code)]
    pub(crate) fn begin_execution(
        &self,
        hart_id: usize,
    ) -> Result<(), crate::memory::address_space::AddressSpaceError> {
        match &self.address_space {
            TaskAddressSpace::Sas => Ok(()),
            TaskAddressSpace::Domain(space) => space.begin_execution(hart_id),
        }
    }

    pub fn set_current_caller_context(
        &mut self,
        sender_tid: usize,
        sender_cell_id: u64,
        sender_generation: u64,
    ) {
        self.caller.current = Some(sender_tid);
        self.caller.cell_id = sender_cell_id;
        self.caller.cell_generation = sender_generation;
        self.caller.request_generation = self.caller.next_request_generation;
        self.caller.next_request_generation = self.caller.next_request_generation.saturating_add(1);
        self.current_async_op = 0;
    }

    pub fn set_received_caller_context(
        &mut self,
        sender_tid: usize,
        sender_cell_id: u64,
        sender_generation: u64,
    ) {
        // Token 0: any holder row left for this task is stale and is ignored.
        let _ = self.set_received_request_context(sender_tid, sender_cell_id, sender_generation, 0);
    }

    /// Commit caller identity and bounded-operation token together. Preserving
    /// VFS's outer grant authority must also preserve its operation token, even
    /// when a nested dependency reply happens to have the same sender TID.
    ///
    /// Returns the token now held as `current_async_op` (0: none) when the
    /// context changed, for the caller to record with
    /// [`async_ipc::note_holder`](crate::task::async_ipc::note_holder);
    /// `None` when VFS keeps its outer context.
    #[must_use = "record the new holder in the scheduler's async index"]
    pub fn set_received_request_context(
        &mut self,
        sender_tid: usize,
        sender_cell_id: u64,
        sender_generation: u64,
        token: usize,
    ) -> Option<usize> {
        // A RETAINS_OUTER_CALLER provider (VFS) may perform nested IPC while
        // serving a request. Its outer caller remains the authority for grants
        // and owner-death watches until it sends that caller's response; a
        // storage reply must not replace it.
        if self.caller.current.is_some()
            && crate::cell::service_registry::retains_outer_caller(
                self.cell_id.0,
                self.cell_generation,
            )
        {
            return None;
        }
        self.set_current_caller_context(sender_tid, sender_cell_id, sender_generation);
        self.current_async_op = token;
        Some(token)
    }

    /// Drop VFS's public request context and return the exact lease identity
    /// that must be released after the scheduler lock is dropped.
    pub fn begin_receive_context(&mut self, mask: usize) -> Option<(usize, u64)> {
        // A RETAINS_OUTER_CALLER provider uses a wildcard receive only at its
        // public request loop. Masked receives are nested dependency replies
        // and keep the outer authority.
        // Nothing to drop without a caller context: skip the registry read.
        if mask == 0
            && (self.caller.current.is_some() || self.current_async_op != 0)
            && crate::cell::service_registry::retains_outer_caller(
                self.cell_id.0,
                self.cell_generation,
            )
        {
            let dropped = self
                .caller
                .current
                .map(|grant_owner| (grant_owner, self.caller.request_generation));
            self.clear_current_caller_context();
            return dropped;
        }
        None
    }

    /// Whether a receive mask may dequeue a tokenized owner-death event.
    ///
    /// Owner death is routed on a registered watcher's public plane only (the
    /// `OWNER_WATCH` providers). Its root TID and subscription token identify the event after this
    /// predicate succeeds; neither is a sender match and therefore neither can
    /// authorize a masked backend receive.
    #[inline]
    pub(crate) const fn owner_death_matches_receive_mask(mask: usize) -> bool {
        mask == 0
    }

    pub fn clear_current_caller_context(&mut self) {
        self.caller.current = None;
        self.caller.cell_id = 0;
        self.caller.cell_generation = 0;
        self.caller.request_generation = 0;
        self.current_async_op = 0;
    }

    pub fn clear_current_caller_context_if(
        &mut self,
        sender_tid: usize,
        request_generation: u64,
    ) -> bool {
        if self.caller.current == Some(sender_tid)
            && self.caller.request_generation == request_generation
        {
            self.clear_current_caller_context();
            return true;
        }
        false
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        #[cfg(feature = "p01-trace")]
        crate::p01_resources::prepare_drop(self);
        #[cfg(all(feature = "native-domains", cellos_wide))]
        if self.has_dynamic_stack_mapping {
            #[cfg_attr(not(feature = "p01-trace"), allow(unused_mut))]
            if let (TaskAddressSpace::Domain(space), Some(mut kernel_stack), Some(mut user_stack)) = (
                &self.address_space,
                self.kernel_stack.take(),
                self.user_stack.take(),
            ) {
                if let Err(error) = space.unmap_existing_task_stacks(&kernel_stack, &user_stack) {
                    // The tag's invalidation is unconfirmed, so a remote hart may
                    // still resolve these stack addresses and walk into the
                    // backing frames. The unmap retained the tables it owns; the
                    // backing is this Task's, and the one thing it must not do is
                    // hand it to the allocator now. The runs are quarantined
                    // behind the tag (counted, logged) and the reaper frees them
                    // only once a later epoch confirms it.
                    let pages = kernel_stack.pages + user_stack.pages;
                    #[cfg(feature = "p01-trace")]
                    {
                        let owner = crate::p01_resources::owner(self);
                        // Unlike the existing runtime leak counter, include a
                        // kernel stack's guard frames: each run, by its PA.
                        for stack in [&kernel_stack, &user_stack] {
                            if let Some((base, frames)) = stack.frames() {
                                crate::p01_retained::begin(
                                    Some(owner),
                                    base,
                                    frames,
                                    crate::p01_retained::Reason::TaskStackForget,
                                );
                            }
                        }
                        crate::p01_resources::retained(owner, 1);
                        crate::p01_resources::finish(kernel_stack.p01_obligation.take(), 5);
                        crate::p01_resources::finish(user_stack.p01_obligation.take(), 5);
                    }
                    log::error!(
                        "[aspace] task stack teardown deferred ({:?}): {} frame(s) quarantined until the tag's invalidation is confirmed",
                        error,
                        pages
                    );
                    crate::task::stack::Stack::retire_behind_tag(
                        space.asid(),
                        [kernel_stack, user_stack],
                    );
                }
            }
        }
    }
}
