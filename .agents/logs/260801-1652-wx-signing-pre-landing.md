# 2026-08-01 — W^X and signing pre-landing

## What happened
Prepared the 44-commit W^X, signing, completion, and VFS branch for landing.
The ship review found and closed two completion-wait publication races.

## Decisions
- Completion waiters use owned RAII cleanup so stale registrations cannot survive.
- NET_RX uses explicit Armed/Completing/Idle states without nested ISR locks.
- The registry retains the final queue Arc outside allocator-safe syscall context.

## Lessons
- Removing a source reservation is not equivalent to publishing its completion.
- Interrupt-safe ownership includes destructor timing, not only lock ordering.

## Next steps
- Add a CI lane for the signing-required kernel feature.
- Add cross-hart TLB shootdown before claiming SMP-safe W^X.
