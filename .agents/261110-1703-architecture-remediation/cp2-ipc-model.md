# CP2 — one IPC operation model (Phase 11)

**Date:** 2026-11-12 · **CP1:** [cp1-ipc-model.md](cp1-ipc-model.md) (owner confirmation 1, 2026-11-11) · **Law 1:** confirmation 2 given 2026-11-12 · **Branch:** `p11-step7` (steps 3–7 in seven commits on `53b0d3200`)

## 1. Delta as implemented

| # | CP1 item | As implemented | Deviation |
|---|---|---|---|
| D1 | `RegisterService` `a2 = flags` | `api::syscall::service_flags { RETAINS_OUTER_CALLER, OWNER_WATCH, ALL }`. Trusted init or a `SupervisorCap` holder only (`PermissionDenied`); an undefined bit or flags on a `tid=0` self-registration is `InvalidInput`. Rows carry flags; a hot-swap commit hands them to the replacement. `OWNER_WATCH` holds while Active, `RETAINS_OUTER_CALLER` while Active or Paused. Policy is `ostd::service::namespace_flags` (init and the supervisor's rollback). | Gate is "trusted init or SupervisorCap", the code's namespace owner (CP1 wrote "SpawnCap holder"). A paused VFS starts no new owner watch (the CellId rule allowed it). |
| D2 | `WaitIrq` on completion source IRQ | `WaitIrq` parks as `WaitKind::Completion { source: IRQ, delivery: IrqLine(line) }` and shares the completion sweep rule (`completion_wait::sweep_wake`); the sweep consumes the line and frees the waiter slot exactly once. | Owner choice (2026-11-12): the wait state and wake paths are folded, not the queue. No slot reservation (no new `TryAgain`), and the legacy-line ownership/ack rules are unchanged; `source::IRQ`'s doc is not widened. |
| D3 | `WaitForEvent`, `IpcWait` over completion sources | Same fold: `Delivery::EventMask(mask)` (fired NET_RX bits returned, receive-style timeout) and `Delivery::IpcTerminal` (one IPC wake rule for every wait naming IPC; silent deadline). Results unchanged, pinned by `the_sweep_keeps_each_completion_delivery_result`. | As D2. |
| D4 | Retired opcodes reserved | `[[reserved]]` rows (number + bit, never reused; the parser refuses a live row on either): `Call` 2/4, `Reply` 3/3, `Exec` 6/35, `AudioPlay` 218/47, `NetTx` 310/27, `NetRx` 311/28. No `ViSyscall` variant; the kernel decodes them to nothing (`retired_numbers_decode_to_nothing`). Base allowlist drops `Reply`; 27 cell lists drop retired names. | **`Yield` 104 is not retired** (it is the only yield; CP1 census error). **`NetTx`/`NetRx`/`AudioPlay` had callers** (net's no-driver path, `audio-demo`); owner decision: retire, remove the net path (no frame moves without a NIC driver, as the kernel arms already did) and delete `audio-demo`. A hidden raw fallback mapped 3 to `SetTimer`; removed. |
| D5 | Retire `LookupService` 206 after migration | 206 is `[[reserved]]` without a bit: bit 37 stays with `LookupServiceBound` (they shared it), so images that declared `LookupService` still resolve bindings. ~150 call sites, the posix VFS client and 83 allowlists moved; `ostd::service::lookup` returns the binding, `provider_tid` is where to send, provider checks use `sender_is_live_provider`. | — |
| D6 | `ProcessInfo` encoding unchanged | 0/1/2/3 unchanged; every `Blocked` reports 2; 3 = Terminated or Retiring. | — |
| D7 | Spec 17 §1/§6/§9 | §1 (exact operation is request/reply, events are posts, `sys_send` raw and forbidden outside `libs/ostd`/`cells/tests`, binding lookup), §6 (no service parks on a client; bounded requests; input pacing as built), §9 checklist and amendment entry 2026-11-12, §11.4 (`fast_ipc` deleted). Spec 20/23 and the guides follow. | — |

Not Law-1 but in CP1 §3: typed `CallOutcome` (step 2), `Incoming`/`reply_to`/`post` (step 3), kernel `Rename` on an exact operation (step 3), `fast_ipc` deleted on both sides (step 4), 13 parked states → `Blocked { wait, deadline }` and `Polling`/dummy waker deleted (step 5), dead `Task` fields and internal variants deleted (step 6), every SDK and cell request bounded with `call_bytes` / `service_call*_bounded` and `clippy.toml` `disallowed-methods` on `sys_send` (step 7).

## 2. Proposed for confirmation 2

1. Confirm D1–D7 as implemented, with the deviations above.
2. **`libs/api/src/abi/fast_ipc.rs` (`TrustedHandle`, `VfsCell`)** has no in-tree user after step 4. Removing an `api` module is a Law-1 delta CP1 did not list; it is kept and proposed here for removal in a later slice.
3. `MkDir` raw 110, `Create` raw 111, `FileOp` op 1 and the raw name lookup 100 (`ServiceLookup`, always permitted) still answer fixed results; listed for a later CP, as CP1 said.

## 3. Proof obligations (CP1 §4)

| # | Obligation | Evidence |
|---|---|---|
| 1 | Non-receiving client fixture | `bench nonreceiving-client` / `x86_services_do_not_park_on_a_client_that_never_receives`: PASS on every step's tree; fails on `e4755d8b0` (before step 3). |
| 2 | `Rename` with VFS killed mid-request | Host test `a_kernel_submitted_request_terminates_when_its_provider_dies_mid_request` on the real scheduler state (a QEMU kill cannot be timed; Deviation Log step 3b). |
| 3 | Bridge ordering (CONFIGURE_ACK) | By construction: the state event is posted, only the status goes through the reply (no UI-lane witness; Deviation Log step 3a). |
| 4 | Unchanged lanes | See §4. |
| 5 | Golden dump | `syscalls.golden.json` changes only in rows 2, 3, 6, 206, 218, 310, 311 (gone) and 100 (`kernel_vi` no longer `Call`). The three Law-1 digest scripts pass unchanged (no re-pin). Headers check passes. |
| 6 | Negative controls | Retired opcodes decode to nothing (kernel `retired_numbers_decode_to_nothing`, api `unknown_id_decodes_to_unknown_variant`); flags from a non-owner / undefined bit refused (`register_service_refuses_well_known_hijack_by_a_spawn_only_cell`, `registration_flags_follow_the_row`); a `sys_send` added outside the exempt trees is reported by clippy. `post` to a full mailbox fails `Busy` (step 3 SDK). |

## 4. Evidence

- **Host tests.** Kernel 631/631 (new: `registration_flags_follow_the_row`, `the_sweep_keeps_each_completion_delivery_result`, `retired_numbers_decode_to_nothing`, `a_cell_posts_into_a_bounded_share_of_a_mailbox`); `api`, `ostd`, `service-vfs`, `service-input`, `service-net`, `service-kms`, `app-init`, `supervisor`, `app-net-tools`, `app-cellbox`, `hypha-*`, `robot-demo`, `service-net-broker --lib` green. Crates that fail on the baseline too (`driver-dwc2-usb`, `service-net-broker` without `--lib`) are unchanged.
- **Clippy.** `x86_64` (with `test-hooks`) and RV64 (with `recovery-demo/fault-inject`) workspaces report nothing in touched files; their remaining findings are pre-existing (compositor, `ostd::display::linear`, `kernel/hypervisor`, `cell_session`). The `disallowed-methods` negative control fires.
- **Per-step lanes** (each step's own tree): steps 4, 5, 6 — `local-service-lifecycle-x86` 4/4, RV64 `atomic-publication`, RV64 boot subset 12–15/15 incl. `vfs_lifetime_selftest_passes`, AArch64 `virt`/`virt-vhe`/`virt-el2-novhe` to shell.
- **Checkpoint** (`haily-tester`, independent, on `ad5f4f73b`, sequential, CI env; logs `/tmp/p11-checkpoint/`). No regression:

| Lane | Result |
|---|---|
| x86 `qemu-x86_64-boot` (ISO, `x86_64-boot` at `-smp 4`, HPET off → PM timer) | PASS |
| x86-c2c: two-node TCG + negative control, release posture, domain test, `local-service-lifecycle-x86` | PASS (image dir moved outside the repo: CI's verbatim step is refused by `assemble-x86-image.py` on the baseline too) |
| C2C node restart + negative control (KVM, script defaults) | PASS |
| `hotswap-smoke` | 13/13 |
| RV64 `gen_disk` + CI `boot` allowlist | 57/57 |
| RV64 `atomic-publication` | flake (AP-13 two-hart skip), PASS on rerun |
| RV64 `pcie-bar-window`, `srv-cellosfs` 5/5, `qemu-boot-test.sh` | PASS |
| RV64 `vfs-quota`, broker oracle | FAIL on baseline too (`shell: authenticated serial session failed: NotFound`: the image ships no input service) |
| RV64 shell-test kernel build | FAIL on baseline too (`SFN SHELL` check vs `/bin/cellbox`) |
| std-smoke riscv64/aarch64/x86_64 + sysroot overlays | PASS; promotion-validator digest pins fail on baseline too |
| AArch64 `virt`, Image on `virt`/`virt-vhe`/`virt-el2-novhe` | PASS (4 boots) |
| Law-1 digests (3), headers check, `syscall_golden` | PASS (two scripts re-pinned, §3 row 5) |

The two-node nightly lane was run through the x86-c2c two-node TCG gate; no physical or KVM two-node run.

## 5. Review

`haily-reviewer`, read-only, on `c6d519e7f..d43c9acef` (step 3 had its own review). Verdict BLOCKED on one HIGH, fixed in `ad5f4f73b`: bounded calls read the scheduler clock before submitting and 13 cells lacked `GetTime`. Also fixed: wasm `SendGather` (MED); a per-sender post quota (16 of 64 slots) so one client cannot fill a service's mailbox (MED); three LOWs. Steps 4–6 found clean (every non-test `Blocked` site, sweep rules, flag authority, lock order, retired decode paths). Dispositions in the phase Deviation Log.

## 6. Not done / residual

- nc/httpd accept loops retry after `Indeterminate`; an accept that expires after NET accepted would lose that connection (NET answers within one loop turn).
- While a client waits on a compositor call, input frames the compositor `sys_try_send`s to it are dropped (one compositor turn); they used to be routed by the old receive loop.
- The kernel accepts any defined flag on any row from a `SupervisorCap` holder; the policy table is the namespace owner's (`ostd::service::namespace_flags`). Only `/bin/supervisor` holds the cap.
- CP1 §4.3 bridge ordering has no UI-lane witness; CP1 §4.2 `Rename` kill is a host test (QEMU kill cannot be timed).
- Pre-existing, not touched: CI's x86-c2c image step refuses its in-repo `--out-dir`; the shell-test lane's `SFN SHELL` check; images without an input service cannot start a serial shell (`vfs-quota`, broker oracle); promotion-validator digest pins; `native-workload` and `actor-supervisor` (step 3 log).
- `allowed_drivers` and `pku_key` stay (copied to threads), as CP1 said.

## 7. Owner decision

2026-11-12: **CP2 confirmed** (Law-1 confirmation 2) for D1–D7 as implemented in §1, with their deviations. Not part of this confirmation: removing `libs/api/src/abi/fast_ipc.rs` (§2 item 2) stays a proposal for a later slice, and the raw fixed-result opcodes (§2 item 3) stay listed for a later CP.
