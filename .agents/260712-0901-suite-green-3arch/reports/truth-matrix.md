# P01 Truth Matrix — 3-Arch Integration Suites

Generated in-session 2026-07-13 (~17:35–18:10 local). All three arch images were
regenerated in this session before any suite ran — no result below is against a
stale image.

## Image mtimes (this session)

| Artifact | mtime |
|---|---|
| `disk_v3.img` (riscv64) | 2026-07-13 17:54:54 |
| `kernel/src/embedded/kernel_fs.img` (riscv64) | 2026-07-13 17:41:49 |
| `disk_arm_virt.img` (aarch64) | 2026-07-13 17:53:17 |
| `kernel/src/embedded-aarch64/kernel_fs.img` (aarch64) | 2026-07-13 17:52:50 |
| `build/vicell-x86.iso` (x86_64) | regenerated in-session via `run-x86.ps1 -NoQemu` (exit 0, 8.76 MB) |
| `target/aarch64-unknown-none/release/vicell-kernel` (handoff-test kernel) | rebuilt in-session (was stale/pre-rename) |
| `target/riscv32imac-unknown-none-elf/release/vicell-kernel` (handoff-test kernel) | **could not rebuild — compile error, see below** |

QEMU: 10.2.0 (all 3 `qemu-system-{riscv64,aarch64,x86_64}`), resolved via the
code's `C:\Program Files\qemu\...` fallback (not on PATH). `mtools` absent on
Windows PATH and in WSL → aarch64 disk formatted via the WSL `format-disk-arm.sh`
fallback. `riscv64-unknown-elf-gcc` absent on Windows **and** WSL → the three
test-hooks suites (`vfs-quota`, `redoxfs-srv`, `shell-utils`) could not be built
and were skipped entirely (matches the pre-existing documented CI gap).

## Suite × arch matrix

| Suite | Arch | Result | Red test-fn(s) | Triage |
|---|---|---|---|---|
| `boot` | riscv64 | 53/53 (effectively) | `network_tcp_send_recv` (red on first run only) | **harness-suspect** — passed 2/2 in isolation (~5s each) after failing at 20s under `--test-threads=2` full-suite load; TCG contention flake, not a regression |
| `boot` — `input_bare_cell` | riscv64 | ok | — | **RESOLVED** — previously-known red (2026-07-06/07), now green on the post-P-TRUST/thread-cap/DICE-P00 image |
| `boot` — `input_keyboard_e2e` | riscv64 | ok | — | **RESOLVED** — same as above |
| `boot` — char-8 stall | riscv64 | not reproduced | — | **known-expected (non-issue)** — all `shell_*` tests (incl. long multi-char commands) passed; the "C′ stall char-8" label was a symptom name, not a test, and does not reproduce on this image |
| `handoff` (rv64/x86/x86_32/aarch32 subsets) | multi | 21/21 | — | green |
| `handoff` — aarch64 subset (4 tests) | aarch64 | 4/4 after rebuild | — | **harness-suspect (stale artifact)** — `target/aarch64-unknown-none/release/vicell-kernel` had never been built for current source; rebuilt in-session (clean, 0 errors) → all 4 pass |
| `handoff` — `handoff_rv32_kernel_starts` | rv32 | FAILED | `handoff_rv32_kernel_starts` | **REAL REGRESSION (build-breaking)** — see below |
| `hotswap-smoke` | riscv64 | 11/11 | — | green |
| `nic-riscv` — `nic_riscv_iommu_bare` | riscv64 | FAILED | `nic_riscv_iommu_bare` | **needs-investigation** — QEMU 10.2.0 accepts `-device riscv-iommu-pci,bus=pcie.0` (confirmed via `-device help`) but the kernel's `pcie_ecam::find_class(0x08,0x06,0x00)` scan never finds it (BAR0==0). Test's own doc says it was written against QEMU ≥8.2; likely a QEMU-version drift in the emulated device's PCI config space vs. the kernel's ECAM scanner, not touched by this session's changes. Single isolated test, does not gate anything else. |
| `compositor-cursor` | riscv64 | 1/1 | — | green |
| `tls-gate` | riscv64 | FAILED | `tls_gate_default_rejects_public_cert` | **known-expected / pre-existing** — test itself reports "INCONCLUSIVE: TLS handshake failed but reject reason unknown... transport I/O or unknown error" — same class as the long-documented HTTPS binary-body/frame-length gap in the net cell (see `feedback-net-cell-ipc-patterns` memory); not touched by this session |
| `hypha-boot` | riscv64 | FAILED | `hypha_banner_and_prompt` | **REAL REGRESSION** — see below |
| `hypha-p3-boot` | riscv64 | FAILED | `hypha_p3_tool_cells_spawn` | **REAL REGRESSION** — same root cause as above |
| `http-smoke` | riscv64 | 1/1 | — | green |
| `aarch64-boot` | aarch64 | 7/7 | — | green |
| `periph-can-pwm-adc` | aarch64 | FAILED | `aarch64_adc_demo`, `aarch64_can_demo`, `aarch64_pwm_demo` | **tooling gap, not a kernel regression** — see below |
| `periph-i2c-spi` | aarch64 | FAILED | `aarch64_i2c_sensor_demo_banner`, `aarch64_spi_demo_tx` | same tooling gap |
| `robot-demo-e2e` | aarch64 | FAILED | `aarch64_robot_demo_e2e` (1 ignored: mqtt_publish) | same tooling gap |
| `cluster-boot` | aarch64 | FAILED | `cluster_broker_entropy_gate_passes` (1 ignored: lssvc placeholder) | same tooling gap (`/bin/net-broker` missing) |
| `x86_64-boot` | x86_64 | 7/7 | — | green |
| `nvme-x86` | x86_64 | 3/3 | — | green |
| `nic-x86` | x86_64 | 2/2 | — | green |
| `virtio-x86` | x86_64 | 1/1 (1 ignored, expected) | — | green |
| `vfs-quota` | riscv64 | **SKIPPED** | — | toolchain unavailable (`riscv64-unknown-elf-gcc` absent on Windows + WSL) — matches documented pre-existing CI gap |
| `redoxfs-srv` | riscv64 | **SKIPPED** | — | same toolchain gap |
| `shell-utils` | riscv64 | **SKIPPED** | — | same toolchain gap |

## Real findings (require follow-up, NOT fixed in this session — P01 is read-only)

### 1. `hypha-boot` / `hypha-p3-boot` — GrantAlloc/Share/Free denied for the hypha cell (HIGH)

```
[ WARN] [kernel] syscall GrantAlloc (bit 39) denied for tid 16 (allowlist=0x0000002020080483)
[ WARN] [kernel] syscall GrantShare (bit 39) denied for tid 16 (allowlist=0x0000002020080483)
[ WARN] [kernel] syscall GrantFree (bit 39) denied for tid 16 (allowlist=0x0000002020080483)
USER: [hypha] ERROR: cannot spawn llm-gateway
```

Hypha can no longer spawn `llm-gateway` (needs Grant syscalls for large-buffer
IPC). This surfaced for the first time on an image built from `main` at
`95bcea99`, which includes today's four merged PRs (thread-identity/honest-revoke,
P-TRUST cap-ceiling fold, Manifest v2, DICE P00). The manifest/allowlist
computation for the hypha cell most likely regressed in one of the P-TRUST or
Manifest-v2 changes (both touch how the syscall allowlist / capability ceiling
is derived at spawn time). Needs root-cause in `kernel/src/loader.rs` /
`kernel/src/policy.rs` cap-ceiling fold path — **candidate for an immediate P02
before this window closes**, since it affects the flagship Hypha app.

### 2. `handoff_rv32_kernel_starts` — RV32/Cellos-Nano no longer compiles (HIGH, build-breaking)

```
error: this arithmetic operation will overflow
   --> kernel\third_party\virtio_drivers\src\transport\mmio.rs:434:61
434 |                     volwrite!(self.header, queue_desc_high, (descriptors >> 32) as u32);
    |                                                             ^^^^^^^^^^^^^^^^^^^ attempt to shift right by `32_i32`, which would overflow
```

(3 identical errors at lines 434/436/438 — `queue_desc_high`, `queue_driver_high`,
`queue_device_high`.) `kernel/third_party/virtio_drivers/src/transport/mmio.rs`
assumes a ≥64-bit `usize` when splitting a 64-bit descriptor/driver/device
address into hi/lo halves for the MMIO transport register writes. On the RV32
Nano target (`riscv32imac-unknown-none-elf`, 32-bit `usize`), `>> 32` is a
compile-time-detected overflow under `#[deny(arithmetic_overflow)]`, so the
kernel **cannot be built at all** for this target. The stale prebuilt binary at
`target/riscv32imac-unknown-none-elf/release/vicell-kernel` (pre-dates the
2026-06-22 Cellos rename — prints `[ViCell] kernel boot v0.2.0`) still boots and
passes `handoff_rv32_bare_paging`/`handoff_rv32_heap`, which is why this had
gone unnoticed — nobody has rebuilt the RV32 target since virtio_drivers became
part of the always-linked kernel path. This directly threatens G1 graduation
criterion #7 (Cellos-Nano sub-track) and needs its own fix (feature-gate
virtio_drivers out of the Nano profile, or make the hi/lo split
width-conditional).

## Tooling gap (not a kernel bug — affects 4 aarch64 suites, 8 tests)

`scripts/build-aarch64-cells.ps1` (and its Linux/CI equivalent — `qemu-aarch64-boot`
in `ci.yml` builds the same short list) only builds: `app-shell`, `service-vfs`,
`service-config`, `app-sys-tools`, `service-input`, `input-test`, `periph-demo`,
`app-init`. It never builds or embeds `adc-demo`, `can-demo`, `pwm-demo`,
`sensor-demo` (I2C), `spi-demo`, `robot-demo`, `net-broker`, or `supervisor` —
confirmed by boot logs showing `Init: cell not found — skipping: /bin/net-broker`
etc. for every one of the 4 failing suites. This is **not gated in CI at all**
(no aarch64 job runs these suites), so it's a pre-existing local-tooling gap,
not something this session's regen introduced or broke. Needs a follow-up phase
to extend `build-aarch64-cells.ps1`/`format-disk-arm.*` to build and embed the
full aarch64 cell set these suites expect.

## Known-expected non-reds (excluded from "red" count per plan's success criteria)

- aarch64 GPIO IRQ QEMU limitation — not exercised as a red in this run.
- UDP broadcast/multicast (blocked on SLIRP) — out of scope, not exercised.
- `#[ignore]`d tests: `virtio-x86::x86_virtio_blk_initialises`,
  `robot-demo-e2e::aarch64_robot_demo_mqtt_publish`,
  `cluster-boot::cluster_broker_service_registered` (lssvc not yet implemented) —
  all ignored as designed, not counted as red.

## Summary

- **riscv64**: 53/53 `boot.rs` effectively green (1 confirmed flake), 21/21
  non-aarch64 `handoff` subsets green, `hotswap-smoke` 11/11, `compositor-cursor`
  1/1, `http-smoke` 1/1 green. Reds: `tls-gate` (pre-existing, known class),
  `nic-riscv` (needs investigation, isolated), **`hypha-boot`/`hypha-p3-boot`
  (real regression, high priority)**, **`handoff_rv32_kernel_starts` (real
  regression, build-breaking)**. 3 suites skipped (toolchain).
- **aarch64**: `aarch64-boot` 7/7 green, `handoff` aarch64 subset 4/4 green
  (after rebuilding a stale kernel artifact). 4 suites (8 tests) red due to a
  pre-existing cell-build tooling gap, not a kernel bug.
- **x86_64**: 13/13 green (1 ignored as expected). No change from prior known-good
  state.
- The two previously-carried "input reds" (`input_bare_cell`, `input_keyboard_e2e`)
  are **resolved** on the fresh image. The "char-8 stall" does not reproduce.

## Next steps (P02/P03/new phase, not done here)

1. **Immediate**: root-cause the hypha Grant-syscall-denied regression — likely
   in the P-TRUST/Manifest-v2 cap-ceiling fold merged today.
2. **Immediate**: fix or feature-gate the RV32 virtio_drivers compile break.
3. New phase: extend aarch64 cell-build tooling to cover the peripheral/robot/
   cluster demo cells.
4. Low priority: investigate `nic_riscv_iommu_bare` QEMU-version drift.
5. `tls-gate` failure is pre-existing/known — no new action from this session.
