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
    /// The ring's capacity, charged to the creating incarnation and refunded
    /// to it when the pipe is dropped.
    _charge: crate::memory::cell_quota::Reservation,
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

static PIPES: Spinlock<BTreeMap<usize, Pipe>> =
    Spinlock::ranked(BTreeMap::new(), crate::sync::rank::PIPES);
static NEXT_PIPE_ID: Spinlock<usize> = Spinlock::ranked(1, crate::sync::rank::NEXT_PIPE_ID);

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

/// Create a pipe. The ring capacity is charged exactly once to `owner`.
pub(crate) fn create(
    capacity: usize,
    owner: crate::memory::cell_quota::Charge,
) -> Result<(PipeHandle, PipeHandle), crate::task::syscall::SyscallError> {
    let capacity = if capacity == 0 {
        DEFAULT_CAPACITY
    } else {
        capacity.clamp(MIN_CAPACITY, MAX_CAPACITY)
    };
    let charge = crate::memory::cell_quota::Reservation::take(
        owner,
        crate::memory::cell_quota::Account::Memory,
        capacity,
    )
    .map_err(|_| crate::task::syscall::SyscallError::OutOfMemory)?;

    // The creator is charged through `charge`; the ring and the table entry
    // are kernel memory, so the allocator never adds a second per-cell charge.
    let _kernel =
        crate::memory::heap::ChargeScope::enter(crate::memory::cell_quota::Charge::KERNEL);
    let mut buffer = Vec::new();
    #[cfg(feature = "p01-trace")]
    let _semantic =
        crate::memory::heap::p01_owner::creation(None, crate::memory::heap::p01_owner::PIPE_SHARED);
    if buffer.try_reserve_exact(capacity).is_err() {
        return Err(crate::task::syscall::SyscallError::OutOfMemory);
    }
    buffer.resize(capacity, 0);
    #[cfg(feature = "p01-trace")]
    drop(_semantic);
    let pipe = Pipe {
        buffer,
        head: 0,
        len: 0,
        readers: 1,
        writers: 1,
        read_waiters: VecDeque::new(),
        write_waiters: VecDeque::new(),
        _charge: charge,
    };
    let id = next_pipe_id();
    {
        let mut pipes = PIPES.lock();
        #[cfg(feature = "p01-trace")]
        let _semantic = crate::memory::heap::p01_owner::creation(
            None,
            crate::memory::heap::p01_owner::PIPE_SHARED,
        );
        pipes.insert(id, pipe);
    }
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
        drop(pipe);
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
            #[cfg(feature = "p01-trace")]
            let _semantic = crate::memory::heap::p01_owner::creation(
                None,
                crate::memory::heap::p01_owner::PIPE_SHARED,
            );
            pipe.read_waiters.push_back(tid);
        }
    }
}

/// Park `tid` as a writer (waiting for space or a reader).
pub(crate) fn park_writer(handle: PipeHandle, tid: usize) {
    let mut pipes = PIPES.lock();
    if let Some(pipe) = pipes.get_mut(&handle.pipe) {
        if !pipe.write_waiters.contains(&tid) {
            #[cfg(feature = "p01-trace")]
            let _semantic = crate::memory::heap::p01_owner::creation(
                None,
                crate::memory::heap::p01_owner::PIPE_SHARED,
            );
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
        drop(released);
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

/// Session transport uses this same bounded ring owner, but never installs a
/// legacy PipeHandle in any task's legacy table. All calls hold SCHEDULER.
fn session_wake(sched: &mut crate::sched::scheduler::Scheduler, tid: usize) {
    if let Some(task) = sched.tasks.get_mut(&tid) {
        if matches!(
            task.state,
            crate::task::tcb::TaskState::Blocked {
                wait: crate::task::tcb::WaitKind::PipeRead { .. },
                ..
            } | crate::task::tcb::TaskState::Blocked {
                wait: crate::task::tcb::WaitKind::PipeWrite { .. },
                ..
            }
        ) {
            task.state = crate::task::tcb::TaskState::Ready;
            sched.push_ready(tid);
        }
    }
}
pub(crate) fn session_read(
    sched: &mut crate::sched::scheduler::Scheduler,
    handle: PipeHandle,
    dst: &mut [u8],
) -> Option<ReadOutcome> {
    let mut pipes = PIPES.lock();
    let pipe = pipes.get_mut(&handle.pipe)?;
    if !pipe.is_empty() {
        let count = pipe.read(dst);
        while let Some(tid) = pipe.write_waiters.pop_front() {
            session_wake(sched, tid);
        }
        Some(ReadOutcome::Read(count))
    } else if pipe.writers == 0 {
        Some(ReadOutcome::Eof)
    } else {
        Some(ReadOutcome::Empty)
    }
}
pub(crate) fn session_write(
    sched: &mut crate::sched::scheduler::Scheduler,
    handle: PipeHandle,
    src: &[u8],
) -> Option<WriteOutcome> {
    let mut pipes = PIPES.lock();
    let pipe = pipes.get_mut(&handle.pipe)?;
    if pipe.readers == 0 {
        return Some(WriteOutcome::NoReader);
    }
    if pipe.is_full() {
        return Some(WriteOutcome::Full);
    }
    let count = pipe.write(src);
    while let Some(tid) = pipe.read_waiters.pop_front() {
        session_wake(sched, tid);
    }
    Some(WriteOutcome::Wrote(count))
}
pub(crate) fn session_counts(handle: PipeHandle) -> Option<(usize, usize)> {
    PIPES
        .lock()
        .get(&handle.pipe)
        .map(|p| (p.readers, p.writers))
}
/// Discard buffered bytes and wake writers blocked on a now-empty ring.
pub(crate) fn session_flush(
    sched: &mut crate::sched::scheduler::Scheduler,
    handle: PipeHandle,
) -> bool {
    let mut pipes = PIPES.lock();
    let Some(pipe) = pipes.get_mut(&handle.pipe) else {
        return false;
    };
    pipe.head = 0;
    pipe.len = 0;
    while let Some(tid) = pipe.write_waiters.pop_front() {
        session_wake(sched, tid);
    }
    true
}
/// Wake every task parked on this ring so it rechecks its typed endpoint state
/// (closed, revoked or cancelled). A revoked holder is not necessarily the last
/// end, so end-count transitions alone cannot cover it.
pub(crate) fn session_wake_all(sched: &mut crate::sched::scheduler::Scheduler, handle: PipeHandle) {
    let mut pipes = PIPES.lock();
    let Some(pipe) = pipes.get_mut(&handle.pipe) else {
        return;
    };
    while let Some(tid) = pipe.read_waiters.pop_front() {
        session_wake(sched, tid);
    }
    while let Some(tid) = pipe.write_waiters.pop_front() {
        session_wake(sched, tid);
    }
}
pub(crate) fn session_close(
    sched: &mut crate::sched::scheduler::Scheduler,
    handle: PipeHandle,
) -> bool {
    let mut pipes = PIPES.lock();
    let Some(pipe) = pipes.get_mut(&handle.pipe) else {
        return false;
    };
    match handle.end {
        PipeEnd::Read => {
            assert!(pipe.readers != 0);
            pipe.readers -= 1;
        }
        PipeEnd::Write => {
            assert!(pipe.writers != 0);
            pipe.writers -= 1;
        }
    }
    let final_reader = pipe.readers == 0;
    if final_reader || pipe.writers == 0 {
        while let Some(tid) = pipe.read_waiters.pop_front() {
            session_wake(sched, tid);
        }
        while let Some(tid) = pipe.write_waiters.pop_front() {
            session_wake(sched, tid);
        }
    }
    if pipe.readers == 0 && pipe.writers == 0 {
        let pipe = pipes.remove(&handle.pipe).expect("locked ring disappeared");
        drop(pipes);
        drop(pipe);
    }
    final_reader
}
