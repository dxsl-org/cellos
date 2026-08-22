use super::tcb::{FileHandle, SyscallFuture, Task, TaskState};
use alloc::boxed::Box;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;
use core::task::{Context, Poll};
use log::info;
use types::*;

/// Upper bound on live tasks sharing one `CellId` — the cell's own task plus its
/// threads. Each costs a contiguous run of `STACK_PAGES + 1` frames that cannot be
/// satisfied from fragmented memory, so this is a fragmentation bound, not a
/// fairness one. Chosen well above any current cell's use (no cell spawns threads
/// today; `ostd` exposes `sys_spawn` but nothing calls it) and far below the point
/// where 65-frame runs stop being findable.
///
/// It is also the tighter of the two bounds a thread now meets: the default 16 MiB
/// memory quota would admit about 63 stacks of `STACK_PAGES + 1` frames on its own,
/// so a cell hits this count first and the quota only binds a cell that is already
/// holding heap. Keeping the count as the first refusal is deliberate — it fails on
/// a number the operator can reason about rather than on whatever heap the cell
/// happened to be holding at the time.
pub const MAX_THREADS_PER_CELL: usize = 32;

/// Read the currently-executing cell ID (0 = kernel).
///
/// Delegates to `hart_local` which reads the per-hart `current_cell_id` field
/// via the `tp` CSR — O(1), no lock.  Safe to call from the allocator hot path.
pub fn current_cell_id() -> usize {
    super::hart_local::current_cell_id()
}

// Dummy Waker
// In a real executor, we'd have a way to wake specific tasks.
// Here we just poll in the loop.
// We need a dummy waker to pass to poll.
use core::task::{RawWaker, RawWakerVTable, Waker};

fn dummy_waker() -> Waker {
    unsafe { Waker::from_raw(dummy_raw_waker()) }
}

fn dummy_raw_waker() -> RawWaker {
    RawWaker::new(core::ptr::null(), &DUMMY_VTABLE)
}

static DUMMY_VTABLE: RawWakerVTable =
    RawWakerVTable::new(|_| dummy_raw_waker(), |_| {}, |_| {}, |_| {});

/// Publish boot ownership at the task→boot boundary on targets without an
/// incoming-side completion hook.
///
/// RV64 leaves the outgoing task and Cell visible until
/// `vi_context_switch_complete` runs on the boot stack after the raw Context
/// save. RV32, AArch64, and x86_64 cannot make that post-switch publication, so
/// they must enter boot with the scheduler identity already cleared.
#[inline(always)]
fn prepare_task_to_boot_switch(hart_id: usize) {
    use super::hart_local::ready as rl;

    if rl::CLEAR_TASK_TO_BOOT_IDENTITY_BEFORE_SWITCH {
        rl::set_current_task_id(hart_id, 0);
        super::hart_local::set_current_cell_context(0, 0);
    }
}

/// CPU-monopoly watchdog budget, in 10 ms scheduler ticks. A task may run this
/// many consecutive ticks WITHOUT voluntarily blocking before it is deemed a
/// runaway (infinite loop / livelock) and terminated. 500 ticks = 5 s of
/// uninterrupted CPU — far beyond any cooperative or real-time cell, which block
/// (Recv/Send/Sleep) every iteration — so legitimate work never trips it. The
/// budget is kernel-owned; a cell cannot extend its own.
const WATCHDOG_BUDGET_TICKS: u32 = 500;

/// CPU-monopoly *warning* threshold (80% of the kill budget). An RT cell that crosses
/// this without yielding gets a one-shot `RtCpuOverrun` audit event — an early signal
/// that it is trending toward the hard watchdog kill, so an operator/log analysis can
/// catch a degrading RT loop before it is terminated. Observability only.
const WATCHDOG_WARN_TICKS: u32 = WATCHDOG_BUDGET_TICKS * 4 / 5;

/// Sentinel recorded as the `cause` of a `CellFault` audit entry for a watchdog
/// kill, to distinguish it from a real hardware trap. Deliberately not a valid
/// syndrome on any supported architecture.
const WATCHDOG_FAULT_CAUSE: u32 = 0x0000_DEAD;

/// Death-notification subscriptions: `watched_tid → [watcher_tid, …]`.
///
/// A watcher (a `SpawnCap` holder, e.g. a supervisor) registers via the
/// `NotifyOnExit` syscall; `exit_task` delivers to each watcher when the watched
/// task dies (wakes a parked `Recv`, or queues onto `Task::pending_deaths` if the
/// watcher is busy). One-shot: the subscription is removed on delivery, so a
/// supervisor re-registers for the respawned child.
///
/// Lock order: only ever locked while already holding (or after releasing)
/// SCHEDULER — never SUBSCRIBERS-then-SCHEDULER — to avoid deadlock.
static DEATH_SUBSCRIBERS: crate::sync::Spinlock<BTreeMap<usize, Vec<usize>>> =
    crate::sync::Spinlock::new(BTreeMap::new());

/// Scheduler-owned lifetime record for a bounded reusable CellId slot.
///
/// This is intentionally an array, not a task-table lookup: CellIds are quota
/// slots and task IDs are monotonic transport identities.
#[derive(Clone, Copy)]
enum CellOwnerSlot {
    Empty,
    Live(api::cell_owner::CellOwner),
    Retiring(api::cell_owner::CellOwner),
}

#[derive(Clone, Copy)]
struct CellOwnerWatch {
    watched_root_tid: usize,
    watcher_tid: usize,
}

pub(crate) struct RootRetirement {
    pub owner: api::cell_owner::CellOwner,
    pub member_tids: Vec<usize>,
    /// Matching zombies move with their generation's resource release, rather
    /// than waiting for a later global zombie sweep after quota is reusable.
    pub zombies: Vec<Box<Task>>,
    requested_switch_completion: [usize; super::smp::MAX_HARTS],
}

/// Exact VFS lease release deferred after a holder abandons its request context.
///
/// The key remains holder+owner+generation so only that request can release its
/// lease or owner-dead quarantined frames.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct VfsLeaseRelease {
    pub holder_tid: usize,
    pub grant_owner: usize,
    pub request_generation: u64,
}

const MAX_RETURNABLE_OWNER_WATCH_TOKEN: u64 = isize::MAX as u64;


/// Register `watcher` to be notified when `watched` exits or faults.
pub fn subscribe_death(watched: usize, watcher: usize) {
    DEATH_SUBSCRIBERS
        .lock()
        .entry(watched)
        .or_default()
        .push(watcher);
}

/// Central task table (Hubris-like).
///
/// Ready queues and current_task_id are now PER-HART in `ViHartLocal::ready`
/// and `ViHartLocal::current_task_id` (Phase 03).  This struct keeps only the
/// shared state that requires the global SCHEDULER lock: the task table itself,
/// the zombie list, and the next-id counter.
pub struct Scheduler {
    pub tasks: BTreeMap<usize, Box<Task>>,
    pub zombies: Vec<Box<Task>>,
    /// Fixed CellId/generation → root-TID authority. A slot leaves `Live`
    /// before any root resources are released or its CellId is reusable.
    cell_owners: [CellOwnerSlot; crate::memory::cell_quota::MAX_CELLS],
    /// Token-indexed VFS-only root-death subscriptions.
    cell_owner_watches: BTreeMap<u64, CellOwnerWatch>,
    next_cell_owner_watch: u64,
    pub next_task_id: usize,
    /// Task IDs whose grant pages must be reaped outside the SCHEDULER lock.
    ///
    /// Watchdog kill paths push here instead of calling reap_grants_for_task directly,
    /// because free_grant_pages acquires KERNEL_ROOT and FRAME_ALLOCATOR while the
    /// watchdog runs inside SCHEDULER — inverting the documented lock order.
    /// yield_cpu() drains this list after dropping SCHEDULER, matching the zombie-reaper pattern.
    /// Task IDs whose task-local resources must be reaped outside SCHEDULER.
    /// Root-member IDs remain in `pending_root_retirements` until every hart
    /// has completed a switch away from the retiring generation.
    pub(super) pending_grant_reap: Vec<usize>,
    /// Root retirements awaiting completed context switches before CellId-wide
    /// resource release and owner-slot reuse.
    pending_root_retirements: Vec<RootRetirement>,
    /// Dead VFS holder IDs. Every lease held by a dead task releases only
    /// after its saved context is no longer executing on any hart.
    pub(super) pending_vfs_holder_release: Vec<usize>,
    /// Exact VFS request contexts released at a public boundary. These records
    /// retain holder, owner, and generation, unlike the holder-death queue.
    pending_vfs_context_release: Vec<VfsLeaseRelease>,
    /// TIMER reservations owned by dead tasks, released outside SCHEDULER.
    pending_completion_release: Vec<(
        usize,
        alloc::sync::Arc<super::completion::CompletionQueue>,
        super::completion::SlotId,
    )>,
    pub last_global_sweep_tick: usize,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            tasks: BTreeMap::new(),
            zombies: Vec::new(),
            cell_owners: [CellOwnerSlot::Empty; crate::memory::cell_quota::MAX_CELLS],
            cell_owner_watches: BTreeMap::new(),
            next_cell_owner_watch: 1,
            next_task_id: 1,
            pending_grant_reap: Vec::new(),
            pending_root_retirements: Vec::new(),
            pending_vfs_holder_release: Vec::new(),
            pending_vfs_context_release: Vec::new(),
            pending_completion_release: Vec::new(),
            last_global_sweep_tick: 0,
        }
    }

    /// Return the root endpoint only while this exact Cell generation is live.
    pub fn resolve_live_cell_owner(
        &self,
        cell_id: CellId,
        generation: u64,
    ) -> Option<api::cell_owner::CellOwner> {
        let slot = self.cell_owners.get(cell_id.0 as usize)?;
        let CellOwnerSlot::Live(owner) = slot else {
            return None;
        };
        if owner.cell_id != cell_id.0 || owner.generation != generation || !owner.is_live() {
            return None;
        }
        let root = self.tasks.get(&(owner.root_tid as usize))?;
        (root.id as u64 == owner.root_tid
            && root.root_tid as u64 == owner.root_tid
            && root.cell_id.0 == owner.cell_id
            && root.cell_generation == owner.generation)
            .then_some(*owner)
    }

    /// Publish the owner slot during the launch commit, after task initialization
    /// and before the task is made runnable.
    pub(crate) fn publish_live_cell_owner(&mut self, owner: api::cell_owner::CellOwner) -> bool {
        let Some(slot) = self.cell_owners.get_mut(owner.cell_id as usize) else {
            return false;
        };
        if !owner.is_live() || !matches!(slot, CellOwnerSlot::Empty) {
            return false;
        }
        *slot = CellOwnerSlot::Live(owner);
        true
    }

    pub(crate) fn cell_owner_slot_is_empty(&self, cell_id: CellId) -> bool {
        self.cell_owners
            .get(cell_id.0 as usize)
            .is_some_and(|slot| matches!(slot, CellOwnerSlot::Empty))
    }

    pub(crate) fn live_cell_owner_for_id(
        &self,
        cell_id: CellId,
    ) -> Option<api::cell_owner::CellOwner> {
        let CellOwnerSlot::Live(owner) = self.cell_owners.get(cell_id.0 as usize)? else {
            return None;
        };
        self.resolve_live_cell_owner(cell_id, owner.generation)
    }

    fn begin_root_retirement(&mut self, owner: api::cell_owner::CellOwner) {
        if let Some(slot) = self.cell_owners.get_mut(owner.cell_id as usize) {
            *slot = CellOwnerSlot::Retiring(owner);
        }
    }
    /// Withdraw a retiring owner slot at the post-quiescence release boundary.
    ///
    /// The result is the admission invariant: callers MUST retain the matching
    /// CellId quota unless this exact `Retiring(owner)` slot became `Empty`.
    pub(crate) fn finish_root_retirement(&mut self, owner: api::cell_owner::CellOwner) -> bool {
        let Some(slot) = self.cell_owners.get_mut(owner.cell_id as usize) else {
            return false;
        };
        if matches!(slot, CellOwnerSlot::Retiring(current) if *current == owner) {
            *slot = CellOwnerSlot::Empty;
            true
        } else {
            false
        }
    }

    #[cfg(feature = "test-hooks")]
    pub(crate) fn clear_live_cell_owner_for_test(&mut self, owner: api::cell_owner::CellOwner) {
        if let Some(slot) = self.cell_owners.get_mut(owner.cell_id as usize) {
            if matches!(slot, CellOwnerSlot::Live(current) if *current == owner) {
                *slot = CellOwnerSlot::Empty;
            }
        }
    }

    /// Snapshot every live or zombie task that belongs to `owner`'s exact
    /// generation. A zombie remains a member because its saved context can be
    /// the one still executing on another hart.
    fn root_generation_member_tids(&self, owner: api::cell_owner::CellOwner) -> Vec<usize> {
        self.tasks
            .values()
            .chain(self.zombies.iter())
            .filter_map(|task| {
                (task.cell_id.0 == owner.cell_id
                    && task.cell_generation == owner.generation
                    && task.root_tid as u64 == owner.root_tid)
                    .then_some(task.id)
            })
            .collect()
    }

    /// Take one positive token representable by every syscall return ABI.
    fn take_cell_owner_watch_token(&mut self) -> Option<u64> {
        let token = self.next_cell_owner_watch;
        if token == 0 || token > MAX_RETURNABLE_OWNER_WATCH_TOKEN {
            return None;
        }
        // Never expose a token the syscall return ABI cannot carry. Saturating
        // the sequence at zero fails closed after the final valid token instead
        // of wrapping into a live token.
        self.next_cell_owner_watch = token.checked_add(1).unwrap_or(0);
        Some(token)
    }

    /// Atomically attest the VFS receive principal and install an exact root
    /// death subscription. The caller holds no authority over other cells.
    pub fn watch_live_cell_owner(
        &mut self,
        watcher_tid: usize,
        cell_id: CellId,
        generation: u64,
    ) -> Option<(api::cell_owner::CellOwner, u64)> {
        let watcher = self.tasks.get(&watcher_tid)?;
        if !crate::fast_ipc::is_registered_vfs_cell(watcher.cell_id.0 as usize)
            || watcher.current_caller_cell_id != cell_id.0
            || watcher.current_caller_cell_generation != generation
        {
            return None;
        }
        let owner = self.resolve_live_cell_owner(cell_id, generation)?;
        let token = self.take_cell_owner_watch_token()?;
        self.cell_owner_watches.insert(
            token,
            CellOwnerWatch { watched_root_tid: owner.root_tid as usize, watcher_tid },
        );
        Some((owner, token))
    }

    pub fn cancel_cell_owner_watch(&mut self, watcher_tid: usize, token: u64) {
        if self
            .cell_owner_watches
            .get(&token)
            .is_some_and(|watch| watch.watcher_tid == watcher_tid)
        {
            self.cell_owner_watches.remove(&token);
        }
    }

    #[cfg(feature = "test-hooks")]
    pub(crate) fn publication_snapshot_counters(&self) -> (usize, usize, usize, usize) {
        (
            self.pending_grant_reap.len(),
            self.pending_vfs_holder_release.len(),
            self.pending_completion_release.len(),
            self.last_global_sweep_tick,
        )
    }

    /// Push task `id` onto the CALLING hart's local ready queue.
    ///
    /// Returns the priority level used so callers can optionally call
    /// `pend_preempt_if_needed(priority)` to trigger zero-latency RT preemption.
    ///
    /// Call while holding SCHEDULER (lock order: SCHEDULER → per-hart ready).
    pub fn push_ready(&mut self, id: usize) -> u8 {
        let priority = self
            .tasks
            .get(&id)
            .map(|t| t.priority)
            .unwrap_or(api::TaskPriority::Normal as u8);
        // RT tasks target the dedicated RT hart when it is online; fall back to
        // the current hart on single-hart systems (e.g. QEMU without -smp 2).
        let target_hart = if priority >= api::TaskPriority::RealTime as u8
            && crate::task::smp::is_rt_hart_online()
        {
            crate::task::smp::HART_RT
        } else {
            super::hart_local::current_hart_id()
        };
        super::hart_local::ready::push_on_hart(target_hart, id, priority);
        priority
    }

    /// Pend an S-mode software interrupt if `new_priority` exceeds the current
    /// running task's priority.
    ///
    /// Call this after any syscall that transitions a task from blocked → Ready
    /// so that a newly-runnable RealTime cell preempts a Normal/Background cell
    /// within the same syscall return, rather than waiting for the next timer tick.
    ///
    /// The interrupt fires when the trap handler returns via `sret` and
    /// `sstatus.SIE` is restored by hardware.
    #[cfg(target_arch = "riscv64")]
    pub fn pend_preempt_if_needed(&self, new_priority: u8) {
        let hart_id = super::hart_local::current_hart_id();
        let current_tid = super::hart_local::ready::current_task_id_for(hart_id);
        let current_priority = if current_tid > 0 {
            self.tasks
                .get(&current_tid)
                .map(|t| t.priority)
                .unwrap_or(0)
        } else {
            0
        };

        if new_priority > current_priority {
            // RT tasks land on HART_RT when online; fall back to current hart on single-hart systems.
            let target_hart = if new_priority >= api::TaskPriority::RealTime as u8
                && crate::task::smp::is_rt_hart_online()
            {
                crate::task::smp::HART_RT
            } else {
                hart_id
            };
            if target_hart == hart_id {
                // SAFETY: csrsi on sip.SSIP is permitted from S-mode (RISC-V priv spec §4.1.3).
                // The interrupt fires after sret restores sstatus.SIE.
                unsafe { core::arch::asm!("csrsi sip, 0x2") };
            } else {
                // Cross-hart IPI: SSIP fires on the target hart's next interrupt check.
                if let Some((mask, base)) = crate::task::smp::logical_sbi_target(target_hart) {
                    let _ = hal::common::sbi::sbi_send_ipi(mask, base);
                }
            }
        }
    }

    #[cfg(not(target_arch = "riscv64"))]
    pub fn pend_preempt_if_needed(&self, _new_priority: u8) {
        // No-op on non-riscv64 targets.
    }

    /// Allocate a stack pair and register a task around it.
    ///
    /// Prefer [`Self::spawn_with_stacks`] from the cell-spawn path, which already
    /// owns its stacks — calling this there allocated a second pair that was
    /// overwritten and dropped a few lines later.
    pub fn spawn(
        &mut self,
        name: &str,
        cell_id: CellId,
        allowed_drivers: alloc::vec::Vec<usize>,
    ) -> Result<usize, ViError> {
        let pages = crate::task::stack_pages_for(name);
        let kstack = crate::task::stack::Stack::new_kernel(pages)?;
        let ustack = crate::task::stack::Stack::new_user(pages)?;
        Ok(self.spawn_with_stacks(name, cell_id, allowed_drivers, kstack, ustack))
    }

    /// Register a task around stacks the caller already owns.
    ///
    /// Taking the stacks by value keeps the cell-spawn path's ordering guarantee:
    /// it allocates every per-task resource *before* touching the scheduler, so a
    /// failure unwinds through `Drop` without a half-built task ever being
    /// reachable. Allocating here as well would break that, and did — the pair
    /// this function used to allocate was overwritten by the caller's pair
    /// immediately after, so every cell spawn demanded 4 contiguous 65-frame runs
    /// and used 2.
    pub fn spawn_with_stacks(
        &mut self,
        name: &str,
        cell_id: CellId,
        allowed_drivers: alloc::vec::Vec<usize>,
        kstack: crate::task::stack::Stack,
        ustack: crate::task::stack::Stack,
    ) -> usize {
        self.spawn_with_stacks_configured(name, cell_id, allowed_drivers, kstack, ustack, |_| {})
    }

    /// Build, configure, then publish a task while the scheduler remains locked.
    ///
    /// The callback runs after stack ownership and the default context are
    /// installed but before the task reaches any ready queue. Test-only entry
    /// shims use this to avoid exposing a half-configured task to SMP stealing.
    pub(crate) fn spawn_with_stacks_configured<F>(
        &mut self,
        name: &str,
        cell_id: CellId,
        allowed_drivers: alloc::vec::Vec<usize>,
        kstack: crate::task::stack::Stack,
        ustack: crate::task::stack::Stack,
        configure: F,
    ) -> usize
    where
        F: FnOnce(&mut Task),
    {
        let mut task = Box::new(Task::new(self.next_task_id, cell_id, name, allowed_drivers));
        task.state = TaskState::Ready;
        let id = task.id;

        // Stack grows DOWN. Top is at end of region.
        let stack_top = kstack.top;
        let stack_base = kstack.base;

        // Zero the usable stack pages. Skip every guard frame at `stack_base`;
        // `Stack::usable_start` is derived from the allocation's actual guard count.
        //
        // The length comes from the Stack we were handed, never from a constant:
        // with a constant, handing in a smaller stack writes past its end, and in
        // a single address space that lands in another cell's frames with no fault
        // and no log.
        // SAFETY: we own these freshly-allocated, mapped frames exclusively, and
        // the range is exactly the usable extent this Stack reports.
        unsafe {
            core::ptr::write_bytes(
                kstack.usable_start() as *mut u8,
                0,
                kstack.pages * crate::memory::paging::PAGE_SIZE,
            );
        }
        #[cfg(feature = "test-hooks")]
        kstack.test_hook_prime_watermark();
        #[cfg(feature = "test-hooks")]
        ustack.test_hook_prime_watermark();

        let entry = task_entry_point as *const () as usize;
        let (_gp, _tp) = crate::task::get_kernel_gp_tp();

        let ustack_top = ustack.top;

        task.context.sp = stack_top as _;
        task.trap_frame.sepc = entry as _;
        task.trap_frame.sstatus = 0x20_u64 as _; // SPIE enabled, SPP=0 (User Mode)
        task.trap_frame.regs[2] = ustack_top as _; // sp = x2
        #[cfg(target_arch = "riscv64")]
        {
            task.context.ra = entry;
            task.context.gp = _gp;
            task.context.tp = _tp;
        }
        #[cfg(target_arch = "aarch64")]
        {
            task.context.x30 = entry as u64;
        }
        #[cfg(target_arch = "x86_64")]
        {
            task.context.rip = entry as u64;
            task.context.kernel_trap_sp = stack_top as u64;
        }
        task.kernel_stack = Some(kstack);
        task.user_stack = Some(ustack);
        configure(&mut task);

        info!(
            "Task '{}' (ID {}): Stack 0x{:X}-0x{:X}, Entry 0x{:X}",
            name, id, stack_base, stack_top, entry
        );

        self.tasks.insert(id, task);
        self.push_ready(id);
        self.next_task_id += 1;
        id
    }

    /// Spawn a thread only after resolving the Cell's live root record.
    pub fn spawn_thread(
        &mut self,
        name: &str,
        cell_id: CellId,
        allowed_drivers: alloc::vec::Vec<usize>,
        entry: usize,
        arg: usize,
    ) -> Result<usize, ViError> {
        let owner = self
            .cell_owners
            .get(cell_id.0 as usize)
            .and_then(|slot| match slot {
                CellOwnerSlot::Live(owner) => {
                    self.resolve_live_cell_owner(cell_id, owner.generation)
                }
                _ => None,
            })
            .ok_or(ViError::PermissionDenied)?;
        let live = self
            .tasks
            .values()
            .filter(|task| task.cell_id == cell_id && task.cell_generation == owner.generation)
            .count();
        if live >= MAX_THREADS_PER_CELL {
            log::warn!(
                "[sched] cell {:?} at thread cap ({}) — refusing spawn_thread",
                cell_id,
                MAX_THREADS_PER_CELL
            );
            crate::audit::log_event(
                crate::audit::AuditEvent::ThreadCapReached,
                &crate::audit::encode_u32x2(cell_id.0 as u32, live as u32),
            );
            return Err(ViError::OutOfMemory);
        }

        let mut task = Box::new(Task::new(self.next_task_id, cell_id, name, allowed_drivers));
        task.state = TaskState::Ready;
        task.cell_generation = owner.generation;
        task.root_tid = owner.root_tid as usize;
        let id = task.id;

        let kstack = crate::task::stack::Stack::new_kernel(crate::task::stack_pages_for(name))?;
        let ustack = crate::task::stack::Stack::new_user(crate::task::stack_pages_for(name))?;

        // Charge the frames the thread actually took, read back from the Stack
        // rather than recomputed from STACK_PAGES: a second, independent use of
        // the same constant is how the charge and the allocation drift apart.
        // On refusal `kstack` drops here and its frames go straight back, and
        // `charge` has already rolled its own optimistic add back, so nothing
        // needs unwinding by hand.
        let stack_bytes = kstack
            .allocated_bytes()
            .saturating_add(ustack.allocated_bytes());
        if !crate::memory::cell_quota::charge(cell_id.0 as usize, stack_bytes) {
            log::warn!(
                "[sched] cell {:?} cannot afford thread stacks ({} bytes, {} in use) — refusing spawn_thread",
                cell_id,
                stack_bytes,
                crate::memory::cell_quota::in_use(cell_id)
            );
            return Err(ViError::OutOfMemory);
        }
        task.stack_quota_charge = stack_bytes;

        let user_stack_top = ustack.top;
        let stack_base = kstack.base;
        let kstack_pages = kstack.pages;
        let ustack_pages = ustack.pages;

        // SAFETY: We own the allocated stack memory exclusively. The pointer is valid.
        // Setting up task context with valid register values for thread initialization.
        unsafe {
            // Skip every guard frame and zero exactly the usable extent this
            // Stack reports, never a global constant.
            core::ptr::write_bytes(
                kstack.usable_start() as *mut u8,
                0,
                kstack_pages * crate::memory::paging::PAGE_SIZE,
            );
            core::ptr::write_bytes(
                ustack.usable_start() as *mut u8,
                0,
                ustack_pages * crate::memory::paging::PAGE_SIZE,
            );
            #[cfg(feature = "test-hooks")]
            kstack.test_hook_prime_watermark();
            #[cfg(feature = "test-hooks")]
            ustack.test_hook_prime_watermark();

            task.kernel_stack = Some(kstack);
            task.user_stack = Some(ustack);
            super::prime_user_mode_entry(&mut task, entry, arg);

            info!(
                "Thread '{}' (ID {}): KStack 0x{:X}-0x{:X}, UStackTop 0x{:X}, Entry 0x{:X}, Arg 0x{:X}",
                name,
                id,
                stack_base,
                task.kernel_stack.as_ref().map(|stack| stack.top).unwrap_or(0),
                user_stack_top,
                entry,
                arg
            );
        }

        self.tasks.insert(id, task);
        self.push_ready(id);
        self.next_task_id += 1;
        Ok(id)
    }

    /// Consume one trap-captured fault after the trap path has changed
    /// allocation attribution to the kernel. The fixed record protects the
    /// fault funnel from heap work; this scheduler-side operation may use the
    /// ordinary retirement vectors and locks.
    ///
    /// A task ID alone is not sufficient because CellId slots are reusable. A
    /// matching zombie is an idempotent terminal handoff: a selected Context
    /// may fault after its root already moved the exact generation out of
    /// `tasks`, but a mismatched generation remains a fatal integrity error.
    pub fn retire_deferred_fault(&mut self, fault: super::hart_local::DeferredFault) {
        assert!(
            fault.is_trap_proven_user_fault(),
            "[fault] deferred handoff lacks trap-proven U-mode origin"
        );

        let matches_fault = self.tasks.get(&fault.tid).is_some_and(|task| {
            task.cell_id.0 as usize == fault.cell_id && task.cell_generation == fault.generation
        });
        if !matches_fault {
            let already_retired = self.zombies.iter().any(|task| {
                task.id == fault.tid
                    && task.cell_id.0 as usize == fault.cell_id
                    && task.cell_generation == fault.generation
            });
            if already_retired {
                // A selected Context may take a deferred fault after its root
                // already retired it. The matching zombie is terminal state,
                // not an invalid cross-generation handoff.
                return;
            }
            panic!(
                "[fault] invalid deferred handoff: cell={} generation={} task={} cause={:#x} pc={:#x} addr={:#x}",
                fault.cell_id,
                fault.generation,
                fault.tid,
                fault.cause,
                fault.pc,
                fault.fault_addr,
            );
        }

        // The `[fault] Cell` prefix is consumed by the QEMU smoke gate.  Keep
        // diagnostics scalar-only: cloning `Task::name` in trap context can
        // re-enter the exhausted Cell allocator.
        log::error!(
            "[fault] Cell {} (task {} generation {}) terminated: cause={:#x} pc={:#x} addr={:#x}",
            fault.cell_id,
            fault.tid,
            fault.generation,
            fault.cause,
            fault.pc,
            fault.fault_addr,
        );
        crate::audit::log_event(
            crate::audit::AuditEvent::CellFault,
            &crate::audit::encode_u32x2(fault.cell_id as u32, fault.cause as u32),
        );
        self.exit_task(fault.tid, usize::MAX);
    }

    /// Consume one clean `Exit` captured before the caller surrendered its Cell
    /// allocation attribution.  The scheduler owns root classification, so a
    /// worker remains task-local while a root enters the established quiescent
    /// generation-retirement funnel.
    pub fn retire_deferred_exit(&mut self, exit: super::hart_local::DeferredExit) {
        let matches_exit = self.tasks.get(&exit.tid).is_some_and(|task| {
            task.cell_id.0 as usize == exit.cell_id && task.cell_generation == exit.generation
        });
        if !matches_exit {
            let already_retired = self.zombies.iter().any(|task| {
                task.id == exit.tid
                    && task.cell_id.0 as usize == exit.cell_id
                    && task.cell_generation == exit.generation
            });
            if already_retired {
                return;
            }
            panic!(
                "[task] invalid deferred Exit: cell={} generation={} task={}",
                exit.cell_id, exit.generation, exit.tid
            );
        }

        if self
            .tasks
            .get(&exit.tid)
            .is_some_and(|task| task.state == TaskState::Retiring)
        {
            return;
        }

        if let Some(task) = self.tasks.get_mut(&exit.tid) {
            task.exit_code = Some(exit.code);
        }
        crate::audit::log_event(
            crate::audit::AuditEvent::CellExit,
            &crate::audit::encode_u32x2(exit.tid as u32, exit.code as u32),
        );
        log::info!(
            "Syscall::Exit: task {} exited with code {}",
            exit.tid,
            exit.code
        );
        self.exit_task(exit.tid, exit.code);
    }

    /// Reap a task: move it to the zombie list, purge ready queues, unblock
    /// senders stuck on it, and wake any `Wait`-ers with `exit_reason`.
    ///
    /// `exit_reason` is delivered to waiters as their `reply_value` — the exit
    /// code for a clean `Exit`, or `usize::MAX` for a fault / force-kill.
    /// Centralizing the waiter-wake here is the contract that ALL death paths
    /// (clean `Exit`, `ForceExit`, AND hardware faults) notify waiters uniformly;
    /// the fault path previously skipped it, so `Wait(tid)` hung forever when the
    /// target died by fault.
    pub fn exit_task(&mut self, tid: usize, exit_reason: usize) {
        // A deferred fault can arrive from a Context whose root retirement
        // already marked it terminal. Its task record stays in `tasks` until
        // every remote switch completes; do not run terminal cleanup twice.
        if self
            .tasks
            .get(&tid)
            .is_some_and(|task| task.state == TaskState::Retiring)
        {
            return;
        }

        let root_owner = self.tasks.get(&tid).and_then(|task| {
            (task.root_tid == tid).then(|| {
                self.resolve_live_cell_owner(task.cell_id, task.cell_generation)
            })?
        });
        if let Some(owner) = root_owner {
            // A root's exit terminates its whole generation. Mark it retiring
            // before selecting members so neither thread creation nor owner
            // attestation can race the teardown.
            self.begin_root_retirement(owner);
            let live_members: Vec<usize> = self
                .tasks
                .iter()
                .filter_map(|(&member_tid, task)| {
                    (member_tid != tid
                        && task.cell_id.0 == owner.cell_id
                        && task.cell_generation == owner.generation
                        && task.root_tid as u64 == owner.root_tid)
                        .then_some(member_tid)
                })
                .collect();
            for member_tid in live_members {
                self.exit_task(member_tid, exit_reason);
            }
            // A worker can already be a zombie while its saved context remains
            // current on a remote hart. Snapshot both tables after every live
            // member has entered the same retirement funnel so its resources,
            // owner slot, and CellId all share one post-switch release point.
            let members = self.root_generation_member_tids(owner);
            // A member that exited before its root may still have task-local
            // cleanup queued. Transfer it to the generation funnel so no
            // remaining grant, pin, IOMMU mapping, BDF, VM, or VFS lease is
            // released ahead of this retirement's switch completion.
            self.pending_grant_reap
                .retain(|pending_tid| !members.contains(pending_tid));
            self.pending_vfs_holder_release
                .retain(|pending_tid| !members.contains(pending_tid));
            self.pending_root_retirements.push(RootRetirement {
                owner,
                member_tids: members,
                zombies: Vec::new(),
                requested_switch_completion: [0; super::smp::MAX_HARTS],
            });
        }
        let (dead_caller, retiring_member) = self
            .tasks
            .get(&tid)
            .map(|task| {
                let retiring_member = matches!(
                    self.cell_owners.get(task.cell_id.0 as usize),
                    Some(CellOwnerSlot::Retiring(owner))
                        if owner.cell_id == task.cell_id.0
                            && owner.generation == task.cell_generation
                            && owner.root_tid as usize == task.root_tid
                );
                ((task.cell_id.0, task.cell_generation), retiring_member)
            })
            .unwrap_or(((0, 0), false));
        // Keep retiring root members in the dispatch table through remote
        // quiescence. A stale Context can still trap on another hart during
        // this interval, and syscall dispatch must identify it as retiring
        // rather than falling back to early-boot/kernel authority.
        if retiring_member {
            if let Some(task) = self.tasks.get_mut(&tid) {
                task.state = TaskState::Retiring;
            }
        }


        // A dead VFS watcher cannot retain subscriptions after its task-local
        // state is removed. Exact watcher matching preserves other tokens.
        self.cell_owner_watches
            .retain(|_, watch| watch.watcher_tid != tid);

        // Scheduler exit is the terminal lifecycle funnel for clean exits,
        // faults, watchdogs, and hot-swap retirement. Clear any replacement
        // authority before the TID can disappear. Callers hold SCHEDULER, so
        // this preserves the global SCHEDULER -> SWAP_CEILINGS lock order.
        crate::cell::hotswap::clear_swap_ceiling(tid);

        let timer_release = self.tasks.get_mut(&tid).and_then(|task| {
            let wait = task.completion_wait.take()?;
            if wait.source != api::completion::source::TIMER {
                return None;
            }
            task.completion
                .as_ref()
                .map(|queue| (tid, queue.clone(), wait.slot))
        });
        if let Some(release) = timer_release {
            self.pending_completion_release.push(release);
        }

        // Capture waiters BEFORE the task is removed from the table.
        let waiters: Vec<usize> = self
            .tasks
            .get_mut(&tid)
            .map(|t| core::mem::take(&mut t.waiters))
            .unwrap_or_default();

        // Give the cell back the quota its thread stack was charged. Every death
        // path — clean Exit, ForceExit, hardware fault, CPU watchdog, heartbeat
        // kill, hot-swap retirement — funnels through here, which is why the refund
        // lives here and not at reap: a zombie may sit unreaped for a long time, and
        // billing a cell for a thread that has already died turns the quota into a
        // slow leak that eventually refuses legitimate work. `take` makes the refund
        // exactly-once even if this runs twice for the same tid.
        if let Some(t) = self.tasks.get_mut(&tid) {
            #[cfg(feature = "test-hooks")]
            crate::task::maybe_emit_exit_stack_baseline(
                &t.name,
                t.kernel_stack.as_ref(),
                t.user_stack.as_ref(),
            );
            let charge = core::mem::take(&mut t.stack_quota_charge);
            if charge != 0 {
                let cell_raw = t.cell_id.0 as usize;
                crate::memory::cell_quota::refund(cell_raw, charge);
            }
        }

        // Free the dying cell's address space NOW (unmap its segment VAs) so a
        // respawn can reuse the fixed VA and the load-time overwrite guard only
        // ever sees LIVE cells' mappings. Frames are freed lazily at reap.
        // A remote root member may still be executing its saved context. Its
        // address space remains mapped until the retirement IPI is acknowledged
        // and the member has switched away; task reaping then drops it safely.
        if !retiring_member {
            if let Some(t) = self.tasks.get(&tid) {
                if let Some(seg) = &t.segment_mem {
                    seg.eager_unmap();
                }
            }
        }

        // Async-future safety: terminal tasks must not remain in the poll set.
        // Retiring root members stay in `tasks` as dispatch-visible tombstones,
        // but `TaskState::Retiring` is not polled and they move to zombies only
        // after the remote switch-completion proof.
        if !retiring_member {
            if let Some(task) = self.tasks.remove(&tid) {
                self.zombies.push(task);
            }
        }
        if !retiring_member {
            self.pending_vfs_holder_release.push(tid);
        }

        // Service-registry cleanup: drop any well-known service_id that pointed at this
        // tid, so a client lookup in the death→respawn window returns "none" (and retries)
        // rather than a dead provider. The supervisor re-registers the replacement's tid.
        // Locks only REGISTRY (a leaf), safe under the SCHEDULER lock.
        crate::cell::service_registry::clear_tid(tid);

        // Driver-role cleanup: prevent service lookups, IRQ cache hits, or the input
        // poll path from targeting a dead or recycled TID. The supervisor re-registers
        // replacements.
        crate::task::drivers::driver_cell::deregister_all_for(tid);

        // Remove from every hart's ready queue if present.
        super::hart_local::ready::remove_from_all(tid);

        // Best-effort IPC cleanup: unblock tasks stuck sending to the dead task,
        // and clear stale current_caller references. A plain reply waiter that has
        // already left `Sending` is still not woken here (known pre-existing gap,
        // 2026-07-31 Recv buffer-pinning audit); fixing it needs a state-machine audit.
        let mut to_wake = Vec::new();
        for (id, task) in self.tasks.iter_mut() {
            if let TaskState::Sending { target, .. } = task.state {
                if target == tid {
                    task.state = TaskState::Ready;
                    // The Send handler resumes inside the kernel and reads
                    // reply_value; trap a0 alone would be overwritten on return.
                    task.reply_value = Some(usize::MAX);
                    task.trap_frame.regs[10] = usize::MAX as _; // error return: target gone
                    to_wake.push(*id);
                }
            }
            if task.current_caller == Some(tid) {
                // A live VFS holder may still be using the raw GrantSlice
                // pointer. Mark precisely its holder+owner+generation lease
                // pending revoke, but retain the task context and pin until it
                // completes the reply or drops the request at a public receive.
                let release = VfsLeaseRelease {
                    holder_tid: *id,
                    grant_owner: tid,
                    request_generation: task.current_caller_request_generation,
                };
                if crate::memory::pin::mark_vfs_lease_pending_revoke(
                    release.holder_tid,
                    release.grant_owner,
                    release.request_generation,
                ) {
                    log::info!(
                        "[grant] owner {tid} pending-revoked VFS lease holder={} request={}",
                        release.holder_tid,
                        release.request_generation
                    );
                } else {
                    // A context without a GrantSlice is ordinary IPC state and
                    // has no raw pointer whose lifetime must be extended.
                    task.clear_current_caller_context();
                }
            }
        }
        for id in to_wake {
            self.push_ready(id);
        }

        // Wake tasks blocked on Wait(tid).  Last use of `w` ends its borrow of
        // self.tasks before push_ready re-borrows self (NLL) — mirrors the
        // former in-handler pattern, now the single source of truth.
        for wid in waiters {
            if let Some(w) = self.tasks.get_mut(&wid) {
                w.state = TaskState::Ready;
                w.reply_value = Some(exit_reason);
                self.push_ready(wid);
            }
        }

        // Deliver NotifyOnExit death notifications. One-shot: the subscription is
        // removed here. Wake a watcher parked in Recv (its Recv returns
        // current_caller = this dead tid), else queue onto pending_deaths so the
        // watcher gets it on its next Recv (covers a death during respawn).
        let watchers = DEATH_SUBSCRIBERS.lock().remove(&tid).unwrap_or_default();
        let mut woken_watchers = Vec::new();
        for w in watchers {
            if let Some(wt) = self.tasks.get_mut(&w) {
                if matches!(wt.state, TaskState::Recv { .. }) {
                    // Stash the exit reason for delivery as the recv payload (NotifyOnExit
                    // contract). The actual buffer write happens when the watcher's Recv
                    // RESUMES, in the watcher's own syscall context — writing a USER buffer
                    // from here (the trap/fault context) faults (S-mode store to a U page,
                    // SSTATUS.SUM not set).
                    wt.set_received_caller_context(tid, dead_caller.0, dead_caller.1);
                    wt.pending_exit_reason = Some(exit_reason);
                    wt.state = TaskState::Ready;
                    woken_watchers.push(w);
                } else {
                    wt.pending_deaths.push((tid, exit_reason));
                }
            }
        }
        for w in woken_watchers {
            self.push_ready(w);
        }

        // Owner watches are token-indexed and use a distinct delivery lane.
        // Backend receives are masked to a service TID, so only a wildcard VFS
        // public receive may wake for this event; masked receives leave it queued.
        let owner_watchers: Vec<(u64, usize)> = self
            .cell_owner_watches
            .iter()
            .filter_map(|(&token, watch)| {
                (watch.watched_root_tid == tid).then_some((token, watch.watcher_tid))
            })
            .collect();
        let mut woken_owner_watchers = Vec::new();
        for (token, watcher) in owner_watchers {
            self.cell_owner_watches.remove(&token);
            if let Some(task) = self.tasks.get_mut(&watcher) {
                task.pending_owner_deaths.push((token, tid, exit_reason));
                if matches!(
                    task.state,
                    TaskState::Recv { mask, .. }
                        if Task::owner_death_matches_receive_mask(mask)
                ) {
                    task.set_received_caller_context(tid, dead_caller.0, dead_caller.1);
                    task.state = TaskState::Ready;
                    woken_owner_watchers.push(watcher);
                }
            }
        }
        for watcher in woken_owner_watchers {
            self.push_ready(watcher);
        }

        // Worker exit is task-local. Root-member resources and CellId-wide
        // resources are released only by the deferred retirement funnel after
        // completed context-switch proof.
        if root_owner.is_none() && !retiring_member {
            self.pending_grant_reap.push(tid);
        }
    }

    /// Remove and return zombies whose saved context is neither selected nor
    /// executing on any hart. Scheduler selection is published before the raw
    /// switch, so both ownership identities are part of the liveness proof.
    ///
    /// The caller MUST drop the returned tasks OUTSIDE the SCHEDULER lock: dropping
    /// a `Box<Task>` runs `Stack::drop`, which locks `FRAME_ALLOCATOR` and unmaps
    /// via `KERNEL_ROOT`; doing that while holding `SCHEDULER` would invert the lock
    /// order. Returning the tasks (cheap pointer moves) keeps the lock window tiny.
    ///
    /// This is what actually frees a dead cell's kernel + user stack frames (the
    /// largest per-cell allocation) — without it, zombies accumulate forever and
    /// `Stack::drop` never runs (every cell death leaked its stacks).
    pub fn take_reapable_zombies(&mut self) -> Vec<Box<super::tcb::Task>> {
        if self.zombies.is_empty() {
            return Vec::new();
        }

        let mut keep = Vec::new();
        let mut reap = Vec::new();
        for z in core::mem::take(&mut self.zombies) {
            let retained_by_root_retirement = self
                .pending_root_retirements
                .iter()
                .any(|retirement| retirement.member_tids.contains(&z.id));
            if super::hart_local::ready::any_hart_running(z.id) || retained_by_root_retirement {
                keep.push(z);
            } else {
                reap.push(z);
            }
        }
        self.zombies = keep;
        reap
    }

    /// Take task IDs whose resources can be reaped outside SCHEDULER. A task
    /// remains pending until its outgoing saved context has stopped executing.
    pub fn take_pending_grant_reap(&mut self) -> Vec<usize> {
        let mut reap = Vec::new();
        self.pending_grant_reap.retain(|tid| {
            if super::hart_local::ready::any_hart_running(*tid) {
                true
            } else {
                reap.push(*tid);
                false
            }
        });
        reap
    }

    /// Return root retirements only after every member has left every hart and
    /// each remote hart that selected or executed a member has completed its raw
    /// context switch. The owner slot remains `Retiring` while this returns
    /// nothing, so neither quota nor CellId reuse can race a selected Context.
    pub(crate) fn take_quiescent_root_retirements(&mut self) -> Vec<RootRetirement> {
        let current_hart = super::hart_local::current_hart_id();
        let mut ready = Vec::new();
        let mut pending = Vec::new();
        for mut retirement in core::mem::take(&mut self.pending_root_retirements) {
            let mut members_live = false;
            for hart in 0..super::smp::MAX_HARTS {
                // Read selection before execution. Switch completion publishes
                // executing (Release) before clearing selection (Release), so
                // these Acquire loads cannot observe a false quiescent gap.
                let selected = super::hart_local::ready::selected_task_id_for(hart);
                let current = super::hart_local::ready::current_task_id_for(hart);
                let executing = super::hart_local::ready::executing_task_id_for(hart);
                if retirement.member_tids.contains(&selected)
                    || retirement.member_tids.contains(&current)
                    || retirement.member_tids.contains(&executing)
                {
                    members_live = true;
                    if hart != current_hart
                        && (retirement.requested_switch_completion[hart] == 0
                            || super::smp::retirement_switch_completed(
                                hart,
                                retirement.requested_switch_completion[hart],
                            ))
                    {
                        // A completed epoch while the member is still live only
                        // proves a switch *into* (or back to) that member. Arm a
                        // fresh epoch so release waits for its eventual switch-away.
                        retirement.requested_switch_completion[hart] =
                            super::smp::request_retirement_switch(hart);
                    }
                }
            }
            let completed = retirement
                .requested_switch_completion
                .iter()
                .enumerate()
                .all(|(hart, &epoch)| super::smp::retirement_switch_completed(hart, epoch));
            if !members_live && completed {
                // Retiring members remain in `tasks` until this exact
                // quiescence boundary so a stale remote Context is visibly
                // terminal to syscall dispatch. Move those records and any
                // pre-existing matching zombies together before resources or
                // the owner slot can be released.
                let mut retirement_zombies = Vec::new();
                for member_tid in &retirement.member_tids {
                    if let Some(task) = self.tasks.remove(member_tid) {
                        retirement_zombies.push(task);
                    }
                }
                let mut other_zombies = Vec::new();
                for zombie in core::mem::take(&mut self.zombies) {
                    if retirement.member_tids.contains(&zombie.id) {
                        retirement_zombies.push(zombie);
                    } else {
                        other_zombies.push(zombie);
                    }
                }
                self.zombies = other_zombies;
                retirement.zombies = retirement_zombies;
                ready.push(retirement);
            } else {
                pending.push(retirement);
            }
        }
        self.pending_root_retirements = pending;
        ready
    }

    /// Take holders proven no longer current, then release all of each
    /// holder's leases outside the scheduler lock.
    pub fn take_pending_vfs_holder_release(&mut self) -> Vec<usize> {
        let mut release = Vec::new();
        self.pending_vfs_holder_release.retain(|tid| {
            if super::hart_local::ready::any_hart_running(*tid) {
                true
            } else {
                release.push(*tid);
                false
            }
        });
        release
    }

    /// Take dead-task TIMER slots for release outside the scheduler lock.
    pub fn take_pending_completion_release(
        &mut self,
    ) -> Vec<(
        usize,
        alloc::sync::Arc<super::completion::CompletionQueue>,
        super::completion::SlotId,
    )> {
        core::mem::take(&mut self.pending_completion_release)
    }


    pub(crate) fn take_pending_vfs_context_release(&mut self) -> Vec<VfsLeaseRelease> {
        core::mem::take(&mut self.pending_vfs_context_release)
    }
    /// Picks the next task to run on `hart_id` and returns pointers for context switch.
    ///
    /// Hart 0 also runs the global sweep (timer wakes, heartbeat, async-poll, watchdog).
    /// Other harts only do the per-hart pick + work stealing.
    ///
    /// Returns: Option<(current_context_ptr, next_context_ptr)>
    pub fn pick_next(
        &mut self,
        hart_id: usize,
    ) -> Option<(
        *mut crate::hal::arch::Context,
        *const crate::hal::arch::Context,
    )> {
        let now = crate::task::system_ticks();
        // Global sweep (timer wakes, heartbeat, async-poll, watchdog) runs on hart 0 only
        // to prevent double-wake races on multihart setups.
        if hart_id != 0 {
            return self.pick_next_local(hart_id, now);
        }

        let time_advanced = now > self.last_global_sweep_tick;
        let events_pending = crate::task::waker::has_any_pending();

        if time_advanced || events_pending {
            if time_advanced {
                self.last_global_sweep_tick = now;
            }

            // 1. Wake tasks whose deadline elapsed: Sleeping (timer) and RecvTimeout
            //    (a Recv with a deadline). Without the RecvTimeout sweep a cell that
            //    RecvTimeout's a peer that never replies would block forever — the
            //    infinite-block-on-dead-peer hazard. Deadlines are absolute
            //    `system_ticks` (the dispatch stores `system_ticks() + timeout`).
            let mut waking_tasks = VecDeque::new();
            let mut vfs_context_drops = Vec::new();
            for (id, task) in self.tasks.iter_mut() {
                let mut should_wake = false;
                let mut timed_out = false;
                match &task.state {
                    TaskState::Sleeping { until } if now >= *until => {
                        should_wake = true;
                    }
                    // `deadline` is u64 (mtime-domain field); `now` is usize system
                    // ticks. On rv64 usize == u64, so the cast is lossless.
                    TaskState::Recv {
                        deadline: Some(d), ..
                    } if now as u64 >= *d => {
                        should_wake = true;
                        timed_out = true;
                    }
                    TaskState::WaitEvent { mask, deadline } => {
                        let fired = super::waker::consume_pending(*mask);
                        if fired != 0 {
                            // Return fired mask as the syscall result.
                            task.trap_frame.regs[10] = fired as _;
                            should_wake = true;
                        } else if deadline.map(|d| now as u64 >= d).unwrap_or(false) {
                            task.trap_frame.regs[10] = 0; // timeout — return 0
                            should_wake = true;
                            timed_out = true;
                        }
                    }
                    TaskState::WaitCompletion {
                        source, deadline, ..
                    } if deadline.map(|d| now as u64 >= d).unwrap_or(false) => {
                        task.trap_frame.regs[10] = 0;
                        should_wake = true;
                        timed_out = *source != api::completion::source::TIMER;
                    }
                    // WaitIrq: IRQ-only wake — no deadline, no timeout.
                    // ISR sets IRQ_PENDING[irq] atomically (no lock, no scheduler access).
                    // This sweep is the only place that transitions WaitIrq → Ready.
                    TaskState::WaitIrq { irq }
                        if crate::task::drivers::irq_wait::take_pending(*irq) =>
                    {
                        crate::task::drivers::irq_wait::clear_waiter(*irq);
                        should_wake = true;
                    }
                    _ => {}
                }
                if should_wake {
                    // ostd `sys_recv_timeout` returns Ok(0) on timeout; the syscall
                    // return register is regs[10], restored by sret when the task runs.
                    if timed_out {
                        task.trap_frame.regs[10] = 0;
                        // A timed-out public VFS receive drops the old request
                        // at this syscall boundary. Queue only its exact lease
                        // for release outside SCHEDULER; a masked backend wait
                        // retains the outer request context.
                        if let Some((grant_owner, request_generation)) =
                            task.begin_receive_context(0)
                        {
                            let release = VfsLeaseRelease {
                                holder_tid: *id,
                                grant_owner,
                                request_generation,
                            };
                            if crate::memory::pin::find_vfs_lease(
                                release.holder_tid,
                                release.grant_owner,
                                release.request_generation,
                            )
                            .is_some()
                            {
                                vfs_context_drops.push(release);
                            }
                        } else {
                            task.clear_current_caller_context();
                        }
                        task.deadline_misses = task.deadline_misses.saturating_add(1);
                        // Observability: an RT cell whose awaited message missed its deadline
                        // is a missed control-loop cycle — record it (no enforcement). Gated to
                        // RT priority so the safety-timeout use on Normal cells stays quiet.
                        if task.priority >= api::TaskPriority::RealTime as u8 {
                            crate::audit::log_event(
                                crate::audit::AuditEvent::RtDeadlineMiss,
                                &crate::audit::encode_u32x2(
                                    task.cell_id.0 as u32,
                                    task.deadline_misses,
                                ),
                            );
                        }
                    }
                    task.state = TaskState::Ready;
                    waking_tasks.push_back(*id);
                }
            }
            self.pending_vfs_context_release.extend(vfs_context_drops);
            for id in waking_tasks {
                self.push_ready(id);
            }

            // 1b. Heartbeat liveness sweep: terminate any cell that opted into heartbeating
            //     but missed its deadline — a SILENT hang (deadlock / stuck loop) that the
            //     CPU-monopoly watchdog cannot see (that only fires on RT compute hogs). The
            //     death flows through the normal path so the supervisor restarts it. Collect
            //     first, then `exit_task` outside the iteration (it mutates self.tasks).
            let mut hung: Vec<(usize, u64)> = Vec::new();
            for (id, task) in self.tasks.iter() {
                if let Some(d) = task.heartbeat_deadline {
                    if now as u64 >= d {
                        hung.push((*id, task.cell_id.0));
                    }
                }
            }
            for (tid, cell_raw) in hung {
                log::error!(
                    "[heartbeat] task {} (cell {}) missed liveness deadline — terminating (hung)",
                    tid,
                    cell_raw
                );
                // Dump the hung task's park point (Sending{target}/Recv{mask}/…) —
                // a silent kill costs hours of triage; the blocked-on peer's state
                // is the single most useful forensic fact for IPC hangs.
                if let Some(t) = self.tasks.get(&tid) {
                    log::error!("[heartbeat] task {} state at kill: {:?}", tid, t.state);
                    if let TaskState::Sending { target, .. } = t.state {
                        if let Some(tt) = self.tasks.get(&target) {
                            log::error!(
                                "[heartbeat] send target {} state: {:?} pending_msgs={}",
                                target,
                                tt.state,
                                tt.pending_msgs.len()
                            );
                        } else {
                            log::error!("[heartbeat] send target {} no longer exists", target);
                        }
                    }
                }
                crate::audit::log_event(
                    crate::audit::AuditEvent::CellHung,
                    &crate::audit::encode_u32x2(cell_raw as u32, tid as u32),
                );
                // Release resources the hung cell owned (each locks its own state, not
                // `exit_task` classifies worker versus root teardown. Root
                // CellId-wide cleanup waits for the common quiescent funnel.
                self.exit_task(tid, usize::MAX);
                // The terminal task remains this hart's identity until the raw
                // task→boot switch has saved its Context. `vi_context_switch_complete`
                // clears the task and Cell tuple from the incoming boot stack.
            }
        }

        // 2. Poll Async Tasks
        let has_polling = self.tasks.values().any(|t| t.state == TaskState::Polling);
        if has_polling {
            let mut polled_tasks = Vec::new();
            let waker = dummy_waker();
            let mut cx = Context::from_waker(&waker);

            // Iterate keys to avoid borrow check issues
            let keys: Vec<usize> = self.tasks.keys().cloned().collect();
            for id in keys {
                if let Some(task) = self.tasks.get_mut(&id) {
                    if task.state == TaskState::Polling {
                        if let Some(ref mut future_enum) = task.pending_future {
                            match future_enum {
                                SyscallFuture::FileRead(fd, future) => {
                                    // Poll the future
                                    match future.as_mut().poll(&mut cx) {
                                        Poll::Ready((file, res)) => {
                                            // Restore file handle
                                            // file is Box<dyn ViFile>
                                            task.open_files.insert(*fd, FileHandle::new(file));

                                            // Set return value (a0 / regs[10]); errors use the
                                            // syscall ABI sentinel usize::MAX (same encoding as
                                            // ViCell_syscall_dispatch), not a fake 0-byte success.
                                            task.trap_frame.regs[10] =
                                                res.unwrap_or(usize::MAX) as _;

                                            // Wake task
                                            task.state = TaskState::Ready;
                                            task.pending_future = None;
                                            polled_tasks.push(id);
                                        }
                                        Poll::Pending => {
                                            // Still waiting
                                        } //
                                    }
                                }
                            }
                        }
                    }
                }
            }
            for id in polled_tasks {
                self.push_ready(id);
            }
        }

        // After global sweep, fall through to per-hart pick.
        self.pick_next_local(hart_id, now)
    }

    /// Per-hart task selection: watchdog on current task, then pop from local queue
    /// (with work-stealing fallback).  Called by `pick_next` for both hart 0
    /// (after global sweep) and all other harts.
    fn pick_next_local(
        &mut self,
        hart_id: usize,
        _now: usize,
    ) -> Option<(
        *mut crate::hal::arch::Context,
        *const crate::hal::arch::Context,
    )> {
        use super::hart_local::ready as rl;

        // 3. Decide if the current task yields, and run the CPU-monopoly watchdog.
        let current_id_raw = rl::current_task_id_for(hart_id);
        let current_id: Option<usize> = if current_id_raw > 0 {
            Some(current_id_raw)
        } else {
            None
        };
        if let Some(cid) = current_id {
            enum WdAction {
                None,
                Requeue,
                Kill(u64),
            }
            let mut action = WdAction::None;
            if let Some(task) = self.tasks.get_mut(&cid) {
                if task.state == TaskState::Running {
                    task.cpu_run_ticks = task.cpu_run_ticks.saturating_add(1);
                    // Only RealTime-priority tasks can livelock the system.
                    if task.priority >= api::TaskPriority::RealTime as u8 {
                        task.run_ticks = task.run_ticks.saturating_add(1);
                        if task.run_ticks >= WATCHDOG_WARN_TICKS && !task.rt_overrun_warned {
                            task.rt_overrun_warned = true;
                            crate::audit::log_event(
                                crate::audit::AuditEvent::RtCpuOverrun,
                                &crate::audit::encode_u32x2(task.cell_id.0 as u32, task.run_ticks),
                            );
                        }
                        if task.run_ticks > WATCHDOG_BUDGET_TICKS {
                            action = WdAction::Kill(task.cell_id.0);
                        } else {
                            task.state = TaskState::Ready;
                            action = WdAction::Requeue;
                        }
                    } else {
                        task.state = TaskState::Ready;
                        action = WdAction::Requeue;
                    }
                } else {
                    task.run_ticks = 0;
                    task.rt_overrun_warned = false;
                }
            }
            match action {
                WdAction::Requeue => {
                    self.push_ready(cid);
                }
                WdAction::Kill(cell_raw) => {
                    log::error!(
                        "[watchdog] task {} (cell {}) monopolized CPU >{} ticks (~{}s) — terminating",
                        cid, cell_raw, WATCHDOG_BUDGET_TICKS, WATCHDOG_BUDGET_TICKS / 100
                    );
                    crate::audit::log_event(
                        crate::audit::AuditEvent::CellFault,
                        &crate::audit::encode_u32x2(cell_raw as u32, WATCHDOG_FAULT_CAUSE),
                    );
                    // `exit_task` defers root teardown until all members have
                    // acknowledged a scheduling boundary.
                    self.exit_task(cid, usize::MAX);
                }
                WdAction::None => {}
            }
        }

        // 4. Get next task: local queue first, then work-steal from busiest other hart.
        // A wake may have queued a task while its origin still owns the old
        // Context. `pick_local_eligible` leaves that entry deferred until the
        // incoming completion hook releases the origin handoff state.
        let next_id = rl::pick_local_eligible(hart_id).or_else(|| {
            super::hart_local::ready::steal_from_busiest(hart_id);
            rl::pick_local_eligible(hart_id)
        });

        if let Some(nid) = next_id {
            if let Some(next_task) = self.tasks.get_mut(&nid) {
                next_task.state = TaskState::Running;
                super::hart_local::set_current_cell_context(
                    next_task.cell_id.0 as usize,
                    next_task.cell_generation,
                );

                // An RV64 Context carries kernel `tp`. A task can be selected
                // on a different hart from the one that created it, so bind it
                // to the destination before the raw switch invokes its
                // incoming-side completion callback.
                #[cfg(target_arch = "riscv64")]
                {
                    next_task.context.tp = super::hart_local::HART_LOCAL_TP_ADDRS[hart_id]
                        .load(core::sync::atomic::Ordering::Acquire);
                }

                // x86_64 PKU: update CPU_LOCAL.pku_value for the incoming task so
                // the asm ring-3 exit path restores the correct PKRU. Must run while
                // we still hold a reference to the task (before releasing the lock).
                #[cfg(target_arch = "x86_64")]
                crate::hal::syscall::set_task_pku(next_task.pku_value);
            }

            // A controlled test reservation protects only the queued interval:
            // once this hart has removed the task from its queue, no peer can
            // steal it. Release before the raw switch so the reservation cannot
            // survive until the task next runs on an unrelated stack.
            #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
            let _ = rl::release_test_dispatch_on_hart(hart_id, nid);
            if Some(nid) == current_id {
                rl::set_current_task_id(hart_id, nid);
                return None;
            }
            // SAFETY: Box<Task> pins the Task on the heap; pointer is valid until reap.
            let next_ctx: *const crate::hal::arch::Context = self
                .tasks
                .get(&nid)
                .map(|t| &t.context as *const _)
                .unwrap_or_default();
            rl::set_current_task_id(hart_id, nid);

            if let Some(cid) = current_id {
                let curr_ctx: *mut crate::hal::arch::Context =
                    if let Some(t) = self.tasks.get_mut(&cid) {
                        &mut t.context as *mut _
                    } else if let Some(t) = self.zombies.iter_mut().find(|t| t.id == cid) {
                        &mut t.context as *mut _
                    } else {
                        core::ptr::null_mut()
                    };
                if !curr_ctx.is_null() && !next_ctx.is_null() {
                    // `cid` has been requeued but its saved Context is still
                    // live on this CPU.  Publish the no-steal guard immediately
                    // before relinquishing SCHEDULER; RV64 clears it only after
                    // `__switch` saved this Context on the incoming stack.
                    rl::begin_outgoing_context_save(hart_id, cid);
                    if rl::HAS_INCOMING_SWITCH_COMPLETION_HOOK {
                        rl::set_selected_task_id(hart_id, nid);
                    }
                    return Some((curr_ctx, next_ctx));
                }
            } else if !next_ctx.is_null() {
                if rl::HAS_INCOMING_SWITCH_COMPLETION_HOOK {
                    rl::set_selected_task_id(hart_id, nid);
                }
                return Some((core::ptr::null_mut(), next_ctx)); // first switch from boot context
            }
        } else {
            // No ready tasks.
            if let Some(cid) = current_id {
                if self.zombies.iter().any(|t| t.id == cid) {
                    // Zombie with no successor: switch to the idle boot context so
                    // it can be reaped without holding the SCHEDULER lock.
                    let curr_ctx = self
                        .zombies
                        .iter_mut()
                        .find(|t| t.id == cid)
                        .map(|t| &mut t.context as *mut _);
                    if let Some(c) = curr_ctx {
                        // There is no task successor, but boot is still an
                        // incoming Context. RV64 keeps the outgoing zombie and
                        // its hart attribution live until boot-side completion
                        // has saved the Context; targets without that hook must
                        // clear before entering boot.
                        rl::begin_outgoing_context_save(hart_id, cid);
                        prepare_task_to_boot_switch(hart_id);
                        return Some((c, core::ptr::null()));
                    }
                } else if let Some(task) = self.tasks.get_mut(&cid) {
                    // Live blocked task with no peer ready to run.  Suspend it
                    // cleanly by switching to the idle (boot) context so the CPU
                    // can enter WFI and wake when a real event unblocks someone.
                    //
                    // Without this switch, yield_cpu returns without a context
                    // change, the SVC handler gets stale Ok(0) results, and
                    // current_task_id is reset to 0 — causing every subsequent
                    // SVC to be denied.
                    //
                    // The yielding hart's boot context is valid here: its first
                    // boot→task switch saved the scheduler stack and hart-local
                    // registers before any cell SVC.
                    let curr_ctx = &mut task.context as *mut _;
                    // A remote IPC wake can race this task→idle fallback. Arm
                    // before the raw switch; RV64 boot completion releases the
                    // saved Context and clears attribution, while non-hook
                    // targets clear before switching to boot.
                    rl::begin_outgoing_context_save(hart_id, cid);
                    prepare_task_to_boot_switch(hart_id);
                    return Some((curr_ctx, core::ptr::null()));
                } else {
                    // A nonzero current TID must retain a Context in either
                    // `tasks` or `zombies`. Returning to an unaccounted user
                    // stack would be worse than failing closed.
                    panic!(
                        "[scheduler] current task {} has no Context for required task→boot switch",
                        cid
                    );
                }
            } else {
                rl::set_current_task_id(hart_id, 0);
                super::hart_local::set_current_cell_context(0, 0);
            }
        }
        None
    }

    pub fn current_task_mut(&mut self) -> Option<&mut Task> {
        let tid =
            super::hart_local::ready::current_task_id_for(super::hart_local::current_hart_id());
        if tid > 0 {
            self.tasks.get_mut(&tid).map(|b| &mut **b)
        } else {
            None
        }
    }

    pub fn current_task_ref(&self) -> Option<&Task> {
        let tid =
            super::hart_local::ready::current_task_id_for(super::hart_local::current_hart_id());
        if tid > 0 {
            self.tasks.get(&tid).map(|b| &**b)
        } else {
            None
        }
    }

    pub fn has_ready_tasks(&self) -> bool {
        super::hart_local::ready::total_ready_count() > 0
    }

}

#[cfg(test)]
mod retirement_tests {
    use super::*;
    #[cfg(not(target_arch = "riscv64"))]
    #[test]
    fn blocked_task_requeued_from_boot_is_selectable() {
        const TID: usize = 60_004;
        const CELL_RAW: u64 = 61;
        let hart = super::super::hart_local::current_hart_id();
        let old_tid = super::super::hart_local::ready::current_task_id_for(hart);
        let old_cell = super::super::hart_local::current_cell_id();
        let old_generation = super::super::hart_local::current_cell_generation();
        let mut scheduler = Scheduler::new();
        let mut blocked = Box::new(Task::new(TID, CellId(CELL_RAW), "blocked-boot", alloc::vec![]));
        blocked.state = TaskState::Waiting { target: 0 };
        scheduler.tasks.insert(TID, blocked);

        super::super::hart_local::ready::set_current_task_id(hart, TID);
        super::super::hart_local::set_current_cell_context(CELL_RAW as usize, 1);
        let (_, boot) = scheduler
            .pick_next_local(hart, 0)
            .expect("blocked task must switch to boot");
        assert!(boot.is_null());
        assert_eq!(
            super::super::hart_local::ready::current_task_id_for(hart),
            0,
            "non-hook task→boot must not retain the blocked task identity"
        );
        assert_eq!(super::super::hart_local::current_cell_id(), 0);

        scheduler.tasks.get_mut(&TID).expect("blocked task").state = TaskState::Ready;
        super::super::hart_local::ready::push_on_hart(
            hart,
            TID,
            api::TaskPriority::Normal as u8,
        );
        let (curr, next) = scheduler
            .pick_next_local(hart, 0)
            .expect("woken task must be selectable from boot");
        assert!(curr.is_null());
        assert!(!next.is_null());

        super::super::hart_local::ready::remove_from_all(TID);
        super::super::hart_local::ready::set_current_task_id(hart, old_tid);
        super::super::hart_local::set_current_cell_context(old_cell, old_generation);
    }

    #[cfg(not(target_arch = "riscv64"))]
    #[test]
    fn faulted_task_to_boot_allows_successor_selection() {
        const FAULTED_TID: usize = 60_005;
        const SUCCESSOR_TID: usize = 60_006;
        const CELL_RAW: u64 = 60;
        let hart = super::super::hart_local::current_hart_id();
        let old_tid = super::super::hart_local::ready::current_task_id_for(hart);
        let old_cell = super::super::hart_local::current_cell_id();
        let old_generation = super::super::hart_local::current_cell_generation();
        let mut scheduler = Scheduler::new();
        let mut faulted = Box::new(Task::new(
            FAULTED_TID,
            CellId(CELL_RAW),
            "faulted-boot",
            alloc::vec![],
        ));
        faulted.state = TaskState::Terminated;
        scheduler.zombies.push(faulted);
        scheduler.tasks.insert(
            SUCCESSOR_TID,
            Box::new(Task::new(
                SUCCESSOR_TID,
                CellId(CELL_RAW),
                "boot-successor",
                alloc::vec![],
            )),
        );

        super::super::hart_local::ready::set_current_task_id(hart, FAULTED_TID);
        super::super::hart_local::set_current_cell_context(CELL_RAW as usize, 1);
        let (_, boot) = scheduler
            .pick_next_local(hart, 0)
            .expect("faulted task must switch to boot");
        assert!(boot.is_null());
        assert_eq!(
            super::super::hart_local::ready::current_task_id_for(hart),
            0,
            "non-hook task→boot must not retain the faulted task identity"
        );
        assert_eq!(super::super::hart_local::current_cell_id(), 0);

        super::super::hart_local::ready::push_on_hart(
            hart,
            SUCCESSOR_TID,
            api::TaskPriority::Normal as u8,
        );
        let (curr, next) = scheduler
            .pick_next_local(hart, 0)
            .expect("successor must be selectable from boot");
        assert!(curr.is_null());
        assert!(!next.is_null());

        super::super::hart_local::ready::remove_from_all(SUCCESSOR_TID);
        super::super::hart_local::ready::set_current_task_id(hart, old_tid);
        super::super::hart_local::set_current_cell_context(old_cell, old_generation);
    }

    #[test]
    fn remote_root_retirement_waits_for_zombie_worker_switch_before_slot_reuse() {
        const ROOT_TID: usize = 60_001;
        const WORKER_TID: usize = 60_002;
        const CELL_RAW: u64 = 63;
        const GENERATION: u64 = 7;
        let remote_hart = if super::super::hart_local::current_hart_id()
            == super::super::smp::HART_RT
        {
            0
        } else {
            super::super::smp::HART_RT
        };

        let owner = api::cell_owner::CellOwner::new(CELL_RAW, GENERATION, ROOT_TID as u64);
        let mut scheduler = Scheduler::new();
        let mut root = Box::new(Task::new(
            ROOT_TID,
            CellId(CELL_RAW),
            "retirement-root",
            alloc::vec![],
        ));
        root.cell_generation = GENERATION;
        let mut worker = Box::new(Task::new(
            WORKER_TID,
            CellId(CELL_RAW),
            "retirement-worker-zombie",
            alloc::vec![],
        ));
        worker.root_tid = ROOT_TID;
        worker.cell_generation = GENERATION;
        scheduler.tasks.insert(ROOT_TID, root);
        scheduler.zombies.push(worker);
        assert!(scheduler.publish_live_cell_owner(owner));
        scheduler.begin_root_retirement(owner);

        let members = scheduler.root_generation_member_tids(owner);
        assert!(members.contains(&ROOT_TID) && members.contains(&WORKER_TID));
        scheduler.pending_root_retirements.push(RootRetirement {
            owner,
            member_tids: members,
            zombies: Vec::new(),
            requested_switch_completion: [0; super::super::smp::MAX_HARTS],
        });

        let old_current =
            super::super::hart_local::ready::current_task_id_for(remote_hart);
        let old_selected =
            super::super::hart_local::ready::selected_task_id_for(remote_hart);
        let old_executing =
            super::super::hart_local::ready::executing_task_id_for(remote_hart);

        // Deterministically model root exit after the worker Context pointer was
        // selected but before the incoming switch published it as executing.
        super::super::hart_local::ready::set_current_task_id(remote_hart, WORKER_TID);
        super::super::hart_local::ready::set_executing_task_id(remote_hart, 0);
        super::super::hart_local::ready::set_selected_task_id(remote_hart, WORKER_TID);

        assert!(scheduler.take_reapable_zombies().is_empty());
        assert!(scheduler.take_quiescent_root_retirements().is_empty());
        assert!(!scheduler.cell_owner_slot_is_empty(CellId(CELL_RAW)));

        // Incoming-side completion transfers the ownership pin without a gap:
        // executing is published before selection is cleared.
        super::super::hart_local::ready::complete_selected_switch(remote_hart, WORKER_TID);
        assert_eq!(
            super::super::hart_local::ready::selected_task_id_for(remote_hart),
            0
        );
        assert_eq!(
            super::super::hart_local::ready::executing_task_id_for(remote_hart),
            WORKER_TID
        );
        assert!(scheduler.take_quiescent_root_retirements().is_empty());

        super::super::hart_local::ready::set_current_task_id(remote_hart, 0);
        super::super::hart_local::ready::set_executing_task_id(remote_hart, 0);
        super::super::smp::complete_retirement_switch(remote_hart);
        // Model the ordinary zombie sweep happening between remote completion
        // and the root-retirement sweep. The generation-owned zombie must stay
        // retained until it moves with the quiescent retirement.
        assert!(scheduler.take_reapable_zombies().is_empty());
        let mut ready = scheduler.take_quiescent_root_retirements();
        assert_eq!(ready.len(), 1);
        let retirement = ready.pop().expect("one quiescent retirement");
        assert!(retirement.member_tids.contains(&WORKER_TID));
        assert_eq!(retirement.zombies.len(), 1);
        assert_eq!(retirement.zombies[0].id, WORKER_TID);
        assert!(!scheduler.cell_owner_slot_is_empty(CellId(CELL_RAW)));
        let owner = retirement.owner;
        drop(retirement.zombies);
        scheduler.finish_root_retirement(owner);
        assert!(scheduler.cell_owner_slot_is_empty(CellId(CELL_RAW)));

        super::super::hart_local::ready::set_current_task_id(remote_hart, old_current);
        super::super::hart_local::ready::set_executing_task_id(remote_hart, old_executing);
        super::super::hart_local::ready::set_selected_task_id(remote_hart, old_selected);
    }

    #[test]
    fn matching_zombie_deferred_fault_is_idempotent() {
        const TID: usize = 60_003;
        const CELL_RAW: u64 = 62;
        const GENERATION: u64 = 9;
        let mut scheduler = Scheduler::new();
        let mut zombie = Box::new(Task::new(TID, CellId(CELL_RAW), "faulted-zombie", alloc::vec![]));
        zombie.cell_generation = GENERATION;
        scheduler.zombies.push(zombie);

        scheduler.retire_deferred_fault(
            super::super::hart_local::DeferredFault::test_trap_proven_user(
                TID,
                CELL_RAW as usize,
                GENERATION,
                0xdead,
                0,
                0,
            ),
        );

        assert_eq!(scheduler.zombies.len(), 1);
        assert_eq!(scheduler.zombies[0].id, TID);
    }

    #[test]
    fn owner_watch_tokens_stop_before_the_signed_return_boundary() {
        let mut scheduler = Scheduler::new();
        scheduler.next_cell_owner_watch = MAX_RETURNABLE_OWNER_WATCH_TOKEN;

        assert_eq!(
            scheduler.take_cell_owner_watch_token(),
            Some(MAX_RETURNABLE_OWNER_WATCH_TOKEN)
        );
        assert_eq!(
            scheduler.next_cell_owner_watch,
            MAX_RETURNABLE_OWNER_WATCH_TOKEN + 1
        );
        assert_eq!(scheduler.take_cell_owner_watch_token(), None);
        assert!(scheduler.cell_owner_watches.is_empty());
    }
}

/// Default entry point for kernel tasks
#[no_mangle]
extern "C" fn task_entry_point() {
    // SAFETY: This is the entry point for new tasks. We need to:
    // 1. Force unlock the scheduler (safe because we're in a new task context)
    // 2. Initialize HAL for this task context
    // 3. Enable interrupts (safe because stack is properly set up)
    unsafe {
        crate::task::SCHEDULER.force_unlock();
        crate::hal::arch::init();
        // Enable Interrupts MANUALLY now that we're safe and stack is clean
        crate::hal::arch::enable_interrupts();
    }
    info!("Task started!");
    loop {
        for _ in 0..10_000_000 {
            core::hint::spin_loop();
        }
        info!("Task tick (ID: {})...", crate::task::current_task_id());
        crate::task::yield_cpu();
    }
}
