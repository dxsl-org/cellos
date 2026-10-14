# Standalone Proposal Review: OGJ1 Codec and Unknown/Lifecycle Protocol

**Review Date:** 2026-10-14  
**Target Proposal:** [ogj1-codec-unknown-lifecycle-design.md](ogj1-codec-unknown-lifecycle-design.md)  
**Verification Harness:** [scripts/verify-ogj1-codec.py](../../scripts/verify-ogj1-codec.py)  
**Verification Receipt:** [build/ogj1-codec-verification.json](../../build/ogj1-codec-verification.json)  
**Review Type:** Standalone adversarial proposal review (P01 / G2 persistence prerequisites).  
**Review Disposition:** **APPROVED FOR SPECIFICATION & TEST MODEL; RUNTIME PROMOTION GATED.**

---

## 1. Executive Summary and Scope Boundaries

This standalone review evaluates the proposed Ordinary Generic Journal v1 (`OGJ1`) binary codec and the associated `Unknown` prospective-charge lifecycle protocol for Cellos persistence.

### Invariant Boundaries Confirmed
- **No public wire opcode or ABI delta:** `libs/api/src/abi/` and VFS wire protocols remain frozen.
- **No VFS runtime activation:** `cells/services/vfs/src/**` is not modified; no native mount is switched to journal mode.
- **No KMS or protected storage modification:** 186-byte KMS records, two-slot storage, CAS, signed time, and TLS relay remain strictly untouched.
- **No synthetic issuer minting:** Cryptographic signers, production pins, and genesis authorities are not fabricated.
- **No CP1 / CP2 promotion:** All 23 downstream checkpoint pairs and 38 legacy locks retain their pending/locked status.

---

## 2. Invariant Verification

The proposal and executable test harness were reviewed against the 10 governing EARS criteria ([ogj1-codec-unknown-lifecycle-design.md §6](ogj1-codec-unknown-lifecycle-design.md#6-ears-acceptance-criteria)). All invariants were confirmed via formal negative controls and simulation:

| Invariant | Specification | Verification Result |
|---|---|---|
| **Fixed Header Geometry** | Exactly 112 bytes, little-endian, fixed offsets, magic `b"OGJ1"`, version `1`, reserved `0`. | **PASS** — Enforced by `struct.pack("<4s H B B I 32s 16s Q Q 32s I")`; corrupt magic, version 2, and non-zero reserved reject (TC01–TC04). |
| **Domain Separation** | Trailer is 32-byte SHA-256 over domain prefix `cellos/ordinary-journal/v1\0` + header + body. | **PASS** — Bit flips in body or header tampering immediately trigger digest verification failure (TC05, TC06). |
| **Commit Body Constraint** | Commit and Switch bodies are strictly 40 bytes: slot (u32), ref_len (u32), ref_digest (32 B). | **PASS** — Length != 40 bytes rejected (TC09). |
| **Payload Bound** | Canonical payload bodies $\le 256$ bytes in development experiment model. | **PASS** — Oversized bodies ($>256$ B) rejected (TC08). |
| **Finite CellosFS Storage** | 9 directories, $\le 12$ files, 21 inodes under `/srv/cellos/journal/{tx,cp}`. Write-once slots. | **PASS** — Inode limits fit inline inode storage (428 B payload); slot re-use rejected (TC10, TC12). |
| **D1 / D2 Double Barrier** | D1 proves Payload durability; D2 proves Commit durability; client Ack only after D2. | **PASS** — Verified order; D1 alone acknowledges no transaction (TC10). |
| **Prospective Charge Retention** | Any failure/ambiguity transitions to `Unknown(same T)` with full charge retained. | **PASS** — Simulated D1 failure, pre-D2 crash, D2 failure, and reply loss retain 100% of charge vector (TC11, TC13). |
| **Conflicting Mutation Gate** | No mutation permitted while in `Unknown` state. | **PASS** — Mutations during `Unknown` rejected (TC12). |
| **Recovery Chain Replay** | Linear contiguous Commit chain validation from Genesis/Checkpoint. | **PASS** — Contiguous chain successfully recovers `WRITABLE` state (TC14). |
| **Orphan Payload Handling** | Uncommitted Payload retained under `Unknown(T)`; never deleted or refunded. | **PASS** — Recovery flags orphan payload, sets `UNKNOWN`, retains prospective charge (TC15). |
| **Fail-Closed on Tamper/Fork** | Torn commit, revision gap, conflicting fork, or fence regression $\to$ `RecoveryFailed`. | **PASS** — Broken chain, revision jump, and fence regression fail closed (TC16–TC18). |

---

## 3. Adversarial Risk Analysis & Resolution

### Risk 1: Silent Charge Dropping on In-Flight Ambiguity
- **Hazard:** A network timeout, host kill, or short write after I/O dispatch might cause the system to drop the prospective charge or return a generic error, allowing double-spending or resource exhaustion.
- **Resolution:** The protocol mandates pre-allocation of the full prospective vector *before* issuing barrier D1. Any unobserved or failing barrier transition sets `active_unknown = T` and preserves `retained_charges[T]`. The charge remains held until authoritative supervisor reconciliation. Verified in TC11 and TC13.

### Risk 2: Domain Prefix and Preimage Substitution Attacks
- **Hazard:** A raw payload body could be crafted to mirror a valid header/trailer, or an unauthenticated record from another subsystem could be injected.
- **Resolution:** A 27-byte domain separator prefix `b"cellos/ordinary-journal/v1\0"` is strictly prepended to the hash preimage. Furthermore, total record length and internal body length are redundantly declared in the header and cross-checked against on-disk file size. Verified in TC05, TC06, and TC07.

### Risk 3: CellosFS Inode Exhaustion via Hexadecimal Filenames
- **Hazard:** Naive journal designs store transaction IDs as directory entries (e.g. `tx/0123456789abcdef...`). In CellosFS, directory blocks hold at most 7 entries with `MAX_NAME_LEN=46`. Long or unbounded filenames rapidly exhaust inode blocks.
- **Resolution:** The proposal selects a fixed, finite 4-way slot directory tree (`tx/{0..3}` and `cp/{0..1}`) using fixed short names (`payload`, `commit`, `checkpoint`, `switch`). The entire journal uses exactly 21 inodes and fits within inline inode storage limits. Verified in layout design §3.

### Risk 4: Torn Commit Misinterpretation
- **Hazard:** A power-loss crash mid-commit write could leave a partial file on disk. If treated as valid, state corrupts; if treated as empty, an acknowledged commit is lost.
- **Resolution:** The codec requires Commit records to be exactly 184 bytes with a valid cryptographic trailer. A torn commit fails trailer validation, is treated as uncommitted, and causes recovery to transition to `Unknown(T)` (rather than advancing revision or deleting the file). Verified in TC16.

### Risk 5: Forking or Revision Gaps Across Reboots
- **Hazard:** If two distinct commits exist at the same revision, or a commit skips a revision number, linear ledger invariants are violated.
- **Resolution:** Recovery strictly enforces $r = \text{current} + 1$ and `prev_commit_digest == current_digest`. Any gap or fork immediately halts recovery and enters `RecoveryFailed`. Verified in TC17.

### Risk 6: Authority Fence Regression
- **Hazard:** An outdated image or replayed commit could attempt to install an authority fence lower than the local node's boot floor.
- **Resolution:** The state machine and recovery algorithm verify $\text{fence} \ge \text{boot\_floor}$ at every record. Any regression immediately transitions to `RecoveryFailed`. Verified in TC18.

---

## 4. Verification Evidence

The standalone verification script was executed:
```bash
python3 scripts/verify-ogj1-codec.py --out build/ogj1-codec-verification.json
```

**Output:**
```
OGJ1 Codec Verification: 18/18 tests passed.
  [PASS] TC01_roundtrip_all_kinds: All 4 kinds encoded and decoded cleanly
  [PASS] TC02_reject_invalid_magic: Corrupt magic rejected
  [PASS] TC03_reject_unsupported_version: Version 2 rejected
  [PASS] TC04_reject_nonzero_reserved: Nonzero reserved rejected
  [PASS] TC05_reject_tampered_body: Bit flip in body triggers digest verification failure
  [PASS] TC06_reject_tampered_header: Modified revision detected by trailer digest
  [PASS] TC07_reject_truncated_record: Truncated record rejected
  [PASS] TC08_reject_oversized_payload: Canonical payload > 256 bytes rejected
  [PASS] TC09_reject_invalid_commit_body_len: Commit body != 40 bytes rejected
  [PASS] TC10_successful_double_barrier: D1 then D2 advances revision and confirms
  [PASS] TC11_d1_failure_unknown_charge_retained: D1 barrier fail transitions to UNKNOWN, charge retained
  [PASS] TC12_refuse_mutation_during_unknown: Conflicting mutation refused while Unknown active
  [PASS] TC13_post_d1_pre_d2_crash_charge_retained: Post-D1 crash creates orphan payload, retains charge
  [PASS] TC14_recover_contiguous_chain: Contiguous commits recovered into WRITABLE state
  [PASS] TC15_recover_orphan_payload_unknown: Orphan payload recovered into UNKNOWN state, not deleted
  [PASS] TC16_recover_torn_commit_unknown: Torn commit detected, does not commit, yields UNKNOWN
  [PASS] TC17_recover_revision_gap_fails: Revision gap triggers RECOVERY_FAILED
  [PASS] TC18_recover_fence_regression_fails: Fence regression triggers RECOVERY_FAILED
```

---

## 5. Reviewer Recommendation and Next Steps

1. **Accept Specification:** The OGJ1 binary layout, double-barrier sequence, finite CellosFS layout, and Unknown lifecycle invariants in [ogj1-codec-unknown-lifecycle-design.md](ogj1-codec-unknown-lifecycle-design.md) are structurally complete and validated.
2. **Preserve Runtime Gates:** Do NOT integrate this into `cells/services/vfs/src/**` or expose wire opcodes until formal maintainer approval of Checkpoint-1 (CP1) is authorized.
3. **Downstream Handoff:** Provide this design and verification evidence to P05 (Resource Authority), P06 (Recovery), and P08-C (Distributed Authority) as the concrete foundation for durable lifecycle transactions.
