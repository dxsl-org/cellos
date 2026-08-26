---
title: "Cell Package Distribution (pkg installer)"
description: "Install/update/remove signed cell packages into the cell-store at runtime; the signed ELF is the package and the kernel spawn-gate is the enforcement boundary."
status: pending
priority: P2
effort: 4 phases (~9-13 dev-days)
branch: main
tags: [g2, package-manager, cell-store, signing, capability, vfs, sas, lbi, distribution]
created: 2026-07-12
---

# Cell Package Distribution

> Roadmap `docs/project-roadmap.md` §E: "Package manager / app distribution [G2] — no install/update
> mechanism beyond baking into the disk image." This plan closes that gap **without a Linux-style
> package manager**: no install-time scripts/hooks/postinst (arbitrary install-time code execution is
> exactly what the capability model forbids). The package is a **signed ELF**; the kernel spawn-gate
> (Ed25519 sig + capability manifest + policy intersection) is the enforcement boundary. That gate is
> the differentiator vs dpkg/rpm — **the package IS the security boundary**.

## Thesis (verified in code)
"Install" = get a signed, capability-bearing ELF into `/bin` (the FAT cell-store), then it is spawnable.
This is not incidental — it is **forced by the security model**: `kernel/src/loader.rs:156` denies spawn
of any cell whose path is not under `/bin/` and whose manifest declares any privilege
(`AuditEvent::CellSpawnDenied`). A useful (capability-bearing) app therefore *must* live in the
cell-store. Everything install needs already exists except one thing: **`/bin` is read-only at runtime**
(a VFS-cell policy, not a kernel limit).

## Primitives that already exist (cite)
| Primitive | Evidence |
|-----------|----------|
| FAT cell-store partition P6, served at `/bin` | `libs/api/src/abi/disk.rs:48-54`; `gen_disk.ps1:484-506`; base LBA 1,062,144 / 32 MB |
| FatBackend write/append/unlink/mkdir/rmdir (FAT32) | `cells/services/vfs/src/backend_fat.rs:232-306` — **fully works** (used writable for `/mnt/sd`) |
| `/bin` read-only enforcement (to unlock) | `backend_bin_overlay.rs:63-68` (all writes → `false`); `access.rs:33` (`/bin/` write=false); `manager.rs:54` (`mount("/bin", …, false)`) |
| `sys_spawn_from_elf` (238) + `sys_spawn_from_path` (VFS→grant→spawn) | `syscall.rs:1761`; `libs/ostd/src/syscall.rs:263-323`; `fs.rs:267-296` |
| Ed25519 sig gate at spawn (fail-closed) | `signing.rs:45,61`; `loader.rs:114-127` → `AuditEvent::CellSignatureVerified/Failed (21/22)` |
| Capability manifest (8 B, flag→cap names) | `libs/api/src/abi/manifest.rs:23-155`; `cap.rs:163-179`; policy `∩` `loader.rs:262-269` |
| Offline signer | `scripts/sign-cell.py` |
| Writable FAT32 source volume `/mnt/sd` | `manager.rs:34-38` (`writable:true`) |
| HTTP/TLS download transport | `libs/ostd/src/http/client.rs`; `clients/tls_stream.rs`; `cells/demos/http-smoke` |
| Shell built-in dispatch (add `pkg` ≈ 5 LOC) | `cells/tools/shell/src/executor.rs:703-789` |

## Kernel changes: **0.** libs/api changes: **0** (design reuses existing `Write`/`Unlink` VfsRequest ops).
The one *missing* op — file **RENAME** (no `VfsRequest::Rename`, confirmed) — is designed around with
copy+overwrite and a `.prev` copy; **no new syscall or ABI variant required.** All work lands in two
userspace cells (`vfs`, `shell`) plus docs.

## Trust model (decided)
- **Enforcement point = kernel spawn-gate** (sig + manifest + policy). Unchanged. An unsigned/tampered
  ELF that lands in `/bin` simply **cannot spawn** (`CellSignatureFailed`, fail-closed) — the write is
  harmless.
- **Install-time = advisory UX**: reject obviously-broken packages early (ELF magic, `__ViCell_sig` +
  `__ViCell_manifest` present) and **show the requested capabilities** (manifest flags → human names)
  before writing. No redundant crypto enforcement in v1 (userspace Ed25519 verify is an optional
  follow-up).
- **Cell-store write gate (v1, G1 first-party threat model)**: writes to `/bin` are allowed only via the
  new install path, and refused for a **protected-core denylist** (`vfs, shell, net, init, supervisor,
  config, block, platform`) so no install can overwrite a trusted-core / bootstrap-shadowed cell.
  Untrusted third-party install → a hardware/kernel install-capability is a G2 follow-up (see §16/§1.4
  MMIO-gating note).

## Phase overview
| Phase | Title | Tier | Status | Depends | Top risk |
|------|-------|------|--------|---------|----------|
| [01](phase-01-writable-cell-store.md) | Gated writable cell-store (VFS unlock) | thinking | pending | — | unlocks a security-sensitive read-only invariant |
| [02](phase-02-pkg-installer-builtin.md) | `pkg` shell built-in (install/remove/list/info) | medium | pending | 01 | advisory-vs-enforcement confusion; large-ELF IPC |
| [03](phase-03-upgrade-rollback.md) | `pkg upgrade` + `.prev` rollback | medium | pending | 02 | non-atomic (no rename); running-instance semantics |
| [04](phase-04-http-source-multiarch-docs.md) | HTTP source (opt) + x86 regression + docs | medium | pending | 02 | net test-env dependence; x86 NVMe cell-store write |

**Shippable v1 = Phases 01-03** (fully boot-verifiable on QEMU riscv64, no network). Phase 04 adds the
optional HTTP source, the x86/NVMe multi-arch check, and doc reconciliation.

## Design answers (the 7 questions)
1. **Format** — bare **signed ELF**. It already carries manifest+sig+entry; an outer archive would add
   only version/deps/cap-summary, all of which are derivable (caps from manifest) or unused (no deps in
   SAS). YAGNI → no wrapper. Filename = cell name.
2. **Who verifies** — kernel at **spawn** (enforcement, unchanged). Install-time verify is **advisory
   UX** (structural sanity + cap review), not redundant crypto.
3. **Installer home** — `pkg` **shell built-in** (UX); the privileged mutation is a gated VFS op inside
   the VFS cell. **0 kernel changes**; the only gap flagged (RENAME) is designed around.
4. **Version store** — **none in v1** (YAGNI). `pkg list`/`info` derive from `ls /bin` + manifest parse;
   `.prev` file convention for rollback. A version index is a follow-up when a remote repo lands.
5. **Source** — local VFS path first (`/mnt/sd`, `/tmp`, `/data`). HTTP download via existing ostd
   client is an **optional** P04 step. **Remote repo protocol = OUT OF SCOPE v1** (follow-up). **No
   dependency resolution** — cells are self-contained (static, no shared libs in SAS).
6. **Update ↔ Supervisor** — `pkg upgrade` replaces the on-disk ELF; **running instances keep their
   in-memory code** (SAS: spawn reads `/bin` fresh), so the new code applies on next spawn/restart. Live
   hot-swap of a running cell is the **Supervisory Cell's** job (`service::SUPERVISOR=11`,
   `.agents/260712-0800-supervisory-cell-migration/`) — v1 documents the seam (`pkg upgrade --hotswap`),
   does not build it.
7. **Rollback** — keep one `.prev` copy on upgrade. Cheap insurance. `pkg rollback <name>` restores it.

## Cross-cutting risks & mitigations
- **Overwriting a trusted-core / bootstrap cell** → protected-core **denylist** (P01), enforced in VFS.
- **Filling the 32 MB cell-store** → size check + existing VFS quota; reject on overflow.
- **Torn/partial write (power loss mid-install)** → spawn-time sig verify rejects corruption
  (fail-closed); `.prev` enables recovery; write-new-then-nothing ordering (no destructive pre-step).
- **VIFS1 name-shadow**: a ramdisk bootstrap name shadows the cell-store on read (`backend_bin_overlay`
  VIFS1-first) — installing `/bin/vfs` would be dead code. Denylist + a `pkg` warn cover it.
- **Large ELF over 512 B IPC** → use grant-based write (or chunked `Write`); verify path in P01.

## Definition of Done
Not "cargo check clean." Each phase closes on **boot-log evidence + a green integration test** in the
QEMU riscv64 hardened suite. v1 acceptance: `pkg install /mnt/sd/hello.cell && hello` runs; `pkg info`
shows requested caps; a **tampered** package installs but **fails to spawn** with an audit event; a write
targeting `/bin/vfs` is refused; `grep kernel/src` shows **no** kernel edits from this plan.

## Open questions
- Cell-store write mechanism for MB-scale ELFs: chunked `Write` (works today, slow) vs a path-based
  grant write helper (faster, ~1 new VFS helper, still no ABI change). Decide in P01 by measuring hello.
- Should `pkg` be feature-gated (`--features pkg`) so the recovery-hatch shell can ship without install
  authority? Leaning yes for defense-in-depth.
- Install-time userspace Ed25519 verify (reject-early) — worth the pubkey/payload duplication, or leave
  advisory-only? Deferred to P02 review.
