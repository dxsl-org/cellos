# Slice S4 (phases 10, 11, 12) — lock-handoff request

Status: **pending owner decision (checklist §6).** Written 2026-11-07 against HEAD.

## 1. Recon

### 1.1 Phase 10: Console beyond COM1 (SPCR, COM2/MMIO 16550, Framebuffer text console)

- **Firmware SPCR:**
  - `kernel/src/acpi/spcr.rs` already parses the ACPI SPCR table into `SpcrInfo`: `interface_type` (0x00=16550, 0x01=16450, 0x12=16550-compatible GAS), `base: Gas` (space_id: 0=MMIO, 1=I/O port), `gsi: u32`, `baud_code: u8`, `uart_clock_hz: u32`.
  - Tested in `kernel/src/acpi/tests/tables.rs:300-324`.
- **UART parameterisation (`hal/arch/x86/src/x86_64/uart_16550.rs`):**
  - Currently hardcodes `port_base()` to I/O ports. Parameterise with `UartAddress` (I/O port vs MMIO virtual address) and register stride (1 byte for legacy I/O, 4 bytes for MMIO 32-bit registers).
  - Default unchanged (COM1 0x3F8 I/O port, stride 1). Early boot uses COM1 as before.
- **Console selection policy (`kernel/src/task/drivers/console_select.rs`):**
  - After ACPI parse in `kmain`, select active console (first match wins):
    1. Command line `cellos.console=` (`ttyS0..3`, `mmio@0x...`, `fb`);
    2. SPCR table (if present and 16550-family);
    3. COM1 (if probed present);
    4. Framebuffer console.
- **Framebuffer text console (`kernel/src/task/drivers/fb_console.rs`, `fb_font.rs`):**
  - Uses Limine linear framebuffer mapped in `kernel/src/main.rs:1523-1566`.
  - Embedded 8×16 public-domain bitmap font (`fb_font.rs`).
  - Text buffer and scrolling; integrated into `DirectWriter` in `kernel/src/task/drivers/uart.rs`.
  - Handover to display cell when `service::COMPOSITOR` activates (`reserve/activate_display_framebuffer`).

### 1.2 Phase 11: xHCI for physical machines (BIOS handoff, hubs, multi-controller)

- **USBLEGSUP BIOS handoff:**
  - Walk xECP capability list for USB Legacy Support (capability ID 1).
  - Set OS-owned semaphore bit, wait up to 1 s for BIOS-owned to clear (force on timeout and log), clear SMI enables in USBLEGCTLSTS.
- **Hub class:**
  - USB 2.0 hub class descriptor (`GET_HUB_DESCRIPTOR`), per-port power/reset/status.
  - Route string + root hub port + TT info in slot context for downstream devices.
- **Multiple controllers:**
  - One driver cell per controller: each calls `FindPcieDevice(0C/03/30)` and binds the next controller not owned by a live driver.
  - Input flows through existing `service::USB_HID_PRODUCER` (opcode 423).

### 1.3 Phase 12: VT-d multi-unit, RMRR, interrupt remapping

- **Multi-DRHD units (≤ 16):**
  - `VtdUnit` instances from DMAR. Device → unit lookup by scope: endpoint, bridge subtree, then INCLUDE_PCI_ALL unit.
  - Domain per cell per unit; `GrantDma` maps into the unit owning the BDF.
- **RMRR identity mapping:**
  - Identity-map RMRR ranges for scoped devices (USB, iGPU) before enabling translation.
  - Unblocks native Intel iGPU compute (ADR-0033 condition 3).
- **Interrupt remapping (IR):**
  - IRTA table per unit (≤ 4096 IRTEs), EIM when x2APIC is active.
  - Remappable-format MSI and IOAPIC RTEs. Blocks forged MSIs.
  - Lift APIC ID > 255 limitation when IR is active.

## 2. Exact file set

| Path | Purpose |
|---|---|
| `hal/arch/x86/src/x86_64/uart_16550.rs` | Parameterised UART addressing (I/O vs MMIO, stride 1 vs 4) |
| `kernel/src/task/drivers/console_select.rs` (C) | Console selection policy + SPCR / cmdline resolution |
| `kernel/src/task/drivers/fb_font.rs` (C) | Public-domain 8×16 bitmap font table |
| `kernel/src/task/drivers/fb_console.rs` (C) | Framebuffer text console renderer + scrolling |
| `kernel/src/task/drivers/console_drv.rs` | Output sink integration + input source dispatch |
| `kernel/src/task/drivers/uart.rs` | DirectWriter dispatch to selected console / framebuffer |
| `kernel/src/main.rs` | Hook console selection after ACPI parse and framebuffer setup |
| `kernel/src/board.rs` | Parse `cellos.console=` from cmdline |
| `tests/fixtures/acpi/spcr-com2.bin` (C) | SPCR test table fixture for COM2 redirection |
| `tests/integration/tests/console-x86.rs` (C) | QEMU SPCR and framebuffer console integration test |
| `cells/drivers/xhci/src/legacy.rs` (C) | xHCI USBLEGSUP BIOS handoff |
| `cells/drivers/xhci/src/hub.rs` (C) | USB hub class descriptor & port management |
| `cells/drivers/xhci/src/topology.rs` (C) | Route string and slot topology mapping |
| `cells/drivers/xhci/src/{main.rs, controller.rs}` | Multi-controller discovery & hub integration |
| `kernel/src/task/drivers/iommu_x86/**` | Split multi-unit VT-d: `mod.rs`, `unit.rs`, `context.rs`, `domain.rs`, `invalidate.rs`, `rmrr.rs`, `ir.rs` |
| `kernel/src/task/drivers/pcie_msi.rs` | Remappable MSI format under active IR |
| `hal/arch/x86/src/x86_64/apic.rs` | IOAPIC remappable format under active IR |
| `tests/integration/tests/vtd-ir-x86.rs` (C) | VT-d multi-unit & interrupt remapping integration test |
| `docs/hardware-compatibility-list.md`, `CHANGELOG.md` | HCL R1 reworded; changelog entry |

## 3. Law-1 items

- **No Law-1 delta:**
  - No syscall numbers, allowlist bits, service IDs, or ABI records change.
  - Console selection, SPCR, and framebuffer text console are kernel-internal.
  - xHCI driver changes are driver-internal; input arrives through existing `service::USB_HID_PRODUCER` (423).
  - VT-d multi-unit, RMRR, and IR are kernel-internal IOMMU enforcement; user cells continue using `GrantDma` (233) and `AllocPcieIrq` (435).

## 4. Overlap and Lock Relations

- S4 takes the Intel single-WIP lane (S3 is merged and closed).
- Unblocks ADR-0033 condition 3 (Phase 12 VT-d multi-unit / RMRR) for the native Intel iGPU compute lane.
- All 38 `CELL-CONTRACT-0024` lock relations remain preserved.

## 5. Owner Decision Checklist

| ID | Item | Recommended |
|---|---|---|
| H-S4-1 | Approve S4 file-bounded exception for phases 10, 11, 12 | Yes |
| H-S4-2 | Order of implementation: Phase 10 (Console) → Phase 11 (xHCI) → Phase 12 (VT-d/IR) | Yes |
| H-S4-3 | Confirm no Law-1 ABI delta (all interfaces kernel/driver internal) | Yes |
| H-S4-4 | Framebuffer console font: embed public-domain 8×16 bitmap font in `fb_font.rs` | Yes |
