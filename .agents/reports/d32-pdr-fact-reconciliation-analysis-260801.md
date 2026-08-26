# D32 — Reconcile PDR status claims with current evidence

**Status:** approved/applied 2026-08-01. No code changed.

## Finding

The PDR contradicts itself: ARM/x86 are both planned and complete; VFS is both basic
read-only FAT and a shipped multi-backend service; test coverage is limited, 75%, and
80%+ complete; reproducible builds are checked without a bit-for-bit CI harness. It also
embeds stale LOC and completion percentages.

## Recommended ruling [FINAL]

**Approve recommendation A: make the PDR evidence-based and link moving facts.**

1. Distinguish compile/smoke/implementation/qualification per architecture instead of a
   single "all complete" badge.
2. Describe the MountTable VFS, FAT write support, littlefs `/data`, and staged RedoxFS.
3. Mark coverage as unmeasured until the documented coverage command produces an
   artifact; remove unsupported 75%/80% claims.
4. Mark bit-for-bit reproducibility unverified until a CI harness compares artifacts.
5. Remove hand-maintained LOC and documentation-completion percentages; link generated
   status when available.
