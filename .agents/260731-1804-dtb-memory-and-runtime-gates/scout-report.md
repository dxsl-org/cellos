# Scout Report

## Relevant Files

- `kernel/src/main.rs:92` receives the firmware DTB; `kernel/src/platform.rs:106` separately resolves a Limine DTB today.
- `kernel/src/boot.rs:235` gives RV64 QEMU only 190 MiB usable RAM; `fallback_boot_info` ignores the RV64 DTB.
- `kernel/src/memory/frame.rs:49` selects the largest `Usable` interval, so map entries must never overlap protected ranges.
- `kernel/src/memory/paging.rs:87` maps `Usable`, `Kernel`, and `Bootloader` entries and skips `Reserved` entries.
- `kernel/linker.ld:61` exposes `__stack_top`, the runtime upper bound for the loaded kernel and boot stack.
- `tests/integration/src/lib.rs` hardcodes RV64 QEMU memory and needs a parameterized boot helper.
- Phase 09 lives in `kernel/src/policy.rs`, `kernel/src/task/cap.rs`, `kernel/src/audit.rs`, and `scripts/sign-policy.py`.
- Phase 11's runtime proof uses `scripts/test-cell-signing.sh`, `scripts/cellos-sign`, and the image build path.

## Patterns and Constraints

- Use half-open physical intervals and checked arithmetic; align usable ranges inward and protected ranges outward.
- Parse DTB semantics in the boot layer. The allocator and paging code should continue consuming normalized entries.
- Preserve all `/memreserve/` entries and enabled static `/reserved-memory` children; reject dynamic reservations.
- A generated-map overflow is a hard fallback, never a truncated partial map.
- Resolve one effective DTB pointer and pass it to CPU, platform, and boot-memory consumers.
- Final runtime evidence must identify commit, command, artifact hash/path, pass count, and decisive log markers.

## Precedents

- `86c1fcb5` reads AArch64 DTB RAM and uses `__stack_top`; useful for kernel-span sizing but not reservation subtraction.
- `13bea199` fixed a prior memory-map corruption where an undersized kernel reservation reissued live image frames.
- `3afd524c` already proves a normal RV64 policy boot with a complete policy and zero strip events.
- `13d5c5f6` implements F1/F5 signing; `f8eb7525` plus the preserved W^X run supplies its missing image-runtime proof.

## Prior Failures

- Build scripts may return success despite an inner cargo failure; scan output for `FATAL`.
- The installed toolchain uses `riscv64-unknown-elf-*`; current C build scripts may request `riscv-none-elf-*`.
- Linux integration tests require `--target x86_64-unknown-linux-gnu`.
- The shell prompt has no newline; automation should synchronize on `=== ViCell shell ready ===`.
- Existing temporary worktrees lag `976a6ac2`; their artifacts are not final-head certification artifacts.

## Blast Radius

- Boot contract: DTB selection, memory normalization, frame allocation, and identity paging.
- Board compatibility: QEMU virt, VisionFive2, and Pioneer RV64 fallbacks.
- Security: an overlap can allocate OpenSBI, kernel, DTB-reserved, or firmware-reserved frames.
- Runtime gates: operator-policy fail-closed behavior and signed-cell image admission.

## Deferred

- Multi-region frame allocation, RV32 DTB support, A2, A3, D8+, and build-script cleanup are not part of this plan.
