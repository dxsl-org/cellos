# CP2 — phase 04a: PCIe function query, MSI/MSI-X allocation, skip-owned lookup

Status: **approved by the owner on 2026-11-04 (CP2-04a-1, -2, -3: all accepted).**
v1 of 434/435 and the 418/424 semantics are frozen; phase 04b may start.
Written 2026-11-04 after implementation, against HEAD `7fdc088af` plus the
uncommitted phase 04a tree.
CP1: [cp1-pcie-irq-power-abi.md](cp1-pcie-irq-power-abi.md), approved in
[cp1-approval.md](cp1-approval.md). Evidence:
[reports/phase-04a-evidence.md](reports/phase-04a-evidence.md).

## 1. ABI delta as implemented

| Item | As approved in CP1 | As implemented | Delta |
|---|---|---|---|
| `QueryPcieFunction = 434` | `a0 = bdf`, `a1 = out_ptr`, `a2 = out_len (584)`, `a3..a5 = 0`; v1 record (§3) | same; record layout byte-for-byte as §3 (`libs/api/src/abi/pcie_function.rs`, offsets pinned by `field_offsets_match_cp1_layout`) | `a4`/`a5` never reach the kernel (the cell stub carries four arguments), so only `a3 == 0` is checked |
| `AllocPcieIrq = 435` | `a0 = bdf`, `a1 = want (1..=32)`, `a2 = mode`, return `first_irq \| count<<8 \| kind<<16` | same; MSI-X preferred, MSI rounded down to a power of two and aligned to its size, repeat call returns the existing allocation | none |
| Allowlist | 434/435 share bit 50, gated by `PcieDriverCap` (D-3) | same (`pcie_function_syscalls_share_driver_registration_bit`) | none |
| 418/424 | skip functions owned by another live task (D-5) | same (`vendor_lookup_skips_functions_owned_by_another_driver`) | none |
| `MAX_IRQ` | 64 → 256 | 256 | none |

## 2. Additions that need confirmation

1. **Error sentinels.** CP1 named the error classes but not their values. 434/435
   return `usize::MAX` (denied/other), `MAX-2` no function, `MAX-3` invalid
   input, `MAX-5` not supported, `MAX-6` vectors exhausted
   (`api::pcie_function::ERR_*`). `MAX-1` and `MAX-4` are unused so the values
   stay aligned with the existing typed sentinels of 432/433.
2. **x86 `WaitIrq` gate.** On x86, `WaitIrq(irq)` now also requires that the
   caller holds an `AllocPcieIrq` allocation covering `irq`. Without it a cell
   with `PcieDriverCap` could park on another driver's vector and drain its
   wakeups. No x86 cell called `WaitIrq` before this phase. The wire shape of 234
   is unchanged; riscv64/aarch64 behaviour is unchanged.
3. **Logical IRQ = vector.** `first_irq` is the first IDT vector of a contiguous
   block in `0x30..=0xDF` (without `0x80`). Cells must treat it as opaque.
4. **MSI-X carve rule.** A layout is carved, reported `MSIX_CARVED` and offered
   MSI-X only if the table and PBA lie inside their BARs and every page they
   touch starts with an MSI-X structure (the checkable form of the PCIe rule
   that no other CSR shares those 4 KiB ranges). Otherwise the BAR is mapped
   whole and only MSI is offered.

## 3. Not changed / out of scope (unchanged from CP1)

Interrupt remapping (phase 12; boot logs `[msi] … no interrupt remapping`), MSI
off x86 (435 returns not supported, 434 not found), extended capabilities,
an IRQ free call, `SystemPower = 436` (phase 06, own handoff).

## 4. Known limit carried forward

The kernel scan covers the first admitted bus. A function that only the
Platform Cell registered (bus ≥ 1 on physical machines) carries BAR0 alone, so
its 434 record lists only BAR0. QEMU q35 lanes are unaffected. To be closed
before physical qualification (slice S4).

## 5. Owner decision

| ID | Question | Recommendation |
|---|---|---|
| CP2-04a-1 | Accept §1 as the frozen v1 ABI for 434/435 and the 418/424 semantics | Yes |
| CP2-04a-2 | Accept the four additions in §2 | Yes |
| CP2-04a-3 | Proceed to phase 04b (convert nvme/ahci/igb/e1000/xhci to `irq=msix\|msi`) | Yes |

## 6. Approval record

2026-11-04, owner, in session: CP2-04a-1 accepted, CP2-04a-2 accepted (all four
additions in §2), CP2-04a-3 accepted (proceed to phase 04b). Commit authority was
granted for the phase 04a change set after approval.

## 7. Amendment 2026-11-04 — `FLAG_DMA_REMAPPED`

Found while preparing phase 05: a virtio cell must refuse a device that does
not negotiate `VIRTIO_F_ACCESS_PLATFORM` whenever an IOMMU translates DMA (such
a device bypasses the vIOMMU), but no cell-visible signal said whether
translation is active (IOVA equals the physical address, `QueryMachineFacts` is
SpawnCap-only, init passes no argv). The owner approved adding `flags` bit 3
`FLAG_DMA_REMAPPED` to `ViPcieFunctionV1` v1 (set when VT-d is active), before
any consumer of v1 exists outside tests. `FLAGS_KNOWN` now covers bits 0–3;
the strict decoder still refuses bits 4–31.
