# Phase 02 gate — IRQ, timer, trap, SMP (checkpoint)

**Run:** 2026-11-02. Working tree on HEAD `91f1e0f42` (Phase 01 commit); not committed.
**Reference:** [baseline-2026-11-02.md](baseline-2026-11-02.md). The previous phase is in [phase-01-gate.md](phase-01-gate.md).
**Result:** every gate row **PASSES**. This is a checkpoint: Phase 03 is not started, and the user approves before Phase 04.
**Evidence ceiling:** QEMU 8.2.2 TCG (`virt`, `raspi3b`) on WSL2. RPi4 is checked by compile and size only. No physical claim.

## 1. Change

### Backend traits (`hal/arch/arm/src/aarch64/backend/mod.rs`)

`ArmPlatform` is now `ArmPlatform: ArmIrqController + ArmTimer` and gains these items:
- `SECONDARY_START` (`SecondaryStart::{Psci, None(reason)}`);
- `console_enable_rx_irq()`;
- `probe_uncategorized_trap(frame, vector_kind)`.

`ArmIrqController` items:
- `HAS_IPI`;
- `USB_ONE_SHOT_IRQ: Option<u32>`;
- `init()`, `init_cpu()`, `send_ipi(cpu)`, `unmask_usb_irq()`, `handle_irq()`.

`ArmTimer` items: `init()`, `reset()`.

| Item | `QemuVirt` | `Bcm2837` | `Bcm2711` |
|---|---|---|---|
| IRQ controller | `gic::*::<Self>` | `bcm2836_irq::init` + `bcm2835_legacy_irq::init` (boot and secondary cores) | `gic::*::<Self>` |
| `handle_irq` | `trap::handle_gic_irq::<Self>` | `bcm2837::trap::handle_irq` (moved verbatim from `trap.rs`) | `trap::handle_gic_irq::<Self>` |
| `HAS_IPI` / `send_ipi` | `true` / GIC SGI 0 | `false` / never called | `true` / GIC SGI 0 |
| `USB_ONE_SHOT_IRQ` | `None` | `Some(USB_IRQ)`; unmask = `bcm2835_legacy_irq::enable_irq(USB_IRQ)` | `None` |
| Tick | `timer::init_gic::<Self>` / `timer::reset_generic` | `bcm2837::timer::{init, reset}` (moved verbatim from `timer.rs`, including the C/I/S diagnostic) | as virt |
| `SECONDARY_START` | `Psci` | `None("BCM2836 has no SGI path in this kernel — keeping Cellos single-hart")` | `Psci` |
| `probe_uncategorized_trap` | no-op | `bcm2837::trap::probe_uncategorized_el2_fault` (moved verbatim) | no-op |

### GIC parameterised by profile (`gic.rs`)

`GicV2Layout` holds:
- `DISTRIBUTOR_BASE` and `CPU_INTERFACE_BASE`, taken from the `hal/soc` profiles;
- `GPIO_GIC_ID` (virt PL061 = 39; RPi4 `u32::MAX`, as before);
- `enable_board_irqs()`. Virt enables the VirtIO MMIO SPIs and the PL061 at the same point in distributor bring-up as before; RPi4 does nothing.
- `dispatch_spi(irq)`. Virt calls `vi_handle_virtio_irq`; RPi4 does nothing. On RPi4 the VirtIO hook is therefore never referenced, as before.

Every GIC function is generic over the layout. RPi3 instantiates none of them.

### Other files

- **`trap.rs`**: `vi_aarch64_irq_handler` is now `<Board as ArmIrqController>::handle_irq()`. The GIC claim/dispatch/complete cycle is `handle_gic_irq<G>` (`#[inline(always)]`) and is otherwise unchanged. The EC 0 probe goes through `Board::probe_uncategorized_trap`.
- **`timer.rs`**:
  - `init`/`reset` dispatch to `ArmTimer`.
  - The generic CNTP/CNTHP and GIC tick is `init_gic<G>`/`reset_generic`.
  - `ticks_per_quantum` is now unconditional.
- **`aarch64.rs`**: `Arch::init` and `init_secondary_hart` call `ArmIrqController::{init, init_cpu}`, and the four BCM module gates are gone.

### Moves (Q3, `git mv`, content unchanged)

`aarch64/{bcm2835_legacy_irq,bcm2835_systimer,bcm2836_irq,uart_bcm_mini}.rs` moved to `backend/bcm2837/`. Their `super::` sibling paths still resolve, so no line inside them changed.

New files `backend/bcm2837/{timer,trap}.rs` hold the RPi3 code moved out of `timer.rs` and `trap.rs`. `backend/mod.rs` declares `pub mod bcm2837` so that the Phase 04 callers can name it.

### Kernel

- **`task/smp.rs`**: no `board-rpi*` remains.
  - `send_ipi` returns early when `!HAS_IPI` and otherwise calls `ArmIrqController::send_ipi`.
  - `start_secondaries` matches on `SECONDARY_START`. `Psci` calls the PSCI path, renamed `start_psci_secondaries`; `None(reason)` logs `[smp] {reason}`, which produces the same output as before.
  - `SECONDARY_CONTEXT` and `smp_aarch64_secondary_main` are now gated `cfg(target_arch = "aarch64")`.
- **`task/syscall.rs` `WaitIrq` (Q2)**: the RPi3-only unmask is now board-neutral. It applies `if let Some(usb) = USB_ONE_SHOT_IRQ { if caller_has_usb_driver && irq == usb as u8 { unmask_usb_irq() } }` under `cfg(target_arch = "aarch64")`. The permission gate, the bounds and the error mapping are unchanged.

### Sites

- `rg 'feature *= *"board-rpi'` finds no hits in `trap.rs`, `timer.rs`, `gic.rs`, `task/smp.rs` or `syscall.rs:8307-8317`.
- `aarch64.rs` keeps exactly one hit, the `monitor` module gate (deviation 1).
- Across `hal/arch/arm/src` and `kernel/src` the count is 172, down from 234 at baseline. Six of them are the selection lines in `backend/mod.rs`.

### File-set amendments (approved by Main, 2026-11-02)

These are path-only edits with no change to any cfg or logic.
- `kernel/src/main.rs:792`: `uart_bcm_mini::enable_rx_interrupt()` → `Board::console_enable_rx_irq()`.
- `kernel/src/main.rs:1688,1710-1711,1716,1721,1733`: `uart_bcm_mini::probe_put` → `Board::diag_probe` (the Q/K/L probes). The `board-rpi3` cfgs stay for Phase 04.
- `kernel/src/hypervisor/registry.rs`, all inside the RPi3-only `pi_monitor_smoke`:
  - `:832` `timer::prepare_monitor_smoke_window` → `backend::bcm2837::timer::prepare_monitor_smoke_window`;
  - `:1009` `timer::arm_monitor_smoke_preemption` → `backend::bcm2837::timer::arm_monitor_smoke_preemption`;
  - `:1032` `bcm2835_systimer::init` → `backend::bcm2837::bcm2835_systimer::init`.
- **Q7 (conditional, triggered):** the key `kernel::task::smp::SECONDARY_CONTEXT` changed its `@cfg` from `all(aarch64, not(board-rpi3))` to `target_arch = "aarch64"`. `docs/generated/mutable-globals.md` was regenerated with `--write` (one row). `scripts/mutable-globals.toml` is unchanged; it has no entry for this key.
- **Handoff text:** coupling C2 in [lock-handoff-approval.md](../lock-handoff-approval.md) is corrected.
  - `hypervisor/registry.rs` never called `uart_bcm_mini`.
  - Its real BCM callers are the `bcm2835_systimer` and RPi3 timer smoke helpers at `:832,1009,1032`.

## 2. Release `.text` (gate rows)

The commands are those of baseline §2, with per-variant `CARGO_TARGET_DIR=~/.cache/cellos-a64-p02/<variant>`. Measurements use `llvm-size -A` and the `aarch64-linux-gnu-nm -S` sum of t/T symbols. Each build has exactly 1 warning, the same future-incompat note the baseline builds carry. `cargo check` for virt, rpi3 and rpi4 shows no new warnings.

| Variant | `.text` (baseline → now) | Δ | Σ t/T (baseline → now) | Δ | t/T symbols | Inside ±1%? | ELF sha256 (first 16) |
|---|---|---:|---|---:|---|---|---|
| virt | 1 314 816 → **1 310 720** | −0.31% | 1 310 348 → **1 307 488** | −2 860 (−0.218%) | 1305 → 1306 | yes / yes | `b23f9257b10d920e` |
| `board-rpi3` | 1 331 200 → **1 331 200** | 0 | 1 325 040 → **1 325 100** | +60 (+0.005%) | 1323 → 1323 | yes / yes | `d739cd3653e3e5e1` |
| `board-rpi3-bring-up` | 1 331 200 → **1 331 200** | 0 | 1 325 040 → **1 325 100** | +60 (+0.005%) | 1323 → 1323 | yes / yes | `027e62cb5486d85d` |
| `board-rpi4` | 1 302 528 → **1 302 528** | 0 | 1 298 540 → **1 299 768** | +1 228 (+0.095%) | 1309 → 1312 | yes / yes | `d4fc1c5d7b068b7d` |

Against Phase 01:
- **virt:** +4 B. The only real change is `vi_aarch64_irq_handler` 448 → 452; every other line in the per-symbol diff is a reordering of same-named generic instances.
- **rpi3:** −76 B. The deltas are link-time-optimisation (LTO) inlining moves in untouched code: `prepare_gated` −2 404 against outlined `cap_registry::attach_file_to_ticket` +2 284, plus `launch::SessionPublication::*`.
  - `vi_aarch64_irq_handler` keeps its size (396 B). `vi_aarch64_trap_handler` is 5 776 → 5 764 B, because the EC 0 probe call now goes through the inlined `Board` wrapper.
  - `smp_aarch64_secondary_main` is absent from the RPi3 ELF, as before.
- **rpi4:** the symbol sum is identical to Phase 01, though the bytes differ.
- `.text.boot` and `.text.vectors` are unchanged everywhere. On rpi3, `.data`/`.bss` are 43 000/584 384 (baseline 42 952/584 384).

Reference rows, not gate rows:
- `run-rpi3.ps1` flag set: `.text` 1 347 584 (baseline 1 351 680), Σ 1 344 772.
- Test-hooks kernel: `.text` 1 699 840 (baseline 1 699 840), Σ 1 697 108 (baseline 1 695 504).

## 3. QEMU `virt` lanes

The test-hooks build and runs used a detached worktree of HEAD with this diff applied, plus the gitignored `.cargo/config.toml`. The worktree is removed. L1 ran against the gate ELF above. Logs are in `~/.cache/cellos-a64-p02/`.

| Lane | Result |
|---|---|
| L1 `BOOT_WINDOW=90 bash scripts/qemu-aarch64-test.sh <virt ELF> <disk copy>` | **PASS: aarch64 shell prompt reached**, exit 0, 0 `[fault] Cell` lines. The first 120 bytes match the baseline byte for byte. After normalising, the only differences are the order of the async `[net-bridge]` and DHCP user lines. VirtIO SPI dispatch works: `[net-rx-producer] irq->completion PASS` and `DHCP acquired`. |
| L2 build `bash scripts/build-aarch64-test-hooks-ci.sh` | exit 0 (`-D warnings`) |
| L2 SMP=1, run 1 | **PASS**, exit 0. `96 PASS, 0 FAIL`; `[smp] hart 1: firmware reports no such core — single-hart`; `SMP-IPI: skipped`; 6 fixture faults (baseline: 6). |
| L2 `QEMU_SMP=2`, runs 1, 2 and 3 | **PASS ×3**, exit 0 each. Each run shows `[smp] hart 1 online, parked`, `[selftest] SMP-IPI: PASS hart=1 epoch=1`, `96 PASS, 0 FAIL` and 6 fixture faults. Runs 2 and 3 were extra checkpoint evidence, not retries. |

## 4. RPi3 — QEMU `raspi3b` (Q10(a))

The flow is the same as in Phase 01 §4: `pwsh build-aarch64-cells.ps1 -BoardRpi3`, then the `run-rpi3.ps1` flag-set kernel build with `EMBEDDED_OVERRIDE`, then `timeout 120 qemu-system-aarch64 -machine raspi3b …`. The cell image matches the baseline R2 image exactly (`kernel_fs.img` `ceacad8860cdb66e…`, `init` `4f0967fbe55b0a23…`).

**Result: 8 of 8 runs reach `Cellos > ` with 0 `[fault] Cell` lines.**
- The first 160 raw bytes are identical to the baseline: the CFI/MTE lines, `CIS` (the systimer diagnostic, now in `backend/bcm2837/timer.rs`) and the banner.
- Every run logs `[smp] BCM2836 has no SGI path in this kernel — keeping Cellos single-hart`, emits `V`, and shows the `K000` IRQ snapshot and four idle `L` probes.

**Filtered diff against baseline §4.** The filter removes the probe bytes, the periodic NIC lines and the QEMU termination line.
- 5 runs are identical.
- 1 run (run 7) differs only because the QEMU termination text landed on the prompt line.
- 2 runs (runs 2 and 6) move `USER: [net] DHCP: deconfigured` after `Init: net launched.`

The DHCP reordering is a race between the net cell and init. To show that it exists at baseline, the unchanged baseline ELF (`0c0eacf5…`, byte-identical to `rpi3-full.elf`) was booted 3 times in parallel with Phase 02 runs 6–8, so both kernels ran under the same host load:

| | DHCP line before `net launched` |
|---|---|
| baseline ELF, runs 6–8 | 2 of 3 (run 7 reorders) |
| Phase 02, runs 6–8 | 2 of 3 (run 6 reorders) |

Probe placement relative to the shell banner varies in both kernels too; Phase 01 §4 showed this.

**Tick proxies.** Q10(a) restates the tick criterion as the sequence above, but the proxies are recorded here:
- `K000` appears in 8 of 8 runs.
- Each run has 4 `L` probes.
- The periodic `[net] NIC driver unavailable` line, counted after `supervising services` in a 120 s window, appears 0–1 times in Phase 02 runs and 1–2 times in baseline-ELF runs. Run 7's 0 is its window cut at the prompt.

## 5. Other gate rows

| Gate | Result |
|---|---|
| x86_64 kernel release (Q9) | exit 0 |
| `bash scripts/check-board-configs.sh` (Q9) | exit 0. It covers RV64 (touched only by Phase 01), x86_64, aarch64 virt/rpi3/rpi4, and the expected conflict errors. |
| Kernel host tests | 377 passed, 0 failed |
| `mutable-globals --check` | The first run failed: `docs/generated/mutable-globals.md is not current`, caused by the `SECONDARY_CONTEXT` `@cfg` key above. After `--write` (Q7): `603 declarations, 218 need review, 0 problems`, exit 0. |
| Format | The new and rewritten files pass `rustfmt --check`. `cargo fmt --check` reports no hunks in lines this phase touched. |

## 6. Deviations

1. **`aarch64.rs` keeps the `monitor` module gate** (`any(board-rpi3, board-rpi4)`), so the "four files free of `board-rpi*`" criterion holds for `trap.rs`, `timer.rs` and `smp.rs` but not fully for `aarch64.rs`.
   - `monitor.rs` has `global_asm!` and `#[no_mangle]` items, so it cannot compile on virt.
   - Clearing the gate means moving the file under `backend/`. The handoff gives that move to Phase 04, and all of its callers are Phase 04 sites: `cpu_features.rs:146`, `registry.rs:99,105,361`, `main.rs:755-759`, `aarch64/paging.rs:359`, `stage2_regs.rs:164-222`.
   - Main was informed, with this as the default. Phase 04 should move `monitor.rs` together with its callers.
2. **`ArmIrqController` differs from the plan's list** (init, enable, claim, complete, `send_sgi -> Option`).
   - Claim and complete are GIC-only; the BCM2836/BCM2835 dispatch has no claim/complete cycle. They therefore live behind `GicV2Layout`, and the trait exposes the whole `handle_irq`.
   - `send_sgi -> Option` became `HAS_IPI` plus `send_ipi`, so RPi3's `send_ipi` still compiles to nothing. An `Option` return would have added the guard and the CPU lookup to the RPi3 path.
   - Per-IRQ enable stays a GIC function: `gic::enable_irq::<G>` for the timer PPI. RPi3 routes its timer through `bcm2836_irq::add_timer_enable`.
3. **`ArmTimer` has no `routing` item.** Routing is part of each backend's `init`: a GIC PPI enable on virt and RPi4, `add_timer_enable` on RPi3.
4. **`backend::bcm2837` is `pub`.** The Phase 04 callers (`pi_monitor_smoke`) name `bcm2837::timer::*` and `bcm2837::bcm2835_systimer::init` directly until Phase 04 removes their cfgs.
5. **`syscall.rs:8307-8311` (Q2)** was made board-neutral (`USB_ONE_SHOT_IRQ`) rather than only repointed, because Phase 02 owns those lines. Behaviour per board:
   - RPi3 runs the same check and unmask as before;
   - virt and RPi4 compile the block away, as before.
6. **The SMP code is now compiled on RPi3, although it never runs there.**
   - `SECONDARY_CONTEXT`, `start_psci_secondaries` and `smp_aarch64_secondary_main` are now type-checked on RPi3.
   - At runtime they are unreachable, because `SECONDARY_START` is `None` and `_secondary_entry` is still assembled only when `secondary_psci` is set (`boot.rs`, Phase 04).
   - LTO drops them from the RPi3 ELF, as the size and symbol checks confirm.

## 7. Proposed CHANGELOG bullet (not applied)

> - aarch64: interrupt controller, host tick, IRQ dispatch, cross-hart IPI and secondary-core start now go through the static board backend (`ArmIrqController`, `ArmTimer`, `SecondaryStart`); the GIC driver is parameterised by each board's `hal/soc` profile, and the BCM2836/BCM2835 IRQ, system-timer and mini-UART code moved under `backend/bcm2837/` (ADR-0029 §6, phase 02). Behaviour-preserving: release `.text` is unchanged against phase 01 on every variant; RPi3 stays single-hart.
