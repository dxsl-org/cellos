# 2026-09-16 — Tier 1 Rust `std` PAL In-Tree Implementation

## What happened
Unblocked `PAL-IMPLEMENTATION-CHECKPOINT` following Phase 03 baseline ledger recording. Designed, implemented, and verified all 5 phases of the Tier 1 Rust `std` PAL in-tree implementation, enabling Rust `std` cell binaries on CellOS Tier 1 Real-time SAS across RISC-V 64, AArch64, and x86_64.

## Decisions
- Ephemeral sysroot overlay via `__CARGO_TESTS_ONLY_SRC_ROOT` over `rust-src`: builds standard library without toolchain poisoning or vendoring full rustc tree in git.
- Self-contained 1 MiB per-cell static heap allocator inside `sys/pal/cellos/alloc.rs`: avoids modifying `libs/ostd/src/heap.rs`, protecting the feasibility manifest sha256 digest while satisfying `PAL-007`.
- Strict No Ambient Authority: `fs`, `net`, `process`, and `thread::spawn` fail closed with `io::ErrorKind::Unsupported`.
- Security primitives: `PAL-019` enforces zero/error fail-closed on entropy failure; `PAL-031` validates buffer pointer bounds before syscalls.

## Lessons
- Cargo's internal `__CARGO_TESTS_ONLY_SRC_ROOT` environment variable allows point-in-time redirection of standard library crate sources for `-Z build-std`.
- Adding `target_os == "cellos"` to `library/std/build.rs` eliminates the `restricted_std` compilation error for custom targets.

## Next steps
- Hardware qualification on physical Raspberry Pi 3 Model B+ (USB Policy v3, Level IRQ 9, I2C/SPI sensors).
- ABI freezing and Law 1 double-confirmation for G2 Level A AI inference interface.
