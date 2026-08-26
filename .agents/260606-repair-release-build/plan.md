---
title: Repair Release Build + Verification Gate
slug: repair-release-build
created: 2026-06-06
status: planned
owner: ViOS Team
priority: P0 (blocks all other work — kernel does not release-build at HEAD)
report: .agents/reports/debugger-260605-2239-release-build-broken-at-head.md
---

# Repair Release Build + Verification Gate

The ViCell kernel **does not release-build at HEAD**. `cargo check` passes (it skips inline-asm
codegen + linking) and `run.ps1` only rebuilds when the binary is absent, so the breakage was
invisible. Three independent pre-existing blockers; all diagnosed. This task repairs them and
closes the verification gap so it cannot recur.

> Independent of the Reliability track. The reliability Phase 00 source is complete and
> `cargo check`-clean but **cannot be boot-verified until this lands**.

## Blockers (all root-caused — see report)

| # | Blocker | Fix | Risk |
|---|---------|-----|------|
| 1 | `csrsi sie, 0x20` invalid immediate (5-bit imm max 31; STIE=mask 0x20) — Phase 25 | ✅ **Already fixed** in working tree: register-form `csrs sie, {reg}` ([task.rs](../../kernel/src/task.rs)) | none |
| 2 | `-pie` (build.rs, for KASLR) without rustc `relocation-model=pic` → `R_RISCV_64 .L0` link errors | Phase 01 — add `relocation-model=pic` to `.cargo/config.toml` | low (confirmed working) |
| 3 | `vi_set/clear_fast_ipc_vfs_cell` undefined — kernel `extern`s them; defined in `libs/ostd` (not a kernel dep) | Phase 02 — scout Phase 27 fast_ipc refactor, then define kernel-side | med (design) |
| — | Broken release build shipped silently (only `cargo check` ran) | Phase 03 — CI/local gate runs real `cargo build --release` + boot smoke | low |

## Status: build + boot RESOLVED (2026-06-06)
Kernel builds (`RUSTFLAGS=pic`, scoped) AND boots end-to-end to `ViCell >`. Commits:
c66ce992, d444c448, 0635b762, e10f9ab9 (cell_quota deadlock), 610880e0 (boot gate).
5 bugs fixed total (csrsi, PIC-link, kernel fast_ipc symbols, PIE self-reloc, cell_quota deadlock).

## Phases

| # | Phase | Status | Notes |
|---|-------|--------|-------|
| 01 | [Enable PIC relocation model](phase-01-pic-relocation-model.md) | ✅ done | Scoped to kernel via RUSTFLAGS (NOT .cargo/config — that broke cells) |
| 02 | [fast-IPC symbol linkage](phase-02-fast-ipc-symbol-linkage.md) | 🟡 partial | Steps 1-2 done (kernel-owned fast_ipc, builds). Steps 3-6 (loader dynsym, approach A) PENDING — boot works without it; fast-IPC is ecall-fallback. |
| 03 | [Real-build verification gate](phase-03-build-verification-gate.md) | ✅ done | qemu-boot-test.sh fixed + verified locally (PASS@ViCell>); ci.yml binary-name + rv64 pic fixed (unverified in Actions) |

> Beyond the original 3 blockers: also fixed PIE self-relocation (boot.rs), PIC-scoping
> (cells must be non-PIC), and a latent `cell_quota::register` deadlock. Boot is functionally
> verified end-to-end. Only Phase 02 steps 3-6 (cross-cell fast-IPC via loader dynsym) remain.

## Critical path
`01 → 02 → 03`. After 02 the kernel links; after a green `cargo build --release` + QEMU boot,
the Reliability track's Phase 00 can finally be boot-verified.

## Success criteria
- `cargo build --release -p vicell-kernel` completes with **0 errors** (warnings ok).
- Kernel boots in QEMU (`run.ps1`) to the `ViCell>` shell prompt.
- A local/CI gate runs the real release build (not just `cargo check`) and fails loudly on
  asm/link errors — so this class of breakage is caught immediately.

## Out of scope
- Reliability track phases (separate plan `.agents/260605-2107-full-reliability-track/`).
- ARM/x86 build paths (riscv64 is the primary target; touch only if trivially affected).
- KASLR slide logic itself (only ensuring PIE links; slide=0 direct boot must keep working).
