# Spec 18 — Cell Trust Tiers (ADR)

> **Status**: Accepted 2026-07-30; amended 2026-08-01 by D13. Supersedes the WASM
> runtime tier wherever older documents mention it. Tier 2 and fleet-secure Tier-1
> admission are accepted designs, not current production mechanisms.

## 1. Context

Cellos isolation is language-based (LBI): the Rust type system is the wall between
Cells sharing one address space (Spec 16). That wall only holds for code that provably
contains no `unsafe`. Two facts broke the previous story:

1. **A signature proves only possession of its key.** `scripts/sign-cell.py` signs
   PT_LOAD segments + manifest with Ed25519. The normal `cellos-sign` route now checks
   F1/F5 before signing, but that meaning depends on a controlled pipeline and key:
   the scan covers `cells/` (with a reviewed allowlist), not every dependency; the CLI
   accepts target ELFs after checking the source tree rather than proving they came from
   that build; and the reproducible dev seed is public and forgeable.
2. **There is no algorithm that verifies memory-safety of a compiled ELF.** Midori
   could verify at install time because apps shipped as typed bytecode (MSIL). Native
   ELF cannot be verified after the fact — safety must be checked at *build* time
   (when source is visible) or enforced at *run* time (by hardware).

Third-party developers will not upload source to a Cellos build service (IP exposure),
and client-side tooling on an untrusted machine can always be forged — no pure-software
scheme survives a hostile developer. The WASM tier was evaluated as the untrusted-code
answer and **rejected** (2026-07-30): interpreter-grade execution speed, and it solved
no problem the tiers below don't solve better.

## 2. Decision

Three tiers are the accepted destination. The current loader does **not** assign a memory
tier from signature verification: every admitted native cell uses the shared SAS. The
tier decision point arrives only when Spec 19 Layer B is implemented.

| Tier | Status | Who | Isolation mechanism | Execution speed | IPC |
|------|--------|-----|---------------------|-----------------|-----|
| **1 — SAS cell** | SAS shipped; fleet signed-only admission **not shipped** | Operationally trusted first-party/platform cells; future fleet posture requires a controlled signing pipeline | LBI: rustc + F1 policy outside reviewed exceptions | Native, zero-cost boundary | Zero-copy grants |
| **2 — Domain cell** | **accepted, NOT implemented** | Any unsigned native ELF (third-party developers) | Hardware: private page-table view — same VA layout as the SAS, but *other cells' pages are simply not mapped* | Native inside the domain; `satp`+ASID switch at the boundary | Kernel-copied messages; grants mapped explicitly per-share |
| **3 — Silo VM** | shipped (aarch64) | Whole legacy stacks (Linux guests) | Stage-2 paging (H-extension) | Native inside guest | virtio / proxy |

Tier 2 **adds** a containment option; it does not retract the standing advice that untrusted
third-party code belongs in Tier 3 **until Tier 2's mechanism exists**. Today the kernel has
one root page table (`kernel/src/memory/paging.rs:38`) and no context switch writes
`satp`/`TTBR0`/`CR3`, so there is no domain to place a cell in. The operative rule until then
is Tier 3 or nothing (`docs/security-model.md`).

A note on an apparent conflict: `security-model.md` records a 2026-06-05 decision that
per-Cell SATP isolation is "explicitly NOT pursued". That decision is about **Tier 1**, where
a page-table switch per cell would destroy zero-copy IPC and the SAS economy. Tier 2 pays
that exact cost on purpose, and only for cells that have not been verified — which is why the
two decisions coexist rather than contradict.

Target invariant: **there is no unverified native code inside the shared SAS view in a
fleet-secure build.** Current G1/dev builds do not enforce this invariant: when
`signing-required` is off, an ELF with no `__ViCell_sig` section is admitted to the SAS.
This is a development posture, not a sandbox for hostile native code.

### 2.1 Tier 1 admission — `cellos-sign`

In a controlled fleet pipeline, the platform signature is intended to mean **"approved
by a pipeline that enforced F1/F5"**. The normal `cellos-sign` route refuses to sign
unless its repository checks pass:

- Cell crate roots and tracked Rust files under `cells/` satisfy the F1 attribute/token
  checks outside `scripts/unsafe-allowlist.toml`; `libs/*` remains reviewed TCB rather
  than part of this ratchet;
- the toolchain matches the pinned `rust-toolchain.toml` (policy F5);
- the controlled CI job binds the target ELF to the checked build before releasing the
  production signing key. The CLI itself accepts target paths and does not establish
  that build-to-artifact provenance.

The production signing key must live in CI/KMS, not on developer machines; key policy and
artifact provenance are the guarantee. The current `[0x43; 32]` dev seed and
`--unchecked-dev-signature` route are test fixtures: the kernel cannot distinguish their
signatures from a checked dev signature, so they establish no adversarial provenance.

Current admission behavior is explicit:

- signature present but invalid: deny in every build;
- signature absent + `signing-required`: deny;
- signature absent + default G1 features: admit to the SAS;
- disabling `dev-signing-key` selects a `[0u8; 32]` placeholder, not a provisioned fleet
  key, so it cannot form a usable production profile.

### 2.2 Tier 2 admission — nothing

That is the point of the accepted design. A third-party developer would build a normal
cell ELF with the public SDK, omit a platform signature, withhold source, and run at native
speed behind an MMU wall once Tier 2 exists. `unsafe` in a Tier-2 cell would corrupt only
that cell. The costs a Tier-2
cell pays, relative to Tier 1: address-space switch at its scheduling boundary
(ASID-tagged, no full TLB flush — VF2/Pioneer/RK3588 all have MMU+ASID; none has
MTE/PKU, which is why page tables are the mechanism), and copied IPC instead of
zero-copy grants. Verification buys performance; it is never a license to exist.

### 2.3 Upgrade path (G2+, optional)

**Confidential build**: `cellos-sign` + pinned toolchain packaged as a measured
confidential-VM image (SEV-SNP / TDX / ARM CCA). Developer source enters the enclave
encrypted; the Cellos key server releases a signing credential only to an enclave that
attests the exact builder image. Neither the developer's machine nor Cellos operators
can read the other side's secrets — this is the voluntary road from Tier 2 to Tier 1
for developers who want zero-copy performance without disclosing source. Infrastructure
item; not a prerequisite for anything above.

## 3. Rejected alternatives

- **Fork rustc / bespoke compiler** — a fork adds maintenance of a multi-million-line
  compiler without adding verification: the compiler is still *trusted*, now with fewer
  eyes. Spec 16 already provides the qualified-toolchain path (pin + Ferrocene).
- **WASM tier** — rejected for execution speed and because it duplicates what Tier 2
  provides with zero speed penalty and no new runtime to maintain. Historical references
  remain in `docs/project-changelog.md` and `docs/research/` only.
- **Client-side attestation without TEE** — forgeable by construction; see §1.
- **Mandatory build service for third parties** — rejected: source-disclosure
  requirement would strangle the ecosystem; Tier 2 removes the need.

## 4. Consequences

- The future kernel loader gains a tier decision point: an artifact approved by the
  fleet pipeline may use the SAS; an unapproved/unsigned artifact uses a domain mapping
  once Spec 19 Layer B exists. Today there is no domain branch: default builds admit an
  absent signature to the SAS, while `signing-required` builds refuse it.
- A production profile must provision the kernel's immutable cell-signing public key,
  enable `signing-required` and `policy-required`, remove dev-key/weak-RNG features, bind
  checked source to the signed artifact, and test unsigned, stripped, wrong-key,
  dev-key, tampered, and unchecked-dev-signed negative cases.
- Secure boot remains required to anchor the kernel and its embedded trust key.
- `GrantShare` to/from a Tier-2 cell maps the grant into the domain's table explicitly;
  `DataPtr`-style raw pointers (`GetFile`) are unrepresentable across the tier boundary
  — consistent with their planned removal (midori-lessons phase 06).
- Scheduler context-switch path becomes tier-aware (`satp` swap only when crossing
  domains; SAS→SAS switches stay as cheap as today).
- Spec 02 (memory) and Spec 17 (IPC wire contract) need addenda when Tier 2 lands.
- WASM crates are still present in the workspace. Their retain-vs-remove disposition and
  Tier-2/runtime qualification remain unresolved; do not describe removal as landed.

## 5. Cross-references

| Topic | Document |
|-------|----------|
| rustc as TCB, policies F1–F7 | `docs/specs/16-rustc-tcb.md` |
| Hardware isolation layers (W^X, MPK, domain tables) | `docs/specs/19-hardware-isolation-layers.md` |
| Signing pipeline (current) | `scripts/sign-cell.py`, `scripts/lib-sign-cells.sh`, `kernel/src/signing.rs` |
| Capability model / manifest | `docs/specs/01-core.md` |
| Hypervisor tier (silo) | `.agents/260607-1420-h-ext-hypervisor-cap/` (plan record) |
