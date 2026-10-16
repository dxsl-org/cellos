# Scoped CP1 and G0/G2 development authorization

## Authority and recorded decision

Recorded 2026-10-15. Decision source: explicit user instruction **“phê duyệt CP1 cùng điều kiện G0 và G2”** in this session, following the established dependency-checkpoint governance framework. This records the requested **development CP1 and conditional prerequisite authorization**, authorizing native integration and VMware qualification of the native DCO1 codec and BlockDevice journal engine.

This is a development authorization, not an unverified claim of fleet production readiness or hardware deployment admission. All 38 legacy scoped locks remain formally effective; downstream CP2 and production release remain `blocked`.

## Scope and approved components

1. **CP1 Candidate Approval:**
   - Standalone native codec library `libs/durable-charge-codec/` (`no_std`, `#![forbid(unsafe_code)]`, allocation-free): complete implementation of all 27 DCO1 record kinds (1..11, 32..36, 48..58), acyclic reference validation, and Ed25519 NCF1 proof verification (35 host integration tests pass, compiles on `x86_64-unknown-none`).
   - Native journal engine `libs/durable-charge-journal/` (`no_std`, `#![forbid(unsafe_code)]`): double-buffered A/B superblocks, double-barrier atomic commit over `cellos_fs::BlockDevice`, crash-recovery selecting the valid superblock with highest sequence, and lifecycle authority state machine `AuthorityGate` enforcing two-plane resize accounting ($G = \max(Q - B, 0)$, $L = \max(Q - E, 0)$), fold without credit, partial shrink credit installments, cancel no-app, `Closing` intent gate, and `release_once` (7 crash and lifecycle tests pass, compiles on `x86_64-unknown-none`).

2. **Condition G0 (Instrumentation & Hardware Boundary):**
   - The private measurement exporter and software descriptor trees are accepted for the development and software-isolated target lane.
   - The 15 blocker classes recorded in `build/cp1-prerequisites/g0-vtd-readiness-20261014.json` (unsettled samples, sample-to-sample jitter, and physical hardware DMA remapping) remain explicit hardware-lane prerequisites; physical Intel VMX/IOMMU hardware publication is not claimed from software emulator runs.

3. **Condition G2 (Durable Journaling & Persistence Boundary):**
   - The atomic double-barrier DCO1 journal over `BlockDevice` is accepted as the native owning-service journal engine, resolving earlier Python-model counterexamples (S8/S9) by strictly refusing uninitialized and corrupted superblocks.
   - Physical power-loss persistence across un-flushed hardware drive caches and independent protected anti-rollback remain explicit operational boundaries; they are not claimed from virtual-volume or emulator execution.

4. **Integration Gate:**
   - Authorizes Step 4: executing native Cellos VMware fault-injection and storage qualification using the native SATA/AHCI transport verified in Step 1.
   - Does not authorize modifying unrelated production kernel/cell interfaces or bypassing the Law-1 CP2 gate.
