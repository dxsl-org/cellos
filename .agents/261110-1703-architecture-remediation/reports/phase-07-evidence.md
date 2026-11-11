# Phase 07 evidence — one rights record per Cell incarnation

Date 2026-11-11 · host + QEMU TCG, local only (CI not run, nothing pushed) · base `7e7519313`, commits `d54abcc22`, `246acddad`, `13a5208c0`, and the review-fix commit that carries this file.

## H-1 before / after

| Probe | Before (`7e7519313`) | After |
|---|---|---|
| `cap_revoke_on_a_worker_removes_the_right_from_every_thread_of_the_cell` (host) | FAIL: `tid 202 kept SpawnCap` (the root kept the right) | PASS |
| `cap_revoke_pending_mask_holds_and_survives_target_exit` (host; review finding 1/2) | n/a | PASS |
| Boot selftest `REVOKE-CELL-WIDE` (spawned thread revoked, root checked) | n/a | PASS on x86, RV64, AArch64 |
| Per-TCB cap field readers | 10 token fields + 12 right fields, 23 copy sites | 0 (grep) |

## Host checks
- Kernel host tests: 611 pass with `compile-only` and with `compile-only,test-hooks`, three runs each. One test-hooks run failed in `scheduler::retirement_tests::faulted_task_to_boot_allows_successor_selection`; that is the same pre-existing flake seen on base `d14b69266` (1 in 10 runs) and is unrelated to this change.
- Clippy `-D warnings` (`compile-only`) is clean on riscv64, aarch64 and x86_64, and `cargo check` passes with `test-hooks` on all three.
- `scripts/x86/test_abi_refusal_fixtures.py`: 12/12 pass.
- `mutable-globals --check`: 0 problems; the `CELL_REGISTRY` row is removed.
- `cellos-sign --check --strict`: F1 and F5 OK.

## QEMU lanes (final tree, `/tmp/s1-lanes/p07e`)

| Lane | Result |
|---|---|
| x86 boot to shell, `x86_64-boot` 10/10 | PASS |
| ahci 15/15, igb, nic, vmxnet3, virtio, pvscsi, serial, power | PASS |
| `xhci-x86` | 4/7. The same 3 hub tests also fail on the original HEAD `8a005f998` |
| two-node C2C (TCG) | PASS |
| x86 Tier-2 domain test, 1 CPU and **2 CPUs** | PASS |
| `local-service-lifecycle-x86` | PASS (3/3) |
| RV64 boot, AArch64 boot | PASS |
| `qemu-phase09-ssh-test.sh --no-faults` | PASS |
| release posture | PASS (fails closed at VFS bootstrap) |
| ABI refusal witness (`run-abi-refusal-witness.py`, fresh ISO) | PASS: 7 format refusals read from the new subject layout, 1 signature refusal, 3 admits |
| Boot selftests (13 kinds) on x86 / RV64 / AArch64 | all PASS |

Two intermediate failures were found and fixed in this phase, both caused by the new subject audit layout:
- The domain lane's `ATOMIC_PUBLICATION_AP-00` assumed every audit record is 18 bytes.
- The first fix for that also mis-sized AP-05/06/09/10/11.

## Independent review
`haily-reviewer` ran twice, read-only.
- **First pass:** PASS_WITH_RISK with 8 findings, among them a High: a revoke was lost when the target thread exited mid-teardown. All 8 are fixed; see the phase Deviation Log.
- **Second pass:** confirmed findings 1–8 fixed and that the SCHEDULER/drop ordering is sound. It raised three Lows:
  - The pending-bit set lacked the incarnation guard. Fixed.
  - `sys_cap_revoke` reports every refusal as one error, because the kernel's generic error sentinel is ABI. The docs were corrected to call a concurrent or failed-teardown refusal transient and retry-safe. Distinguishing them would need a Law-1 return-code change, which is not done here.
  - While a revoke is pending, a ceiling snapshot from `cell_rights` leaves out the pending bits even if the teardown later fails. This fails closed, is accepted by design, and is recorded.

## Residual
- Device, BDF and service ownership stay keyed by tid. The revoke tears down every member of the incarnation, so tid keying no longer leaves a thread holding a revoked right. Re-keying ownership itself is left for Phase 08/09 [INFERENCE: no consumer needs it yet].
- Domain-admission `CellSpawnDenied` (draining, live grant) keeps the 8-byte payload with no subject.
- The record lives on the root TCB rather than in `CellAuthority` (D2 deviation; reasons in the phase log). Gates still take `SCHEDULER`, as before; lock ranks are Phase 10.
