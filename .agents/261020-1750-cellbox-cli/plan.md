---
title: "Cellbox: CLI-first essential tools, typed pipelines and selective Ratatui TUI"
status: planned
priority: P1
tier: thinking
created: 2026-10-20
blockedBy: []
blocks: []
executionGate: "Documentation/planning approved; implementation slice selection and exact retained gates required"
---
# Cellbox implementation plan

## Scope and authority

Deliver all accepted [ADR-0028](../../docs/decisions/0028-cellbox-cli-first-structured-pipelines-and-tui.md) decisions under the [CLI design](../../docs/cellbox-cli.md). The maintainer requested recording decisions and planning, not code execution. **All implementation phases remain pending.** `blockedBy: []` means no dependency on executing an old locked plan; it is not implementation admission. Read the execution gate below before any direct phase-file entry.

This is a bounded Intel x86-64 operational-tooling workstream supporting named console/config, Cell observation, file/log inspection and guest/other-host access consumers. It does not replace ADR-0022's program or reopen new hardware/application ecosystems. IDEs, language-server/build environments, general app distribution, a wholesale Linux/Nushell port and inbound SSH-server development are excluded.

The implementation layout selected by this plan is one `/bin/cellbox` with a `sh` entrypoint for the migrated shell module. This avoids embedding the same command engine in a separate shell ELF. It preserves shell state semantics, not old paths/wrappers. Standalone apps/services remain standalone and can consume public terminal/TUI libraries independently.

## Entry gate and ownership

1. Phase 01 is the next preparation step after a future implementation request; do not auto-run it from this planning session.
2. Reconcile current source and exact approval evidence: portfolio/current-focus retain both freeze text and later completion reports. Do not infer missing source from old checkboxes or blanket permission from a completion heading.
3. Name one integrating owner for shell/startup identity, API/SDK, kernel launch/telemetry, VFS and input/session changes. Adopt any overlapping unfinished authority/runtime prerequisite as a named controlling-plan slice; no circular dependency on running a locked predecessor.
4. Gate each concrete Law-1 delta separately: **CP1 design approval**, bound to the exact specification digest, precedes editing that ABI surface; merely recording Pending/Blocked does not admit its dependent source phase. After implementation in an explicitly admitted disposable verification profile, record the exact diff, consumer migration census and runtime evidence and obtain **CP2 implementation approval before final caller/image retirement or rollout for that delta**. CP2 is not a prerequisite for writing the implementation it must review. P10 audits every delta's CP2 closure. Retained frozen ABI, protected identity, signing and physical/remote/production gates remain independent; no approval-by-agent or blanket root/union rights.
5. Update portfolio and only affected lock relations for the selected slice with a file-owner handoff. WIP stays one implementation slice; independent preparatory research/review may run concurrently.

## Phases and dependencies

| Phase | Deliverable | Depends on | State |
|---|---|---|---|
| 01 | [Catalog, baseline, upstream/runtime and exact contract gates](phase-01-baseline-contracts.md) | — | pending |
| 02 | [Public terminal SDK and real child/session streams](phase-02-terminal-sdk-streams.md) | 01 | pending |
| 03 | [Typed command data, bounded pipelines and explicit conversions](phase-03-typed-pipelines.md) | 01,02 | pending |
| 04 | [Single cellbox artifact and complete caller/image cutover](phase-04-multicall-cutover.md) | 01,02,03 | pending |
| 05 | [Ratatui backend and complete GUI-terminal emulation](phase-05-ratatui-terminal.md) | 02,04 | pending |
| 06 | [Cell-centric telemetry/top, task drill-down and honest metrics](phase-06-cell-top.md) | 03,04,05 | pending |
| 07 | [Ripgrep-based VFS search and direct result records](phase-07-ripgrep-search.md) | 03,04 | pending |
| 08 | [Compact editor and verified save semantics](phase-08-compact-editor.md) | 04,05 | pending |
| 09 | [Secure outbound SSH and guest/other-host interoperability](phase-09-ssh-client.md) | 02,04 | pending |
| 10 | [Integrated signed-image qualification and measured footprint](phase-10-qualification.md) | 01,02,03,04,05,06,07,08,09 | pending |

Each phase includes real target smoke proof; Phase 10 is integrated qualification, not the first runtime exercise. Phases 06–09 have partly independent prerequisites, but shared-source execution remains single-owner/single-slice. Do not pad the DAG with unneeded runtime/platform projects.

## Required outcomes and decision coverage

[acceptance.md](acceptance.md) owns CBX-01..CBX-29, all pending. No excluded/blocked feature may be called complete or silently narrowed.

| ADR decisions | Implementation owner | Acceptance |
|---|---|---|
| D01 CLI-first/shared backend, D03 curated scope | 01,04,10 | CBX-01,09,10,12,26,28 |
| D02 modular single artifact, independent app boundary | 04,05,10 | CBX-09,10,16,26 |
| D04 typed/native and raw data, explicit serialization | 02,03,06,07,09 | CBX-04..08,18..20,23 |
| D05 selective TUI | 05,06,08,09 | CBX-13,15,18,21,23,29 |
| D06 reusable Terminal SDK/Ratatui outside viUI | 02,04,05 | CBX-03..05,12..16 |
| D07 Kibi-inspired editor | 08 | CBX-21,22 |
| D08 ripgrep-based search | 07 | CBX-19,20 |
| D09 Cell monitoring | 06 | CBX-17,18 |
| D10 outbound SSH | 09 | CBX-23..25 |
| D11 authority/licensing/clean migration | 01,04,09,10 | CBX-01,02,10,11,24..28 |

## Assumptions requiring entry verification

- Ratatui's selected release/features and backend compile within Cellos's admitted allocation/atomic/runtime profile. Verify a target build plus a throwaway real terminal consumer in 01/05.
- Required ripgrep components can use an admitted runtime or bounded VFS adapter without a new general runtime program. Verify exact crate/dependency graph and target I/O path in 01/07; do not rebrand grep if not.
- Safe file replacement/conflict detection and host-trust persistence can be supplied by existing VFS contracts or an explicitly approved delta. Verify failure/durability behavior in 01/08/09.
- Cell-incarnation telemetry and terminal session attachment have an authoritative admitted provider. Current task records/local GUI booleans are insufficient. Verify exact source/layout/owner before 02/06.
- A controlled reachable Tier-3 SSH server and a separate other-host fixture are available for proof. Verify endpoint/route/server/key provenance; unavailable guest evidence blocks only that scenario, not invented success.

Facts and source references live in [scout-report.md](scout-report.md). Proposed crate/API/test names are implementation targets, not pre-existing facilities.

## Verification strategy and command boundaries

[verification.md](verification.md) separates currently existing runners from planned cellbox consumers. Pure deterministic tests cover data/stream/editor/search/metric invariants; actual signed x86 images demonstrate serial, GUI-terminal and SSH paths. Host-only output never qualifies a Cellos port. Capture keyboard/resize and the resulting real screen/state for TUI. Do not test source copies, forwarded mock echoes or incidental wording.

Before packaging, all affected builds must succeed; warnings/stale files cannot masquerade as new cellbox artifacts. Use private writable image copies, unique QMP endpoints and no unsolicited network exposure. Do not run `run-desktop.sh` unattended: it kills existing image users and exposes an inbound SSH forward. Use controlled fixture servers, never the unsafe current Cellos sshd.

## Risk and rollback

- Authority consolidation is the principal risk. Stop at an unresolved launch/service boundary instead of granting all invocations administrative/device rights.
- After a selected consumer cutover, remove old implementations/targets/routes together; do not leave shims. Recover an unsuccessful rollout using a matched known-safe code/image/policy set and cold boot, not an insecure compatibility fallback.
- User file writes, remote command effects and persisted trust decisions are not undone by code rollback. Use disposable fixtures and backups; preserve unknown outcomes and never replay automatically.
- No metric target or compatibility breadth is fabricated. Record actual before/after feature sets and the evidence ceiling; physical Intel, guest VMX, production and protected authority gates remain independent.

## Review and handoff

See [review.md](review.md) for adversarial review findings/dispositions and [verification.md](verification.md) for documentation checks. Architecture is accepted; concrete syntax/schema/wire/version and implementation gates remain entry work, not user-product decisions to reopen.

**Plan path:** `/home/dmin/cellos/.agents/261020-1750-cellbox-cli/plan.md`  
**Future handoff:** `$hc-cook /home/dmin/cellos/.agents/261020-1750-cellbox-cli/plan.md` only after an explicit implementation request and the phase's retained entry gates. This invocation is not authorization to bypass locks or auto-start code now.
