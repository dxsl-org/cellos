# Phase 01 review-ready contract candidates and retained approvals

**Preparation revision:** 2026-10-20, candidate v1. **CP1 design consent recorded:** the user explicitly said “tôi duyệt, làm tiếp đi” for the published candidate contract/catalog package. That consent remains valid and is not withdrawn. **Full protected-source admission is incomplete:** accountable owner/file handoffs, allocated codec/namespace annexes and provider/profile/dependency qualification inputs remain missing. An assistant's preparation-integration role is not protected file ownership. **CP2 is pending/not eligible; all acceptance is withdrawn and artifact rollout is blocked.** [reports/rollout-containment.md](reports/rollout-containment.md) is the authoritative current withdrawal record.

This supersedes the old “no candidates/digests exist” inventory claim, not retained authority gates. The candidate package now makes reviewable decisions rather than asking a future implementer to invent product semantics. Existing [spec.md](spec.md) and [abi-session-launch.md](abi-session-launch.md) were reused, not replaced with a competing design. [abi-vfs-telemetry.md](abi-vfs-telemetry.md) supplies their D03/D04 counterpart. [command-catalog.md](command-catalog.md) owns the observed-versus-proposed caller/right matrix; it is not evidence every proposed grant already exists.

## Candidate package and exact byte identities

Run from repository root:

```sh
sha256sum .agents/261020-1750-cellbox-cli/spec.md .agents/261020-1750-cellbox-cli/abi-session-launch.md .agents/261020-1750-cellbox-cli/abi-vfs-telemetry.md .agents/261020-1750-cellbox-cli/command-catalog.md .agents/261020-1750-cellbox-cli/command-contracts.md
```

Observed command results for the candidate bytes:

| Candidate document | SHA-256 of whole UTF-8 file, including final newline |
|---|---|
| `spec.md` | `d318e63fae7da4e65ba581767f08c94cf6bb054e6cd3a4aff91169970f967c8a` |
| `abi-session-launch.md` | `422df04c1f39d41aa37f4975caba359be65c48bffc2a004e7fec0f6c26822b44` |
| `abi-vfs-telemetry.md` | `0edda7171a424619ac49ec2f2d7569a91e45490e9f77adf793bd1ca25d7cec50` |
| `command-catalog.md` | `6db8f1d1055f6160cbbcb38db0710fe59fb04b3cc1df93eb6ededf28a8cbf214` |
| `command-contracts.md` | `4ff1c3be46a39312d400877e0655a715b024563f2d84db7aecf27947efb2090d` |

This digest table lives **outside** all five hashed documents; none contains its own digest. There is no newline/Markdown normalization, executable filter, self-referential exclusion regex or hash of an approval label. All bytes are input. Changing any candidate bytes invalidates the corresponding digest and requires a new review identity. `contract-deltas.md` is the manifest/approval index, not one of its own hashed inputs. The command compatibility/right matrix and [rg/editor/SSH annex](command-contracts.md) are explicitly included. The user approved this candidate-design package; the five candidate files are unchanged by this correction. Other source anchors, dependency review and baseline are supporting evidence, not transitively frozen by these five digests. The full protected CP1 admission dossier still needs actual source/codec/allocation and accountable owner inputs. Hashes identify documents, not implementation verification, namespace allocation or CP2 approval.

## Frozen candidate semantics versus remaining owner inputs

| Delta / review identity | Concrete decisions now reviewable | Exact representation boundary | Remaining actual admission gate |
|---|---|---|---|
| **Engine v1** — `spec.md` §§1–4; no new protected transport claimed | Distinct Value/Rows/Frames/Bytes/Session; exact native types, field comparisons and typed JSON(L); no implicit bytes/JSON sniffing. Whole-pipeline type/stateful/background/redirect validation before effects. One JobId/cancel root, each child reaped once, final-stage status with explicit startup/denied/notfound/cancel/timeout status and stderr diagnostics. Bounded streaming and explicit finite sorting; machine rendering never acquires a keyboard. | Native Value and typed JSON envelope specified; existing shell parser remains controlling for legacy syntax. New fields/CLI semantics are design decisions, not claims about current parser behavior. | User candidate-design consent recorded; selected source owner/handoff and dependent protected CP1 dossiers remain incomplete. Runtime implementation acceptance is absent. |
| **CBX-D01 v1** — launch candidate §§2–4,7 | Single `/bin/cellbox` with `sh`, no argv-selected privilege union; authenticated caller/route/image ceiling plus separately attenuated service rights. Supervisor owns scoped lifecycle control; child prepare/bind/commit is all-or-nothing and generation-bound. Explicit argv/cwd/environment/stdin/out/err grant handoff. | Proposed LE ObjectRef/Principal/ChildRef, 48-byte context header, bounded section and 40-byte grant carrier layouts; candidate does not authorize reinterpretation of legacy ABI. Numeric selector/discriminant allocation and logical-enum codec annex remain unallocated. | User design consent recorded, but accountable loader/API/supervisor/net/config owners and file handoff are missing. Raw Shutdown dispatch bypass, missing net operation attenuation and missing config per-key grants remain provider/enforcement prerequisites. Exact owner allocation/encoding annex and full protected CP1 dossier are incomplete; no CP2. |
| **CBX-D02 v1** — launch candidate §§5–7 and terminal profile in `spec.md` | Existing kernel pipe owner enforces generational holder/lineage/operation grants; existing input service owns sessions, attach, foreground epoch and scoped mode restoration. No duplicate GUI authority. Last-authorized-writer EOF, distinct WouldBlock/TimedOut/Eof, partial progress, cancellation/revoke races, last-consumer subtree cancellation and unrelated-session isolation specified. | Typed ref/header/event record layouts proposed; session/pipe operation names are logical, not minted syscalls. Existing PipeRead/Share and input enum positions cannot be repurposed without explicitly governed migration. | User design consent recorded; accountable kernel/API/SDK/input handoff, finite charged profile, cross-owner publication/foreground locking and namespace allocation annex remain missing. Full protected P02 source-admission dossier incomplete; no CP2. |
| **CBX-D03-save v1** — VFS candidate §§1–3,6–7 | VFS-issued absent/present observation bound to mount/directory/name/file revisions. Explicit confirmation tied to observation; create-if-absent or replace-if-exact-version; delete/recreate/rename/ABA conflict. Private bounded staging, exact seal, compare+publish linearization, cancellation/death/query rules and truthful committed-visible/durable/proven-no-commit/indeterminate outcomes. | Exact logical fields/types and failure semantics; **no invented persistent-format bytes or VFS opcode**. Current bool write/sync cannot be reinterpreted as transaction results. | User design consent recorded; accountable VFS/API/backend handoff and admitted preserve-old-on-failure transaction provider remain missing. Native in-place replace/FAT remove-create are insufficient. Owner codec/allocation and any storage-format/durability migration dossier remain incomplete; no CP2. Unknown backend capability cannot be supplied by a documentation promise. |
| **CBX-D03-trust v1** — VFS candidate §4 | Existing VFS owner hosts narrow protected trust worker, not common-ELF ambient key/trust mutation. Exact host+port canonicalization/key matching, changed-key rejection, challenge+foreground confirmation binding, conditional whole-store concurrency, durable-only persistent acceptance and secret non-disclosure selected. | Logical version-1 bounded trust schema only. Byte codec/import/key-format and protected storage placement remain unspecified; design consent does not allocate them. | User candidate-design consent recorded; actual bootstrap/grant/storage issuer and accountable file-owner handoff remain missing. Dependency-selected key algorithms/formats, canonical persistent codec/migration and admitted durable backend still need the full protected admission dossier. No key/download/network execution permission or CP2 is inferred from design consent. |
| **CBX-D04 v1** — VFS candidate §§5–7 | Kernel scheduler/Cell registry is identity provider; exact Cell generation/task incarnation, snapshot/page scope/completeness/reset quality; explicit core-equivalent CPU convention (>100% allowed), no shared-heap double count, separate system allocator data. Bounded immutable paging, expiry and denied observation semantics. | Reviewable version-1 field/type schema, not guessed C struct/opcode. Existing ProcessInfoV2/MemInfo bytes cannot be reinterpreted by candidate approval. Clock-unit unavailable is explicit until owner mapping exists. | User design consent recorded; accountable kernel/API/accounting handoff, exact counter/monotonic mapping, provider continuity/task-incarnation producer, charged snapshot profile and canonical codec/allocation/observation policy remain missing. Full protected CP1 dossier incomplete; no CP2. Raw scheduler counts cannot be called nanoseconds. |

D03-save and D03-trust retain separate protected admission and CP2 scopes; neither scope's eventual handoff or implementation approval closes the other. User consent covers the published candidate design, not those missing dossiers. D01 and D02 share context refs/layout, so an admitted layout change requires updating both consumers/digests, not incompatible copies. A single source WIP slice is still required; the suggested initial D02 SDK/pipe/input slice is a proposal, not a registered/admitted owner handoff.

## Bounded capacity rationale — source versus candidate

The candidates intentionally distinguish **source constants** from proposed admission budgets. A source bound is not a measured safe reservation, and proposing a finite limit is not assigning it authority.

| Existing source evidence | Candidate consequence and refusal behavior |
|---|---|
| `kernel/src/task/pipe.rs:31-35,155-222`, `kernel/src/task/syscall.rs:5098-5290`: 4096-byte default, 64..65536 ring range, 512-byte syscall chunks, creator quota charge | Retain these byte-ring/chunk scales; no allocation-free unlimited pipeline claim. |
| `libs/ostd/src/args.rs:16-95`: 512-byte structured argv; `libs/api/src/abi/dir_handles.rs:32-60`: eight directory-carrier entries | Child context keeps 512-byte argv and at most eight typed grants; adds explicitly proposed 4 KiB cwd/environment and bounded prepare transfer, never a giant inline IPC frame. |
| `spec.md` §§1,4; D02 §5 | Proposed 16 stages, eight records/256 KiB per native edge, 4096 records/4 MiB aggregate sort; proposed 64 endpoint nodes/operation, 32 delegated children, 8 sessions/creator, 64 system, 256 input/64 control events. These are explicit safety ceilings awaiting charged owner-profile evidence, **not existing kernel limits**. Resize coalesces only named state; queue excess rejects rather than silently dropping input. |
| `cells/services/vfs/src/file_handles/table.rs:10-11`; `kernel/src/memory/cell_quota.rs:26`: 32 handles/caller and default 16 MiB heap | Proposed four save objects/caller count within that existing handle budget; 32 system saves, 4 MiB staged file, 512-byte chunks, finite receipts; quota before accept. Trust store 1024 entries/4 KiB key/1 MiB total is an explicit dependency-checked proposal, not known-host implementation evidence. |
| `libs/text-engine/src/top.rs:16-17`; `kernel/src/memory/cell_quota.rs:21-24`; `kernel/src/task/scheduler.rs:22`: 64-row old window; 64/4096 Cell slots; 32 threads/Cell | Keep 64-row pages, not 64-row silent inventory. Proposed 4096-row/1 MiB frame, one/caller and eight/system retained frames, five-second expiry; larger experimental populations report deterministic truncation/counts, not success pretending complete. |

Quota/ref-generation/unknown-version/unknown-flag failure is before publication. Overflow never wraps into a valid ref, capacity never overwrites a live lease, and counters never silently clamp/reset. All owner tables/temporary bytes are charged; exact measured reservation/profile remains the implementation admission dossier.

## Approval record and handoff contract

| Scope | Preparation integration | Accountable protected file owner / handoff | CP1 design approval | CP2 implementation approval |
|---|---|---|---|---|
| Engine semantics | Preparation session; candidate digest above | **Missing:** selected accountable engine source owner/file handoff and protected-dependency admissions | User candidate-design consent recorded; source-admission dossier incomplete | Pending/not eligible; no qualifying integration dossier |
| D01 launch/rights/context | Current assistant, bounded Phase02 integrating owner | User handoff “Giao slice Phase 02”; exact child-context/publication and narrow VFS cwd binder closure in [approved annex](phase-02-source-admission.md). Supervisor/net/config/admin lifecycle scopes remain unassigned | **Approved for this slice:** user selected “Duyệt CP1 và fixture riêng”; [exact authorization](phase-02-cp1-approval.md), annex SHA-256 `73bc7483ae6d05d6435c27f12a1c2890e3b550e63666b4339ed75f9b8371ae67` | Pending/not eligible |
| D02 stream/session | Same single integrating owner | Kernel/API/ostd/input/headless-shell exact closure; one kernel publication serialization owner, no GUI cutover or blanket lock release | **Approved:** exact codec/selectors, syscall262 and private local fixture execution per [authorization record](phase-02-cp1-approval.md); implementation evidence outstanding | Pending/not eligible |
| D03 save / trust (separate decisions) | Preparation session; candidate digest above | **Missing:** controlling P01/P05/P06 VFS/backend/storage/bootstrap/API handoff | User candidate-design consent recorded; provider/issuer/codec/allocation dossier incomplete for each scope | Pending/not eligible for either scope |
| D04 telemetry | Preparation session; candidate digest above | **Missing:** controlling P01/P02/P03/P05/P07 scheduler/accounting/API handoff | User candidate-design consent recorded; provider/clock/profile/codec/allocation dossier incomplete | Pending/not eligible |

A valid full source-admission handoff must name the accountable human/owner, accepted exact candidate digests plus assigned codec/allocation annex, repository-relative file set, adopted controlling slice/overlapping lock relations, source/consumer revision and selected single WIP slice. The user has now explicitly delegated the bounded Phase02 integration role above; the remaining exact dossier approval is not minted by that role. Actual candidate-design consent is preserved; namespace allocation, runtime acceptance and CP2 cannot be inferred from consent, “candidate”, SHA-256 or this table.

Phase02 exact CP1/source/private-fixture approval is now explicit: the user selected **“Duyệt CP1 và fixture riêng”** for [annex](phase-02-source-admission.md) SHA-256 `73bc7483ae6d05d6435c27f12a1c2890e3b550e63666b4339ed75f9b8371ae67`, bound [source snapshot](phase-02-source-snapshot.json) SHA-256 `2aaa62972b778ff84f3cdd2a9ea76f86ef75258fee265b21ae93184372027e3b`. [Authorization scope](phase-02-cp1-approval.md) supersedes proposal-time pending wording without rewriting the approved bytes. No runtime acceptance, rollout or CP2 follows. **Implementation status 2026-10-21:** D01/D02 Phase02 slice implemented and exercised on a private signed x86 image ([evidence](reports/phase-02-runtime-evidence.md)); the table's CP2 column is unchanged (pending/not eligible).

The approved candidate design and separately completed protected CP1/source/execution admission dossier are required before dependent protected edits. Source changes, builds, development signing and packaging subsequently occurred, but that fact does not retroactively complete the dossier. CP2 requires the actual diff, complete caller/generated/artifact census, boundary/race/denial evidence and disposable serial+GUI/service runtime captures. Those qualifying integration inputs and explicit CP2 approval are absent, so CP2 is pending/not eligible and final caller/image retirement or rollout is blocked. Old selector reservations remain frozen; no fallback that revives obsolete authority is permitted. Refuse or stage a migration rather than reinterpret old wire bytes.

## Controlling authority reconciliation

1. Cellbox user candidate-design consent and the subsequent explicit bounded Phase02 integrating-owner handoff are recorded. The [Phase02 dossier](phase-02-source-admission.md) still requires exact codec/source/fixture approval; rollout stays blocked. `blockedBy: []` does not release a scoped lock, and the new handoff does not release other owners or legacy scopes.
2. The controlling authority plan `../261011-1200-cell-contract-authority/plan.md:93-123` assigns kernel/loader/API and VFS/service adoption to phase owners with two checkpoints. Its historical completed rows at `:78-87` and `architecture-handoff-signoff.md:11-17` do not identify these new Cellbox digests.
3. The narrower `../261011-1200-cell-contract-authority/cluster-01-approval.md:4-5,19-35` preserves limited user consent for C03/C04/C05 while correcting unsupported complete CP1/CP2 claims and retaining all 38 locks. Cellbox D01–D04 are not those approved packages.
4. `../261011-1200-cell-contract-authority/abi-deltas.md:7-18,141-162` separates observed substrate, selected architecture, exact representations and missing authentic producer/profile/storage inputs. This candidate package supplies reachable Cellbox semantics; it does not fill those external authority/capability inputs by assertion.
5. `../cell-contract-0024-locks.md:52-78,113-152` preserves direct-phase precedence and all scoped relations. Adopt unfinished prerequisites through the controlling slice and explicitly hand off only overlapping files; never execute an old locked predecessor or mark all locks released.

**Reachable preparation result:** user-visible engine behavior, terminal grants/lifetime/error semantics, per-delta migration boundaries and VFS/telemetry semantic schemas have concrete candidate identities and actual user design consent. **Remaining gates:** accountable file-owner handoffs, namespace/codec allocation, real transactional/durable backend and bootstrap trust issuer, authoritative counter/time producer and finite-profile evidence, exact dependency/runtime qualification and separately admitted execution. The preparation-only stage performed no build/probe/signing/key/server/remote effect; later source/build/sign/package changes did occur and are historical development evidence, not qualifying integration proof. Phase 01 remains incomplete, P02–P09 incomplete/blocked, P10 blocked/unqualified, all CBX pending and CP2 pending/not eligible. See [the current withdrawal record](reports/rollout-containment.md).

## Preparation-time source inventory (historical baseline, not a current acceptance census)

### Terminal / input / session

- `cells/apps/terminal/src/session.rs:10-34` — `TerminalSession` is a local object (`session_id`, grid, `has_reader_lease`, input string, close flag); initialization simply sets `has_reader_lease = true`.
- `cells/apps/terminal/src/session.rs:39-142` — character-by-character local input and hard-coded command matcher/output (`help`, `uname`, `pwd`, `ls`, `ps`, `date`, `echo`, `exit`). No shell spawn, process attachment, pipes, terminal modes, or independent session authority.
- `cells/apps/terminal/src/main.rs:165-214` — terminal requests input focus and locally polls/renders events; it never launches or attaches to the shell.
- `cells/apps/terminal/src/grid.rs:57-131` — simple character grid handles CR/LF/tab/backspace and scrollback; no ANSI/VT parser or terminal-mode state.
- `libs/ostd/src/input.rs:35-81` — input SDK requests/releases focus through input-service IPC, trusting kernel-attested sender identity rather than supplied `cell_tid`.
- `cells/services/input/src/dispatcher.rs:3-17,30-40,71-78` and `cells/services/input/src/main.rs:577-604` — input service enforces one focused TID and sender-only clearing; it has no session/incarnation/foreground-job model.
- `cells/tools/shell/src/executor.rs:879-953` — existing shell foreground behavior drains input, releases focus, waits for child TID, then reacquires focus. This is the existing transition pattern to preserve/reconcile, but it is not an endpoint/session contract.

### Pipes and shell pipeline/input contracts

- `libs/api/src/abi/syscall.rs:192-219` — current frozen pipe syscall surface: create/read/write/close/share; pipe share takes target TID.
- `kernel/src/task/pipe.rs:1-27` — stated current primitives: task-table endpoint ownership, bounded backpressure, last-writer EOF, deadline waits.
- `kernel/src/task/pipe.rs:31-35,155-222` — capacities are default 4096 and clamped 64–65536; pipe capacity is charged to creator’s Cell quota.
- `kernel/src/task/pipe.rs:226-355` — endpoint counts determine EOF/broken-pipe; readers/writers wake each other.
- `kernel/src/task/pipe.rs:367-422` — task death closes its endpoints and wakes dependent waiters. This is task teardown, not operation-scoped delegated-subtree revocation.
- `kernel/src/task/syscall.rs:5098-5290` — pipe creation, bounded 512-byte syscall transfer chunks, timeout behavior, task handle ownership, and owner-only share dispatch.
- `kernel/src/task/syscall.rs:5294-5333` — `PipeShare` duplicates an endpoint directly into target task’s table; no child identity/generation or delegation chain is carried.
- `libs/ostd/src/syscall.rs:357-453` — SDK represents pipe endpoints as an opaque `usize`; read contract says `0` for either EOF or deadline.
- `cells/tools/shell/src/executor.rs:354-388` — shell pipelines capture each intermediate command to a `Vec<u8>` sequentially.
- `cells/tools/shell/src/shell_state.rs:89-137` — pipeline stdin is a global mutex-protected cloned `Vec<u8>`, rather than byte-stream endpoints.
- `cells/tools/shell/src/executor.rs:879-913` — external spawn only stages argv and selects a path route; it does not pass stdin/stdout/stderr or cwd/environment as a structured child handoff.

### Launch identity and rights

- `kernel/src/loader/launch_profile/mod.rs:1-91` — kernel enforces exact caller-name/route/path profiles; `SpawnFromElf` cannot borrow a non-empty privilege profile from advisory path bytes.
- `kernel/src/loader/launch_profile/profiles.rs:19-23,56-62,78-93` — `/bin/shell` and `/bin/terminal` exist in existing reviewed target/profile tables; shell uses a dedicated launch edge; tool-spawn has a separate edge.
- `kernel/src/loader/launch_profile/targets.rs:51-57` — `/bin/terminal` has an empty child ceiling, alongside other capability-free client paths.
- `kernel/src/loader/launch_profile/tests.rs:42-83` — existing tests enforce that shell path/ELF edges do not widen capability-free child ceilings and reject privileged target forgery.
- `cells/tools/shell/src/main.rs:7-54` — shell declares no ambient block/network capability and documents reviewed launch-edge enforcement.
- `cells/apps/desktop/src/apps.rs:19-25` and `scripts/package-desktop-x86.py:24-30` — present desktop caller and package/image entries for `/bin/terminal`. These are callers/artifact-list entries to include in the P01 census.

### Telemetry

- `libs/api/src/abi/syscall.rs:139-159` — `GetProcs2` and `MemInfo` are separate ABI calls; `MemInfo` intentionally exposes global allocator state only by opt-in.
- `libs/api/src/abi/syscall.rs:1427-1462` — `ProcessInfoV2` is task-oriented and has no Cell ID/generation; `ViMemInfoV1` reports global committed allocator frames.
- `cells/tools/shell/src/top.rs:50-54` — current shell top samples a bounded array of `ProcessInfoV2`.
- `libs/text-engine/src/top.rs:3-4,93-117,162-168` — reusable top core compares two task snapshots and computes CPU tenths; it does not establish Cell incarnation or memory attribution semantics.

### VFS create/replace/trust prerequisites

- `cells/services/vfs/src/caller.rs:16-29,68-77` — VFS principal is Cell ID + generation; a missing generation cannot own durable VFS state.
- `cells/services/vfs/src/access.rs:58-64,110-132` — VFS access policy enforces mutation authorization and special protected-path behavior.
- `cells/services/vfs/src/paths.rs:20-72` — general path write authorizes, charges, then invokes backend write; it returns only `Ok`, denied, quota, or I/O error.
- `cells/services/vfs/src/backend.rs:47-49,81-88` — backend write is create-or-truncate; no-replace rename is optional and backend-specific.
- `cells/services/vfs/src/backend_cellosfs.rs:140-168,196-215` — native CellosFS writes ensure/create then replace; separately supports an atomic no-replace rename after preflight.
- `cells/services/vfs/src/backend_fat.rs:317-356` — FAT write is remove-then-create, so it cannot be claimed as atomic replacement.
- `cells/services/vfs/src/dispatch_paths.rs:139-170` — current VFS `Rename` uses `rename_no_replace`; this is the closest current primitive, but it is not a destination-version-bound commit contract.
- `libs/api/src/services/vfs_file_handles.rs:1-10` — VFS file handles are service-local and never carried across spawn; this blocks assuming a kernel/inherited-handle solution without a new specified contract.
- SSH trust search found no outbound SSH or known-host implementation. `cells/services/sshd/src/kex.rs:30-38` instead seeds an inbound server host key deterministically; it is an explicit negative input for CBX-24/25, not reusable trust infrastructure.

## Architecture

The preparation-time baseline flow was:

`desktop catalog → /bin/terminal → local grid/session + input focus → fixed command matcher`

and separately:

`shell parser → sequential Vec capture pipeline / staged argv → reviewed kernel launch profile → child TID + focus release/wait`.

Kernel pipes are a third, independent mechanism:

`task pipe-handle table → bounded kernel ring → owner-only TID share → task teardown EOF/wake`.

P02’s intended session/stream design must connect these three paths under **one named owner and one authority model**; it must not layer a second GUI-local “lease” model over focus TIDs and pipe ownership. VFS and telemetry are independent service/kernel contracts whose required Cellbox enhancements must each be separately classified and gated.

## Reconciled delta index (2026-10-25)

Prepared for [Phase 01 closure dossier](phase-01-closure-dossier.md) gap C2-b. This index records state only: it approves nothing and does not alter the 2026-10-20 bytes above. Where it disagrees with the "Approval record and handoff contract" table above (CP2 shown as "Pending/not eligible", owners shown as "Missing"), this index states the current record. That table stays as the preparation-time snapshot. Per-delta scope and evidence detail is in closure dossier §2.1.

**Owner.** The assistant is the integrating owner and the user the approver. Explicit handoffs were recorded for Phase 02 ("Giao slice Phase 02"), Phase 06 (D60-6) and Phase 07 (D70-4). **On 2026-10-25 the user retroactively confirmed the assistant as integrating owner for the Phase 03, 04, 05, 08 and 09 file sets under the recorded CP1s.** No other human owner is named.

### Current digests of the five candidate documents

Command: `sha256sum` as above, run 2026-10-25.

| Document | Frozen (2026-10-20) | Current (2026-10-25) | Change record |
|---|---|---|---|
| `spec.md` | `d318e63f…7c8a` | `cde241787ca4ab01e75bb057a5af0ca94dd50755816e1bfc2a71b1ab6dd3906a` | §4 LF amendment (2026-10-23, ratified in [P05 CP2](phase-05-cp2-dossier.md) §13); §1/§2 D63 (P06 CP2 2026-10-24) |
| `abi-session-launch.md` | `422df04c…2b44` | `422df04c1f39d41aa37f4975caba359be65c48bffc2a004e7fec0f6c26822b44` | unchanged |
| `abi-vfs-telemetry.md` | `0edda717…ec50` | `0edda7171a424619ac49ec2f2d7569a91e45490e9f77adf793bd1ca25d7cec50` | unchanged |
| `command-catalog.md` | `6db8f1d1…f214` | `f2a82fce61101f9f1dd6d62a75bece735e3eb9129a2295b187c2f49209485339` | S2-lite ledger table and P04/P06 rows (CP2-approved slices); 2026-10-25 C1-a..d revision (doc-only). **The user confirmed this digest as the frozen catalog on 2026-10-25.** Last earlier recorded identity: `6f6766b9…` ([P06 CP2](phase-06-cp2-dossier.md):113) |
| `command-contracts.md` | `4ff1c3be…090d` | `4ff1c3be46a39312d400877e0655a715b024563f2d84db7aecf27947efb2090d` | unchanged; P07–P09 deviations are recorded in the phase files |

**2026-10-26 catalog revision.** `command-catalog.md` is now SHA-256 `2a4c3e6652c90b5dad840c5a403276a3a8fa4236b8bab12ca1b319bc237cbd70` (was `f2a82fce…9339`). Phase 10 slice S10-0 changed §1 and §3.1, added §3.2 (the one-shot table, host-tested against `oneshot::TABLE`) and the revision log entry. **The user acknowledged it with the S10-0 CP2 on 2026-10-26; it is the current frozen catalog.**

**2026-10-29 catalog revision (S10-5).** `command-catalog.md` is now SHA-256 `f06f38ce7447758f7b49e58d6e98dc8309b583515bf1ffd3608e665011a5fd0f` (was `2a4c3e66…cbd70`). Phase 10 slice S10-5 changed only §6 ("As built" and "Source changes since preparation") and added a revision-log entry; the ledger block, §3 and the command contracts are unchanged. **The user acknowledged this digest with the S10-5 CP2 on 2026-10-29; it is the current frozen catalog.** CP2 also accepted S10-5.13 deviations 1–8 ([phase-10-s0-proposal.md](phase-10-s0-proposal.md)).

**2026-10-26 spec revision (S10-3a).** `spec.md` is now SHA-256 `f9c550f7a75ce4d79ac0aa6940dd21b529ba1232d0a7de081f2b105e4eba2988` (was `cde24178…3906a`). One paragraph was added after the terminal-grammar line, the Control+j accept-line rule for canonical shell line readers; the decoding grammar itself is unchanged. This is the spec sentence of the S10-3a CP1 (D3a-1 A, approved by the user on 2026-10-26).

**2026-10-26 supersession (user decision (e)).** The S10-3a Control+j accept-line rule **supersedes Phase 02 amendment A2 item 1** ([phase-02-limitations-amendment.md](phase-02-limitations-amendment.md): UART Ctrl-J deliberately not Enter) for the shell's canonical line readers. The decoder grammar and A2 items 2–5 are unchanged. The P02 check `ctrl_j_not_enter` is replaced by `ctrl_j_submits`, and the rerun PASSes ([Phase 02 log](phase-02-terminal-sdk-streams.md), 2026-10-26).

**2026-10-28 Phase 10 CP2 record (user, relayed by Main).** S10-3a, S10-3b, S10-3c, S10-3d and S10-3 are approved, including their recorded deviations/disclosures. S10-3a ratifies the `f9c550f7…2988` Control+j accept-line amendment and supersession (e), without changing the terminal decoder grammar. S10-3b covers interruptible waits/Collected sinks and `cat` file errors; S10-3c/d are internal timeout/bootstrap/CQ-progress/harness changes, with no new protected wire/namespace/right allocation. Their exact implementation, controlled-probe and full-battery evidence is [S10-3d.7/S10-3.14](phase-10-s0-proposal.md); qualified candidate `4fae8b20ba283b38688028b14008eb0abcf4104ca929a5a80be354675ad32821`. SC6 corrected an obsolete audit-script pin using the already-approved S10-3b script; both audit verdicts remain, and no document content/new hash approval was fabricated. CP2 does **not** promote canonical artifacts or lift containment; S10-4/S10-5, SSH partial gaps, physical/VMX/production and incidental DMA/DSTRD issues remain separate.

### Delta index

"Approval docs" names the binding file, with its SHA-256 where one is recorded. Every CP1 and CP2 below is a user decision recorded in the cited file.

| Delta | Approval docs / digest | Owner | CP1 | CP2 |
|---|---|---|---|---|
| Engine v1 (spec §1–§4) | `spec.md` current digest above | assistant (user-confirmed 2026-10-25 for P03) | design consent 2026-10-20; P03 2026-10-21; S10-3a 2026-10-26 | P03 2026-10-21; amendments P05 2026-10-23/P06 2026-10-24; **S10-3a Control+j amendment/supersession (e) CP2 2026-10-28** |
| D01 child context / cwd (P02) | annex `73bc7483…ae67`, snapshot `2aaa6297…7e3b` ([CP1 record](phase-02-cp1-approval.md)) | assistant, handoff "Giao slice Phase 02" | 2026-10-21 | 2026-10-21 baseline ([dossier](phase-02-cp2-dossier.md)); phase closure 2026-10-25 |
| D02 stream/session (P02) | same annex | same | 2026-10-21 | 2026-10-21 / 2026-10-25 |
| P03 A3 external byte stages / A4 `QueryChild` | [A3](phase-03-external-bytes-amendment.md), [A4](phase-03-child-status-amendment.md) (no SHA) | assistant (user-confirmed 2026-10-25) | 2026-10-21 | 2026-10-21 |
| P04 S1 / S2-lite / S2c-min / S3 launch edge | [mini-dossier](phase-04-s1-s3-cp2-dossier.md) (whole-file hashes, §4; pre-amendment SHA-256 `8e84ead7…cd1f`) | assistant (user-confirmed 2026-10-25) | 2026-10-21 | **2026-10-25** (user), including deviations 1–7 |
| P04 S3-output, S3-gui, OOM, S2-K, S2-N, S2-T/W, S4a, C5 | their CP2 dossiers (OOM `588fff65…`, S2-K `4447cc50…`, S2-N `5a7fa43d…`, S2-T/W `1ea82014…`, S4a `901ae31a…`, C5 `131ec5a5…`) | assistant (user-confirmed 2026-10-25) | 2026-10-22 | 2026-10-22 |
| P05 S5-1 … S5-4, S5-3c (selectors 23–28) | [P05 CP2 dossier](phase-05-cp2-dossier.md) §16; S5-3c annex `41e7f0b1…dea6` | assistant (user-confirmed 2026-10-25) | 2026-10-22/23 (S5-3/S5-4 retroactive) | 2026-10-23 |
| D04 telemetry + P06 S6-1..S6-5 | [P06 CP2 dossier](phase-06-cp2-dossier.md) (entry `01abc820…`, `da65dde9…`, `85159a2f…`, CP1 `88602bb0…0fce`) | assistant, handoff D60-6 | 2026-10-23 | 2026-10-24 (fixes U/Z/V and `$(…)` ratified at CP2 without CP1) |
| P07 S7-0..S7-4 (vigrep, VFS read path, `rg`) | decision IDs D71–D77, plus a hash addendum in [phase-07](phase-07-ripgrep-search.md) (2026-10-25, post-approval state) | assistant, handoff D70-4 | 2026-10-24 | 2026-10-24 |
| D03-save (P08) | D78-1..3 + D03 §3.1, plus a hash addendum in [phase-08](phase-08-compact-editor.md) | assistant (user-confirmed 2026-10-25) | 2026-10-24 | S8-1..S8-4 2026-10-24; S8-5 2026-10-25 |
| D03-trust, launch/entropy/completion (P09) | D79-1..15, plus a hash addendum in [phase-09](phase-09-ssh-client.md) | assistant (user-confirmed 2026-10-25) | 2026-10-25 | S9-1..S9-4c, S9-6 2026-10-25; S9-5 deferred (D79-14) |
| Q Config per-key write right (`export`) | [phase-01-config-right-cp1.md](phase-01-config-right-cp1.md); CP1 text `26d5b6a6…83b4`, pre-CP2-amendment `72895bc8…115b8f` (the file carries its own status line) | assistant; implemented by P01DenyConfig | **2026-10-25** (user): option Q1, a kernel `config_write` right, with every recommendation D-Q2..D-Q7 | **2026-10-26** (user), including deviations 1 (`/bin/config` only with feature `ai`; without it `export` reports `config unavailable`, status 1) and 2 (oversized `List` reply → `0xFF`; unused `Mutex` removed). Evidence: P04 466/466 (`/tmp/cellbox-phase04-t8uH`, 6 config markers), GUI 290, P03 67, P02 48, kernel lane with 7/7 VFS self-tests, P09 103; host kernel 284, cellbox 189, service-config 7, api 153; canonical hashes unchanged |
| S10-0 `/bin/cellbox <command>` one-shot + in-shell explicit form (no kernel, edge or allowlist change) | [phase-10-s0-proposal.md](phase-10-s0-proposal.md) §2 S10-0; catalog `2a4c3e66…cbd70` | assistant | **2026-10-26** (user): DS0-1..DS0-6 as recommended | **2026-10-26** (user), including deviations 1–7 (S10-0.11). Evidence: P04 494/494, GUI 290, P02 48, kernel lane pass, P03 67, P09 103, host 205 |
| S10-3a lone-LF accept-line / fleet-profile integration | [S0](phase-10-s0-proposal.md) S10-3a.8; spec `f9c550f7…2988`; no decoder/ABI change | assistant | **2026-10-26** D3a-1..4 | **2026-10-28** user, recorded deviations and supersession (e) |
| S10-3b Ctrl-C waits/Collected sinks / `cat` VFS bytes-errors | [S0](phase-10-s0-proposal.md) S10-3b.8; cellbox-only behavior, no new authority allocation | assistant | **2026-10-26** D3b-1 B, D3b-2 A, D3b-3 A | **2026-10-28** user, deviations 1–4 |
| S10-3c elapsed-time NVMe/init waits / H1–H3 | [S0](phase-10-s0-proposal.md) S10-3c.11; existing GetTime and fail-closed saves, no new wire/right | assistant | **2026-10-27** D3c-1 A, D3c-2 B, D3c-3 A, D3c-4 A, D3c-5 | **2026-10-28** user, recorded deviations including retained SC5 history |
| S10-3d bootstrap scan lifetime / bounded stale-CQ progression | [S0](phase-10-s0-proposal.md) S10-3d.7; existing process table/clock, no kernel-main/ABI/policy change | assistant | **2026-10-28** D3d-1 A, D3d-2/3/4 | **2026-10-28** user, deviations/disclosures 1–6 including SC6 correction |
| S10-3 matched signed dev-posture x86 candidate qualification | [S0](phase-10-s0-proposal.md) S10-3.14; candidate `4fae8b20…2821`, full generated evidence `reports/s10-3d-evidence/` | assistant | **2026-10-26** D3-1..7 plus recorded follow-on decisions | **2026-10-28** user, L0/L4 6/6, complete lanes/L6/audit and disclosed existing limits; **candidate-only, not promotion** |
| D01 lifecycle owner (`KillTarget`/`ResetSystem`/`SnapshotTarget`/`ReplaceService`) | — | — | none; effects refused (S2c-min Option B) | — |
| D04 clock units (`uptime`/`date`) | — | — | none; open defect D60-3 | — |

**Recorded exceptions (no action).** Edits made before CP1 on 2026-10-20 were withdrawn or quarantined ([rollout-containment.md](reports/rollout-containment.md)). The CP1 for P05 S5-3/S5-4 is retroactive. The P06 fixes landed outside CP1 and were ratified at CP2. Update 2026-10-29: the S10-5 CP2 (user, 2026-10-29) accepted the promotion of the qualified dev-posture desktop bytes to the canonical x86 paths, so canonical packaging and Phase 04 criterion 4 are no longer blocked at that ceiling; Phase 10 is closed-partial (CBX-27 unmet, CBX-23/24/25 partial), and physical/VMX/production/remote gates, cloud formats and non-Intel recipes remain blocked/refused.