---
phase: 1
title: "Catalog, source baseline and exact contract gates"
status: pending
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

Create phase artifacts in this plan directory: `command-catalog.md`, `contract-deltas.md`, `dependency-review.md` and `baseline.md`. These are future implementation-entry outputs, not missing pieces of the current planning deliverable. No source or ABI edits are required merely to produce the inventory.

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

- [ ] Every current suite name/caller has a disposition and acceptance scenario; no silent feature deletion or fake rg substitution.
- [ ] Each delta has exact scope/digest/owner and a CP1 state; downstream ABI editing remains blocked until that delta's CP1 is explicitly approved, and its final cutover remains blocked until CP2 is approved after implementation evidence.
- [ ] Ratatui is still selected; upstream port feasibility and SSH library choice are backed by exact evidence or an exact blocker.
- [ ] No physical/remote/production gate or legacy lock is released by a completion label.

## Assumptions

Exact upstream versions/features, VFS replacement guarantees and admitted runtime dependencies are unverified. Verify manifests/source and scoped target probes; a repository README or host build alone is insufficient.

## Security and risk assessment

No downloads/installations/exposure/key generation without scoped execution approval. Untrusted dependency code and input data do not grant rights. Reverse this phase by superseding candidate documents; it must not alter persistent state. Later ABI ratification cannot be undone by deleting an inventory.

## Deviation log

None; implementation has not started. Record each future Decision/Deviation/Surprise with why, impact and revert/escalation at occurrence.
