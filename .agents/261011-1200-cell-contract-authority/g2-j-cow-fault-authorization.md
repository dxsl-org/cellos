# G2-J CoW/fault scoped development authorization — r1

2026-10-14. Authority basis: user's explicit request to review the G2-J proposal and settle authorization scope before CoW/fault implementation. This records permission only for the named development prerequisite, not entire G2-J, persistent-format CP1/CP2, release or any legacy unlock. No implementation starts in this review turn.

## Granted scope

Implement and exercise a **default-off direct-library CoW experiment** on freshly offline-formatted disposable memory/file images, at most64MiB per image. Use feature `g2-cow-experiment`, default absent; new `libs/cellos-fs/src/cow_experiment.rs` is a PRIVATE module selected internally through existing `CellosFs` signatures in feature-enabled host tests. No `pub mod`, re-export, new exported signature or stable experiment API is authorized. Feature-off code paths and every VFS consumer remain unchanged; VFS builds must not enable the feature, including Cargo feature unification. Before touching existing exported symbols, use LSP references or record actual server failure and complete caller fallback. No normal mount selects the experiment.

Exact allowed existing source paths:

- `libs/cellos-fs/Cargo.toml`: add only default-off feature.
- `libs/cellos-fs/src/lib.rs`: feature-gated private module inclusion only; no public export.
- `libs/cellos-fs/src/engine.rs`, `libs/cellos-fs/src/allocator.rs`: only feature-gated/internal reuse needed by the experiment; existing default path unchanged.
- `libs/cellos-fs/tests/fs_tests.rs`, `libs/cellos-fs/tests/power_cut_fuzz.rs`: retain old scenarios and add meaningful feature-gated fault behavior, or put those tests in the allowed new file below.
- New `libs/cellos-fs/src/cow_experiment.rs`, `libs/cellos-fs/tests/cow_experiment.rs`.

Preparation records, this plan's review/authorization, census/manifest and `CHANGELOG.md` may be updated with exercised evidence. Build receipts belong under `build/g2-j-cow/`. No other source/build/provisioner/CI path is implicitly allowed. Pin baseline bytes and absent-new-path status in the companion manifest before editing. Intervening source changes require integration-owner reconciliation; never repin original consent retroactively.

## Required algorithm contract

Read both physical root slots. Include all checksum-valid geometrically/semantically valid roots; allow fresh one-root state and torn inactive target with one surviving root. Valid-checksum malformed graph refuses. Reconstruct allocation from full valid-root union and fixed reservations, not shared bitmap free bits. Detect aliases, cycles, metadata/data overlap, duplicate candidate assignments and overlap with prior unpublished reservations. Bound scan/traversal/buffers by declared geometry before any write; no avoidable dynamic unbounded maps or vectors in the experimental core.

CoW every modified data/table block, reserve candidate before write, FS1 data/metadata barrier before alternate-root publication, FS2 before acknowledgement. Keep all blocks reachable from every valid root until durable slot displacement. Error after mutation entry closes that instance permanently; fresh reopen after fault is read-only observation then discard. No rollback repair, normal fallback or resumed mutation after ambiguous reply. V1 encoding/version/errors unchanged; do not use reserved bytes or introduce OGJ1. Host failure model: atomic512byte sectors, independently injectable ordered writes and truthful flush persistence; explicitly distinguish volatile/durable backing and pre-effect versus post-effect lost acknowledgement.

## Required evidence before accepting the slice

- Actual compiled fixed storage and bounded call-stack evidence; include traversal/conflict metadata, root descriptors, bitsets, reservations, buffers, lock/diagnostic overhead. The prior8192byte bitsets and30block arithmetic are NOT total profile proof.
- Real library create/modify/inline-to-extent/extent-to-inline/namespace changes through experimental CoW, with byte-exact previous acknowledged graph preservation or explicit refusal after each cut.
- Every512byte sector boundary of every changed4KiB block; FS1/FS2 pre-effect and post-effect/lost-ack cuts; bootstrap one-root and later two-root rotation; unreadable/malformed root, torn bitmap, shared immutable roots, full space, counter exhaustion and reservation alias refusal.
- Verify protected blocks never enter allocation and delayed reclaim only after durable displacement; retain all nonpassing cases. Tests must not allow arbitrary older successful graph as acknowledgement preservation.
- Build/test both feature-enabled and default-disabled library. Observe actual changed engine path via executable host scenario, not source-text assertions or a mock echo. Record default-path regression and no normal-image/VFS activation. No whole hosted-kernel PASS inferred.
- Actual-diff review before evidence interpretation. Success is software algorithm evidence only; no physical power-loss/media or production qualification.

## Explicit exclusions and stop rules

No `cells/services/vfs/src/**`, wire/API/error/manifest permission, quota/reservation/Unknown owner transfer, durable lifecycle schema, OGJ1 payload/commit/checkpoint/switch, genesis, replay/retirement, issuer/fence/floor, KMS/protected provider, hardware/VMX/remote activation, production image or retained operator volume. No authenticated floor mocked into success. A requirement needing any excluded change stops the affected slice for a separately pinned review; it does not expand this grant.

The complete journal candidate stays blocked on real codecs/owners/backend/floor and exact persistent-byte approvals. The selected once-only session is a design input, not a fabricated application implementation. All23 downstream checkpoint pairs,38legacy locks and ten pending phases remain unchanged.

Rollback before durable use removes only experiment-owned changes and disposable evidence images; preserve failures. After any future non-disposable effect, deletion/binary rollback is not state recovery—close admission, retain obligations and use separately authorized forward recovery. This grant never authorizes such effects.
