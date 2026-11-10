# Phase 06 evidence — launch by measured identity, per-device PCIe handout

Date 2026-11-10 · host + QEMU TCG, local only (CI not run: nothing pushed) · base `d14b69266`.

## C-1 negative control

| Probe | Before (old tree, throwaway probe) | After |
|---|---|---|
| Caller named `cellbox`, no spawn right → `/bin/curl` | admitted, net ceiling | no identity → no row |
| Caller named `cellbox` → `/bin/hypervisor` | admitted, hypervisor ceiling | no identity → no row |
| Task `shutdown` from `/home/x/shutdown`, raw 502 | accepted | `PermissionDenied` |
| Signed `shutdown` bytes via caller ELF route | (n/a) | `PermissionDenied` (`launch_named=false`) |
| Signed `cellbox` file at `/bin/cellbox-ssh` | no edges (name `cellbox-ssh`) | no edges (alias, empty identity) |

Permanent tests: `task::syscall::tests::raw_shutdown_is_accepted_only_for_the_signed_shutdown_image`, `catalog::tests::launch_identity_needs_a_non_alias_install_path`, `catalog::tests::{ethernet_ids_belong_to_their_driver_program_only, class_drivers_cannot_claim_each_other}`.

## Host checks

- Kernel host tests `--features compile-only`: 609 passed, run three times in a row (review finding 3: order-dependent latch fixed with tids ≥ 100).
- `--features compile-only,test-hooks`: 609 passed. One earlier run failed in `scheduler::retirement_tests::faulted_task_to_boot_allows_successor_selection` (hart-local cell id 60 ≠ 0). It passed in the next 3 runs; the test does not touch the code changed here. Logged as a flake.
- Clippy `-D warnings`, `compile-only`: riscv64, aarch64, x86_64 clean. (`test-hooks` x86 clippy fails on `hypervisor/svm_registry.rs:643` redundant guard. That code is not touched here, and CI does not run clippy with `test-hooks`.)
- Guards pass: plan-status, lanes, host-test-crates, catalog (with the new `pcie_driver` ↔ `pci_drivers` rule; deleting `virtio-net` from the JSON makes it fail), disk-layout-literals, spec-anchors, cargo-config-parity, code metrics (regenerated), mutable-globals (0 problems), `cargo fmt --check`.
- `POLICY.BIN` (dev) is still `97e8d5a9…cdb27`.

## QEMU lanes (final tree, `/tmp/s1-lanes/p06c`)

| Lane | Result |
|---|---|
| x86 boot to shell | PASS |
| `x86_64-boot` (10) | PASS |
| `ahci-x86` (15) | PASS |
| `igb-x86`, `nic-x86`, `vmxnet3-x86` | PASS |
| `virtio-x86`, `pvscsi-x86` (pc) | PASS |
| `serial-x86`, `power-x86` | PASS |
| `xhci-x86` | 4/7: the 3 hub/second-controller tests time out. They fail the same way on the original HEAD `8a005f998` (checked), so this is pre-existing |
| two-node C2C (TCG) | PASS |
| x86 Tier-2 domain test | PASS |
| `local-service-lifecycle-x86` | first run 2/3 (`multi_source_wait_holds_on_two_cpus` timed out mid-scenario, no denial in log); rerun 3/3 PASS |
| RV64 boot | PASS |
| AArch64 boot | PASS |
| release posture (throwaway anchors) | PASS (fails closed at VFS bootstrap) |
| `qemu-phase09-ssh-test.sh --no-faults` (manual tier; exercises `/bin/cellbox-ssh`) | PASS, 34 checks incl. `no-cellbox-ssh-edge-error`, `boot-ceiling-selftest-pass` |

An intermediate tree named only Path/Pinned children. Every driver lost its identity because init starts drivers through `SpawnFromElf`. The lanes caught it: 424 was denied and the NIC, xhci and igb lanes failed. The fix is that children of trusted init are named on any route.

## Independent review

Two `haily-reviewer` passes over the S2 authority diff (Phases 04–06), read-only.

The first pass returned BLOCKED with 7 findings. Findings 1–5 are fixed and re-verified:
1. `cellbox-ssh` alias (High) — fixed with `IDENTITY_ALIASES`.
2. ELF-route identity (High) — fixed with `launch_named`.
3. Latch test flake — fixed.
4. Init latch window — fixed; the latch is published before `push_ready`.
5. PCI table drift — fixed; checker rule added.

Remaining after the second pass, recorded but not fixed:
- Low: `IDENTITY_ALIASES` is a deny-list. A future second `/bin` install of a signed binary would inherit its rows. An allow-list needs an expected program per install path, because hypha, serial and block differ from their basenames. Candidate for the catalog in Phase 07+.
- Low: `register_service_admits_machine_pin_only_from_trusted_init_itself` latches tid 1 for the whole test process. No current test is affected.
- Low: `launch_profile/tests.rs` still uses the caller name `cellbox-ssh`, which is unreachable now. The tests are harmless.
- Low (Phase 04): the release lane cannot tell an owner-admission refusal from a signature refusal. The release artifact cannot boot past init until an A/B slot is admitted; that is the owner's decision, recorded earlier.
- Low: the 418 comment was stale; fixed.

## Supervisor half (CP1 option A, 2026-11-11)

See [CP2 record](../cp2-supervisor-caller-identity.md). On a headless image assembled from this tree, these pass: `power-x86` (3/3, operator poweroff and reboot plus the power button), x86 boot, `x86_64-boot` (10/10) and two-node C2C. RV64 and AArch64 boot lanes (cells built from source) pass; the supervisor does not run in them. Kernel: 610 host tests pass. Supervisor: 12 host tests pass. ostd host tests pass. Clippy is clean on 3 architectures.
