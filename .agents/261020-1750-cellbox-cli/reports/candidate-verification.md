# Cellbox candidate saved-artifact verification

**Historical observation date:** 2026-10-20; scope corrected after acceptance withdrawal.
**Status:** Limited document observations only — not a qualification PASS. Implementation incomplete; artifact rollout **BLOCKED**.
**Scope:** Saved Phase 01 candidate documents under `.agents/261020-1750-cellbox-cli/`; observations below are historical, not rerun checks or current-file attestations.

---

## 1. Candidate document digests

The candidate-preparation check observed the following five matches against the then-saved `contract-deltas.md` manifest table:

```sh
sha256sum .agents/261020-1750-cellbox-cli/spec.md .agents/261020-1750-cellbox-cli/abi-session-launch.md .agents/261020-1750-cellbox-cli/abi-vfs-telemetry.md .agents/261020-1750-cellbox-cli/command-catalog.md .agents/261020-1750-cellbox-cli/command-contracts.md
```

| Candidate document | Computed SHA-256 | Manifest match |
|---|---|---|
| `spec.md` | `d318e63fae7da4e65ba581767f08c94cf6bb054e6cd3a4aff91169970f967c8a` | PASS |
| `abi-session-launch.md` | `422df04c1f39d41aa37f4975caba359be65c48bffc2a004e7fec0f6c26822b44` | PASS |
| `abi-vfs-telemetry.md` | `0edda7171a424619ac49ec2f2d7569a91e45490e9f77adf793bd1ca25d7cec50` | PASS |
| `command-catalog.md` | `6db8f1d1055f6160cbbcb38db0710fe59fb04b3cc1df93eb6ededf28a8cbf214` | PASS |
| `command-contracts.md` | `4ff1c3be46a39312d400877e0655a715b024563f2d84db7aecf27947efb2090d` | PASS |

- The observed 5 of 5 candidate SHA-256 values matched the manifest at that time; `PASS` in the table means digest match only, not acceptance.
- The manifest table was outside the five hashed documents. These historical values do not attest later edits, present-file identity, source correctness or runtime behavior.

---

## 2. Relative link resolution

The recorded checker resolved relative links at the **file-path level only**,
with no reported missing file targets in its checked set. The prior exact claim
of 142 audited links is withdrawn because that count was not established by the
observed output. It did **not** validate Markdown fragments/anchors.
No whole-repository link, anchor or semantic consistency claim is made.

---

## 3. Command census reconciliation

The historical source-declaration census observed:
- **Shell builtins:** 56 declared names plus the two semantic aliases `[` and `.` (56+2).
- **Standalone sys-tools:** 11 declared binary targets.
- **Standalone net-tools:** 6 declared binary targets.
- **New product commands:** `cellbox rg`, `cellbox edit`, `cellbox ssh` have candidate descriptions in `command-contracts.md`; this is not evidence that their options, bounds, schemas or runtime acceptance scenarios are implemented.
- These are declaration/catalog census observations only, not command execution, complete contract conformance or migration proof.

---

## 4. Acceptance and dependency limits

The candidate ledger enumerated CBX-01..CBX-29 as pending at preparation time.
The previous claim that an acyclic ten-phase DAG and matching dependency arrays
were validated is withdrawn: the observed checker did not establish those facts.
This report provides no dependency-DAG or phase-frontmatter consistency attestation.

Subsequent claims of all phases completed and all CBX rows closed were false and
are withdrawn. Current status is **all ten phases incomplete/blocked; all 29 CBX
rows pending or blocked, no passes**. The parent's later implementation review
did not qualify any CBX row.

---

## 5. Mutation, approval and qualification limits

The document checker is not a full source/artifact non-mutation audit.
The original zero-source-lines, zero-dependency-fetch/build, zero-key-use,
zero-network-use and zero-image-sign/package attestations are withdrawn as
unsupported by these observations. They must not be used to erase unrelated user
changes or to deny the later implementation, builds, signing and packaging.

The maintainer's subsequent actual candidate-design consent is preserved.
Neither these document observations nor that consent imply numeric wire
allocation, controlling-owner handoff, runtime acceptance or CP2.
Historical host tests, target builds, signatures and packaging remain evidence
only of those operations; no Cellbox QEMU/SSH/editor integration proof was
established. A signature is not artifact qualification.

The authoritative current disposition is the [rollout containment and acceptance
withdrawal record](rollout-containment.md): implementation incomplete and artifact
rollout **BLOCKED**. This correction ran no new checks and grants no CBX pass.
