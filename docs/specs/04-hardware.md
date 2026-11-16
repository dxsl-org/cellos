# Cellos Architecture: Hardware Layer
**Version**: 0.3 (Universal HAL & Multi-Arch Strategy)
**Status**: Definitive

---

## 1. Multi-Architecture Strategy (The `arch/` Seam)
Cellos runs on x86_64, aarch64 and riscv64. Architecture differences live in two places: the HAL crates (`hal/arch/{x86,arm,riscv}`, re-exported by `hal-core` under one feature per arch; `x86_64` is the default) and the kernel's `kernel/src/arch/`. Shared kernel code names no architecture.

Anchor: impl kernel/src/boot/platform_ready.rs::PlatformReady · impl kernel/src/board.rs::active · impl hal/arch/x86/src/x86_64/context.rs::SwitchPlan · impl hal/arch/x86/src/x86_64/paging.rs::leaf

### Kernel seam
| Piece | Where | Contract |
| :--- | :--- | :--- |
| **Arch selection** | `kernel/src/arch/mod.rs` | One `#[cfg(target_arch)]` per arch on `mod <arch>` and `pub use <arch>::*`. `build.rs` emits `cellos_wide` for the three 64-bit arches; the 32-bit code (riscv32, arm, x86) sits in `arch/legacy` under `not(cellos_wide)` and does not build. |
| **Function sets** | `kernel/src/arch/<arch>/` | Each arch exports the same sets with the same signatures: `boot`, `irq`, `clock`, `power`, `cpu`, `tlb`, `user_access`, `page_probe`, `iommu`, `hart_local`, `virtio_mmio`, `gpio_irq`, `sdhci`, `vm`, `sched`, `smp_bringup`. Single-arch modules (ACPI, VT-d, the aarch64/riscv64 `smp` fixtures, …) sit in their arch directory. Shared code calls `crate::arch::<set>::<fn>()`. A set function exists as a no-op only where the arch had no code there. |
| **Boot** | `arch::boot::early` → heap → `arch::boot::platform_init` | `platform_init` returns a typed `PlatformReady { console, irq, clock, pci, iommu, storage }`; `kmain` runs the arch-neutral tail on it. `iommu: IommuReady` is required (`Activated { isolation }`, `Deferred` or `Absent { reason }`), so the DMA-isolation path is settled before any DMA-capable Cell starts; `Activated` can only carry the `Isolation` that `iommu::activate_isolation` returns (it takes the token `iommu::init` returns), which says whether a remapper was found and translates. Interrupts are enabled only through `IrqReady::enable(ClockReady)`. |
| **Context switch** | `hal::arch::{SwitchPlan, switch, IrqSaved}` | One `switch(plan) -> IrqSaved` per arch. `SwitchPlan { out, inc, root: Option<Root>, irq }`; `IrqSaved` is `#[must_use]`, `!Send` and restores the interrupt state on drop. The kernel's `sched::domain_switch::RootDecision` chooses the root and runs its side effects; `arch::sched` holds the per-arch constants. |
| **Page tables** | `hal_paging::PageTableTrait` | One path on every arch, including the x86 shared kernel root. `hal::paging::{leaf, current_root, phys_to_virt}` serve the copy and probe paths; the kernel keeps only the raw root-register read (`arch::page_probe`). |
| **Board** | `kernel/src/board.rs` | One `board::active() -> &'static BoardDescriptor` (riscv64, aarch64: the validated compiled-in descriptor; x86_64: the profile bound at boot, which panics before binding). `board::compiled()` is the `const` form for riscv64 and aarch64; the SoC policy is `arch::{riscv64,x86_64}::board::soc()`; aarch64 reads `hal::aarch64::backend::Board`. |
| **Ratchet** | `scripts/check-hal-boundaries.sh` | Caps `target_arch` lines and `asm!` sites in `kernel/src` outside `kernel/src/arch/` (`arch_cfg_max`, `arch_asm_max`; `asm!` is 0). A new one fails CI; the caps only go down. |

### HAL traits
| Trait | Role |
| :--- | :--- |
| **`Arch`** (`hal-arch-trait`) | `init`, interrupt enable/disable/wait/query on the `ARCH` static; `Context` type. |
| **`PageTableTrait`** (`hal-paging`) | Map, unmap, protect, translate and table walks for every root. |
| **`ViUart`, `ViMmcHost`, `ViGpio`, `Framebuffer`, …** | Device-class traits for board-agnostic drivers. |

The HAL has no generic timer or interrupt-controller trait: tick sources and controllers are per-arch HAL code reached through `arch::{clock, irq}` (aarch64 boards: `hal::aarch64::backend::Board`). `hal-hypervisor` carries only `ViVmExit`; each VMM calls its arch's vCPU code directly.

## 2. Platform HAL vs. Device Driver Cells
* **Platform HAL**: Được biên dịch **cùng** Nano Kernel. Chịu trách nhiệm khởi tạo CPU, RAM và các thành phần cốt lõi.
* **Driver Cells**: Được nạp động dưới dạng **Cells**. Chịu trách nhiệm cho các ngoại vi (NIC, GPU, cảm biến Robot).

### C Driver Integration via Tier 1 `ffi-posix` runtime profile
C driver integration uses the trusted Tier 1 `ffi-posix` runtime profile
(legacy alias: Tier 1b FFI, Newlib/POSIX shim) — see
[specs/05-application.md §3](05-application.md).

## 3. Interrupt Model: `WaitIrq`
Cellos dùng mô hình ngắt chia hai nửa (amended 2026-11-16 cho khớp code):
1. **Top-Half (Kernel)**: ISR ack IRQ và đặt `IRQ_PENDING[irq]` (atomic, không lock); không có `Waker` trong kernel.
2. **Bottom-Half (Cell)**: Driver Cell chặn trong syscall `WaitIrq { irq }`; một lượt quét của scheduler đánh thức nó, và nó xử lý ngắt trong task của chính nó sau một context switch bình thường (`kernel/src/task/syscall/decode.rs:751-756`).

## 4. Resource Registry (MMIO Isolation)
Trong SAS, việc hai driver cùng ghi vào một địa chỉ phần cứng là thảm họa.
* **Registry**: `kernel/src/resource_registry.rs`. Cửa sổ MMIO hợp lệ là allowlist cố định theo target; DTB discovery chưa có (`resource_registry.rs:1-27`).
* **Exclusive Access**: Driver gọi `sys_request_mmio(base, size)` (syscall `RequestMmio`). Vùng ngoài allowlist bị từ chối; vùng đã có chủ trả `AlreadyExists`. Khi task thoát, cửa sổ được hạ về kernel-only trên mọi hart rồi mới được cấp lại.

## 5. SMP & Real-Time Affinity
* **Harts**: logical hart 0 is the boot hart and logical hart 1 the RealTime hart on
  every architecture. `MAX_HARTS` per architecture is the generated
  [arch-status](../generated/arch-status.md) table (`kernel/src/sched/smp.rs:28-43`).
  x86_64 brings every firmware-reported CPU online up to `MAX_HARTS`; logical
  2..N are additional normal harts. A CPU beyond the limit is logged `cpu N ignored:
  MAX_HARTS`; a LAPIC ID above 255 stays offline until interrupt remapping exists.
  APs come up one at a time, and the first AP that cannot be started or refuses root
  operation ends bring-up, so the online set is always `0..online`. An AP that was
  released but never answers a bring-up step within its bounded budget panics the boot
  (fail-closed); a CPU is never silently missing. After an AP is online it receives one
  more acknowledged invalidation, since shootdowns before that excluded it.
* **Work Stealing**: an idle hart steals Normal/Background work from the busiest other
  hart's queue, at most half of the stealable entries. Victim selection is a bounded
  scan (each other hart slot once, starting after the thief, ties to the first found;
  harts above 1 only while online). The move holds exactly two per-hart ready locks,
  taken in ascending hart id. RealTime tasks are never stolen and remain
  affinity-controlled; the RT hart steals and is stolen from only for Normal/Background
  work, as on two harts.
* **Cross-hart invalidation**: a TLB shootdown publishes a fresh epoch for, and sends an
  IPI to, every other online hart first, then waits over that set under one deadline.
  An unconfirmed hart never counts as acknowledged and never panics the kernel: the flush
  returns `Unconfirmed`, the frames its translations named are quarantined
  (`SharedUnconfirmed`, `TagUnconfirmed`) and a permission lowering fails closed
  (`kernel/src/memory/tlb_shootdown.rs:425-440`; Spec 02 §3.1). Retirement-switch
  waits are per-hart epochs over all hart slots.
* **Affinity**: RT routing is policy over scheduler mechanisms; it does not grant
  exclusive ownership of a core unless an explicit isolation policy proves that property.

## 6. Hang detection
Cellos không có kernel Resource-Graph deadlock detector. Shipped detection uses the
RT CPU-monopoly watchdog plus opt-in liveness heartbeats; termination flows through the
normal exit path and supervisor policy. Lock-order review and focused deadlock tests remain
the prevention layer.

---

## 7. Hardware Support Matrix — Chipsets & Drivers by Stage

> Decided 2026-06-06. Source: `.agents/reports/brainstorm-260606-2205-chipset-driver-strategy.md`

### Target platforms

| Stage | CPU arch | Dev/test | Real board |
|-------|----------|----------|-----------|
| G1 | ARM64 + RV64 | QEMU ARM virt (**QEMU-first**) | RPi 4 (BCM2711) → VisionFive2 (JH7110) |
| G1 sub-track (parked) | RV32 | none: the 32-bit code sits in `kernel/src/arch/legacy` and does not build | SiFive E21 / CHERIoT-Nano |
| G2 | RV64 | Milk-V Pioneer (X60, now) | Alibaba C930 (2026) |
| G2 | x86_64 | QEMU x86_64 | x86 PC (when G2 starts) |
| G3 | ARM64 | Radxa ROCK 5 / OrangePi 5+ (RK3588) | — |
| G3 | RV64 | — | X390-class hardware — blocked until purchasable silicon, board docs, and software stack exist |

**QEMU-first rule:** device-class HAL traits (`ViGpio`, `ViUart`, …) stay **board-agnostic**. A new board is a `boards/` descriptor plus its HAL code (on aarch64 a `hal::aarch64::backend::Board` type behind a `board-*` feature); in the kernel it touches only `kernel/src/board.rs` (descriptor selection behind `board::active()`) and `kernel/src/arch/<arch>/`. Shared kernel code does not change.

### G1 peripheral driver priority (cells/drivers/)

| Priority | Driver | Bus/IP | QEMU target | Real-board target |
|----------|--------|--------|-------------|------------------|
| P0 | GPIO cell | PL061 | `0x0903_0000` | BCM2711 GPIO / JH7110 GPIO |
| P0 | UART configure | PL011 | `0x0900_0000` | same (extend serial cell) |
| P1 | I2C cell | I2C master | _(v2 — need board)_ | BCM I2C / DW I2C |
| P1 | SPI cell | SPI master | _(v2)_ | BCM SPI / JH7110 SPI |
| P2 | PWM cell | PWM timer | _(v2)_ | BCM PWM |
| P2 | ADC cell | SPI-ADC / ADS1x | _(v2)_ | external ADC |
| P3 | CAN cell | CAN controller | _(defer)_ | MCP2515 via SPI |

### G2 server driver priority (strict order — each prerequisite for next)

```
1. PCIe ECAM host controller   — port Redox OS enum logic, adapt MmioRegion layer
2. RISC-V IOMMU (2023 spec)    — MANDATORY before NIC (SAS DMA safety)
3. NVMe driver (~3-5K LOC)     — real storage, replaces VirtIO block
4. RTL8125 / Intel i225 2.5G   — real NIC (~5-8K LOC), replaces VirtIO net
5. Intel i40e 10G              — only when inference server needs bandwidth
```

> Not planned: Mellanox mlx5 (100K+ LOC), Bluetooth/WiFi, USB xHCI before G2, full ACPI, audio.

### x86_64 PC class driver priority (planned — no physical qualification)

This list does not change the RISC-V G2 order above, and `q35-x86_64` remains the only
**qualified** x86 target — a generic `x86_64-pc` descriptor exists (phase 01) but no machine
is qualified. It records the prerequisites for the x86_64 PC/server lane
([hardware-tracks.md](../roadmap/hardware-tracks.md)). Code exists for items 1, 3 and 4
(`cells/drivers/ahci`, `cells/drivers/igb`, `kernel/src/acpi/dmar.rs`); none is physically
qualified.

```
1. AHCI/SATA storage cell     — **landed** (`.agents/261004-1957-x86-pc-lane/` 02a/02b; `ahci-x86` 5/5, two-boot persistence)
2. xHCI + HID                 — **reopened 2026-10-05** for the x86_64 PC lane (phase 03); previously frozen out by decision (item 5 of the G1-G3 driver plan phase 01)
3. Intel igb NIC (i210/i211)  — first real NIC family; e1000 today binds 82540EM and 82545EM only (`cells/drivers/e1000/src/identity.rs:5`) and fail-closes all others
4. ACPI DMAR discovery        — replaces the hardcoded q35 IOMMU base
5. Multi-port COM / RS232-485 — industrial peripherals, scheduled only after the boot path
```

> Prerequisite before any of the above is scheduled: a published HCL for the `x86_64-pc`
> board descriptor (`boards/pc/x86_64-pc/board.rs`), and an exact Intel/AMD board pair. On x86 the
> only working log and input path is a 16550-compatible UART at COM1 (`0x3F8`,
> IRQ 4) — `hal/soc/x86/src/lib.rs`, `kernel/src/arch/x86_64/boot.rs` (`early`) — so treat it as a
> must-have HCL row, including BMC serial-over-LAN on servers. Boards whose
> firmware offers no way to disable Secure Boot are **not compatible**: there is
> no signed/measured x86 boot path today and code-signing/secure-boot belongs to
> the Security track. Reopening the
> frozen USB xHCI row requires a scope decision, not an implementation patch.

---

## 8. G2: PCIe ECAM + RISC-V IOMMU Strategy

### PCIe ECAM host controller

**Approach:** Port Redox OS [`pci` crate](https://gitlab.redox-os.org/redox-os/drivers) BAR enumeration / capability parsing logic (~40-60% reuse). Rewrite MMIO access layer:

| Layer | Redox approach | Cellos adaptation |
|-------|---------------|------------------|
| MMIO access | Raw pointer cast | `ostd::mmio::MmioRegion` via Resource Registry |
| Driver isolation | Redox userspace process | Cellos Driver Cell (`#![forbid(unsafe_code)]`) |
| BAR mapping | `mmap()` syscall | `request_mmio(base, len)` → `MmioRegion` grant |
| Interrupt | Redox IRQ system | PLIC/GIC async waker dispatch (§3 above) |

Result: PCIe enumeration logic reused, Cellos safety invariants preserved.

### RISC-V IOMMU (non-optional before NIC)

In SAS, a NIC performing DMA without IOMMU control can write to **any** physical address — including kernel TCB/stack. This is a critical security invariant:

- Implement the [RISC-V IOMMU spec (ratified 2023)](https://github.com/riscv-non-isa/riscv-iommu) before installing NIC driver.
- IOMMU maps NIC's DMA window to per-Cell physical regions only.
- `request_mmio` registry extended: DMA range declared → kernel programs IOMMU page tables.

---

## 9. G3: NPU Driver Path

Two implementations are required before deciding whether a shared accelerator
contract is justified.

### Level A — Tier 1 `ffi-posix` profile (G2 work, no kernel change)

- **RKNN Runtime FFI cell** (ARM64, RK3588): trusted Tier 1 C wrapper calls `rknn_init/run/outputs_get`.
- Validates real-world `load_model / submit / wait` semantics before freezing trait contract.
- Prerequisite: Tier 1 `ffi-posix` entropy shim + net shim (see [05-application.md §3](05-application.md)).

### Level B — Kernel NPU scheduler (G3)

Do not draft a concrete trait shape until at least two months of RKNN work on
real RK3588 hardware and a second implementation have produced comparable
evidence. Record model-context lifetime, submission/completion behavior,
capability discovery, buffer ownership, cancellation, and failure semantics as
observations; do not turn those observations into Law-1 types or methods here.

### Level B+ — SiFive X390 VCIX (second impl)

Do not start an X390 VCIX driver cell from an availability forecast. The
[Phase 06 evidence gate](../research/g3-accelerator-evidence.md) first requires
purchasable silicon, board documentation, and a usable software stack. Only
after that hardware evidence may RKNN and X390 observations be compared to
decide whether a shared accelerator contract is justified.

### Level C — Zero-copy tensor pipeline (G3, after sys_grant_pages)

`sys_grant_tensor` + `TensorBuffer` dual-domain (CpuRam/NpuDram) — prerequisite: `sys_grant_pages` large-buffer IPC (G2 extension). Do not design before Level B is validated.
