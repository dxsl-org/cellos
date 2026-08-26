---
phase: 4
title: "Complete Phase 04 Deprivilege"
status: completed (respawn proof deferred)
priority: P1
effort: 3d
dependencies: [2]
tier: thinking
---

# Phase 04: Complete Phase 04 Deprivilege

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Overview

Finish init/shell deprivilege with kernel-enforced launch-edge authority. The plan intentionally drops the spawn-broker slice: the minimal broker API would be additive service ID 13 plus typed IPC and attested receive, but it preserves ambient init authority behind a reachable deputy and does not fix the boolean `SpawnCap` ceiling problem. This phase is recorded complete with init-respawn proof deferred because the final lane did not directly exercise that path.

## Requirements

- Functional: shell-initiated `/bin/<name>` launches remain possible through existing spawn syscalls, but shell must not hold lifecycle authority (`ForceExit`, `HotSwap`, service registration, restart supervision).
- Functional: init keeps supervised lifecycle authority for boot and respawn, but child capability grants must be bounded by a kernel launch profile for the exact `(spawner role, target path, spawn route)` edge.
- Functional: reject `SpawnFromMem`/`SpawnFromElf` attempts that cannot be tied to an approved launch edge; caller-controlled names must not select `/bin/` authority.
- Non-functional: no new service ID, no new public request/response ABI, no ambient broker. Any later change under `libs/api/` or `libs/types/` remains a Law 1 2/2 stop.
- Non-functional: denials must be observable in QEMU logs; no silent-deny or prompt-only proof.

## Architecture

Current observed flow: init declares `spawn = true` (`cells/tools/init/src/main.rs:7-8`), spawns supervised services from `paths` (`cells/tools/init/src/main.rs:89-100`, `cells/tools/init/src/main.rs:193-199`), registers services (`cells/tools/init/src/main.rs:106-115`), and respawns from the same table (`cells/tools/init/src/main.rs:273-355`). Shell declares `spawn = true` and console MMIO solely so its children inherit authority (`cells/tools/shell/src/main.rs:7-20`), and launches external commands through `sys_spawn_from_path` (`cells/tools/shell/src/executor.rs:866-880`) plus the older `sys_spawn_from_mem` exec path (`cells/tools/shell/src/commands.rs:78-98`).

Existing gates: `SpawnFromPath`, `SpawnFromElf`, and `SpawnFromMem` all check `caller_has_spawn` before they know the full launch edge (`kernel/src/task/syscall.rs:2448-2473`, `kernel/src/task/syscall.rs:2580-2641`, `kernel/src/task/syscall.rs:3075-3126`). The loader already applies per-path boot ceilings and policy (`kernel/src/loader.rs:249-304`), and `boot_ceiling::lookup` is per-path, not union-shaped (`kernel/src/loader/boot_ceiling.rs:1-22`, `kernel/src/loader/boot_ceiling.rs:51-127`).

Target flow:

```text
spawn syscall -> parse route + target label/path -> kernel launch-edge table
  -> approve by caller role/identity + route + exact path/profile
  -> loader spawn_gated(path, Spawner::Ceiling(profile.parent_ceiling))
  -> policy applies as final cap intersection
```

Design decisions:
- Add kernel-internal `loader::launch_profile` (or equivalent) as the single edge table. Do not add `service::SPAWN_BROKER = 13` unless the user explicitly rejects this design after the Law 1 checkpoint.
- Split gates in `kernel/src/task/syscall.rs`: lifecycle operations keep `caller_has_spawn`; launch operations use `authorize_launch_edge` after input validation. This preserves init/supervisor authority while allowing shell to lose `SpawnCap`.
- Treat `sys_spawn_from_mem` as cross-cutting. Default action is to deny privileged/lifecycle-bearing manifests from `SpawnFromMem` unless a reviewed route-specific profile exists; shell command execution should converge on `SpawnFromPath` or `SpawnFromElf` with an exact path hint before shell loses `SpawnCap`.
- Keep attested receive as a reusable primitive only. It exists (`libs/api/src/abi/caller_identity.rs:10-45`, `kernel/src/task/syscall.rs:555-600`) but is not a sufficient reason to add a broker in this phase.

## Assumptions

- **Claim:** Existing `sys_spawn_from_mem` usage outside shell/init does not require privileged child caps.
  **Confidence:** medium
  **How to verify:** `rg -n "sys_spawn_from_mem|SpawnFromMem" cells libs kernel tests -S` and classify every caller before implementation.
- **Claim:** Kernel-internal launch profiles can be implemented without changing `libs/api/` or `libs/types/`.
  **Confidence:** high
  **How to verify:** prototype only in `kernel/src/loader*` and `kernel/src/task/syscall.rs`; if a public wrapper/type is needed, stop for Law 1 2/2.

## Related Files

- Create: `kernel/src/loader/launch_profile.rs`
- Modify: `cells/tools/init/src/main.rs`
- Modify: `kernel/src/loader.rs`
- Modify: `kernel/src/loader/boot_ceiling.rs`
- Modify: `kernel/src/loader/boot_ceiling/selftest.rs`
- Modify: `kernel/src/task/cap.rs`
- Modify: `kernel/src/task/syscall.rs`
- Modify: `cells/tools/shell/src/main.rs`
- Modify: `cells/tools/shell/src/executor.rs`
- Modify: `cells/tools/shell/src/commands.rs`
- Modify: `scripts/sign-policy.py`
- Modify: `tests/integration/*` or add one focused integration test under existing integration harness
- Modify: `.agents/260727-2101-midori-lessons-cellos/phase-04-deprivilege-init-shell.md`

## Implementation Steps

1. Confirmation checkpoint A: explicitly record that Phase04 will avoid Law 1 by adding no service ID, syscall ID, public ABI type, or `libs/types` contract. If implementation later needs any `libs/api/` or `libs/types` edit, stop and obtain Law 1 confirmation #1 and #2 for the exact diff. 2026-08-05 note: confirmation 2/2 was later granted for comments-only ABI documentation updates that describe the already-approved spawn semantic change without altering IDs, signatures, layout, or values.
2. Inventory spawn callers and classify them as lifecycle, exact-path launch, exact-path hotswap, or memory launch. Mandatory commands: `rg -n "caller_has_spawn|SpawnFromPath|SpawnFromElf|SpawnFromMem|sys_spawn_from_path|sys_spawn_from_mem" kernel/src cells libs tests -S`.
3. Add `kernel/src/loader/launch_profile.rs` with static rows for approved launch edges. Include caller role, route (`path`, `elf`, `mem`), target path or `/mem/` class, child cap ceiling, whether lifecycle authority is required, and denial log label.
4. Wire `mod launch_profile` in `kernel/src/loader.rs`; reuse `CapSet` and existing `Spawner::Ceiling` instead of introducing another cap representation.
5. Split `kernel/src/task/syscall.rs` gates: keep `caller_has_spawn` for lifecycle operations (`ForceExit`, `NotifyOnExit`, `RegisterService`, hotswap/snapshot/freeze/resume/kill); replace the launch-only precheck with `authorize_launch_edge(caller_id, route, path_or_label)` after pointer validation.
6. For `SpawnFromPath` and `SpawnFromElf`, pass the launch profile's parent ceiling into `spawn_from_path` / `spawn_gated`. Fail closed if the path is not exact, overlong, outside `/bin/`, or absent from the edge table.
7. For `SpawnFromMem`, keep `mem_spawn_gate` label sanitization (`kernel/src/loader/mem_spawn_gate.rs:1-13`, `kernel/src/loader/mem_spawn_gate.rs:51-64`) and deny privileged child manifests by default. Do not use the caller-chosen name as a policy/ceiling path.
8. Remove shell `spawn = true` and shell lifecycle syscalls from its manifest/allowlist. Keep only launch syscall(s) actually needed by the implemented path. Update `commands.rs` so the legacy mem exec path is either removed, routed to exact-path spawn, or explicitly denied with a visible message.
9. Narrow `/bin/shell` in `boot_ceiling::lookup` and `scripts/sign-policy.py` to remove `spawn`; keep only MMIO needed for child delegation if launch profile explicitly preserves that edge. Do not narrow `/bin/init` until all boot service launch edges are represented and pass QEMU.
10. Add self-tests: launch table rejects unknown path, shell lacks lifecycle authority, init can still supervise service respawn, `/mem/` labels cannot acquire `/bin/` path caps, and boot ceiling remains per-path.
11. Runtime validation: boot QEMU, prove `Init: service registry verified.`, prove shell can launch one approved harmless binary, prove unauthorized shell lifecycle operation is denied, and prove an unapproved memory spawn is denied with a kernel log line.
12. Update Midori Phase04 source plan/status and same-commit evidence notes only after runtime evidence is captured.

## Todo List

- [x] Confirmation checkpoint A recorded: no Law 1 surface in this phase.
- [x] Spawn callers classified and every caller listed or explicitly counted.
- [x] Kernel launch-edge table added with exact path/profile rows.
- [x] Spawn syscall gates split between launch authorization and lifecycle authority.
- [x] Shell loses `SpawnCap` and lifecycle syscalls.
- [x] `SpawnFromMem` privileged path-forgery case denied.
- [x] Boot/policy/self-tests updated with no silent-deny paths.

## Success Criteria

- [ ] `rg -n "SPAWN_BROKER|service::SPAWN|SpawnBroker|SpawnBrokerRequest|SpawnBrokerResponse" libs/api libs/types cells kernel` returns no Phase04 broker additions.
- [x] `tests/integration/tests/launch-profile.rs` and manual QEMU proof cover launch-profile, boot-ceiling, and mem-spawn denial behavior.
- [x] QEMU boot log proves init, VFS, shell, and service registry verification after shell loses `SpawnCap`.
- [x] Runtime denial proof shows shell cannot perform lifecycle actions formerly implied by `SpawnCap`.
- [x] Runtime positive proof shows shell can launch an approved command without lifecycle authority.
- [x] Phase 04 status text updates in the same change as runtime evidence.

## Security Considerations

Broker rejected: `LookupService` is open (`libs/api/src/abi/syscall.rs:610-611`, `kernel/src/task/syscall.rs:815-816`), so a service endpoint is not a security boundary. Attested receive is useful but does not remove ambient broker authority. Kernel launch-edge enforcement is smaller and removes the confused-deputy hop.

The remaining high-risk boundary is `SpawnFromMem`: existing code already reduces caller names to `/mem/` labels so they cannot select `/bin/` path authority (`kernel/src/loader/mem_spawn_gate.rs:4-13`). Phase04 must preserve that invariant and add explicit route policy rather than relaxing it.

## Risk Notes

| Risk | Likelihood x Impact | Mitigation | Rollback |
|------|---------------------|------------|----------|
| Launch table under-specifies boot edges | Medium x Critical | Start with observed init table and fail-loud logs before narrowing `/bin/init` | Revert `launch_profile` wiring and restore `caller_has_spawn` launch gate |
| Shell loses ability to run demos | Medium x High | Positive QEMU launch proof for one ordinary command and one MMIO child if retained | Restore shell `spawn = true` and policy row, then re-plan narrower edges |
| `SpawnFromMem` carve-out creates path-cap bypass | Medium x Critical | Default deny privileged mem launches; preserve `/mem/` label invariant tests | Revert mem-route authorization change |
| Accidental Law 1 edit | Low x High | Confirmation checkpoint before any `libs/api` or `libs/types` edit | Revert public API edit; kernel-only plan remains valid |
| Init/supervisor lifecycle gate regression | Medium x High | Keep lifecycle gate on existing `SpawnCap`; test service respawn | Revert syscall gate split only |

## Backwards Compatibility

No service IDs or syscall IDs change. Existing service IDs remain exactly as defined through `GPU_DRIVER = 12` (`libs/api/src/abi/syscall.rs:813-850`); service ID 13 stays unused in this phase. Shell command behavior should stay compatible for approved `/bin/*` launches, but lifecycle/hotswap/force-exit behavior intentionally narrows.

Rollback plan: if runtime evidence fails, revert kernel launch-profile wiring plus shell/policy narrowing as one unit. Cannot roll back the security conclusion that a broker is a weaker design without a new explicit architecture decision.

## Deviation Log

- 2026-08-05 — Confirmation checkpoint A: this implementation stays kernel-only and Phase04-owned. No `libs/api/` or `libs/types/` edits are permitted; if a public ABI change becomes necessary, stop for Law 1 double confirmation.
- 2026-08-05 — Law 1 confirmation 2/2 granted for the exact semantic change already implemented: `SpawnFromPath` / `SpawnFromElf` / `SpawnPinned` are exact kernel launch-profile routes, lifecycle operations remain `SpawnCap`-gated, and IDs/signatures/layouts stay unchanged. Applied only comments/docs updates in `libs/api/src/abi/syscall.rs`.
- 2026-08-05 — Spawn caller inventory: `sys_spawn_from_mem` is only used by `cells/tools/shell/src/commands.rs`, so the mem-route hardening can fail closed there and move shell `exec` to the exact-path `SpawnFromElf` route without touching any other caller.
- 2026-08-05 — Compatibility audit correction: the first launch-edge table undercounted non-shell callers. Added explicit reviewed routes for `hypha` fixed children (`/bin/llm-gateway`, `/bin/tool-fs`, `/bin/tool-sys`, `/bin/tool-spawn`), `tool-spawn` reviewed-user launches, supervisor hotswap launches gated by `SupervisorCap`, and pinned-only edges for `bench`, `capacity-probe`, and `periph-demo`.
- 2026-08-05 — Compatibility rationale narrowed after review: removed the dead Lua launch-profile branch because the Lua runtime no longer exposes `SpawnFromPath` / `os.execute`. Kept only the exact `periph-demo` `SpawnPinned("/bin/periph-demo")` compatibility edge.
- 2026-08-05 — Runtime evidence captured manually in QEMU and encoded into `tests/integration/tests/launch-profile.rs`: boot selftest pass, `Init: service registry verified.`, shell prompt, positive `vfs-test` shell spawn, denied `snapshot`, and no wrapped `usize::MAX` snapshot success line.
- 2026-08-05 — Respawn proof was not directly exercised in the final lane; record this phase as completed with that proof deferred rather than claiming full init-respawn coverage.
