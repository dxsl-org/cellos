---
title: "Cellbox: CLI-first essential tools, typed pipelines and selective Ratatui TUI"
status: closed-private-evidence-at-stated-ceilings
priority: P1
tier: thinking
created: 2026-10-20
blockedBy: []
blocks: []
executionGate: "Design consent recorded; dev-posture Intel x86 QEMU/TCG candidate qualified and its desktop bytes promoted to the canonical paths (S10-5 CP2 granted by the user 2026-10-29); CBX-27 unmet, CBX-23/24/25 partial; production/physical/VMX/remote gates, cloud formats, non-Intel recipes and all 38 CELL-CONTRACT-0024 lock relations remain blocked/refused"
---
# Cellbox implementation plan

## Scope and authority

Deliver all accepted [ADR-0028](../../docs/decisions/0028-cellbox-cli-first-structured-pipelines-and-tui.md) decisions under the [CLI design](../../docs/cellbox-cli.md). The original session approved documentation/planning; the later explicit “tôi duyệt, làm tiếp đi” approved the published candidate design and continuation. **That actual design consent is preserved.** It did not supply missing protected file-owner handoffs, numeric namespace/codec allocations, provider/profile qualification, CP2 implementation approval or runtime acceptance. Source, build, signing and image changes subsequently occurred, but their former completion/acceptance claims are withdrawn. *(2026-10-20 state, historical: Phase 01 incomplete, Phases 02–09 incomplete/blocked, Phase 10 blocked and unqualified, artifact rollout blocked. **Update 2026-10-29:** Phases 01–08 are `completed-private-evidence`, Phase 09 `closed-partial-private-evidence`, Phase 04 `completed-private-evidence` and Phase 10 `closed-partial-private-evidence`; S10-5 promoted the qualified dev-posture desktop bytes and the user granted its CP2 on 2026-10-29. CBX-27 is unmet, CBX-23/24/25 partial, production/physical/VMX/remote gates remain blocked.)* [reports/rollout-containment.md](reports/rollout-containment.md) is the authoritative current withdrawal/containment record. `blockedBy: []` is not source or rollout admission.

This is a bounded Intel x86-64 operational-tooling workstream supporting named console/config, Cell observation, file/log inspection and guest/other-host access consumers. It does not replace ADR-0022's program or reopen new hardware/application ecosystems. IDEs, language-server/build environments, general app distribution, a wholesale Linux/Nushell port and inbound SSH-server development are excluded.

The implementation layout selected by this plan is one `/bin/cellbox` with a `sh` entrypoint for the migrated shell module. This avoids embedding the same command engine in a separate shell ELF. It preserves shell state semantics, not old paths/wrappers. Standalone apps/services remain standalone and can consume public terminal/TUI libraries independently.

## Entry gate and ownership

1. Phase 01 preparation was entered by the explicit implementation request, and user design consent was later recorded. The allocated contract/owner-handoff dossier remains incomplete; design consent must not be erased or expanded into those missing inputs, runtime acceptance or permission to roll out artifacts.
2. Reconcile current source and exact approval evidence: portfolio/current-focus retain both freeze text and later completion reports. Do not infer missing source from old checkboxes or blanket permission from a completion heading.
3. Name one integrating owner for shell/startup identity, API/SDK, kernel launch/telemetry, VFS and input/session changes. Adopt any overlapping unfinished authority/runtime prerequisite as a named controlling-plan slice; no circular dependency on running a locked predecessor.
4. Gate each concrete Law-1 delta separately: **CP1 design approval** records the user's candidate-design consent, while full protected-source admission still requires its exact digest-bound owner, codec/allocation and retained-authority dossier. That dossier remains incomplete; neither a Pending/Blocked label nor design consent alone fills it. After separately admitted implementation in a disposable verification profile, record the exact diff, consumer migration census and runtime evidence and obtain **CP2 implementation approval before final caller/image retirement or rollout for that delta**. CP2 is pending/not eligible on current evidence. P10 audits every delta's CP2 closure. Retained frozen ABI, protected identity, signing and physical/remote/production gates remain independent; no approval-by-agent or blanket root/union rights.
5. Update portfolio and only affected lock relations for the selected slice with a file-owner handoff. WIP stays one implementation slice; independent preparatory research/review may run concurrently.

## Phases and dependencies

| Phase | Deliverable | Depends on | State |
|---|---|---|---|
| 01 | [Catalog, baseline, upstream/runtime and exact contract gates](phase-01-baseline-contracts.md) | — | completed-private-evidence (2026-10-26): criteria C1–C4 checked; catalog frozen `f2a82fce…9339`; every implemented delta has CP1 and CP2 ([delta index](contract-deltas.md)), the last being the Q Config write right (CP2 2026-10-26, P04 466); CBX-01/02 met; [closure dossier](phase-01-closure-dossier.md) §7. Phase 10 still audits; rollout blocked |
| 02 | [Public terminal SDK and real child/session streams](phase-02-terminal-sdk-streams.md) | 01 | completed-private-evidence: CP2 approved by the user 2026-10-25; [private signed-x86 evidence](reports/phase-02-runtime-evidence.md), latest P02 48; rollout blocked |
| 03 | [Typed command data, bounded pipelines and explicit conversions](phase-03-typed-pipelines.md) | 01,02 | completed (CP2 approved 2026-10-21; private-image evidence) |
| 04 | [Single cellbox artifact and complete caller/image cutover](phase-04-multicall-cutover.md) | 01,02,03 | **completed-private-evidence (2026-10-29; S10-5 CP2 granted by the user):** criteria 1–6 checked on private QEMU/TCG fixtures; criterion 4 (canonical one-artifact packaging, including `/bin/cellbox-ssh`) checked on the promoted dev-posture x86 bytes only (hash-identical to candidate `4fae8b20…`; [S10-5.13](phase-10-s0-proposal.md)); non-Intel recipes not covered. Every Phase 04 delta is CP2-approved: S3-output, S3-gui, OOM, S2-K, S2-N, S2-T/W, S4a and C5 on 2026-10-22; S1, S2-lite, S2c-min and the S3 launch edge on 2026-10-25 ([mini-dossier](phase-04-s1-s3-cp2-dossier.md)) ([closure review](phase-04-closure-review.md) §6) |
| 05 | [Ratatui backend and complete GUI-terminal emulation](phase-05-ratatui-terminal.md) | 02,04 | completed-private-evidence: CP2 approved 2026-10-23 for all six slices ([dossier](phase-05-cp2-dossier.md) §16); rollout blocked |
| 06 | [Cell-centric telemetry/top, task drill-down and honest metrics](phase-06-cell-top.md) | 03,04,05 | completed-private-evidence: CP2 approved 2026-10-24 for all nine items ([dossier](phase-06-cp2-dossier.md) §20); rollout blocked |
| 07 | [Ripgrep-based VFS search and direct result records](phase-07-ripgrep-search.md) | 03,04 | completed-private-evidence: every slice S7-0..S7-4 CP2-approved 2026-10-24; all four success criteria checked (link no-follow/cycle accepted on host-fake evidence by user decision, no link-kind backend); private runs terminal 449, P03 67, P02 48 + rg Ctrl-C 130, kernel lane PASS; rollout blocked |
| 08 | [Compact editor and verified save semantics](phase-08-compact-editor.md) | 04,05 | completed-private-evidence: CP1 2026-10-24 (D78-1..3); every slice S8-1..S8-5 CP2-approved (S8-5 on 2026-10-25); all four success criteria checked (fault paths on VFS self-test/host evidence by approved deviation); private runs terminal 459 (10 `edit-*`), GUI 290 (EDIT-0..5), P02 48, kernel 39/39 + 7/7 VFS self-tests, P03 67; rollout blocked |
| 09 | [Secure outbound SSH and guest/other-host interoperability](phase-09-ssh-client.md) | 02,04 | closed-partial-private-evidence (user, 2026-10-25): S9-1..S9-4c and S9-6 CP2-approved; the 128-byte line drop is fixed (4096-byte limit, longer lines refused whole); Phase 09 lane 103/103 twice, P04 459, P02 48. Criteria 3 and 4 are met. Criteria 1 and 2 are partial: the Tier-3 guest is blocked, and S9-5 publickey/KMS custody is deferred to the roadmap (plan-portfolio, KMS protected persistence; D79-14). Rollout blocked |
| 10 | [Integrated signed-image qualification and measured footprint](phase-10-qualification.md) | 01,02,03,04,05,06,07,08,09 | **closed-partial-private-evidence (2026-10-30; S10-9 CP2 and CBX-27 disposition granted by the user; S10-5 CP2 2026-10-29): criteria 2, 3, 4, 5 checked; criterion 1 open (CBX-23/24/25 partial).** Criterion 4 and CBX-27 are met **at their stated ceiling by explicit user disposition, not as a feature-equivalent comparison** (S10-4..S10-9: static footprint, untraced timing, candidate class-A full-schedule memory on test-hooks measurement kernels, non-equivalent B2 shared-subset baseline; no RSS beyond mapped-backed, no promoted-kernel timing, 10 s Ctrl-C recovery not evaluated). S10-3a/b/c/d and S10-3 CP2 approved 2026-10-28, candidate 4fae8b20… qualified. **S10-5 CP1 and CP2 APPROVED by the user 2026-10-29 (DS5-1..DS5-4 as recommended; deviations 1–8 and the new `command-catalog.md` digest `f06f38ce…` acknowledged); executed, P0–P9 and V1–V7 PASS ([S10-5.13](phase-10-s0-proposal.md)):** durable candidate copy, hash-recorded quarantine archive, S10-2b (G1, G3 by image content plus ELF binding, canonical assembler route re-opened; G2b kept forever), hash-verified copy of the candidate's desktop `kernel_fs.img` `3848cefb…`, `init` `c0bd1458…` and kernel `dfbe8c55…`, ISO via the reopened G3, P04 terminal default mode 494/494, L6 once 0 regressions (39 pass, 1 pre-existing xHCI fail, 4 skip, 1 ignored). Headless-profile lanes do not cover the promoted desktop bytes. Dev posture, Intel x86 QEMU/TCG only; no physical, VMX, production, remote or CI claim; no cloud formats; non-Intel recipes refused |

Each phase requires real target smoke proof; no such proof is established by the withdrawn completion claims. Phase 10 is integrated qualification, not the first runtime exercise. Phases 06–09 have partly independent prerequisites, but shared-source execution remains single-owner/single-slice. Do not pad the DAG with unneeded runtime/platform projects.

## Required outcomes and decision coverage

[acceptance.md](acceptance.md) owns CBX-01..CBX-29. The rows recorded there as met are on private evidence only; all others are pending. No excluded/blocked feature may be called complete or silently narrowed.

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

## Execution evidence — 2026-10-20

- Saved [command compatibility/right matrix](command-catalog.md), [digest-bound contract candidates](contract-deltas.md), [primary-source dependency review](dependency-review.md), [source/test baseline](baseline.md) and [disposable fixture design](runtime-fixtures.md). Existing candidate [engine semantics](spec.md) and [launch/session layouts](abi-session-launch.md) are reused; [VFS/trust/telemetry candidates](abi-vfs-telemetry.md) complete the semantic package. Candidate publication is not CP1 or a CBX pass.
- Existing host command-core baseline: `cargo test --offline --locked -p text-engine --target x86_64-unknown-linux-gnu` — 38 tests passed, zero failures, retained from the earlier run; not rerun for candidate documentation. No full-workspace, target-build or runtime qualification claim.
- The interactive preparation continuation selected completion of reachable Phase 01 preparation. Candidate behavior, caller/right dispositions, document digests, upstream review pins and safe fixture instructions were prepared. The subsequent user statement “tôi duyệt, làm tiếp đi” records candidate-design consent. Remaining admission inputs are accountable controlling/file-owner handoffs, transport/codec allocations, dependency acquisition/probe admission and resolved audit/target proof, authentic transactional/trust/telemetry providers, and a reproducible signed execution profile. The full CP1 admission dossier remains incomplete; CP2 is pending/not eligible.
- At that preparation stage, no source/ABI, image/signing, key, service exposure or remote/production change had been made. This is a historical stage boundary, not a claim about later work: subsequent source, signing and image changes did occur. No source-owner handoff or legacy lock release follows from the preparation record.

- **2026-10-20 — Acceptance withdrawn; rollout blocked.** Source changes created `terminal-sdk` (`libs/terminal`), `tui-cellos` (`libs/tui-cellos`), pipeline/editor/search/telemetry code in `libs/text-engine` and `app-cellbox` in `cells/tools/cellbox`; build/sign/package operations produced candidate bytes in `kernel_fs.img`. Historical host results include 14 terminal-sdk and 3 tui-cellos tests, text-engine snapshots of 42/43/46/51 tests, target/PIC compilation, F1/F5 checks and signing/package observations. These are development/artifact evidence only, not additive independent acceptance totals. The recorded cellbox size is 88,576 bytes and terminal size 595,032 bytes; neither proves startup, resident memory or stream bounds. Review found the shell exits, the GUI command matcher remains, SSH/editor/transforms are no-op paths, selected Ratatui/ripgrep/Sunset imports are absent, telemetry identities are fabricated, stream authentication/cancellation is flawed and editor data safety is not established. No QEMU/SSH/editor integration proof or CP2 supports closure. All 29 CBX rows are pending; all ten completion claims are withdrawn. See [the containment record](reports/rollout-containment.md); existing candidate artifacts are not qualified or authorized for rollout.


**Plan path:** `/home/dmin/cellos/.agents/261020-1750-cellbox-cli/plan.md`  
**Current handoff:** The user selected **“Giao slice Phase 02”**, assigning the current assistant bounded D01 child-context/D02 stream/session integration through the controlling owner. The [allocation/source/private-fixture dossier](phase-02-source-admission.md) must be reviewed and its exact digest approved before protected edits or build/sign/image/boot effects. This is no longer a containment-only preparation scope, but it is not completed source admission, acceptance or permission to promote artifacts. Existing rollout/CP2 blocks and unrelated locks remain.

### KSSH K6 bounded slice (2026-10-30)

S9-5 via KSSH K6 is admitted as a bounded Cellbox slice by user approval ([record](../261030-1000-kms-ssh-key-custody/k6-cp1-approval.md)): client, vissh and cellbox/terminal edits listed in its CP1 section 4, host tests only, no re-opening of closed phases; CBX-24 stays partial until K3/K5/K7/K8 evidence.

### KSSH K8 T0 lane (2026-10-30)

User approved [K8 CP1](../261030-1000-kms-ssh-key-custody/k8-cp1-approval.md): private lane edits and one RT0 plus one RT0f QEMU/TCG evidence boot (no promotion, no rerun, no T1). CBX-24 stays partial until the user's explicit disposition after K8 CP2.
