# Phase 04 — Tier 3 Qualification C5

## Context Links
`.agents/TODO.md:29-34`; `docs/specs/05-application.md:361-409`; `docs/guides/tier3b-linux-vm.md:189-202`; `docs/project-roadmap.md:67-73`.

## Overview
Qualify Tier 3 as the current route for untrusted code using independent architecture lanes.

## Key Insights
ARM64, AMD, Intel, and RISC-V have different maturity/blockers; one physical lane suffices without false parity.

## Requirements
Canonicalize `linux-guest`/Guest Integration SDK. Primary physical lane is ARM64 Raspberry Pi 3, confirmed available by the user. Before qualification, pass a feasibility/access gate recording board revision/serial, RAM, firmware/boot files and hashes, custodian, access window, power control, UART capture, storage image, and reproducible build revision. Matrix implemented/QEMU/KVM/hardware status. Cover VirtIO, lifecycle, persistence, isolation, budgets, hostile descriptors, fuzzed queues, reset, crash consistency, and recovery. QEMU may regress code but never substitutes for the physical PASS.

## Architecture
Guest → VMM backend → bounded guest memory → VirtIO proxy → services → supervisor → ledger.

## Assumptions
Raspberry Pi 3 is available; qualification is blocked until feasibility, custodian/access, firmware hashes, UART, and power-reset capture are recorded. RISC-V H-ext stays BLOCKED.

## Related Code Files
`hal/traits/hypervisor/src/lib.rs:69`; `kernel/src/hypervisor/registry.rs:554-612`; `cells/services/hypervisor/src/vmm.rs:9-89`; `cells/services/hypervisor/src/virtio_blk.rs`; `cells/services/hypervisor/src/virtio_net.rs`.

## Implementation Steps
Capture Raspberry Pi 3 feasibility/access/firmware gate; split lane records; inventory gaps; run hostile VirtIO fuzz/reset and persistence crash-recovery campaigns first in QEMU then on-device where applicable; qualify only from physical evidence; update ledger.

## Todo List
- [ ] Lane matrices approved.
- [ ] One physical lane qualifies.
- [ ] Restart/persistence/security pass.

## Success Criteria
The identified Raspberry Pi 3 physically passes captured boot, HTTP, persistent write/crash/reboot/read recovery, restart, hostile guest-memory, malformed VirtIO, fuzz, and reset tests; every PASS binds firmware/build/log hashes and no QEMU result substitutes.

## Risk Assessment
Hardware may stall a lane. Disable only that backend; hardware absence cannot be undone.

## Security Considerations
Bounds-check guest addresses; enforce vCPU budget; Stage-2 contains failure.

## Next Steps
Provide Phase 07 fallback prerequisite.

## Deviation Log
None.
