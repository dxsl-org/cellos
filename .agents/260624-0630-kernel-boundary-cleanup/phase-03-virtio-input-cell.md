# Phase 03 — VirtIO Input → Input Service (driver merge)

## Context Links
- Plan: [plan.md](plan.md) · Prereq: [phase-00](phase-00-prerequisites.md)
- Source: `kernel/src/task/drivers/virtio_input.rs` (188) + `input_map.rs` (387, scancode→KeySym translation)
- Target: `cells/services/input/` (already exists) — `src/main.rs`, `src/dispatcher.rs`
- Kernel IRQ today: `virtio_blk.rs:102` `vi_handle_virtio_irq` calls `virtio_input::ack_irq` + `poll_events` + `dispatch_pending`.

## Overview
- **Priority:** P2 (parallel after Phase 00).
- **Status:** pending
- **Risk:** MED — the input service IPC protocol has a known buf[0]-dispatch vs postcard collision (memory: input-ipc-protocol-collision); current code discriminates by **sender** (0=kernel raw, >0=typed). Moving the driver into the service must preserve that.
- **Description:** The input service currently receives raw VirtIO input frames *pushed by the kernel* (`sender=0`, 9-byte `[opcode][code u32][value u32]`). Migrate the VirtIO input *driver* (device probe + queue poll) AND the `input_map.rs` translation into the input service so it polls the device directly. Kernel stops pushing input frames.

## Key Insights (verified)
- Input service today (`cells/services/input/src/main.rs:9-28`): kernel pushes raw frames (`sender=0`); GUI apps send typed `InputRequest` (`sender>0`, postcard). Translation `scancode→KeySym` currently lives kernel-side (`input_map.rs`) and the kernel sends already-... actually verify: the kernel pushes raw EV_KEY/EV_REL/EV_ABS; the service's `handle_message` translates. `input_map.rs` (387 LOC) is the US-QWERTY layout table — confirm whether it's kernel or already mirrored in the service. Move the authoritative copy into the service.
- After migration: input service claims VirtIO input MMIO via `sys_request_mmio`, polls the device queue itself, and on RISC-V/ARM64 blocks on `sys_wait_irq(input_irq)`. No more kernel→service raw-frame push (`sender=0` path retires; the service generates events internally then dispatches to the focused cell exactly as today).
- The input service needs `PcieDriverCap` (or an MMIO grant) — granted by init at spawn via path `/bin/input` (extend loader path-grant). This is a **capability escalation** of an existing service.
- The buf[0] dispatch collision (memory): once the kernel stops pushing `sender=0` frames, the collision risk for the raw-vs-typed multiplex shrinks — but keep sender-based discrimination intact for the UART relay (`EV_ASCII` opcode 0x04 from console driver) which still arrives via `sender=0` until console_drv is simplified (Phase 08 / coordinate).

## Requirements
### Functional
1. Input service claims VirtIO input MMIO, polls events, translates via in-service `input_map`.
2. `sys_wait_irq(input_irq)` on RISC-V/ARM64; poll on x86_64.
3. Kernel stops pushing input frames; `virtio_input.rs` retired (Phase 08).
4. Typed `InputRequest` focus protocol unchanged; UART relay path preserved until console simplified.

### Non-Functional
- `#![forbid(unsafe_code)]` except input MMIO island.
- No regression in keyboard/mouse latency in compositor/DOOM/shell.

## Architecture
```
BEFORE: kernel virtio_input ISR → poll_events → IPC push (sender=0) → input service → translate → focused cell
AFTER:  input service claims MMIO → sys_wait_irq(input_irq) → drain queue → translate (in-service input_map) → focused cell
```
UART relay (console_drv → input service `EV_ASCII`) stays until Phase 08 console simplification; route it through a distinct sender or opcode to avoid the known collision.

## Related Code Files
**Modify (extend input service):**
- `cells/services/input/src/main.rs` — add Init: claim MMIO + start poll/IRQ loop; remove reliance on kernel raw push.
- `cells/services/input/src/device.rs` (NEW) — VirtIO input device probe + queue drain (port `virtio_input.rs`).
- `cells/services/input/src/keymap.rs` (NEW) — port `input_map.rs` translation table.
- `cells/services/input/Cargo.toml` — add `ostd::mmio`/`dma` usage (already deps types/api/ostd).

**Modify (kernel):**
- `kernel/src/loader.rs` — grant input MMIO cap (PcieDriverCap or a new InputCap) to `/bin/input`.
- `kernel/src/task/drivers/virtio_blk.rs:102` — remove the `virtio_input::ack_irq/poll_events/dispatch_pending` branch (Phase 08; during transition, gate it so it's inert once the service registers).
- `kernel/src/main.rs` — stop kernel virtio_input init once service owns it (transition flag).

## Implementation Steps
1. Confirm where `input_map.rs` translation is authoritative (kernel vs service). Read both `virtio_input.rs` and the input service's `handle_message`.
2. Port `virtio_input.rs` device probe + queue drain into `cells/services/input/src/device.rs` (ostd::mmio).
3. Port `input_map.rs` → `cells/services/input/src/keymap.rs`.
4. Input service Init: `sys_request_mmio(input_base, len)` (find VirtIO input slot/BAR), start the poll/`sys_wait_irq` loop.
5. Grant input service the MMIO cap at spawn (loader path-grant `/bin/input`).
6. Disable kernel input push behind a transition flag; remove the `vi_handle_virtio_irq` input branch (gate now, delete Phase 08).
7. Preserve UART-relay `EV_ASCII` path (console_drv → service) with collision-safe routing.
8. gen_disk: input service already built/signed — confirm new cap manifest if needed.
9. Boot GUI test: keyboard types into shell, mouse moves cursor, focus routing to robot-dashboard works.

## Todo List
- [ ] Locate authoritative input_map translation
- [ ] Port device probe/drain → device.rs
- [ ] Port keymap → keymap.rs
- [ ] Service Init claim MMIO + IRQ/poll loop
- [ ] Loader cap grant /bin/input
- [ ] Disable kernel push + IRQ branch (transition flag)
- [ ] Preserve UART relay collision-safe
- [ ] Boot GUI keyboard+mouse test

## Success Criteria
- [ ] Keyboard input in shell + DOOM works with kernel `virtio_input` push disabled.
- [ ] Mouse moves compositor cursor; focus routing intact.
- [ ] RISC-V/ARM64: input service wakes on `sys_wait_irq`; x86_64 polls.
- [ ] UART relay still delivers serial keystrokes (until Phase 08).
- [ ] Disabling input MMIO claim → graceful (no panic; just no input).

## Risk Assessment
| Risk | L | I | Mitigation |
|------|---|---|-----------|
| buf[0]/postcard collision resurfaces | Med | Med | Keep sender-based discrimination; route UART relay distinctly |
| Input service cap escalation widens TCB | Low | Med | Input is already trusted (focus authority); MMIO grant scoped to input BDF only |
| Lost keystrokes during IRQ migration | Med | Med | Pending latch (Phase 00) + drain-all-on-wake |
| QEMU input device discovery differs by arch | Med | Med | Probe both VirtIO-MMIO (RISC-V/ARM) and VirtIO-PCI (x86) like blk |

## Security Considerations
- Input service already holds focus-routing authority (kernel-verified sender_tid for SetFocus). Adding MMIO claim keeps it the single input TCB element — acceptable.
- Input MMIO scoped to the input device BDF; cannot read other devices.

## Next Steps
- Phase 08 deletes `virtio_input.rs`, `input_map.rs`, and the kernel input-push path; simplifies `console_drv.rs` to UART-only (the EV_ASCII relay moves fully into the input service).
