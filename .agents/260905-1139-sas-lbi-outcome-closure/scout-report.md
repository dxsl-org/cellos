# SAS/LBI Outcome Closure — Scout Report

## Project Type and Approval
- Rust bare-metal, multi-architecture cellular SAS/LBI OS; Tier 1 trusted shared SAS, experimental RV64 Tier-2 substrate, Tier-3 Linux guests.
- User approved Approach A after the in-session architecture/root-cause brainstorm: strengthen current contracts and measurements before workload-led expansion.
- Research and four scout reports from this session reused; no new broad scout or implementation performed.
- Current source and maintained roadmap supersede historical plans. Approval does not open frozen ABI, production, sensor, remote or hardware gates.

## Evidence Provenance
- OBSERVED in preceding investigation: existing comparator returned exit 0 for empty, malformed and missing-IPC latest measurements even with a valid historical record; real sustained-regression control returned exit 1. Temporary inputs removed.
- OBSERVED then: cargo metadata counted 120 workspace members; existing metric generator rendered kernel/core nLOC 34,907/29,581 without writing docs. Moving counts, not release evidence or full TCB size.
- SOURCE: main.rs:569-589 reserves 8192 frames (32 MiB) for heap; historical 129.49 MiB report is not a fresh measurement.
- SOURCE: benchmark runner discards operation errors; source scan/signature/native trust are different boundaries.
- PRIOR runtime PASS counts in roadmap remain prior evidence. No new boot, hardware run, quota or hot-swap reproduction occurred during planning.
- REVIEW CORRECTION: generic IPC spawns bench-probe (`ipc_send_recv.rs:13,25-26`), whose actual reply is `[0]` (`bench-probe.rs:44-51`); the main dispatcher's empty reply is not this scenario's peer.
- REVIEW SOURCE FACTS: existing VFS restart request is hypervisor-only with hypervisor-min init wiring. Hotswap requester checks use forgeable display names; stash state has no transaction/source-owner binding. These are source-inspected paths, not executed attacks; plan Phase03/05 own their exact authority/fixture gates.

## Relevant Files and Patterns
- `scripts/compare-bench-results.sh`, `.github/workflows/perf.yml`: history parser, error handling, collection, artifact and verdict boundaries.
- `cells/tests/bench/src/framework/{runner,report}.rs`, `main.rs`, `scenarios/*`: error propagation and required workload inventory. Preserve public `api::benchmark` contracts.
- `scripts/cellos_sign/policy.py:20-31` correctly limits forbid to crate and identifies libs as TCB; `docs/specs/16-rustc-tcb.md:38-46` incorrectly describes dependency-tree coverage.
- `kernel/src/memory/heap.rs:18-34`: current-context charge/refund. `kernel/src/task/scheduler.rs:137-143,309-346,1030-1067`: cross-lifecycle bookkeeping. Quota defect remains a hypothesis until reproduced.
- Existing allocation/drop attribution pattern: `kernel/src/task/ipc_wire.rs:43-56,97-104`; pending mailbox has receiver-owned accounting. Preserve those semantics.
- `libs/ostd/src/grant.rs`: into_raw detaches Rust Drop, not kernel ownership. `GrantFree` checks owner TID, not merely same CellId; GrantShare is access, not transfer.
- Real into_raw caller: `cells/tests/vfs-test/src/grant_io.rs:23-26` rewraps locally in GrantRegion; do not delete this correct use. Other owners: shell commands and hypervisor VirtIO block alloc_copy_from_slice.
- `cells/services/supervisor/src/hotswap.rs`: optional SnapshotTimeout then atomic commit; both demo Restore handlers call ready even after restore error.
- `cells/tests/bench/src/scenarios/hotswap_{cli_probe,supervisor}.rs`, `tests/integration/tests/hotswap-smoke.rs`: existing counter, FIFO and old-TID runtime witnesses. Host harness early-returns on missing prerequisites; cannot accept skipped boot as proof.
- `libs/ostd/src/fs.rs`: existing file/grant APIs; use current VFS authority rather than inventing generic filesystem interface.
- `docs/code-standards.md:40-46`: libs/api and libs/types require exact design approval before edits and implementation approval after exact delta/evidence review. This plan avoids both directories.

## Precedents
- `41412b62 feat(hotswap): make supervisor cutover atomic` touched supervisor, kernel cell/service_registry, task/tcb/syscall, ostd wrappers, bench witness and hotswap integration. Preserve atomic ingress/FIFO semantics; timeout-only patch is insufficient.
- `1a7748b6 fix(drivers): preserve registration ownership` touched task retirement, driver/IOMMU/BDF publication, registries and integration tests. Recent work must not be reverted by quota/lifetime edits.
- `98ab00b3 fix(ci): document benchmark log schema` touched C2C benchmark report. Generic perf parser must not conflate or rewrite C2C oracle semantics.
- No `.agents/failure-history.jsonl` or `.agents/incidents/` files found; failure-ledger read-back skipped, not assumed clean history.

## Cross-Plan Relationships
- `260731-1930-capacity-observability` and `260902-solo-maintenance-slices` completed MemInfo/OOM/free delivery. Do not reopen or silently change their metric.
- `260806-1026-midori-reactor-stack-closure` completed reactor/stack work; current VFS conservative stack supersedes old six-path sizing summary. Do not replay blanket stack shrink.
- `260827-1004-hardware-independent-roadmap` remains in progress. Its Phase 08 owns shared roadmap projection; its security children retain governance. This is shared-file coordination, NOT whole-plan blockedBy dependency.
- No new causal whole-plan dependency found. Keep blockedBy/blocks empty rather than serializing unrelated lanes or rewriting historical plans.

## Blast Radius and Invariants
- Main integration owner serializes roadmap/changelog edits and any shared kernel source boundary; phase-local source owners handle independent files.
- No libs/api/types, syscall IDs, allowlist-bit allocations, Manifest-v3, KMS ABI, public native-domain route or production-key changes.
- Preserve default-deny opt-in MemInfo, exact owner/generation retirement, grant/pin quarantine and IOMMU acknowledgement ordering.
- No physical, production, remote or hard-RT claims from QEMU/host output. Keep raw evidence bound to exact source/build/features/machine.

## Tool and Environment Assumptions
- LSP status initially resolved rust-analyzer launcher, but references request exited: binary unavailable in pinned nightly-2026-05-01. Targeted grep fallback used; retry LSP before exported-symbol edits when available.
- Integration tests are a standalone crate; run with explicit Linux host target, not root default bare-metal target.
- Current QEMU/image buildability, usable RedoxFS scratch path, and workload latency/resource bounds require pre-Build verification; not established by source existence.

## Lab and Organizational Profile Extension — 2026-09-05
- Owner-selected LAB-01 dry carrier transfer, BASE-01 tray handoff and ASSEMBLY-01 stationary integration; robot hardware remains on paper. Main preserves original phases 01–05 and all ABI/hardware/production gates.
- Two writing agents inspected bench scenario/role dispatch, OSTD VFS, hotswap and RedoxFS integration fixtures before drafting phases 06–08; no build, test or physical run occurred in this extension.
- Existing `cells/demos/robot-demo/src/main.rs` includes synthetic sensor fallback and unchecked GPIO results; its marker is not a physical workflow oracle. The periodic `control_loop.rs` bench is jitter evidence, not base stability.
- Proposed private workflow/oracle paths are explicitly proposals, not existing robot drivers or qualified hardware. Phase05's real backend and authority controls remain prerequisites for native composition.
- Tier2 status is implemented RV64/QEMU substrate and cross-hart migration with physical containment/DMA quarantine/production gates open; app-guide projection now matches the roadmap.
- G2 scope is ORG-SRV-01 organizational web/app/microservice servers and ORG-PC-01 ordinary office PCs, not specialist machines. Reference applications are compatibility targets, not observed Cellos support.
- Precedent `0a43e9b9` changed roadmap, current-focus, risk-register and product-stage projections together; `git show --stat` confirmed that footprint. This extension follows current projection ownership without rewriting the immutable legacy roadmap.
