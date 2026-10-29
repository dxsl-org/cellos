# Phase02 CP1 and private-fixture authorization

Status: **approved for this bounded implementation/execution slice; Phase02 acceptance and CP2 remain pending**.

## Exact user decision

After the user selected the scoped handoff “Giao slice Phase 02”, the assistant presented the exact approval question covering syscall262, reviewed codecs/ownership, the narrow VFS cwd binder and private x86 build/sign/image/boot. The user selected **“Duyệt CP1 và fixture riêng”**. This is the explicit exact-annex/source/private-execution approval; it is not inferred from earlier general design consent or continuation. Earlier plan/candidate design consent and this exact ABI approval are distinct decisions; neither grants CP2.

- Approved immutable annex: [phase-02-source-admission.md](phase-02-source-admission.md).
- Approved SHA-256: `73bc7483ae6d05d6435c27f12a1c2890e3b550e63666b4339ed75f9b8371ae67`.
- Bound source snapshot: [phase-02-source-snapshot.json](phase-02-source-snapshot.json), SHA-256 `2aaa62972b778ff84f3cdd2a9ea76f86ef75258fee265b21ae93184372027e3b`.
- Source HEAD: `fabd6ab7e3f2d9ea90a931fe53b7d43e509888dd`; exact78-path closure,63 existing dirty-byte identities and15 proposed new paths. No clean-worktree claim.
- [Independent admission review](reports/phase-02-admission-review.md) is design evidence only, not target acceptance.

The approved annex remains byte-for-byte immutable as the approval subject; its proposal-time pending wording is historical, superseded only by this explicit authorization record and the controlling ledger entry.

## Execution boundary

The current assistant is the single integrating owner through CELL-CONTRACT-0024 for D01 child context/publication and D02 streams/input/SDK/headless consumers, including only the annex's narrow VFS preparation binder and exact private fixture file closure. Independent non-overlapping implementation/review work may be delegated, but kernel publication/owner integration has one serialization owner. No old locked predecessor is resumed and no blanket lock is released.

Owned private verification directory: `/tmp/cellbox-phase02-zRWExI6S`. All Cargo output, signing keys, assembled images, captures and VM resources must stay private/noncanonical. The baseline command excludes the kernel workspace package to avoid its build script embedding the quarantined canonical ramdisk; kernel behavior is covered separately by existing pure host harnesses and the later fresh private signed image. A failed baseline compile is recorded, not relabeled as a passing suite.

Before source edits: capture applicable full-suite baseline/failure identities and preserve logs. Do not overwrite unrelated dirty source. Later builds/signing/assembly/boot may use only source-matched private inputs, fail closed and follow the exact annex's provenance/runner/isolation rules. No external dependency acquisition or remote service effect is authorized by this record.

## Unchanged prohibitions

- Preserve the containment guards and all three quarantined artifact bytes/identities; no canonical packaging or rollout.
- No GUI/SSH bridge, D03 trust/save, D04 telemetry, suite-path cutover, unrelated manifest capability or lifecycle-admin expansion.
- All29 CBX outcomes remain pending until attributable actual evidence; CBX-03..05 require real private signed-x86 scenarios.
- CP2 remains separately required and not eligible until its actual implementation/runtime dossier exists.

Rollback is slice-only source reversal and cleanup of this owned private VM/images after saving sanitized evidence; no source rollback can undo already delivered IPC or private-guest writes. No persistent user disk, remote endpoint or quarantined artifact receives authorized writes.

## Amendment A1 — allocation-free publication reservations

User selected **“Duyệt bổ sung 3 file”** for [phase-02-publication-reservation-amendment.md](phase-02-publication-reservation-amendment.md), SHA-256 `ae6be1c39453287d19cf7027681f5e24c19b112cad387b3f0ff919c80aa676e0`. The admitted closure is the original78 paths plus `kernel/src/measurement_log.rs`, `kernel/src/task/hart_local.rs`, `kernel/src/task/hart_local/ready.rs` (81 total), with pre-edit hashes recorded in that amendment. All stated invariants bind implementation, including the explicit prohibition on editing `kernel/src/loader/manifest_section_tests.rs` or adding a compatibility shim. Two independent read-only reviews found it safe to present; neither is implementation evidence. No ABI, authority, route, rollout or CP2 change.

## Admitted interpretation notes (implementation-time, no new surface)

- Headless Serial `AttachFrontend` admits only frontend principal == attested controller; foreign payload principals are Denied (no canonical liveness transport exists). GUI/Ssh stay Unsupported. Cross-process frontend attach is **not** claimed by Phase02.
- Physical UART echo suppression cannot be enforced by input. Echo-off is guaranteed only for private virtual Serial submissions; evidence must keep that distinction. No secure-capture claim for arbitrary host serial.
- Legacy no-context fd0/1/2 console behavior is unchanged; only a context-present committed child routes fd0/1/2 to its owner-enforced endpoints.

## Amendment A2 — remaining serial/stream limitations

User selected **“Duyệt A2”** for [phase-02-limitations-amendment.md](phase-02-limitations-amendment.md) (full file closure and behavior as written there; physical echo-off stays unenforceable and refused). No ABI, authority, route, rollout or CP2 change.

A2 follow-up: the user ratified the extra edit `cells/tools/shell/src/top/render.rs` and approved an image `rm -i` check over the RAM `/tmp` mount (runner change only; no new authority).

CP2 (Phase 02 implementation baseline) approved 2026-10-21; see [dossier](phase-02-cp2-dossier.md). Phase 03 admitted to proceed against the Phase 02 contracts. Quarantine and rollout unchanged.
