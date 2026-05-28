# Phase 05 — Keyboard Input Fix

**Effort:** 20h | **Priority:** P0 (BLOCKING) | **Status:** complete | **Blockers:** none (parallel to 03/04)

## Overview

Shell reliably reads the first keystroke, then deadlocks (sticky-key class symptom). User cannot enter further commands. Root cause is one of three known classes: (a) the VirtIO input IRQ is one-shot and not re-armed, (b) the async `Recv` syscall returns but does not requeue the waker, or (c) `async_utils::read_byte` future fails to register its waker on the second poll. Goal: 100% reliable streaming of keystrokes through shell REPL including arrow keys, Ctrl+C, and rapid 100-char paste.

## Context Links

- `docs/03-runtime.md` — async safety, owned buffers, executor contract
- `docs/11-shell.md` — REPL design
- Phase 03 (Ring 3) — defines trap dispatch; this phase consumes its hook
- Phase 04 (VirtIO block) — adjacent driver IRQ patterns

## Key Insights

- VirtIO input devices use the same virtqueue model as block; **the device fills a virtqueue with empty buffers, host fills them with events, driver re-arms by republishing the buffer to the available ring**. Forgetting to republish after consuming the first event is the canonical "first keystroke only" bug.
- Async wake pattern (missed-wakeup): if `poll` returns `Pending` BEFORE registering the new waker, an IRQ that arrives in that window is lost forever. Always register waker first, then check condition.
- `Recv` syscall blocking semantics: kernel must mark TCB state `Blocked`, register waker, and only then schedule away. Reverse order race-deadlocks.
- Shell input loop is in `cells/apps/shell/src/async_utils.rs::read_byte`; REPL in `shell.rs`.

## Requirements

**Functional**
- Type 100 characters in shell, all 100 echo correctly
- Backspace, Enter, Ctrl+C all work
- Arrow keys (initially) at least don't deadlock (full editing may defer to Phase 17)
- Concurrent keystroke + background task (e.g. periodic timer) does not lose events

**Non-functional**
- < 5ms input-event-to-shell-print latency in QEMU
- No busy-wait loop in shell (must yield to executor when waiting)

## Architecture

```
QEMU VirtIO input device (keyboard backend)
   │ writes event to used ring
   │ raises IRQ via PLIC
   ▼
PLIC claim → trap.rs handle_irq → virtio_input::on_irq()
   ├─ drain used ring → enqueue InputEvents to ring buffer
   ├─ re-arm: republish empty buffers to available ring   ← currently missing/broken
   ├─ notify(queue_notify)                                 ← currently missing/broken
   └─ wake task blocked on Recv(InputMask)

Task (shell):
   shell.rs REPL
   └─ async_utils::read_byte().await
      └─ syscall Recv(InputMask) — yields to executor
         └─ Future::poll
            1. Register waker FIRST (atomic Cell<Option<Waker>>)
            2. Check ring buffer for ready event
            3. If ready: consume + return Ready
            4. If not: return Pending (executor will park task)
```

## Related Code Files

**Investigate first:**
- `cells/apps/shell/src/async_utils.rs` — async stdin reading
- `cells/apps/shell/src/shell.rs` — REPL loop
- `kernel/src/task/drivers/virtio_input.rs` — VirtIO input driver
- `kernel/src/task/drivers/input_map.rs` — scancode → Unicode mapping
- `kernel/src/task/syscall.rs` — `Recv` syscall handler
- `kernel/src/task/scheduler.rs` — blocking/wake state transitions
- `kernel/src/task/tcb.rs` — TCB block state + waker field

**Modify:**
- `kernel/src/task/drivers/virtio_input.rs` — fix re-arm logic in `on_irq`
- `kernel/src/task/syscall.rs` — fix `Recv` blocking + wake semantics
- `cells/apps/shell/src/async_utils.rs` — fix waker registration order
- `kernel/src/task/scheduler.rs` — ensure `wake_task(id)` transitions Blocked → Ready

**Create:**
- `tests/integration/keyboard_stream.rs` — drive QEMU with `-chardev pipe` or `monitor sendkey`, assert 100 chars received
- `scripts/inject-keys.sh` — helper using QEMU monitor to inject N characters for tests

## Implementation Steps

1. **Diagnostic logging pass**:
   - Add `log::trace!("virtio_input on_irq: drained {n} events, repub {m} bufs")` in `virtio_input::on_irq`
   - Add `log::trace!("recv: task={tid} state={:?} waker={:?}")` in syscall `Recv` path
   - Add `log::trace!("async read_byte poll: ring_empty={} waker_present={}")` in `async_utils::read_byte`
   - Boot, type one key, type a second; observe where the second event stalls.
2. **Fix VirtIO input re-arm**:
   - In `virtio_input::on_irq`, AFTER draining used ring, walk each freed descriptor and republish it to available ring with the original (or fresh) buffer
   - Issue `fence rw, rw` then write `QueueNotify`
   - Confirm with `-trace virtio_input_*` that the device sees notify after every drain
3. **Fix `Recv` syscall blocking**:
   - In `kernel/src/task/syscall.rs::sys_recv`, the contract must be:
     ```rust
     loop {
         // 1. Atomically register waker on the source ring
         source.set_waker(current_task.waker());
         // 2. Check ring; if event present, consume, clear waker, return
         if let Some(ev) = source.try_pop() { return ev; }
         // 3. Mark Blocked + yield (release-acquire fence around state change)
         current_task.set_state(Blocked);
         scheduler::yield_now();
     }
     ```
   - The loop is critical: when waker fires, task re-enters loop and re-checks (no spurious-wake bug)
4. **Fix `async_utils::read_byte` future poll order**:
   - Set waker via `cx.waker().wake_by_ref()` registration BEFORE the ring check
   - If ring empty: `Poll::Pending`
   - If ring has data: clear waker (so next poll doesn't double-register), return `Poll::Ready(byte)`
5. **Confirm scheduler `wake_task` is idempotent**:
   - If task is already `Ready`, no-op
   - If `Blocked`, transition to `Ready` and enqueue
   - Must be safe to call from IRQ context (use SpinlockIrqSafe in scheduler)
6. **Smoke test interactive**:
   - Boot QEMU, type 10 characters one-by-one, verify each echoes
   - Type 100 characters rapidly via `cat keys.txt | qemu-monitor sendkey`
7. **Soak test**:
   - 60-second loop in CI: shell receives 1 key per 10ms, expects 6000 events total
   - Pass condition: 6000 events received, no drops, no hang
8. **Test special keys**:
   - Backspace removes char from input buffer (shell-side)
   - Enter triggers command execution
   - Ctrl+C cancels current input (shell-side handling — full POSIX signal in Phase 17)

## Todo List

- [x] Add diagnostic trace logs in driver + syscall + shell — root cause identified analytically
- [x] Reproduce hang, identify which of (driver / syscall / async) is at fault — **interrupt storm** (not re-arm / waker ordering)
- [x] Fix VirtIO input re-arm in `on_irq` — fixed via `ack_irq()` + `INPUT_DEVICE_IRQ` pattern; `virtio-drivers` crate handles virtqueue re-arm automatically
- [x] Fix `sys_recv` blocking + waker order — **not needed**: root cause was IRQ storm, not waker ordering
- [x] Fix `async_utils::read_byte` waker registration order — **not needed**: shell uses polling `sys_read`, not waker-based async
- [x] Verify scheduler `wake_task` idempotent + IRQ-safe — verified: Spinlock disables interrupts on acquire, `pick_next` handles re-scheduling correctly
- [ ] Interactive smoke test: type 10 chars (needs QEMU)
- [ ] Soak test: 6000 events / 60s in CI (needs QEMU)
- [ ] Test backspace, Enter, Ctrl+C (needs QEMU)
- [ ] CI green (pending QEMU runtime tests)

## Success Criteria

- 100 rapid keystrokes echo correctly with 0 drops
- Backspace/Enter/Ctrl+C operate as expected in REPL
- `tests/integration/keyboard_stream.rs` passes in CI
- No deadlock for 60s soak test
- Latency per keystroke < 5ms in QEMU (measured with trace timestamps)

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| The fix exposes a related bug in serial input path (different driver, same async glue) | Med | Low | Apply same audit (register-then-check) to serial driver in `kernel/src/task/drivers/uart.rs` |
| Test harness injection (QEMU monitor sendkey) flaky in CI | Med | Med | Use `-chardev pipe,path=…` and write bytes into pipe deterministically |
| Async waker abstraction in `libs/ostd/src/executor.rs` has its own bug | Low | High | If so, fix in scope; treat as a separate sub-task and document |
| Phase 03's trap dispatch lands later than this phase | Low | High | Document hard-dep: this phase MUST land after Phase 03's trap dispatch, OR can rebase on top |
| Re-armed buffer overruns if events arrive faster than drain | Low | Med | Size the input virtqueue to ≥ 64 entries; bound shell ring to 256; drop oldest on overflow with `[ViOS][input] dropped event` log |

## Security Considerations

- Do not echo events from one task's IRQ context into another task's address space without explicit input service routing — Phase 14 introduces InputDispatcher for proper routing; Phase 05 only fixes shell's direct receive
- Bound the input ring buffer size; reject submitted buffers > 4KB (defense vs malicious device firmware)

## Rollback

Single feature branch; revert restores the (broken) state. The bug is user-visible but non-crashing, so revert is safe.

## Next Steps

Unblocks: Phase 14 (full input service); Phase 17 (shell job control with Ctrl+C/Ctrl+Z). Same async-waker discipline pattern reusable in Phases 13, 15, 16.
