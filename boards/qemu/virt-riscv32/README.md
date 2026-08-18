# QEMU virt riscv32

This directory is a planning placeholder for future `qemu-system-riscv32 -machine virt`
work only.

Current status:

- not implemented
- no `board.rs`
- no `BoardDescriptor`
- no `Architecture` or `SocId` variant
- no Cargo feature or build contract
- no CI lane
- no boot or runtime evidence
- not a supported Cellos board

Until 32-bit RISC-V bring-up exists, only `boards/qemu/virt-riscv64` is active.
