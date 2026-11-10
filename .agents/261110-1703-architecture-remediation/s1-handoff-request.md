# S1 handoff request — build/CI (phases 02, 03)

**Date:** 2026-11-10 · **Plan:** [plan.md](plan.md) · **Admission:** [admission-approval.md](admission-approval.md) · **Lane:** Intel WIP lane recorded free 2026-11-10 (S4 close)

## 1. Scope

- Phase 02 — x86 C2C gate, lane manifest, reproducible CI.
- Phase 03 — explicit kernel embed inputs, manifest verification, single image recipe per arch, generated disk layout, release builds the x86 ISO.

No `libs/api`/`libs/types` change → no Law-1 delta. No kernel source change except `kernel/build.rs` and a `compile-only` feature in `kernel/Cargo.toml`. Evidence ceiling: host + x86 QEMU TCG (KVM where available).

## 2. File set (file-bounded exception)

Phase 02
- New: `scripts/lanes.toml`, `scripts/check-lanes.py`, `scripts/host-test-crates.txt`, `scripts/check-host-test-crates.py`, `.github/actions/setup/action.yml`, `requirements-ci.txt`
- Modify: `.github/workflows/{ci,perf,release,docs,security,dco}.yml` (pins, `--locked`, concurrency, timeouts, the new `x86-c2c` job, x86 clippy), `docs/specs/10-testing.md` §5

Phase 03
- Modify: `kernel/build.rs`, `kernel/Cargo.toml` (`compile-only` feature only), `gen_disk.ps1`, `gen_disk_rpi3.ps1`, `scripts/gen-disk-ci.sh`, `scripts/gen-disk-aarch64-ci.sh`, `scripts/format-disk-arm.sh`, `scripts/mksrv-img.sh`, `tools/write-mbr.py`, `tools/write-rpi3-mbr.py`, `tools/cellos-fs-provision/provision.sh`, `.gitignore`, every script that sets or reads `EMBEDDED_OVERRIDE` (rename to `CELLOS_EMBED_DIR`; list fixed in the phase evidence before editing)
- New: `scripts/assemble-image.py`, `scripts/images/{riscv64,aarch64}.toml`, `scripts/check-disk-layout-literals.py`, a host test emitting `disk-layout.json` from `api::disk` placed outside `libs/api` (reads, does not modify it)
- Untrack (git rm --cached, files stay on disk): `kernel/src/embedded*/{init,kernel_fs.img}`, `gh_2.55.0_linux_amd64/`, tracked files under `build/`, `limine/` objects and `.deb`

Both: this plan directory, `.agents/plan-portfolio.md`, `.agents/cell-contract-0024-locks.md`, `docs/project-changelog.md`.

## 3. Irreversible or externally visible effects

- Untracking binaries does not shrink git history; no history rewrite is proposed.
- Release workflow change affects only future tags; published releases are not touched.
- SHA-pinning actions changes nothing at runtime but must be re-pinned on upgrades.

## 4. Conflicts

- iGPU DMA-export slice (approved, not effective) touches `kernel/src/task/{syscall,task}.rs`, `iommu*`, `libs/api/src/abi/syscall.rs` — no overlap.
- x86 Tier-2 GPU runtime R1–R4/R6 — user-space files only, no overlap.
- x86 breadth S5 NIC drivers (no handoff record) — no overlap; lane rows in `lanes.toml` will list their lanes as `manual`.

## 5. Order and stop points

Phase 02 first (regression net), checkpoint at end of Phase 03. Untracking happens only after one full CI cycle runs on the explicit embed path.
