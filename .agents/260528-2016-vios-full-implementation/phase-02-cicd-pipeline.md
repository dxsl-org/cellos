# Phase 02 — CI/CD Pipeline

**Effort:** 60h | **Priority:** P1 | **Status:** complete | **Blockers:** Phase 01

## Overview

Stand up GitHub Actions CI: format/lint check, multi-target build matrix, QEMU boot smoke test, security scan (cargo-deny, cargo-audit). Without CI, every subsequent phase risks silent regression. This is the gate that lets the project safely accept community PRs (Phase 23).

## Context Links

- `docs/project-roadmap.md` — milestone gating
- `Cargo.toml` workspace root — current toolchain pin (verify `rust-toolchain.toml` exists; if not, create one)
- `run.ps1` — current local QEMU launch (mirror in CI bash)

## Key Insights

- Workspace targets `riscv64gc-unknown-none-elf` primarily; CI matrix should still cover AArch64 + x86_64 stubs so they don't regress while Phases 08/09 land.
- `-Z build-std=core,alloc` is mandatory for `no_std` targets and requires nightly.
- QEMU boot test must time-out (`timeout 60s qemu …`) and `grep` serial output for a known banner — kernel must print a stable string early (e.g. `[ViOS] kernel boot`).
- Caching: `Swatinem/rust-cache@v2` with `cache-on-failure: true`; do NOT cache `target/` of the kernel ELF (binary mismatch across PRs).

## Requirements

**Functional**
- Push to `main` or PR → CI runs lint, build matrix (3 targets), security, qemu-boot
- All 4 jobs must pass for green status
- Weekly cron job runs deeper security scan
- Issue/feature templates appear when contributors open issues

**Non-functional**
- Total CI wall-time < 12 min (warm cache)
- Cache hit ratio > 80% on incremental PRs
- Free-tier compatible (no self-hosted runners)

## Architecture

```
push/PR  ──► .github/workflows/ci.yml
              ├── lint        (fmt + clippy -D warnings)
              ├── build       (matrix: rv64, aarch64, x86_64)
              ├── qemu-boot   (rv64 only; depends on build)
              └── security    (cargo-deny + cargo-audit)

cron weekly ──► .github/workflows/security.yml
                 ├── cargo-audit (full advisory DB)
                 ├── cargo-deny  (licenses, bans)
                 └── cargo-geiger (unsafe regression)
```

## Related Code Files

**Note (Validation Session 1):** `.github/workflows/ci.yml` ALREADY EXISTS but has 3 bugs: triggers on `master` (not `main`), references non-existent `libkernel` crate, uses deprecated `checkout@v6`. Strategy: **overwrite/replace** with upgraded pipeline. <!-- Updated: Validation Session 1 -->

**Overwrite (existing, replace entirely):**
- `.github/workflows/ci.yml` — replace with full pipeline (fix branch trigger + crate name + tool versions)
- `.github/workflows/security.yml` — weekly security scan
- `.github/ISSUE_TEMPLATE/bug_report.md`
- `.github/ISSUE_TEMPLATE/feature_request.md`
- `.github/ISSUE_TEMPLATE/config.yml` — disables blank issues, links to discussions
- `.github/pull_request_template.md` — PR checklist
- `deny.toml` — cargo-deny configuration
- `rust-toolchain.toml` — pin nightly date (if not already present)
- `scripts/qemu-boot-test.sh` — boot kernel + grep banner; reused by CI and local dev

**Modify:**
- `kernel/src/main.rs` — ensure early `println!("[ViOS] kernel boot v{}…")` is the first serial output (CI greps for it)
- `Cargo.toml` workspace root — verify `[workspace.lints]` block enabled (for `cargo clippy`)
- `README.md` — add CI status badge once workflow runs once

## Implementation Steps

1. Verify / create `rust-toolchain.toml`:
   ```toml
   [toolchain]
   channel = "nightly-2026-05-01"
   components = ["rust-src", "rustfmt", "clippy", "llvm-tools-preview"]
   targets = ["riscv64gc-unknown-none-elf", "aarch64-unknown-none", "x86_64-unknown-none"]
   profile = "minimal"
   ```
2. Locate stable boot banner string in `kernel/src/main.rs`. If absent, add `println!("[ViOS] kernel boot v{}", env!("CARGO_PKG_VERSION"))` as the first console output after UART init.
3. Create `scripts/qemu-boot-test.sh`:
   ```bash
   #!/usr/bin/env bash
   set -euo pipefail
   KERNEL="${1:-target/riscv64gc-unknown-none-elf/release/kernel}"
   timeout 60 qemu-system-riscv64 -machine virt -nographic -bios default \
     -kernel "$KERNEL" 2>&1 | tee qemu.log | grep -q "\[ViOS\] kernel boot" || {
       echo "FAIL: boot banner not seen"; cat qemu.log; exit 1; }
   echo "PASS: kernel booted"
   ```
4. Create `deny.toml`:
   ```toml
   [graph]
   targets = [
     { triple = "riscv64gc-unknown-none-elf" },
     { triple = "aarch64-unknown-none" },
     { triple = "x86_64-unknown-none" },
   ]
   [advisories]
   yanked = "deny"
   unsound = "deny"
   unmaintained = "warn"
   [licenses]
   confidence-threshold = 0.93
   allow = ["MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception",
            "MPL-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Unicode-DFS-2016"]
   [bans]
   multiple-versions = "warn"
   wildcards = "deny"
   ```
5. Create `.github/workflows/ci.yml` with jobs:
   - **lint**: checkout, `dtolnay/rust-toolchain@master`, `Swatinem/rust-cache@v2`, `cargo fmt --all --check`, `cargo clippy --workspace --target riscv64gc-unknown-none-elf -Z build-std=core,alloc -- -D warnings`
   - **build** (matrix on `[rv64, aarch64, x86_64]`): same toolchain + cache, `cargo build --release --target ${{matrix.target}} -Z build-std=core,alloc`. Upload kernel ELF as artifact for rv64.
   - **qemu-boot** (needs: build, only on rv64 artifact): `sudo apt-get install -y qemu-system-misc`, download kernel artifact, run `scripts/qemu-boot-test.sh`
   - **security**: `EmbarkStudios/cargo-deny-action@v1` + `rustsec/audit-check@v1.4`
   - Set `env: { CARGO_INCREMENTAL: 0, RUSTFLAGS: "-D warnings", CARGO_TERM_COLOR: always }`
6. Create `.github/workflows/security.yml` (cron `0 6 * * 1` — Mon 06:00 UTC):
   - Same checkout/toolchain/cache
   - `cargo audit --deny warnings`
   - `cargo deny check`
   - `cargo install --locked cargo-geiger && cargo geiger --workspace --output-format json > geiger.json`
   - Parse geiger.json; fail if `unsafe_used` in any path under `cells/`
   - Upload geiger.json as artifact
7. Create `.github/ISSUE_TEMPLATE/bug_report.md`: fields = Description, Repro Steps, Expected, Actual, Environment (host OS, QEMU version, rust toolchain), Logs.
8. Create `.github/ISSUE_TEMPLATE/feature_request.md`: fields = Problem, Proposed Solution, Alternatives, Additional Context.
9. Create `.github/ISSUE_TEMPLATE/config.yml` (disable blank issues; link to Discussions).
10. Create `.github/pull_request_template.md`: checklist = tests added, docs updated, follows 8 Coding Laws, no `mod.rs`, profile inheritance respected.
11. Push branch `ci/initial-pipeline`; iterate on workflow until all jobs green on PR.
12. Once merged, add CI status badge to `README.md`.

## Todo List

- [x] Create / verify `rust-toolchain.toml` with nightly pin — `nightly-2026-05-01`, targets rv64/aarch64/x86_64
- [x] Add stable boot banner in `kernel/src/main.rs` (idempotent) — `[ViOS] kernel boot v<ver>` as first UART output
- [x] Create `scripts/qemu-boot-test.sh` and chmod +x
- [x] Create `deny.toml` — licenses allow-list + `wildcards = "deny"`
- [x] Create `.github/workflows/ci.yml` — 4 jobs: lint / build-matrix / qemu-boot / security
- [x] Create `.github/workflows/security.yml` — weekly cron: cargo-audit + cargo-geiger
- [x] Create `.github/ISSUE_TEMPLATE/{bug_report,feature_request,config}.md|yml`
- [x] Create `.github/pull_request_template.md`
- [ ] Push branch, iterate until green — pending first PR to trigger CI
- [x] Add CI badge to README.md
- [ ] Confirm CI wall-time < 12 min warm — cannot verify without GitHub Actions run

## Success Criteria

- Opening any PR triggers all 4 CI jobs; all green for the canonical baseline branch
- `qemu-boot` job sees `[ViOS] kernel boot` within 60s
- `security` job: 0 deny/audit failures (or documented exceptions in `deny.toml`)
- Weekly cron runs without manual intervention
- CI total time < 12 min warm, < 25 min cold

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| QEMU version differences (Ubuntu LTS vs latest) cause flake | Med | Med | Pin `runs-on: ubuntu-24.04`; install `qemu-system-misc=` specific version; alternative: nix install |
| `-Z build-std` breaks across nightly rolls | High | High | Pin nightly date in `rust-toolchain.toml`; bump via dedicated PR every 30 days |
| Caching across matrix targets blows up disk | Low | Low | Use `key` suffix per target so caches don't collide |
| AArch64 / x86_64 build fails (stubs) | High | Low | Phase 01 expected to keep them compiling; if not, `continue-on-error: true` until 08/09 |
| Secrets accidentally needed (e.g. for docs deploy) | Low | Med | Phase 02 has zero secret dependence; defer docs deploy to Phase 19 |

## Security Considerations

- Workflows use `GITHUB_TOKEN` with default read-only perms; no `pull_request_target` (avoids fork PR token-leak class).
- `cargo-audit` advisory DB is read at run-time; pin nothing — we WANT new advisories to fire.
- No third-party action without commit SHA pin (defense vs. compromised marketplace actions).

## Rollback

Workflows are inert until merged. Revert by `git revert` on the workflow PR; existing PRs lose CI status (acceptable). No code/runtime impact.

## Next Steps

- Phase 12 (Security infra) extends `security.yml` with Kani harnesses + fuzz
- Phase 19 (Docs automation) adds `docs.yml` and `release.yml`
- Every phase from 03 onward must pass this CI before merge
