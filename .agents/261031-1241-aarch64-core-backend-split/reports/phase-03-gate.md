# Phase 03 gate — platform map, resources, driver selectors

**Run:** 2026-11-02. Working tree on HEAD `c2f7b46b5` (the Phase 02 commit); not committed.
**Reference:** [baseline-2026-11-02.md](baseline-2026-11-02.md). The previous phase is in [phase-02-gate.md](phase-02-gate.md).
**Result:** every gate row **PASSES**. Phase 04 is not started.
**Evidence ceiling:** QEMU 8.2.2 TCG (`virt`, `raspi3b`) on WSL2. RPi4 is checked by compile and size only. No physical claim.

## 1. Change

### Backend data (`hal/arch/arm/src/aarch64/backend/*`)

A new trait `ArmResources` holds the board's platform map as `const`s, so it adds no runtime cost. `ArmPlatform` now requires `ArmIrqController + ArmTimer + ArmResources`. Every value comes from the existing `hal/soc` profiles; there is no new address table.

| Item | `QemuVirt` | `Bcm2837` | `Bcm2711` |
|---|---|---|---|
| `SOC` | `QemuArmVirt` | `Bcm2837` | `Bcm2711` |
| `PLATFORM` (uart, irq, rtc, published VirtIO slots, announce) | PL011 / SPI 1 / PL031 / 4 / none | `mini_uart_io` / 0 / none / 0 / `"RPi 3 BCM2837"` + peripheral base | PL011 UART0 / 0 / none / 0 / none |
| `KERNEL_MMIO_MAPS` (in mapping order; C = Cell, K = kernel-only) | GIC K, peripherals C, VirtIO C, platform bus K, ECAM K | peripherals C, local controller K | GPIO C, UART C, SDHCI C, GICD K, GICC K |
| `VIRTIO_MMIO` | `Some(QEMU_ARM_VIRT.virtio)` | `None` | `None` |
| `GPIO_MMIO_BASE` | PL061 | BCM2837 GPIO | BCM2711 GPIO |
| `DWC2_WINDOW` | `None` | `Some(dwc2_base, dwc2_grant_size)` | `None` |
| `DISPLAY` (mailbox, framebuffer limit) | `None` | `Some(mailbox, peripheral_base)` | `None` |
| `SDHCI` (base, policy, pinmux GPIO) | `None` | `Some(…, Some(gpio_base))` | `Some(…, Some(gpio_base))` |
| `BOARD_PINMUX_GPIO_BASE` | `None` | `Some(gpio_base)` | `None` |

The map ends that used to be computed with overflow checks at boot are now computed when the constants are built.
- BCM2837 still uses `peripheral_end()` and `local_controller_end()`.
- BCM2711 uses a `checked_add` helper.
- An overflowing profile is now a compile error rather than a boot `expect` or `InvalidAddress`.

### Kernel

- **`platform.rs`:** one AArch64 `init` replaces the three per-board variants. Each step runs as before:
  1. `crate::board::active()`. This is the same validate-and-return body as `selected_{rpi3,qemu_arm_virt,rpi4}`.
  2. The RTC is initialised when the board has one.
  3. `PlatformInfo` is built from `PLATFORM` and `VIRTIO_MMIO`, then published.
  4. The announce line is logged when the board has one; on RPi3 it reads exactly as before.
  `arm_virtio_slots` is now gated on `target_arch` only.
- **`memory/paging.rs`:** the three per-board blocks are now one loop over `KERNEL_MMIO_MAPS`, using the same Cell (USER) or kernel-only flags as before.
- **`resource_registry.rs` (Q5):**
  - **Tables stay kernel-side.** `BCM2837_ALLOWED`, `QEMU_ARM_VIRT_ALLOWED` and `BCM2711_ALLOWED` were moved byte-for-byte, keeping the same profile expressions and the same entry order.
  - **Selection.** `ALLOWED` is a `const` `match` on `<Board as ArmResources>::SOC`. The `DEV_*` bits stay kernel-local, so this is no Law-1 change.
  - **Const assertions per board.** Every AArch64 build checks all three tables against a frozen literal snapshot of the pre-plan values, using `allowlists_equal`: order-sensitive, field by field.
  - **Negative control.** Changing the BCM2711 snapshot's GPIO length to `0x2000` in a scratch worktree made a **virt** `cargo check` fail with `E0080 … assertion failed: allowlists_equal(BCM2711_ALLOWED, BCM2711_ALLOWED_SNAPSHOT)`. A drift in any table therefore breaks every AArch64 build.
  - `is_dwc2_mmio` reads `DWC2_WINDOW`.
  - **`self_test`** dispatches on `SOC`:
    - `Bcm2837` runs the old RPi3 test, moved verbatim as `bcm2837_self_test`.
    - `QemuArmVirt` runs the **new virt self-test (Q5)**. It checks that `ALLOWED` equals the virt snapshot, that each window authorises its own class, and that each window refuses every other class (`!class`).
    - `Bcm2711` passes trivially, as before.
- **`task/drivers.rs`:** the `bcm_pinmux` module gate is now `target_arch` only. The call runs when `BOARD_PINMUX_GPIO_BASE` is `Some`, which is RPi3 only, as before.
- **`task/drivers/gpio_irq.rs`:** `GPIO_MMIO_BASE` comes from the backend. `vi_signal_usb_irq` signals `USB_ONE_SHOT_IRQ`; on RPi3 this is the same IRQ 9 as before.
- **`task/drivers/mmc.rs`:**
  - `pinmux_bcm` is now gated on `target_arch` only.
  - `selected_sdhci_config` reads `SDHCI`.
  - The SD pinmux uses `SDHCI.pinmux_gpio_base` with `crate::board::active().wiring`. This is the same function body as the old `selected_rpi3()`/`selected_rpi4()`.
- **`task/drivers/virtio_common.rs`:** `virtio_slots` and `ack_unclaimed` read `VIRTIO_MMIO`; `None` yields no slots, as before on the BCM boards.
- **`task/syscall.rs` `RegisterDisplayFramebuffer`:**
  - The cfg is now `any(aarch64, x86_64)`. On AArch64, a `let Some(display) = DISPLAY else { Unknown }` comes first, so virt and RPi4 still refuse with `Unknown` before any other check.
  - On RPi3 the mailbox `owns_exact_mmio` check and the `end > framebuffer_limit` check read `display`.
  - Permission gates, bounds and error mapping are unchanged.
  - On x86_64 the code is identical; only the cfg spelling of its arms changed (`not(aarch64)`).

### Sites

- `rg 'feature *= *"board-rpi'` finds no hits in `platform.rs`, `memory/paging.rs`, `resource_registry.rs`, `task/drivers.rs`, `gpio_irq.rs`, `mmc.rs` or `virtio_common.rs`.
- In `syscall.rs` it finds only `:9406,9408` (`VcpuRegs`, Phase 04).
- Across `hal/arch/arm/src` and `kernel/src` the count is **126**: baseline 234, Phase 02 172.

### Q6 and Q7

- Q6: `pcie_ecam.rs` is not touched.
- Q7: no inventoried static moved; `mutable-globals --check` passes with no regeneration.

### File-set amendments

None. Every edit is inside the Phase 03 set. The handoff's alternatives for the equality test were `resource_registry_tests.rs` or `resource_registry_selftest.rs`; neither was needed, because the const assertions sit beside the tables and the virt self-test lives in `resource_registry.rs::self_test`.

## 2. Release `.text` (gate rows)

The commands are those of baseline §2, with `CARGO_TARGET_DIR=~/.cache/cellos-a64-p03/<variant>`. Measurements use `llvm-size -A` and the `nm -S` sum of t/T symbols. Each build has exactly 1 warning, the same future-incompat note as the baseline.

| Variant | `.text` (baseline → now) | Δ | Σ t/T (baseline → now) | Δ | t/T symbols | Inside ±1%? | ELF sha256 (first 16) |
|---|---|---:|---|---:|---|---|---|
| virt | 1 314 816 → **1 310 720** | −0.31% | 1 310 348 → **1 308 600** | −1 748 (−0.133%) | 1305 → 1298 | yes / yes | `42b749fb00442f05` |
| `board-rpi3` | 1 331 200 → **1 327 104** | −0.31% | 1 325 040 → **1 323 488** | −1 552 (−0.117%) | 1323 → 1317 | yes / yes | `159f1413274855cd` |
| `board-rpi3-bring-up` | 1 331 200 → **1 327 104** | −0.31% | 1 325 040 → **1 323 488** | −1 552 (−0.117%) | 1323 → 1317 | yes / yes | `75afbe64d5b60f36` |
| `board-rpi4` | 1 302 528 → **1 302 528** | 0 | 1 298 540 → **1 300 260** | +1 720 (+0.132%) | 1309 → 1307 | yes / yes | `5a024b9459b7e7f5` |

Against Phase 02:
- `invalid_descriptor` and `ValidationError::fmt` (−1.4 KB) are gone in every variant. Calling `board::active()` lets the optimiser fold the constant descriptor validation. Validation still runs, but its failure path is now provably dead.
- **virt:** `kmain` +1 200, which is the inlined virt allowlist self-test. The remaining changes are link-time-optimisation (LTO) inlining moves in untouched code: `boot_ceiling::log_refusal` −4 204 against `prepare_gated` +5 712, and `ed25519` verify.
- **rpi3:** −1 612. The changes are LTO moves (`launch::SessionPublication::*` against `prepare_gated`).
- **rpi4:** `yield_cpu` +12 620 against `pick_next_domain` −12 332 inlined. Note that this pair moved in the opposite direction in Phase 01.
- `.text.boot` and `.text.vectors` are unchanged.

Reference rows, not gate rows:
- `run-rpi3.ps1` flag set: `.text` 1 347 584, Σ 1 343 256.
- Test-hooks kernel: `.text` 1 699 840, Σ 1 694 168.

## 3. QEMU `virt` lanes

The test-hooks build used a detached worktree of HEAD with this diff applied, plus the gitignored `.cargo/config.toml`. The worktree is removed. Logs are in `~/.cache/cellos-a64-p03/`.

| Lane | Result |
|---|---|
| L1 `BOOT_WINDOW=90 bash scripts/qemu-aarch64-test.sh <virt ELF> <disk copy>` | **PASS: aarch64 shell prompt reached**, exit 0, 0 `[fault] Cell` lines. The first 120 bytes match the baseline byte for byte. The new virt self-test logs `mmio-allowlist self-test PASS (known windows live, DWC2 denied)` (same text as before). After normalising, the only differences are the order of async lines (`Init: input launched`/`service-registry`, `[net-bridge]`, `supervising`). VirtIO and the mapping work: `[net-rx-producer] irq->completion PASS` and `DHCP acquired`. |
| L2 build | exit 0 (`-D warnings`) |
| L2 SMP=1, run 1 | **PASS**, exit 0. `PCIE-BAR-WINDOW: PASS`, `mmio-allowlist self-test PASS`, `96 PASS, 0 FAIL`, `SMP-IPI: skipped`, 6 fixture faults (baseline: 6). |
| L2 `QEMU_SMP=2`, run 1 | **PASS**, exit 0. `hart 1 online, parked`, `SMP-IPI: PASS hart=1 epoch=1`, `mmio-allowlist self-test PASS`, `96 PASS, 0 FAIL`, 6 fixture faults. No retry was needed. |

## 4. RPi3 — QEMU `raspi3b` (Q10(a))

The flow is the same as in Phase 01 §4. The cell image matches the baseline exactly (`kernel_fs.img` `ceacad8860cdb66e…`, `init` `4f0967fbe55b0a23…`). Five Phase 03 runs were booted in parallel with three runs of the unchanged baseline ELF (`0c0eacf5…`), so both kernels ran under the same host load.

| Run | Prompt | `[fault] Cell` | `CIS`/`V`/`K000` | `[pinmux]` lines | `[mmc] no card found` | `mmio-allowlist … PASS` | Filtered diff vs baseline §4 |
|---|---|---|---|---|---|---|---|
| Phase 03, runs 1–5 | 5/5 | 0 | 5/5 | 2 each | 1 each | 5/5 | **identical ×5** |
| baseline ELF, runs 1–3 | 3/3 | 0 | 3/3 | 2 each | 1 each | 3/3 | identical ×3 |

- The first 160 raw bytes are identical to the baseline.
- The filtered diff covers everything that depends on this phase:
  - the pinmux path (`BOARD_PINMUX_GPIO_BASE`);
  - the SDHCI probe and its SD pinmux (`SDHCI`);
  - the kernel MMIO map (BCM peripherals and local controller: the mini UART, IRQ controller and SDHCI accesses all run through it);
  - the RPi3 allowlist self-test;
  - `DHCP: deconfigured` before `Init: net launched` in all 8 runs.
- The `[platform] RPi 3 BCM2837 …` line is not in either log. It is emitted before the logger exists, as at baseline.

## 5. Other gate rows

| Gate | Result |
|---|---|
| x86_64 kernel release (Q9) | exit 0. The `RegisterDisplayFramebuffer` x86 arms are the same code. |
| `bash scripts/check-board-configs.sh` (Q9) | exit 0. This covers the RV64 `drivers.rs` path. |
| Kernel host tests | 377 passed, 0 failed |
| `mutable-globals --check` | `603 declarations, 218 need review, 0 problems`, exit 0. No regeneration was needed (Q7 not triggered). |
| Format | No `cargo fmt --check` hunk in any line this phase touched. The backend files pass `rustfmt --check`. |

## 6. Deviations

1. **The plan's accessors differ from what was built.**
   - The plan lists `platform_info()`, `kernel_mmio_maps() -> &[MapSpec]`, `mmio_authority() -> &[ResourceEntry]`, `has_virtio`, `has_ecam` and `gpio_base`.
   - What was built:
     - **HAL-side `const` data:** `PLATFORM` (a HAL `PlatformFacts`, because the HAL cannot name the kernel's `PlatformInfo`), `KERNEL_MMIO_MAPS: &[MmioMap]`, `VIRTIO_MMIO: Option<layout>` (in place of `has_virtio`), `GPIO_MMIO_BASE`, and `DWC2_WINDOW`/`DISPLAY`/`SDHCI`/`BOARD_PINMUX_GPIO_BASE` for the remaining selectors.
     - **`mmio_authority()` is replaced by `SOC`:** the tables stay kernel-side, per Q5.
     - **No `has_ecam`:** the ECAM window is one of virt's `KERNEL_MMIO_MAPS` entries, and `pcie_ecam.rs` is dropped (Q6).
2. **The equality test is a compile-time `const` assertion** (Q5), not a host unit test (coupling C4). It checks all three tables in every AArch64 build, so RPi3 and RPi4 are covered with no runtime lane. The virt runtime self-test is the Q5 complement.
3. **The virt self-test runs in the production virt kernel.** It adds about 1.2 KB, inlined into `kmain`. It sits in the existing every-boot `mmio-allowlist` self-test, which used to return `true` on virt, so `main.rs` (a Phase 04 file) did not need a new call site. The log text is unchanged.
4. **Board-specific kernel code is compiled into every AArch64 build.** This covers `bcm_pinmux`, `mmc/pinmux_bcm`, the RPi3 allowlist self-test and the framebuffer syscall body. The const `Option`s and the `SOC` match select it, and LTO removes it where it is unused. The symbol diff shows no RPi3-only symbol in the virt ELF.
5. **Overflow checks on the map ends moved from boot to compile time** (see §1). The previous boot-time checks could not fail on these constant profiles.

## 7. Proposed CHANGELOG bullet (not applied)

> - aarch64: platform facts, kernel MMIO identity maps, VirtIO/GPIO/DWC2/display/SDHCI and pin-mux selectors now come from the board backend's `ArmResources` constants; the per-SoC MMIO authority tables stay kernel-side, are selected by `ArmResources::SOC`, and are pinned by compile-time equality assertions against the pre-plan snapshot (all three boards in every AArch64 build) plus a QEMU virt boot self-test (ADR-0029 §6, phase 03). Behaviour-preserving: release `.text` stays within ±0.31% of the pre-plan baseline on every variant.
