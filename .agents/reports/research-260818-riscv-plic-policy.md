## Hardcoded IRQ window is still inside the shared PLIC mechanism
**Verdict:** The next HAL split should remove fixed device IRQ numbers from `hal/arch/riscv`; shared mechanism must consume runtime board/platform data instead.
- `hal/arch/riscv/src/common/plic.rs` enables fixed IRQs `1..=8` plus UART IRQ `10` for PLIC context `1`.
- The root board descriptor already owns those concrete values: QEMU RV64 stores UART IRQ `10` and VirtIO MMIO IRQs `1..5`.
- `kernel/src/platform.rs` already exposes `uart_irq` and `[Option<VirtioEntry>; 8]`, so the mechanism does not need any new per-board driver copy to enable the right sources.
- Project docs explicitly say shared drivers stay single-copy and `hal/soc/riscv` must stay data-only.
**Source:** `hal/arch/riscv/src/common/plic.rs:90`, `boards/qemu/virt-riscv64/board.rs:38`, `kernel/src/platform.rs:25`, `docs/system-architecture.md:58`

## Trap dispatch still bakes QEMU/JH7110 routing policy into arch code
**Verdict:** `rv64/trap.rs` still mixes mechanism with board/SoC IRQ classification; dispatch must switch to runtime lookups, not fixed ranges.
- The S-mode external interrupt path routes `1..=8` to `vi_handle_virtio_irq` and `10` to `vi_handle_uart_irq`.
- The same file claims and completes only PLIC context `1`, so the trap path currently assumes one exact interrupt layout and one exact context.
- `platform.rs` already parses UART IRQs and VirtIO slots from DTB or board fallback, so classification can move to a small runtime helper without duplicating any driver.
- The current plan already marked this exact PLIC policy as deferred technical debt.
**Source:** `hal/arch/riscv/src/rv64/trap.rs:102`, `kernel/src/platform.rs:248`, `.agents/260818-0513-hal-soc-riscv-profiles/plan.md:63`

## Unclaimed VirtIO ACK path reconstructs MMIO base from IRQ using a QEMU formula
**Verdict:** This is the sharpest policy leak after `trap.rs`; it should lookup slot base by configured IRQ, not derive `0x1000_1000 + (irq-1)*0x1000`.
- `kernel/src/task/drivers/virtio_common.rs` special-cases RV64 IRQs `1..9` and rebuilds a base address from the IRQ number.
- That bypasses both DTB discovery and board fallback data even though `virtio_slots()` already returns `(base, irq)` pairs from `platform::PLATFORM`.
- If a board keeps VirtIO MMIO but renumbers IRQs or holes the window, the shared ACK path becomes wrong while the descriptor data remains correct.
- This change stays DRY: one generic `irq -> base` lookup in `virtio_slots()` replaces a hardcoded arithmetic branch.
**Source:** `kernel/src/task/drivers/virtio_common.rs:121`, `kernel/src/task/drivers/virtio_common.rs:24`, `kernel/src/platform.rs:308`, `boards/qemu/virt-riscv64/board.rs:62`

## PLIC context selection is still single-hart policy, not pure mechanism
**Verdict:** The smallest SoC-side extraction is a tiny context policy, not a new board driver.
- `plic.rs` documents and enforces “Hart 0 S-mode -> Context 1”.
- `rv64/trap.rs` hardcodes `PLIC.claim(1)` and `PLIC.complete(1, irq)`.
- `kernel/src/task/smp.rs` already brings up logical hart `1`, restores a hart-specific vector, and enables external interrupts; if future routing delivers external IRQs there, claiming context `1` is structurally wrong.
- This is SoC/integration policy, not per-board pinmux: a minimal `PlicContextPolicy` in `hal/soc/riscv` is enough, for example `s_mode_context_base` + `stride`, or a `context_for_logical_hart(hart)` helper.
**Source:** `hal/arch/riscv/src/common/plic.rs:12`, `hal/arch/riscv/src/rv64/trap.rs:176`, `kernel/src/task/smp.rs:244`

## CLINT does not need to expand this slice
**Verdict:** Do not widen the next checkpoint into CLINT/timer work; the current debt is PLIC routing, not timer mechanism.
- `hal/soc/riscv` already carries CLINT compatible families as data.
- `platform.rs` already resolves `clint_base` from DTB or board fallback.
- The running timer path relies on SBI timer setup in SMP/bootstrap code, not board-specific CLINT IRQ wiring in the shared trap dispatcher.
**Source:** `hal/soc/riscv/src/catalog.rs:4`, `kernel/src/platform.rs:261`, `kernel/src/task/smp.rs:256`

## Ranked extraction boundary
**Verdict:** Rank 1 is runtime IRQ routing from `PlatformInfo`; Rank 2 is a tiny `hal/soc/riscv` PLIC context policy; avoid any board-specific PLIC driver fork.
- Rank 1: replace `1..=8`, `10`, and `0x1000_1000 + ...` with helpers that read `platform::with(|p| p.uart_irq / p.virtio_mmio)` and enable/dispatch/ACK from that runtime map.
- Rank 2: add a data-only `PlicContextPolicy` to `RiscvSocProfile` for `logical_hart -> S-mode context` mapping; keep MMIO addresses and IRQ lists in `boards/` and `PlatformInfo`.
- Rank 3: only after the above, consider consuming `enabled_drivers` for build/boot gating; it is not required to cleanly de-hardcode the shared PLIC path.
- Do not put this into `boards/` alone: concrete device IRQs are already board data there, but PLIC context numbering is a SoC/controller integration fact and belongs with the RISC-V SoC profile.
**Source:** `kernel/src/platform.rs:174`, `boards/src/descriptor.rs:59`, `hal/soc/riscv/src/profile.rs:3`, `docs/system-architecture.md:52`

## Compatibility risks if left as-is
**Verdict:** The current path is safe only while boards mimic QEMU/VF2 IRQ numbering and external IRQs stay on hart 0.
- SG2042 currently avoids most fallout only because the SoC profile disables MMIO UART/RTC/VirtIO, shrinking the active IRQ surface.
- Any future board with valid PLIC + NS16550/VirtIO MMIO but different IRQ numbering will parse correct DTB/descriptor data and still dispatch the wrong handlers.
- Any future multi-hart external-IRQ delivery will claim/complete the wrong context from secondaries.
- Because the mechanism is shared, every new board added under the current model increases hidden coupling instead of reusing the descriptor split.
**Source:** `hal/soc/riscv/src/catalog.rs:26`, `kernel/src/platform.rs:83`, `hal/arch/riscv/src/rv64/trap.rs:179`

## Focused test matrix for the next slice
**Verdict:** The next checkpoint needs mostly pure unit tests plus existing RV64 compile/QEMU gates; hardware bring-up is not required to prove the extraction.
- Unit: pure helper for `irq -> route` covers UART hit, VirtIO hit, unknown IRQ, and SG2042 empty-virtio/zero-UART cases.
- Unit: pure helper for `irq -> VirtIO base` covers DTB order, board fallback order, and non-contiguous IRQ numbers so the old arithmetic assumption cannot regress.
- Unit: `PlicContextPolicy` covers hart `0 -> 1` and hart `1 -> 3` for current SiFive-style layouts; combined-feature precedence (`board-pioneer` over `board-vf2`) should keep returning the intended profile.
- Compile: RV64 default, `--features board-vf2`, `--features board-pioneer`, and combined-feature build.
- Smoke: existing RV64 QEMU boot plus one VirtIO-triggering path remains sufficient to prove the shared driver still works after de-hardcoding.
**Source:** `hal/soc/riscv/src/tests.rs:3`, `kernel/src/platform.rs:174`, `kernel/src/task/drivers/virtio_common.rs:159`, `hal/arch/riscv/src/common/plic.rs:92`
