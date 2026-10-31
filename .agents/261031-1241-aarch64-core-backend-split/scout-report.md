# Scout report — aarch64 board coupling

Full recon: `local://arm-recon.md` (session artifact). Summary:

- About 190 `cfg(feature = "board-rpi*")` sites. `board-rpi3-bring-up` has none of its own (it is a feature composition, `kernel/Cargo.toml:137-140`).
- Coupling clusters:
  1. IRQ/timer/trap/SMP: `aarch64.rs`, `gic.rs`, `timer.rs`, `trap.rs`, `task/smp.rs`.
  2. Console/diagnostics: `main.rs`, `cfi.rs`, `mte.rs`, `uart_pl011.rs`, `console_drv.rs`.
  3. Platform/MMIO/resources: `platform.rs:159-226`, `memory/paging.rs:267-390`, `resource_registry.rs`, `gpio_irq.rs`, `mmc.rs`, `pcie_ecam.rs`, `virtio_common.rs`, `syscall.rs` framebuffer/USB.
  4. Boot/DTB/selection: `board.rs:108-203`, `boot.rs`, ARM boot asm `secondary_psci`.
  5. EL2/vCPU: `el2.rs`, `paging.rs:357-360`, `stage2_regs.rs`, `vcpu.rs`, `hypervisor/registry.rs`, `cpu_features.rs`.
- The generic traits `hal/traits/interrupt` and `hal/traits/timer` exist but have no aarch64 implementations. aarch64 calls free functions directly.
- Data profiles already exist: `hal/soc/arm-virt/src/lib.rs:78-209`, `hal/soc/bcm27xx/src/{profile,bcm2711}.rs`.
- CI covers only `virt` (`.github/workflows/ci.yml:149-321,549-653`). RPi builds are not in CI.
- No existing plan covers this split. The RPi3 EL1-drop plan (`.agents/2026-08-16-rpi3-el1-drop-fix/`) touches `boot.rs` and must not run concurrently.
