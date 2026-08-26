# 2026-08-21 — App Tiers completion plan

## What happened
Created and adversarially validated the umbrella program for TODO items 2–9.
The program coordinates nine independently gated workstreams and authorizes no
implementation by itself.

## Decisions
- Completion requires `FULLY_QUALIFIED`; BLOCKED/PLANNED work cannot close scope.
- Raspberry Pi 3 is the primary physical Tier 3 qualification lane.
- Tier 1 admission uses a signed atomic A/B store bound to an external
  non-replayable floor; production remains disabled until that floor qualifies.
- Manifest v3 extends the publisher-signed canonical envelope; owner consent
  remains a separate digest-pinned record.
- Rust `std` promotion requires p99 syscall/IPC regression at or below 5%.

## Lessons
- Evidence integrity, child lifecycle, and approval ownership must be explicit.
- A/B generations alone do not prevent rollback when both slots can be replayed.
- Tier 2 teardown must quarantine resources when a hart or device cannot fence.

## Next steps
- Approve and execute Phase 01, Native SDK Contract C2, as the first child plan.
