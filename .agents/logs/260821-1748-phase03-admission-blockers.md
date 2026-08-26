# 2026-08-21 — Phase 03 admission blockers

## What happened
Analyzed every Tier 1 admission blocker and implemented the safe backend-neutral core plus hostile harness. Production admission remains disabled because current RPi3 hardware cannot qualify an external floor.

## Decisions
- Selected Core+harness-only: Pi 3 OTP lacks atomic tuple binding and supported secure boot; no TPM/RPMB/secure-element backend exists in-tree.
- Admission state uses the full external binding `(generation, transaction_id, intent_digest, backend_identity)` and never derives floor state from A/B slots.
- Kept fake floor and all 31 hostile tests behind `test-hooks`; no loader/boot/task wiring landed.

## Lessons
- A TPM counter alone cannot atomically bind generation and transaction intent.
- Code/security review evidence cannot substitute for human security-owner approval or physical fault qualification.

## Next steps
- Choose additional secure hardware or a secure-boot/boot-network remote CAS architecture.
- Qualify the real backend with replay and power-loss drills, then implement parsers, anchors, persistence, and common loader enforcement.
