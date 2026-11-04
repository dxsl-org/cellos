# CP1 — PCIe function query, MSI/MSI-X allocation, system power, skip-owned device lookup

Status: **draft for Law-1 confirmation 1 and the CELL-CONTRACT-0024 lock handoff for slice S1 (§7). Documentation only.** Written 2026-11-04 against HEAD `64ad34027`. No source file is dirty (`git status --short` shows only this plan directory and the proposed ADR-0031/ADR-0022 edits). Every `file:line` cited here was read in this session. `[INFERENCE]` marks claims that were not read. `OPEN (D-n)` marks owner decisions (§8).

## 1. Scope

**In:**
- three append-only syscalls (`QueryPcieFunction = 434`, `AllocPcieIrq = 435`, `SystemPower = 436`);
- one semantic change, with no wire change, to `FindPcieDevice = 418` and `FindPcieDeviceByVendor = 424`;
- the S1 lock handoff (§7).

**Serves:** [ADR-0031](../../docs/decisions/0031-x86-platform-breadth-kvm-vmware-intel.md) (proposed), plan phases 04a, 04b, 05, 06, 11, 14.

**Out:**
- interrupt remapping (phase 12, kernel-internal);
- MSI for non-x86 architectures;
- any record carrying PCIe extended capabilities (≥ 0x100);
- an explicit IRQ free syscall;
- ACPI sleep states other than S5.

## 2. Evidence

| # | Fact | Evidence |
|---|---|---|
| E1 | A cell sees only BAR0 and the first memory BAR; no capability list | `libs/ostd/src/syscall.rs:2747-2766`, `kernel/src/task/syscall.rs:2428-2470` |
| E2 | The kernel captures MSI-X/PM capabilities but never programs MSI/MSI-X; it only quiesces them | `kernel/src/task/drivers/pcie_ecam.rs:162-203,547-597,768-776`; `kernel/src/task.rs:1920-1935` |
| E3 | PCIe driver cells poll (no `WaitIrq` caller among nvme/ahci/igb/e1000/xhci) | grep, this session; `cells/drivers/xhci/src/controller.rs:303` |
| E4 | `WaitIrq = 234` takes `irq_num: u8`; `MAX_IRQ = 64` | `libs/api/src/abi/syscall.rs:90-95`; `kernel/src/task/drivers/irq_wait.rs:26` |
| E5 | The virtio-modern PCI transport needs vendor capabilities (ID 0x09) that point into arbitrary BARs; the vendored `PciTransport` sizes BARs by writing config space, which a cell cannot do | `kernel/third_party/virtio_drivers/src/transport/pci/bus.rs:267` |
| E6 | 418/424 return the first match; if a live driver owns it, they return 0 without writing, so a second identical device is unreachable | `pcie_ecam.rs:1166-1209`; `kernel/src/task/syscall.rs:2462-2465,8713-8745`; `kernel/src/resource_registry.rs:877-891` |
| E7 | No reset or poweroff path exists on x86 | grep of `kernel/src`, `hal/arch/x86` (scout report) |
| E8 | The allowlist is full (bits 0-62 assigned, 63 = VFS-mutate); 418/424 share bit 50 under `PcieDriverCap`; supervisor operations share bit 49 | `libs/api/src/abi/syscall.rs:1173-1192` |

## 3. `QueryPcieFunction = 434` and `ViPcieFunctionV1`

**Envelope.** `a0 = bdf` (`bus<<8 | dev<<3 | fn`, ≤ 0xFFFF), `a1 = out_ptr`, `a2 = out_len`; `a3..a5` must be 0.
- On success, returns 0 and writes exactly `PCIE_FUNCTION_RECORD_LEN = 584` bytes.
- Errors:
  - `InvalidInput`: `out_len != 584`, non-zero `a3..a5`, or BDF out of range.
  - `PermissionDenied`: no `PcieDriverCap`, or the BDF is not owned by the caller (ownership comes from 418/424, `resource_registry::owner_of_bdf`).
  - `FileNotFound`: no such function.
- The full record is assembled before any write. Nothing in device or kernel state changes.

**Record** (class K, `repr(C)`, little-endian, field order = wire order):

| Off | Size | Field | Rule |
|---|---|---|---|
| 0 | 2 | `version` | exactly 1 |
| 2 | 2 | `cap_count` | 0..=16 |
| 4 | 4 | `flags` | bit0 `MSIX_PRESENT`, bit1 `MSI_PRESENT`, bit2 `MSI_FORBIDDEN_BY_FIRMWARE` (FADT IAPC_BOOT_ARCH bit 3); other bits 0 |
| 8 | 4 | `bdf` | echo |
| 12 | 2+2 | `vendor_id`, `device_id` | |
| 16 | 2+2 | `subsystem_vendor_id`, `subsystem_id` | 0 for non-type-0 headers |
| 20 | 4×1 | `class`, `subclass`, `prog_if`, `revision` | |
| 24 | 8 | `reserved0` | 0 |
| 32 | 6×24 | `bars[i] = { base u64, len u64, kind u32, bar_flags u32 }` | `kind`: 0 absent, 1 mem32, 2 mem64, 3 io. The upper half of a 64-bit BAR is reported as `kind = 0`. `bar_flags`: bit0 prefetchable, bit1 `MSIX_CARVED` (§4.3) |
| 176 | 16 | `msix = { table_size u16, table_bar u8, pba_bar u8, table_offset u32, pba_offset u32, reserved u32 }` | zero unless `MSIX_PRESENT`; `table_size` = entries (N), not N-1 |
| 192 | 8 | `msi = { control u16, reserved u16, reserved u32 }` | zero unless `MSI_PRESENT` |
| 200 | 16×24 | `caps[i] = { id u8, offset u8, len u8, reserved u8, bytes [u8; 20] }` | standard capability list (0x40..0xFF) in walk order. `len` = valid bytes: for ID 0x09, `min(cap_len_byte, 20)`; otherwise `min(20, 0x100 - offset)`. Entries ≥ `cap_count` and bytes past `len` are 0 |

**Why one record and no separate capability read:** a virtio PCI capability is 16 bytes, or 20 for the notify capability [Virtio 1.2 §4.1.4]. Inlining 20 bytes per capability covers virtio, vmxnet3 and MSI/MSI-X discovery without a raw config-read primitive. The kernel copies these bytes once at scan time. Cells never read or write config space.

**Versioning.** A future record is a new version with its own length. 434 writes only v1, and a v1 consumer refuses any other `version` (ADR record rule, `docs/abi-boundary-records.md` §1).

## 4. `AllocPcieIrq = 435`

**Envelope.** `a0 = bdf`, `a1 = want` (1..=32), `a2 = mode` (bit0 `ALLOW_MSIX`, bit1 `ALLOW_MSI`; at least one set; other bits 0); `a3..a5` must be 0.
- On success, returns `first_irq | count << 8 | kind << 16`. `kind` is 1 for MSI-X, 2 for MSI. `count` ≥ 1 and ≤ `want`; MSI rounds down to a power of two, and MSI-X is capped at the table size.
- Errors:
  - `InvalidInput`
  - `PermissionDenied`: the same gate as 434.
  - `NotSupported`: no allowed capability, or FADT forbids MSI. The driver keeps its polled path and logs `irq=polled reason=…`.
  - `OutOfMemory`: vector or IRQ-slot space is exhausted.

**4.1 Semantics.**
- The kernel allocates `count` IDT vectors and `count` contiguous logical IRQ numbers, each bound to one vector. It then:
  - programs MSI-X entries (address = destination LAPIC, data = vector) and unmasks them; or programs MSI address/data (multi-message enable);
  - sets the MSI or MSI-X enable bit and bus mastering;
  - for MSI-X, also sets `INTx disable`.
- MSI-X is preferred when both are allowed.
- The cell waits with `WaitIrq(first_irq + i, 0)`. `WaitIrq` does not change. `MAX_IRQ` grows from 64 to 256 (kernel-internal; `irq_num` is already `u8`).
- A repeat call by the same owner returns the existing allocation unchanged.

**4.2 Lifetime.**
- The allocation lives until the owning cell dies.
- The existing death path quiesces the function (`task.rs:1920-1935`: MSI off, MSI-X function-masked, BME off). Only then are the vectors and IRQ numbers freed, so a dead cell's device cannot fire into a reused vector.
- There is no free syscall in v1.

**4.3 MSI-X table protection.**
- `sys_request_mmio` on a BAR that holds the MSI-X table or PBA maps the BAR without those pages, and 434 reports `MSIX_CARVED`.
- If the table or PBA shares a page with other registers, 435 refuses MSI-X and offers MSI instead.
- **Limit (red team 1):** this does not stop a cell from making its device DMA into the MSI window (`0xFEE0_0000`), which spoofs interrupts but does not write memory. Interrupt remapping (phase 12) closes that gap. Until then, `Required` logs `msi: no interrupt remapping` at boot.

## 5. `SystemPower = 436`

**Envelope.** `a0 = action` (1 reboot, 2 poweroff); `a1..a5` must be 0.
- Gate: `SupervisorCap` (`caller_has_supervisor`, `kernel/src/task/syscall.rs:2174-2176`). Allowlist bit 49, shared with the supervisor operations (OPEN D-4).
- On success the call does not return. Reboot runs the FADT reset register, then `0xCF9`, then the 8042 pulse, then a triple fault. Poweroff writes PM1 `SLP_TYP|SLP_EN` from a static `\_S5_` package (owner decision 2026-11-03).
- Errors:
  - `InvalidInput`
  - `PermissionDenied`
  - `NotSupported`: poweroff without a static `\_S5_`, or no PM1 control block.
- Orderly shutdown (stop cells, flush VFS, then 436) is supervisor policy. The shell `reboot`/`poweroff` commands go through the supervisor, not straight to 436.

## 6. 418/424 skip-owned lookup (semantic change, wire unchanged)

**Today (E6):** the first device matching the query is offered. If a live driver owns it, the call returns 0 ("not found").

**Proposed:** the first matching device that is unowned *or already owned by the caller* is offered; devices owned by another live task are skipped.

**Effects:**
- A second instance of the same driver gets the next identical device: second xHCI (phase 11), I350 port 1 (phase 14), second NVMe/virtio disk.
- A re-query by the owner stays idempotent (it gets its own device first only if that device comes first in scan order; documented).
- A restarted driver still gets its old device back after reap, because the dead owner's claim is released on reap.

**Compatibility hazard:** a cell that relied on "not found while another instance runs" to decide it is redundant would now drive a second device. No current cell does this: each image launches one instance per driver [INFERENCE: from `scripts/x86/make-iso-ci.sh` cell lists; recheck in phase 04a]. Block-route registration is per BDF (ADR-0029 amendment), so two storage cells do not collide.

## 7. Lock handoff requested for slice S1 (phases 01-05)

File-bounded exception under CELL-CONTRACT-0024, effective when this CP1 and ADR-0031 are approved and no other slice holds the WIP lane (true on 2026-11-04: C2C Phase 03 and x86 runtime board selection are closed in `.agents/plan-portfolio.md`):

- **Kernel and HAL:**
  - `kernel/src/acpi.rs` → `kernel/src/acpi/**`;
  - `kernel/src/main.rs` (x86 init only, non-G1 branches);
  - `kernel/src/cpu_features.rs`, `kernel/src/boot/limine.rs` (MP request flags);
  - `kernel/src/task/{syscall,telemetry,smp}.rs` (smp: u32 APIC IDs only);
  - `kernel/src/task/drivers/{pcie_ecam,irq_wait,iommu,iommu_x86}.rs`, new `kernel/src/task/drivers/pcie_msi.rs`;
  - `kernel/src/resource_registry.rs` (MSI-X carve-out);
  - `kernel/src/task.rs` (death path only);
  - `hal/arch/x86/**`, `hal/soc/x86/**`.
- **ABI** (after CP1, finalized at CP2): `libs/api/src/abi/syscall.rs`, new `libs/api/src/abi/pcie_function.rs`, `libs/ostd/src/syscall.rs`.
- **Boards:** `boards/**` (new `boards/vm/x86_64-vm`).
- **Cells:** `cells/drivers/{nvme,ahci,igb,e1000,xhci,virtio-blk,virtio-net}/**`, new `cells/drivers/virtio-scsi`, `libs/virtio-pci-transport`, `libs/scsi-block`.
- **Tests and CI:**
  - `tests/integration/src/lib.rs`, `tests/integration/tests/{x86_64-boot,serial-x86,virtio-x86,nvme-x86,ahci-x86,nic-x86,igb-x86,xhci-x86,iommu-dmar-x86}.rs`;
  - `scripts/qemu-x86_64-test.sh`, `scripts/x86/{make-iso-ci.sh,make_iso.py,qemu-domain-test.sh}`;
  - `.github/workflows/ci.yml` (the `qemu-x86_64-boot` job only).
- **Docs:** `docs/hardware-compatibility-list.md` (R2), `docs/abi-boundary-records.md`, `boards/pc/x86_64-pc/README.md`, `CHANGELOG.md`.

**Constraints:**
- Default profile semantics are unchanged (`x86_64-pc` = `Required`).
- The G1 private lane (`cellos_g1_cpu`) is untouched.
- Every CELL-CONTRACT-0024 whole-plan relation stays.
- Evidence ceiling: QEMU TCG and QEMU/KVM (local).
- Phase 06 (436) and the later slices need their own handoff.

## 8. Decisions for the owner

| ID | Question | Recommendation |
|---|---|---|
| D-1 | One record with inline capability bytes (434) instead of a raw capability-read call | Yes (§3) |
| D-2 | 435 return packing `first_irq | count<<8 | kind<<16`, no free syscall in v1 | Yes (§4) |
| D-3 | 434/435 share allowlist bit 50, gated by `PcieDriverCap` at dispatch | Yes (E8; 418/424 precedent) |
| D-4 | 436 gated by `SupervisorCap`, shares bit 49 | Yes |
| D-5 | 418/424 skip-owned semantics (§6) | Yes (owner chose this on 2026-11-03) |
| D-6 | S1 lock handoff with the §7 file set | Yes |
| D-7 | Order: 434/435/418-424 are implemented in phase 04a; 436 in phase 06, behind its own handoff; CP2 per phase with the exact ABI delta and evidence | Yes |

## 9. Acceptance (maps to plan phases)

- **04a:**
  - kernel unit tests: vector allocator; carve-out ranges; MSI rounding; record layout (`size_of::<ViPcieFunctionV1>() == 584`, field offsets); skip-owned with two identical functions;
  - a throwaway smoke: NVMe admin completion by MSI-X.
- **04b:** all PCIe x86 suites under TCG/KVM log `irq=msix|msi`.
- **05:** virtio-pci over 434 capabilities, KVM default-VM acceptance.
- **06:** `reboot` and `poweroff` lanes, plus a unit test that 436 is refused without `SupervisorCap`.

## 10. Not checked

- The exact virtio notify capability layout is taken from the Virtio 1.2 spec, not from repo code.
- Whether any script launches two instances of one driver cell. Rechecked in phase 04a.
