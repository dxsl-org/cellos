# Phase 06 — NVMe Kernel Driver (G2 Deferred)

**Status**: Deferred
**Gate**: Phase 05 (PCIe ECAM) complete + G2 hardware available
**Replaces**: VirtIO-BLK as the `/srv` block transport (transparent to VFS via `ViBlockDevice` trait)

---

## Context Links

- `docs/specs/04-hardware.md:88-110` — NVMe in PCIe strategy
- Phase 05 (`phase-05-pcie-ecam.md`) — mandatory prerequisite
- `kernel/src/task/drivers/blk_virtio.rs` (Phase 01) — pattern to mirror
- NVMe spec: [NVMe 1.3d](https://nvmexpress.org/wp-content/uploads/NVM-Express-1_3d-2019.03.20-Ratified.pdf)

---

## Overview

Implement a minimal NVMe polling driver that registers as the active `ViBlockDevice`, replacing
VirtIO-BLK for `/srv` when an NVMe controller is detected at PCIe enumeration time.

The `RedoxFsBackend` and `VicellDisk` from Phase 03 require zero changes — the block transport
swap is fully transparent via the `ViBlockDevice` trait.

---

## NVMe Initialization Sequence

```
1. PCIe enumeration (Phase 05) finds NVMe at class 01:08
   → BAR0 = 64-bit MMIO base (NVMe Controller Registers)

2. Controller reset:
   CC.EN = 0  (offset 0x14)
   poll CSTS.RDY → 0  (offset 0x0C, timeout 500ms)
   read CAP (offset 0x00): MQES (max queue entries), DSTRD (doorbell stride)

3. Admin queue allocation:
   alloc 1 page ASQ (submission), 1 page ACQ (completion)
   write AQA (0x24): ASQS=64, ACQS=64
   write ASQ (0x28): physical address
   write ACQ (0x30): physical address
   CC.EN = 1; poll CSTS.RDY → 1

4. Identify controller (admin cmd opcode 0x06):
   submit to ASQ; poll ACQ; read MDTS (max data transfer size)

5. Create I/O queues (admin cmds 0x05 + 0x01):
   Create I/O Completion Queue (alloc 1 page CQ)
   Create I/O Submission Queue (alloc 1 page SQ, linked to CQ above)

6. Block I/O (NVM command set):
   Read (opcode 0x02): SLB A, SLBC B, PRP1 = phys(buf)
   Write (opcode 0x01): same; ring SQ tail doorbell at BAR0 + 0x1000 + 2*qid*DSTRD
   Poll CQ for completion (no interrupt in Phase 06 — polling only)
```

---

## DMA in HHDM SAS

PRP (Physical Region Page) values = physical address = `virt_addr - HHDM_BASE`.
For ≤4 KB transfers: `PRP1 = phys(buf)`, `PRP2 = 0`.
For ≤8 KB transfers: `PRP1 = phys(buf_page0)`, `PRP2 = phys(buf_page1)`.
For larger transfers: allocate a physically contiguous PRP list page.

Phase 06 supports only ≤4 KB transfers (single sector). Larger transfers added if needed.

---

## QEMU Test Command

```bash
qemu-system-riscv64 \
    ... \
    -drive file=srv.img,if=none,id=nvm0 \
    -device nvme,serial=vicell-srv,drive=nvm0
```

For x86_64 q35:
```bash
qemu-system-x86_64 -M q35 \
    ... \
    -drive file=srv.img,if=none,id=nvm0 \
    -device nvme,serial=vicell-srv,drive=nvm0
```

---

## Files to Create

| File | Purpose |
|------|---------|
| `kernel/src/task/drivers/blk_nvme.rs` | NVMe init + polling read/write; implements `ViBlockDevice` |
| `kernel/src/task/drivers/mod.rs` | Probe NVMe after PCIe enumeration; prefer NVMe over VirtIO-BLK |

---

## Crate Evaluation

- `nvme-oxide`: closest to self-contained, covers admin+IO queue lifecycle. Verify source quality
  before adopting — the crates.io page exists but the GitHub was inaccessible during research.
  If abandoned/low-quality, write from scratch (~1500 LOC including PCIe portion).
- Prioritise a minimal polling implementation over feature completeness.
- MSI-X interrupts (vs. polling) can be added in a follow-on phase.

---

## Success Criteria

- QEMU q35 boot: `[nvme] controller found, {capacity} sectors` in log
- NVMe replaces VirtIO-BLK as active block device; `/srv` RedoxFS still mounts and passes
  all scenarios from Phase 04
- `cargo clippy -p vicell-kernel --target x86_64-unknown-none` clean
- All `// SAFETY:` comments on PRP buffer construction and BAR MMIO access
