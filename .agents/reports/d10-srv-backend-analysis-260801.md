# D10 — `/srv` RedoxFS activation versus ADR 09b

**Date**: 2026-08-01 · **Question from the docket**: were ADR 09b's three trigger
conditions waived, or is the current RedoxFS mount premature? · **Method**: compare the
accepted ADR, implementation history, current block path, integration tests, hardware
status, and storage benchmark coverage.

## Answer first

**The `/srv` RedoxFS mount is premature relative to ADR 09b as written.** No amendment,
superseding ADR, or explicit waiver was found. The implementation landed two days after
the ADR while none of its three conjunctive triggers had been met.

The code is nevertheless real, tested, persistent, and gracefully degrading. Reverting it
to `StubBackend` would restore documentary compliance by removing a working G1/QEMU
capability. The recommended ruling is therefore to **amend ADR 09b to ratify a phased
strategy**:

- G1 proof-of-function: RedoxFS on P5 through the generic block Driver Cell, with
  VirtIO-BLK integration tests — already shipped.
- G2 production activation: RedoxFS through NVMe on real hardware, gated by an automated
  NVMe write/read test, a defined `<100 us` filesystem-read benchmark, hardware results,
  and an explicit partition-capability decision — still open.

Until that ruling is made, the repository has an unresolved architecture violation rather
than evidence that the three conditions were waived.

## 1. The accepted rule is unambiguous and unchanged

ADR 09b remains `Status: Accepted` (`docs/specs/09b-vfs-native-fs-adr.md:3`). It says to
implement `RedoxFsBackend` only when **all three** are true (`:84-91`):

1. the NVMe Driver Cell ships and passes block read/write integration tests;
2. a C930/P870 G2 board is available for hardware validation;
3. the `<100 us read latency` target is defined and measurable.

Until then `/srv` must serve `StubBackend`. Git history contains no later amendment: the
trigger language originates in `afbbcfd2` (2026-06-11), and the only later change touching
the ADR is a project-wide naming/version update.

`docs/specs/09-vfs.md:50` still describes the stub as mounted, so the normative VFS text
also contradicts the implementation.

## 2. The code crossed the gate without recording a waiver

Commit `a010d14c` (2026-06-13) replaced the stub with RedoxFS. Current construction is
unconditional: `cells/services/vfs/src/manager.rs:64-68` mounts
`RedoxFsBackend::mount("/srv")`. `StubBackend` still exists but is not mounted.

This is not a dangerous fail-open mount. `cells/services/vfs/src/backend_redoxfs.rs:37`
stores no filesystem instance when P5 is absent or unformatted, and operations then return
empty/false. Tests in `tests/integration/tests/redoxfs-srv.rs` cover:

- basic mount, create, write/read, list, mkdir, and unlink on RV64 QEMU;
- graceful degradation without a VirtIO block disk;
- persistence across two boots.

Those tests establish a useful G1 proof-of-function. They do not establish that ADR 09b's
G2 activation gates passed.

## 3. Trigger status

### Trigger 1 — partially satisfied later, not satisfied when the mount landed

The NVMe Driver Cell now exists and registers as `BLOCK_DRIVER`
(`cells/drivers/nvme/src/main.rs:91`). The current x86 tests verify controller
initialization/registration and an actual sector-read path through FAT32
(`tests/integration/tests/nvme-x86.rs:73`, `:102`). The changelog records a shell
write/read round trip (`docs/project-changelog.md:543`).

However:

- this work landed after the RedoxFS mount;
- the current Rust integration suite has no dedicated NVMe write assertion for `/srv`;
- `redoxfs-srv` boots RV64 with VirtIO-BLK, not x86 with NVMe.

Therefore trigger 1 is at best **substantially met for the generic block stack**, not
demonstrated end-to-end for RedoxFS-on-NVMe.

### Trigger 2 — not met under the ADR's literal text

The named boards are unavailable. `docs/research/research-riscv-ai-ecosystem.md:17-18`
records C930 as IP without a board before 2027 and P870 as IP without a purchasable board.
SG2042, RK3588, and x86 Q35 may be sensible substitutes, but ADR 09b never authorizes that
substitution. Physical G2 validation also remains pending in the roadmap.

### Trigger 3 — not met

No storage-read latency scenario, measurement definition, harness, or result was found.
`docs/performance-report.md` covers context switching, IPC, syscall, and memory latency,
not RedoxFS reads. Correctness and persistence tests cannot satisfy a `<100 us` latency
gate.

## 4. The implementation also diverges from the ADR's named architecture

ADR 09b describes:

```text
/srv -> RedoxFsBackend -> NvmeBlockAdapter -> DMA Grant block API
```

The shipped path is transport-neutral:

```text
/srv -> RedoxFsBackend -> VicellDisk -> blk_router -> BLOCK_DRIVER IPC
```

`cells/services/vfs/src/disk_redoxfs.rs:19` translates each 4 KiB RedoxFS block into eight
512-byte sectors. `cells/services/vfs/src/blk_router.rs:41` routes those sectors to whichever
block Driver Cell is registered (VirtIO-BLK or NVMe), with a kernel-block fallback. There
is no `NvmeBlockAdapter`, DMA Grant block API, or lazy mount after an NVMe `ServiceReady`
notification.

This generic path may be a better architecture than the ADR's NVMe-specific adapter, but
that is another reason to amend the ADR rather than treating the existing text as fulfilled.

## 5. Partition capability is weaker than the accepted design

ADR 09b required a dedicated `MANIFEST_FLAG_PART_SRV` bit after Law-1 confirmation
(`docs/specs/09b-vfs-native-fs-adr.md:79-80`). Manifest v1 has only `u8` flags, so no new
bit was added. P5 is co-granted with the existing LFS block-region flag
(`kernel/src/task/syscall.rs:445-453`), while `libs/api/src/abi/disk.rs:41-43` states that
`/srv` access is controlled through the VFS `AccessTable` until a future manifest version.

This avoided an unauthorized ABI edit, but it does not provide the partition-level
separation the ADR intended. An amendment should either accept VFS path authorization as
the permanent boundary or retain a dedicated partition capability as a G2 gate.

## 6. Why the later plan does not close D10

`.agents/260613-1200-native-fs-srv-redoxfs-nvme/plan.md:3-22` explicitly reframes the work
as RedoxFS on VirtIO-BLK for G1 immediately, then NVMe for G2 later. That plan explains the
implementation and is strong evidence of intent, but `.agents/` is non-normative and the
plan never says ADR 09b is amended or its triggers are waived.

The repository therefore contains an intended phased strategy without the architecture
decision that authorizes it.

## 7. Ruling options

### A — Recommended: amend ADR 09b for phased activation

Ratify the shipped VirtIO proof-of-function and keep production G2 readiness gated. The
amendment should:

1. distinguish G1/QEMU functional availability from G2 production qualification;
2. replace the NVMe-specific adapter diagram with the generic `blk_router` path, if that is
   the architecture of record;
3. name approved real-hardware targets instead of unavailable C930/P870 boards;
4. define the exact cold/warm read workload and `<100 us` statistic;
5. require an automated RedoxFS-on-NVMe write/read/persistence test;
6. rule on dedicated P5 capability isolation.

### B — Enforce ADR 09b literally

Restore `StubBackend` at `/srv` until all three original triggers pass. This is the strict
document-first outcome, but it withdraws working and tested G1 functionality and leaves the
generic storage stack unused for `/srv`.

## 8. Recommended docket answer

**No evidence shows the three conditions were waived. The mount is premature under the
accepted ADR, but the preferred correction is to amend the ADR for phased G1/G2 activation,
not to discard the tested backend.** D10 remains open until the architect selects A or B.
