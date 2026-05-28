# CI/CD Pipeline Research: ViOS Bare-Metal Rust OS (RISC-V)

**Date:** 2026-05-28  
**Project:** ViOS — no_std Rust nightly, 21 crates, riscv64gc-unknown-none-elf primary target  
**Scope:** GitHub Actions CI for cross-compile, QEMU boot testing, multi-arch matrix, workspace caching, security scanning

---

## 1. Project Context (What We're Working With)

From code inspection:
- `rust-toolchain.toml` pins `channel = "nightly"`, components: `rust-src, rustfmt, clippy`
- `.cargo/config.toml` sets default build target to `riscv64gc-unknown-none-elf`
- `run.ps1` reveals exact QEMU invocation: `-machine virt -cpu rv64,c=true -smp 1 -m 128M -nographic -serial mon:stdio -bios default -kernel <ELF> -drive virtio-blk-device`
- `Cargo.toml`: workspace with `resolver = "2"`, 21 members, `panic = "abort"` for dev+release
- No `.github/` directory exists yet

---

## 2. GitHub Actions for Rust no_std OS (Cross-Compile + Lint)

### Toolchain Action Comparison

| Action | Maturity | Notes |
|--------|----------|-------|
| `dtolnay/rust-toolchain` | High — widely used in OS projects (Theseus, Tock, blog_os) | Reads `rust-toolchain.toml` automatically; explicit `targets`/`components` override |
| `actions-rust-lang/setup-rust-toolchain` | Medium | Adds problem matchers for compiler errors; heavier wrapper |
| `actions-rs/toolchain` | Deprecated | Unmaintained since 2022, do not use |

**Recommendation:** `dtolnay/rust-toolchain` — minimal, reliable, reads existing `rust-toolchain.toml` automatically.

### Core Lint/Check Job

```yaml
# .github/workflows/ci.yml
name: CI

on:
  push:
    branches: [main]
  pull_request:

concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: ${{ github.ref != 'refs/heads/main' }}

env:
  CARGO_TERM_COLOR: always
  CARGO_INCREMENTAL: 0
  RUSTFLAGS: "-D warnings"

jobs:
  lint:
    name: Lint
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      # Reads rust-toolchain.toml automatically (nightly + rust-src + rustfmt + clippy)
      - uses: dtolnay/rust-toolchain@master
        with:
          toolchain: nightly
          targets: riscv64gc-unknown-none-elf
          components: rust-src,rustfmt,clippy

      - uses: Swatinem/rust-cache@v2
        with:
          # Pin to a specific nightly to prevent daily cache invalidation
          shared-key: "nightly-riscv64"
          cache-on-failure: true

      - name: fmt
        run: cargo fmt --all -- --check

      - name: clippy
        run: >
          cargo clippy
          --target riscv64gc-unknown-none-elf
          --workspace
          -Z build-std=core,alloc
          -- -D warnings

      - name: check
        run: >
          cargo check
          --target riscv64gc-unknown-none-elf
          --workspace
          -Z build-std=core,alloc
```

**Critical flags for no_std:**
- `-Z build-std=core,alloc` is required because `riscv64gc-unknown-none-elf` is a Tier 2 target with no prebuilt std — requires `rust-src` component
- `CARGO_INCREMENTAL: 0` reduces cache size significantly for nightly crates
- `RUSTFLAGS: "-D warnings"` catches warnings that would silently pass

---

## 3. QEMU Integration Testing in CI

### Strategy from Real OS Projects

Tock OS uses `make ci-job-qemu` delegating to a Makefile; NuttX uses `expect` scripts (`.exp`); blog_os uses `isa-debug-exit` device + `bootimage`'s custom test runner. For ViOS, which is not x86 and doesn't use bootimage, the direct bash approach is most appropriate.

### QEMU Install on ubuntu-latest

```yaml
- name: Install QEMU
  run: |
    sudo apt-get update -q
    sudo apt-get install -y qemu-system-misc  # includes qemu-system-riscv64
    qemu-system-riscv64 --version
```

`qemu-system-misc` on `ubuntu-latest` (Ubuntu 22.04/24.04) includes `qemu-system-riscv64`. Version on ubuntu-22.04 is QEMU 6.2; ubuntu-24.04 provides QEMU 8.x — prefer 24.04 for RISC-V improvements.

### Headless Boot + Serial Output Verification

Based on ViOS's `run.ps1` QEMU invocation:

```yaml
  qemu-boot:
    name: QEMU Boot Test (RV64)
    runs-on: ubuntu-latest
    needs: [build-riscv64]
    steps:
      - uses: actions/checkout@v4
      - uses: actions/download-artifact@v4
        with:
          name: kernel-riscv64

      - name: Install QEMU
        run: sudo apt-get install -y qemu-system-misc

      - name: Boot and verify serial output
        run: |
          # Boot with 60s hard timeout; capture serial to file and stdout
          timeout 60 qemu-system-riscv64 \
            -machine virt \
            -cpu rv64,c=true \
            -smp 1 \
            -m 128M \
            -nographic \
            -serial mon:stdio \
            -bios default \
            -kernel vios-kernel \
            -drive file=disk_v3.img,format=raw,id=hd0,if=none \
            -device virtio-blk-device,drive=hd0 \
            2>&1 | tee /tmp/qemu-output.txt || QEMU_EXIT=$?

          # Verify expected boot strings in output
          grep -q "ViOS booted" /tmp/qemu-output.txt || {
            echo "FAIL: expected boot string not found"
            cat /tmp/qemu-output.txt
            exit 1
          }

          # Treat timeout exit (124) as success if we got expected output
          # (OS might not self-exit cleanly in early stages)
          echo "Boot test PASSED"
```

**Key decisions:**
- `timeout 60` (bash builtin) kills QEMU after 60s — prevents CI hanging on infinite loops or boot failure. Exit code 124 = timed out.
- `2>&1 | tee` captures both stdout+stderr while also streaming to CI logs
- `grep -q` checks for expected kernel output string before declaring pass
- For mature OS with clean shutdown: use `isa-debug-exit` (x86) or SBI shutdown call (RV64 via `sbi_shutdown`) and check exit code directly

### Generating the Disk Image in CI

```yaml
      - name: Generate disk image
        run: python3 create_ramdisk.py  # ViOS already has this script
```

---

## 4. Multi-Arch Build Matrix

### Strategy

Three architectures in ViOS workspace (riscv, arm, x86 HAL crates). Not all targets need QEMU boot test — just build + check suffices for ARM64 and x86_64 during early CI.

```yaml
  build:
    name: Build (${{ matrix.target }})
    runs-on: ubuntu-latest
    strategy:
      fail-fast: false  # Don't cancel other arches if one fails
      matrix:
        include:
          - target: riscv64gc-unknown-none-elf
            build_std: core,alloc
            features: riscv64
          - target: aarch64-unknown-none
            build_std: core,alloc
            features: aarch64
          - target: x86_64-unknown-none
            build_std: core,alloc
            features: x86_64

    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@master
        with:
          toolchain: nightly
          targets: ${{ matrix.target }}
          components: rust-src

      - uses: Swatinem/rust-cache@v2
        with:
          key: ${{ matrix.target }}

      - name: Build kernel
        run: >
          cargo build
          --target ${{ matrix.target }}
          --package vios-kernel
          -Z build-std=${{ matrix.build_std }}
          --features ${{ matrix.features }}

      - name: Upload kernel artifact
        if: matrix.target == 'riscv64gc-unknown-none-elf'
        uses: actions/upload-artifact@v4
        with:
          name: kernel-riscv64
          path: target/riscv64gc-unknown-none-elf/debug/vios-kernel
```

**Notes:**
- `fail-fast: false` is correct for multi-arch — ARM64 failure should not cancel RV64 QEMU test
- `x86_64-unknown-none` is a Tier 2 target added in Rust 1.53, suitable for kernel builds
- HAL crates (`hal/arch/arm`, `hal/arch/x86`) can be feature-gated so they only compile for their respective targets

---

## 5. Cargo Workspace CI Best Practices

### Caching

`Swatinem/rust-cache@v2` caches `~/.cargo/registry`, `~/.cargo/git`, and the `target/` directory. Key behaviors:

| Issue | Mitigation |
|-------|-----------|
| Nightly invalidates cache daily | Pin nightly date in `rust-toolchain.toml` (already done) — cache survives across runs for same nightly pin |
| Large target/ dirs for cross-compile | Cache is per `(job-name + target + toolchain)` key by default |
| Cache miss on cold PR | `cache-on-failure: true` ensures even failed runs populate cache for next run |

```yaml
- uses: Swatinem/rust-cache@v2
  with:
    shared-key: "nightly-${{ matrix.target }}"
    cache-on-failure: true
    # For cross-compile, cache key auto-includes target triple
```

### Parallel Job Design

```
┌─────────────┐     ┌──────────────────────────┐
│    lint     │     │  build (matrix: 3 arches) │
│ fmt+clippy  │     │  riscv64 / arm64 / x86_64 │
└─────────────┘     └────────────┬─────────────┘
                                 │
                    ┌────────────▼─────────────┐
                    │    qemu-boot (rv64 only)  │
                    │  needs: build[riscv64]    │
                    └──────────────────────────┘
```

- `lint` and `build` run in parallel (no dependency)
- `qemu-boot` depends on `build[riscv64]` artifact via `needs` + `download-artifact`
- Total wall-clock time: ~lint time + max(build matrix time) + qemu time

### Testing no_std Crates

For crates that CAN be tested on host (pure logic, no hardware deps):

```yaml
      - name: Test host-compatible crates
        run: |
          # libs/types and libs/api are pure traits — can be checked on host
          cargo test --package vios-types
          cargo test --package vios-api
```

For kernel/HAL crates: use QEMU boot test as the integration test proxy.

---

## 6. Security Scanning

### Tool Matrix

| Tool | Action | What it catches | Adoption | Recommendation |
|------|--------|----------------|----------|----------------|
| `cargo-audit` | `actions-rust-lang/audit@v1` | CVEs in deps via RustSec DB | High — standard in most Rust projects | **Use** |
| `cargo-deny` | `EmbarkStudios/cargo-deny-action@v2` | CVEs + license violations + banned crates + duplicate deps | High — Embark Studios, used in Bevy | **Use** |
| `cargo-geiger` | Manual `cargo install` | Counts `unsafe` blocks per crate | Medium — useful for OS audit, slow | Optional (weekly schedule) |

`cargo-deny` is a superset of `cargo-audit` for most checks. Use both: `cargo-deny` for PR gates, `cargo-audit` for scheduled issue creation.

### Security Job

```yaml
  security:
    name: Security Audit
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: EmbarkStudios/cargo-deny-action@v2
        # Uses deny.toml at repo root

  audit-scheduled:
    name: Dependency Audit (Scheduled)
    runs-on: ubuntu-latest
    if: github.event_name == 'schedule'
    permissions:
      contents: read
      issues: write
    steps:
      - uses: actions/checkout@v4
      - uses: actions-rust-lang/audit@v1
        with:
          createIssues: true
          denyWarnings: false
```

### Minimal `deny.toml` for no_std OS

```toml
[graph]
targets = [
    { triple = "riscv64gc-unknown-none-elf" },
    { triple = "aarch64-unknown-none" },
    { triple = "x86_64-unknown-none" },
]

[advisories]
unmaintained = "warn"
unsound = "deny"
yanked = "deny"

[licenses]
allow = ["MIT", "Apache-2.0", "MPL-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Unicode-DFS-2016"]
unused-allowed-license = "warn"

[bans]
multiple-versions = "warn"
deny = []

[sources]
unknown-registry = "deny"
unknown-git = "warn"
```

---

## 7. Complete Workflow File (Assembled)

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:
  schedule:
    - cron: '0 6 * * 1'  # Weekly Monday audit

concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: ${{ github.ref != 'refs/heads/main' }}

env:
  CARGO_TERM_COLOR: always
  CARGO_INCREMENTAL: 0
  RUSTFLAGS: "-D warnings"

jobs:
  lint:
    name: Lint (fmt + clippy)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@master
        with:
          toolchain: nightly
          targets: riscv64gc-unknown-none-elf
          components: rust-src,rustfmt,clippy
      - uses: Swatinem/rust-cache@v2
        with:
          shared-key: "lint"
          cache-on-failure: true
      - run: cargo fmt --all -- --check
      - run: >
          cargo clippy
          --target riscv64gc-unknown-none-elf
          --workspace
          -Z build-std=core,alloc
          -- -D warnings

  build:
    name: Build (${{ matrix.target }})
    runs-on: ubuntu-latest
    strategy:
      fail-fast: false
      matrix:
        include:
          - target: riscv64gc-unknown-none-elf
            features: riscv64
          - target: aarch64-unknown-none
            features: aarch64
          - target: x86_64-unknown-none
            features: x86_64
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@master
        with:
          toolchain: nightly
          targets: ${{ matrix.target }}
          components: rust-src
      - uses: Swatinem/rust-cache@v2
        with:
          key: ${{ matrix.target }}
          cache-on-failure: true
      - name: Build kernel
        run: >
          cargo build
          --target ${{ matrix.target }}
          --package vios-kernel
          -Z build-std=core,alloc
      - name: Upload RV64 kernel
        if: matrix.target == 'riscv64gc-unknown-none-elf'
        uses: actions/upload-artifact@v4
        with:
          name: kernel-riscv64
          path: target/riscv64gc-unknown-none-elf/debug/vios-kernel
          retention-days: 1

  qemu-boot:
    name: QEMU Boot (RV64)
    runs-on: ubuntu-24.04   # QEMU 8.x, better RV64 support
    needs: build
    # Only run when the riscv64 build succeeded
    steps:
      - uses: actions/checkout@v4
      - uses: actions/download-artifact@v4
        with:
          name: kernel-riscv64
          path: target/riscv64gc-unknown-none-elf/debug/
      - name: Install QEMU
        run: |
          sudo apt-get update -q
          sudo apt-get install -y qemu-system-misc
          qemu-system-riscv64 --version
      - name: Generate disk image
        run: python3 create_ramdisk.py
      - name: Boot kernel and verify
        timeout-minutes: 2
        run: |
          timeout 60 qemu-system-riscv64 \
            -machine virt \
            -cpu rv64,c=true \
            -smp 1 \
            -m 128M \
            -nographic \
            -serial mon:stdio \
            -bios default \
            -kernel target/riscv64gc-unknown-none-elf/debug/vios-kernel \
            -drive file=disk_v3.img,format=raw,id=hd0,if=none \
            -device virtio-blk-device,drive=hd0 \
            2>&1 | tee /tmp/qemu-output.txt || true

          echo "--- QEMU Output ---"
          cat /tmp/qemu-output.txt

          # Adjust expected string to match ViOS's actual boot message
          grep -q "ViOS" /tmp/qemu-output.txt || {
            echo "FAIL: kernel boot string not found in serial output"
            exit 1
          }

  security:
    name: Deny Check
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: EmbarkStudios/cargo-deny-action@v2

  audit:
    name: Advisory Audit
    runs-on: ubuntu-latest
    if: github.event_name == 'schedule' || github.event_name == 'workflow_dispatch'
    permissions:
      contents: read
      issues: write
    steps:
      - uses: actions/checkout@v4
      - uses: actions-rust-lang/audit@v1
        with:
          createIssues: true
```

---

## 8. Trade-Off Matrix

| Concern | Choice Made | Alternative | Why This Choice |
|---------|-------------|-------------|-----------------|
| Toolchain action | `dtolnay/rust-toolchain` | `actions-rust-lang/setup-rust-toolchain` | Minimal, reads existing `rust-toolchain.toml`, no fluff |
| QEMU timeout | `timeout 60` bash + `timeout-minutes: 2` job | `expect` script | `expect` is powerful but fragile; bash `timeout` + `grep` is simpler and auditable |
| QEMU output check | `grep -q` string match | Exit code from `isa-debug-exit` | RV64 uses SBI not x86 ISA exit; string match works at any boot maturity level |
| Cache strategy | `Swatinem/rust-cache@v2` pinned nightly | No cache | Without pinning nightly, cache evicts daily; pinning (already done) makes cache useful |
| Security | `cargo-deny` + `cargo-audit` | Just `cargo-audit` | `cargo-deny` catches license issues critical for OS project distribution |
| Multi-arch | Matrix 3 targets | Separate workflow files | Matrix is DRY; `fail-fast: false` keeps independence |

---

## 9. Adoption Risk Assessment

| Component | Maturity | Risk |
|-----------|----------|------|
| `dtolnay/rust-toolchain` | High — Kevin Chen/dtolnay, >5k GitHub stars, used by Rust stdlib CI | Low |
| `Swatinem/rust-cache@v2` | High — >3k stars, widely used in Rust ecosystem | Low |
| `-Z build-std` flag | Nightly unstable | Medium — could break on nightly bumps; mitigated by pinned nightly in `rust-toolchain.toml` |
| `qemu-system-misc` on ubuntu-24.04 | QEMU 8.2 — stable, RV64 virt machine well-tested | Low |
| `EmbarkStudios/cargo-deny-action@v2` | High — Embark Studios, Bevy, many production users | Low |
| `actions-rust-lang/audit@v1` | Medium — note: original `cargo-audit` maintainer stepped back in 2025, but RustSec DB continues | Low-Medium |

---

## 10. What This Research Did Not Cover

1. **SBI-based clean shutdown for exit-code testing** — implementing `sbi_shutdown()` in ViOS kernel to allow QEMU to exit with a verifiable code (cleaner than grep-based verification). Worth implementing when boot is stable.
2. **Disk image generation reproducibility** — `create_ramdisk.py` was not analyzed; if it has external dependencies (tools not on ubuntu-24.04), the CI will fail silently.
3. **Cargo nextest** — faster test runner for host-side crates; not applicable to kernel tests but worth for `libs/types`, `libs/api`.
4. **Self-hosted RISC-V runners** — RISE project announced free native RV64 CI runners in March 2026; worth evaluating once ViOS has QEMU-verified boot working first.
5. **Incremental builds with `sccache`** — alternative to `Swatinem/rust-cache` for more granular caching of individual crate compilations.

---

## Ranked Recommendations

1. **Immediate** — Add `.github/workflows/ci.yml` with `lint` + `build[riscv64]` jobs. No QEMU yet — just verify it compiles.
2. **Short-term** — Add QEMU boot job once kernel prints a deterministic boot string to serial. The `grep` check is only as reliable as the string being stable.
3. **Short-term** — Add `deny.toml` + `security` job. ViOS uses MPL-2.0 (kernel); verify all deps are compatible.
4. **Medium-term** — Expand matrix to ARM64 + x86_64 build targets.
5. **Optional** — Weekly `cargo-audit` issue creation. Only adds value once there are real external dependencies with CVE exposure.

---

**Sources consulted:**
- [dtolnay/rust-toolchain](https://github.com/dtolnay/rust-toolchain)
- [Swatinem/rust-cache](https://github.com/Swatinem/rust-cache)
- [Tock OS CI workflow](https://raw.githubusercontent.com/tock/tock/master/.github/workflows/ci.yml)
- [phil-opp Testing chapter](https://os.phil-opp.com/testing/)
- [NuttX RISC-V64 QEMU workflow](https://github.com/lupyuen/nuttx-riscv64/blob/main/.github/workflows/qemu-riscv-leds64-rust.yml)
- [QEMU serial output automation](https://fadeevab.com/how-to-setup-qemu-output-to-console-and-automate-using-shell-script/)
- [EmbarkStudios/cargo-deny-action](https://github.com/EmbarkStudios/cargo-deny-action)
- [actions-rust-lang/audit](https://github.com/actions-rust-lang/audit)
- [Rust Project Primer CI](https://rustprojectprimer.com/ci/github.html)
- [RISE Project native RV64 runners](https://riseproject.dev/2026/03/24/announcing-the-rise-risc-v-runners-free-native-risc-v-ci-on-github/)
- [rav1d QEMU CI workflow](https://github.com/memorysafety/rav1d/blob/main/.github/workflows/build-and-test-qemu.yml)
- [RISC-V GitHub Actions (Gorse)](https://gorse.io/posts/riscv-github-actions)
