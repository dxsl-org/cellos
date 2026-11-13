# S7 handoff approval: kernel `arch/` seam and boot split (Phase 14)

**Date:** 2026-11-13 · **Request:** [s7-handoff-request.md](s7-handoff-request.md)

The owner approved Phase 14 **staged, continuing**:

- **Steps 1–7** run under the request §3 file set, in order. Work stops for the owner only if a step finds a behaviour difference, or if an ABI change turns out to be needed.
- **Old context-switch path:** deleted within Phase 14, once the A/B fixture shows identical markers on x86_64, riscv64 and aarch64.
- **32-bit code:** quarantined under `kernel/src/arch/legacy`, build-checked only, as owner decision 4 says. It is not removed.
- **Commits:** one per step, or one per arch within a step. Nothing is pushed.
- **Ends** when Phase 14 closes. Phases 15 and 16 need their own requests.
