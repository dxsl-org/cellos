# Phase 04 gate — boot/DTB, board selection, EL2/vCPU monitor (checkpoint)

**Run:** 2026-11-02. Working tree on HEAD `1dce7b3cb` (the Phase 03 commit); not committed.
**Reference:** [baseline-2026-11-02.md](baseline-2026-11-02.md) (pre-plan, `6d5cdeb0c`). Previous phases: [01](phase-01-gate.md), [02](phase-02-gate.md), [03](phase-03-gate.md).
**Result:** every gate row **PASSES**. The hypervisor lane fails *identically* at the pre-plan baseline and with Phase 04 (§3). This checkpoint closes the plan once the user approves it.
**Law 1:** no delta. No path in `libs/api`, `libs/types` or `libs/ostd` was touched. The `VcpuRegs` buffer and modes and the `AArch64Vcpu` layout are unchanged per build (§1 assertions).
**Evidence ceiling:** QEMU 8.2.2 TCG (`virt`, `raspi3b`) on WSL2. RPi4 is checked by compile and size only. No physical claim.

## 1. Change

### Backend (`hal/arch/arm/src/aarch64/backend/*`)

New `ArmPlatform` items:

| Item | `QemuVirt` | `Bcm2837` | `Bcm2711` |
|---|---|---|---|
| `PI_MONITOR_BOOT` (boot asm `board_bcm`) | `false` | `true` | `true` |
| `SECONDARY_START` (boot asm `secondary_psci`, from Phase 02) | `Psci` | `None(…)` | `Psci` |
| `DTB_POLICY` | `Optional` | `Conservative` | `Required` |
| `IDLE_PROBES` | 0 | 4 | 0 |
| `type VcpuExt` | `NoVcpuExt` (`[u64; 0]`) | `PiVcpuExt` (FP bank + CPACR/CNTKCTL/TPIDRRO) | `NoVcpuExt` |
| `type El2Monitor` | `NoEl2Monitor` | `PiEl2Monitor` | `NoEl2Monitor` |
| `irq_snapshot_probe()` | no-op | the `K` probe, moved verbatim from `main.rs` | no-op |
| `ArmResources::SUPERVISOR_DEVICE_MMIO` | GIC map, peripheral map | peripherals, local controller | none |

New traits and types:
- **`ArmVcpuExt`:**
  - `HostBank`, `PI_WORLD_SWITCH`, `REGS_MAX_MODE`, `VCPU_SIZE` and `VCPU_EXT_OFFSET`;
  - `save_host`/`restore_guest`/`save_guest`/`restore_host`, which hold the Pi `cpacr`/`cntkctl`/`tpidrro` asm moved verbatim out of `run_vcpu_impl`.
- **`ArmEl2Monitor`:**
  - `PRESENT`, `init`, `is_ready`, `is_verified`, `mark_verified`, `entry_el`, `probe_status`, `gateway`, `run`, `run_probed`;
  - the smoke host-tick hooks `prepare_smoke_window`, `arm_smoke_preemption` and `restore_host_tick`.
- **`DtbPolicy`**, **`MmioSpan`**, **`GuestRunProbe`** (moved out of `monitor.rs`), and the `El2Monitor` alias.
- **`monitor.rs`** was moved with `git mv` to `backend/monitor.rs`. Its only edits are the `use` paths and the shared `GuestRunProbe`. Its module gate, `any(board-rpi3, board-rpi4)`, now sits in `backend/mod.rs`. Both BCM boards still compile it, because their boot asm installs its vectors and stack; only `Bcm2837` uses it, as before.

### Layout assertions (Q1, `VcpuRegs` as `Board::VcpuExt`)

`vcpu.rs` asserts per build `offset_of!(AArch64Vcpu, ext) == VCPU_EXT_OFFSET` and `size_of::<AArch64Vcpu>() == VCPU_SIZE`. These sit beside the existing shared-ABI asserts (gp 0 … `g_sp_el0` 528). `bcm2837/vcpu_ext.rs` asserts the absolute offsets the world-switch assembly uses.

| Build | `ext` offset | `size_of::<AArch64Vcpu>()` | Pi FP offsets (absolute) |
|---|---:|---:|---|
| virt, rpi4 | 536 | 536 | — |
| rpi3 | 544 | 1632 | `g_fp_regs` 544, `g_fpcr` 1056, `h_fp_regs` 1072, `h_fpcr` 1584 |

All of these values are the pre-plan ones. All three builds compile, so all the assertions pass. The world-switch `pi` asm flag is `VcpuExt::PI_WORLD_SWITCH`, so the RPi3 alone gets the FP bank and `PI_GUEST_VI` paths, as before.

### HAL core

- `aarch64.rs`: the `monitor` module gate was removed (the Phase 02 deviation 1, closed here).
- `boot.rs`: `BOARD_BCM` and `SECONDARY_PSCI` are `const`s taken from `Board`.
- `el2.rs`: `pi_monitor_mmu_init` is ungated (`pub`). Only the monitor calls it, and it is dead code on virt.
- `paging.rs`: `El2Monitor::init(ttbr0)`, which is a no-op except on RPi3, as before.
- `stage2_regs.rs`: the four Stage-2 helpers use `if El2Monitor::PRESENT { gateway } else { direct }`.
- `vcpu.rs`: as described above.

### Kernel

- **`board.rs`:** one `AARCH64_BOARD`, chosen by a `const` `match` on `ArmResources::SOC`, with `default_aarch64_board()` and `active()`. These replace the three `default_*`/`selected_*`/`active` sets. The validation body is unchanged.
- **`boot.rs`:** `fallback_boot_info` matches on `DTB_POLICY`.
  - **`optional_dtb_boot_info`** (virt) is moved verbatim. RAM base and end come from the descriptor's fallback spans, which have the same values as `FALLBACK_KERNEL`/`FALLBACK_USABLE`. Its runtime map static is renamed `RUNTIME_MEMORY_MAP`.
  - **`conservative_dtb_boot_info`** (RPi3) is moved verbatim, log text included.
  - **`required_dtb_boot_info`** (RPi4) is moved verbatim, panic text included.
  - The BCM static map and `FALLBACK_BOOT_INFO` come from the selected descriptor. `kernel_phys_base` is `boot.kernel_load_base`. On RPi3 this replaces `fallback_memory[0].base`; a `const` assertion pins that the two are equal (both `0x80000`).
  - `dtb_memory` and the shared helper and static gates are now `any(riscv64, aarch64)`.
- **`cpu_features.rs`:** `has_el2` returns `El2Monitor::is_verified()` when the board has a monitor (`PRESENT`), and `HAS_EL2` otherwise.
- **`hypervisor/registry.rs`:**
  - `PI_MONITOR = El2Monitor::PRESENT` selects:
    - the `create_vm` refusal;
    - GICV passthrough and `vgic::enable`;
    - VI injection or GICH list-register (LR) loading and re-marking;
    - `El2Monitor::run` or `run_vcpu_impl`.
  - `vcpu_regs` mode 2 is guarded by `VcpuExt::REGS_MAX_MODE >= 2`.
  - `pi_monitor_smoke` is now gated on `target_arch` only. It calls `El2Monitor::*`, and `restore_host_tick` replaces the `bcm2835_systimer::init` and `timer::reset` pair.
- **`main.rs`:**
  - The pi-monitor log and smoke run only when `El2Monitor::PRESENT`.
  - The DTB RAM range log runs only under `DtbPolicy::Optional`.
  - The RX init runs when `CONSOLE_RX_IRQ` is set.
  - The native-domain supervisor ranges come from `SUPERVISOR_DEVICE_MMIO`.
  - The `Q` probe calls `diag_probe` (a no-op off RPi3); the `K` probe calls `irq_snapshot_probe`. The idle `L` probes run under `if IDLE_PROBES > 0`, which yields the same plain idle loop elsewhere.
- **`task/syscall.rs` `VcpuRegs`:** `max_mode = VcpuExt::REGS_MAX_MODE` on AArch64; 1 elsewhere. The permission gate, bounds and errors are unchanged.
- **Q7:**
  - `docs/generated/mutable-globals.md` is regenerated: 601 declarations. The two removed rows are the per-board duplicate boot statics; the `monitor` statics and the boot statics changed their key path or `@cfg`.
  - In `scripts/mutable-globals.toml`, the `kmain()::IDLE_COUNT` text is updated, because its declaration block changed from `#[cfg(feature = "board-rpi3")]` to the `IDLE_PROBES` block.
- **Docs:** `docs/system-architecture.md` §4 gains "AArch64 board backends".

### End-state `rg` (Main's criterion)

`rg 'feature *= *"board-rpi'` over `hal/arch/arm/src` and `kernel/src`:
- `hal/arch/arm/src/aarch64/backend/mod.rs:13,15,17,22,26,29,32` — the backend selection and the `monitor` module gate;
- `kernel/src/main.rs:13` — the board-conflict `compile_error!`.

There is no other cfg hit. The baseline had 234 sites; after Phase 01 there were 172 (Phase 02) and 126 (Phase 03).

The only other `board-rpi` text is prose in two doc comments of files moved verbatim in Phase 02:
- `backend/bcm2837/bcm2835_systimer.rs:42`;
- `backend/bcm2837/bcm2836_irq.rs:46`.

These are comments, not cfgs. They were left untouched under the content-unchanged move rule.

### File-set amendments

All edits are inside the Phase 04 set (with Q1 and Q7). The one exception is `hal/arch/arm/src/aarch64.rs`: the `monitor` gate removal is the line the Phase 02 report left for this phase.

## 2. Release `.text` (gate rows)

The commands are those of baseline §2, with `CARGO_TARGET_DIR=~/.cache/cellos-a64-p04/<variant>`. Measurements use `llvm-size -A` and the `nm -S` sum of t/T symbols. Each build has exactly 1 warning, the same future-incompat note as the baseline.

| Variant | `.text` (baseline → now) | Δ | Σ t/T (baseline → now) | Δ | t/T symbols | Inside ±1%? | ELF sha256 (first 16) |
|---|---|---:|---|---:|---|---|---|
| virt | 1 314 816 → **1 310 720** | −0.31% | 1 310 348 → **1 308 648** | −1 700 (−0.130%) | 1305 → 1298 | yes / yes | `ed614d449ac5dc31` |
| `board-rpi3` | 1 331 200 → **1 331 200** | 0 | 1 325 040 → **1 325 060** | +20 (+0.002%) | 1323 → 1319 | yes / yes | `27068c186aa5f56c` |
| `board-rpi3-bring-up` | 1 331 200 → **1 331 200** | 0 | 1 325 040 → **1 325 060** | +20 (+0.002%) | 1323 → 1319 | yes / yes | `7efe3d2ad0f368d5` |
| `board-rpi4` | 1 302 528 → **1 302 528** | 0 | 1 298 540 → **1 297 508** | −1 032 (−0.079%) | 1309 → 1297 | yes / yes | `9cb5ed22fd76bf75` |

Against Phase 03:
- **virt:** `kmain` +48 is the only non-generic change.
- **rpi3:** the monitor symbols are renamed (`hal_arm::aarch64::backend::monitor::*`) with identical sizes. `kmain` +672 comes from the inlined IRQ-snapshot probe and monitor wrappers. `disable_stage2` is now outlined (112 B). The rest is link-time-optimisation (LTO) inlining noise in IPC.
- **rpi4:** LTO moves in untouched code (`yield_cpu` and `pick_next_domain` again, `ed25519::verify`).
- The monitor symbols (`el2_mark_monitor_boot`, `__pi_monitor_vectors`, `pi_monitor_dispatch`) are present on RPi3 and RPi4 and absent on virt, exactly as before.
- `.text.boot` and `.text.vectors` are unchanged in every variant.

Reference rows, not gate rows:
- `run-rpi3.ps1` flag set: `.text` 1 347 584, Σ 1 343 736.
- Test-hooks kernel: `.text` 1 699 840, Σ 1 693 996.
- Hypervisor kernel (`qemu-virt-1g`, embedded Alpine): `.text` 1 310 720 against 1 314 816 for the same build at `6d5cdeb0c` (Σ 1 308 668 against 1 310 760).

## 3. QEMU lanes

The test-hooks, RPi3 and hypervisor builds used a detached worktree of HEAD with this diff applied, plus the gitignored `.cargo/config.toml` and a copy of `.alpine-cache`. The worktrees are removed. Logs are in `~/.cache/cellos-a64-p04/`.

| Lane | Result |
|---|---|
| L1 `BOOT_WINDOW=90 bash scripts/qemu-aarch64-test.sh <virt ELF> <disk copy>` | **PASS: aarch64 shell prompt reached**, exit 0, 0 `[fault] Cell` lines. The first 120 bytes match the baseline byte for byte, and the normalised log is **line-set identical** to the baseline (`sort`ed diff empty). `[boot] DTB RAM range` is logged as before (Optional policy), and so are the VirtIO IRQ and DHCP lines. |
| L2 build | exit 0 (`-D warnings`) |
| L2 SMP=1, run 1 | **PASS**, exit 0. `96 PASS, 0 FAIL`, `mmio-allowlist PASS`, `SMP-IPI: skipped`, 6 fixture faults (baseline: 6). |
| L2 `QEMU_SMP=2`, run 1 | **PASS**, exit 0. `hart 1 online, parked`, `SMP-IPI: PASS hart=1 epoch=1`, `96 PASS, 0 FAIL`, 6 fixture faults. No retry was needed. |
| Hypervisor (Q11): the bash analogue of CI `qemu-hypervisor-machinery` (§3.1) | **FAIL at the pre-plan baseline and with Phase 04, identically.** The VMM machinery is equivalent (see below). |

### 3.1 Hypervisor lane (Q11)

The steps follow `ci.yml:690-…`:
1. Build the cells (`app-init`, `app-cellbox`, `service-vfs`, `service-config`, `service-net`, `service-hypervisor`, `driver-virtio-blk`).
2. `bash scripts/make-hypervisor-fs.sh --skip-fetch`, which produced `kernel/src/embedded-hv` with `init` `b98f37b07d30a578…` and `kernel_fs.img` `2646a564d719502b…`.
3. Build the kernel: `RUSTFLAGS="-C relocation-model=pic -C target-feature=+bti,+paca,+pacg" EMBEDDED_OVERRIDE=kernel/src/embedded-hv cargo build --release -p cellos-kernel --features qemu-virt-1g …`.
4. `bash scripts/format-disk-hv-arm.sh`.
5. `HV_SMOKE_MODE=machinery BOOT_WINDOW=120 bash scripts/qemu-hypervisor-smoke.sh`.

For an A/B comparison, the **same** embedded image and disk were used to build and boot the kernel at the pre-plan commit `6d5cdeb0c`. Both kernels ran concurrently.

The two runs agree on every hypervisor step:
- they boot to `Init: hypervisor launched`;
- `[hv] VM created vm_id=1` (`create_vm`, Stage-2, GICV passthrough, vGIC);
- they stream the guest kernel and initrd, and log `[hv] vCPU ready — entering run loop`;
- the Linux 6.12.13 guest boots through `software IO TLB: mapped`;
- the guest then panics with `VFS: Unable to mount root fs`. Its command line asks for `rdinit=/bin/pi-guest-init`, and the stock Alpine `initramfs-virt` does not contain that file.
- `[hv] PSCI SYSTEM_OFF`, `[hv] guest exited`, `[hv] service quiesced`.

The smoke script's `KERNEL PANIC` grep matches the *guest's* panic, so both runs report `FAIL: kernel panic / cell fault detected`. The normalised logs differ only in how the guest console bytes were chunked into lines.

This is an environment or artefact issue in the lane, not a regression: the guest init that the profile names is missing from the cached initramfs. The **machinery** this phase touched behaves identically before and after: Stage-2 helpers, `run_vcpu_impl` with `NoVcpuExt`, vGIC, PSCI exit handling. **Q11's lane cannot be called a PASS until that initramfs is fixed.**

### 3.2 RPi3 — QEMU `raspi3b` (Q10(a))

The flow is the same as in Phase 01 §4. The cell image matches the baseline exactly (`kernel_fs.img` `ceacad8860cdb66e…`, `init` `4f0967fbe55b0a23…`). Five Phase 04 runs were booted in parallel with three runs of the unchanged baseline ELF (`0c0eacf5…`), so both kernels ran under the same host load.

| Run | Prompt | `[fault] Cell` | `CIS`/`V`/`K000` | `[pi-monitor]` lines | `smoke PASS; HypervisorCap open` | Filtered diff vs baseline §4 |
|---|---|---|---|---|---|---|
| Phase 04, runs 1–5 | 5/5 | 0 | 5/5 | 6 each | 5/5 | **identical ×5** |
| baseline ELF, runs 1–3 | 3/3 | 0 | 3/3 | 6 each | 3/3 | identical ×3 |

The first 160 raw bytes are identical to the baseline.

The RPi3 monitor path runs end to end in every run:
- `init` through `El2Monitor::init`;
- the `[pi-monitor] entry/probe/first-run…` lines;
- the HVC, MMIO, VI and PREEMPT smoke, through `El2Monitor::run`/`run_probed`, the smoke-window hooks, `restore_host_tick` and `mark_verified`;
- Stage-2 bind, unbind and flush through `gateway`;
- the Pi `VcpuExt` world switch (`PI_WORLD_SWITCH` asm plus `PiVcpuExt` save and restore).

The conservative DTB policy and the `K000` and idle `L` probes are byte-identical as well.

## 4. Other gate rows

| Gate | Result |
|---|---|
| x86_64 kernel release (Q9) | exit 0 |
| `bash scripts/check-board-configs.sh` (Q9) | exit 0. This covers RV64, where `boot.rs` gates now include aarch64, and the conflict cases. |
| Kernel host tests | 377 passed, 0 failed |
| `mutable-globals --check` | `601 declarations, 218 need review, 0 problems`, exit 0, after the Q7 regeneration and the `IDLE_COUNT` text update |
| Format | `cargo fmt --check` reports no hunk in lines this phase wrote. The moved `monitor.rs` keeps its pre-existing format diffs; it already had 5 at HEAD and was not reformatted. |

## 5. Deviations

1. **The plan's `ArmPlatform::DESCRIPTOR` became a kernel-side `const` `match` on `SOC`** (`board.rs`). `hal-arm` cannot name `cellos_boards` without a manifest change, and Q4 forbids manifest changes.
2. **The plan's `el2_monitor: Option<…>` became the associated type `El2Monitor`** with a `PRESENT` const. `NoEl2Monitor`'s non-trivial methods are `unreachable!`, and every call is guarded by `PRESENT`.
   - The kernel's Pi-only code (`pi_monitor_smoke`, the VI injection path, the `K`/`L` probe blocks) now compiles on every AArch64 build and is const-folded away. The symbol diff shows no Pi monitor symbol in the virt ELF.
3. **`monitor.rs` went to `backend/monitor.rs`, not `backend/bcm2837/`.** RPi4's boot asm also references the monitor's vectors and `el2_mark_monitor_boot`, so the module is shared by both BCM backends. Its gate is in `backend/mod.rs`.
4. **The DTB policy functions keep their board-specific texts** ("RPi3 DTB memory map rejected", "Raspberry Pi 4 requires a firmware DTB"), so the logs and panic messages stay byte-identical. `DtbPolicy::Conservative` and `Required` each have exactly one user today.
5. **Values derived from descriptor data instead of named constants** (identical values):
   - The BCM `FALLBACK_BOOT_INFO.kernel_phys_base` uses `boot.kernel_load_base`; RPi3 used `fallback_memory[0].base`. Both are `0x80000`, and a const assertion pins this.
   - Virt's RAM base and end use `fallback_memory[0]`/`[1]` instead of `qemu_virt_aarch64::FALLBACK_KERNEL`/`FALLBACK_USABLE`, which are the same constants.
6. **`PiVcpuExt` derives `Clone` and `Copy`,** so the unchanged `AArch64Vcpu::new` (`..Default::default()` on a `Drop` type) still compiles. **`NoVcpuExt` is a `#[repr(C)]` `[u64; 0]` newtype** so the `extern "C"` world-switch signature stays FFI-safe. `AArch64Vcpu`'s `Debug` output now prints the board fields under `ext`.
7. **The hypervisor lane fails** (§3.1) at the baseline and after this phase, for the same pre-existing artefact reason. It is reported, not fixed: fixing the guest initramfs is outside this behaviour-preserving plan.
8. **CHANGELOG was not edited** (bullet below), as instructed. Phase 04's own text listed it.

## 6. Proposed CHANGELOG bullet — whole plan (not applied)

> - aarch64: the shared AArch64 core no longer branches on `board-rpi*`. Board behaviour sits behind a static, zero-cost `hal::aarch64::backend::Board` (`QemuVirt`, `Bcm2837`, `Bcm2711`) selected in one place, covering:
>   - console and diagnostics;
>   - interrupt controller, host tick, IRQ dispatch, cross-hart IPI and secondary start;
>   - platform facts, kernel MMIO maps, MMIO-authority-table selection and driver selectors;
>   - DTB memory policy, board descriptor, Pi EL2 monitor and per-board vCPU state (`ArmVcpuExt`, layout pinned per build).
>
>   The GIC driver is parameterised by the `hal/soc` profiles. The BCM IRQ, system-timer, mini-UART and monitor code moved under `backend/`. ADR-0029 §6, plan `261031-1241-aarch64-core-backend-split` phases 01–04.
>
>   The change is behaviour-preserving. Release `.text` stays within ±0.31% of the pre-plan baseline on virt, rpi3 and rpi4. The QEMU `virt`, test-hooks (SMP=1/2) and `raspi3b` boot sequences match the baseline. No ABI or Law-1 change.
