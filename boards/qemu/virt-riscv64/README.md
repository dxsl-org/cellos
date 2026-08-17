# QEMU Virt RISC-V64 Board Package

This package is the immutable fallback descriptor for the `qemu-virt-riscv64`
machine that Cellos already boots today.

It intentionally carries data only:

- board identity and `compatible` strings
- boot and firmware contract (`OpenSBI` plus DTB fallback asset)
- fallback memory map copied from `kernel/src/boot.rs`
- shared MMIO wiring for UART, PLIC, CLINT, RTC, and VirtIO
- empty pinmux and PHY wiring because QEMU virt does not need board-local muxing
- enabled shared-driver identifiers

It does not include:

- runtime parsing
- generated code
- board-local driver forks

The kernel consumes this descriptor from boot/platform code. Firmware DTB data
remains authoritative when present; this package supplies audited fallbacks for
direct-kernel boot and incomplete DTBs.
