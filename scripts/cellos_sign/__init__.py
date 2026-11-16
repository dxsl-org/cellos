"""`cellos-sign` — Tier-1 admission gate: check F1, tier class, closure and F5, then sign.

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

F1 covers `cells/`. The code Tier-1 cells link from outside `cells/` is covered
by the closure ratchet below, not by F1 (see `policy.py` for the exact boundary
and for what `forbid` does not catch).

Tier class (Spec 18 §2.1) — a crate under `cells/` whose allowlist entries
include class `c-ffi` or `legacy-raw-ptr` must declare manifest class FFI or
UNTRUSTED, which the kernel loads in Tier 2, unless a reviewed
`[[tier_exception]]` names it and the manifest class it accepts. The class is
read from source and fails closed, and before signing it is read again from
each target ELF exactly as the loader reads it (`tier_class.py`,
`elf_manifest.py`). A file with an `extern` block must be classed `c-ffi` (or
`test-harness` / `test-violation`).

Tier-1 closure (Spec 16 §6, F1b) — every file of an in-tree crate linked into a
Tier-1 cell (`libs/ostd`, `libs/wire`, the vendored `virtio-drivers`, …, listed
in `scripts/tier1-closure.generated.toml`) is held to reviewed ceilings of
`unsafe` tokens and unsafe attributes (`no_mangle`, `export_name`,
`link_section`, `global_asm!`) in `scripts/tcb-unsafe-baseline.toml`, and so is
the combined Tier-1 total of cells plus closure. A rise fails; a drop fails CI
(`--check --strict`) and is a NOTE on `--sign` until `--write-baseline` lowers
the ceiling. Third-party crates and proc-macros are pinned by name, version and
full source in the generated file, which CI regenerates and diffs (`tcb.py`).

False positives are accepted; false negatives are not. The allowlist is the
escape hatch, and every entry carries a reason, an approver and a date — it is
an approved hole in the LBI wall and is reviewed as such.

F5 (Spec 16) — the running toolchain matches `rust-toolchain.toml`.
"""

__all__ = ["allowlist", "elf_manifest", "lexer", "policy", "scan", "signing", "tcb", "tier_class",
           "toolchain"]
