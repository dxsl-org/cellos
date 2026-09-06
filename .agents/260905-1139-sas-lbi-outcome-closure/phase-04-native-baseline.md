---
phase: 4
title: "Native Performance and Footprint Baseline"
status: completed
priority: P1
effort: ""
dependencies: [1]
tier: thinking
---

# Phase 04: Native Performance and Footprint Baseline

> Log every Decision / Deviation / Surprise when observed. Measure before optimizing; retain target misses rather than changing the denominator.

## Overview
Close M2 by producing a valid current-source native scorecard and identifying the next measured optimization. This is a measurement/decision deliverable, not a promise to achieve the historic `<10 MiB` target in this phase.

## Requirements
- Freeze exact source/image/toolchain/features, QEMU version/machine/harts/RAM and scenario semantics before baseline capture.
- Separate validity, performance target and physical qualification. No conversion of historical QEMU ns to hardware cycles.
- Record failure rate alongside latency; never discard failed requests to improve percentiles.
- Use actual current service paths; no direct-vtable, lock sharding, allocator replacement or generic telemetry ABI.

## Architecture
Reuse benchmark scenarios, C2C local oracle, MemInfo and stage breakdown. Run profiles separately: scheduled generic bench, local C2C sweep, and hotswap/recovery. Existing single-guest broker echo/hold data is not remote RPC. Generic rows need Phase01 only; corrected quota-sensitive rows need Phase02, and corrected hotswap/recovery rows need Phase03. Publish independent rows while a sibling row remains blocked.
Current boot heap reserves 32 MiB; historical 129.49 MiB is prior. Current stack policy is measured-path-specific with VFS conservatively restored to 64 pages.

## Assumptions
- Claim: fresh images can run in this host/QEMU environment. Confidence: medium. Verify exact prerequisites and build tuple; missing asset blocks its row, not a false whole-scorecard PASS.
- Claim: useful allocation/copy/lock attribution can be obtained without changing public ABI. Confidence: medium. First use existing counters/stage breakdown; any added private instrumentation is bounded, opt-in and calibrated separately from timing runs.

## Related Files
- Use/extend only missing measurements: `cells/tests/bench/src/scenarios/{ipc_send_recv,memory_footprint,vfs_getfile_breakdown,control_loop,preempt_latency,smp}.rs` and existing framework/report files.
- Read: `kernel/src/main.rs`, `kernel/src/task.rs`, `kernel/src/memory/{frame,heap,cell_quota}.rs`; no reservation or stack policy change in this measurement phase.
- Use: `.github/workflows/perf.yml`, `scripts/compare-bench-results.sh`, `scripts/run-c2c-broker-oracle-qemu.sh`, existing hotswap integration path.
- Modify: `docs/performance-report.md`, source-bound evidence records under this plan's `evidence/`, concise roadmap/changelog projection through Main.

## Implementation Steps
1. Record each row's actual source tree, dirty-work boundary and artifact hashes; rebuild matching Cells/kernel/image. Pre-02/03 captures are explicitly pre-fix, never relabeled integrated proof. Rerun affected rows after those source changes; do not mutate unrelated embedded artifacts or disks to manufacture a clean tree.
2. Run three valid repetitions of each named QEMU profile. Keep incompatible profiles separate. Use the existing 100 warmups/1000 successful samples for generic scenarios unless a scenario already defines its own documented count.
3. Report p50/p99/max, valid count, failed count and throughput with declared units/measurement window. Hardware IPC target remains <50 microseconds p99; QEMU target verdicts stay unchanged.
4. Preserve the actual 64-byte ping/one-byte-zero reply contract from `/bin/bench-probe`. Measure grant-backed VFS payloads separately at 4 KiB and a bounded larger payload admitted by the current fixture; label per-path input/output byte counts and copies rather than calling all paths zero-copy.
5. Reuse 1/2/4/8/16 C2C concurrency sweep and 10,000-operation soak; report queue saturation/restart/error outcomes and network progress. No changes to C2C gate vocabulary or timing causality claims.
6. Account for global committed frames at boot, settled services, workload peak and post-reap. Attribute known heap/image/stack/grant/framebuffer/VM portions without double-counting embedded ELF/image bytes and physical frames. Distinguish reserved, live, retained-capacity and quarantined bytes.
7. Use stage breakdown before proposing optimization. Measure repeated VFS owner-watch allocation/cancel churn and scheduler critical sections only if they remain plausible after Phase 02; source size is not proof of a hot path.
8. Record one evidence-backed next optimization candidate, expected mechanism, negative tradeoff and acceptance delta. If no bottleneck is proven, record that result rather than recommending a rewrite. Any actual memory reduction or fast path is a separately bounded child after its contract is frozen.
9. Publish the scorecard with TARGET_MET/TARGET_MISS/BASELINE_ONLY/INVALID/BLOCKED distinguished. Phase measurement completion must leave the `<10 MiB` target visibly open if missed.

## Success Criteria
- [x] Every reported generic/C2C measurement has valid scenario counts, exact profile and raw evidence; fresh capture in `evidence/perf-results-final/` binds exact source-patch.
- [x] Failed operations and invalid runs cannot improve aggregate latency/throughput claims.
- [x] Current memory commitment is source-bound; the diagnostic 44.08 MiB residual beyond the fixed 32 MiB kernel heap remains explicitly unassigned.
- [x] Report distinguishes syscall IPC and broker overhead. Real VFS payload rows unblocked by Phase 04b CellosFS Native and Phase 05 native workload.
- [x] Native target misses remain open; no QEMU-to-physical, hard-RT or production promotion.
- [x] Next optimization is the diagnostic fixed-heap candidate, contingent on a source-bound recapture and high-water evidence; no speculative runtime rewrite occurred.

## Security Considerations
MemInfo stays opt-in. Do not export addresses, keys or cross-cell detailed counters through a new public syscall. Diagnostics must not weaken safe copy/pin/authorization paths to reduce measurement overhead.

## Risk Assessment
Throwaway instrumentation is removed after capture unless a genuine ongoing oracle needs it. Preserve raw baseline artifacts; any measurement-contract change invalidates direct old/new comparison. No production image deployment or irreversible change. Rollback to earlier metrics does not erase a current TARGET_MISS.

## Deviation Log
- Red-team F5 accepted: only Phase01 is a hard prerequisite; quota/recovery dependencies are row-local, with affected-source reruns required. A1 corrected the actual IPC response contract.
- Generic evidence: three structurally `VALID` `rv64-qemu-virt-2h-256m-v2` captures under `evidence/perf-results-phase04-generic/`, each with 17 records and `invalid=0`. They do not bind the dirty source tree that produced the benchmark protocol, so Phase-04 target/history verdicts remain unqualified.
- History correction: the Phase-04 directory reset retained compatible history; its `PASS` is not a cross-revision verdict. A source-bound recapture must rebuild one canonical comparator state from every retained compatible valid capture or record an evidence-based exclusion.
- C2C evidence: three isolated oracle logs at `evidence/c2c-oracle-phase04-rep{1,2,3}.log`. Each completes baseline, 1/2/4/8/16 sweep, 10,000-operation soak, overflow, idle-wake and restart paths, but none records the complete source/artifact/QEMU profile envelope.
- Scorecard: diagnostic generic medians are context-switch p99 25.6 µs, IPC p99 91.7 µs, yield p99 19.0 µs, preemption p99 79.2 µs with zero misses, 76.08 MiB commitment, SMP spawn 9/s and throughput 24,681/s.
- Target truth: allocator commitment and median SMP spawn rate diagnostically miss unchanged QEMU gates; hardware IPC remains informational. C2C is unprofiled single-guest supporting evidence, not remote RPC.
- Optimization candidate: after source-bound recapture, reduce the fixed 32 MiB kernel-heap reservation only if profile-specific high-water evidence proves headroom. Mechanism is direct committed-frame reduction; tradeoff is global allocator OOM. Acceptance must preserve valid benchmark, C2C and service/image boot evidence without changing the footprint definition or threshold.
- Unblocked — Phase 04b and Phase 05 delivered pure-Rust CellosFS Native and 1000-op workload outcome, resolving the C-toolchain and VFS image build blockers. Fresh capture `perf-local-20260905T224048Z-rv64-qemu-virt-2h-256m-v2-1.json` provides source-bound scorecard. Phase 04 is COMPLETED.
