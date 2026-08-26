---
title: "Manifest v2 — versioned 16-byte fixed struct (tier floor + u16 device flags + cap_args hook)"
description: "One Law-1 bump that unblocks PKU per-tier isolation, CAN/ADC MMIO device flags, and reserves the cap_args hook — spent once, no v3 forced."
status: complete (P00-P02 landed; P03 deferred)
priority: P2
effort: 4 phases (~2.5 days once P00 confirmed)
branch: main
tags: [manifest, abi, law1, pku, capabilities, kernel-boundary]
created: 2026-07-12
---

# Manifest v2

Versioned FIXED 16-byte `#[repr(C)]` manifest replacing the 8-byte v1. NOT a TLV
(rejected: in-kernel hot parser at spawn gate must stay minimal — see dossier).
Spends the Law-1 confirmation ONCE on a struct with headroom for the next decade.

Design authority (locked, do not re-litigate):
`.agents/260712-1836-mythos-g123-analysis/dossier-2-manifest-v2.md`

## The load-bearing invariant — tier is a FLOOR, not a ceiling

`granted_tier = max(manifest.tier, floor)`. Higher tier = MORE isolation = LESS
authority → self-restriction, always allowed. Lower tier = LESS isolation (→ PKU
key 0 full access) → gated: a cell can never promote itself into the trusted
domain. Inverting this = privilege escalation. Every phase touching tier restates it.

## Compatibility contract (both directions fail-closed) — a global success criterion

- v2 kernel reads v1 cell: `from_bytes` upcasts, `tier=TIER_LEGACY` sentinel →
  loader keeps today's `is_trusted` heuristic byte-for-byte. No re-sign for read.
- v1 kernel reads v2 cell: version 2 ≠ 1 → `from_bytes` returns `None` → fail-closed
  (spawn denied under signing-required / legacy path fallback). Safe.
- Section name unchanged: `__ViCell_manifest` (`cell-build/src/cell.ld.in:61`).

## Phases

| # | Phase | Status | Depends |
|---|-------|--------|---------|
| P00 | [Law-1 confirm gate](phase-00-law1-confirm-gate.md) | complete | P-TRUST landed (.agents/260712-1100) |
| P01 | [v2 struct + from_bytes + macros + re-sign](phase-01-v2-struct-and-macros.md) | complete (`c25f3185`) | P00 |
| P02 | [loader: tier→PKU floor gate + CAN/ADC device flags](phase-02-loader-tier-pku-and-device-flags.md) | complete (`c25f3185`) | P01 |
| P03 | [DEFERRED: cap_args section + offset parse](phase-03-deferred-cap-args-hook.md) | deferred | P02 + concrete case |

> **D35 portfolio ruling (2026-08-01):** child of Trust & Identity; do not merge
> physically with revocation or DICE. P03 remains YAGNI-deferred until a concrete
> parameterized-capability consumer exists.

## Dependency / land order

P-TRUST (dossier 1, `.agents/260712-1100`) folds path-caps into the unified CapSet
ceiling FIRST — the tier floor in P02 interacts with that ceiling. Then P00 → P01 →
P02. P03 is reserved-only; do NOT build until a concrete parameterized-cap case
appears (YAGNI — the 4-byte `cap_args_off` slot is already carved out by P01).

## Law-1 gates (2× user confirmation)

Single confirmation event at P00 covers the whole ABI surface:
- `CellManifest` 8→16 bytes, `flags` u8→u16, add `tier`/`cap_args_off`/`reserved`
- `MANIFEST_VERSION` 1→2, `from_bytes` version-branch
- `declare_manifest!` + `app_entry!` gain `tier`/device args
File: `libs/api/src/abi/manifest.rs` (ABI-frozen). `sign-cell.py` needs NO change
(reads section by `sh_size` generically, `sign-cell.py:145-148`) — but any cell
that MIGRATES to a v2 manifest must be re-signed (build discipline).

## Explicit non-goals (locked)

- Do NOT migrate privileged path-caps (pcie_driver/platform/supervisor) into flags
  — they stay path-based (`loader.rs:301-317`). A manifest bit would let any `/bin/`
  cell declare supervisor authority; path-identity is the safer trust model.
- Do NOT build cap_args now (P03 reserved-only).
- Do NOT convert to TLV.
