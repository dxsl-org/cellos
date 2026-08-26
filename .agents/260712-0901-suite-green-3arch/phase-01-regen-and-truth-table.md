# Phase 01 — Regenerate 3-Arch Images + Full Suite Truth Table

**Context:** [plan.md](plan.md) · Prereq for P02, P03, P04.

## Overview

- **Priority:** P1 (gates everything else)
- **Status:** done (2026-07-13) — see [reports/truth-matrix.md](reports/truth-matrix.md)
- **Goal:** Rebuild all three arch images *in this session*, run every integration
  suite per arch, and produce a written truth matrix of pass/fail counts + exact
  red test-fn names. This is the ground truth every later phase depends on.

## Key insights

- **Build-skew is the top risk.** `gen_disk.ps1:51` explicitly warns that a stale
  binary makes "every later QEMU verify meaningless." The truth matrix is only
  valid if each arch's image was regenerated before its suites ran.
- Integration tests are **host binaries** that spawn QEMU (`run-tests.ps1`). Run
  from `tests/integration/` so its `.cargo/config.toml` overrides the root's
  riscv64 target (`run-tests.ps1:5`).
- `ci_guard` (`lib.rs:28`) HARD-FAILS a suite if `CI` is set and prerequisites are
  missing — locally it silent-skips. Do **not** set `CI` for exploratory local runs
  or a missing image panics instead of skipping.
- Some suites need a **test-hooks kernel** (not the default): `vfs-quota`,
  `redoxfs-srv`, `shell-utils` (see `ci.yml:360,391,497` → `scripts/build-*-ci.sh`).
  These must be built before their suites, or they skip.
- Suites are **arch-bound** by their boot constructor. Do not run an aarch64 suite
  against the riscv64 image.

## Per-arch regen commands (VERIFY exact invocation first)

### riscv64 — `disk_v3.img` + embedded `kernel_fs.img` + kernel
```
pwsh ./gen_disk.ps1
```
Builds all cells → signs (Ed25519 dev key) → writes `kernel_fs.img` → rebuilds
kernel with `RUSTFLAGS=-C relocation-model=pic` → writes `disk_v3.img` (MBR) +
P6 FAT cell-store. Env on non-Windows: `CC_riscv64gc_unknown_none_elf`, `OBJCOPY`.

### aarch64 — embedded `kernel_fs.img` + `disk_arm_virt.img` + kernel
```
pwsh ./scripts/build-aarch64-cells.ps1     # embedded kernel_fs.img + init
# rebuild kernel:
$env:RUSTFLAGS = "-C relocation-model=pic -C target-feature=+bti,+paca,+pacg"
cargo build --release -p vicell-kernel --target aarch64-unknown-none-softfloat -Z build-std=core,alloc
$env:RUSTFLAGS = $null
pwsh ./scripts/format-disk-arm.ps1         # disk_arm_virt.img (needs mtools: mformat/mcopy/mmd)
```
NOTE: the aarch64 kernel loads cells from the **embedded ramdisk**
(`kernel/src/embedded-aarch64/kernel_fs.img`), so `build-aarch64-cells.ps1` +
kernel rebuild is the load-bearing step; `disk_arm_virt.img` is the supplemental
`/data` + `/bin` overlay (and carries `/bin/net-broker` for cluster-boot).

### x86_64 — `build/vicell-x86.iso`
```
pwsh ./run-x86.ps1 -NoQemu                 # build-x86_64-cells + Limine ISO, no launch
```
CI equivalent: `scripts/x86/make-iso-ci.sh build/vicell-x86.iso`.

## Suite → arch map (from inventory)

| Arch | Suites (test count) | Notes |
|------|--------------------|-------|
| riscv64 | boot (53), handoff (rv64 subset of 26), hotswap-smoke (11), nic-riscv (1), compositor-cursor (1), redoxfs-srv (3†), shell-utils (1†), vfs-quota (1†), tls-gate (1), hypha-boot (1), hypha-p3-boot (1), http-smoke (1) | † needs test-hooks/srv/shell-test kernel |
| aarch64 | aarch64-boot (7), handoff (aarch64 subset), periph-can-pwm-adc (3), periph-i2c-spi (2), robot-demo-e2e (2), cluster-boot (2, 1 ignored) | uses `disk_arm_virt.img` |
| x86_64 | x86_64-boot (7), handoff (x86 subset), nvme-x86 (3), nic-x86 (2), virtio-x86 (2, 1 ignored) | `pwsh ./scripts/ci-x86-integration.ps1` |

## Implementation steps

1. **Prereq check.** Confirm `qemu-system-{riscv64,aarch64,x86_64}` resolve
   (`lib.rs` `qemu_binary*`), `python`, `mtools` (aarch64), WSL+xorriso+limine (x86).
   Record versions in the matrix header (QEMU version affects input routing — P02 H2).
2. **Regen riscv64** via `gen_disk.ps1`. Capture the "Done. disk_v3.img is ready."
   line and note any "optional cell FAILED" warnings (gen_disk.ps1:516) — a stale
   optional binary is a footnote, not a blocker, but must be recorded.
3. **Regen aarch64** (3 sub-commands above). If mtools absent, fall back to WSL
   `bash scripts/format-disk-arm.sh disk_arm_virt.img`.
4. **Regen x86** via `run-x86.ps1 -NoQemu`.
5. **Record image mtimes** (`disk_v3.img`, `disk_arm_virt.img`,
   `kernel/src/embedded*/kernel_fs.img`, `build/vicell-x86.iso`) — the truth matrix
   cites these so a reader can prove no cell ran against a stale artifact.
6. **Build test-hooks kernels** for the †-marked riscv64 suites before running them.
7. **Run each suite** from `tests/integration/`:
   - riscv64: `cargo test --test boot -- --test-threads=2` (TCG is CPU-bound; each
     test boots its own QEMU), then each other suite.
   - aarch64: run the aarch64-bound suites.
   - x86: `pwsh ./scripts/ci-x86-integration.ps1` (all 5 suites).
8. **For every failure**, capture the exact `#[test]` fn name, the panic line, and
   run the **footgun triage**: is the failing `wait_for` a NO-OP barrier or matching
   the command's own echo? Tag each red `harness-suspect` | `real-regression` |
   `known-expected`.
9. **Write `reports/truth-matrix.md`**: rows = suites, columns per arch =
   `pass/total` + red fn-names + triage tag + image-mtime.

## Data flow

`regen script → image artifact (mtime recorded) → cargo test (host) → QEMU boot →
serial TCP capture → wait_for oracle → pass/fail → matrix cell`.

## Related code files

- Modify: none (source-read-only). Regenerates: `disk_v3.img`, `disk_arm_virt.img`,
  `kernel_fs.img`, `build/vicell-x86.iso`.
- Create: `reports/truth-matrix.md`.
- Read: `tests/integration/src/lib.rs`, all `tests/integration/tests/*.rs`,
  `gen_disk.ps1`, `scripts/build-aarch64-cells.ps1`, `scripts/format-disk-arm.ps1`,
  `run-x86.ps1`, `scripts/ci-x86-integration.ps1`.

## Todo

- [x] Prereq + version check recorded
- [x] riscv64 image regenerated (mtime captured)
- [x] aarch64 image + embedded kfs + kernel regenerated (mtime captured)
- [x] x86 ISO regenerated (mtime captured)
- [x] test-hooks kernels built for vfs-quota/redoxfs-srv/shell-utils — **SKIPPED**: `riscv64-unknown-elf-gcc` unavailable on Windows + WSL (pre-existing CI toolchain gap)
- [x] All riscv64 suites run + reds captured
- [x] All aarch64 suites run + reds captured
- [x] All x86 suites run + reds captured (no `ci-x86-integration.ps1` exists — ran each `cargo test --test <suite>` directly)
- [x] Every red triaged (harness / real / expected) — 2 real regressions found (hypha Grant-syscall denial, RV32 virtio_drivers compile break), 1 tooling gap (aarch64 demo cells never built), 1 confirmed flake, 1 needs-investigation, 1 pre-existing/known
- [x] `reports/truth-matrix.md` written

## Success criteria

- Every suite × arch has a matrix cell with `pass/total`, red fn-names, triage tag,
  and the image mtime it ran against.
- The two input reds' current status is explicit (red or green on fresh images).
- char-8 reproduction status is explicit (does any boot.rs test stall mid-line?).
- Known-expected non-reds documented and excluded: aarch64 GPIO IRQ QEMU limitation
  (not a red); UDP broadcast/multicast (blocked on SLIRP — out of scope, note only);
  `#[ignore]`d tests (virtio-x86 blk, cluster-boot one).

## Risk assessment

| Issue | Mitigation |
|-------|-----------|
| Stale image → false result | Regen-in-session gate; mtime in every cell |
| aarch64 mtools missing | WSL `format-disk-arm.sh` fallback |
| test-hooks suite skips silently | Build test-hooks kernel first; assert not-skipped |
| Optional cell build fail ships stale binary | Record gen_disk warnings; exclude affected tests |
| TCG flake | `--test-threads=2`, generous windows, re-run reds 2/2 |

## Security considerations

None new — regeneration re-signs cells with the fixed dev Ed25519 seed
(`gen_disk.ps1:150`); no secret material introduced.

## Next steps

Feeds P02 (input reds' exact status + which arch) and P03 (char-8 reproduction).
