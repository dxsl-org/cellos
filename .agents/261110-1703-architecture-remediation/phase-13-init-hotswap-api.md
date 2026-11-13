---
phase: 13
title: "Init reduction, hot-swap decision, api split"
status: in-progress
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
- [ ] Crate split with facade
- [ ] Init reduced
- [ ] Init-death policy
- [ ] Hot-swap witness or removal
- [ ] Facade removed
- [ ] CP2

## Success Criteria

- [ ] `cargo tree -p cellos-kernel` shows no dependency on `wire` or `posix-shim`.
- [ ] x86 fixture: killing init produces the fault record and a platform reset, not a hung system.
- [ ] x86 QEMU: config service hot-swap completes with state preserved (a key written before the swap is read after) — or, if removal was chosen, grep finds no swap target without a handler.
- [ ] Checkpoint: full `pr`-tier suite on three arches, `x86-c2c`, release-posture boot, review recorded.

## Risk Assessment

- Crate rename churn across ~200 members: the facade limits it to mechanical import edits. Rollback: keep the facade.
- Init changes can brick boot: every step verified on the x86 boot lane before the next.

## Security Considerations

PID 1 loses its IPC-reachable parsers, shrinking the attack surface of the one unrestartable process. The libc shim stops being compiled into the kernel's dependency graph.

## Next Steps

S6 closes here. Phase 14 may already be done if the owner ran S7 earlier.

## Deviation Log

None.
