# Native five-step closure — source-bound execution record

## User scope and ordering

The user requested completion of the five ordered native steps, not another Linux/SQLite lab. Preserve the order: native profile → DCO1/proof adapters → prerequisites/complete-package CP1/native journal and lifecycle → native VMware fault matrix → native finite profile/caller cutover/target qualification/CP2. The completed Linux lab is prior evidence only. This record cannot mark later native steps complete based on that lab.

## Step 1: selected transport candidate and actual missing assignments

Select a single disposable native x86 VMware development lane: Cellos ISO on IDE CD-ROM, exactly one separately preprovisioned CellosFS disk exposed through VMware SATA/AHCI, no network/shared folders, one CPU and256MiB. Native `/bin/ahci` is the transport candidate; Linux `ata_piix`, ext4, SQLite and the lab role roster are not used. P5 `/srv` uses existing LBA931072/131072sectors; P4/P5 are formatted offline only by the existing CellosFS provisioner. This is a candidate transport, not an admitted journal/backend/floor.

Source evidence:

- `cells/services/vfs/src/blk_router.rs`: driver-Cell IPC owns block I/O. No-driver raw syscall fallback is not an x86 VMware block transport.
- `cells/services/vfs/src/disk_cellosfs.rs`: bounds-checked4KiB block mapping to eight512B sectors and driver flush.
- `cells/services/vfs/src/backend_cellosfs.rs`: open-existing-only, failure poisoning, explicit warning that observed native sync is not crash-preserving acknowledgement.
- `cells/services/vfs/src/manager.rs`: existing `/srv` mount is not an admitted durable journal.
- `kernel/src/task/launch.rs::publish_prepared`: local generation/root publication mechanism, not a stable creation-key/live-claim/closed-key protocol.
- `libs/authority-protocol`: typed authentication and protected-store traits; concrete admitted budget issuer/store/floor adapters are not present.
- KMS186B two-slot journal is a separate protocol without the required DCO1 flush/floor/ownership contract. Supervisor and local quota cannot become issuer or global ledger by renaming.

Actual native probe completed: Cellos ISO SHA256 `06f1317e96e143126596c8e8ee42885d2cf6b9114a59d9d1e5486346c32024e8` booted with the separately offline-provisioned native disk at `/mnt/c/Temp/cellos-vmware/native-cellos-profile-xnv4zkuu/`. Native AHCI bound BDF00:10.0/ABAR0xfebff000, IDENTIFY reported VMware Virtual SATA Hard Drive firmware00000001 and1062144sectors, and registered BLOCK_DRIVER tid4. Both existing CellosFS mount markers were observed; native IDE driver and Linux/SQLite were not involved. FAT P1–P3 remain deliberately blank and their mount/bounds refusals are not journal results. The guest was hard-stopped and its absence confirmed; retained serial SHA256 is `8cfdf36096efddb521f548902ce14b0dc78a8351fbfb7c46f3bbd2fad78e0a38`. Receipt: `build/native-cellos-vmware-profile-probe.json`. This selects an observed native development transport, not crash-safe filesystem/journal admission.

## Step 2: DCO1 codec and proof adapters implemented

Implemented standalone allocation-free `libs/durable-charge-codec/` (`no_std`, `#![forbid(unsafe_code)]`):
- `record.rs`: complete structural encoding and decoding for all 27 DCO1 record kinds (1..11, 32..36, 48..58) with packed offsets, static schema IDs, little-endian unsigned integers, and domain-separated SHA-256 trailers.
- `references.rs`: bounded acyclic reference traversal and profile equality checking; unassigned external schemas return typed `AdmissionRequired` instead of silent success.
- `proof.rs`: NCF1 fixed 1660-byte signed-fact carrier and verification using `ed25519-compact` version 2 (`no_std`), binding role, purpose, lane, scope, journal, genesis, O, A, T, writer/authority fences, revisions, and public pins.
- Verified: 35 host integration tests pass (`tests/codec.rs`, `tests/proof.rs`), `examples/smoke.rs` passes, and clean compilation on `x86_64-unknown-none`.

## Step 3: native journal implementation and governance blocker boundary

Implemented standalone allocation-free `libs/durable-charge-journal/` (`no_std`, `#![forbid(unsafe_code)]`):
- `superblock.rs`: double-buffered (A/B, blocks 0 and 1) superblocks with monotonic sequence numbers, capacity bounds, and SHA-256 checksums.
- `journal.rs`: atomic double-barrier DCO1 record append over `cellos_fs::BlockDevice` (data block writes -> flush -> alternate superblock update -> flush), and recovery choosing the valid superblock with the highest sequence number.
- `gates.rs`: native lifecycle authority state machine (`AuthorityGate`) enforcing two-plane resize accounting (`G = max(Q - B, 0)`, `L = max(Q - E, 0)`), fold without credit, partial shrink credit installments, proven-no-application cancellation, `Closing` intent gate blocking future resizes, one-credit `release_once`, and bootstrap adoption reconciliation gating.
- Verified: 7 integration and crash-recovery tests pass (`tests/journal_crash.rs`, `tests/gates_lifecycle.rs`), and clean compilation on `x86_64-unknown-none`.

**Gated governance boundary:**
Integrating this native journal engine into `kernel` and `cells/services/supervisor` requires modifying locked files protected by the 38 scoped locks. Complete-package CP1 approval on the 23 exact delta packages and closing the remaining upstream prerequisites (15 G0 blocker classes in `build/cp1-prerequisites/g0-vtd-readiness-20261014.json`, G2 owning-service D1/D2 qualification, and production issuer credentials) require maintainer authority and cannot be self-approved. Steps 4 and 5 remain sequenced behind this gate.

The native O1–O9 producers are not currently assigned/implemented: O1 unique budget genesis; O2 reserve; O3 live claim/Closing; O4 keyed kernel publication/fence; O5 destination intent/result/closure recovery; O6 all-owner use/quiescence/Gone; O7 fixed-home credit; O8 whole-vector resize; O9 authenticated recovery/floor. Existing source seams do not establish those authority facts. New implementation is necessary; this is not an unanswered architecture-choice question.

## Hard dependencies before step 3

Current G0 receipt reports15blocker classes, all captures not fully settled, incomplete per-owner high-water and no physical remapper qualification (`g0-instrumentation-reconciliation.md:40–46`). G2 owning-service journal/Unknown recovery remains distinct from the corrected Python model and default-off host CoW experiment. The native profile must either satisfy the exact required G0–G2 conjunction or receive a separately scoped bounded native-experiment handoff with an independent finite basis; this record does not silently waive it.

Complete exact native CP1 needs an actual pinned packet: producer/consumer/principal/home/public pins; native signer/verifier and proof-byte profile; backend/floor/failure contract; publication/fence lock and I/O staging; finite pre-effect capacities; changed public/persistent carrier/error/output/caller/removal tables and reciprocal source handoffs. User approval of the presented semantic candidates is retained, but it is not evidence that these absent fields or their implementations already exist. CP2 requires actual later native traces, not the lab receipts.

## Truthful completion rule

A mounted filesystem proves transport/mount only. A digest-valid DCO1 vector proves codec behavior only. No host model, Linux guest signature, successful `sync`, source trait, desired component name, timeout or process restart closes native ledger/floor/keyed lifecycle qualification. All five items remain separately tracked; unavailable external/hardware inputs must be named rather than replaced with success-shaped fixtures.
