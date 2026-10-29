// SPDX-License-Identifier: Apache-2.0
//! Cell-aware telemetry snapshot ABI (`TelemetrySnapshot = 430`).
//!
//! Phase 06 S6-1 (`.agents/261020-1750-cellbox-cli/phase-06-entry-dossier.md` §5,
//! approved CP1 2026-10-23). One call writes, in one capture, a header followed by
//! whole Cell rows and then whole task rows into a caller buffer:
//!
//! ```text
//! TelemetryHeaderV1 (96) | TelemetryCellV1 (96) × emitted_cells | TelemetryTaskV1 (88) × emitted_tasks
//! ```
//!
//! Identity is `(provider_epoch, cell_id, generation)` for a Cell incarnation and
//! `(provider_epoch, tid)` for a task: TIDs are never reissued within one boot and
//! generations are global, so neither needs a separate incarnation field (DV-1/DV-2).
//!
//! Units: every `*_ns` field is nanoseconds on the monotonic clock `GetTime(0)`
//! exposes, so `cpu_ns` deltas divide directly by `capture_start_ns` deltas.
//! `cpu_ns` is exact on-CPU time (charged at every scheduler pass), and a Cell's
//! `cpu_ns` includes the CPU of its already-exited threads for this incarnation.
//! Memory: `quota_charged_bytes` is the Cell's whole quota charge (kernel heap in
//! its context, worker stacks, pipes, session objects), not a heap size, and task
//! `user_stack_bytes`/`segment_bytes` are informational — never sum them into the
//! Cell (DV-4).
//!
//! Encoding is explicit little-endian at fixed offsets; the `repr(C)` structs are
//! the documented layout, never reinterpreted from raw bytes. A V1 row is never a
//! V2 row: a later layout gets a new version number.

/// Header `version` written by this layout.
pub const TELEMETRY_VERSION: u32 = 1;
/// Exact byte size of [`TelemetryHeaderV1`].
pub const HEADER_LEN: usize = 96;
/// Exact byte size of [`TelemetryCellV1`].
pub const CELL_LEN: usize = 96;
/// Exact byte size of [`TelemetryTaskV1`].
pub const TASK_LEN: usize = 88;
/// Required alignment of the caller buffer.
pub const BUFFER_ALIGN: usize = 8;
/// Bytes of a row's `name`; longer names are cut at a UTF-8 boundary and flagged.
pub const NAME_LEN: usize = 32;
/// Header `scope`: every Cell and task in the system (the only scope offered, DV-6).
pub const SCOPE_SYSTEM: u32 = 1;

/// `a2` request flags. Every other bit must be zero.
pub mod request {
    /// Emit task rows after the Cell rows.
    pub const TASKS: usize = 1 << 0;
    /// Fill the system-memory block (also needs the caller's `observe_memory` right).
    pub const SYSTEM_MEMORY: usize = 1 << 1;
    /// Every defined request bit.
    pub const ALL: usize = TASKS | SYSTEM_MEMORY;
}

/// [`TelemetryHeaderV1::flags`].
pub mod header_flags {
    /// Every requested row was emitted; no row was dropped for lack of space.
    pub const COMPLETE: u32 = 1 << 0;
    /// The monotonic clock was available: `cpu_ns` and the capture times are valid.
    pub const CPU_AVAILABLE: u32 = 1 << 1;
    /// The `mem_*` fields hold the global frame-allocator snapshot.
    pub const SYSMEM_PRESENT: u32 = 1 << 2;
    /// System memory was requested but the caller lacks `observe_memory`; `mem_*` are 0.
    pub const SYSMEM_DENIED: u32 = 1 << 3;
    /// Memory counters are lock-free and were read during the capture interval, so
    /// they are not instant-consistent with membership and CPU.
    pub const MEMORY_INTERVAL: u32 = 1 << 4;
    /// Every defined header flag.
    pub const ALL: u32 = COMPLETE | CPU_AVAILABLE | SYSMEM_PRESENT | SYSMEM_DENIED | MEMORY_INTERVAL;
}

/// [`TelemetryCellV1::flags`].
pub mod cell_flags {
    pub const CPU_AVAILABLE: u32 = 1 << 0;
    pub const QUOTA_AVAILABLE: u32 = 1 << 1;
    pub const LIMIT_AVAILABLE: u32 = 1 << 2;
    pub const NAME_TRUNCATED: u32 = 1 << 3;
    pub const ALL: u32 = CPU_AVAILABLE | QUOTA_AVAILABLE | LIMIT_AVAILABLE | NAME_TRUNCATED;
}

/// [`TelemetryTaskV1::flags`].
pub mod task_flags {
    pub const CPU_AVAILABLE: u32 = 1 << 0;
    pub const NAME_TRUNCATED: u32 = 1 << 1;
    /// The task's user stack is charged to its Cell's quota (worker threads), so
    /// it is already inside the Cell's `quota_charged_bytes`.
    pub const STACK_IN_QUOTA: u32 = 1 << 2;
    pub const ALL: u32 = CPU_AVAILABLE | NAME_TRUNCATED | STACK_IN_QUOTA;
}

/// [`TelemetryCellV1::state`].
pub mod cell_state {
    pub const LIVE: u32 = 0;
    pub const RETIRING: u32 = 1;
}

/// [`TelemetryTaskV1::state`].
pub mod task_state {
    pub const READY: u32 = 0;
    pub const RUNNING: u32 = 1;
    pub const WAITING: u32 = 2;
    pub const RETIRING: u32 = 3;
}

/// Snapshot header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TelemetryHeaderV1 {
    pub version: u32,
    pub flags: u32,
    /// Nonzero per-boot value; equal epochs mean the same boot.
    pub provider_epoch: u64,
    /// Strictly increasing per capture within one epoch.
    pub sample_id: u64,
    /// The single `now` used for every in-flight CPU adjustment.
    pub capture_start_ns: u64,
    pub capture_end_ns: u64,
    pub online_cpus: u32,
    pub scope: u32,
    pub total_cells: u32,
    pub total_tasks: u32,
    pub emitted_cells: u32,
    pub emitted_tasks: u32,
    pub mem_total_frames: u64,
    pub mem_used_frames: u64,
    pub mem_free_frames: u64,
    pub mem_page_size: u64,
}

/// One live or retiring Cell incarnation.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TelemetryCellV1 {
    pub cell_id: u64,
    pub generation: u64,
    pub root_tid: u64,
    /// Exited members' CPU for this generation plus every live member's.
    pub cpu_ns: u64,
    pub quota_charged_bytes: u64,
    pub quota_limit_bytes: u64,
    pub task_count: u32,
    pub state: u32,
    pub flags: u32,
    pub reserved: u32,
    /// Root task name, NUL-padded.
    pub name: [u8; NAME_LEN],
}

/// One task.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TelemetryTaskV1 {
    pub tid: u64,
    pub cell_id: u64,
    pub generation: u64,
    pub cpu_ns: u64,
    pub user_stack_bytes: u64,
    pub segment_bytes: u64,
    pub state: u32,
    pub flags: u32,
    pub name: [u8; NAME_LEN],
}

/// Why kernel-written bytes are not a V1 snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TelemetryDecodeError {
    /// Fewer bytes than a header.
    Short,
    /// `version` is not [`TELEMETRY_VERSION`].
    Version(u32),
    /// `scope` is not [`SCOPE_SYSTEM`].
    Scope(u32),
    /// The byte count does not match the emitted row counts exactly.
    Length,
    /// A count, flag, state, reserved field or ordering rule is violated.
    Malformed,
}

/// Cut `name` to [`NAME_LEN`] bytes at a UTF-8 boundary; `true` when it was cut.
pub fn encode_name(name: &str) -> ([u8; NAME_LEN], bool) {
    let mut out = [0u8; NAME_LEN];
    let mut end = name.len().min(NAME_LEN);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    out[..end].copy_from_slice(&name.as_bytes()[..end]);
    (out, end < name.len())
}

/// The NUL-terminated prefix of a row name.
pub fn name_bytes(name: &[u8; NAME_LEN]) -> &[u8] {
    let end = name.iter().position(|&b| b == 0).unwrap_or(NAME_LEN);
    &name[..end]
}

fn put_u32(out: &mut [u8], at: usize, value: u32) {
    out[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn put_u64(out: &mut [u8], at: usize, value: u64) {
    out[at..at + 8].copy_from_slice(&value.to_le_bytes());
}
fn get_u32(bytes: &[u8], at: usize) -> u32 {
    let mut word = [0u8; 4];
    word.copy_from_slice(&bytes[at..at + 4]);
    u32::from_le_bytes(word)
}
fn get_u64(bytes: &[u8], at: usize) -> u64 {
    let mut word = [0u8; 8];
    word.copy_from_slice(&bytes[at..at + 8]);
    u64::from_le_bytes(word)
}
fn get_name(bytes: &[u8], at: usize) -> [u8; NAME_LEN] {
    let mut name = [0u8; NAME_LEN];
    name.copy_from_slice(&bytes[at..at + NAME_LEN]);
    name
}

impl TelemetryHeaderV1 {
    pub fn to_bytes(&self) -> [u8; HEADER_LEN] {
        let mut out = [0u8; HEADER_LEN];
        put_u32(&mut out, 0, self.version);
        put_u32(&mut out, 4, self.flags);
        put_u64(&mut out, 8, self.provider_epoch);
        put_u64(&mut out, 16, self.sample_id);
        put_u64(&mut out, 24, self.capture_start_ns);
        put_u64(&mut out, 32, self.capture_end_ns);
        put_u32(&mut out, 40, self.online_cpus);
        put_u32(&mut out, 44, self.scope);
        put_u32(&mut out, 48, self.total_cells);
        put_u32(&mut out, 52, self.total_tasks);
        put_u32(&mut out, 56, self.emitted_cells);
        put_u32(&mut out, 60, self.emitted_tasks);
        put_u64(&mut out, 64, self.mem_total_frames);
        put_u64(&mut out, 72, self.mem_used_frames);
        put_u64(&mut out, 80, self.mem_free_frames);
        put_u64(&mut out, 88, self.mem_page_size);
        out
    }

    pub fn read(bytes: &[u8; HEADER_LEN]) -> Self {
        Self {
            version: get_u32(bytes, 0),
            flags: get_u32(bytes, 4),
            provider_epoch: get_u64(bytes, 8),
            sample_id: get_u64(bytes, 16),
            capture_start_ns: get_u64(bytes, 24),
            capture_end_ns: get_u64(bytes, 32),
            online_cpus: get_u32(bytes, 40),
            scope: get_u32(bytes, 44),
            total_cells: get_u32(bytes, 48),
            total_tasks: get_u32(bytes, 52),
            emitted_cells: get_u32(bytes, 56),
            emitted_tasks: get_u32(bytes, 60),
            mem_total_frames: get_u64(bytes, 64),
            mem_used_frames: get_u64(bytes, 72),
            mem_free_frames: get_u64(bytes, 80),
            mem_page_size: get_u64(bytes, 88),
        }
    }

    pub const fn has(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }

    /// Exact byte count of the snapshot this header describes.
    pub fn snapshot_len(&self) -> Option<usize> {
        (self.emitted_cells as usize)
            .checked_mul(CELL_LEN)?
            .checked_add((self.emitted_tasks as usize).checked_mul(TASK_LEN)?)?
            .checked_add(HEADER_LEN)
    }

    fn validate(&self) -> Result<(), TelemetryDecodeError> {
        use header_flags::*;
        if self.version != TELEMETRY_VERSION {
            return Err(TelemetryDecodeError::Version(self.version));
        }
        if self.scope != SCOPE_SYSTEM {
            return Err(TelemetryDecodeError::Scope(self.scope));
        }
        let memory = [self.mem_total_frames, self.mem_used_frames, self.mem_free_frames, self.mem_page_size];
        let malformed = self.flags & !ALL != 0
            || self.provider_epoch == 0
            || self.sample_id == 0
            || self.emitted_cells > self.total_cells
            || self.emitted_tasks > self.total_tasks
            || (self.has(COMPLETE) && self.emitted_cells != self.total_cells)
            || (self.has(SYSMEM_PRESENT) && self.has(SYSMEM_DENIED))
            || (!self.has(SYSMEM_PRESENT) && memory.iter().any(|&word| word != 0))
            || (self.has(CPU_AVAILABLE) && self.capture_end_ns < self.capture_start_ns);
        if malformed { Err(TelemetryDecodeError::Malformed) } else { Ok(()) }
    }
}

impl TelemetryCellV1 {
    pub fn to_bytes(&self) -> [u8; CELL_LEN] {
        let mut out = [0u8; CELL_LEN];
        put_u64(&mut out, 0, self.cell_id);
        put_u64(&mut out, 8, self.generation);
        put_u64(&mut out, 16, self.root_tid);
        put_u64(&mut out, 24, self.cpu_ns);
        put_u64(&mut out, 32, self.quota_charged_bytes);
        put_u64(&mut out, 40, self.quota_limit_bytes);
        put_u32(&mut out, 48, self.task_count);
        put_u32(&mut out, 52, self.state);
        put_u32(&mut out, 56, self.flags);
        put_u32(&mut out, 60, self.reserved);
        out[64..96].copy_from_slice(&self.name);
        out
    }

    pub fn read(bytes: &[u8; CELL_LEN]) -> Self {
        Self {
            cell_id: get_u64(bytes, 0),
            generation: get_u64(bytes, 8),
            root_tid: get_u64(bytes, 16),
            cpu_ns: get_u64(bytes, 24),
            quota_charged_bytes: get_u64(bytes, 32),
            quota_limit_bytes: get_u64(bytes, 40),
            task_count: get_u32(bytes, 48),
            state: get_u32(bytes, 52),
            flags: get_u32(bytes, 56),
            reserved: get_u32(bytes, 60),
            name: get_name(bytes, 64),
        }
    }

    pub const fn has(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }

    fn valid(&self) -> bool {
        self.reserved == 0
            && self.flags & !cell_flags::ALL == 0
            && self.state <= cell_state::RETIRING
            && self.generation != 0
    }
}

impl TelemetryTaskV1 {
    pub fn to_bytes(&self) -> [u8; TASK_LEN] {
        let mut out = [0u8; TASK_LEN];
        put_u64(&mut out, 0, self.tid);
        put_u64(&mut out, 8, self.cell_id);
        put_u64(&mut out, 16, self.generation);
        put_u64(&mut out, 24, self.cpu_ns);
        put_u64(&mut out, 32, self.user_stack_bytes);
        put_u64(&mut out, 40, self.segment_bytes);
        put_u32(&mut out, 48, self.state);
        put_u32(&mut out, 52, self.flags);
        out[56..88].copy_from_slice(&self.name);
        out
    }

    pub fn read(bytes: &[u8; TASK_LEN]) -> Self {
        Self {
            tid: get_u64(bytes, 0),
            cell_id: get_u64(bytes, 8),
            generation: get_u64(bytes, 16),
            cpu_ns: get_u64(bytes, 24),
            user_stack_bytes: get_u64(bytes, 32),
            segment_bytes: get_u64(bytes, 40),
            state: get_u32(bytes, 48),
            flags: get_u32(bytes, 52),
            name: get_name(bytes, 56),
        }
    }

    pub const fn has(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }

    fn valid(&self) -> bool {
        self.flags & !task_flags::ALL == 0 && self.state <= task_state::RETIRING && self.tid != 0
    }
}

/// A validated view over kernel-written snapshot bytes.
#[derive(Clone, Copy, Debug)]
pub struct TelemetrySnapshot<'a> {
    header: TelemetryHeaderV1,
    cells: &'a [u8],
    tasks: &'a [u8],
}

impl<'a> TelemetrySnapshot<'a> {
    /// Parse exactly the bytes `TelemetrySnapshot` reported writing.
    ///
    /// Rejects anything but a whole V1 snapshot: wrong version or scope, a byte
    /// count that disagrees with the emitted counts, unknown flag bits, invalid
    /// states, a nonzero reserved field, or rows out of the documented order
    /// (Cells by generation, tasks by `(generation, tid)`).
    pub fn parse(bytes: &'a [u8]) -> Result<Self, TelemetryDecodeError> {
        let head: &[u8; HEADER_LEN] = bytes
            .get(..HEADER_LEN)
            .and_then(|head| head.try_into().ok())
            .ok_or(TelemetryDecodeError::Short)?;
        let header = TelemetryHeaderV1::read(head);
        header.validate()?;
        if header.snapshot_len() != Some(bytes.len()) {
            return Err(TelemetryDecodeError::Length);
        }
        let split = HEADER_LEN + header.emitted_cells as usize * CELL_LEN;
        let snapshot = Self { header, cells: &bytes[HEADER_LEN..split], tasks: &bytes[split..] };
        let mut previous = 0u64;
        for cell in snapshot.cells() {
            if !cell.valid() || cell.generation <= previous {
                return Err(TelemetryDecodeError::Malformed);
            }
            previous = cell.generation;
        }
        let mut previous = (0u64, 0u64);
        for task in snapshot.tasks() {
            let key = (task.generation, task.tid);
            if !task.valid() || key <= previous {
                return Err(TelemetryDecodeError::Malformed);
            }
            previous = key;
        }
        Ok(snapshot)
    }

    pub fn header(&self) -> &TelemetryHeaderV1 {
        &self.header
    }

    pub fn cells(&self) -> impl Iterator<Item = TelemetryCellV1> + 'a {
        self.cells
            .chunks_exact(CELL_LEN)
            .filter_map(|row| row.try_into().ok())
            .map(TelemetryCellV1::read)
    }

    pub fn tasks(&self) -> impl Iterator<Item = TelemetryTaskV1> + 'a {
        self.tasks
            .chunks_exact(TASK_LEN)
            .filter_map(|row| row.try_into().ok())
            .map(TelemetryTaskV1::read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, offset_of, size_of};

    #[test]
    fn layouts_match_the_documented_offsets() {
        assert_eq!((size_of::<TelemetryHeaderV1>(), align_of::<TelemetryHeaderV1>()), (HEADER_LEN, 8));
        assert_eq!((size_of::<TelemetryCellV1>(), align_of::<TelemetryCellV1>()), (CELL_LEN, 8));
        assert_eq!((size_of::<TelemetryTaskV1>(), align_of::<TelemetryTaskV1>()), (TASK_LEN, 8));
        assert_eq!(offset_of!(TelemetryHeaderV1, online_cpus), 40);
        assert_eq!(offset_of!(TelemetryHeaderV1, mem_total_frames), 64);
        assert_eq!(offset_of!(TelemetryCellV1, task_count), 48);
        assert_eq!(offset_of!(TelemetryCellV1, name), 64);
        assert_eq!(offset_of!(TelemetryTaskV1, state), 48);
        assert_eq!(offset_of!(TelemetryTaskV1, name), 56);
    }

    fn header(cells: u32, tasks: u32) -> TelemetryHeaderV1 {
        TelemetryHeaderV1 {
            version: TELEMETRY_VERSION,
            flags: header_flags::COMPLETE | header_flags::CPU_AVAILABLE,
            provider_epoch: 7,
            sample_id: 1,
            capture_start_ns: 10,
            capture_end_ns: 12,
            online_cpus: 1,
            scope: SCOPE_SYSTEM,
            total_cells: cells,
            total_tasks: tasks,
            emitted_cells: cells,
            emitted_tasks: tasks,
            ..TelemetryHeaderV1::default()
        }
    }

    fn cell(generation: u64) -> TelemetryCellV1 {
        TelemetryCellV1 { cell_id: 3, generation, root_tid: 9, cpu_ns: 5, task_count: 1,
            flags: cell_flags::CPU_AVAILABLE, name: encode_name("cell").0, ..TelemetryCellV1::default() }
    }

    fn task(generation: u64, tid: u64) -> TelemetryTaskV1 {
        TelemetryTaskV1 { tid, cell_id: 3, generation, cpu_ns: 5, state: task_state::RUNNING,
            flags: task_flags::CPU_AVAILABLE, name: encode_name("task").0, ..TelemetryTaskV1::default() }
    }

    fn encode(header: &TelemetryHeaderV1, cells: &[TelemetryCellV1], tasks: &[TelemetryTaskV1]) -> alloc::vec::Vec<u8> {
        let mut out = alloc::vec::Vec::from(header.to_bytes());
        cells.iter().for_each(|c| out.extend_from_slice(&c.to_bytes()));
        tasks.iter().for_each(|t| out.extend_from_slice(&t.to_bytes()));
        out
    }

    #[test]
    fn whole_snapshot_round_trips() {
        let h = header(2, 2);
        let cells = [cell(4), cell(6)];
        let tasks = [task(4, 20), task(6, 11)];
        let bytes = encode(&h, &cells, &tasks);
        let snapshot = TelemetrySnapshot::parse(&bytes).expect("valid snapshot");
        assert_eq!(*snapshot.header(), h);
        assert!(snapshot.cells().eq(cells));
        assert!(snapshot.tasks().eq(tasks));
    }

    #[test]
    fn byte_count_must_match_the_emitted_rows() {
        let bytes = encode(&header(1, 0), &[cell(4)], &[]);
        assert_eq!(TelemetrySnapshot::parse(&bytes[..bytes.len() - 1]).err(), Some(TelemetryDecodeError::Length));
        assert_eq!(TelemetrySnapshot::parse(&bytes[..HEADER_LEN - 1]).err(), Some(TelemetryDecodeError::Short));
        let mut longer = bytes.clone();
        longer.push(0);
        assert_eq!(TelemetrySnapshot::parse(&longer).err(), Some(TelemetryDecodeError::Length));
    }

    #[test]
    fn complete_cannot_hide_dropped_cells() {
        let mut h = header(1, 0);
        h.total_cells = 2;
        let bytes = encode(&h, &[cell(4)], &[]);
        assert_eq!(TelemetrySnapshot::parse(&bytes).err(), Some(TelemetryDecodeError::Malformed));
        h.flags &= !header_flags::COMPLETE;
        let bytes = encode(&h, &[cell(4)], &[]);
        let snapshot = TelemetrySnapshot::parse(&bytes).expect("explicitly truncated");
        assert_eq!(snapshot.header().total_cells - snapshot.header().emitted_cells, 1);
    }

    #[test]
    fn rows_out_of_order_or_unknown_bits_are_refused() {
        let bytes = encode(&header(2, 0), &[cell(6), cell(4)], &[]);
        assert_eq!(TelemetrySnapshot::parse(&bytes).err(), Some(TelemetryDecodeError::Malformed));
        let bytes = encode(&header(0, 2), &[], &[task(4, 20), task(4, 11)]);
        assert_eq!(TelemetrySnapshot::parse(&bytes).err(), Some(TelemetryDecodeError::Malformed));
        let mut odd = cell(4);
        odd.reserved = 1;
        assert!(TelemetrySnapshot::parse(&encode(&header(1, 0), &[odd], &[])).is_err());
        let mut h = header(0, 0);
        h.flags |= 1 << 31;
        assert!(TelemetrySnapshot::parse(&encode(&h, &[], &[])).is_err());
        h = header(0, 0);
        h.version = 2;
        assert_eq!(TelemetrySnapshot::parse(&encode(&h, &[], &[])).err(), Some(TelemetryDecodeError::Version(2)));
    }

    #[test]
    fn denied_memory_carries_no_values() {
        let mut h = header(0, 0);
        h.flags |= header_flags::SYSMEM_DENIED;
        assert!(TelemetrySnapshot::parse(&encode(&h, &[], &[])).is_ok());
        h.mem_total_frames = 1;
        assert!(TelemetrySnapshot::parse(&encode(&h, &[], &[])).is_err());
    }

    #[test]
    fn names_are_cut_on_a_char_boundary_and_flagged() {
        assert_eq!(name_bytes(&encode_name("top").0), b"top");
        assert!(!encode_name("top").1);
        let long = "é".repeat(17); // 34 bytes; 16 whole chars fit in 32
        let (name, cut) = encode_name(&long);
        assert!(cut);
        assert_eq!(name_bytes(&name).len(), 32);
        assert!(core::str::from_utf8(name_bytes(&name)).is_ok());
        // A two-byte char straddling byte 32 is dropped whole, never split.
        let (name, cut) = encode_name(&alloc::format!("{}é", "x".repeat(31)));
        assert!(cut);
        assert_eq!(name_bytes(&name), "x".repeat(31).as_bytes());
    }
}
