"""`cellos-sign` — Tier-1 admission gate: check F1/F5, then sign.

The platform signature means **"built by a pipeline that enforced F1"**, not
merely "these bytes are ours" (Spec 18 §2.1). This package implements the check
side of that promise and makes signing unreachable without it.

Threat model — read this before trusting the gate
-------------------------------------------------
`cellos-sign` defends against **unintentional mistakes by trusted developers**:
an `unsafe` block added to a Cell without review, a crate root that lost its
`#![forbid(unsafe_code)]`, a build on an unpinned toolchain.

It does **NOT** defend against a malicious developer holding the signing key.
Every check here runs on the same machine as the signer, from source the signer
controls; anyone who can sign can also edit the allowlist. Defence against a
hostile signer is the *key policy's* job (the production key lives only in
CI/KMS, never on a developer machine) and Tier 2's job (unsigned third-party
cells get a hardware page-table wall instead — `docs/specs/18-cell-trust-tiers.md`).
Do not describe this tool as verification of untrusted code. It is not.

Checks
------
F1 (Spec 16 §6) — two complementary source-level layers, both on text reduced
to code (comments and string/char literals removed):
  * *attribute*: every crate root under `cells/` carries `#![forbid(unsafe_code)]`,
    unless the crate is allowlisted;
  * *token*: no `.rs` file under `cells/` contains the `unsafe` keyword, unless
    the file is allowlisted. This catches files excluded from the module graph,
    which the attribute alone cannot.

F1 covers `cells/` only: `libs/*` is trusted TCB, reviewed rather than ratcheted
(see `policy.py` for the exact boundary and for what `forbid` does not catch).

False positives are accepted; false negatives are not. The allowlist is the
escape hatch, and every entry carries a reason, an approver and a date — it is
an approved hole in the LBI wall and is reviewed as such.

F5 (Spec 16) — the running toolchain matches `rust-toolchain.toml`.
"""

__all__ = ["allowlist", "lexer", "policy", "scan", "signing", "toolchain"]
