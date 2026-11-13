# CP2 — init reduction, hot-swap, `api` split (Phase 13)

**Date:** 2026-11-13 · **CP1:** [cp1-api-split.md](cp1-api-split.md) (owner confirmation 1, 2026-11-13) · **Law 1:** confirmation 2 requested · **Branch:** `p13-api-split` (rebased on `9141b2724`)

## 1. Delta as implemented

| # | CP1 item | As implemented | Deviation |
|---|---|---|---|
| A1 | Crate split | `libs/abi` (ABI at crate root, `build.rs`, `abi/syscalls.toml`, table, golden; deps `types` + `serde` derive), `libs/wire` (service protocols; feature `hostile-backend-recovery`), `libs/posix-shim` (feature `mlibc`), facade for one step, then every import migrated and `libs/api` deleted (step 6). | `ostd` re-exports only `ViError`, `ViResult` and a hidden `declare_manifest` path for `app_entry!`; the shim links only into the 12 C/mlibc cells that depend on it. |
| A2 | Kernel-facing contracts in `abi` | `abi::{fs, block, app_envelope, kernel_ipc, supervisor_power, input_producer}`; `wire` re-exports them at their old paths. | Module names differ from the wire paths (a root `ipc`/`proto` would collide in the facade glob). |
| A3 | Kernel `Rename` | `kernel/src/task/syscall/rename_wire.rs` hand-codes `VfsRequest::Rename` (variant 29) and `VfsResponse::{Ok (3), Err (4)}`; host tests compare with postcard for 7 path pairs and the reply shapes. `cargo tree -p cellos-kernel --edges normal` on x86_64/riscv64/aarch64: only `abi` and `types` from the split; no `wire`, `posix-shim`, `kms-codec`, `postcard`. | Non-`Ok` replies keep today's `PermissionDenied`. `libs/boot-config` was a second kernel path to `api`; it uses `abi` now. |
| A4 | `types` | KMS codecs in `libs/kms-codec` (no dependencies); `types::kms` users import `kms_codec`. | `api::kms` (Spec 23 STABLE) maps to `kms_codec` in Spec 23 §4.2. |
| A5 | Init-death record | `[init-death] tid= cell= gen= reason= pc= action=reset|halt`; `abi::retirement::INIT_BOOT_STOP = 0xB007_5709` → halt; any other init death → reset (x86 ACPI reset, RISC-V SBI cold reset, aarch64 new PSCI `SYSTEM_RESET`). Record set in `exit_task`; action from `yield_cpu` outside `SCHEDULER`. | — |
| A6 | Hot-swap | `serve` handles `[MAGIC, KIND_SNAPSHOT/RESTORE]` only from the live supervisor provider (attested), never replies, stashes `on_snapshot`, restores via `on_restore`, then `sys_hotswap_ready`. Config implements `ostd::hotswap::ViStateTransfer`; init adopts a swap successor (reason `0xAAAA_AAAA` + a live, different registered provider); supervisor targets `config` and `hotswap-demo`. The `wire` `ViStateTransfer` trait, `impl_state_transfer!`, `serde_helpers` and the unused vfs/cellbox impls are deleted. | `AppContext` (demos) still accepts the envelopes from any sender. |

Not Law-1 (CP1 §3): `/bin/machine-pin` and `/bin/activator` on `serve`; init has no inbound parser; append-only `MachinePinRequest` ops `Gate` (3) and `ConsoleShell` (4); kernel rules (MACHINE_PIN only for init's live `machine-pin` child, 433 only to that provider, activator may register exactly ids 16/18 for its own children, `Task::spawner`); ADR-0029 amended; `/bin/fixture-launcher` with the old init fixtures (test edges only under `test-hooks`).

## 2. Proposed for confirmation 2

Confirm A1–A6 as implemented. Re-pins: `check-multi-source-wait-law1-digests.sh` (kernel text `api::completion::source::` → `abi::completion::source::`, file paths), `check-lookupservicebound-law1-digests.sh` and `check-ai-law1-digests.sh` (paths only), the assembler sha256 pin and signed sets (`scripts/cellos_sign/signing.py`), `catalog.generated.json`. Golden dump unchanged; headers regenerated (path markers only).

## 3. Proof obligations (CP1 §4)

| # | Obligation | Result |
|---|---|---|
| 1 | Kernel dependencies | `cargo tree -p cellos-kernel --edges normal` on x86_64-linux-gnu, x86_64-none, riscv64gc-none-elf, aarch64-none-softfloat: only `abi` and `types` from the split; no `wire`, `posix-shim`, `kms-codec`, `postcard`, `api`. |
| 2 | Byte tests | Kernel `Rename` hand codec = postcard (3 host tests, 7 path pairs, reply shapes); proto/kms/tombstone pins moved with their crates and pass; `HOTSWAP_RETIRED`/`INIT_BOOT_STOP` pinned; `MachinePinRequest` `Gate`/`ConsoleShell` pinned. |
| 3 | Init death on x86 | `init-death-x86`: `[init-death] tid=2 … reason=fault … action=reset`, then a second boot banner. Boot-stop halt: host tests only. |
| 4 | Config swap on x86 | `config-hotswap-x86`: key read back after the swap, tid 7 → 14, `Init: config hot-swapped: adopted tid 14 (was 7)`, no restart. |
| 5 | Endpoints | Init's binary has no `OCLSREQ1`; init has no activation or pin module. `machine-pin-x86` 5/6 as on base (the q35 Platform-line test fails on base too). Ocel lanes fail on base too (Reader-mode default); on the branch they get further. |
| 6 | Checkpoint | pr-tier suite on x86_64, riscv64, aarch64, `x86-c2c` (two-node, negative control, release posture, domain, lifecycle), phase 09 ssh: no deterministic regression (§4). Independent review recorded (§5). Re-pins listed in §2 (+ `check-ai-law1-digests.sh`: the `ViError` conversion's path). |

## 4. Evidence

Checkpoint run (`haily-tester`, 2026-11-13, about 2 h 55 min, QEMU lanes sequential; logs `/tmp/p13/cp2run/`), every non-pass compared with the pre-phase base:

**Pass on the branch:** x86-boot, smp4, hpet-off, igb, nic, vmxnet3, virtio, iommu-dmar, serial, power, init-death-x86, config-hotswap-x86, srv-cellosfs, native-workload (all criteria; it does not exercise adoption), RV qemu-boot, the four aarch64 boots (virt, Image, vhe, novhe), pcie-bar-window, launch-profile, tier2-fault-isolation (both disks), the 57-test boot allowlist, std-smoke riscv64 and x86_64, std sysroot, aarch64 and x86 hypervisor smokes, tier3 python/nginx/virtio-e2e/hostile, phase 09 ssh (34 checks), x86-c2c two-node / negative control / release posture / domain / lifecycle 4/4, compile-only kernels on x86 and aarch64.

| Lane | Branch | Base | Verdict |
|---|---|---|---|
| ahci-x86 | 14/15, rerun 15/15 | pass | Flaky under host load (`[nvme] slow completion` → VFS bootstrap failed); not reproduced |
| pvscsi-x86 | 2/3, rerun 3/3 | pass | Same signature; not reproduced |
| machine-pin-x86 | 5/6 | 5/6 | Pre-existing (q35 Platform line) |
| std-smoke aarch64 | 1 hang, 1 pass | pass | Regression in timing, not in the kernel: std-smoke's own condvar lost-wakeup (`notify_one` on a condvar guarding two conditions) became reachable once `/bin/fixture-launcher` (not init) starts it. Fixed in the test (`notify_all`, C witness `broadcast`); aarch64 std-smoke 6/6 after |
| xhci-x86 | 4/7 | 4/7 | Pre-existing |
| vfs-quota, shell-utils build, window-policy, viui-managed-surface, ocel-browser/quickjs/static, c2c-broker-oracle, ai-oracle rv/a64, rust-std promotion tests, rv64/aarch64 workspace cell builds, x86-c2c image build in-repo, x86 hypervisor ISO step | fail | fail | Pre-existing on base |
| http-smoke | fail | fail | Environment (port 8080 in use) |


## 5. Review

Independent review (`haily-reviewer`, read-only, on steps 2–6 + the rebase fix): PASS_WITH_RISK, 4 medium, 8 low. Fixed in `fix(phase13): review findings…`:

| Severity | Finding | Disposition |
|---|---|---|
| Medium | A SupervisorCap holder could pause, freeze and replace the `MACHINE_PIN` provider through the hot-swap path (only `register_service` checked the id), and init would adopt the replacement. | Fixed: the kernel refuses pause, freeze, replacement and commit for `MACHINE_PIN`; init never adopts a successor for the pin row. Kernel and init tests. |
| Medium | aarch64 init-death reset hard-coded the HVC conduit. | Fixed: shared `usable_psci_conduit()` (declared conduit; refuses EL2 HVC and spin-table boards); otherwise log and halt. |
| Medium | A successor that died before init's exit watch would be adopted unwatched and never restarted. | Fixed: a failed watch on a dead successor is handled as that service's death. |
| Medium | A crashed activator left its engines registered and alive. | Fixed: a starting activator force-exits engines registered under `OCEL_JS`/`OCEL_PDF` that are not proven dead. |
| Low | `0xAAAA_AAAA` copied in three places. | Fixed: `abi::retirement::HOTSWAP_RETIRED`, byte-pinned with `INIT_BOOT_STOP`. |
| Low | Syscall 433 doc still named SpawnCap. | Fixed. |
| Low | Config accepted duplicate keys; restore capped at 64 KiB vs 1 MiB snapshots. | Fixed: duplicates refused; restore sized to the stash entry. |
| Low | Activator lost the bounded retry for posted replies. | Fixed (within `LEGACY_TICKS`). |
| Low | Stash keys are one global namespace with a predictable swap id: a `StateStash` cell can squat a key and make config swaps roll back (DoS only). | Residual. |
| Low | `ConsoleShell` trust rests on init's tid from a 16-row `GetProcs` scan; a miss strands the machine in safe mode (fails closed). | Residual. |
| Low | Every non-boot-stop init death resets with no backoff, so a deterministic late init fault reboot-loops. | Residual (owner chose reset). |
| Low | The `tier2-grant-pair` compile guards against `development-silo-provider`/`c2c-broker` were removed with the init feature. | Residual. |


## 6. Not done / residual

- The app-tier acceptance ledger is not re-bound (needs a signed lifecycle event by a named steward and reviewer); `validate-app-tier-acceptance.py` already failed on the base.
- `tests/rust-std-promotion/test_validator.py` and `verify-cell-contract-p01.py` reference `libs/api` evidence paths in `.agents`; both already failed on the base.
- The init boot-stop halt path is proven by host tests only; no lane boots it.
- Pre-existing failures unchanged: `ocel-browser`/`ocel-quickjs` (Ocel's Reader-mode default), `machine-pin-x86` q35 Platform log line, `check-lanes.py` tier3 rows, `check-posix-ledger.py` (40), lua/tetris-lua RV header clash, `doom` source, `app-mlibc-smoke` libc.

## 7. Owner decision

Pending.
