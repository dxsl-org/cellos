## Central ABI crate covers the HAL↔kernel hook surface
**Verdict:** `hal/traits/arch/src/kernel_abi.rs` is the right single source for the current HAL-owned Rust ABI hooks.
- It defines shared trap-frame layouts plus one declaration site for 12 exported hook symbols, including the RV32/RV64 split for `ViCell_syscall_dispatch`.
- Every touched HAL callsite now imports these declarations from `hal_arch_trait` instead of carrying local `extern "Rust"` blocks.
- Kernel definitions are tied back to the shared signatures with `const _: crate::hal::<Type> = symbol;` assertions for timer, fault, cell-id, syscall, PLIC, UART, VirtIO, GPIO, and AArch64 fault hooks.
**Source:** hal/traits/arch/src/kernel_abi.rs:1-124

## One kernel symbol is still outside the typed assertion net
**Verdict:** `vi_handle_page_fault` is still declared centrally but not compiler-asserted back to `crate::hal::HandlePageFault`, so the x86 page-fault hook remains the last silent-signature risk in this slice.
- The shared ABI crate declares `HandlePageFault` and `vi_handle_page_fault(...)`.
- The x86 IDT path calls that symbol through the shared import.
- The kernel exports `vi_handle_page_fault(...)`, but this commit adds no `const _: crate::hal::HandlePageFault = vi_handle_page_fault;` assertion beside the definition.
**Source:** hal/traits/arch/src/kernel_abi.rs:80-83; hal/traits/arch/src/kernel_abi.rs:121-123; hal/arch/x86/src/x86_64/idt.rs:178-204; kernel/src/memory/paging.rs:933-935

## The branch is clean for rv64, aarch64/RPi3, and x86_64, but rv32 remains red for older reasons
**Verdict:** This ABI refactor does not regress the three active lanes, and RV32 is still blocked by pre-existing kernel debt rather than the new central ABI file.
- CI already installs and builds the three active targets `riscv64gc-unknown-none-elf`, `aarch64-unknown-none-softfloat`, and `x86_64-unknown-none`.
- Local verification on this branch passed `cargo check -p hal-arch-trait`, `cargo check -p cellos-kernel --target x86_64-unknown-none`, `--target riscv64gc-unknown-none-elf`, and `--target aarch64-unknown-none-softfloat --features board-rpi3`.
- RV32 still fails in unrelated codepaths (`hal::paging` missing, `AtomicU64` missing, `u32`→`usize` mismatches), so it should stay outside the acceptance gate for this slice.
**Source:** .github/workflows/ci.yml:108-148; docs/TODO.md:23-25

## Two file changes are noise and should not ship with the ABI slice
**Verdict:** `docs/TODO.md` and `__build.bat` are unrelated to the single-source ABI objective and should be split out or dropped before merge.
- `docs/TODO.md` rewrites backlog ordering and adds a separate `BUG` section; none of that is required to centralize hook signatures.
- `__build.bat` changes only the missing trailing newline.
- Shipping them together makes the ABI fix harder to review and raises merge-churn risk for no runtime benefit.
**Source:** docs/TODO.md:1-40; __build.bat:1-4

## Ranked recommendation and minimum gate
**Verdict:** Rank 1 is “keep the central ABI crate, add the missing x86 page-fault type assertion, and merge only code-path changes”; every broader option costs more review surface than it buys.
- Rank 1: code-only ABI commit plus `HandlePageFault` assertion. Fit: exact match to the existing `boards -> hal/soc -> hal/arch` layering and DRY goal. Risk: low; uses current Rust type-checking only.
- Rank 2: macro-generate declarations and assertions for all hook pairs. Fit: acceptable later, but over-engineering now because the hook set is small and already centralized.
- Rank 3: keep per-arch `extern "Rust"` declarations and rely on CI/runtime smoke. Fit: poor; `docs/TODO.md` already records that rustc/linking do not catch cross-crate signature drift.
- Minimum acceptance gate: pass the three active compile lanes above, keep RV32 explicitly excluded as known debt, and rerun the existing x86 QEMU boot smoke because CI already treats that as the only runtime lane among these architectures.
**Source:** docs/TODO.md:12-25; .github/workflows/ci.yml:638-709
