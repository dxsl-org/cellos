# Standalone Proposal Review: OGJ1 Codec and Unknown/Lifecycle Protocol

**Historical Review Date:** 2026-10-14; disposition correction recorded 2026-10-15.
**Target Proposal:** [ogj1-codec-unknown-lifecycle-design.md](ogj1-codec-unknown-lifecycle-design.md)
**Verification Harness:** [scripts/verify-ogj1-codec.py](../../scripts/verify-ogj1-codec.py)
**Historical Verification Receipt:** [build/ogj1-codec-verification.json](../../build/ogj1-codec-verification.json)
**Review Type:** Correction of an author-written standalone proposal review; not independent approval or a maintainer decision.
**Review Disposition:** **SUPERSEDED — NOT APPROVED; G2/OGJ1 lifecycle qualification and G3 admission BLOCKED.** The historical 18/18 result did not establish the advertised prerequisites. Parent-observed empty and corrupt-payload-only storage both recovered as `WRITABLE`; see [historical counterexamples](../../build/g3-prerequisite-counterexamples.json) and [independent upstream audit §9](g3-lifecycle-authority-gates.md#9-independent-upstream-audit--2026-10-15). Subsequent [model regressions](../../build/ogj1-corrected-model-verification.json) and [direct smoke](../../build/ogj1-corrected-model-smoke.json) correct bounded model behavior only, not authority, native durability, durable charge recovery or checkpoint implementation.

---

## 1. Scope and Evidence Ceiling

The standalone Python model covers an OGJ1 envelope and simulated transaction/recovery logic. Its historical receipt is software-model evidence only: no real issuer authorization, initialized genesis, admitted storage/floor, durable charge-vector ownership, native D1/D2 persistence, checkpoint recovery or execution proof follows. A digest can establish integrity/chain linkage, not issuer authentication or successful execution.

This documentation correction grants no public wire opcode/ABI delta, VFS/kernel/runtime integration, persistent-byte cutover, KMS/protected persistence change, synthetic signer or backend admission. Ten pending phases, 38 legacy locks and all 23 downstream CP1/CP2 pairs remain unchanged. Runtime and authority gates remain closed.

The controlling upstream [G2 candidate §5](g2-ordinary-persistence-candidate.md#5-bounded-journal-and-filesystem-prerequisites--proposed) and [G3 gates §§5,9](g3-lifecycle-authority-gates.md#5-unknown-ownership-and-journal-evidence-gate) require more than passing codec fixtures. These documents are proposals/evidence requests, not owner assignments or approval records.

## 2. Historical 18-Case Receipt — Not a Qualification Verdict

The retained JSON receipt records `status: PASS`, `total_tests: 18`, `passed: 18`, `failed: 0`. That is an honest description of the historical script's own assertions, **not** confirmation that all governing invariants held. The earlier review's universal PASS narrative and recommendation to accept the specification are withdrawn.

The historical review recorded this command:

```bash
python3 scripts/verify-ogj1-codec.py --out build/ogj1-codec-verification.json
```

No test, build, lint, formatter, model execution or counterexample rerun was performed for this documentation correction. The old receipt and original test names below are preserved as history, not corrected-run evidence.

| Historical cases | Recorded result | Evidence ceiling / independent audit disposition |
|---|---|---|
| TC01 `roundtrip_all_kinds` | All four kinds encoded/decoded. | Generic Payload/Commit/Checkpoint/Switch roundtrip only; no checkpoint selection, complete snapshot, switch recovery, compaction or retirement qualification. |
| TC02–TC04 `reject_invalid_magic`, `reject_unsupported_version`, `reject_nonzero_reserved` | Corrupt magic, version 2 and nonzero reserved rejected. | Narrow envelope controls; no initialized ledger, issuer or authority proof. |
| TC05–TC07 `reject_tampered_body`, `reject_tampered_header`, `reject_truncated_record` | Body/header tampering and truncation rejected by codec. | Does not prove recovery rejects every corrupt present record; corrupt-payload-only recovery later returned `WRITABLE`. Hashes are recomputable and do not authenticate issuers. |
| TC08 `reject_oversized_payload` | Canonical Payload above 256 bytes rejected. | Experiment codec bound only. No complete lifecycle snapshot-fit or finite filesystem/profile certificate. |
| TC09 `reject_invalid_commit_body_len` | Commit body not 40 bytes rejected. | Body-length control only; no complete cross-record binding qualification. |
| TC10 `successful_double_barrier` | Simulated D1 then D2 advanced/confirmed revision. | No native barrier/failure-tuple evidence. Physical durable commit is possible despite lost D2 acknowledgement; observed D2 is required for the original reply, not the definition of physical commit. |
| TC11 `d1_failure_unknown_charge_retained` | Simulated D1 failure entered Unknown with local charge retained. | Exercises D1 failure, not D2 failure or post-D2 reply loss. Process-local charge retention does not survive owner death. |
| TC12 `refuse_mutation_during_unknown` | Conflicting mutation refused while Unknown active. | Model gate only, not durable owner handoff or filesystem admission. |
| TC13 `post_d1_pre_d2_crash_charge_retained` | Simulated pre-D2 crash left an orphan Payload with charge retained. | Pre-D2 crash only; neither D2 failure nor post-D2 reply loss was exercised by TC11/TC13. No authenticated durable adoption/reconstruction. |
| TC14 `recover_contiguous_chain` | Contiguous fixtures recovered `WRITABLE`. | Historical model had no required initialized-context input; this is not admitted genesis/checkpoint/floor recovery evidence. |
| TC15 `recover_orphan_payload_unknown` | Orphan fixture recovered Unknown without deletion. | Correct proposed distinction is **valid** Payload with **absent** Commit; a corrupt present record must not be downgraded to an orphan. |
| TC16 `recover_torn_commit_unknown` | Torn Commit yielded Unknown. | Historical assertion encoded the wrong rule. Present torn/corrupt Commit requires `RecoveryFailed`, not treatment as absent or a passing Unknown result. |
| TC17 `recover_revision_gap_fails` | Fixture yielded `RECOVERY_FAILED`. | Independent audit found the fixture also had a mismatched referenced Payload digest; that result alone does not isolate revision-gap enforcement. |
| TC18 `recover_fence_regression_fails` | Fixture yielded `RECOVERY_FAILED`. | A boot-floor comparison alone does not qualify fence nonregression across a chain or an admitted/protected floor. |

No inode-inline-fit, storage admission, durable lifecycle ownership or independent approval conclusion is supported by these 18 cases.

## 3. Recorded Counterexamples and Independent Audit

The parent recorded the following historical observations in [build/g3-prerequisite-counterexamples.json](../../build/g3-prerequisite-counterexamples.json); they were not rerun here:

| Scenario | Observed historical state | Required disposition |
|---|---|---|
| `recover(ledger_id=b'A'*32, boot_floor=100, on_disk_slots={})` | `WRITABLE` | Without explicit initialized context, fail closed; empty storage is not authorized genesis. |
| `recover(ledger_id=b'A'*32, boot_floor=100, on_disk_slots={0: {'payload': b'corrupt'}})` | `WRITABLE` | A present corrupt Payload must yield `RecoveryFailed`, never be silently omitted. |

The read-only independent reviewer `OGJGateAudit` ran no runtime tests and did not approve the proposal. Its [G3 audit §9](g3-lifecycle-authority-gates.md#9-independent-upstream-audit--2026-10-15) blocks GA-5 and requires:

1. Explicit initialized genesis/checkpoint identity and full recovery binding checks: ledger, kinds/slots, same transaction/revision/fence/predecessor, referenced length/digest, contiguous revisions and chain fence nonregression. A model-only initialized context may exercise these rules, but is not authentic genesis, authority or floor evidence.
2. Present torn/corrupt/foreign records → `RecoveryFailed`; a valid orphan Payload with absent Commit → retained `Unknown(same T)`. Missing required initialized state may not become genesis or an older plausible balance.
3. Exact-byte resolution of the reference-digest ambiguity: model references use the validated domain/header/body trailer, while upstream says “referenced entire-record SHA-256.” The corrected design **proposes** the trailer convention; upstream ambiguity remains blocked for explicit owner/maintainer review. No persistent bytes are approved or silently migrated.
4. Correct finite storage geometry: directory payload428 / entry56 means seven entries, not eight directory blocks per inode. Indexed inode capacity is 32 × 8 = 256. Journal subtree21 inodes excludes surrounding parents and other volume users. Maximum record436 exceeds inline428; canonical bodies249..256 require extents, while bodies up to248 fit inline. No complete capacity/profile claim follows.
5. Canonical durable prospective-charge ownership and authenticated fenced adoption/recovery across owner death. Model dictionaries do not reconstruct charge vectors or transfer ownership; a supervisor cannot automatically assume them. These remain unmet, unimplemented runtime prerequisites.
6. Complete checkpoint schemas, independently finite snapshot capacity and real durable switch/replay-retirement witnesses preserving all Unknown/outcome/closed-key obligations and prior acknowledged roots. A 256-byte codec bound does not establish full snapshot fit.
7. Isolated negative controls and honest branch coverage. Historical TC11/TC13 do not cover D2 failure or post-D2 reply loss; TC17 did not isolate a gap.

## 4. Corrected Proposal Semantics — No Execution Claim

The design now separates observed D2 acknowledgement from physical commit: D1 alone acknowledges nothing; only observed successful D2 permits the original confirmed-state advancement and success reply. If a durable Commit exists but the D2 acknowledgement/reply is lost, the operation remains `Unknown(same T)` until authoritative lookup reports the existing committed result; no re-execution, fresh identity or duplicate credit is allowed.

Recovery must be explicit and non-formatting. The standalone model must require a model-only initialized recovery context rather than default genesis, reject corrupt present records, and validate the whole chain/bindings. Such fixtures still do not authenticate issuer/genesis/floor or authorize production mutation. Real recovery must reconstruct charged retained obligations under qualified ownership before reopening mutation.

The corrected model keeps mutation closed even after valid replay: `RECOVERED_MUTATION_CLOSED` for a valid initialized chain/base, `UNKNOWN` for valid orphan ambiguity and `RECOVERY_FAILED` for invalid/uninitialized input, with `mutation_admission=False`, `charge_recovery=UNIMPLEMENTED` and `accounting_recovered=False`. Parent execution receipts linked above exercise these model outcomes, not durable accounting.

Durable charge adoption, lifecycle-vector codec, actual owner transfer, checkpoint selection/compaction and replay-safe retirement remain blocked and unimplemented. No “automatic supervisor” assumption, digest-based execution proof, complete 256-byte snapshot claim or native persistence claim survives this correction.

This section records corrected proposal semantics. Subsequent parent-produced model regression receipts and direct smoke linked above are separate model execution evidence. Neither those runs, historical18 nor this text supplies an owner/maintainer approval or G2/G3 qualification.

## 5. Review Disposition and Remaining Gates

**Do not accept or promote this package on the historical receipt.** Preserve both historical receipts and counterexamples; retain the failed/contradictory evidence denominator.

Future qualification requires corrected scoped model receipts with isolated regressions, independent review, exact reference-byte/schema decisions, actual admitted initialization/issuer/floor/storage facts, durable lifecycle ownership/reconstruction, finite complete checkpoint/capacity/retirement design, real failure receipts and explicit owner/maintainer checkpoint decisions. Model corrections alone cannot discharge those gates.

This documentation is review preparation only. It grants no runtime/persistent implementation handoff, independent approval, CP1/CP2 promotion, G3 admission or lock release.
