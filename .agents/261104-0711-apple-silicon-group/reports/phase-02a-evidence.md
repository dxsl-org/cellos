# Phase 02a evidence — arm64 Image entry and VHE host

**Captured:** 2026-11-05, slice A1, branch `feat/apple-a1-vhe-entry` on base `6fee21927`.
**Ceiling:** QEMU 8.2.2 TCG (`virt`, `raspi3b`) on WSL2 Linux 6.18 x86_64. RPi4 compile-only. No KVM, no hardware.
**Method:** the baseline is a detached worktree of `6fee21927` built and run side by side with the change. Both sides used the same embedded inputs (`kernel/src/embedded-aarch64/{init,kernel_fs.img}` assembled by the CI recipe, identical bytes on both sides), the CI codegen flags (`-C relocation-model=pic -C target-feature=+bti,+paca,+pacg`), separate `CARGO_TARGET_DIR`s, and per-run disk copies. Artefacts: `~/.cache/cellos-a1/` (not durable).

## 1. Group-1 `.text` (gate: ±1%)

Commands as in the split plan's [baseline](../../261031-1241-aarch64-core-backend-split/reports/baseline-2026-11-02.md) §2 (`cargo build --release -p cellos-kernel --target aarch64-unknown-none-softfloat -Z build-std=core,alloc [--features …]`, `aarch64-linux-gnu-size -A`, `nm -S` t/T sum).

| Variant | `.text` before → after | Σ t/T before → after | `.text.boot` before → after |
|---|---|---|---|
| virt | 1 339 392 → 1 339 392 (0.000%) | 1 336 516 → 1 336 700 (+0.014%) | 384 → 572 |
| `board-rpi3` | 1 351 680 → 1 351 680 (0.000%) | 1 348 524 → 1 348 552 (+0.002%) | 392 → 392 |
| `board-rpi3-bring-up` | 1 351 680 → 1 351 680 (0.000%) | 1 347 936 → 1 347 964 (+0.002%) | 392 → 392 |
| `board-rpi4` | 1 310 720 → 1 310 720 (0.000%) | 1 308 116 → 1 308 120 (+0.000%) | 492 → 492 |
| virt, `CELLOS_AARCH64_HOST_REGIME=vhe` (new) | — → 1 339 392 | — → 1 336 548 | — → 524 |

- virt `.text.boot` +188 B = Image header (64) + `HCR_EL2` read-back (8) + non-VHE EL2 secondary install (116).
- RPi3 and RPi4 `.text.boot`: `objdump -d` of before and after is identical instruction for instruction once branch targets are normalised.
- These absolute numbers differ from the 2026-11-02 baseline because that capture embedded different inputs. Measured here: building with the stub `kernel_fs.img` instead of the assembled one moves virt `.text` by 49 152 B (3.8%), so a ±1% comparison is only valid with identical embedded inputs.

## 2. QEMU `virt` lanes (`scripts/qemu-aarch64-test.sh`, `BOOT_WINDOW=120`)

| Run | Kernel | Machine / CPU / SMP | Result | Decisive lines |
|---|---|---|---|---|
| L1 before | baseline ELF | `virt` / cortex-a57 / 1 | PASS | `PSCI conduit Hvc, kernel at EL1`; `USER: Cellos >`; 0 `[fault] Cell` |
| L1 after | ELF | same | PASS | same lines; `DTB RAM range 0x0..0x0` as before |
| Image after | `cellos-kernel.Image` | same | PASS | `[Cellos] kernel boot v0.2.1`; `DTB RAM range 0x40000000..0x50000000` (FDT taken from x0); `USER: Cellos >` |
| `LANE=virt-vhe` | VHE `Image` | `virt,virtualization=on,gic-version=2` / cortex-a76 / 2 | PASS (`aarch64-virt-vhe boot markers present`) | `[INFO] host regime: EL2 VHE (E2H=1 TGE=1)`; `PSCI conduit Smc, kernel at EL2`; `[smp] hart 1 online, parked`; `SMP-IPI: PASS hart=1 epoch=1`; `USER: Init: OS ready.`; `USER: Cellos >`; 0 faults |
| `LANE=virt-vhe QEMU_CPU=max` | VHE `Image` | same, `-cpu max` | PASS | same lines (assumption A2: `-cpu max` provides VHE — holds) |
| non-VHE EL2, 2 harts | non-VHE `Image` | as virt-vhe | prompt reached, 0 faults; the lane's VHE markers absent as expected | `PSCI conduit Smc, kernel at EL2`; `hart 1 online, parked`; `SMP-IPI: PASS` |
| negative | VHE `Image` | `virtualization=on`, cortex-a57 (no FEAT_VHE) | stops at entry, as designed | `[aarch64] host regime: CPU lacks VHE, backend requires E2H` |

The opposite check (`CPU forces E2H, backend requires non-VHE`) cannot be exercised here: QEMU 8.2.2 has no FEAT_E2H0 model. It is covered only by the compile-time verdict table in `backend/mod.rs`.

Selector validation (`cargo check -p hal-arm`):
- `CELLOS_AARCH64_HOST_REGIME=bogus` fails with `evaluation panicked: CELLOS_AARCH64_HOST_REGIME must be absent, \`nonvhe\` or \`vhe\``.
- `CELLOS_AARCH64_HOST_REGIME=vhe --features board-rpi3` fails with `… selects the QEMU virt host regime only`.

Lint: `cargo clippy -p cellos-kernel --target aarch64-unknown-none-softfloat -Z build-std=core,alloc -- -D warnings` (the CI `aarch64-lint` command) exits 0 for the default build and for `CELLOS_AARCH64_HOST_REGIME=vhe`. `cargo fmt --check -p hal-arm` exits 0. The same clippy command with `--features board-rpi3` (not a CI lane) reports 4 errors, all in files this slice does not touch (`backend/monitor.rs`, `backend/bcm2837/trap.rs`).

## 3. Hypervisor lane (`scripts/qemu-hypervisor-smoke.sh`, machinery mode, `BOOT_WINDOW=300`)

| Run | Kernel | CPU | Result |
|---|---|---|---|
| before | baseline, non-VHE | cortex-a72 (default) | `PASS: Alpine guest shell prompt '~ #' reached` |
| after | non-VHE | cortex-a72 (default) | `PASS: Alpine guest shell prompt '~ #' reached` |
| after | VHE (`_EL12` world switch) | `HV_SMOKE_CPU=cortex-a76` | `PASS: Alpine guest shell prompt '~ #' reached`; log shows `host regime: EL2 VHE` and `[hv] vCPU ready` |

Both the before kernel and an after non-VHE kernel also passed on cortex-a76. That after kernel was an intermediate build: it differed from the final one only in the non-VHE EL2 block of `_secondary_entry`, which a single-CPU run never executes. This isolated the one VHE-only failure found during the work (deviation log, PAN).

## 4. `raspi3b` (run-rpi3.ps1 kernel-direct reproduction, RPi3 cell image from `build-aarch64-cells.ps1 -BoardRpi3`)

- Before and after both reach `USER: Cellos >` with 0 `[fault] Cell`, and both logs are 171 lines.
- Normalise hex and decimal numbers, then compare. The sorted logs differ only where the asynchronous `QK000L`/`LLL` probe bytes and the `qemu … terminating` line interleave differently with `USER:` lines. The ordered logs differ only in where two periodic `[net]` lines fall.

## 5. Local test-hooks lanes (not in CI; regression context only)

- `scripts/qemu-aarch64-test-hooks.sh`, SMP=1: before and after both print `PASS: AArch64 test-hooks self-tests passed (semihosting enabled)` and `[vfs-test] Results: 96 PASS, 0 FAIL`.
- Development-Silo lane (`CELLOS_AARCH64_TEST_HOOKS_DEVELOPMENT_SILO=1`, `BOOT_WINDOW=150`): **fails identically on the baseline and on the change** with `[aarch64] kernel trap ec=0x25 esr=0x96000006 … far=0x100000` followed by `KERNEL PANIC`. The sorted, number-normalised logs have no differences. This failure predates the slice.

## 6. Security: isolation under the VHE host

- **EL0 isolation.** EL0 Cells and the kernel share the EL2&0 regime through `TTBR0_EL2` and the kernel root. Its descriptors use EL1&0 semantics:
  - non-USER leaves have `AP[1]=0`, so EL0 cannot access them;
  - USER leaves carry `PXN`, so the kernel cannot execute Cell pages;
  - non-executable leaves carry `UXN|PXN`.

  The non-VHE EL2 host instead mirrors the root into the EL1&0 regime. [INFERENCE, Arm ARM `HCR_EL2.TGE`: with `{E2H,TGE}={0,1}` the PE treats `SCTLR_EL1.M` as 0, so that mirror does not translate EL0 at all.] On that reading, the VHE host's EL0 isolation is at least as strong as the non-VHE arrangement. It is not weaker.
- **TTBR1_EL2.** `TCR_EL2.EPD1=1`, so nothing is reachable through TTBR1. The kernel has no upper-half mappings.
- **PAN.**
  - `SCTLR_EL2.SPAN=1`, so the host never sets PAN on exception entry. This matches the EL1 host and the non-VHE EL2 host, neither of which uses PAN.
  - Every lower-EL vector clears an inherited `PSTATE.PAN` (deviation log).
  - Enabling PAN for the host is a hardening step outside 02a.
- **UAO.** It only changes `LDTR`/`STTR`, and the kernel has none (`grep` over `hal/` and `kernel/src`).
- **Private roots.** Tier-2 private roots stay closed at EL2, exactly as on the non-VHE host (`context.rs` asserts a zero root at EL2, and `domain_admission` refuses at EL2).

## 7. Provenance

This session read no Apple Silicon source of any class (m1n1, Asahi, OpenBSD, Apple binaries). The `Image` header follows the documented arm64 boot protocol (Linux `Documentation/arch/arm64/booting.rst`: field offsets, flags, magic). VHE behaviour follows the Arm architecture (`HCR_EL2.E2H`, `_EL12`/`_EL02` encodings, PAN/SPAN rules). There is no Apple-derived behaviour, so this slice adds no ledger row.
