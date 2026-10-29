# Phase02 source-admission review — not runtime acceptance

## Decision and exact subject

**Ready to request exact CP1 approval; not approved, implemented or accepted.** Both independent read-only reviews found no remaining blocking issue in their assigned scopes after the dossier corrections below. The user has accepted only the bounded integrating-owner handoff, which explicitly retained exact annex approval before protected edits.

- [Admission annex](../phase-02-source-admission.md), final SHA-256: `73bc7483ae6d05d6435c27f12a1c2890e3b550e63666b4339ed75f9b8371ae67`.
- [Source snapshot](../phase-02-source-snapshot.json), SHA-256: `2aaa62972b778ff84f3cdd2a9ea76f86ef75258fee265b21ae93184372027e3b`.
- Source HEAD: `fabd6ab7e3f2d9ea90a931fe53b7d43e509888dd`; 78 exact declared paths, 63 existing dirty-byte identities and 15 proposed new files. No clean-worktree claim.
- The five candidate document hashes in the annex remain unchanged. The final annex adds checked source provenance after the protocol snapshot reviewed by both reviewers; it changes no reviewed selector/authority behavior.

`Phase02CodecCheck` reviewed codec/bootstrap/context/control; `Phase02RaceCheck` reviewed authority/race/lifecycle. Both final verdicts refer to annex snapshot `#D892`. Earlier reviewer-specialist attempts failed at the provider and produced no review; they are not counted as independent passes. Read-only generic review agents supplied the actual verdicts.

## Accepted findings and dossier corrections

These were **proposal defects**, not evidence of runtime bugs fixed. Each row is accepted and corrected in the proposed approval subject; implementation and tests remain outstanding.

| ID | Finding | Exact correction now in the annex |
|---|---|---|
| F01 | Input could not bootstrap foreign-held stream peers or an initial reader lease | Kernel selectors21/22 create a controller-owned, input-incarnation-pinned bootstrap; service verifies kernel-recorded actor/operation; epoch1 baseline lease; scoped claim/activate/abort |
| F02 | Nonzero request identity/retained outcomes missing | 24-byte headers, UnknownRequest25, exact sender/operation/issuer binding, bounded retained outcomes and ACK/high-water fence; no effect replay |
| F03 | New child cwd grant was minted after immutable context completion | VFS reserve from attested prepared identity → final context naming child grant → digest-bound finalize; unusable before publication and sealed before first access |
| F04 | Context kind/slot and result-state numbers remained unspecified | Explicit kinds1..5, slots0..7, supported combinations, owner mapping and numeric operation/receipt/endpoint/child states; future delta kinds unsupported |
| F05 | Meta/lock modifiers and frontend kinds contradicted frozen input namespace | Only Shift/Control/Alt; unsupported keys rejected; candidate Serial/Gui/Ssh identities preserved; common frozen byte codec retained |
| F06 | Named fixture launch edges could not dispatch within listed files | Include launch_profile/mod.rs and exact feature-gated shell→parent, parent→child, child→child path rows without ordinary-profile broadening |
| F07 | Retained raw stdin readers could consume a child's new input | Distinct child-epoch rings; projected exclusive child-reader set; old rings receive no new-epoch bytes |
| F08 | Input had no admitted proof of ordinary child death | Issuer/receipt-scoped QueryReceiptChild24, canonical terminal sequence/reason and source-owner retirement; no arbitrary process observer |
| F09 | Infinite PollEvents conflicted with hard IPC expiry and could lose events on rejected reply | At most100-tick RPC waits under existing3000-tick expiry, saved exact token, retained cursor/request outcome until ACK; no speculative consuming retry |
| F10 | Terminal receipts had no safe acknowledgement/reclamation transition | ResolveBindReceipt materialization ACK, terminal restoration before reclaim, bounded retained records and nonwrapping replay fence; query non-consuming |
| F11 | Delegation bypassed prepared-child publication | DelegateEndpoint4 targets only an attested prepared descendant; install solely in CommitChild |
| F12 | Closing the preparer's sole reader before commit triggered final-consumer cancellation | Atomic child-reader installation/source-reader consumption under publication lock; projected holder-set validation; no premature-close exception |
| F13 | Frontend lacked clean stdin half-close | EndInput21 drains admitted input then closes only that epoch's sole moved producer; one Eof control, outputs/session preserved; stale/foreign denied |
| F14 | Control endpoint framing/routing and stale baseline contents were unspecified | Exclusive EventHeader+payload control stream, incremental decode/ACK, no duplicate Poll controls; scoped RestoreSessionInput29 clears old baseline/control before resumed routing |

F01/F07/F08/F09/F10/F12 authority/lifecycle issues block source admission if their corrections are removed. Rollback before CP1 is withdrawal of this unapproved annex without any executable effect. After implementation, rollback must use the slice-only source/private-VM procedure in the annex; it cannot unsend delivered IPC or undo guest-disk writes. No user/canonical disk or remote effect is authorized.

## Exercised checks

A read-only in-memory Python check completed successfully:

- All five immutable candidate SHA-256 identities match the existing manifest.
- Kernel local selectors1..29 and input local selectors1..22 are unique and contiguous.
- The source-snapshot digest and declared78/63/15 counts match.
- 179 relative file links in the annex and four updated plan/ledger documents resolve; heading anchors were not checked.
- All three quarantined artifact SHA-256 identities match [rollout-containment.md](rollout-containment.md).
- Phase02 remains blocked; no CBX row was closed.

Arithmetic checked by independent review: BootstrapSession success136 bytes including24-byte header; ResolveBootstrap success192 bytes including header; input seal192 bytes; VFS seal128 bytes; borrowed4000-byte inner service records fit the attested4096-byte IPC buffer envelope.

**Not run:** test/build baseline, source compilation, signing, image assembly, QEMU, real child/session scenarios. This proposal therefore provides zero new CBX runtime evidence. Existing historical host tests/containment tests are not relabeled as Phase02 acceptance.

## Required next gate

Explicitly approve the exact annex digest, the narrow VFS binder extension and private local fixture build/sign/image/boot admission—or revise/decline them. Approval records the single controlling slice and only its affected file relations; it releases no legacy plan wholesale. Then capture Build-entry test baseline, implement serially through the integrating owner, exercise actual private signed-x86 child/descendant/two-session scenarios and collect CBX-03..05 evidence. CP2, canonical rollout and Phase03..10 stay blocked.
