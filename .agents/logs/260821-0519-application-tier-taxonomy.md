# 2026-08-21 — Application tier taxonomy

## What happened
Phases 2–3 added canonical Manifest protection-class names, retained every
legacy source/ABI surface, and completed cross-target verification and review.

## Decisions
- Keep the public `tier` field and `tier =` macro syntax to avoid source and wire breakage.
- Make `PROTECTION_CLASS_*`, `protection_class()`, and `granted_protection_class()` canonical for new code.
- Keep Zig manifests at intentional v1/8-byte layout until a separate native-v2 migration.
- Keep Tier 2 implementation outside this terminology migration.

## Lessons
- The host kernel/ostd test harness has pre-existing `std` panic/allocator conflicts; bare-metal target checks are the reliable regression lane.
- Active ADRs must avoid literal retired SDK tier labels when grep gates require their removal.

## Next steps
- Phase 4 remains pending and needs separate approval before Tier 2 design work.
