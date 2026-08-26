# 2026-07-31 — DTB memory and runtime gates

## What happened
A1 replaced RV64's fixed 190 MiB OpenSBI map with reservation-safe DTB discovery and a 2 GiB
capacity gate. A4 closed phase 11 and runtime-verified phase 09's missing-policy strip path while
recording the ARM demo packaging and full-suite timeout gaps.

## Decisions
- Reject malformed, unsupported, dynamically reserved, or over-capacity DTB maps and retain the
  audited static board map as the fail-closed fallback.
- Use explicit `.got` placement and `__kernel_end`; `__stack_top` did not own the orphaned GOT.
- Accept DT memory nodes only when status is absent, `ok`, or `okay`.
- Keep the incomplete-policy signer test-only and import the production signer rather than adding
  a production bypass flag.

## Lessons
- Runtime capacity tests exposed a linker ownership bug that compile and parser fixtures missed.
- A zero-event policy test is meaningless unless it first proves a valid complete policy loaded.
- Shared dirty-tree builds can move `__kernel_end` by a page; capacity thresholds should not pin
  an image-size-dependent address.

## Next steps
- Obtain two explicit Law-1 confirmations for the A2/A3 ABI package in
  `.agents/260731-1930-capacity-observability/plan.md`.
- After confirmation, implement `-2` spawn OOM propagation and opt-in MemInfo telemetry.
- Preserve the unresolved fresh full serial RV64 verdict and ARM sensor/robot packaging gaps.
