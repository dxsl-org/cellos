# A3 Research — MemInfo and a real memory-footprint benchmark

**Date:** 2026-07-31  
**Scope:** planning only; no production files edited  
**Verdict:** A3 is mechanically small but is a stable-ABI change and exposes a metric-definition conflict. Do not edit `libs/api/` or `libs/types/` until the user has explicitly confirmed the exact ABI twice.

## Findings

1. The benchmark is a false green. `MemoryFootprintBench::run_once` assigns the compile-time `APPROX_BOOT_BYTES = 3_500_000` and returns it; the runner then compares that value to 10 MiB and prints PASS (`cells/tests/bench/src/scenarios/memory_footprint.rs:9`, `:52`; `cells/tests/bench/src/main.rs:257`). The repository itself records the measurement as estimated/unmeasured (`docs/performance-report.md:68`, `:72`, `:85`).
2. The frame allocator already has the right ownership boundary but no accounting. It stores `total_frames` and a used/free bitmap (`kernel/src/memory/frame.rs:31`), yet `used_memory()` is a stub returning zero (`kernel/src/memory/frame.rs:206`). Allocation and deallocation funnel through private `mark_used`/`mark_free` helpers (`kernel/src/memory/frame.rs:119`, `:141`, `:178`, `:184`), so an exact `used_frames` counter can be maintained without scanning the bitmap on every syscall.
3. A physical-frame reading will honestly fail the current 10 MiB target before normal services are counted. Boot eagerly allocates `HEAP_FRAMES = 4_096`, i.e. 16 MiB, from the frame allocator (`kernel/src/main.rs:450`). Those frames are marked used even when most heap bytes are idle. Conversely, the allocator starts in the largest `Usable` range and excludes the live kernel image range (`kernel/src/main.rs:272`; `kernel/src/boot/dtb_memory.rs:129`), so `allocated_frames * 4096` is neither the documented “kernel + 3 services” resident footprint nor the old 3.5 MB ELF-size estimate. This is useful capacity telemetry, but the PDR metric must be named honestly.
4. Existing per-task telemetry cannot substitute safely. `GetProcs2` exposes `heap_bytes` and `owned_bytes` (`libs/api/src/abi/syscall.rs:862`; `kernel/src/task/syscall.rs:599`), but heap usage is keyed by `cell_id` and copied into every task row while task stack/segments are per task (`kernel/src/task/syscall.rs:603`). Summing rows can double-count a multi-threaded cell, and it omits kernel/static memory. A separate global MemInfo snapshot is the correct interface for capacity.
5. Global used/free memory is a cross-cell side channel. The precedent is `GetProcs2`: it owns opt-in allowlist bit 55 and has a compile-time assertion preventing implicit grants (`libs/api/src/abi/syscall.rs:662`; `libs/ostd/src/runtime.rs:103`). MemInfo should likewise be explicitly declared by the benchmark, not inherited from `spawn`, network, or block-I/O capabilities.

## Stable ABI proposal

Use the next append-only opcode and bit:

- `ViSyscall::MemInfo = 243`; opcode 243 is currently pinned as unknown by the ABI collision test (`libs/api/src/abi/syscall_tests.rs:98`). Move that unknown sentinel to 244 when adding the new case.
- Allowlist bit 56, opt-in only. Add a `libs/ostd/src/runtime.rs` assertion analogous to `GetProcs2` so coarse manifest flags never grant it.
- Add a fixed-width, append-only `#[repr(C)]` structure in `libs/api/src/abi/syscall.rs`, e.g. `ViMemInfoV1 { total_frames: u64, used_frames: u64, free_frames: u64, page_size: u64 }`. Fixed `u64` fields keep RV32, RV64, AArch64, and x86_64 layouts identical; pin size/alignment in `libs/api/src/abi/syscall_tests.rs`.
- ABI: `a0 = out_ptr`, `a1 = out_len`, return bytes written on success and the existing all-ones error sentinel on failure. Passing a byte length permits a clean `BufferTooSmall` failure and future versioned structs without mutating v1. Copy as bytes after `validate_user_buf`; do not require caller-provided pointer alignment.
- `ostd::sys_mem_info() -> Result<ViMemInfoV1, SyscallError>` owns the aligned local structure and treats any non-success return as an error. The current dispatcher collapses every kernel `SyscallError` to `usize::MAX`/`u32::MAX` (`kernel/src/task/syscall.rs:4755`, `:4782`, `:4814`), so the wrapper cannot promise a more specific error until A2 changes that contract.

This proposal intentionally reports allocator capacity/commitment, not the documented kernel-plus-three-services resident footprint. The benchmark should label the JSON/report value accordingly or the metric definition must be ruled before implementation. It must not retain a PASS by subtracting the boot heap or another magic baseline.

## Implementation map

1. `kernel/src/memory/frame.rs`: add `used_frames`; initialize it to bitmap pages; increment/decrement only on actual bit transitions; expose `used_frames`, `free_frames`, and byte accessors. Guard double-free/idempotent marking so accounting cannot underflow or drift. The existing `used_memory()` caller in `kernel/src/hypervisor/registry.rs:109` then becomes truthful.
2. `libs/api/src/abi/syscall.rs`: after the second explicit confirmation, add opcode 243, bit 56, `From<usize>`, and `ViMemInfoV1` with ABI documentation.
3. `libs/api/src/abi/syscall_tests.rs`: add the round trip/discriminant, move the unassigned-ID assertion to 244, pin struct layout, and pin bit 56.
4. `kernel/src/task/syscall.rs`: add the internal variant, `syscall_to_vi` mapping, register decode, allowlist coverage, handler, pointer/length validation, and byte copy from one allocator-lock snapshot. Never hold `SCHEDULER` while acquiring `FRAME_ALLOCATOR`.
5. `libs/ostd/src/syscall.rs` and `libs/ostd/src/runtime.rs`: add the safe wrapper and the no-implicit-grant assertion.
6. `cells/tests/bench/src/main.rs`: add `MemInfo` to the explicit syscall manifest. `cells/tests/bench/src/scenarios/memory_footprint.rs`: delete `APPROX_BOOT_BYTES`, call the wrapper once, propagate failure rather than ignoring it, and report the selected real field.
7. `tests/integration/tests/boot.rs` / `.github/workflows/perf.yml`: assert a MemInfo-derived value is emitted and is not 3,500,000. Decide the 10 MiB gate before expecting the perf workflow to remain green: it currently fails any `[bench] ... FAIL` line (`.github/workflows/perf.yml:171`).
8. Update `docs/performance-report.md`, `docs/project-roadmap.md`, `docs/project-overview-pdr.md`, and the A3 docket entry only after runtime evidence exists; these currently claim or preserve the `< 10 MB kernel + 3 services` definition (`docs/performance-report.md:15`; `docs/project-overview-pdr.md:377`; `docs/project-roadmap.md:1352`).

## Required confirmation gate

`docs/code-standards.md` Law 1 says any change under `libs/api/` or `libs/types/` requires **2x explicit user confirmation**. “Continue from section 8” is authorization to research/plan A3, not an explicit confirmation of opcode 243, bit 56, or the public struct layout.

Before implementation:

1. Present the exact proposal above, including opcode 243, bit 56, `ViMemInfoV1` fields/layout, side-channel gate, and the likely 10 MiB false-green becoming a real failure; obtain explicit confirmation #1.
2. Immediately before the first edit to `libs/api/src/abi/syscall.rs`, repeat the exact ABI delta and affected stable files, state that this is the second Law 1 confirmation, and obtain explicit confirmation #2.

No ABI edit, staging, or commit may occur between those confirmations. If the user changes the layout or metric after confirmation #1, the revised proposal resets the two-confirmation sequence.

## Verification matrix

- Host API tests: opcode round trip/collision, allowlist bit, fixed struct size/alignment.
- Allocator tests: initialization includes bitmap frames; single/contiguous/aligned allocation increments exactly; deallocation decrements exactly; double-free cannot underflow; `total = used + free`.
- Kernel syscall tests: register decode, `syscall_to_vi`, bit-56 deny/allow, short/null/overflow buffer rejection, successful snapshot consistency.
- Compile: RV32, RV64, AArch64, x86_64 because the ABI crosses pointer widths.
- Runtime: normal RV64 boot; benchmark reaches completion; emitted memory value differs from 3,500,000; repeated calls preserve `total = used + free`; a controlled allocation increases used frames and release restores them if a bounded fixture is practical.
- CI truth gate: do not mark A3 verified merely because the suite completes. Record whether the real value passes or fails the separately ruled metric.

## Compatibility risks

- Stable ABI drift without both confirmations is prohibited.
- Reusing an existing opcode/allowlist bit can silently authorize the wrong syscall; append-only 243/56 plus pinned tests avoids that.
- Returning `usize` fields would create different RV32/64 layouts; fixed widths avoid it.
- A counter updated without checking bitmap transitions can drift on double-free or repeated range marking.
- Exposing MemInfo broadly leaks system activity; explicit allowlisting limits the side channel.
- Calling the physical-frame number “kernel + 3 services footprint” would replace one false metric with another. The field and benchmark name must state whether they mean allocator-committed frames or resident/live bytes.
