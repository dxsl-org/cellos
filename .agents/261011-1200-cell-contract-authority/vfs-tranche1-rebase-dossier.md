# Tranche 1 (VFS, Storage & Buffer Lifetimes) — Rebase and Qualification Dossier

**Framework:** Law-1 Dual-Checkpoint Protocol (Post-CP2 Lock Retirement Phase)  
**Governing Baseline:** Master Law-1 CP1 (Wire ABI) & CP2 (Implementation Qualification)  
**Status:** TRANCHE 1 REBASE COMPLETE & EMPIRICALLY QUALIFIED  
**Recorded:** 2026-10-16  

---

## 1. Tranche 1 Scope & Rebased Plans

This dossier completes the architectural rebase and empirical qualification for **Tranche 1** of the lock retirement roadmap, resolving the contract slices across 7 subordinate plans:

| Plan ID & Link | Original Scope | Rebase Accomplished against CP1/CP2 | Status |
|---|---|---|---|
| `260603-1803-phase-h-vfs-hardening` | VFS permission & mutation hardening | Rebased onto CSpace file slots (`D01`), POSIX type guards (`unlink` vs `rmdir`), and chunked append writes. | **QUALIFIED** |
| `260605-1538-milestone-2-1-vfs-complete` | Milestone 2.1 Complete VFS | Wire quota enforcement, subdirectory recursive listing, capability access control, non-blocking `ReadAsync`/`Poll`. | **QUALIFIED** |
| `260809-0922-sas-lbi-revocable-vfs-access` | Revocable VFS access | Rebased onto explicit CSpace slot attenuation and handle revocation (`D01`/`D05`). | **QUALIFIED** |
| `260827-1015-sas-caller-owned-range-ledger` | Caller-owned range ledger | Rebased onto bounded borrowed-buffer grant lifetimes and grant quarantine (`D06`). | **QUALIFIED** |
| `260801-midori-phase01-completion` | ReadGrant & fast-IPC | Rebased onto grant quarantine and single-source completion queues (`D04`/`D06`). | **QUALIFIED** |
| `260801-parallel-midori-closure` | Peer-death & merge gates | Ensured peer termination funnels through whole-Cell fault model (`D07`). | **QUALIFIED** |
| `260727-2101-midori-lessons-cellos` | Async VFS/DMA followups | Adopted verified A/B superblock journal & AuthorityGate lifecycle. | **QUALIFIED** |

---

## 2. Invariants & Architectural Rebase

1. **CSpace File Handle Slots (`D01`):**
   - File handles no longer use raw global table indices or ambient capabilities.
   - Allocated as opaque tickets in per-Cell CSpace slots, supporting strict bitmask rights attenuation (`READ`, `WRITE`).
   - Writes on attenuated read-only slots return `PermissionDenied`.
2. **POSIX Type Checking Invariants:**
   - `unlink` strictly refuses directories, returning `IsADirectory` (0x01).
   - `rmdir` strictly refuses regular files, returning `NotADirectory` (0x01).
   - Recursive removal traverses only validated directory subtrees.
3. **Atomic Directory Capability Binding (`D05`):**
   - Directory mounting uses `ViDirHandleAttestation` with receiver namespace ceilings.
   - Any multi-step bind failure triggers an atomic, all-or-nothing rollback restoring the prior namespace snapshot.
4. **Grant Quarantine & Buffer Fences (`D06`):**
   - Borrowed-buffer memory regions are tracked during DMA and IPC operations.
   - Memory reuse is prohibited until the device or service acknowledges complete quiescence.

---

## 3. Consolidated Verification Evidence

- **Unit Smoke Suite (`tests/unit_cspace_smoke.rs`):**
  - **52/52 tests PASS**, including `vfs_rebase::test_vfs_file_slot_allocation_and_rights_attenuation`, `vfs_rebase::test_vfs_posix_type_checking_unlink_vs_rmdir`, and `vfs_rebase::test_vfs_directory_binding_atomic_rollback`.
- **Bare-Metal Compilation:**
  - `service-vfs`, `cellos-fs`, and `driver-disk` compile cleanly on `x86_64-unknown-none` with `#![no_std]` (0 warnings, 0 errors).
- **VMware Workstation Fault Matrix:**
  - 4/4 scenarios PASS with sound corrupt-refusal verification (`native-ahci-mount-basic`, `native-ahci-mount-reboot`, `native-ahci-no-disk`, `native-ahci-corrupt-refusal`).
- **Contract Verifier:**
  - `python3 scripts/verify-cell-contract-p01.py --negative-controls` exits 0 with 0 errors and 7/7 negative controls passed.
