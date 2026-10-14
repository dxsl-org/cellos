# ADR 0016: CellosFS Native replaces RedoxFS as the `/srv` backend

**Date**: 2026-09-15 | **Status**: Accepted | **Supersedes**: [ADR 09b](../specs/09b-vfs-native-fs-adr.md) (RedoxFS as the `/srv` backend)

---

## Decision

`/srv` is served by **CellosFS Native** (`libs/cellos-fs`), a pure-Rust extent
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

The original 2026-09-15 cutover corrected two historical leftovers:

- `scripts/mksrv-img.sh` and `scripts/format-disk-arm.sh` had still built `redoxfs-ar`
  and seeded incompatible P5 bytes. The original correction removed this unused
  formatter and shipped raw P5 for guest initialization. That initialization
  policy is superseded below: an open error cannot authorize destructive formatting.
- The suite, its CI job, its cache key, and the `srv-test` cell banner still carried the RedoxFS
  name, which invited the reading that a RedoxFS failure was behind the job's red state.

## Consequences

- **2026-10-14 scoped G2-R-core/B development cutover:** P4 `/data` and P5 `/srv`
  are explicitly provisioned offline, using `tools/cellos-fs-provision` and the
  actual CellosFS library. Runtime mounts open existing volumes only. Blank,
  corrupt, legacy or unavailable volumes stay unavailable with
  `[vfs] WARNING: CellosFS open failed — volume unavailable`; unrelated boot/RAM/FAT
  and the no-disk lane remain available.
- Fresh image producers recreate their disposable images and provision only the
  bounded, completely zero-filled selected native windows. They preserve existing
  boot-config precedence, MBR constants, P1/P2/P3/P6 and guest disk placement in FAT.
  Hypervisor image reuse requires explicit read-only verification before any image
  rewrite; truncate is not blank detection. Existing valid V1 native bytes are not
  reformatted. FAT-only Raspberry Pi/PowerShell producers are intentionally unchanged.
- First-boot, existing-volume reboot and blank/corrupt refusal scenarios remain
  functional development checks, not crash-preserving acknowledgement evidence.
  Host sync plus read-only reopen and SHA-256 receipts bind image/source/formatter/
  partition identity and explicit intent; they grant no issuer genesis, ledger
  migration, device identity, anti-rollback, protected storage or production admission.
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

## Rejected initialization alternatives

- Format on any runtime open failure (including a blank-looking volume): rejected
  because transport errors/corruption cannot prove fresh output or authorize erasure.
- Unconditionally format P4/P5 after truncating a reused image: rejected because
  truncation preserves bytes inside the retained length and can destroy existing state.
- Continue shipping raw P5 and defer producer migration: rejected because open-only
  mounts would deliberately break the known fresh development and integration lanes.

## Offline development commands

Run with no QEMU/device/image users. These commands prepare disposable/staged images;
they do not migrate an initialized ledger. Keep receipts alongside the published image.

```sh
HOST_TARGET=$(rustc -vV | sed -n 's/^host: //p')
# Creates a fresh sparse image, provisions P4/P5 and emits exclusive final receipts:
bash scripts/mksrv-img.sh build/disk_srv.img
# Existing image verification is read-only and never reformats:
bash tools/cellos-fs-provision/provision.sh build/disk_srv.img verify-existing
# Fixed host scenarios: bounds, complete blank scan, existing/nonblank/corrupt refusal,
# exclusive locks/receipts, neighbor-byte preservation and reopen/receipt identity:
cargo test --locked --manifest-path tools/cellos-fs-provision/Cargo.toml --target "$HOST_TARGET"
# VM scenarios require freshly rebuilt kernels and the preprovisioned fixture:
cargo test --manifest-path tests/integration/Cargo.toml --target "$HOST_TARGET" --test srv-cellosfs
# For an isolated current-source capture, bind absolute test-only artifact paths:
# CELLOS_SRV_TEST_KERNEL=/path/to/current/cellos-kernel-srv-test \
# CELLOS_SRV_TEST_DISK=/path/to/staged/disk_srv.img cargo test \
#   --manifest-path tests/integration/Cargo.toml --target "$HOST_TARGET" --test srv-cellosfs
```

Direct single-window CLI (the receipt must not already exist):

```sh
cargo run --locked --manifest-path tools/cellos-fs-provision/Cargo.toml --target "$HOST_TARGET" -- \
  --image build/disk_srv.img --base 931072 --sectors 131072 \
  --intent verify-existing --receipt build/p5-verify-new.json
```

Use `fresh-blank` only for an explicitly staged, all-zero canonical P4/P5 window.
Any mutation/sync failure permanently ends mutation on that image; preserve it for
fresh read-only fault observation, then discard. Reopening does not authorize retries.
G2-J, production/durability qualification and the original protected-domain gates
remain closed. The commands above are runnable scenario sources, not executed evidence.
