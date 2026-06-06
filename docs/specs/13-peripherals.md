# 13 — Peripheral Driver Bus (HAL) — PLACEHOLDER

**Status**: 📋 PLANNED (placeholder — full spec to be authored separately)
**Stage**: G1 (Robot & Embedded) — defining requirement for "complete for robots"
**Roadmap**: [project-roadmap.md](../project-roadmap.md) → "Peripheral Driver Track `[G1]`"

---

> ⚠️ This file is a **placeholder / stub**, not a complete specification. It holds the slot and
> records intended scope. The detailed design (trait signatures, IPC opcodes, capability model,
> per-arch backends) must go through its own brainstorm → plan → cook cycle before implementation.

## Why

Robots and embedded devices control **sensors and actuators** over hardware buses. The current
ViCell stack has **no device-bus abstraction** — only VirtIO block/input/net/GPU. This is the
largest gap between "runs on QEMU" and "drives a real robot".

## Intended Scope

### HAL bus traits (`hal/traits/`)
- `ViGpio` — digital in/out, pin direction, interrupt-on-edge (minimal, first)
- `ViUart` — serial TX/RX (minimal, first)
- `ViI2c` — master read/write, 7/10-bit addressing
- `ViSpi` — full-duplex transfer, mode/clock config
- **Extension**: `ViCan` (CAN 2.0/FD), `ViPwm` (servo/motor), `ViAdc` (analog sensors)

### Driver Cells (`cells/drivers/`)
- Each peripheral driver is a Cell with `#![forbid(unsafe_code)]` (Law 4) — hardware `unsafe`
  lives only in the HAL/arch backend.
- Capability-gated via ELF manifest (Phase 30) — a Cell must declare e.g. `gpio`, `i2c` caps.
- Owned-buffer async I/O (Law 2) for bus transfers.

### Per-arch backends (`hal/arch/`)
- RV64 (QEMU virt + real SBC), ARM64 (RPi/Jetson GPIO/I2C controllers), RV32-Nano (sub-track).

## Open Questions (resolve in full spec)
- IPC contract: typed postcard enums (like VFS/net) vs raw opcodes for hot bus transfers?
- Interrupt delivery for GPIO edge / UART RX — reuse PLIC/GIC dispatch pattern?
- Capability granularity: per-bus vs per-pin/per-device?
- Real-board validation target (which SBC first)?

## Acceptance (G1 graduation contribution)
- GPIO + UART working on QEMU + ≥1 real board.
- Reference robot demo (sensor→compute→actuator over GPIO/CAN + MQTT telemetry) runs end-to-end.

## See Also
- [04-hardware.md](04-hardware.md) — multi-arch HAL
- [05-application.md](05-application.md) — Cell tiers & isolation
- Phase 30 (ELF capability manifests) — privilege gating mechanism reused here
