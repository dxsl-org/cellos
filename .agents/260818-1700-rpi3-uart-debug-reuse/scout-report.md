# Scout Report

## Relevant Files

- `kernel/src/task.rs:1097-1129` duplicates mini-UART LSR polling and IO writes inside the RPi3 TrapFrame diagnostic.
- `hal/arch/arm/src/aarch64/uart_bcm_mini.rs:112-120` already exposes the matching FIFO-safe `probe_put` mechanism.
- `hal/arch/arm/src/aarch64.rs:42-43` exposes the helper under the same AArch64/RPi3 cfg gate.

## Boundary

ARM HAL owns UART register access and FIFO readiness. Kernel task setup owns diagnostic values and byte ordering.

## Precedents

- `0690b9ad` finished routing BCM IRQ consumers through shared SoC facts.
- `546f4de5` moved BCM controller-base consumption into ARM HAL.
- `a84b9fc3` is the current real-board RPi3 bring-up baseline.

## Blast Radius

One debug-only block in `kernel/src/task.rs`; no public contract changes.

## Deferred Work

UART initialization, pinmux, baud, IRQ/RX, profile data, and physical RPi3 validation are excluded.
