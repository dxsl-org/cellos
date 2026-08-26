---
name: recv-send-state-machine
description: Verified inventory of ipc_send/ipc_recv/ipc_try_recv state machine, pending_msgs mailbox, task-exit cleanup, and every TaskState::Recv/Sending call site
---

# Recv/Send State Machine — Verified Against Tree

## ipc_send — full branch structure
Three outcomes; any target state other than `Recv`/`Frozen` is treated identically (caller parks `Sending`).
- Target not found → `Err(())` — `kernel/src/task.rs:1160-1165`.
- Target `Frozen{..}` → buffered into `pending_msgs` (bounded `HOTSWAP_MSG_QUEUE_DEPTH`), caller returns `Ok(0)` without parking — `kernel/src/task.rs:1175-1209`; full → `Err(())` → `TryAgain`.
- Target `Recv{mask,..}` matching → direct copy, target `Ready`, caller returns `Ok(0)` staying `Running` — `kernel/src/task.rs:1212-1248`.
- Otherwise → caller parked `Sending{target,msg_ptr,msg_len}`, `Ok(1)` — `kernel/src/task.rs:1249-1257`.

## ipc_recv / ipc_try_recv
`ipc_recv` scans all tasks for `Sending{target==caller_id}`; on miss parks `Recv{deadline:None}`. `ipc_try_recv` is identical except the miss branch just returns `Ok(0)` — never sets `TaskState::Recv`.
- Found → copy, wake sender — `kernel/src/task.rs:1330-1367` (recv) / `1406-1441` (try_recv).
- Not found (recv) → `caller.state = Recv{mask,buf_ptr,buf_len,deadline:None}` — `task.rs:1368-1377`.
- Not found (try_recv) → `Ok(0)`, no state change — `task.rs:1442-1444`.

## RecvTimeout / RecvScatter / SendGather / TrySend
All thin wrappers over `ipc_send`/`ipc_recv`/`ipc_try_recv`/`ipc_try_send`, but the syscall-layer handlers for `Recv`/`RecvTimeout`/`TryRecv` each independently drain `pending_msgs` first, and `RecvTimeout` back-patches `deadline` into `TaskState::Recv` after parking.
- `SendGather` → one `ipc_send` after iovec concat — `syscall.rs:1416-1469`. `RecvScatter` → one `ipc_recv` then scatter — `syscall.rs:1470-1525`.
- `RecvTimeout` → drain `pending_msgs` (`syscall.rs:1537-1566`) → `ipc_recv` → on park, patch deadline into `Recv{deadline: ref mut d,..}` (`syscall.rs:1573-1583`) → yield.
- `TryRecv` → drain `pending_msgs` (`syscall.rs:1610-1636`) → `ipc_try_recv`.
- `Recv` also checks `pending_deaths` (NotifyOnExit) and a hotswap-drain fallback before the `pending_msgs` drain (`syscall.rs:1290-1373`).

## pending_msgs is a second, independent delivery mechanism
Not sugar over Recv — a separate bounded mailbox with 3 producers and 4 drain sites, all bypassing `TaskState::Recv`/`Sending` entirely.
- Depths: `HOTSWAP_MSG_QUEUE_DEPTH=64` (`tcb.rs:17`), `INPUT_EVENT_QUEUE_DEPTH=512` (`tcb.rs:28`, raised after a real burst-truncation bug). **Open question — verify during implementation:** whether these are two distinct queue fields or one `pending_msgs` Vec bounds-checked against different constants per producer.
- Producers: `ipc_send`→Frozen target (`task.rs:1195-1199`), `ipc_post_nonblock`→busy target (`task.rs:1308-1317`), `ipc_try_send`→input cell only (`task.rs:1503-1521`).
- Drains: `Recv`/`RecvTimeout`/`TryRecv` syscall handlers (`syscall.rs:1334-1369/1537-1566/1610-1636`), plus hot-swap Step 5 replay (`hotswap.rs:467-518`, guarded on new cell already being in `Recv`, `hotswap.rs:502-518`).

## Every TaskState::Recv / TaskState::Sending call site (15 non-test hits)
Fully contained to `kernel/src/task.rs`, `kernel/src/task/scheduler.rs`, `kernel/src/cell/hotswap.rs` (plus comments in `syscall.rs`). No hits elsewhere in `kernel/src`.
- `task.rs:1215` (send direct-deliver match), `1251` (send park), `1285` (`ipc_post_nonblock` match — **no mask check**, latent bug), `1331`/`1407` (recv/try_recv scan for Sending), `1370` (recv park), `1467` (`ipc_try_send` match, mask-honoring).
- `scheduler.rs:517` (exit: wake Sending-target), `551` (exit: NotifyOnExit watcher wake), `653` (deadline sweep), `742` (heartbeat diagnostic log only, not a transition).
- `hotswap.rs:507` (Step-5 drain guard); `488` comment only.
- `syscall.rs:1575` (RecvTimeout deadline patch — live code, not just a comment).

## Shell/keyboard input path — non-blocking-send call sites
The ADR's claim maps to two live kernel sites plus a third device sharing the identical hazard, not named in the ADR.
- UART relay: `console_drv.rs::relay_ascii_to_input` (`kernel/src/task/drivers/console_drv.rs:178-222`) → `ipc_post_nonblock` (`task.rs:1274-1320`), checks `Recv` at `task.rs:1284-1289` (no mask), falls back to `pending_msgs` (depth 64... or 512, see open question above).
- `Syscall::TrySend` (`syscall.rs:1259-1270`) → `ipc_try_send` (`task.rs:1455-1527`), the input-service dispatcher path; checks `Recv` at `task.rs:1467-1474`, input-only fallback to `pending_msgs`.
- GPIO IRQ notify (`kernel/src/task/drivers/gpio_irq.rs:38-48`) — same fire-and-forget-on-Recv hazard, not mentioned by the ADR, worth including in migration scope.

## Task exit cleanup — Scheduler::exit_task, three internal blocks
All exit-triggered unblocking is in `Scheduler::exit_task` (`kernel/src/task/scheduler.rs:451-569`), reached from 8 call sites. None match a completion-queue park state.
- Sending-wake (`517-530`, no multi-hop support), current_caller clear (`524-526`), join-wake (`532-541`), NotifyOnExit watcher wake (`543-568`).
- Call sites: `task.rs:373` (fault), `task.rs:873`, `scheduler.rs:767` (heartbeat kill), `scheduler.rs:903` (watchdog kill), `syscall.rs:1847` (Exit), `syscall.rs:1944` (ForceExit), `syscall.rs:2608`, `hotswap.rs:189` (hot-swap retirement).

## Gap not named in the original brief: reply-wait Recv isn't covered by exit_task unless NotifyOnExit was called
After `ipc_send` delivers directly, the sender is NOT parked — it must call `ipc_recv(mask=target)` itself to await a reply, landing in `Recv{mask:target}` (`task.rs:1354-1358`). `exit_task`'s only Recv-cleanup fires solely for `DEATH_SUBSCRIBERS` entries (explicit `NotifyOnExit`, `syscall.rs:2079`) — a plain reply-waiter isn't one. If the target dies before replying, that caller hangs forever unless it separately subscribed. **Pre-existing hole, orthogonal to this migration — document, do not fix in this change.**

## RecvTimeout deadline sweep
`scheduler.rs:653-658`, inside the single global sweep (hart 0 only):
```rust
TaskState::Recv { deadline: Some(d), .. } if now as u64 >= *d => {
    should_wake = true; timed_out = true;
}
```
On timeout: `regs[10]=0`, `current_caller=None` (`scheduler.rs:685-695`). Never touches `pending_msgs` — a message arriving between timeout-fire and resume is drained on the task's next Recv/RecvTimeout/TryRecv call, not lost, just delayed.

## Precedent: how NET_RX actually migrated
NET_RX reused the existing `TaskState::WaitEvent{mask,deadline}` as its park state rather than inventing a new variant; the completion queue supplies only the result. `is_parked()` (`completion.rs:340-345`) excludes only `Ready|Running|Terminated|Frozen` — generically wakes any other park state, so layering a slot onto `Recv` works without touching this gate.

## Additional flag: ipc_post_nonblock ignores mask entirely
`ipc_post_nonblock`'s direct-delivery match (`task.rs:1284-1289`) doesn't destructure `mask` at all — delivers to any Recv-parked target regardless of requested sender mask, unlike `ipc_send`/`ipc_try_send` which both guard `mask==0 || mask==caller_id`. Currently latent (only wildcard-Recv targets call it today). **Flag for a separate follow-up commit, not bundled into this migration.**

## Limitations
Covers `kernel/src/task*` and `kernel/src/cell/hotswap.rs` exhaustively; does not cover userspace service code (`cells/services/input`, shell). Two `exit_task` call sites (`task.rs:873`, `syscall.rs:2608`) located but not individually read line-by-line — verify before finalizing implementation if the fault/kill taxonomy matters.
