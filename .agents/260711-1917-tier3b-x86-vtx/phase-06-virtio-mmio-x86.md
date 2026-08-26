# Phase 06 — virtio-mmio on x86 guest + virtio-console (reuse arch-generic stack)

## Context Links
- Plan: [plan.md](plan.md) · Prev: [phase-05](phase-05-cell-pvh-boot-alpine.md)
- Sibling ARM: `.agents/260613-2134-tier3b-vmm-arm64-el2/phase-06-virtio-mmio-console.md`
- Verified: `cells/services/hypervisor/src/virtio_mmio.rs:1-... ` (arch-generic transport), `virtqueue.rs`
  (split virtqueue), `virtio_console.rs` (console device) — reused unchanged; `run_loop.rs:79-108`
  (`virtio_mmio::owns` / `slot_and_offset` dispatch already generic).

## Overview
- **Priority:** P1 · **Status:** pending · **Depends on:** 05
- Expose the guest's virtio devices over **virtio-mmio** (NOT virtio-pci — no PCI bus emulation, matching
  the ARM microvm profile), reusing the arch-generic `virtio_mmio.rs` + `virtqueue.rs` +
  `virtio_console.rs`. The only x86-new work is: placing the virtio-mmio window at an EPT/NPT-unmapped
  GPA, delivering the device IRQ through the 8259 PIC (P05), and passing the guest the right
  `virtio_mmio.device=` cmdline. Success = guest sees `hvc0` virtio-console and I/O flows over the
  virtqueue.

## Key Insights
- **Reuse, do not rewrite (plan invariant):** `virtio_mmio.rs`, `virtqueue.rs`, `virtio_console.rs` are
  arch-generic (the ARM plan built them; only `vmm.rs`/`timer.rs` had cfg gates). The MMIO dispatch in
  `run_loop.rs:79-108` (`virtio_mmio::owns(ipa)` + `slot_and_offset`) is already generic — the x86 run
  loop's `MmioRead`/`MmioWrite` arms route into the same code. GPA replaces IPA; the transport is
  identical.
- **x86 Linux supports virtio-mmio (verify CONFIG):** modern Alpine x86_64 kernels compile
  `CONFIG_VIRTIO_MMIO=y` (confirm in aports config; the `virt` flavor targets microvm-style VMs). The
  guest discovers non-DT devices via the cmdline `virtio_mmio.device=<size>@<gpa>:<irq>` (one per slot),
  e.g. `virtio_mmio.device=0x1000@0xd0000000:5` — size, base GPA, IRQ line (8259 vector). Confirm the
  exact current param syntax against the kernel `drivers/virtio/virtio_mmio.c` cmdline parser.
- **MMIO window placement:** the virtio-mmio window (4 slots × 0x1000) is one of the EPT/NPT-unmapped
  GPA ranges frozen in P02 (e.g. `0xd000_0000..0xd000_4000`). Guest MMIO to it → EPT-violation/NPF →
  `MmioRead`/`MmioWrite` → transport dispatch. **Must be in the P02 frozen unmapped set** (M3 analog) so
  the guest probing it at boot traps correctly rather than reading host DRAM.
- **IRQ delivery via 8259 (P05):** virtio-mmio uses a plain interrupt line; assign each slot an 8259 IRQ
  (e.g. IRQ5/IRQ6/IRQ7). On a used-buffer notification, the device model requests
  `sys_inject_irq(vector)` through the PIC model — no MSI-X, no IOAPIC needed for the MVP.
- **Console first (like ARM P06):** bring up `virtio_console` as slot 0 to validate the transport before
  blk/net. It also gives a cleaner console than the 16550 for interactive use (16550 stays as
  earlyprintk).

## Requirements
**Functional**
- Place virtio-mmio window at the P02-frozen unmapped GPA; wire `virtio_mmio::owns`/`slot_and_offset` to
  it (constants may need x86 GPA values vs ARM IPA values — parameterize, don't fork the logic).
- Assign 8259 IRQ lines per slot; used-buffer notify → `sys_inject_irq(vector)` via PIC.
- Build the `virtio_mmio.device=` cmdline entries (P05 `boot_info` cmdline) matching the window/IRQs.
- virtio-console (slot 0) end-to-end: guest `hvc0` TX/RX over the split virtqueue.

**Non-functional**
- Law 2: guest buffer copies into `Box<[u8]>` before any IPC. Law 4 cell `#![forbid(unsafe_code)]`.
- No PCI bus emulation (rejected — out of scope, matches microvm profile).

## Architecture
```
guest cmdline: ...virtio_mmio.device=0x1000@0xd0000000:5 (console) ...
run_loop x86 MmioWrite/MmioRead @ 0xd000_0000+ → virtio_mmio::slot_and_offset → slot 0 = console
  QueueNotify → console.process_tx → bytes to ViCell serial
  used-buffer ready → pic.deliver(IRQ5) → sys_inject_irq(vector5)
```

## Related Code Files
**Modify**
- `cells/services/hypervisor/src/virtio_mmio.rs` — parameterize the MMIO base (x86 GPA vs ARM IPA) — a
  const/config, not a logic fork
- `cells/services/hypervisor/src/run_loop.rs` — ensure x86 Mmio arms route to `virtio_mmio` dispatch +
  wire per-slot IRQ delivery through `pic_8259`
- `cells/services/hypervisor/src/boot_info.rs` — emit `virtio_mmio.device=` cmdline entries
**Reuse unchanged**
- `virtqueue.rs`, `virtio_console.rs`
**Verify**
- Alpine x86 kernel `CONFIG_VIRTIO_MMIO=y` + current `virtio_mmio.device=` param syntax

## Implementation Steps
1. Confirm `CONFIG_VIRTIO_MMIO=y` in the Alpine x86 kernel; confirm cmdline param syntax.
2. Set the x86 virtio-mmio window GPA (must equal the P02-frozen unmapped range); parameterize base.
3. Wire per-slot 8259 IRQ delivery: device notify → `pic.deliver(irq)` → `sys_inject_irq(vector)`.
4. Add `virtio_mmio.device=` cmdline entries in `boot_info` matching window + IRQs.
5. Bring up virtio-console (slot 0); validate guest `hvc0` TX (bytes to serial) then RX.
6. Confirm the guest enumerates the virtio-mmio device at boot without EPT/NPF misbehavior.

## Todo List
- [ ] Verify Alpine x86 CONFIG_VIRTIO_MMIO=y + cmdline param syntax
- [ ] virtio-mmio window at P02-frozen unmapped GPA (parameterized base)
- [ ] Per-slot 8259 IRQ delivery (device notify → sys_inject_irq)
- [ ] `virtio_mmio.device=` cmdline entries in boot_info
- [ ] virtio-console slot 0 end-to-end (hvc0 TX + RX)

## Success Criteria
- Guest dmesg shows `virtio_mmio` device probe at the window GPA; `hvc0` appears and echoes I/O over the
  virtqueue (TX bytes reach ViCell serial, RX delivers).
- Device-notify IRQ reaches the guest via the 8259 model (no lost/duplicated interrupts under a simple
  echo test).
- No regression to the P05 16550 earlyprintk path.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| Alpine x86 kernel lacks CONFIG_VIRTIO_MMIO | Low×High | Verify config; custom kernel or add module; `virt` flavor targets microvm |
| virtio-mmio window not in P02 unmapped set → reads host DRAM | Med×Crit | Window GPA frozen unmapped in P02 (M3 analog); assert at cell init |
| 8259 IRQ line collision / masked → device hangs | Med×Med | Pick free IRQs (5/6/7); verify PIC unmask; trace injection |
| cmdline param syntax drift across kernel versions | Med×Med | Pin kernel version; verify against virtio_mmio.c parser |

## Security Considerations
- virtqueue descriptors reference guest GPAs; the cell bounds-checks every descriptor against the carve
  (reuse the existing arch-generic virtqueue validation) before touching guest memory.
- No PCI/MMIO passthrough — every virtio access is mediated; the window is host-DRAM-isolated (P02).

## Next Steps
- P07 (virtio-blk → VFS Cell) and P08 (virtio-net → Net Cell) run in parallel on top of this transport.
