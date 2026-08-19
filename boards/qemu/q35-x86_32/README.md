# QEMU q35 x86_32 placeholder

This directory is a planning placeholder for future 32-bit x86
`qemu-system-i386 -machine q35` work only.

Current status:

- not implemented
- no `board.rs`
- no `BoardDescriptor`
- no Cargo feature or build contract
- no CI lane
- no BIOS or UEFI boot evidence
- not a supported Cellos board

Terminology contract:

- Cellos board taxonomy: `x86_32`
- HAL module name: `x86_32`
- Rust target triple: `i686-unknown-none`
- QEMU launcher: `qemu-system-i386 -machine q35`

Until 32-bit x86 bring-up exists, only `boards/qemu/q35-x86_64` is active.
