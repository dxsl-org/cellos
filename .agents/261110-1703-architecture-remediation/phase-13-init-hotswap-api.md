---
phase: 13
title: "Init reduction, hot-swap decision, api split"
status: completed
priority: P2
effort: "8–11d"
dependencies: [12]
tier: thinking
checkpoint: true
---

# Phase 13: Init reduction, hot-swap decision, `api` split

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Context Links

[Review H-13, Medium userspace](../reports/architecture-review-261110.md) · `cells/tools/init/src/{main,supervisor,boot,activation,service_table}.rs` · `cells/services/supervisor/src/{main,hotswap,protocol}.rs` · `libs/api/src/{lib,services}.rs` · `libs/api/src/services/posix/` · `libs/types/src/kms/` · `libs/ostd/src/hotswap.rs` · [Spec 03](../../docs/specs/03-runtime.md) · [Spec 12](../../docs/specs/12-reliability.md)

## Overview

Shrink the only restarter to spawn-and-supervise with a stated death policy, make hot-swap either work for a real service or stop being claimed, and separate frozen ABI from evolvable wire and from the libc shim.

## Key Insights

- Reviewer-cited: init hosts the activation and machine-pin endpoints and registers `OCEL_ACTIVATOR` (`init/src/main.rs:60-66,149`); test fixtures compiled into init squat on production paths (`boot.rs:10-31`); 17 features; `TRUSTED_INIT_TID` is first-wins (`task/syscall.rs:2089-2096`); no init-death handling found.
- Hot-swap target map names vfs/net/compositor/input (`supervisor/src/main.rs:171-179`) but none handles the `0xAC 0xF0/0xF1` envelopes or calls `sys_hotswap_ready`; two traits share the name `ViStateTransfer`.
- `api` re-exports ABI + service wire + a 139-symbol libc shim; the kernel encodes `VfsRequest` and so links all three; `types` is 81% KMS codecs.

## Requirements

- Functional: activator and machine-pin endpoints run as supervised cells; fixtures live in a test cell using catalog fixture rows (possible since Phase 05); kernel policy for init death is explicit (reboot, logged with incarnation); hot-swap: either one production service (config — smallest state) completes a real swap through `serve` hooks, or the map entries, dead impls and Spec 03/12 claims are removed — default is **wire config, remove the other three map entries**; crates `api` → `abi` (no deps) + `wire` (per-service proto from Phase 12) + opt-in `posix-shim`; KMS codecs leave `types`.
- Non-functional: re-exports keep existing import paths compiling for one cycle; byte tests pin wire layouts; Law-1 CP1/CP2.

## Architecture

```
init (PID 1): read config ─ spawn ─ supervise ─ restart      (no IPC-reachable parsers beyond death records)
   ├─ activator cell   (supervised)
   └─ machine-pin cell (supervised)
kernel: init incarnation retired ⇒ record fault evidence ⇒ platform reset
libs/abi  ◄── kernel, ostd          libs/wire ◄── services, clients (kernel only for events it must encode)
libs/posix-shim ◄── ffi-posix cells only
```

## Assumptions

- **Claim:** the kernel can stop depending on service wire once `Rename` moved in Phase 11.
  **Confidence:** medium
  **How to verify:** grep `VfsRequest|VfsResponse` under `kernel/src` after Phase 11.
- **Claim:** config's state is small and self-contained enough to be the hot-swap witness.
  **Confidence:** medium
  **How to verify:** read `cells/services/config/src/main.rs:143` (`impl ViStateTransfer`).
- **Claim:** crate renames do not disturb the signed-artifact names or the unsafe allowlist keys.
  **Confidence:** low
  **How to verify:** check how `scripts/unsafe-allowlist.toml` keys entries (path vs crate).

## Related Code Files

- Create: `libs/abi/`, `libs/wire/`, `libs/posix-shim/`, `libs/kms-codec/`, `cells/services/activator/`, `cells/services/machine-pin/`, `cp1-api-split.md`, `cp2-api-split.md`
- Modify: `cells/tools/init/**`, `kernel/src/task/syscall/` (init-death policy), `cells/services/supervisor/src/{main,hotswap}.rs`, `cells/services/config/src/main.rs`, `libs/api/**` (becomes a re-export facade, then removed), `libs/types/**`, root `Cargo.toml`, `catalog/cells.toml`, `config/services.toml`, `docs/specs/{03-runtime,12-reliability}.md`
- Delete: `cells/services/vfs/src/manager/state_transfer.rs` (dead impl), one of the two `ViStateTransfer` traits, init's fixture modules

## Implementation Steps

1. CP1 for the crate split and any init/kernel contract change. Owner confirmation 1.
2. Split crates behind a facade; byte tests for every wire type; kernel depends on `abi` only.
3. Move endpoints out of init; add catalog rows and service entries; move fixtures to a test cell.
4. Kernel init-death policy with bounded fault record; x86 fixture kills init.
5. Hot-swap: implement config Snapshot/Restore through `serve`; run v1→v2 swap in QEMU; remove the other map entries and dead impls; correct Spec 03 §3 and Spec 12 axis 5.
6. Migrate imports off the facade; delete `libs/api`; rename the `supervisor` service only if the owner wants (naming collision noted, not required).
7. CP2.

## Todo List

- [x] CP1
- [x] Crate split with facade
- [x] Init reduced
- [x] Init-death policy
- [x] Hot-swap witness or removal
- [x] Facade removed
- [x] CP2

## Success Criteria

- [x] `cargo tree -p cellos-kernel` shows no dependency on `wire` or `posix-shim`.
- [x] x86 fixture: killing init produces the fault record and a platform reset, not a hung system.
- [x] x86 QEMU: config service hot-swap completes with state preserved (a key written before the swap is read after) — or, if removal was chosen, grep finds no swap target without a handler.
- [x] Checkpoint: full `pr`-tier suite on three arches, `x86-c2c`, release-posture boot, review recorded.

## Risk Assessment

- Crate rename churn across ~200 members: the facade limits it to mechanical import edits. Rollback: keep the facade.
- Init changes can brick boot: every step verified on the x86 boot lane before the next.

## Security Considerations

PID 1 loses its IPC-reachable parsers, shrinking the attack surface of the one unrestartable process. The libc shim stops being compiled into the kernel's dependency graph.

## Next Steps

S6 closes here. Phase 14 may already be done if the owner ran S7 earlier.

## Deviation Log

- **Surprise (CP1 census):** `api` had 154 dependent crates, not 50; the kernel used the fs/block traits and kernel-emitted frames, not only `Rename`; `abi` needs `serde` (two ABI records derive it). CP1 A2/A3 cover this.
- **Decision (owner, CP1):** kernel links `abi` + `types` only; boot-stop halts, other init deaths reset; ADR-0029 trust root moves to the init-launched `machine-pin`.
- **Surprise (step 2):** `libs/boot-config` was a second path from the kernel to `api`; it now uses `abi`. A2 modules in `abi` are named `kernel_ipc`, `supervisor_power`, `input_producer`, `app_envelope` (root names `ipc`/`proto` would collide in the facade glob); `wire` re-exports them at their old paths. `kms-codec` needs no dependency; `types::kms` users import `kms_codec` directly, while `api::kms` stays (Spec 23 STABLE path).
- **Decision (step 2):** the kernel's `Rename` reply mapping stays as today (every non-`Ok` reply → `PermissionDenied`); the hand codec is pinned to postcard by host tests.
- **Deviation (step 3):** the pin cell needs a gate answer, not only `Status`: `MachinePinRequest` gained append-only op 3 `Gate` (response kind 4) and op 4 `ConsoleShell { tid }`, byte-pinned. Confirm: init hands the console shell's tid to the pin cell; the pin cell accepts it only from init (its launch argument `init=<tid>`).
- **Deviation (step 3):** the pin cell declares `VfsMutate`, so VFS's init mutate-flag exception was removed, not renamed. An x86 image without `/bin/machine-pin` (or one that does not answer in ~2 min) is safe mode.
- **Deviation (step 3):** the fixture launcher is a production catalog row (the RV boot suite needs `vfs-test`/`ai-test` at boot); its Tier-2/rpc/edu/grant-pair edges exist only under `test-hooks`. It tries the kernel path route first, then VFS read + ELF, as init's `sys_spawn_from_path` did.
- **Surprise (step 3 lanes):** `ocel-browser` and `ocel-quickjs` fail on the pre-phase base too (Ocel opens a first load in Reader mode since `4994602b5`, so no script engine is requested); `machine-pin-x86 q35_unbound_root_port_nic_is_in_the_pci_set` waits for a Platform line changed by `06fc50241`; `check-lanes.py` lacks rows for the tier3 chrome/display scripts. All pre-existing, not fixed here. `shell_case_statement` failed 2/3 once under concurrent builds and passed 3/3 alone and 4/4 on base: flaky under load.
- **Decision (step 4):** `INIT_BOOT_STOP = 0xB007_5709` (`abi::retirement`). The record and flag are set in `exit_task`; the reset/halt runs from `yield_cpu` after the deferred-retirement match, outside `SCHEDULER`. aarch64 gained PSCI `SYSTEM_RESET` through the declared conduit (`usable_psci_conduit`, halt if none). The fixture faults by stack-guard recursion (init keeps `forbid(unsafe_code)`; F1 signing refuses unsafe in signed cells). The halt path is covered by host tests only.
- **Decision (step 5):** `serve` authenticates swap envelopes as attested messages from the live supervisor provider and never replies; init adopts a successor only for reason `HOTSWAP_RETIRED` and a live, different registered provider. The config lane builds its own image (the canonical headless profile refuses `/bin/config`) and reads the key back through the existing `/bin/p03-filter` probe. `AppContext` (demos) still accepts the envelopes from any sender.
- **Decision (step 6):** `ostd` re-exports only `ViError`/`ViResult` and a hidden `declare_manifest` path; posix-shim now links only into the 12 C/mlibc cells that depend on it. Spec 23 maps every old `api` path (§4.2); the app-tier ledger is not re-bound (needs a signed lifecycle event).
- **Surprise (step 6):** `rustfmt --check` on stdin exits 0 even with a diff, so the "clean at HEAD" check used in Phases 12–13 never refused anything. Step 6 restored the original formatting of 29 files unformatted at HEAD. Phase 12 (already on `main`) reformatted 9 files that were not rustfmt-clean before (vfs `backend_fat`, `block_stream`, `blk_router*`, net `service-runtime`, hypervisor `virtio_input`, silo-guest `main`, `nvme-x86.rs`).
- **Rebase (CP2):** main gained the native-stateful bench rework; its new code moved from `api::` to `abi`/`wire` (`fix(bench)…`).
- **Review (CP2):** see [cp2-api-split.md §5](cp2-api-split.md). The AI Law-1 digest needle followed the `ViError` conversion's new path (`types::ViError`).
- **Surprise (CP2):** aarch64 std-smoke hung 4 of 6 runs on the branch (base 5/5). Root cause (debugger, futex trace): std-smoke's threading witness used `notify_one` on a condvar guarding two conditions; with the kernel's FIFO futex wake, a worker could consume main's wake. The fixture launcher's exit changed the scheduling enough to hit it. Fixed in the test (`notify_all`; C pthread witness `broadcast`); 6/6 after.
- **Note (CP2):** `init-death-x86` copies `kernel/src/embedded-x86_64`; the committed image predates `/bin/machine-pin`, so the lane needs the image assembled first (as `pr.sh` and CI do).

