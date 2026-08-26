# NET_RX onto the completion queue — syscall 242 `WaitCompletion`

Date: 2026-07-31 · Branch: `feat/wx-post-reloc-and-f1-signing` (not rebased, not amended)
Phase: `.agents/260727-2101-midori-lessons-cellos/phase-07-async-reactor.md` (req 4, 5, 6b)
ADR: `docs/specs/03b-async-reactor-adr.md`

---

## What was built

| File | Change |
|------|--------|
| `libs/api/src/abi/syscall.rs` | `WaitCompletion = 242`, allowlist bit 42 (shared with `WaitForEvent`), `From<usize>` row (+33) |
| `libs/api/src/abi/completion.rs` | **new** — `ViCompletion` wire record, 24 B, tagged + versioned, 6 host tests (148) |
| `libs/api/src/abi.rs` | module registration (+1) |
| `libs/api/src/abi/syscall_tests.rs` | 242 round-trip, discriminant, collision, shared-authority rows (+26) |
| `libs/ostd/src/syscall.rs` | `sys_wait_completion(mask, timeout_ticks) -> Option<ViCompletion>` (+37) |
| `kernel/src/task/waker.rs` | NET_RX reservation registry; `signal_net_rx` completes it; flag kept as backstop (+142/−30) |
| `kernel/src/task/completion_wait.rs` | **new** — the syscall handler (141) |
| `kernel/src/task/completion.rs` | `CompletionQueue::release` — withdraw a reservation without appending (+25) |
| `kernel/src/task/syscall.rs` | `Syscall::WaitCompletion`, decode, dispatch, `validate_user_buf` → `pub(super)` (+36) |
| `kernel/src/task/net_rx_selftest.rs` | **new** — boot self-test for the interrupt half (168) |
| `kernel/src/task/completion_selftest.rs` | withdrawal row; three helpers to `pub(super)` (+66) |
| `kernel/src/main.rs`, `kernel/src/task.rs` | self-test + module registration (+7) |
| `cells/services/net/src/main.rs` | migrated the wait; `WaitForEvent` → `WaitCompletion` in the declare list (+13/−5) |

`Syscall::WaitForEvent` (217) is byte-for-byte unchanged, including its handler,
its decode and its allowlist bit. No other caller was touched.

## The design

**Calling `WaitCompletion(NET_RX)` is the submission.** A level-triggered
hardware condition is not an operation anyone submits, so there is no other
context in which a slot can be reserved from the waiting cell's own stack, which
is what the ADR's reserve-at-submission rule requires. The wait reserves, arms
the source, then parks.

**One global `(queue, slot)` for the source.** One producer, one consumer; a
source-registration table would be machinery for a case that does not exist. It
is a leaf `Spinlock` in `waker.rs`, never held across `complete()`, so the append
path's lock set is still exactly `{queue.ring}`. The interrupt path clones the
`Arc` under the guard and never drops the last reference — the registry holds its
own — so it still reaches no allocator.

**Park state is `WaitEvent { mask: 0, deadline }`.** No new state, so `exit_task`
and `ipc_try_send` see what they see today. Mask 0 stops the sweep consuming a
fired bit on this waiter's behalf; the deadline is the sweep's one remaining job.

## Verification

Static, all clean:

```
cargo check  -p vicell-kernel --target riscv64gc-unknown-none-elf  -Z build-std   OK
cargo check  -p vicell-kernel --target x86_64-unknown-none         -Z build-std   OK
cargo build  -p vicell-kernel --target aarch64-unknown-none-softfloat -Z build-std OK
cargo clippy -p vicell-kernel --target riscv64gc-unknown-none-elf -- -D warnings  OK
cargo check/clippy -p service-net --target riscv64gc … -- -D warnings             OK
CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu cargo test -p api        68 + 2 pass
cargo fmt --all --check    only cells/tests/bench/src/scenarios/vfs_getfile_breakdown.rs:62
                           — pre-existing, not in this change's file set
```

The `cargo check` result was proved non-vacuous by injecting a type error and
confirming it was reported.

Runtime, on the image built by `pwsh -NoProfile -File ./gen_disk.ps1`:

```
scripts/qemu-boot-test.sh …/release/vicell-kernel      PASS: shell prompt reached
  serial: [selftest] COMPLETION-QUEUE: PASS
  serial: [selftest] NET-RX-RESERVATION: PASS (fills, remembers, releases)
integration --test boot          54 passed / 0 failed  (340 s)   = baseline
integration --test hotswap-smoke 11 passed / 0 failed
integration --test http-smoke     1 passed / 0 failed  (14 s)
integration --test nic-riscv     SKIPPED — this QEMU has no riscv-iommu-pci
                                 device. Reported as a skip, not a pass. It is
                                 an IOMMU probe test and drives no RX traffic.
```

**The suite that proved a real RX frame reached the net cell is `http-smoke`.**
It boots with SLIRP, the guest acquires a DHCP lease, and the `http-smoke` cell
performs an HTTP request/response against a host mock at 10.0.2.2 — every reply
frame arrives through the net cell's loop, whose only blocking point is now
`sys_wait_completion`. `boot`'s ten network rows (DHCP, TCP send/recv,
listen/accept, curl, wget, httpd ×2, mqtt ×2, posix-shim-net) traverse the same
loop.

That claim was **mutation-checked, not assumed**: parking with `deadline: None`
made the net cell stall after its first wait — DHCP never completed, the
heartbeat killed and restarted the cell, and `http-smoke` failed in 104 s where
it passes in 14 s. The suite genuinely drives the new syscall, and the deadline
is genuinely preserved.

## A defect the boot suite caught, and the fix

The first cut released an unfilled slot by completing it with `RESULT_ABANDONED`
and draining the result. Completing raises a wake request, and a request raised
by the submitter — which is running, not parked — is still outstanding when that
same task parks microseconds later: `deliver_pending_wakes` runs inside the very
`yield_cpu` that parks it, so the park was cancelled the instant it began. The
net cell never slept again. Ten of 54 boot rows failed, all networking.

`CompletionQueue::release` now returns a `Reserved` slot to `Free` without
appending, and refuses a slot that already holds a result so a real completion
can never be discarded by a withdrawal. `network_dhcp_acquires_ip` goes from a
40 s timeout to passing in 1.4 s. A self-test row pins the property.

## Requirement 6b

**Does not apply to this source, and was not implemented.** It asks that a waiter
register against the tid it depends on so `exit_task` can post a synthetic
completion on that tid's exit. NET_RX depends on hardware, not on another task;
no tid's exit can strand this waiter. `exit_task` is untouched. The requirement
stays open for the IPC sources it was written for.

## Standing concerns

1. **Nothing in the tree calls `signal_net_rx`, and nothing ever did.** One
   mention outside `waker.rs`, and it is a doc comment (`tcb.rs:82`). The NIC
   lives in the virtio-net Driver Cell and owns its IRQ through the separate
   `irq_wait` path, whose entry point (`device.rs:wait_recv`) is itself
   `#[allow(dead_code)]` and unused by the polling main loop. So `NET_RX_PENDING`
   has never been set and `sys_wait_for_event(NET_RX, 10)` was a 100 ms timed
   park. This migration reproduces that exactly and makes the interrupt half
   reachable and tested; it does not switch on an RX fast path, because there
   was none to switch on. Wiring a producer means routing the NIC slot in
   `vi_handle_virtio_irq`, outside the approved scope.
2. **A completion drained by the submitter before it parks leaves a wake request
   set**, costing that submitter one immediate return from its next wait.
   Reachable only when the source fills the slot between arming and parking.
   Self-limiting and never loses a result, so `drain` was left alone rather than
   given a clearing rule inside the append lock protocol.
3. **A cell with two threads waiting at once** may have one thread drain the
   other's completion, leaving its own reservation armed until its next call
   displaces it. Self-correcting, never corrupting, unreachable with one caller.
4. The takeover path logs `[net-rx] cell N took over the reservation held by cell
   M` at `warn`. The boot self-test exercises it, so that line appears once per
   boot, immediately before the self-test's PASS line.

## Follow-ups

- Wire a producer for `signal_net_rx` (route the NIC slot in
  `vi_handle_virtio_irq`, or have the virtio-net Driver Cell report RX) — needs
  its own scope decision, and only then does NET_RX become an interrupt-driven
  wake rather than a timed poll.
- Requirement 6b for the IPC sources, where a dependency tid exists.
- `libs/ostd/src/executor.rs` still busy-polls with a dummy waker; untouched here.
