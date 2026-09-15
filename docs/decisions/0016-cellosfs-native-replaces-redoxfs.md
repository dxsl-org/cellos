# ADR 0016: CellosFS Native replaces RedoxFS as the `/srv` backend

**Date**: 2026-09-15 | **Status**: Accepted | **Supersedes**: [ADR 09b](../specs/09b-vfs-native-fs-adr.md) (RedoxFS as the `/srv` backend)

---

## Decision

`/srv` is served by **CellosFS Native** (`libs/cellos-fs`), a pure-Rust copy-on-write extent
filesystem written for the Cellos single address space. RedoxFS is no longer part of the Cellos
image-formatting path, the VFS service, or the `/srv` integration suite, and `third_party/redoxfs`
stays vendored for reference only.

## Context

ADR 09b chose RedoxFS to avoid writing a custom CoW B-tree, and ADR 0002 kept its activation
phased: G1 functional use through the generic block driver, G2 production qualification gated.
The filesystem was subsequently replaced by CellosFS Native (roadmap Phase 04b: dual cyclic
superblock ring, packed inodes with small-file inlining, extent B-tree, strict partition bounds,
SAS-grant zero-copy compatibility), which is what `cells/services/vfs` links today —
`backend_cellosfs.rs` and `disk_cellosfs.rs` are the only `/srv` backend, and neither
`cells/services/vfs` nor `kernel/src` contains a RedoxFS reference.

Two things did not follow the switch and are corrected here:

- `scripts/mksrv-img.sh` and `scripts/format-disk-arm.sh` still built `redoxfs-ar` from
  `third_party/redoxfs` and formatted P5 as RedoxFS, seeding a `hello.txt` that no test reads.
  The VFS cannot read that volume, so it re-formatted P5 as CellosFS on the first boot of every
  run: the host-side formatting was dead weight that added a full dependency build to the job and
  made the suite look like it still depended on RedoxFS.
- The suite, its CI job, its cache key, and the `srv-test` cell banner still carried the RedoxFS
  name, which invited the reading that a RedoxFS failure was behind the job's red state.

## Consequences

- P5 ships as a **raw partition**; the VFS formats it as CellosFS Native on first mount, and the
  two-boot persistence test exercises that volume. Image builds no longer need a host filesystem
  formatter, and a previous image cannot leak its P5 bytes into a new one (both scripts recreate
  the sparse image before handing it to the guest).
- `/srv` raw evidence still binds the same partition and the same scenarios (S1–S6 plus the POSIX
  directory-lifecycle and rename smoke, plus the no-disk degrade path); only names and the
  formatting step changed.
- The job's red state is a **kernel fault**, not a filesystem-naming problem: with a RedoxFS- or
  CellosFS-formatted P5 alike, `posix-shim-test`'s live mkdir/rmdir step drives an S-mode load page
  fault in `console_drv::viConsole::poll` (`scause=13`, `sepc=0x8023d708`, `stval=0x10000005`).
  That is tracked separately in `.agents/260914-ci-gate-restoration/phase-02-srv-fault.md` and
  `phase-03-console-fault.md`.
- G2 production qualification for `/srv` remains open, as does the measured NVMe/hardware evidence
  ADR 09b called for; that gate now applies to CellosFS Native, not RedoxFS.
- `cells/services/vfs/src/backend_stub.rs` (the `/srv` `StubBackend` placeholder) was no longer
  declared in the VFS crate root and is removed; `backend_cellosfs.rs` is the only `/srv` backend.
  `access/stub.rs` is unrelated (a guest-disk fixture for the access self-test) and stays.
