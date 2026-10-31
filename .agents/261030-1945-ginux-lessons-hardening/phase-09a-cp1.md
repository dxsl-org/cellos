# Phase 09A CP1 — stash ownership and destination-bound handoff (Law-1 semantic change)

Status: CP1 approved by the user 2026-10-31 ("Duyệt CP1"); **CP2 approved by the user 2026-10-31** ("Duyệt CP2 + commit") on the evidence in §7–§8. Complete.

No ABI bytes change: syscall numbers 410/411/412, argument
registers, allowlist bits 33/34 and the `ostd::hotswap` / `ostd::syscall` signatures stay.
Return-value **meaning** changes as listed in §3, so this is a Law-1 semantic delta.

## 1. Defect (observed)
`StateStash/StateRestore/StateStashClear` with a non-argv key read, overwrite or delete a
**global** key→bytes table with no holder check (`kernel/src/task/syscall.rs:7490-7546`,
`kernel/src/cell/state_stash.rs:103-147`). Any cell holding bits 33/34 that guesses a key
(`hotswap_key` is `0x00A3<<48 | swap_id`, a counter) can read or replace another Cell's
migration state or delete it. A second table (`VERSIONED_STASH`) has no syscall consumer.

## 2. Design
Each entry stores `owner = (CellId, generation)` of its first writer, a state
`Open | Sealed { destination: (CellId, generation) }`, and its bytes.

| Operation | Allowed caller | Effect |
|---|---|---|
| Stash (new key) | any holder of bit 33 | creates `Open`, owner = caller; per-owner cap 8 entries |
| Stash (existing key) | owner, entry `Open` | overwrite |
| Seal | kernel, inside `SpawnReplacement(old, path)` | every `Open` entry owned by the old incarnation becomes `Sealed{destination = new incarnation}` |
| Restore, `buf_len > 0` | owner while `Open`; destination while `Sealed` | copy bytes (repeatable until Clear) |
| Restore, `buf_len == 0` (status) | owner, destination, or `caller_has_supervisor` | return stored length; no bytes copied |
| Clear | owner (`Open`), destination (`Sealed` = consume), or supervisor (abort/rollback) | remove |
| Owner retired with `Open` entries | kernel retirement path | entries removed (no orphan slots) |

Everything else (foreign read/write/clear, guessed keys, a later incarnation with the
same CellId, a stale destination) is refused: Stash and Restore return 0, Clear returns
`PermissionDenied`. The argv key (`SPAWN_ARGV_KEY`) path is unchanged. `VERSIONED_STASH`
and its helpers are deleted (one implementation, no competing truth).

Business state authority (revision, schema, writer epoch) is **not** here; that is 09B at
the provider's state authority. This slice only authorizes byte transport.

## 3. Consumer-visible changes
- `sys_state_restore(key, &mut [])` now returns the stored length to authorized callers
  (was always 0). Supervisor `wait_for_stash_key` switches to this empty-buffer status
  probe, so it never reads payload.
- Foreign callers that used to read/overwrite/delete now get 0 / refusal.
- A replacement may restore only after `SpawnReplacement` sealed the entry to it; a cell
  restarted by other means (not `SpawnReplacement`) no longer inherits its old state
  through a guessed key.

## 4. Files
`kernel/src/cell/state_stash.rs`, `kernel/src/task/syscall.rs` (arms 410-412 and the
`SpawnReplacement` seal call), the kernel Cell retirement funnel (one call),
`cells/services/supervisor/src/hotswap.rs` (status probe), `libs/ostd/src/hotswap.rs`
and `libs/ostd/src/syscall.rs` (docs only), `tests/unit_cspace_smoke.rs` (drop versioned
mirror), host tests, docs (`docs/hotswap-guide.md`), CHANGELOG.

## 5. Evidence plan
Kernel host tests: foreign read/write/clear, guessed key, later-incarnation refusal,
seal-to-destination, destination restore then clear, supervisor status without bytes,
per-owner cap, retirement cleanup, argv unaffected. Runtime: existing hotswap lane
(`hotswap-smoke` / supervisor hotswap) still passes, plus a negative cell reading a
foreign key gets 0.

## 6. Rollback
Revert the commit and cold boot. Stash contents are volatile; no persistent format.

## 7. Result (2026-10-31)

- Kernel host tests `cell::state_stash` 7/7: owner round trip/overwrite; guessed key refused for read/status/write/clear; later incarnation refused; seal → only destination reads, old writer fenced, clear consumes; supervisor sees length/aborts, never reads; per-owner cap and dead-owner reclaim; argv unaffected.
- Boot self-test on RV64 and x86: `state-stash: round-trip OK, foreign read refused`.
- x86 kernel builds clean with `-D warnings` (default, `test-hooks`); RV64 image (incl. supervisor) builds.
- `VERSIONED_STASH` and its mirror tests deleted.
- **Runtime hotswap witness blocked (not by 09A):** `hotswap-smoke` on RV64 had two image drifts, fixed: `gen-disk-ci.sh` now defaults `CELLOS_INIT_FEATURES=input`, and a stale persisted autoload on the old `disk_v3.img` (kept as `disk_v3.img.bak-20261031`) masked the regenerated config. The remaining 5 failures are a Cellbox product limit: the SDK serial session refuses background external launches (`cellbox/src/executor.rs:3704`), and the test needs `hotswap-demo-v1 &`. `peer_death_guardrail_is_bounded` passes.

## 8. Runtime evidence (2026-10-31, RV64 QEMU)

User decision: long-running providers are services, not shell background jobs; shell `&` stays refused. `hotswap-smoke` rewritten accordingly: init starts `hotswap-demo-v1` and the supervisor (packaging feature `hotswap-demo`); `bench hotswap-prime [cached]` → `hotswap hotswap-demo /bin/hotswap-demo-v2` → `bench hotswap-verify N`, all foreground. Image fixes: `/bin/hotswap` embedded in the boot ramdisk (the foreground launcher cannot read the cell table copy). The standalone v1/v2 banner tests are replaced by the service boot and the swap itself.

Command: `CELLOS_INIT_FEATURES=input,hotswap-demo bash scripts/gen-disk-ci.sh && cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test hotswap-smoke` → **13/13 pass**: CLI swap preserves counter 5 (v2 restored from the destination-sealed stash, supervisor status probe only), supervisor swap with cached-sender drains to 6, unauthorized sender denied, peer-death guard.

Found, not fixed here: `app_entry!` services arm a 500-tick (~5 s) heartbeat but idle in `sys_recv` without timeout, so an idle service is killed as "hung" (observed on demo-v1 after a failed CLI launch). The swap runs inside that window.
