# Phase01 private benchmark capture contract

This is private bench/CI output, not `libs/api` or a public IPC ABI. Source execution authorized by the owner's “tiếp tục”; no physical or production lane activated.

## Ownership
- Guest owner: `cells/tests/bench/**` only; owns private runner failure result and scenario correctness/cleanup. No SDK/kernel/public ABI writes.
- Host owner: `scripts/compare-bench-results.sh`, a cohesive Python implementation beside it if needed, focused behavior regression tests, `.github/workflows/perf.yml`. Owns raw QEMU capture, serialization/provenance, validation, target/history verdicts.
- Main: documentation/plan integration and settled-source build/runtime verification coordination. No shared agent writes.

## Profile and guest records
Profile ID: `rv64-qemu-virt-2h-256m-v2`. QEMU virt, RV64, TCG, 2 harts, 256 MiB. Preserve existing targets and report structs. Guest starts with exactly one JSON `{"bench_event":"start","profile":"rv64-qemu-virt-2h-256m-v2"}` and ends with exactly one `{"bench_event":"complete","profile":"rv64-qemu-virt-2h-256m-v2","invalid":N}`. An experiment failure emits `{"bench_event":"invalid","scenario":"NAME","stage":"STAGE"}` and contributes to N; it is never a measurement. Complete marker after every required attempt and cleanup, not proof of target success. Host retains every raw line, rejects panic/producer failure/missing records even with a marker.

Latency records retain `name,n,min,p50,p99,max`; RT adds `p999,jitter,miss`. Profile declares units; no public BenchReport change. Distinct under-load names avoid duplicate metrics. Memory record is `name=memory_footprint,n=1,bytes=VALUE` (no invented latency). SMP records use `name,n,value` with profile-declared units/directions. Guest target text remains useful but host derives target verdicts from validated records.

| Required name | Successful n | Warmup | Unit / kind |
|---|---:|---:|---|
| context_switch | 1000 | 100 | ns / latency |
| ipc_send_recv | 1000 | 100 | ns / latency; hardware target informational |
| syscall_yield | 1000 | 100 | ns / latency |
| memory_footprint | 1 | 0 | bytes / footprint |
| preempt_latency | 500 | 50 | ns / RT |
| control_loop | 200 | 0 | ns / RT |
| ipc_send_recv_idle | 1000 | 100 | ns / latency |
| syscall_yield_idle | 1000 | 100 | ns / latency |
| ipc_send_recv_load | 1000 | 100 | ns / latency |
| syscall_yield_load | 1000 | 100 | ns / latency |
| smp_spawn_rate | 8 | 0 | operations/sec / higher-is-better |
| smp_ipc_throughput | 1000 | 0 | operations/sec / higher-is-better |
| smp_work_distribution | 2 | 0 | scale_x100 / higher-is-better |
| stage_encode_request_x1000 | 10 | 2 | ns per 1000 operations / latency |
| stage_decode_reply_x1000 | 10 | 2 | ns per 1000 operations / latency |
| stage_ecall_roundtrip_x1000 | 10 | 2 | ns per 1000 operations / latency |
| total_typed_roundtrip_x1000 | 10 | 2 | ns per 1000 operations / latency |

Source owner must verify SMP spawn count against its current constant before implementation and tell Main/host owner if different; do not silently change experiment counts. All load-cell admissions and relevant teardown outcomes must be checked. No public receive-length field invention: use available syscall metadata and payload handling; impossible exact checks must be reported, not fabricated.

## Host behavior
One capture has immutable ID (CI run+attempt+profile+repetition, or collision-resistant local ID), UTC capture ordering, source/build inputs and hashes of kernel/bench/probe/disk/ramdisk, toolchain/features and actual QEMU command/version/machine/harts/RAM. Source/artifact hashes are provenance, not a prohibition on comparing revisions. Compatibility binds experimental profile/environment, units/counts and semantics; incompatible/legacy history is non-comparable.

Current capture selected explicitly, not whichever old filename sorts latest. Invalid current input fails regardless of valid history, without advancing/resetting streak. Conflicting duplicate IDs fail. Serialize JSON through Python; retain raw log and producer status on all exits. Intentional termination after a complete valid run is distinct from timeout-before-completion or unexpected producer exit. Validity, target and historical verdict are separate. Preserve 20-prior-run median and >10% for three distinct consecutive compatible valid runs; deterministic history replay repairs missing/corrupt state, idempotently and failure-atomically. New profile with no history is BASELINE_ONLY; insufficient existing-profile reconstruction is INVALID/BLOCKED. Target misses remain failure without marking valid measurements invalid.
