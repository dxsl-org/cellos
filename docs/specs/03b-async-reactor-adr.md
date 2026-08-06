# ADR: Completion Queues and the Async Reactor

**Date**: 2026-07-31 | **Status**: Accepted | **Authors**: Cellos core team

---

## Decision

Replace the busy-poll executor with a per-cell completion queue the kernel owns,
and let one parked thread serve many outstanding operations. Four questions had to
be answered first, because each is expensive to change afterwards:

1. **A completion is identified by a submission slot**, allocated when the
   operation is submitted, not by a task id.
2. **The slot is reserved at submission**, so a completion can never fail to find
   somewhere to land.
3. **Cancellation waits for the operation to report done and discards the result.**
   It never truncates work in flight.
4. **Appending a completion takes only the queue's own lock**, and waking the
   waiter is a separate step.

The Phase 04 contract is narrower than a generic reactor. `WaitCompletion` only
covers `NET_RX` and finite `TIMER` waits, while `WaitForEvent` and the `Recv*`
family keep their existing wake and delivery rules.

---

## Context

A cell today spends one thread, and one 512 KiB stack pair, on each thing it waits
for. The executor busy-polls with a dummy waker, so waiting costs CPU as well as
memory. The intended shape is ordinary: submit work, park once, and let the kernel
say which of the outstanding operations finished.

What makes this expensive to get wrong is that the parking mechanism is load-bearing
for message delivery. Three invariants in the current design depend on a waiting
task being parked in one specific state, and all three were verified against the
tree before this record was written:

- A non-blocking send only delivers when the target is parked in the receive state
  with its buffer registered. The shell's input path relies on exactly that. Park a
  cell in a completion queue instead and every such send is discarded in silence —
  the keyboard stops working and nothing reports why.
- Task exit unblocks a peer by matching the sending state, at three separate sites.
  A cell parked on a completion queue matches none of them, so instead of receiving
  an error the caller waits forever, and whatever supervises it waits too.
- The filesystem writes into a caller's grant on the strength of the caller being
  blocked inside its call. Make that future cancellable and the write lands in
  memory the caller has moved on from.

The implementation keeps the reservation and the parked state separate on
purpose. `TaskState::WaitCompletion` carries the source and deadline that the
timer sweep can wake, while `Task::completion_wait` carries the slot bookkeeping
so exit cleanup can release the reservation later without holding `SCHEDULER`.

None of these is an argument against the change. They are the reason the change
cannot be made incrementally in the obvious way, and the reason this record exists.

---

## Point 1 — A completion names a submission, not a task

The options were a task id, a capability id, or a dedicated handle.

A task id is wrong because it is not stable for the lifetime it would need to be.
Task ids are reused, a service that crashes and restarts comes back with a new one,
and a completion arriving for a task id says nothing about *which* of that task's
operations finished. It also conflates the thing waiting with the thing waited on,
which breaks as soon as one thread waits for several operations — the entire point
of the change.

Each submission is therefore given a slot, and the slot number is what a completion
carries. It is meaningful only within the cell that owns the queue, dies with that
cell, and cannot be forged into a reference to anything else. Revocation becomes
simple, because revoking is releasing the slot.

The v1 wire record is fixed at 24 bytes. Its source word sits in bytes 12..16,
between the slot and result fields. `WaitCompletion` submits exactly one source
bit: `NET_RX` or `TIMER`. Zero, multi-bit, and unknown submission masks are
rejected up front. When decoding a record written by the original v1 kernel,
source `0` is accepted as legacy `UNSPECIFIED`; that compatibility rule does not
make `0` a valid new submission.

## Point 2 — Reserve at submission so completion cannot fail

A bounded queue can fill. Dropping the entry loses a wakeup and the waiter hangs;
refusing the driver blocks it, and a driver that cannot report completion deadlocks
the system it was serving. The phase text is right that backpressure beats dropping,
but backpressure has to be applied where it can be handled, and an interrupt handler
cannot handle it.

So the slot is reserved when the operation is submitted, from the submitting cell's
own context, where a refusal is an ordinary error the caller can act on. By the time
anything is in flight its landing place already exists. Completion never allocates
and never fails.

The cost is that a cell's outstanding operations are capped by its queue size, and
a cell that submits without draining will be refused rather than served. That is the
correct direction: the failure is visible, attributable, and confined to the cell
that caused it.

`NET_RX` waits indefinitely when no deadline is supplied. If a finite deadline
expires before a frame is reported, the reservation is released and the syscall
returns `0` without writing a completion. `TIMER` is the opposite: it requires a
finite deadline, and when that deadline expires the resumed waiter writes a
synthetic completion with source `TIMER` and result `0` rather than asking the
interrupt path to append one.

## Point 3 — Cancellation discards a result, never truncates an operation

Cancelling a future must not let memory be reused while a device or the kernel is
still writing to it. The pinning registry already settled the general form of this:
frames stay withheld until an explicit acknowledgement, never on a timer and never
implicitly.

Cancellation therefore means the caller stops waiting for the result, not that the
work stops. The operation runs to completion, its slot stays reserved until it
does, and the result is discarded on arrival. There is one semantic and it does not
vary by driver, which is what the phase requires.

How completion is *signalled* still differs — the kernel knows when a message send
finished, a driver says when it has stopped touching a buffer. That is a difference
in who reports the event, not in what cancellation means, and conflating the two is
what produces a system where cancellation is safe against some drivers and not
others.

Stated plainly: cancelling does not make a slow operation return sooner. Anything
needing prompt abandonment needs a timeout on the operation itself.

## Point 4 — Appending holds one lock, waking is separate

The recorded lock order in this area was wrong and has been corrected: the frame
allocator is acquired first and held across mapping calls that take the page-table
root, not the reverse. The rule that matters is unchanged — neither may be taken
while the scheduler lock is held.

Appending a completion takes the queue's own leaf lock and nothing else. This is
possible only because the queue is kernel-owned memory reachable from the task
record rather than a grant, so there is no address to resolve and no allocator to
consult at append time. That property is the reason for the ownership choice, not a
convenience that follows from it.

Waking the waiter needs the scheduler and is therefore a separate step, deferred the
way the existing grant reap already defers work that cannot run under the sweep's
locks. An append followed by a deferred wake is safe from interrupt context; an
append that wakes inline is not. If a task dies while still holding a reservation,
`exit_task` records the `(tid, queue, slot)` tuple and `yield_cpu` later releases it
outside `SCHEDULER`; dead-task cleanup never frees a slot inline.

---

## Consequences

- One new syscall, and the ABI approval that requires. It is not needed for the
  queue itself, which is kernel-internal: a cell cannot reach it until something is
  migrated onto it, so the call is introduced with the first migration rather than
  ahead of it. Publishing an interface before anything exercises it freezes a shape
  chosen from guesswork.
- `WaitForEvent` keeps its bitmask wake semantics, and the `Recv`/`TryRecv`/
  `RecvTimeout` paths keep their existing parked-receiver contract. `WaitCompletion`
  is additive; it does not absorb the IPC paths that still depend on the parked
  `TaskState::Recv` shape.
- Peer-dependent completion sources stay deferred. Any future source that waits on
  another task must bind that target's generation at submission, because a bare tid
  is reusable and is not a stable completion identity.
- Every block of unsafe code justified by "the caller is blocked" must be audited
  before the executor changes, not after. That justification stops being true the
  moment a future can be cancelled.
- The stack sizing work stays blocked until this lands, because a shim pins a
  future on the caller's stack and changes the watermark for every call in every
  cell.
- The 2026-07-31 buffer-pinning audit is addressed independently of queue
  migration: `ipc_send`, `ipc_post_nonblock`, and `ipc_try_send` now place owned
  bytes in the receiver's existing `pending_msgs` mailbox, and only the resumed
  receiver copies them into its buffer. This also removes the interrupt-side need
  to enable supervisor writes to user pages for console delivery.
- The first end-to-end TIMER userspace proof is still Phase 05. Phase 04 verifies
  the encoded contract, source validation, and dead-task lifecycle guard, but it
  does not claim a real TIMER consumer in userspace.

## Rejected

**Task ids as completion identifiers.** Reused, unstable across restart, and unable
to distinguish two operations belonging to one task.

**Dropping completions when the queue is full.** Converts a full queue into a
hang, at the point furthest from the cause.

**Blocking the driver when the queue is full.** Converts a full queue into a
deadlock, and puts the backpressure where nothing can respond to it.

**Cancellation that abandons an operation immediately.** Cannot be made safe while
anything else may still be writing to the buffer, and the pinning registry already
rejected the equivalent for frames.

**Waking inline while appending.** Requires the scheduler lock in interrupt
context, against the one ordering rule this area already has.
