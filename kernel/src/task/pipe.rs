//! Kernel-owned pipes — one-way byte streams between tasks (ADR-0018 §2.1).
//!
//! A pipe is a fixed-capacity ring with two endpoint kinds (read/write). Handles
//! live in the owning task's table, so a handle is usable by exactly the task that
//! holds it: `PipeShare` duplicates one end into another task's table, and nothing
//! else moves it. That is what makes a handle capability-like without adding an
//! allowlist bit — the authority is ownership, and a task cannot share what it does
//! not own.
//!
//! Semantics, chosen to match what a ported `pipe(2)`/`popen(3)` needs:
//!
//! * **Backpressure is real.** `PipeWrite` copies at most what fits and returns the
//!   count; when the ring is full it parks until a reader drains it (or the deadline
//!   expires). The ring never grows past its creation capacity.
//! * **EOF is exact.** A zero-length write closes the writer end. Readers observe
//!   `0` bytes only when the ring is empty *and* no writer end remains — including
//!   when the last writer died, exited, or faulted, because task teardown closes its
//!   endpoints.
//! * **No post-close reads.** Once an end is closed, its handle is gone; the data
//!   already in the ring stays readable until drained, which is what a pipe does.
//! * **Deadlines, not poll.** Every blocking call takes a deadline so a cell can run
//!   an event loop without `poll(2)`; a wait that times out returns 0 bytes for a
//!   read and 0 for a write, and the caller re-checks.
//!
//! Locking: `PIPES` is a leaf lock taken under `SCHEDULER` (same order as the futex
//! wait queues and the ready queues), never the other way round.

use crate::sync::Spinlock;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;

/// Default ring capacity when a caller asks for 0.
pub(crate) const DEFAULT_CAPACITY: usize = 4096;
/// Smallest ring worth creating.
pub(crate) const MIN_CAPACITY: usize = 64;
/// Largest ring: the whole pipe is kernel memory charged to the creator's quota, so
/// this is a per-pipe ceiling, not a policy on how many pipes a cell may hold.
pub(crate) const MAX_CAPACITY: usize = 64 * 1024;

/// Which end of a pipe a handle names.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum PipeEnd {
    Read,
    Write,
}

/// A handle as the ABI carries it: pipe identity plus end kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct PipeHandle {
    pub(crate) pipe: usize,
    pub(crate) end: PipeEnd,
}

struct Pipe {
    buffer: Vec<u8>,
    head: usize,
    len: usize,
    readers: usize,
    writers: usize,
    read_waiters: VecDeque<usize>,
    write_waiters: VecDeque<usize>,
    /// Cell charged for the ring; the charge is refunded when the pipe is dropped.
    owner_cell: usize,
}

impl Pipe {
    fn capacity(&self) -> usize {
        self.buffer.len()
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn is_full(&self) -> bool {
        self.len == self.buffer.len()
    }

    /// Copy `src` into the ring, returning how many bytes fit.
    fn write(&mut self, src: &[u8]) -> usize {
        let capacity = self.capacity();
        let space = capacity - self.len;
        let count = space.min(src.len());
        for (offset, byte) in src.iter().take(count).enumerate() {
            let index = (self.head + self.len + offset) % capacity;
            self.buffer[index] = *byte;
        }
        self.len += count;
        count
    }

    /// Copy up to `dst.len()` bytes out of the ring.
    fn read(&mut self, dst: &mut [u8]) -> usize {
        let capacity = self.capacity();
        let count = self.len.min(dst.len());
        for (offset, slot) in dst.iter_mut().take(count).enumerate() {
            let index = (self.head + offset) % capacity;
            *slot = self.buffer[index];
        }
        self.head = (self.head + count) % capacity;
        self.len -= count;
        count
    }
}

static PIPES: Spinlock<BTreeMap<usize, Pipe>> = Spinlock::new(BTreeMap::new());
static NEXT_PIPE_ID: Spinlock<usize> = Spinlock::new(1);

/// Pipe rings are kernel objects. Charge their capacity explicitly to the
/// creating cell, but allocate and free their storage as kernel memory so the
/// allocator never silently adds a second per-cell charge or refunds a peer
/// that happens to close the last end.
struct PipeAllocationContext {
    previous_cell: usize,
}

impl PipeAllocationContext {
    fn enter() -> Self {
        let previous_cell = crate::task::hart_local::current_cell_id();
        crate::task::hart_local::set_current_cell_id(0);
        Self { previous_cell }
    }
}

impl Drop for PipeAllocationContext {
    fn drop(&mut self) {
        crate::task::hart_local::set_current_cell_id(self.previous_cell);
    }
}

fn release_pipe(pipe: Pipe) {
    let owner_cell = pipe.owner_cell;
    let capacity = pipe.capacity();
    let _kernel_allocation = PipeAllocationContext::enter();
    drop(pipe);
    crate::memory::cell_quota::refund(owner_cell, capacity);
}

fn next_pipe_id() -> usize {
    let mut next = NEXT_PIPE_ID.lock();
    let id = *next;
    *next = next.saturating_add(1);
    id
}

/// Encode a handle for the ABI: `pipe_id << 1 | end_bit`.
impl PipeHandle {
    pub(crate) fn to_raw(self) -> usize {
        (self.pipe << 1) | matches!(self.end, PipeEnd::Write) as usize
    }

    /// Decode an ABI handle. A zero value is not a valid handle (pipe ids start
    /// at 1), so it is rejected here rather than looked up.
    pub(crate) fn from_raw(raw: usize) -> Option<Self> {
        let pipe = raw >> 1;
        if pipe == 0 {
            return None;
        }
        Some(Self {
            pipe,
            end: if raw & 1 == 1 {
                PipeEnd::Write
            } else {
                PipeEnd::Read
            },
        })
    }
}

/// Create a pipe. The ring capacity is charged exactly once to `owner_cell`.
pub(crate) fn create(
    capacity: usize,
    owner_cell: usize,
) -> Result<(PipeHandle, PipeHandle), crate::task::syscall::SyscallError> {
    let capacity = if capacity == 0 {
        DEFAULT_CAPACITY
    } else {
        capacity.clamp(MIN_CAPACITY, MAX_CAPACITY)
    };
    if !crate::memory::cell_quota::charge(owner_cell, capacity) {
        return Err(crate::task::syscall::SyscallError::OutOfMemory);
    }

    let _kernel_allocation = PipeAllocationContext::enter();
    let mut buffer = Vec::new();
    if buffer.try_reserve_exact(capacity).is_err() {
        drop(_kernel_allocation);
        crate::memory::cell_quota::refund(owner_cell, capacity);
        return Err(crate::task::syscall::SyscallError::OutOfMemory);
    }
    buffer.resize(capacity, 0);
    let pipe = Pipe {
        buffer,
        head: 0,
        len: 0,
        readers: 1,
        writers: 1,
        read_waiters: VecDeque::new(),
        write_waiters: VecDeque::new(),
        owner_cell,
    };
    let id = next_pipe_id();
    PIPES.lock().insert(id, pipe);
    Ok((
        PipeHandle {
            pipe: id,
            end: PipeEnd::Read,
        },
        PipeHandle {
            pipe: id,
            end: PipeEnd::Write,
        },
    ))
}

/// Duplicate an existing end for another task. The endpoint count—not merely
/// object liveness—defines EOF and broken-pipe behavior, so every shared handle
/// must be represented here before it is installed in a task table.
pub(crate) fn duplicate_end(handle: PipeHandle) -> bool {
    let mut pipes = PIPES.lock();
    let Some(pipe) = pipes.get_mut(&handle.pipe) else {
        return false;
    };
    match handle.end {
        PipeEnd::Read => pipe.readers = pipe.readers.saturating_add(1),
        PipeEnd::Write => pipe.writers = pipe.writers.saturating_add(1),
    }
    true
}

/// Close one end. Returns true when the pipe object was dropped (last end).
///
/// Closing the last writer end makes readers observe EOF; closing the last reader
/// end makes writers fail with a closed pipe.
pub(crate) fn close_end(handle: PipeHandle, tid: usize) -> bool {
    let mut pipes = PIPES.lock();
    let Some(pipe) = pipes.get_mut(&handle.pipe) else {
        return false;
    };
    match handle.end {
        PipeEnd::Read => {
            pipe.readers = pipe.readers.saturating_sub(1);
            pipe.read_waiters.retain(|queued| *queued != tid);
        }
        PipeEnd::Write => {
            pipe.writers = pipe.writers.saturating_sub(1);
            pipe.write_waiters.retain(|queued| *queued != tid);
        }
    }
    // Both ends gone: release the object and refund the ring.
    if pipe.readers == 0 && pipe.writers == 0 {
        let pipe = pipes
            .remove(&handle.pipe)
            .expect("pipe remained present while holding PIPES");
        drop(pipes);
        release_pipe(pipe);
        return true;
    }
    false
}

/// Writer ends still attached to this pipe.
fn writers_of(pipe: &Pipe) -> usize {
    pipe.writers
}

/// Result of a write attempt.
pub(crate) enum WriteOutcome {
    /// Bytes accepted (0 with `closed` set means the reader end is gone).
    Wrote(usize),
    /// The ring was full; the caller should park and retry.
    Full,
    /// No reader end remains: this is the `EPIPE`-shaped case.
    NoReader,
}

/// Try to write into the pipe.
pub(crate) fn try_write(handle: PipeHandle, src: &[u8], tid: usize) -> Option<WriteOutcome> {
    let mut pipes = PIPES.lock();
    let pipe = pipes.get_mut(&handle.pipe)?;
    if pipe.readers == 0 {
        pipe.write_waiters.retain(|queued| *queued != tid);
        return Some(WriteOutcome::NoReader);
    }
    if pipe.is_full() {
        return Some(WriteOutcome::Full);
    }
    let written = pipe.write(src);
    // A reader may now make progress.
    let waiters: Vec<usize> = pipe.read_waiters.drain(..).collect();
    drop(pipes);
    for waiter in waiters {
        crate::task::wake_task(waiter, 0);
    }
    Some(WriteOutcome::Wrote(written))
}

/// Result of a read attempt.
pub(crate) enum ReadOutcome {
    /// Bytes read; 0 with `eof` set means the stream ended.
    Read(usize),
    Eof,
    /// The ring was empty but a writer end remains: park and retry.
    Empty,
}

/// Try to read from the pipe.
pub(crate) fn try_read(handle: PipeHandle, dst: &mut [u8], tid: usize) -> Option<ReadOutcome> {
    let mut pipes = PIPES.lock();
    let pipe = pipes.get_mut(&handle.pipe)?;
    if !pipe.is_empty() {
        let read = pipe.read(dst);
        // A writer may now make progress.
        let waiters: Vec<usize> = pipe.write_waiters.drain(..).collect();
        drop(pipes);
        for waiter in waiters {
            crate::task::wake_task(waiter, 0);
        }
        return Some(ReadOutcome::Read(read));
    }
    if writers_of(pipe) == 0 {
        pipe.read_waiters.retain(|queued| *queued != tid);
        return Some(ReadOutcome::Eof);
    }
    Some(ReadOutcome::Empty)
}

/// Park `tid` as a reader (waiting for data or EOF).
pub(crate) fn park_reader(handle: PipeHandle, tid: usize) {
    let mut pipes = PIPES.lock();
    if let Some(pipe) = pipes.get_mut(&handle.pipe) {
        if !pipe.read_waiters.contains(&tid) {
            pipe.read_waiters.push_back(tid);
        }
    }
}

/// Park `tid` as a writer (waiting for space or a reader).
pub(crate) fn park_writer(handle: PipeHandle, tid: usize) {
    let mut pipes = PIPES.lock();
    if let Some(pipe) = pipes.get_mut(&handle.pipe) {
        if !pipe.write_waiters.contains(&tid) {
            pipe.write_waiters.push_back(tid);
        }
    }
}

/// Drop every waiter entry and endpoint a dying task owned.
///
/// Called from task teardown, which is what turns a faulted or exited writer into
/// an EOF for its readers instead of a stream that hangs forever.
pub(crate) fn on_task_leaves(tid: usize, owned: &[PipeHandle]) {
    let mut to_wake = Vec::new();
    {
        let mut pipes = PIPES.lock();
        for handle in owned {
            let Some(pipe) = pipes.get_mut(&handle.pipe) else {
                continue;
            };
            match handle.end {
                PipeEnd::Read => {
                    pipe.readers = pipe.readers.saturating_sub(1);
                    pipe.read_waiters.retain(|queued| *queued != tid);
                    if pipe.readers == 0 {
                        // Writers lose their peer: wake them so they can fail.
                        to_wake.extend(pipe.write_waiters.drain(..));
                    }
                }
                PipeEnd::Write => {
                    pipe.writers = pipe.writers.saturating_sub(1);
                    pipe.write_waiters.retain(|queued| *queued != tid);
                    if pipe.writers == 0 {
                        // Readers see EOF: wake them to observe it.
                        to_wake.extend(pipe.read_waiters.drain(..));
                    }
                }
            }
        }
        // Release objects with no ends left.
        let dead: Vec<usize> = pipes
            .iter()
            .filter(|(_, pipe)| pipe.readers == 0 && pipe.writers == 0)
            .map(|(id, _)| *id)
            .collect();
        let released: Vec<Pipe> = dead
            .into_iter()
            .filter_map(|id| pipes.remove(&id))
            .collect();
        drop(pipes);
        for pipe in released {
            release_pipe(pipe);
        }
    }
    for waiter in to_wake {
        crate::task::wake_task(waiter, 0);
    }
}

/// Deadline sweep arm: a parked reader/writer whose deadline elapsed.
///
/// The task is woken with a 0-byte result so the caller re-checks and can run an
/// event loop without `poll(2)`.
pub(crate) fn on_deadline(handle: PipeHandle, tid: usize, end: PipeEnd) {
    let mut pipes = PIPES.lock();
    if let Some(pipe) = pipes.get_mut(&handle.pipe) {
        match end {
            PipeEnd::Read => pipe.read_waiters.retain(|queued| *queued != tid),
            PipeEnd::Write => pipe.write_waiters.retain(|queued| *queued != tid),
        }
    }
}
