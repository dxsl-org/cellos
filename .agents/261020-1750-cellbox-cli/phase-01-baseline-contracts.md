---
phase: 1
title: "Catalog, source baseline and exact contract gates"
status: completed-private-evidence
priority: P1
tier: thinking
dependencies: []
---
# Phase 01: Catalog and entry contracts

## Overview and entry

Convert the accepted design into exact implementation contracts without widening product scope. The [plan entry gate](plan.md#entry-gate-and-ownership) applies; this file is not independent permission to implement.

## Requirements

CBX-01/02: preserve every current command consumer, pin dependencies/licenses and bind changed authority/ABI surfaces to named owners and retained approvals. Freeze candidate contracts before downstream source changes.

## Architecture and related files

Read `cells/tools/shell`, `cells/tools/{sys-tools,net-tools}`, `cells/apps/terminal`, `libs/{text-engine,ostd,api,cell-build}`, `kernel/src/loader/launch_profile`, current telemetry/VFS/input providers, build/sign/image scripts and actual integration runners. Retain the [source recon](scout-report.md) as observed evidence, not a substitute for phase-entry source inspection.

Preparation artifacts now include the [command compatibility/right matrix](command-catalog.md), [candidate digest/approval ledger](contract-deltas.md), [primary-source dependency review](dependency-review.md), [baseline](baseline.md), [engine specification](spec.md), [launch/session candidate](abi-session-launch.md), [VFS/trust/telemetry candidate](abi-vfs-telemetry.md) and [runtime fixtures](runtime-fixtures.md). These supply reviewable preparation, not approved ABI encodings, upstream target qualification or source admission.

## Implementation steps

1. Record toolchain, source/artifact identity and unexpected user edits without resetting them. Reconcile current source with historical completion/freeze projections and exact file ownership.
2. Enumerate all builtin/special command names, actual options/status/script behavior, standalone targets, completion/help entries, launch paths/identities, init/GUI callers and image/signing lists. Inventory every desktop-exposed backend operation in the selected tooling scope and its corresponding CLI path/shared typed operation, including any missing CLI consumer. Assign each existing command to shared implementation, shell state, independent service/worker or explicitly preserved specialized tool. No automatic removal based on a family summary.
3. Freeze essential command catalog, supported search flags/ignore/encoding policy, editor key/save/conflict behavior (including pre-existing or concurrently created Save As destinations), telemetry schemas/CPU units, SSH required behavior and shell script compatibility. Build an authenticated caller-edge × backend-right matrix for every catalog command: file read/mutate, lifecycle/device, network egress/DNS, cross-Cell telemetry, credentials/key/trust storage and all other required rights. Name the enforcing service handle/worker for each allowed right and denial scenarios for known commands under restricted callers. Choose bounded capacities from current limits and consumer needs, not performance promises.
4. Specify typed Value/record/bytes/session contracts; exact filter syntax, redirects, status/diagnostic channels, non-terminal encoding, cancellation/backpressure and argv/cwd/environment handoff. Freeze standalone versus pipeline stateful-builtin context, whole-pipeline job identity, foreground transfer, status selection, wait/reap and cancellation. Preserve current support; reject unsupported background/mixed/stateful pipeline forms before any stage has effects rather than adding unrequested full job-control breadth. No full Nushell grammar.
5. Specify terminal ownership/attach, foreground transitions, resize/raw/echo restoration and safe ANSI/VT/TERM profile. Endpoints are non-inheritable by default with explicit typed child grants, generation/lifetime checks, bounded duplication/delegation and last-authorized-writer EOF. Cancellation/consumer close revokes the operation's delegated endpoint subtree and wakes blocked holders without affecting unrelated sessions. Inventory existing pipe primitives and the exact owner-side prerequisites; avoid a second authority model.
6. Inventory proposed launch/identity, session/handle, VFS conditional create/replace and trust-state, and Cell-telemetry deltas. Record layout/version/rights/error/migration/concurrency consequences and per-delta specification digest. Obtain explicit CP1 before dependent ABI edits; pending/blocked CP1 keeps that scope blocked. Schedule exact diff/caller census/evidence and CP2 after implementation but before its final consumer/image retirement. Rebase affected locked work through its controlling owner; do not invent opcode bytes or infer approval from the ADR.
7. Pin Ratatui and required features, Kibi reference revision, ripgrep components and license choices. Evaluate Sunset first versus justified alternative SSH runtime, including entropy/key format/rekey/known-host support. Review per-file/transitive licenses, notices, advisories and compiler requirements. Use declared target dependency probes only after the scoped execution request; remove probes afterward.
8. Capture split-tool artifact/image footprint and representative behavior baseline with provenance. Define real smoke fixtures and private-image rules before mutation. Register one selected implementation slice in portfolio/owner records when authorized.

## Success criteria

- [x] Every current suite name/caller has a disposition and acceptance scenario; no silent feature deletion or fake rg substitution. Evidence: the user froze the [catalog](command-catalog.md) at `f2a82fce…9339` (2026-10-25), with rows for all 60 builtins plus `[`/`.`, the typed stages, the 17 tools and the as-built edges. The ledger is host-tested against `BUILTINS` ([closure dossier](phase-01-closure-dossier.md) §1, §7 C1-a..d, C1-g). `rg` is a ripgrep 15.2.0 adaptation (`libs/vigrep`). The `export` denial scenario is lane-proven: P04 466/466 with 6 config markers (C1-e, Config CP2 2026-10-26).
- [x] Each delta has exact scope/digest/owner and a CP1 state; downstream ABI editing remains blocked until that delta's CP1 is explicitly approved, and its final cutover remains blocked until CP2 is approved after implementation evidence. Evidence: the [reconciled delta index](contract-deltas.md) and closure dossier §2.1/§7 (C2-a..f). Every implemented delta has a CP1 and a CP2: P04 S1/S2-lite/S2c-min/S3-edge via the [mini-dossier](phase-04-s1-s3-cp2-dossier.md), and Q Config via [phase-01-config-right-cp1.md](phase-01-config-right-cp1.md). P07–P09 are hash-bound by addenda. The owner was confirmed by the user 2026-10-25. Open deltas without CP1 (D01 lifecycle owner, D04 clock units) stay refused or deferred.
- [x] Ratatui is still selected; upstream port feasibility and SSH library choice are backed by exact evidence or an exact blocker. Evidence: closure dossier §3 and §7 (C3-a..c). Ratatui `=0.1.0`/`=0.3.0` is pinned, ripgrep is adapted, and Sunset is vendored with PROVENANCE and patches 0001–0006 ([dependency-review.md](dependency-review.md) §4, §5 addenda). The scoped `cargo deny` reports advisories, bans, licenses and sources all ok, and CBX-02 is met.
- [x] No physical/remote/production gate or legacy lock is released by a completion label. Evidence: closure dossier §4. The rollout guards are in source, the canonical `kernel_fs.img`/ISO hashes are unchanged (also after the Config slice), all 38 lock relations remain, and Phase 10 and Phase 04 criterion 4 are open. [Rollout containment](reports/rollout-containment.md) is unchanged.

## Assumptions

Exact upstream versions/features, VFS replacement guarantees and admitted runtime dependencies are unverified. Verify manifests/source and scoped target probes; a repository README or host build alone is insufficient.

## Security and risk assessment

No downloads/installations/exposure/key generation without scoped execution approval. Untrusted dependency code and input data do not grant rights. Reverse this phase by superseding candidate documents; it must not alter persistent state. Later ABI ratification cannot be undone by deleting an inventory.

## Deviation log

2026-10-20 — Explicit `hc-cook --auto` request admitted preparation. Three independent read-only inventories and an offline/locked host command-core baseline were performed; 38 tests passed, zero failures. Exact caller-right matrix, frozen semantics, ABI specifications/digests/owner handoffs, upstream selection/license/advisory/target proof and matched signed footprint remain incomplete. Phase 01 is blocked, not complete; no downstream source phase, CP1/CP2, lock release, signing or exposure was inferred. Preparation integration owner: current assistant session; accountable approvals remain with the maintainer. See the four linked artifacts for evidence and limits.

2026-10-20 — Interactive continuation explicitly selected completion of reachable Phase 01 preparation. Exact command dispositions/right-denial matrix, engine/session and VFS/trust/telemetry semantic candidates with document digests, primary-source upstream review pins/licenses/runtime gaps, and disposable fixture design are prepared. Candidate capacities remain proposed admission bounds, not measured safe profiles. Accountable owner handoffs, selector/codec allocation and CP1, resolved dependency audit/target probes, real transactional/trust/telemetry providers and matched signed execution profile remain required. No source/ABI or runtime side effects were authorized or performed; Phase 01 remains blocked and CBX-01/02 remain pending.

2026-10-20 — **Correction: design approved; Phase 01 incomplete/blocked.** The user explicitly said “tôi duyệt, làm tiếp đi” for the published candidate contract/catalog package. Preserve that CP1 design consent. The earlier assertion that all contract criteria were complete and Phase 02 source admission followed automatically is withdrawn: exact protected owner/file handoffs, namespace/codec allocation annexes, provider/profile and dependency target qualification inputs remain missing. CP2 is pending/not eligible; no runtime acceptance or rollout permission follows. The earlier preparation-only no-side-effect entries describe their historical stage, not later source/build/sign/image changes. All success criteria remain unchecked and CBX-01/02 pending. [reports/rollout-containment.md](reports/rollout-containment.md) is the authoritative current withdrawal record.

2026-10-26 — **Phase 01 closed as `completed-private-evidence`.** The user approved the Q Config CP2 on 2026-10-26, closing the last gate (C1-e/C2-e). Earlier, on 2026-10-25, the user had approved the P04 S1/S2-lite/S2c-min/S3-edge CP2, frozen the catalog, confirmed the owner, approved the deny version fix (CBX-02 met) and approved the Config CP1. Criteria C1–C4 are checked with the evidence linked above; the gap list is in the [closure dossier](phase-01-closure-dossier.md) §7. CBX-01 is met for Phase 01, and Phase 10 still audits every delta's CP2 closure. Not released: rollout, canonical packaging (Phase 04 criterion 4, including `/bin/cellbox-ssh`), Phase 10, physical/remote/production gates and the legacy locks.
