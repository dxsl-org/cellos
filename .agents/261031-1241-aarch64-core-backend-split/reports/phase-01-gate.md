# Phase 01 gate — backend trait and console/diagnostics

**Run:** 2026-11-02, working tree on HEAD `75dbc8655` (docs-only on top of the baseline commit `6d5cdeb0c`), uncommitted.
**Reference:** [baseline-2026-11-02.md](baseline-2026-11-02.md) (Q8: the dated name is kept).
**Result:** **PASS** on every gate row.
**Evidence ceiling:** QEMU 8.2.2 TCG (`virt`, `raspi3b`) on WSL2; RPi4 compile/size only. No physical claim.

## 1. Change

### Files
New:
- `hal/arch/arm/src/aarch64/backend/mod.rs`
  - The `Board` alias, selected by the only `board-rpi*` cfg ladder in this phase's files. `board-rpi3` takes precedence over `board-rpi4`, so `hal-arm` still compiles with both features and the kernel's conflict `compile_error!` stays the reported error.
  - `ArmPlatform`, the `ArmIommu` design hook, and `NoIommu`.
- `backend/qemu_virt.rs` (`QemuVirt`), `backend/bcm2837.rs` (`Bcm2837`), `backend/bcm2711.rs` (`Bcm2711`). These are zero-sized types that wrap the existing `uart_pl011`/`uart_bcm_mini` drivers and the `hal/soc` profile constants.

Edited:

| File | Sites removed | Now calls |
|---|---|---|
| `hal/arch/arm/src/aarch64.rs` | — | `pub mod backend;` (plus its `target_arch` gate) |
| `aarch64/cfi.rs` | `:80,82` | `Board::early_puts` |
| `aarch64/mte.rs` | `:24,26` | `Board::early_puts` |
| `aarch64/uart_pl011.rs` | `:8,10` | `PL011_BASE = <Board as ArmPlatform>::PL011_BASE` |
| `kernel/src/main.rs` | `:167,169`, `:263,267`, `:1572,1578`, `:1836,1840` | `console_init`, `early_putc`, `diag_probe` |
| `kernel/src/task/drivers/console_drv.rs` | `:115,124,134` | `CONSOLE_RX_IRQ`, `console_poll_rx` |
| `kernel/src/task/drivers/uart.rs` (Q1) | `:91,95,108,428,436` | `console_write_byte`, `CONSOLE_RX_IRQ`, `console_poll_rx` |

### Trait

| Item | `QemuVirt` | `Bcm2837` | `Bcm2711` |
|---|---|---|---|
| `type Iommu` | `NoIommu` | `NoIommu` | `NoIommu` |
| `const PL011_BASE` | `QEMU_ARM_VIRT.uart.mmio.base` | `QEMU_ARM_VIRT.uart.mmio.base` (unchanged value; no RPi3 path routes output through the PL011) | `BCM2711.mmio.uart_base` |
| `const CONSOLE_RX_IRQ` | `false` | `true` | `false` |
| `console_init` | `uart_pl011::init` | `uart_bcm_mini::init` | `uart_pl011::init` |
| `early_putc` | `uart_pl011::putchar` | `uart_bcm_mini::putchar` | `uart_pl011::putchar` |
| `early_puts` | `uart_pl011::puts` (CRLF) | `uart_bcm_mini::puts` (raw) | `uart_pl011::puts` (CRLF) |
| `console_write_byte(b, service_rx)` | `putchar` | CR before LF, then loop `service_rx(); try_putchar` (moved verbatim from `uart.rs` `write_rpi3_console_byte`) | `putchar` |
| `console_poll_rx` | `uart_pl011::poll_rx` | `uart_bcm_mini::poll_rx` | `uart_pl011::poll_rx` |
| `diag_probe` | no-op | `uart_bcm_mini::probe_put` | no-op |

The early-path methods are `#[inline(always)]` and allocation-free. `console_write_byte` is generic over `service_rx: impl Fn() + Copy`, so the kernel's `vi_handle_uart_irq` is monomorphised in the kernel crate as before.

`uart.rs` `vi_handle_uart_irq`: the shared 16550 MMIO read moved into `poll_serial_mmio()` (`#[inline(always)]`). The RISC-V path calls it unchanged. AArch64 reads `console_poll_rx` when `CONSOLE_RX_IRQ` is set, and `poll_serial_mmio` otherwise.

### Success criterion `rg`
`rg 'feature *= *"board-rpi'` over `cfi.rs`, `mte.rs`, `uart_pl011.rs`, `console_drv.rs` and `uart.rs`: **0 hits**. The only hits under `aarch64/backend/` are in `mod.rs`. `main.rs` keeps only the `:13` conflict `compile_error!` (feature wiring) and the Phase 04 sites (`:746-788`, `:1152-1173`, `:1687-1739`).

## 2. Release `.text` (gate rows)

Commands: baseline §2. CI build command with per-variant `CARGO_TARGET_DIR=~/.cache/cellos-a64-p01/<variant>`, `RUSTFLAGS`/`CARGO_TARGET_DIR` otherwise unset, and the same embedded inputs (`init` `ef64543d…`, `kernel_fs.img` `b084056c…`). Measured with `llvm-size -A` (rustup llvm-tools) and `aarch64-linux-gnu-nm -S` t/T sum. All four builds exited 0 with the same single future-incompat warning as the baseline.

| Variant | `.text` base → now | Δ | Σ t/T base → now | Δ | t/T symbols | in ±1%? | ELF sha256 (16) |
|---|---|---:|---|---:|---|---|---|
| virt | 1 314 816 → **1 310 720** | −4 096 (−0.31%) | 1 310 348 → **1 307 484** | −2 864 (−0.219%) | 1305 → 1306 | yes / yes | `6cfd16559a3d0314` |
| `board-rpi3` | 1 331 200 → **1 331 200** | 0 | 1 325 040 → **1 325 176** | +136 (+0.010%) | 1323 → 1322 | yes / yes | `9be7ba469ae9885d` |
| `board-rpi3-bring-up` | 1 331 200 → **1 331 200** | 0 | 1 325 040 → **1 325 176** | +136 (+0.010%) | 1323 → 1322 | yes / yes | `b7b92d1b6e326252` |
| `board-rpi4` | 1 302 528 → **1 302 528** | 0 | 1 298 540 → **1 299 768** | +1 228 (+0.095%) | 1309 → 1312 | yes / yes | `ff34796dba3cab99` |

Other sections:
- `.text.boot` and `.text.vectors` are unchanged in every variant.
- `.rodata` is unchanged.
- `.data`/`.bss` differ by at most 96 B (virt `.data` 41 424 → 41 520).

Per-symbol diff: the deltas are LTO inlining moves in code this phase did not touch. Examples: virt `loader::governed_spawn::prepare_gated` −5 720 with `boot_ceiling::log_refusal` +4 204 outlined; rpi4 `task::yield_cpu` −12 328 with `Scheduler::pick_next_domain` +12 332 outlined. On rpi3, `DirectWriter::write_str` (540 B) is now inlined into `write_user_record` (+464 B).

Reference rows (not gate rows):
- `run-rpi3.ps1` flag set (§4 build below): `.text` 1 351 680 → 1 347 584 (−0.30%); Σ t/T 1 345 596 → 1 344 740.
- Test-hooks kernel: `.text` 1 699 840 → 1 699 840; Σ t/T 1 695 504 → 1 697 104.

The final edits to declaration order and to the selection ladder were rebuilt, and the hashes above are unchanged.

## 3. QEMU `virt` lanes

**Isolation.** These lanes ran in a detached git worktree of HEAD with the Phase 01 files copied in, plus the gitignored `.cargo/config.toml`. The first attempt without that file failed the cell link (`R_AARCH64_ABS64 … recompile with -fPIC`); that was an environment issue, not a code issue. The worktrees were removed afterwards, and the logs are in `~/.cache/cellos-a64-p01/`.

| Lane | Command | Result |
|---|---|---|
| L1 | `BOOT_WINDOW=90 bash scripts/qemu-aarch64-test.sh <virt ELF> <sparse copy of disk_arm_virt.img>` | **PASS: aarch64 shell prompt reached**, exit 0. The markers are on the same log lines as the baseline: banner :4, `[smp] hart 1 … single-hart` :75, `Init: OS ready.` :165, `Cellos > ` :180; `shell: starting` moved from :174 to :175. There are 0 `[fault] Cell` lines. The first 120 log bytes (the CFI/MTE lines and the banner) are byte-identical. In the normalised diff, the only change is the order of two async `[net-bridge] first e0 TX/RX` user lines. |
| L2 build | `bash scripts/build-aarch64-test-hooks-ci.sh` | exit 0 (`-D warnings`) |
| L2 SMP=1, run 1 of 1 | `bash scripts/qemu-aarch64-test-hooks.sh` | **PASS: AArch64 test-hooks self-tests passed (semihosting enabled)**, exit 0. `[vfs-test] Results: 96 PASS, 0 FAIL`; `[smp] hart 1: firmware reports no such core — single-hart`; `SMP-IPI: skipped`. 6 `[fault] Cell` lines (fixtures; the baseline also had 6). |
| L2 SMP=2, run 1 of 1 | `QEMU_SMP=2 bash scripts/qemu-aarch64-test-hooks.sh` | **PASS**, exit 0. `[smp] hart 1 online, parked`; `[selftest] SMP-IPI: PASS hart=1 epoch=1`; `96 PASS, 0 FAIL`; 6 fixture faults. |

No retries were needed: each lane ran once and passed.

## 4. RPi3 — QEMU `raspi3b` (Q10(a))

**Flow (R2, bash).**
1. In the worktree: `pwsh -NoProfile -File scripts/build-aarch64-cells.ps1 -BoardRpi3`. This produced `target/rpi3-embedded/kernel_fs.img` `ceacad8860cdb66e…` and `init` `4f0967fbe55b0a23…`, the same as the baseline R2 images. No bash equivalent of the `.ps1` exists in `scripts/`.
2. Build the kernel:
   ```
   EMBEDDED_OVERRIDE=<abs>/target/rpi3-embedded RUSTFLAGS="-C relocation-model=pic" cargo build --release --features board-rpi3 -p cellos-kernel --target aarch64-unknown-none-softfloat
   ```
3. Boot: `timeout 120 qemu-system-aarch64 -machine raspi3b -cpu cortex-a53 -m 1G -display none -serial null -serial stdio -kernel <elf> < /dev/null`.

**Result: the prompt is reached in 5 of 5 runs, with 0 `[fault] Cell` lines.**
- The first 160 raw bytes are byte-identical to the baseline: `[INFO] CFI: BTI unavailable`, `[INFO] CFI: PAC-RET unavailable`, `[INFO] MTE: unavailable`, `CIS[Cellos] kernel boot v0.2.1`, `Kernel started …`.
- After normalising CRs, addresses and counters, every run matches the baseline §4 log line for line, from the CFI/MTE lines through `pi-monitor`, `mmc`, policy, self-tests, `V[INFO] Kernel initialization complete`, services, `Init: OS ready.`, `shell: starting`, `=== Cellos shell ready ===` and `Cellos > `. The only differences are:
  - where the idle-loop probe bytes `QK000L…` land relative to the shell banner;
  - how many periodic `[net] NIC driver unavailable` lines fall inside the window.
- Every run emits `Q`, `K000` and four `L` probes in total, plus `V`.

**The probe placement is a race that exists at baseline.** The baseline ELF was rebuilt at HEAD in a second worktree, and it is byte-identical to `rpi3-full.elf` (`0c0eacf5a4ac33de`). It was booted 4 more times. The order after `supervising services` was:

| Run | Order after `supervising services` |
|---|---|
| baseline report (original) | NIC, `QK000L`, ready, prompt |
| baseline ELF rerun 1 | `QK000L`, NIC, ready, prompt |
| baseline ELF rerun 2 | NIC, `QK000L`, ready, prompt |
| baseline ELF rerun 3 | ready, prompt, `QK000L` |
| baseline ELF rerun 4 | ready, prompt, `QK000L` |
| Phase 01 runs 1–3 | ready, prompt, `QK000L` |
| Phase 01 runs 4–5 | NIC, ready, prompt, `QK000L` |

With the probe bytes and the periodic NIC lines filtered out, all 5 Phase 01 runs are identical to the baseline §4 sequence (`diff` empty).

## 5. Other gate rows

| Gate | Command | Result |
|---|---|---|
| x86_64 kernel release (Q9) | `cargo build --release -p cellos-kernel --target x86_64-unknown-none -Z build-std=core,alloc` | exit 0 |
| Board matrix (Q9) | `bash scripts/check-board-configs.sh` | **exit 0**. The first run failed `aarch64-conflict`: with `board-rpi3,board-rpi4`, `hal-arm` hit E0428 (`Board` defined twice) before the kernel's conflict `compile_error!` could fire. The selection was made a precedence ladder (rpi3 over rpi4), and the rerun passes. The kernel's pre-existing `boot.rs` E0428 in that combination is unchanged. |
| Kernel host tests | `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu` | 377 passed, 0 failed |
| Mutable globals | `cargo run --release --target x86_64-unknown-linux-gnu --manifest-path tools/mutable-globals/Cargo.toml -- --check` | `603 declarations, 218 need review, 0 problems`, exit 0. No inventoried static moved, so Q7 was not triggered. |
| Format | `cargo fmt -p hal-arm -p cellos-kernel -- --check` | No diff in any line this phase touched. The files have pre-existing diffs elsewhere. |

## 6. Deviations from the plan text

1. **`uart_bcm_mini.rs` is not moved under `backend/bcm2837/` in this phase (Q3).**
   - Its other callers sit outside the Phase 01 set: `aarch64/trap.rs:144-182` (10 calls, Phase 02) and `kernel/src/main.rs:796,1692-1737` (Phase 04 sites). Moving it now would mean editing them or adding a temporary re-export.
   - Recommendation: Phase 02 moves it together with the `bcm2835_*` files and the `aarch64.rs:51` gate, and Phase 04 repoints the remaining `main.rs` probes. Coupling C2's mention of `hypervisor/registry.rs` is wrong: that file has no `uart_bcm_mini` caller.
2. **`console_base()` became `const PL011_BASE`.** The only consumer is the PL011 driver's `const`. On BCM2837 the console is the mini UART, so a `console_base` there would have to be the AUX base, which `uart_pl011` must not use. The trait therefore names what the driver needs. The extra items `CONSOLE_RX_IRQ`, `console_write_byte`, `console_poll_rx` and `diag_probe` exist to clear `console_drv.rs`, `uart.rs` and the `V`/`F` probes.
3. **Selection ladder.** The plan says "exactly one `cfg`". Each module and alias line has its own attribute, three arms in total, all in `backend/mod.rs`. `board-rpi3` takes precedence over `board-rpi4` only so that the board matrix's conflict check still reports the kernel's message (§5).
4. **Code-standards tension, recorded and not resolved.**
   - Law 5 bans `mod.rs`, but the approved file set names `backend/mod.rs`, and `kernel/src` already has five `mod.rs` files.
   - Law 6 asks for a `Vi` prefix on public traits; the plan names the trait `ArmPlatform`.
   - The plan's names were kept. A rename to `backend.rs`/`ViArmPlatform` would need a file-set amendment.

## 7. Proposed CHANGELOG bullet (not applied)

> - aarch64: board console and early-diagnostic selection now goes through a static `hal::backend::Board` (`QemuVirt`/`Bcm2837`/`Bcm2711`) implementing `ArmPlatform`, with no `board-rpi*` branches left in `cfi.rs`, `mte.rs`, `uart_pl011.rs`, `console_drv.rs` or `uart.rs` (ADR-0029 §6, phase 01). Behaviour-preserving: release `.text` is unchanged on rpi3, rpi3-bring-up and rpi4, and one page smaller (−0.31%) on virt.
