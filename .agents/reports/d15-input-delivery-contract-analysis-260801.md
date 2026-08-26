# D15 — Input delivery: direct call or bounded IPC queue?

**Status:** ruled/applied 2026-08-01. Docs and code comments updated; no runtime or ABI changed.

**Question:** does Spec 06 §2's direct `on_event`/no-queue design yield to ratified Spec 17
§6 and the implemented bounded input queue?

## Answer first

**Yes. Withdraw Spec 06 §2's direct-call/no-queue mechanism in favour of Spec 17 §6.**
There is no cross-cell `on_event(event)` call in the input path. The shipped path is
focus-based, kernel-mediated IPC with a bounded owned mailbox. An app-level runtime may
turn the received frame into `AppEvent::Input`, but that callback boundary occurs only
after IPC delivery; it is not a direct call between cells.

Spec ownership should be split rather than duplicated:

- Spec 06 owns focus/window routing and the product latency objective.
- Spec 17 owns IPC framing, blocking discipline, queue bounds, drop/backpressure policy,
  and sender/mask rules.

The absolute statement “latency must be zero” is also indefensible. It should become a
measurable input-to-handler latency objective with an environment and percentile, or be
removed until measured.

## 1. Actual delivery path

The input service translates raw events and `Dispatcher::send_event` encodes
`[0x10][InputEvent]`, then calls `sys_try_send`
(`cells/services/input/src/dispatcher.rs:108-127`). The kernel implementation is
`ipc_try_send` (`kernel/src/task.rs:1449-1511`):

1. If the focused target is already in a matching `Recv`, the kernel copies the message
   into owned `pending_msgs`, marks the target Ready, and pends priority preemption.
2. If the target is momentarily not receiving and the caller is the registered input
   service, the kernel queues the event into the same mailbox.
3. The mailbox is bounded by `INPUT_EVENT_QUEUE_DEPTH = 512`
   (`kernel/src/task/tcb.rs:20-29`). A full queue returns an error and the event drops.
4. All other `sys_try_send` callers retain drop-if-not-ready behaviour.

When an ostd app drains the frame, `AppRuntime` decodes opcode `0x10` into
`AppEvent::Input` (`libs/ostd/src/app.rs:330-337`). This is a typed application event on
the receiver side, not a shared-address-space function jump.

The UART lane adds backpressure before the input service: bytes that cannot enter the
input service's bounded mailbox are retained in `PENDING_ASCII` and retried before new
UART bytes are drained (`kernel/src/task/drivers/console_drv.rs:41-76`, `:156-175`). A
runtime integration test sends 247 UART bytes/494 key events near the 512-event focus
queue limit and requires lossless completion
(`tests/integration/tests/boot.rs:1457-1487`).

## 2. Why Spec 06 is the wrong owner for queue semantics

Spec 06 §2 currently says the dispatcher directly invokes `on_event(event)` with no OS
queue (`docs/specs/06-graphics.md:18-25`). That contradicts all three relevant layers:

- the input cell uses `sys_try_send`;
- the kernel owns the bounded mailbox and wakeup;
- the app runtime decodes the queued frame after receive.

Ratified Spec 17 §6 describes this mechanism and its failure policy. Keeping a second
mechanism in the graphics spec guarantees drift. Spec 06 only needs to state that focus
selects the destination and link to Spec 17 for delivery.

The docket phrase “try-send-drop exception” is easy to misread. Input is the exception
**to dropping**: try-send remains non-blocking, but the kernel queues input events while
capacity remains. Drop occurs only when no focus exists, the target is gone, or the bound
is exhausted.

## 3. Hidden stale comments and one unresolved focus policy

The ruling should include a mechanical comment cleanup:

- `kernel/src/task.rs:1449-1454` still says a non-ready focused target's event is dropped,
  immediately above code that queues it. The later comment at `:1486-1505` is correct.
- `dispatcher.rs:13-17` says a failed send reverts keyboard focus, and `:65-68` repeats
  that contract. In reality `dispatch()` discards `send_event`'s result (`:75-81`), the
  unused `fallback_tid` stays zero, and only the mouse/compositor path resets its cached
  TID on failure (`:96-105`).

The second point is not needed to rule D15, but documentation must not promise death
reversion that the keyboard path does not implement. A later decision may choose either:

- keep focus until an explicit `SetFocus`, dropping after target death; or
- clear/revert focus when delivery proves the target stale.

## Recommended ruling [FINAL]

**Approve recommendation A:**

1. Replace Spec 06 §2's direct-call/no-queue text with focus routing plus a pointer to
   Spec 17 §6.
2. Remove “zero latency” or replace it with a measurable target; do not invent a number in
   this ruling.
3. Keep Spec 17 as the single owner of queue depth, non-blocking delivery, masking, drop,
   and UART backpressure.
4. Correct the stale `ipc_try_send` and dispatcher comments without changing runtime.
5. Record keyboard focus-on-death as a separate small policy decision; do not silently
   claim either behaviour.

### Rejected alternatives

- **Keep both texts:** creates two mutually exclusive normative mechanisms.
- **Call the receiver callback “direct”:** hides the kernel IPC/mailbox boundary and makes
  latency/security reasoning wrong.
- **Move all input policy to Spec 17:** queue semantics belong there, but focus and window
  routing still belong to graphics/input architecture.
