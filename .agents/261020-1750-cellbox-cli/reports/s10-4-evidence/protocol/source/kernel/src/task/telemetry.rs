//! Phase 06 S6-1 telemetry provider: exact on-CPU accounting and the
//! `TelemetrySnapshot` capture (`.agents/261020-1750-cellbox-cli/phase-06-entry-dossier.md`
//! §5, CP1 approved 2026-10-23: K1, A1, P1, S1, E1, syscall 430 on bit 55).
//!
//! CPU (K1): every scheduler pass on a hart charges that hart's current task the
//! monotonic-ns interval since the hart's previous pass, then restamps the hart.
//! A pass runs from the timer interrupt and from every voluntary yield/block, so
//! the charge is the real on-CPU interval, not a count of passes. Idle time
//! (no current task) is charged to nobody. The clock is the one `GetTime(0)`
//! exposes, so header times and `cpu_ns` share a base by construction; a zero
//! clock (x86 without HPET) charges nothing and the snapshot says
//! `CPU_AVAILABLE = 0`.
//!
//! Cell aggregate (A1): a task leaving the live table adds its final `cpu_ns` to
//! a per-CellId slot tagged with the generation; a later generation in the same
//! slot starts from zero. A zombie still executing on a hart is charged there
//! too, so a Cell's CPU is every member's CPU, exited or live.
//!
//! Retained kernel state: `cpu_ns` (8 bytes per task), one stamp per hart and 16
//! bytes per CellId slot. The capture itself is transient and bounded by the
//! caller's `out_len`.

use alloc::vec::Vec;
use api::syscall::ViMemInfoV1;
use api::telemetry::{
    cell_flags, cell_state, encode_name, header_flags, task_flags, TelemetryCellV1,
    TelemetryHeaderV1, TelemetryTaskV1, CELL_LEN, HEADER_LEN, SCOPE_SYSTEM, TASK_LEN,
    TELEMETRY_VERSION,
};
use core::sync::atomic::{AtomicU64, Ordering};
use types::{CellId, ViError};

use super::hart_local::ready as rl;
use super::scheduler::Scheduler;
use super::smp::MAX_HARTS;

/// Monotonic nanoseconds on the same clock `GetTime(0)` exposes to cells.
pub(crate) fn monotonic_ns() -> Option<u64> {
    #[cfg(target_arch = "x86_64")]
    { Some(hal::hpet::now_ns()) }
    #[cfg(target_arch = "riscv64")]
    { hal::common::timer::read_mtime().checked_mul(100) }
    #[cfg(target_arch = "aarch64")]
    {
        let freq = hal::timer::read_freq();
        if freq == 0 { return None; }
        u64::try_from(u128::from(hal::timer::read_ticks()) * 1_000_000_000u128 / u128::from(freq)).ok()
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "riscv64", target_arch = "aarch64")))]
    { None }
}

/// The exited CPU retained for one CellId slot's current generation.
#[derive(Clone, Copy, Default)]
struct ExitedCpu {
    generation: u64,
    ns: u64,
}

/// Scheduler-owned CPU ledger. Every access is under `SCHEDULER`.
pub(crate) struct CpuLedger {
    /// Monotonic ns of each hart's previous scheduler pass; 0 = never stamped.
    dispatch_ns: [u64; MAX_HARTS],
    /// Indexed by CellId; heap-backed for the same reason as `cell_owners`.
    exited: Vec<ExitedCpu>,
    last_sample_id: u64,
}

impl CpuLedger {
    pub(crate) fn new() -> Self {
        Self {
            dispatch_ns: [0; MAX_HARTS],
            exited: alloc::vec![ExitedCpu::default(); crate::memory::cell_quota::MAX_CELLS],
            last_sample_id: 0,
        }
    }

    /// Restamp `hart` at `now` and return the interval since its previous pass.
    ///
    /// Nothing is charged for the first stamp, for an unavailable (zero) clock,
    /// or for a clock that reads earlier than the previous stamp.
    pub(crate) fn pass(&mut self, hart: usize, now: Option<u64>) -> u64 {
        let Some(now) = now.filter(|&now| now != 0) else { return 0 };
        let Some(stamp) = self.dispatch_ns.get_mut(hart) else { return 0 };
        let previous = core::mem::replace(stamp, now);
        if previous == 0 { 0 } else { now.saturating_sub(previous) }
    }

    /// The still-uncharged interval of `hart`'s current task at `now`.
    fn in_flight(&self, hart: usize, now: u64) -> u64 {
        match self.dispatch_ns.get(hart) {
            Some(&stamp) if stamp != 0 => now.saturating_sub(stamp),
            _ => 0,
        }
    }

    /// Add `ns` of a departed (or departing) member to its Cell incarnation.
    ///
    /// A newer generation takes the slot over from zero; a record for an older
    /// generation than the slot holds is stale and is dropped.
    pub(crate) fn retain_exited(&mut self, cell_id: u64, generation: u64, ns: u64) {
        let Some(slot) = usize::try_from(cell_id).ok().and_then(|id| self.exited.get_mut(id)) else {
            return;
        };
        if generation == 0 || generation < slot.generation {
            return;
        }
        if generation > slot.generation {
            *slot = ExitedCpu { generation, ns: 0 };
        }
        slot.ns = slot.ns.saturating_add(ns);
    }

    /// Retained exited CPU of exactly this Cell incarnation.
    pub(crate) fn exited(&self, cell_id: u64, generation: u64) -> u64 {
        usize::try_from(cell_id)
            .ok()
            .and_then(|id| self.exited.get(id))
            .filter(|slot| slot.generation == generation)
            .map_or(0, |slot| slot.ns)
    }

    fn next_sample_id(&mut self) -> u64 {
        self.last_sample_id = self.last_sample_id.saturating_add(1);
        self.last_sample_id
    }
}

/// Per-boot provider epoch, drawn on first use (E1, falling back to E2).
static PROVIDER_EPOCH: AtomicU64 = AtomicU64::new(0);

/// The nonzero per-boot provider epoch. Call outside `SCHEDULER`.
pub(crate) fn provider_epoch() -> u64 {
    let current = PROVIDER_EPOCH.load(Ordering::Acquire);
    if current != 0 {
        return current;
    }
    let drawn = draw_epoch();
    match PROVIDER_EPOCH.compare_exchange(0, drawn, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => drawn,
        Err(winner) => winner,
    }
}

/// E1: hardware entropy when the secure source answers now (the readiness
/// check). E2 otherwise: the RTC wall-clock instant of boot. Constant 1 only if
/// neither exists, so the epoch is never the zero "absent" value.
fn draw_epoch() -> u64 {
    let mut seed = [0u8; 8];
    if super::drivers::virtio_rng::get_random(&mut seed) == seed.len() {
        let value = u64::from_le_bytes(seed);
        if value != 0 {
            return value;
        }
    }
    let boot = rtc_epoch_ns().saturating_sub(monotonic_ns().unwrap_or(0));
    if boot != 0 { boot } else { 1 }
}

fn rtc_epoch_ns() -> u64 {
    // Hosted unit tests cannot touch CMOS ports or firmware RTCs.
    #[cfg(all(test, not(target_os = "none")))]
    { 0 }
    #[cfg(all(not(all(test, not(target_os = "none"))), target_arch = "x86_64"))]
    { hal::rtc::now_epoch_ns() }
    #[cfg(all(not(all(test, not(target_os = "none"))), target_arch = "aarch64"))]
    { hal::rtc::now_epoch_ns() }
    #[cfg(all(not(all(test, not(target_os = "none"))), target_arch = "riscv64"))]
    { hal::common::rtc::now_epoch_ns() }
    #[cfg(all(
        not(all(test, not(target_os = "none"))),
        not(any(target_arch = "x86_64", target_arch = "aarch64", target_arch = "riscv64"))
    ))]
    { 0 }
}

/// The system-memory block the caller asked for and may see.
pub(crate) enum SystemMemory {
    NotRequested,
    /// Requested without `observe_memory`: flagged, values stay zero.
    Denied,
    /// Requested and allowed, but the frame allocator is not initialised.
    Unavailable,
    Present(ViMemInfoV1),
}

/// One capture taken under `SCHEDULER`; [`Capture::finish`] completes the
/// header after the lock is released.
pub(crate) struct Capture {
    header: TelemetryHeaderV1,
    bytes: Vec<u8>,
}

fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// The `TelemetrySnapshot` argument rule: room for at least a header, an
/// 8-byte-aligned buffer, only defined request bits, and `a3 == 0`.
pub(crate) fn request_is_valid(out_ptr: usize, out_len: usize, flags: usize, reserved: usize) -> bool {
    out_len >= HEADER_LEN
        && out_ptr % api::telemetry::BUFFER_ALIGN == 0
        && flags & !api::telemetry::request::ALL == 0
        && reserved == 0
}

/// Index of the row of exactly this Cell incarnation in CellId-ordered `cells`.
fn member(cells: &[TelemetryCellV1], cell_id: u64, generation: u64) -> Option<usize> {
    cells
        .binary_search_by_key(&cell_id, |cell| cell.cell_id)
        .ok()
        .filter(|&index| cells[index].generation == generation)
}

/// Capture identity, membership and CPU in one scheduler hold.
///
/// `out_len` (validated `>= HEADER_LEN` by the caller) bounds the rows: whole
/// Cell rows first, then whole task rows when `want_tasks`; the remainder is
/// reported by `total_*` versus `emitted_*` and a clear `COMPLETE` flag.
/// Allocation failures are refused, never truncated silently.
pub(crate) fn capture(
    sched: &mut Scheduler,
    provider_epoch: u64,
    now: Option<u64>,
    out_len: usize,
    want_tasks: bool,
) -> Result<Capture, ViError> {
    let clock = now.filter(|&now| now != 0);
    let sample_id = sched.cpu_telemetry.next_sample_id();

    // Each hart's current task and its still-uncharged interval at `clock`.
    let mut running = [(0usize, 0u64); MAX_HARTS];
    if let Some(now) = clock {
        for (hart, slot) in running.iter_mut().enumerate() {
            *slot = (rl::current_task_id_for(hart), sched.cpu_telemetry.in_flight(hart, now));
        }
    }
    let in_flight = |tid: usize| {
        running
            .iter()
            .filter(|&&(current, _)| current != 0 && current == tid)
            .fold(0u64, |sum, &(_, ns)| sum.saturating_add(ns))
    };
    let cpu_flag = |flag: u32| if clock.is_some() { flag } else { 0 };

    // Cells in CellId (slot) order, so tasks can find their row by search.
    let owners = sched.telemetry_cell_owners().count();
    let mut cells: Vec<TelemetryCellV1> = Vec::new();
    cells.try_reserve_exact(owners).map_err(|_| ViError::OutOfMemory)?;
    for (owner, retiring) in sched.telemetry_cell_owners() {
        let root = sched
            .tasks
            .get(&(owner.root_tid as usize))
            .map(|task| &**task)
            .or_else(|| sched.zombies.iter().find(|task| task.id as u64 == owner.root_tid).map(|task| &**task));
        let (name, truncated) = encode_name(root.map_or("", |task| task.name.as_str()));
        let limit = crate::memory::cell_quota::limit(owner.cell_id as usize);
        let mut flags = cpu_flag(cell_flags::CPU_AVAILABLE) | cell_flags::QUOTA_AVAILABLE;
        if limit.is_some() {
            flags |= cell_flags::LIMIT_AVAILABLE;
        }
        if truncated {
            flags |= cell_flags::NAME_TRUNCATED;
        }
        cells.push(TelemetryCellV1 {
            cell_id: owner.cell_id,
            generation: owner.generation,
            root_tid: owner.root_tid,
            cpu_ns: sched.cpu_telemetry.exited(owner.cell_id, owner.generation),
            quota_charged_bytes: crate::memory::cell_quota::in_use(CellId(owner.cell_id)) as u64,
            quota_limit_bytes: limit.unwrap_or(0) as u64,
            task_count: 0,
            state: if retiring { cell_state::RETIRING } else { cell_state::LIVE },
            flags,
            reserved: 0,
            name,
        });
    }
    let total_tasks = sched.tasks.len();
    let mut tasks: Vec<TelemetryTaskV1> = Vec::new();
    if want_tasks {
        tasks.try_reserve_exact(total_tasks).map_err(|_| ViError::OutOfMemory)?;
    }
    for task in sched.tasks.values() {
        let cpu_ns = task.cpu_ns.saturating_add(in_flight(task.id));
        if let Some(index) = member(&cells, task.cell_id.0, task.cell_generation) {
            let cell = &mut cells[index];
            cell.cpu_ns = cell.cpu_ns.saturating_add(cpu_ns);
            cell.task_count = cell.task_count.saturating_add(1);
        }
        if !want_tasks {
            continue;
        }
        let (name, truncated) = encode_name(&task.name);
        let mut flags = cpu_flag(task_flags::CPU_AVAILABLE);
        if truncated {
            flags |= task_flags::NAME_TRUNCATED;
        }
        if task.stack_quota_charge != 0 {
            flags |= task_flags::STACK_IN_QUOTA;
        }
        tasks.push(TelemetryTaskV1 {
            tid: task.id as u64,
            cell_id: task.cell_id.0,
            generation: task.cell_generation,
            cpu_ns,
            user_stack_bytes: task.user_stack.as_ref().map_or(0, |stack| stack.usable_bytes() as u64),
            segment_bytes: task.segment_mem.as_ref().map_or(0, |segments| segments.allocated_bytes() as u64),
            state: super::syscall::encode_task_state(&task.state),
            flags,
            name,
        });
    }
    // A zombie still executing on a hart: its recorded CPU is already retained
    // for its Cell, the interval since the last pass is not yet.
    for &(tid, ns) in &running {
        if tid == 0 || ns == 0 || sched.tasks.get(&tid).is_some() {
            continue;
        }
        if let Some(zombie) = sched.zombies.iter().find(|task| task.id == tid) {
            if let Some(index) = member(&cells, zombie.cell_id.0, zombie.cell_generation) {
                cells[index].cpu_ns = cells[index].cpu_ns.saturating_add(ns);
            }
        }
    }

    cells.sort_unstable_by_key(|cell| cell.generation);
    tasks.sort_unstable_by_key(|task| (task.generation, task.tid));
    let total_cells = cells.len();
    let room = out_len.saturating_sub(HEADER_LEN);
    let emitted_cells = total_cells.min(room / CELL_LEN);
    let room = room - emitted_cells * CELL_LEN;
    let emitted_tasks = tasks.len().min(room / TASK_LEN);
    let complete = emitted_cells == total_cells && (!want_tasks || emitted_tasks == total_tasks);

    let len = HEADER_LEN + emitted_cells * CELL_LEN + emitted_tasks * TASK_LEN;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).map_err(|_| ViError::OutOfMemory)?;
    bytes.resize(HEADER_LEN, 0);
    for cell in &cells[..emitted_cells] {
        bytes.extend_from_slice(&cell.to_bytes());
    }
    for task in &tasks[..emitted_tasks] {
        bytes.extend_from_slice(&task.to_bytes());
    }
    let mut flags = cpu_flag(header_flags::CPU_AVAILABLE) | header_flags::MEMORY_INTERVAL;
    if complete {
        flags |= header_flags::COMPLETE;
    }
    let header = TelemetryHeaderV1 {
        version: TELEMETRY_VERSION,
        flags,
        provider_epoch,
        sample_id,
        capture_start_ns: clock.unwrap_or(0),
        capture_end_ns: 0,
        online_cpus: count(super::smp::online_hart_count()),
        scope: SCOPE_SYSTEM,
        total_cells: count(total_cells),
        total_tasks: count(total_tasks),
        emitted_cells: count(emitted_cells),
        emitted_tasks: count(emitted_tasks),
        ..TelemetryHeaderV1::default()
    };
    Ok(Capture { header, bytes })
}

impl Capture {
    /// Fill the system-memory block and the end time, and return the bytes to
    /// copy out. `end` is read after the memory block.
    pub(crate) fn finish(mut self, memory: SystemMemory, end: Option<u64>) -> Vec<u8> {
        match memory {
            SystemMemory::NotRequested | SystemMemory::Unavailable => {}
            SystemMemory::Denied => self.header.flags |= header_flags::SYSMEM_DENIED,
            SystemMemory::Present(info) => {
                self.header.flags |= header_flags::SYSMEM_PRESENT;
                self.header.mem_total_frames = info.total_frames;
                self.header.mem_used_frames = info.used_frames;
                self.header.mem_free_frames = info.free_frames;
                self.header.mem_page_size = info.page_size;
            }
        }
        if self.header.flags & header_flags::CPU_AVAILABLE != 0 {
            let start = self.header.capture_start_ns;
            self.header.capture_end_ns = end.filter(|&end| end != 0).unwrap_or(start).max(start);
        }
        self.bytes[..HEADER_LEN].copy_from_slice(&self.header.to_bytes());
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::tcb::{Task, TaskState};
    use alloc::boxed::Box;
    use api::cell_owner::CellOwner;
    use api::telemetry::{
        name_bytes, TelemetryDecodeError, TelemetrySnapshot, NAME_LEN,
    };

    const EPOCH: u64 = 77;
    const CELL_A: u64 = 57;
    const CELL_B: u64 = 56;
    const GEN_A: u64 = 20;
    const GEN_B: u64 = 30;

    /// Saves and restores every hart's published current task.
    struct Harts([usize; MAX_HARTS]);
    impl Harts {
        fn save() -> Self {
            let mut saved = [0; MAX_HARTS];
            for (hart, slot) in saved.iter_mut().enumerate() {
                *slot = rl::current_task_id_for(hart);
                rl::set_current_task_id(hart, 0);
            }
            Self(saved)
        }
    }
    impl Drop for Harts {
        fn drop(&mut self) {
            for (hart, &tid) in self.0.iter().enumerate() {
                rl::set_current_task_id(hart, tid);
            }
        }
    }

    fn task(tid: usize, cell: u64, generation: u64, root: usize, name: &str) -> Box<Task> {
        let mut task = Box::new(Task::new(tid, CellId(cell), name, Vec::new()));
        task.cell_generation = generation;
        task.root_tid = root;
        task.state = TaskState::Ready;
        task
    }

    fn live(sched: &mut Scheduler, cell: u64, generation: u64, root: usize) -> CellOwner {
        let owner = CellOwner::new(cell, generation, root as u64);
        sched.reserve_session_owner(owner).expect("vacant slot");
        sched.publish_session_owner(owner);
        owner
    }

    fn run(sched: &mut Scheduler, tid: usize, now: u64) {
        rl::set_current_task_id(0, tid);
        sched.charge_cpu(0, Some(now));
    }

    fn take(sched: &mut Scheduler, now: Option<u64>, out_len: usize, tasks: bool) -> Vec<u8> {
        capture(sched, EPOCH, now, out_len, tasks)
            .expect("capture allocates")
            .finish(SystemMemory::NotRequested, now)
    }

    /// Two Cells whose CellId order is the reverse of their generation order,
    /// one ownerless kernel task, and a known CPU history on hart 0:
    /// a-root 600 ns, a-worker 400 ns, b-root 500 ns, then b-root running.
    fn two_cells() -> Scheduler {
        let mut sched = Scheduler::new();
        sched.tasks.insert(1, task(1, 0, 0, 1, "kernel-idle"));
        sched.tasks.insert(100, task(100, CELL_A, GEN_A, 100, "a-root"));
        let mut worker = task(103, CELL_A, GEN_A, 100, "a-worker");
        worker.stack_quota_charge = 4096;
        sched.tasks.insert(103, worker);
        sched.tasks.insert(101, task(101, CELL_B, GEN_B, 101, "b-root"));
        live(&mut sched, CELL_A, GEN_A, 100);
        live(&mut sched, CELL_B, GEN_B, 101);
        run(&mut sched, 100, 1_000); // first stamp charges nothing
        run(&mut sched, 100, 1_600);
        run(&mut sched, 103, 2_000);
        run(&mut sched, 101, 2_500);
        sched
    }

    #[test]
    fn pass_charges_the_interval_and_ignores_missing_or_backwards_clocks() {
        let mut ledger = CpuLedger::new();
        assert_eq!(ledger.pass(0, Some(500)), 0, "first stamp");
        assert_eq!(ledger.pass(0, Some(800)), 300);
        assert_eq!(ledger.pass(0, None), 0);
        assert_eq!(ledger.pass(0, Some(0)), 0, "HPET absent reads 0");
        assert_eq!(ledger.pass(0, Some(700)), 0, "a backwards clock charges nothing");
        assert_eq!(ledger.pass(0, Some(1_000)), 300);
        assert_eq!(ledger.pass(MAX_HARTS, Some(2_000)), 0, "no such hart");
        assert_eq!(ledger.pass(1, Some(9_000)), 0, "harts are stamped independently");
    }

    #[test]
    fn snapshot_orders_rows_and_sums_cell_cpu_including_the_running_interval() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let _harts = Harts::save();
        let mut sched = two_cells();
        crate::memory::cell_quota::register(CellId(CELL_A), 1 << 20);
        // b-root has been running since 2 500; the capture adds 100 ns in flight.
        let bytes = take(&mut sched, Some(2_600), 1 << 16, true);
        crate::memory::cell_quota::deregister(CellId(CELL_A));
        let snapshot = TelemetrySnapshot::parse(&bytes).expect("kernel bytes decode");
        let header = snapshot.header();
        assert_eq!((header.provider_epoch, header.sample_id), (EPOCH, 1));
        assert_eq!((header.capture_start_ns, header.capture_end_ns), (2_600, 2_600));
        assert_eq!((header.total_cells, header.emitted_cells), (2, 2));
        assert_eq!((header.total_tasks, header.emitted_tasks), (4, 4));
        assert!(header.has(header_flags::COMPLETE) && header.has(header_flags::CPU_AVAILABLE));
        assert!(!header.has(header_flags::SYSMEM_PRESENT) && !header.has(header_flags::SYSMEM_DENIED));

        let cells: Vec<_> = snapshot.cells().collect();
        let ids: Vec<_> = cells.iter().map(|c| (c.cell_id, c.generation, c.cpu_ns, c.task_count)).collect();
        // Generation order, not CellId order; exactly the member CPU, no clamp.
        assert_eq!(ids, [(CELL_A, GEN_A, 1_000, 2), (CELL_B, GEN_B, 600, 1)]);
        assert_eq!(name_bytes(&cells[0].name), b"a-root");
        assert_eq!(cells[0].root_tid, 100);
        assert!(cells[0].has(cell_flags::LIMIT_AVAILABLE) && cells[0].quota_limit_bytes == 1 << 20);
        assert!(!cells[1].has(cell_flags::LIMIT_AVAILABLE) && cells[1].quota_limit_bytes == 0);
        assert!(cells.iter().all(|c| c.state == cell_state::LIVE && c.has(cell_flags::CPU_AVAILABLE)));

        let tasks: Vec<_> = snapshot.tasks().map(|t| (t.generation, t.tid, t.cpu_ns)).collect();
        assert_eq!(tasks, [(0, 1, 0), (GEN_A, 100, 600), (GEN_A, 103, 400), (GEN_B, 101, 600)]);
        let worker = snapshot.tasks().find(|t| t.tid == 103).expect("worker row");
        assert!(worker.has(task_flags::STACK_IN_QUOTA));
        assert!(!snapshot.tasks().find(|t| t.tid == 100).expect("root row").has(task_flags::STACK_IN_QUOTA));
        // The in-flight interval is reported, never written back.
        assert_eq!(sched.tasks.get(&101).map(|t| t.cpu_ns), Some(500));

        let again = take(&mut sched, Some(2_700), 1 << 16, false);
        let again = TelemetrySnapshot::parse(&again).expect("cells only");
        assert_eq!(again.header().sample_id, 2);
        assert_eq!((again.header().total_tasks, again.header().emitted_tasks), (4, 0));
        assert!(again.header().has(header_flags::COMPLETE), "tasks were not requested");
    }

    #[test]
    fn a_short_buffer_keeps_whole_rows_and_reports_what_it_dropped() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let _harts = Harts::save();
        let mut sched = two_cells();
        // Room for one Cell and one task, but 1 byte short of a second Cell.
        let room = HEADER_LEN + 2 * CELL_LEN - 1;
        assert!(room >= HEADER_LEN + CELL_LEN + TASK_LEN);
        let bytes = take(&mut sched, Some(2_600), room, true);
        assert_eq!(bytes.len(), HEADER_LEN + CELL_LEN + TASK_LEN);
        let snapshot = TelemetrySnapshot::parse(&bytes).expect("truncated but valid");
        let header = snapshot.header();
        assert!(!header.has(header_flags::COMPLETE));
        assert_eq!((header.total_cells, header.emitted_cells), (2, 1));
        assert_eq!((header.total_tasks, header.emitted_tasks), (4, 1));
        assert_eq!(snapshot.cells().next().map(|c| c.generation), Some(GEN_A));

        let bytes = take(&mut sched, Some(2_600), HEADER_LEN, true);
        let snapshot = TelemetrySnapshot::parse(&bytes).expect("header only");
        assert_eq!((snapshot.header().emitted_cells, snapshot.header().emitted_tasks), (0, 0));
        assert!(!snapshot.header().has(header_flags::COMPLETE));
    }

    #[test]
    fn an_exited_worker_keeps_its_cpu_in_the_cell() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let _harts = Harts::save();
        let mut sched = two_cells();
        // a-worker runs 300 ns more, exits, and is still on the hart for 200 ns.
        run(&mut sched, 103, 2_800);
        sched.exit_task(103, 0);
        assert!(sched.tasks.get(&103).is_none() && sched.zombies.iter().any(|t| t.id == 103));
        run(&mut sched, 103, 3_000);
        let bytes = take(&mut sched, Some(3_050), 1 << 16, true);
        let snapshot = TelemetrySnapshot::parse(&bytes).expect("decode");
        let a = snapshot.cells().find(|c| c.cell_id == CELL_A).expect("cell A");
        // 600 (root) + 400 + 300 + 200 (exited worker) + 50 in flight on the zombie.
        assert_eq!((a.cpu_ns, a.task_count), (1_550, 1));
        assert!(snapshot.tasks().all(|t| t.tid != 103), "an exited task has no row");
        assert_eq!(sched.cpu_telemetry.exited(CELL_A, GEN_A), 900);
    }

    #[test]
    fn a_new_generation_in_a_reused_cell_id_starts_from_zero() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let _harts = Harts::save();
        let mut sched = Scheduler::new();
        sched.cpu_telemetry.retain_exited(CELL_A, GEN_A, 1_000);
        let next = GEN_A + 5;
        sched.tasks.insert(200, task(200, CELL_A, next, 200, "a-again"));
        sched.tasks.get_mut(&200).expect("root").cpu_ns = 50;
        live(&mut sched, CELL_A, next, 200);
        let bytes = take(&mut sched, Some(5_000), 1 << 16, true);
        let snapshot = TelemetrySnapshot::parse(&bytes).expect("decode");
        let cell = snapshot.cells().next().expect("one cell");
        assert_eq!((cell.cell_id, cell.generation, cell.cpu_ns), (CELL_A, next, 50));

        sched.cpu_telemetry.retain_exited(CELL_A, next, 7);
        sched.cpu_telemetry.retain_exited(CELL_A, GEN_A, 999); // stale: dropped
        assert_eq!(sched.cpu_telemetry.exited(CELL_A, next), 7);
        assert_eq!(sched.cpu_telemetry.exited(CELL_A, GEN_A), 0);
    }

    #[test]
    fn an_unavailable_clock_is_flagged_not_reported_as_zero_load() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let _harts = Harts::save();
        let mut sched = two_cells();
        let bytes = take(&mut sched, None, 1 << 16, true);
        let snapshot = TelemetrySnapshot::parse(&bytes).expect("decode");
        assert!(!snapshot.header().has(header_flags::CPU_AVAILABLE));
        assert_eq!(snapshot.header().capture_start_ns, 0);
        assert!(snapshot.cells().all(|c| !c.has(cell_flags::CPU_AVAILABLE)));
        assert!(snapshot.tasks().all(|t| !t.has(task_flags::CPU_AVAILABLE)));
    }

    #[test]
    fn system_memory_is_present_only_when_allowed() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let _harts = Harts::save();
        let mut sched = Scheduler::new();
        let denied = capture(&mut sched, EPOCH, Some(10), HEADER_LEN, false)
            .expect("capture")
            .finish(SystemMemory::Denied, Some(12));
        let denied = TelemetrySnapshot::parse(&denied).expect("decode");
        assert!(denied.header().has(header_flags::SYSMEM_DENIED));
        assert_eq!(denied.header().mem_total_frames, 0);
        assert_eq!(denied.header().capture_end_ns, 12);

        let info = ViMemInfoV1 { total_frames: 9, used_frames: 4, free_frames: 5, page_size: 4096 };
        let present = capture(&mut sched, EPOCH, Some(10), HEADER_LEN, false)
            .expect("capture")
            .finish(SystemMemory::Present(info), Some(11));
        let present = TelemetrySnapshot::parse(&present).expect("decode");
        assert!(present.header().has(header_flags::SYSMEM_PRESENT));
        assert_eq!((present.header().mem_used_frames, present.header().mem_page_size), (4, 4096));
    }

    #[test]
    fn kernel_names_are_cut_and_flagged_like_the_api_says() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let _harts = Harts::save();
        let mut sched = Scheduler::new();
        let long = "n".repeat(NAME_LEN + 3);
        sched.tasks.insert(300, task(300, CELL_B, GEN_B, 300, &long));
        live(&mut sched, CELL_B, GEN_B, 300);
        let bytes = take(&mut sched, Some(1), 1 << 16, true);
        let snapshot = TelemetrySnapshot::parse(&bytes).expect("decode");
        assert!(snapshot.cells().all(|c| c.has(cell_flags::NAME_TRUNCATED)));
        assert!(snapshot.tasks().all(|t| t.has(task_flags::NAME_TRUNCATED)));
        assert_eq!(TelemetrySnapshot::parse(&bytes[..bytes.len() - 1]).err(), Some(TelemetryDecodeError::Length));
    }
}
