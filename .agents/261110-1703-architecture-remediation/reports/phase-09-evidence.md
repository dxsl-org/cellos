# Phase 09 evidence — kernel residents, self-tests and module layout

Date 2026-11-11 · host + QEMU TCG, local only (CI not run, nothing pushed) · range `856b9ed3b..HEAD` (S4b admission → close).

## Before / after

| Probe | Before (`616120e2d`) | After |
|---|---|---|
| `task/syscall/mod.rs` | 6 635 lines; 153 inline match arms | 2 207 lines; every arm is a handler fn in its domain file |
| Fixture symbols (`selftest`/`self_test`, crypto KATs excluded) in the x86 release kernel | 19 (`release-p07` kernel) | 0 |
| Same, RV64 release kernel (the shipped arch) | not checked | 0 |
| Release lane can fail on a fixture symbol | — | yes. Negative control: the guard rejects the Phase 07 kernel with 19 symbols |
| Snapshot engine (5.7k lines) in non-qualified kernels | compiled; gate was a runtime const | not compiled; `snapshot_disabled.rs` instead (host-tested) |
| Kernel files with a recorded Spec 15 category | 0 | 297/297 (`kernel/residents.toml`): A=81, B=27, C=41, D=28, exception=28, test=92 |
| An unlisted kernel file fails CI | no | yes: `check-kernel-residents.py` plus `--negative-control`. A real `kernel/src/dev/zz_unlisted.rs` gives rc=1 |
| Kernel nLOC (`code-metrics.generated.md`) | 90,370 (stem-match exclusion) | 66,557, measured by resident category and `#[cfg(test)]`, with no code behind the drop. Core nLOC: 78,428 → 56,526 |
| `task/` layout | `drivers/`, scheduler, IPC objects and the file API all inside `task` | `dev/`, `sched/`, `ipc/`, `fsapi.rs` |
| Raw `--cfg` switches set from the environment | `cellos_p01_trace`, `cellos_g1_cpu` | Cargo features `p01-trace`, `g1-cpu-accounting`. The old environment variables are refused |
| `bench-counters` allowed in a release build | yes | refused by `FORBIDDEN_IN_RELEASE` |

## Host checks (final tree)
- Kernel host tests pass: 618 with `compile-only` and 618 with `compile-only,test-hooks`.
- The `snapshot` filter passes with and without `snapshot-qualified`: 62 and 64 tests.
- Clippy `-D warnings` (`compile-only`) is clean on riscv64, aarch64 and x86_64.
- `cargo check` passes with `test-hooks` on all three architectures, and with `bench-counters` on x86.
- The feature matrix compiles:
  - `getrandom-sas-test` (RV64);
  - `snapshot-qualified` (x86; clippy on all three arches);
  - `p01-trace` and `g1-cpu-accounting` (x86, with a dummy 96-byte manifest);
  - release posture on x86 and RV64, with no warning.
- Guards pass:
  - `mutable-globals --check` (0 problems), `cellos-sign --check --strict` (F1 and F5), `check-kernel-residents` and its negative control, `generate-code-metrics --check`;
  - `check-{plan-status,lanes,host-test-crates,catalog,disk-layout-literals,spec-anchors,cargo-config-parity}`, `gen-syscall-headers --check`, `check-hal-boundaries`;
  - the three law-1 digest scripts (one path-only re-pin; see the phase log);
  - `cargo fmt --check`.
- Integration tests that read kernel sources pass: `iommu-publication` 4/4 and `iommu-riscv-command` 5/5.
- `tests/rust-std-promotion` fails 2/33. The same 2 fail on `8a005f998` (frozen artifact digests).

## QEMU lanes

The full set (`/tmp/s1-lanes/p09c`) ran on the tree before the review fixes:

| Lane | Result |
|---|---|
| x86 boot, `x86_64-boot`, ahci, igb, nic, vmxnet3, virtio, pvscsi, serial, power | PASS |
| `xhci-x86` | 4/7. The same 3 hub tests fail on `8a005f998` |
| two-node C2C (TCG) | PASS |
| x86 Tier-2 domain test, 1 CPU and 2 CPUs | PASS |
| `local-service-lifecycle-x86` | 2/3: `x86_multi_source_wait_holds_on_two_cpus` timed out. Known intermittent; see the final-tree row |
| RV64 boot, AArch64 boot (13 self-test kinds per arch) | PASS |
| `qemu-phase09-ssh-test.sh` (incl. `boot-ceiling-selftest-pass` on the dev image) | PASS |
| release posture (fixed symbol guard) | PASS: no fixture symbol, fails closed at VFS bootstrap |
| ABI refusal witness (fresh ISO) | PASS |

Final tree, after the review fixes (`/tmp/s1-lanes/p09d`):

| Lane | Result |
|---|---|
| x86, RV64 and AArch64 dev boots | PASS. 26 `PASS` self-test markers per arch, 0 `FAIL`; the fixtures still run on dev images |
| release posture | PASS: `ok: no self-test symbol`, fails closed at VFS bootstrap |
| `local-service-lifecycle-x86` (rerun) | 3/3 PASS |

## Independent review
`haily-reviewer` ran once, read-only. Verdict: PASS_WITH_RISK.
- It confirmed with a script that all 153 dispatcher arms equal their handlers.
- It confirmed that the NetTx/NetRx inlining, the snapshot gate and the feature conversion keep behaviour.
- One HIGH (the release symbol guard could never fail), two MEDs (fixtures compiled but not linked; snapshot closed gate duplicated and untested) and the LOWs are fixed in `92093daad`, `695e978eb`, `3deac54ab` and `1ba06bfaf`. The disposition of each finding is in the phase Deviation Log.

## Residual / for the owner
- `run_ipi_selftest` (AArch64 IPI health probe) is gated off release kernels with the other boot code. It only logs and enforces nothing. If the owner wants it as a release health check, it moves to the crypto-KAT carve-out.
- `p01-trace` sites are not confined to one module (~400 sites in 40 files). Confining them needs a tracing facade; this is open.
- `Task` keeps about 55 flat fields. Three cohesive groups were extracted.
- `async_ipc` stays in `task`, because moving it would widen 15 private task items.
- Self-test files stay at their current paths. The `test` category in `kernel/residents.toml` marks them, so a `selftest/` directory move adds nothing.
- Two manual, uncompiled source-text integration tests were deleted. They broke when the Phase 08 split removed `task/syscall.rs`.
