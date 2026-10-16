# Native development handoff r1 — P02–P07 dependency-scoped preparation

## Decision requested and current status

**DRAFT FOR REVIEW; NOT AUTHORIZED.** The user selected “Chuẩn bị handoff phát triển native”, which authorizes preparing this packet, not editing frozen interfaces or closing CP1/CP2. Existing scoped consents remain unchanged; D05 and D07 are not inferred approved. This packet is a dependency-scoped development proposal, not a substitute for missing operational/hardware inputs.

The machine-readable companion pins these exact document bytes, all six phase contracts, 12 relevant D preparations, source files, source census and revised conditional controller/portfolio/lock dispositions. Approval must name this revision and digest. Any subsequent change to candidate bytes, source edit set or invariant requires a successor packet, preserving this one.

## Proposed first executable tranche: N02-A per-authority arena invariants only

One integration owner, one tranche in flight. This first tranche is intentionally **internal and non-activating**: correct per-authority arena invariants before proposing public dispatch/caller cutover. It establishes no whole-system metadata bound or P02 completion.

### Exact allowed paths

- `kernel/src/cell/cspace.rs`: existing internal Principal/SlotEntry/CellAuthority implementation and its actual-module tests only.
- `tests/unit_cspace_smoke.rs`: existing host environment for that actual module; no independently reimplemented authority model.
- `CHANGELOG.md`: bounded fix description only.
- `.agents/261011-1200-cell-contract-authority/validation.md`: append observed evidence; preserve prior entries.
- `.agents/261011-1200-cell-contract-authority/consumer-census-source.json` and `exact-deltas/manifest.json` in that same controller directory: preparation-only refresh after preserving full predecessor bytes and hashes in the output records below; no candidate/checkpoint promotion.

New output records are restricted to `build/native-development-n02a/{pre-implementation-manifest.json,pre-implementation-census.json,verification.json,review.json}` and disposable driver/VM captures outside the source tree. The r1 handoff Markdown/JSON, pinned contract inputs and recorded approval decision are immutable after approval; record output hashes separately, never rewrite an approval input to conceal drift.

`kernel/src/task/scheduler.rs`, `kernel/src/task/cap.rs` and `kernel/src/cell.rs` are pinned read-only consumers for this tranche. No syscall/SDK/HAL/assembly changes, new module entrypoint, public reexport, registry replacement or launch-policy change is authorized here.

### Internal behavior to implement and prove

1. Preserve exact-incarnation registry lookup and the existing result/status numeric values and byte codecs. Principal codec decoding establishes shape only, never Kernel or issuer provenance. Do not mint or accept a privileged execution context from payload bytes.
2. Allocation/derivation after authority retirement must return existing `AdmissionCut` before changing tickets, slot occupancy, lineage or accounting. A parent admission cut likewise forbids new descendants with `AdmissionCut`; holder close remains valid cleanup without implying a device fence.
3. Child rights must be a subset of parent rights; finite expiry cannot become absent or later than its parent. Return existing `Denied` before mutation for widening; validate these invariants in the arena mutation path, not merely one dispatcher. Root issuance remains kernel-internal and is not evidence of caller authorization. This tranche does not add an authenticated time source or claim actual expiry-at-invocation enforcement.

Only the already-representable class-rights subset is exercised here. For `CSpaceAuthority` root/child mutation, reject class bits above bit8 with existing `Invalid`; derive still requires the parent subset. SlotEntry has no MMIO/block-region subgrant metadata: N02-A dispatch must reject any nonzero `mmio_mask` or `block_region_mask` with existing `Invalid` before allocation, rather than drop it. Root CSpace slots in this tranche therefore carry class bits only and no subgrant; actual mask-bearing root/child metadata and per-field attenuation belong to the separately approved N02-B contract. Add independent refusal cases for class-bit widening, unsupported MMIO and unsupported block-region masks. This is an explicit unsupported-feature refusal, not implementation of mask delegation.
4. Every descendant keeps retained parent metadata after holder close. No parent-index retarget, stale-ticket admission, counter wrap or partial mutation at depth/capacity/exhaustion is permitted. Existing monotonic recipient-local ticket lookup remains opaque; no public bitfield decoder or compatibility alias.
5. `retire_cell_authority` establishes admission cutoff only; it must not set authority/slot `quiesced`. No N02-A runtime transition produces Quiesced or frees external resources. Ordinary nonretired close/reclaim remains as in the current baseline; **once retired, occupied records including closed leaf holders are retained without slot reuse**, pending the deferred N05/N06 acknowledgement boundary. Exact retired transitions are specified below.
6. Bounds 256 slots and depth 8 are existing experimental structural ceilings, **not measured production limits**. This tranche does not select a production profile, increase them, add unbounded collections on admission or replace fixed fit with heap growth. Refuse safely at capacity.
7. Duplicate registration of the same `(CellId,generation)` must preserve its original record, live/closed ticket namespace, ceiling and retirement cutoff; it must not replace the record with a fresh arena. Keep the existing `register_cell_authority` signature and scheduler callsites unchanged; no implicit generation promotion or ticket reset. A duplicate is an idempotent preserve-existing operation, not approval of changed ceiling inputs.

The global registry is not production-bounded by this tranche. Its allocation/retention accounting, scheduler-lock convergence, holder/issuer/receive/kind admission, actual expiry checking, bootstrap sealing and syscall/SDK cutover remain N02-B obligations. Structural arena bounds must not be reported as whole-system capacity or revocation quiescence.

| Retired action | Existing status / ticket behavior | Metadata transition |
|---|---|---|
| Allocate or Derive | `AdmissionCut`; no new ticket | None; no count/counter/lineage mutation |
| QueryRevoke with retained ticket before Close | `AdmissionCut`, never `Quiesced` | Query only; retain occupied slot and original ticket |
| First Close of open holder | `Success` through either existing internal dispatcher, even while revoking | Clear `holder_open` only; keep `in_use`, ticket, ancestry and retained metadata |
| Repeated Close | `Stale` | None |
| QueryRevoke after Close | `AdmissionCut` on the retained exact ticket | Query-only retained lookup; must not authorize invocation, derivation, another close or another grant |
| Unknown/zero ticket | Existing `Stale`/`Invalid` respectively | None |
| Duplicate register | Preserve existing record and cutoff; unchanged void signature | No replacement, ticket reset, wider ceiling or release |

Nonretired QueryRevoke semantics remain as baseline. There is no new acknowledge/reclaim entry in N02-A. Dispatcher retirement guards must permit only cleanup Close and query-only QueryRevoke; every other action remains cut. Retained closed-ticket lookup is confined to QueryRevoke, not the live holder lookup.

### Verification and publication gate

- Before edits, collect actual references (LSP if available; record failure and concrete lexical/build closure otherwise), inspect all arena callers and run the current 10-test actual-module baseline.
- Keep failing-before/passing-after tests for retired allocation, derive attenuation/expiry, duplicate registration and false quiescence. Retain existing close/revoke/reuse/exhaustion tests; do not re-pin incidental defaults or test copies.
- Run the complete actual-module host harness and x86 kernel check/release build. A disposable executable must exercise actual production methods for at least one accepted and each changed refused transition.

Exact host behavioral command: `rustc --crate-type=rlib --crate-name types libs/types/src/lib.rs --edition=2021 -o /tmp/libn02a-types.rlib`, then `rustc --edition=2021 --test tests/unit_cspace_smoke.rs --extern types=/tmp/libn02a-types.rlib -o /tmp/n02a-cspace` and `/tmp/n02a-cspace --test-threads=1`. New tests must exercise actual methods/dispatchers for the retired transition table, rights/expiry widening, each unsupported mask and duplicate registration. Actual x86 commands are `cargo check -p cellos-kernel --target x86_64-unknown-none` and `cargo build --release -p cellos-kernel --target x86_64-unknown-none -Z build-std=core,alloc`. Host transitions prove only those local behaviors; x86 commands prove compile/link only. No native derive/close/query runtime witness exists because syscall/SDK consumers remain unwired; creating that path is forbidden by this tranche.
- Independently review security/lifetime/consumer compatibility before packaging. If an internal change needs a forbidden file or byte change, stop that tranche and request a pinned amendment; do not reach through a directory grant.
- Native boot smoke only proves the image boots. No SMP, grant, issuer, retirement-Gone, resource/device or phase acceptance follows from it. Normal production deployment, retained disks, remote routes and profile activation remain forbidden.

### Rollback

Cold-boot an independently known-safe image. Never translate live tickets, reset counters/epochs or assert that code rollback undoes admitted effects. Retain failure receipts and any possibly admitted references; disable new admission on uncertainty. No real ledger/key/floor/credential operation is allowed by this tranche.

## Ordered P02–P07 successor handoffs — not executable grants

These are concrete entry/exit contracts for preparing successors. Their phase/preparation bytes are pinned by the companion manifest. They do not open automatically when N02-A passes: each needs its own exact source set, caller table, byte/layout/error/output/removal proposal, independently bounded fit and explicit approval before edits.
Every successor's affected public/frozen change requires an **exact complete candidate revision/digest and Law-1 CP1**, not scoped semantic consent. D05 and D07 are currently blocked and unapproved. Any earlier no-wire internal prerequisite needs its own separately defined/reviewed/approved exception; no implicit exception is created by a row below.


| Successor | Required input and implementation boundary | Required proof before next handoff |
|---|---|---|
| N02-B shared authority/caller cutover | D01/D02/D03/D11 exact native carrier/entry/version decision, real bootstrap/receive ceilings, scheduler-lock publication/admission protocol and complete legacy CapSet/CapRegistry/BootFS caller migration. No second authority table or shim. | Actual forged Kernel/stale/foreign/wrong-kind/ceiling denial; sibling admission versus revoke/replacement; measured metadata and bounded successful narrow grant; actual artifact provenance on every launch route. |
| N03 execution/lifetime | N02-B identity/admission handoff; exact D07 clean-worker versus whole-Cell fault decision and scheduler/context/root/TLB participant fence; private G1 implementation is input, not production quota proof. | Two-CPU distinct-thread TLS/FPU and spawn/retire races, aggregate entitlement with fixed predeclared period/burst/tolerances, complete selected/outgoing participant retention. Clean worker exits locally; unhandled fault retires exact Cell. |
| N04 operations/waits | N02/N03 lifetime; D04/D10/D11 exact operation/reply/wait/take ownership, filtered registration, bounded reservation, terminal/cancel/error and output-copy rules; full SDK/provider caller removal. | Actual reply/cancel/death/take races, single terminal/consumer, no foreign wake, Small output preserves result, no terminal/IRQ allocation, no post-dispatch replay and bounded retained charges. |
| N05 resource ownership | D05/D06 consent and N02–N04; actual immutable receiver ceilings, versioned issuer-owned VFS/NET descriptors/offer/accept/receipt tables, grant/use/pin/ring admission and all parsers/clients. No kernel duplicate VFS ACL or raw-pointer compatibility fallback. | Exact issuer/holder/restart/type/rights denial, all-or-nothing receive/failed delivery reconciliation, close-parent versus revoke, missing DMA acknowledgement quarantines, no early frame/reference reuse. Hardware-sensitive claims stay denied absent physical proof. |
| N06 replacement/recovery | D07 plus N04/N05 lifetime; actual state/effect owner, commit watermark and writer fence; complete keyed publication/intent/result/Unknown contract. Native durable execution separately requires admitted issuer/genesis/backend/floor packet; libraries and filesystem flush are insufficient. | Restore/prepare/output/commit/reply-loss failures do not publish incomplete Ready; old epoch cannot commit after fence; same-key restart cannot duplicate publication; acknowledged state survives the declared failure class; Unknown remains charged. |
| N07 native/compatibility | N04/N05 typed consumers; D08/D09/D11 exact native bootstrap/stream/provider/header/sysdep/image tuple and exhaustive fd/symbol caller disposition. Preserve neutral stream/TLS/futex mechanisms and bootstrap acyclicity. | Native-only file/network/lifecycle without POSIX linkage/provider; separate exact C/C++ subset with actual local fd/TLS/errno/allocator behavior and isolation. No SAS downgrade or source-text linkage test. |

No successor may invent native opcodes, consume reservations, silently reinterpret old postcard enum positions, broaden payload authority, or implement frozen/public/persistent changes merely because a phase is listed here. CP1 precedes such edits; actual phase CP2 precedes release. Development consent is not operational authority.

## External prerequisites that remain unchanged

- Actual owner-approved issuer/genesis/maxima/home/key-purpose/public-pin and Ready/origin/state/source-set assignments are missing. Do not request/store private keys in documentation or promote test keys.
- Native backend admission requires an actual volume/device/driver/image/firmware/cache/barrier/failure tuple and authenticated floor or explicitly owner-admitted nonregression class. SQLite lab roles, ordinary hashes and successful sync are not that decision.
- Protected Build AC-001..011, relay activation AC-012, exact-product NO-GO, relay-first/direct-LAN, physical Intel remapper/VMX and matched Linux qualification remain separate. No SSH qualification host is configured.
- All 38 legacy locks and the ten-phase DAG remain effective. This proposed exception applies only to N02-A's explicit files after approval; it is not a legacy-plan unlock, P02–P07 blanket grant or complete-package CP1.

## Decision record semantics

An approval of this exact r1 would authorize **N02-A only**, under the paths and invariants above, to create real prerequisite implementation evidence. N02-B through N07 remain separately gated successor work. A rejection leaves current source/enforcement unchanged. Neither outcome qualifies a phase or changes production, operational trust or deployment admission.

Before any implementation, append the explicit user decision, revision, **both Markdown candidate SHA-256 and companion JSON SHA-256**, and review references to `.agents/261011-1200-cell-contract-authority/native-development-handoff-decisions.jsonl` (create on first actual decision). This is the only additional decision output path. User approval must name both file hashes; a Markdown digest alone cannot bind a repinned companion. Activation first verifies both immutable approved files, then every embedded source/contract/scheduling pin. Any mismatch keeps admission closed and requires a successor. The record may capture rejection or exact N02-A approval, never infer approval from preparation consent. Conditional controller/portfolio/P01/P02/lock dispositions take effect for N02-A only after that explicit decision; no successor or production gate opens.
