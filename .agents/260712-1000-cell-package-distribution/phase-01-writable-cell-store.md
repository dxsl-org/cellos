# Phase 01 — Gated Writable Cell-Store (VFS unlock)

## Context Links
- Plan: [plan.md](plan.md)
- Kernel Boundary Law: `docs/specs/15-kernel-boundary.md` (§2C, §3.1 — VFS is a Cell, drivers exiled)
- IPC wire contract: `docs/specs/17-ipc-wire-contract.md`
- Prior: `.agents/260707-1726-g2-loader-redesign/` (created the `/bin` overlay + cell-store)

## Overview
- **Priority:** P1 (blocks all install) · **Status:** pending
- Unlock a **gated, capability-scoped write** into the FAT cell-store that surfaces read-only at `/bin`,
  so an installer can persist a signed ELF that later spawns. This is the single missing primitive; it
  is a **VFS-cell (userspace) policy change**, not a kernel change.

> **D36 precedence ruling (2026-08-01): BLOCKED pending redesign.** Midori directory
> capabilities and sealed paths take precedence. This phase must not set
> `allow_write_all`, authorize by process name, or expose ambient path writes to `/bin`.
> Re-plan around a dedicated capability-scoped installer with staging + verified commit;
> spawn signature/policy admission remains mandatory. No existing ABI is approved here.

## Key Insights (verified)
- `/bin` read-only is enforced in **three** userspace places, all in the VFS cell:
  `backend_bin_overlay.rs:63-68` (writes → `false`), `access.rs:33` (`/bin/` `allow_write_all:false`),
  `manager.rs:54` (`mount("/bin", binov, false)`).
- The underlying `FatBackend` **already writes** (`backend_fat.rs:232-306`); `/mnt/sd` uses the same code
  writable (`manager.rs:34-38`). So unlocking = delegate `BinOverlay` writes to its single cell-store
  `FatBackend`, gated — **no new backend code, single FAT cache (no coherence bug)**.
- The cell-store is P6 (`disk.rs:48-54`), **separate** from VIFS1 (kernel ramdisk) and from the P2
  bootstrap table. VIFS1 shadows `/bin` on reads (`backend_bin_overlay` VIFS1-first), so a cell-store
  entry that collides with a bootstrap name is unreadable/unspawnable → must be refused at install.
- **RENAME does not exist** (`libs/api/src/services/ipc.rs` has no `Rename`). Do not design around it.

## Requirements
**Functional**
- A write to `/bin/<name>` via the VFS cell succeeds **only when** the caller is authorized (v1: install
  path) **and** `<name>` is not in the protected-core denylist.
- Delete (`Unlink`) of `/bin/<name>` is likewise gated (needed by `pkg remove`/`upgrade`).
- Reads/spawns via `/bin` are **unchanged** (still read-only for every non-install caller).
- Large ELFs (KB–MB) are writable within IPC limits (grant-based or chunked).

**Non-functional**
- Fail-closed: any gate ambiguity → refuse. `#![forbid(unsafe_code)]`, no `mod.rs`, Vi prefix preserved.
- Zero kernel edits; zero `libs/api` edits (reuse existing `Write`/`WriteGrant`/`Unlink` VfsRequest ops).

## Architecture
```
pkg (shell)  ──VfsRequest::Write{/bin/<name>, chunk}──▶  vfs cell
                                                          dispatch.rs
                                                          ├─ access check (access.rs):
                                                          │   /bin write allowed IFF gate passes
                                                          ├─ gate: caller authorized (P01 §gate)
                                                          │        ∧ name ∉ PROTECTED_CORE
                                                          └─ BinOverlay.write ──▶ FatBackend.write (P6)
```
**Withdrawn gate draft (historical; must not be implemented):**
1. **Protected-core denylist** (const in VFS): `vfs shell net init supervisor config block platform`
   — refuse write/unlink targeting any of these names (protects boot integrity + trusted-core recovery
   hatch `policy.rs:270-272`).
2. **Caller authorization**: v1 identified the installer by cell **name** via `sys_get_procs`
   (`ProcessInfo.name`, stable across restart, not forgeable by a Rust cell under G1) — allow only the
   `pkg`/`shell` caller. Alternatively it proposed feature-gating the writable path.
   D36 rejects both shapes as authorization; this block is retained only for provenance.
3. **Size/quota**: reject if the write would exceed the 32 MB cell-store window (existing VFS quota +
   an explicit remaining-space check).

## Data Flow (write of one ELF)
enter: `Write{path="/bin/foo", content=chunk}` (or grant) from installer →
transform: access rule consults gate → BinOverlay delegates to cell-store FatBackend → FAT32
create/overwrite `foo` → exit: `VfsResponse::Ok`. Read path (`GetFile`/`ReadFileGrant`/spawn) unchanged.

## Related Code Files
**Modify** (all in `cells/services/vfs/src/`)
- `backend_bin_overlay.rs` — `write`/`unlink` delegate to `store` (cell-store FatBackend) instead of
  returning `false`, guarded by the gate + denylist.
- `access.rs` — `/bin/` rule becomes conditionally writable (gate-driven, not blanket `true`).
- `dispatch.rs:48-50` — extend the write access check to consult the gate for `/bin`.
- `manager.rs:54` — reassess the `writable` flag for the `/bin` mount (or keep false and route install
  through an explicit gated op — decide with §gate option chosen).
**Read/verify only:** `backend_fat.rs` (write ops), `block_stream.rs` (cell-store LBA), `quota.rs`.
**Create:** none expected (a small `install_gate.rs` helper if the gate grows past ~30 LOC).

## Implementation Steps
1. Add `PROTECTED_CORE: &[&str]` const + `is_protected(name)` helper in the VFS cell.
2. Add `installer_authorized(sender_tid) -> bool` (name lookup via `sys_get_procs`) OR wire the
   `cellstore-write` feature gate — per review decision.
3. In `BinOverlay::write`/`unlink`, when gate passes and name not protected, delegate to `self.store`.
4. Update `access.rs`/`dispatch.rs` so `/bin` writes reach that delegation only through the gate.
5. Confirm the write mechanism for large ELFs: measure `hello` size; if chunked `Write` is too slow/big,
   add a path-based grant-write helper (no ABI change — reuse grant plumbing from `ReadFileGrant`).
6. Add `#[cfg(feature="test-hooks")]` self-test: gated write of a fixture ELF → read back byte-equal →
   spawn.

## Todo List
- [ ] Protected-core denylist + `is_protected`
- [ ] Caller-authorization gate (name-based) or feature gate — decide + implement
- [ ] `BinOverlay::write`/`unlink` gated delegation to cell-store
- [ ] `access.rs`/`dispatch.rs` write-check extension for `/bin`
- [ ] Large-ELF write mechanism verified (chunked vs grant)
- [ ] Boot self-test: write→read-back→spawn

## Success Criteria
- **Oracle (boot, QEMU riscv64):** from the shell, a gated write of a signed fixture ELF to `/bin/probe`
  succeeds; `ls /bin` shows it; `exec /bin/probe` runs it.
- A write to `/bin/vfs` is **refused** (protected-core).
- A write from an unauthorized caller (or on a hardened/feature-off build) is **refused**.
- `GetFile`/spawn of an existing `/bin` cell is byte-identical to pre-change (no read regression).
- `grep -r kernel/src` shows **no** kernel change.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|-----------|
| Unlocking `/bin` lets a cell overwrite trusted-core → boot compromise | Low×High | Protected-core denylist enforced before any delegation; VIFS1 shadows bootstrap names anyway |
| Cache incoherence (RO read view vs write view) | Low×High | **Single** `FatBackend` inside `BinOverlay` — one cache; no double-mount |
| Torn write corrupts cell-store | Med×Med | Spawn-time sig verify fail-closed; `.prev` (P03); non-destructive write ordering |
| Gate too permissive (identity spoof) | Med×Med | G1 first-party model documented; name-based check + optional feature gate; kernel-cap gate = G2 follow-up |

## Security Considerations
- The read-only `/bin` invariant is a **security boundary**; this phase narrows, not removes, it. Every
  relaxation is gated + denylisted + fail-closed. The kernel spawn-gate remains the true enforcement:
  even a successful hostile write cannot produce a spawnable cell without a valid fleet signature.
- Document the G1 first-party assumption (spec 15 §1.4) wherever the caller-identity gate is claimed.

## Rollback
Revert the VFS-cell diff (writes return `false` again, `/bin` rule back to read-only). No data migration;
any files written to the cell-store during testing are inert (won't spawn without a valid sig) and can be
left or wiped by rebuilding the disk image.

## Next Steps
Unblocks P02 (`pkg` built-in consumes this write path). Feeds the P03 upgrade/rollback file ops
(`Unlink` + overwrite).
