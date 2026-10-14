# G2-J separate proposal review — 2026-10-14

Independent read-only reviewers: `JournalSafetyReview` (crash/root/reservation safety) and `JournalScopeReview` (model/finite layout/authorization). Both inspected proposal/source and ran no tests. Their original r1 findings are preserved below; no independent runtime PASS is inferred.

| Finding | Evidence / risk | Disposition |
|---|---|---|
| High: legitimate one-root state omitted | Original model required two roots; `engine.rs:44-53` creates A with empty B, and torn target may leave active only. | Proposal r2 tracks physical slots vs valid roots; model runs bootstrap and torn-target one-root transitions. Omitting a valid old root remains rejecting control. |
| High: cut rows were unverified oracle | Original script emitted six expected rows without transition validation. | v2 validates14cut states and rejects failed-FS1 successor promotion, ambiguous resume and ambiguous reclamation. Historical r1 output is not promoted. |
| High: outstanding reservation/internal aliases unchecked | Roots-only intersection cannot protect unpublished preparations or duplicate candidate assignments. | Added prior-reservation and allocation-event checks plus two rejecting controls. |
| High: VFS Unknown scope lacked real owner/codec | Success-only quota and namespace reservation Drop cannot retain failure charge; `namespace.rs`/`quota.rs` missing from original footprint. | VFS/Unknown transfer removed entirely from limited grant. Complete lifecycle/namespace/quota migration remains separately blocked. |

Before implementation the minimal safe scope is a default-off direct-library filesystem experiment, not the journal service. The [authorization](g2-j-cow-fault-authorization.md) pins exact paths, source baseline, fresh disposable images, unchanged V1/public/default behavior, storage bounds and actual sector/barrier evidence. Fresh read-only recovery after fault does not authorize mutation resume.

Integration-owner adjudication accepts all four merged findings. Rollback is withholding old model/overbroad VFS scope from consent; no durable runtime effect occurred. Full G2-J remains unapproved on semantic codecs, charge owner transfer, admitted backend/floor/issuer and persistent CP1/CP2. Old-image rollback and media corruption are not solved by checksum/sequence or this grant.

Observed corrected model command: `python3 scripts/verify-g2-j-finite-layout.py --out build/g2-j-reviewed-finite-layout-certificate.json`, exit0,14transitions/11controls. This is finite software-model evidence, not actual engine fault execution. Corrected independent re-review dispositions and pinned manifest integrity are recorded in validation when received.

Final corrected dispositions: both independent reviewers report no blocker specific to the limited development grant. JournalSafetyReview accepted corrected one-root/reservation logic and the landed authorization/current v2 receipt; JournalScopeReview accepted private module/existing signatures, default-off feature and VFS feature-unification exclusion. Earlier re-review snapshots reported missing artifacts/stale narrative while those artifacts were being integrated; these were resolved before final disposition, not ignored. No independent runtime tests were run.
