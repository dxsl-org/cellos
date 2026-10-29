---
title: "CellosFS crash safety and the D03 CommittedDurable outcome"
status: planned
origin: "Phase 09 S9-4a durability audit (2026-10-25); D79-12 as revised by the user"
---
# CellosFS crash safety (separate plan)

## Why

D03 now has a `SaveOutcome::CommittedDurable` variant, and CellosFS has a durable-commit function skeleton (`CellosFs::sync_durable`) that always reports `DurabilityUnavailable`. The S9-4a audit showed that the default engine cannot honour "after the reply the new bytes survive power loss; before it, old or new whole". So nothing returns `CommittedDurable` yet.

Until this plan is done, every D03 commit, including SSH first-trust persistence, is `Committed`: synced, not crash-proof (D78-2).

## Audit findings to fix (file:line at audit time)

1. **No recoverable root protocol.** `sync()` writes copied inode-table blocks, then **overwrites the allocation bitmap in place**, flushes (FS1), writes the alternate superblock and flushes (FS2) (`libs/cellos-fs/src/engine.rs:774-841`). There is no journal.
2. **Old-root data reuse before commit.** `rename_replace_if` frees the replaced destination's extents in memory (`engine.rs:712-771`). The following `sync()` can then allocate an inode-table block onto them and overwrite old data before the new superblock exists.
3. **Bitmap before superblock.** A cut after FS1 but before the superblock leaves the old superblock pointing at blocks the persisted bitmap calls free. `open()` then returns `Corrupted` (`engine.rs:108-119`) instead of the old state.
4. **No mount recovery.** Nothing reconstructs allocation from reachability or replays a transaction (`engine.rs:79-136`, `allocator.rs:59-120`).
5. **Torn 4 KiB blocks.** One filesystem block is eight independent 512 B writes (`cells/services/vfs/src/disk_cellosfs.rs:34-45`, `blk_router.rs:160-190`). The bitmap and data blocks have no checksum.
6. **FLUSH evidence.** NVMe FLUSH (opcode 0) is waited for and its status checked (`cells/drivers/nvme/src/controller.rs:415-524`). A timeout returns an error but does not abort the command, which may still complete later.
7. **Cache state unobserved.** VWC is read from Identify but never used; no FUA is set. QEMU runs with the default cache mode, which is not pinned.
8. **Only a host-side experiment exists.** `cow_experiment.rs` (FS1/FS2 roots, reachability reconstruction) is host-only behind `g2-cow-experiment` (`libs/cellos-fs/src/lib.rs:18-20`).

## Phases (to be planned in detail, each with CP1/CP2)

1. **Design.** Choose a whole-filesystem COW or journal protocol that keeps every block reachable from either valid root until the new root is durable, with a checksummed allocation state or rebuild-on-mount. Decide between promoting `cow_experiment` and a fresh design.
2. **Torn-write model.** Make the metadata and commit records safe under 512 B tears (checksums, a sequence-numbered A/B root, a whole-block commit record).
3. **Device contract.** Make a successful FLUSH completion mandatory for `CommittedDurable`, observe VWC, pin the QEMU cache mode in the lanes, and handle a late completion after a timeout safely.
4. **Mount recovery.** Choose a coherent committed root, rebuild or verify allocation, and reclaim uncommitted candidates.
5. **Outcome.** `sync_durable` performs the proven protocol. VFS `publish_if` returns `CommittedDurable` only on it; `Committed` keeps its weaker meaning.
6. **Evidence.**
   - A host power-cut harness cuts at every 512 B write and both flush points, then remounts. Before the reply the destination must be old or new whole; after `CommittedDurable` it must be new.
   - A private QEMU crash lane SIGKILLs during repeated commits with a pinned write-back cache, cold-boots and checks the same properties.
7. **Migration** of existing volumes, or a format version bump with a refusal of older volumes, and an update of consumers: the editor save in D03 and the SSH trust store in Phase 09 S9-4c.
