# Phase 08C — CP1 addendum: masked Recv completes when its peer dies

Status: direction chosen by the user 2026-10-31 ("Sửa kernel: đánh thức Recv khi task đích chết") over:
- a VFS allowlist change (`RecvTimeout`);
- a TryRecv poll;
- dropping restart.

## Finding

A `Recv` whose mask names one tid could never complete if that tid died. The
kernel delivers a death only to `NotifyOnExit` watchers (SpawnCap) and to
`Sending` tasks blocked on the dead target (`scheduler.rs`, `to_wake`). With
08C's driver restart, VFS sat in `recv(dead_driver)` forever, and every file
operation then timed out (30 s) in the shell.

## Semantic change (kernel IPC)

1. **On task exit.** Every task blocked in `Recv{mask == dead_tid}` that a
   `NotifyOnExit` delivery did not already wake is woken through the same death
   lane as watchers: `Recv` returns `Ok(dead_tid)` with the exit reason in the
   first 8 bytes of the buffer.
2. **On Recv entry.** A masked `Recv` whose target is already gone (absent,
   `Retiring` or `Terminated`) and has no queued message returns
   `Ok(mask)` at once with reason 0, the same synthetic reason `NotifyOnExit`
   uses for an already-dead target.

Receivers that use `mask == 0` are unchanged. A message the peer queued before
it died is still delivered first.

## Client obligation

A masked receiver must not treat `Ok(peer)` alone as a reply. VFS `blk_router`
counts a reply only while the peer is still the registered `BLOCK_DRIVER`,
because death clears the registration before any receiver runs.

## Evidence plan

- kernel host test for the exit-path wake decision, if it can be reached
  host-side;
- the 08C AHCI and NVMe restart witnesses (readback after restart);
- regression: RV64 test-hooks lanes, RV64 boot, the x86 candidate `ahci-x86`,
  `nvme-x86` and `x86_64-boot` lanes.
