# 2026-08-17 — RPi3 UART RX interrupt

## What happened
Real-board testing reproduced deterministic eight-byte UART burst truncation. Cellos now drains mini UART RX through AUX legacy IRQ 29 and passed the original commands plus 100/100 burst iterations.

## Decisions
- Enable RX IRQ only after the 4 KiB heap-backed queue exists; avoids pre-heap allocation and early IRQ writes.
- Keep direct hardware polling as fallback; IRQ routing failure does not remove slow interactive input.
- Check AUX pending independently of CORE0_IRQ_SOURCE.GPU; the source bit is not reliable on every raspi3 environment.
- Remove the board-only per-log-syscall warning; it amplified each echoed character and polluted timing evidence.

## Lessons
- At 115200 baud, an eight-symbol FIFO fills far faster than a 10 ms scheduler poll.
- An exact eight-byte truncation boundary plus a paced positive control distinguishes hardware overrun from parser or IPC loss.
- Always rebuild with `EMBEDDED_OVERRIDE=target/rpi3-embedded` before publishing the board image.

## Next steps
- Continue real-board shell/VFS smoke testing from the verified IRQ image.
- Commit only when explicitly requested; the worktree contains unrelated pre-existing changes.
