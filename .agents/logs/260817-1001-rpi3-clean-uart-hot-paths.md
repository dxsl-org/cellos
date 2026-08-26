# 2026-08-17 — RPi3 clean UART hot paths

## What happened

Real-board UART output was traced to legacy raw bring-up probes in syscall,
timer, scheduler-miss, and context-switch hot paths. The probes were removed,
guarded against recurrence, and verified on the same RPi3/TFTP lane.

## Decisions

- Remove only per-event `T<EC>`, `M`, `N`, and `A` writes; do not alter trap,
  timer, scheduler, or context-switch control flow.
- Retain fault-only `FS0`-`FS3` diagnostics and bounded one-shot boot probes
  until a separate need/risk review justifies removing them.
- Do not use UART-instrumented hot paths for latency or real-time claims.

## Lessons

- Raw marker bytes without framing can look like corruption when interleaved
  with structured logs; exact source tokens and semantic shell output separated
  instrumentation noise from baud or wiring faults.
- Before/after marker counts are a stronger regression gate than visual console
  inspection: `T15` fell from 14,596 to zero and `ANM` to zero.

## Next steps

- Use the now-clean board lane for bounded boot-time and runtime measurements.
- Keep full G1 peripheral and RT qualification separate from this ARM64
  boot/input regression result.
