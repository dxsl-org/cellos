# G3 Accelerator Evidence Envelope

**Date**: 2026-08-20  
**Status**: BLOCKED — no accelerator hardware or accepted vendor SDK is available  
**Scope**: Phase 06 readiness ledger; no stable ABI or kernel scheduler

## Decision

RK3588 is the first probe target. X390 remains the second implementation only
after purchasable silicon, board documentation, and a usable software stack are
available. This ordering follows the current product roadmap and avoids treating
SiFive core IP as a board that Cellos can qualify today.

No `ViAccelerator`, `TensorBuffer`, `sys_grant_tensor`, probe crate, or NPU
scheduler may land while this ledger is BLOCKED.

## Provenance and License Gate

The local reference set at `D:\Cellos\.references` contains Nanvix, Redox,
seL4, Theseus, and Tock. A filename/source scan found no RKNN, RK3588 NPU, X390,
tensor, or accelerator implementation to port.

Official upstream evidence:

- [RKNN Toolkit2](https://github.com/airockchip/rknn-toolkit2) supports RK3588
  and separates host model conversion from board-side C/C++ runtime inference.
- Its [SDK license](https://github.com/airockchip/rknn-toolkit2/blob/master/LICENSE)
  is a Rockchip agreement with use, modification, redistribution, third-party,
  export-control, and termination conditions. It is not treated as a generic
  permissive dependency.
- The older [RKNPU2 repository](https://github.com/rockchip-linux/rknpu2) is
  BSD-3-Clause but is no longer maintained and points to the Toolkit2 tree.
  Its code can be a technical oracle only after file-level provenance is
  reconciled with the current SDK package.
- [SiFive X390](https://www.sifive.com/cores/intelligence-x300-series) is core
  IP with optional SSCI/VCIX coupling. SiFive's 2025 announcement discussed
  licensing and a first Gen-2 silicon timeline; that is not evidence of an
  available Cellos qualification board.

Before importing any vendor file, record its exact version, file-level license,
redistribution permission, required firmware/runtime blobs, and export-control
review. Until then, vendor code and binaries stay outside the repository.

## Existing Large-Buffer Substrate

The current Grant implementation is useful but is not yet a tensor contract:

| Property | Current evidence | G3 consequence |
|---|---|---|
| Allocation | `GrantAlloc`/`GrantRegister` allocate contiguous pages with a 4096-page, 16 MiB ceiling | Large models require chunking or a measured larger-buffer policy |
| Sharing | One `shared_to` task is recorded per Grant | Fan-out and multi-stage inference are not represented |
| Permission | `GrantPerm` is recorded, while current slice access follows active mapping/registration state | Directional CPU access must be enforced before sharing model weights |
| Lifetime | Owner-only free/unregister; task death reaps grants | Suitable base for restart tests, not proof of NPU safety |
| In-flight access | Pinned regions cannot be freed; death quarantines frames until explicit acknowledgement | NPU completion/fence acknowledgement must connect to this lifecycle |
| Bounds | 128 global pins, 48 pins per task, 64 quarantined regions | Limits need workload measurements and exhaustion tests |
| DMA isolation | PCI Driver Cells connect cleanup to IOMMU acknowledgement | RK3588 NPU/SMMU ownership and teardown are not implemented |
| Address model | Grant id is the identity-mapped physical base in the current SAS | Vendor runtime and device-visible addresses must not be assumed identical |

These observations come from `kernel/src/task/syscall.rs`,
`kernel/src/memory/pin.rs`, and `libs/ostd/src/syscall.rs`. Phase 06 makes no
changes to those interfaces.

## Probe Vocabulary — Observations, Not ABI

The first Tier 1 `ffi-posix` probe records vendor behavior without publishing a
Cellos syscall or stable message schema. Each run must retain:

- board identity, firmware, kernel, NPU driver, runtime, and SDK versions;
- accepted SDK/license record and hashes of every binary input;
- model hash, input/output count, shapes, data types, byte sizes, and memory mode;
- observed load, submit/run, wait/output, release, and destroy calls;
- monotonic timestamps for conversion-independent load and inference latency;
- raw vendor result code mapped only in the evidence report, not to `ViError`;
- buffer ownership before and after each call, including cache/fence operations;
- driver-cell restart result, app-cell survival, quarantined frames, and recovery;
- panic, Cell fault, IOMMU/SMMU fault, timeout, and memory-growth counters.

Names such as `load`, `submit`, and `wait` describe observations only. They must
not be copied into `libs/api`, `libs/types`, or a Law-1 interface until the
RK3588 and second-implementation evidence has been compared.

## Promotion Gates

### A. Hardware and legal

- [ ] RK3588 board is physically identified and booted with retained UART log.
- [ ] Exact SDK/runtime version and license acceptance are recorded.
- [ ] Required firmware and binary redistribution are approved separately.
- [ ] Model license and test-data provenance are recorded.

### B. Runtime probe

- [ ] A real Tier 1 `ffi-posix` Cell loads the vendor runtime without widening
      shell or unrelated Cell authority.
- [ ] Load, input setup, run, wait/output, release, and destroy succeed on board.
- [ ] Unsupported model, malformed input, timeout, and device-unavailable paths
      return bounded failures without kernel panic.

### C. Buffer and DMA lifecycle

- [ ] CPU, runtime, and NPU-visible addresses are measured rather than assumed.
- [ ] Cache maintenance and fence semantics are recorded for every memory mode.
- [ ] Grant size/chunking stays within measured bounds.
- [ ] NPU DMA is restricted by a proven RK3588 IOMMU/SMMU ownership path.
- [ ] Driver acknowledgement releases quarantined frames after teardown.

### D. Performance and restart evidence

- [ ] Warm inference loop reports sample count plus P50/P95/P99/max latency.
- [ ] Cold load and first inference are reported separately from steady state.
- [ ] At least one memory-pressure run records allocation failures and recovery.
- [ ] Killing/restarting the probe Cell leaves the calling app Cell alive.
- [ ] Repeated load/run/destroy cycles show bounded memory and no stale DMA.

### E. Second implementation

- [ ] Available X390-class hardware and its software stack repeat the same
      observation ledger without forcing an RKNN-shaped public contract.
- [ ] Only then may a separate plan propose accelerator ABI or scheduling work.

## Current Verdict

The static readiness audit is complete, but every hardware/runtime promotion
gate is open. Phase 06 therefore remains BLOCKED, not failed and not complete.
The next safe action is to obtain an RK3588 board and perform the legal/runtime
preflight; creating a placeholder probe would produce no valid evidence.
