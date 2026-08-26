## RISC-V arch layer is mostly clean already
**Verdict:** Do not split `hal/arch/riscv` broadly; only one part is still carrying platform policy.
- `hal/core` just re-exports the active architecture crate; it has no SoC seam today and does not know about boards or DTB policy (`hal/core/src/lib.rs:29-46`).
- `hal/arch/riscv/src/rv64.rs` owns trap setup, CSR interrupt enable, and context switching; that is architecture work, not SoC work (`hal/arch/riscv/src/rv64.rs:34-77`).
- `hal/arch/riscv/src/rv64/trap.rs` is likewise architecture-generic except that it assumes PLIC context `1`, which is still acceptable for the current single-hart S-mode contract (`hal/arch/riscv/src/rv64/trap.rs:102-194`).
- The real layering leak inside HAL is `common/plic.rs::init()`, which hard-codes the QEMU/JH7110 interrupt policy (`1..=8` VirtIO + `10` UART) instead of just exposing PLIC mechanics (`hal/arch/riscv/src/common/plic.rs:90-102`).
**Source:** `hal/core/src/lib.rs:29-46`, `hal/arch/riscv/src/rv64.rs:34-77`, `hal/arch/riscv/src/rv64/trap.rs:102-194`, `hal/arch/riscv/src/common/plic.rs:90-102`

## Current SoC glue lives in kernel, not HAL
**Verdict:** `kernel/src/platform.rs` is the present extraction target for `hal/soc`, not the whole RISC-V HAL.
- DTB parsing, compatible-string fallback, VirtIO slot collection, and QEMU-default construction are all in `kernel/src/platform.rs` (`kernel/src/platform.rs:38-58`, `185-319`).
- `board-pioneer` injects SG2042-specific policy here: force SBI console, zero RTC, and wipe VirtIO because the SoC MMIO addresses are sv39-inaccessible (`kernel/src/platform.rs:81-110`).
- T-Head C900 PLIC/CLINT compat aliases also live here, which is SoC data, not generic kernel logic (`kernel/src/platform.rs:211-227`).
- `board-vf2` does not touch `platform.rs`; its JH7110 difference is only fallback DRAM geometry in `boot.rs`, which is board/firmware contract, not SoC driver logic (`kernel/src/boot.rs:291-318`).
**Source:** `kernel/src/platform.rs:38-58`, `kernel/src/platform.rs:81-110`, `kernel/src/platform.rs:185-319`, `kernel/src/boot.rs:291-318`

## Board and SoC boundaries are already defined in docs and code
**Verdict:** The intended architecture is explicit; the repo is only halfway there for RV64.
- The board descriptor contract already matches the desired board-only payload: identity, compatibles, boot contract, fallback memory, wiring, and enabled shared drivers (`boards/src/descriptor.rs:29-74`, `docs/system-architecture.md:47-54`).
- QEMU RV64 has been migrated into that model as immutable data only (`boards/qemu/virt-riscv64/board.rs:95-115`).
- The repo policy says hardware support should be layered architecture → SoC family → board, and boards should not own duplicated driver code (`docs/baremetal/debug.md:75-83`, `100-106`).
- `hal/soc/` was intentionally deferred until there was “real SoC glue to extract”; the SG2042/JH7110 leftovers now qualify (`docs/system-architecture.md:53-54`).
**Source:** `boards/src/descriptor.rs:29-74`, `boards/qemu/virt-riscv64/board.rs:95-115`, `docs/baremetal/debug.md:75-83`, `docs/baremetal/debug.md:100-106`, `docs/system-architecture.md:47-54`

## Git history says VF2 and Pioneer were bolt-ons, not a clean layer
**Verdict:** The historical churn confirms that the next reversible split is “extract SoC policy,” not “move more board data first.”
- `5d5a1de8` added `board-vf2` as a fallback-memory special case and explicitly noted that JH7110 reused the QEMU virt PLIC/UART/CLINT addresses, so no HAL driver fork was needed.
- `f42253cf` added Pioneer by patching `platform.rs` with T-Head compat strings plus the `uart_base = 0` / `rtc_base = 0` / `virtio_mmio = [None; 8]` quirk block.
- `c0096ade` then moved only the audited QEMU RV64 fallback data into `boards/`, leaving the real-board quirks in `kernel/src/platform.rs` and `kernel/src/boot.rs`.
**Source:** `git show --stat --summary 5d5a1de8`, `git show --stat --summary f42253cf`, `git show --stat --summary c0096ade`

## Ranked options
**Verdict:** Only option 1 is worth doing now.
- `#1 Best fit:` add a small `hal/soc/riscv` crate for immutable SoC profiles and quirk metadata, then make `kernel/src/platform.rs` consume it. Fit `5/5`, churn `2/5`, regression risk `2/5`, reversibility `5/5`.
- `#2 Acceptable later:` migrate VF2/Pioneer into new board descriptors first. That is larger churn because VF2 fallback DRAM size is board-level, not SoC-level, so the split would still leave the real SoC policy problem unsolved. Fit `3/5`, churn `4/5`, regression risk `3/5`.
- `#3 Reject:` fork PLIC/UART/RTC/SDHCI code per board or per SoC. That directly violates the board contract and current docs. Fit `0/5`, churn `5/5`, regression risk `5/5`.
**Source:** `docs/baremetal/debug.md:75-83`, `docs/system-architecture.md:47-54`, `kernel/src/platform.rs:81-110`, `kernel/src/boot.rs:291-318`

## Smallest independently reversible `hal/soc` slice
**Verdict:** Extract only RISC-V SoC identification data and runtime-access quirks; leave memory maps and device drivers where they are.
- Add `hal/soc/riscv/Cargo.toml` and `hal/soc/riscv/src/lib.rs`, then register the crate in workspace `Cargo.toml`.
- In that crate, define immutable profiles for `generic-virt`, `jh7110`, and `sg2042`: compatible aliases for UART/PLIC/CLINT/RTC lookup, plus access policy flags such as `uart_access = Mmio | SbiDbcnOnly`, `rtc_access = Mmio | Unavailable`, and `virtio_mmio = Present | Absent`.
- Update only `kernel/Cargo.toml` and `kernel/src/platform.rs` to replace hard-coded compat arrays and the Pioneer quirk block with calls into those SoC profiles.
- Do **not** move `kernel/src/boot.rs` fallback memory tables in this slice. QEMU and VF2 fallback RAM geometry is board/firmware contract, not SoC mechanism.
- Do **not** move `hal/arch/riscv/src/common/{plic,rtc,uart_ns16550a}.rs` in this slice. They are shared IP-block drivers; the leak is policy, not the MMIO mechanics.
**Source:** `kernel/src/platform.rs:81-110`, `kernel/src/platform.rs:203-233`, `kernel/src/boot.rs:242-318`, `hal/arch/riscv/src/common/plic.rs:90-102`

## Exact contracts and file touch list
**Verdict:** Keep the first contract scalar-only so HAL does not depend on kernel types.
- New contract should be data-only, for example: `RiscvSocProfile { uart_compatibles, plic_compatibles, clint_compatibles, rtc_compatibles, uart_access, rtc_access, virtio_mmio_support }`.
- Avoid passing `PlatformInfo` or `VirtioEntry` into `hal/soc`; that would invert the dependency and make the seam fake.
- Expected files: `Cargo.toml`, `kernel/Cargo.toml`, `kernel/src/platform.rs`, `hal/soc/riscv/Cargo.toml`, `hal/soc/riscv/src/lib.rs`. Nothing else is required for the first reversible slice.
- Nice-to-have but not required in the same commit: a tiny unit test module inside `hal/soc/riscv` that checks SG2042 exposes `SbiDbcnOnly` and no VirtIO.
**Source:** `kernel/src/platform.rs:23-35`, `kernel/src/platform.rs:271-319`, `docs/system-architecture.md:47-54`

## Configuration risks to watch
**Verdict:** The biggest risk is recreating the same feature sprawl in a new directory.
- Guard the new crate with both `target_arch = "riscv64"` and explicit kernel dependency scoping; `hal/core` already shows why feature-only gating leaks across workspace builds (`hal/core/src/lib.rs:25-32`).
- Keep SoC selection in one place. If `board-vf2` and `board-pioneer` choose profiles in both kernel and `hal/soc`, the split gets worse, not better.
- Do not treat QEMU `virt` as a real SoC family outside this narrow profile role; it is a machine fallback, not silicon. Use it only as the generic RISC-V baseline profile.
- Leave `PLIC::init()` untouched unless you also move the interrupt-source policy into the SoC profile; otherwise a “cleanup” risks silently changing which IRQ lines get enabled.
**Source:** `hal/core/src/lib.rs:25-32`, `hal/arch/riscv/src/common/plic.rs:90-102`, `kernel/Cargo.toml:39-46`, `kernel/Cargo.toml:86-96`

## Verification gate for this slice
**Verdict:** The slice is safe if it clears compile coverage on the three RV64 lanes; runtime proof only needs the existing QEMU virt lane.
- `cargo fmt --all --check`
- `cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf`
- `cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf --features board-vf2`
- `cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf --features board-pioneer`
- `cargo build --release -p cellos-kernel --target riscv64gc-unknown-none-elf`
- `scripts/qemu-boot-test.sh target/riscv64gc-unknown-none-elf/release/cellos-kernel`
**Source:** `docs/project-roadmap.md:27`, `docs/project-changelog.md:5-11`
