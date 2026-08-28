# 2026-08-27 — Managed ViUI surface

## What happened
Implemented and committed the approved single-surface ViUI/compositor slice:
exact damage, lifecycle handling, and a live generated Counter demo.

## Decisions
- Keep desktop chrome and focus in the compositor; managed ViUI consumes only forwarded input.
- Share `ViSurface` only through crate-private handles so lifecycle and rendering retain one owner without exposing `RefCell` reentrancy.
- Require consuming `ManagedSurfaceApp::shutdown()` after accepted close because the compositor intentionally waits for explicit owner destruction.

## Lessons
- A direct input-focus precedent in an older demo was stale after compositor-owned focus shipped.
- Cell `sys_exit` does not run Rust destructors; lifecycle resources requiring IPC cleanup need explicit shutdown before exit.
- Clean-target RISC-V rebuilds are the reliable tie-breaker for contradictory cached check reports.

## Next steps
- Run managed-surface QEMU runtime evidence when `disk_v3.img` is available; do not claim production runtime qualification before then.
