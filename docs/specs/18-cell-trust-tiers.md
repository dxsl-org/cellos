# Spec 18 — Cell Trust Tiers (ADR)

> **Status**: Accepted 2026-07-30 — supersedes the WASM runtime tier wherever older
> documents mention it. Implementation of Tier 2 is scheduled for the plan following
> `midori-lessons` (per-domain page tables); Tier 1 tooling (`cellos-sign`) is phase 11
> of the current plan.

## 1. Context

Cellos isolation is language-based (LBI): the Rust type system is the wall between
Cells sharing one address space (Spec 16). That wall only holds for code that provably
contains no `unsafe`. Two facts broke the previous story:

1. **The signature attests provenance, not safety.** `scripts/sign-cell.py` signs
   PT_LOAD segments + manifest with Ed25519. It proves *who built the binary*, not
   *what the binary can do*. Policy F1 (`#![forbid(unsafe_code)]` on every Cell crate,
   Spec 16 §6) is not enforced by any pipeline — at the time of this ADR only 25 of 71
   cell crates carry the attribute.
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

Three tiers. The loader assigns the tier at spawn from signature verification; the
tier decides which memory model the Cell gets.

| Tier | Status | Who | Isolation mechanism | Execution speed | IPC |
|------|--------|-----|---------------------|-----------------|-----|
| **1 — SAS cell** | shipped | First-party / platform-built cells, signed via `cellos-sign` | LBI: rustc + enforced F1 (build-time verification) | Native, zero-cost boundary | Zero-copy grants |
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

Invariant: **there is no "unverified native code inside the shared SAS view" tier.**
An ELF without a valid platform signature never sees another Cell's pages.

### 2.1 Tier 1 admission — `cellos-sign`

The platform signature changes meaning from "bytes are ours" to **"built by a pipeline
that enforced F1"**. `cellos-sign` (evolution of `scripts/sign-cell.py` +
`lib-sign-cells.sh`) refuses to sign unless, in the same pipeline step as the build:

- `#![forbid(unsafe_code)]` is present on the cell crate and every dependency outside
  a reviewed allowlist (drivers needing MMIO, `ostd`) — each allowlist entry carries a
  written reason and a `// SAFETY:` audit reference;
- the toolchain matches the pinned `rust-toolchain.toml` (policy F5);
- the signed artifact is the artifact produced by that checked build (check and sign
  are one step — the tool never signs a foreign ELF).

The signing key lives in CI/KMS, not on developer machines. Possession of the
`cellos-sign` tool grants nothing; the key policy is the guarantee.

### 2.2 Tier 2 admission — nothing

That is the point. A third-party developer builds a normal cell ELF with the public
SDK, does not sign it, does not disclose source, and it runs at native speed behind an
MMU wall. `unsafe` in a Tier-2 cell can corrupt only that cell. The costs a Tier-2
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

- The kernel loader gains a tier decision point: signature valid → SAS mapping (status
  quo); absent/invalid → domain mapping (new; requires the per-domain page-table
  mechanism of Spec 19 §2). Until that mechanism ships, unsigned cells in production
  posture are refused (dev builds keep `signing_required` off).
- `GrantShare` to/from a Tier-2 cell maps the grant into the domain's table explicitly;
  `DataPtr`-style raw pointers (`GetFile`) are unrepresentable across the tier boundary
  — consistent with their planned removal (midori-lessons phase 06).
- Scheduler context-switch path becomes tier-aware (`satp` swap only when crossing
  domains; SAS→SAS switches stay as cheap as today).
- Spec 02 (memory) and Spec 17 (IPC wire contract) need addenda when Tier 2 lands.
- `cells/drivers/wasm` and wasmi leave the workspace; docs no longer describe WASM.

## 5. Cross-references

| Topic | Document |
|-------|----------|
| rustc as TCB, policies F1–F7 | `docs/specs/16-rustc-tcb.md` |
| Hardware isolation layers (W^X, MPK, domain tables) | `docs/specs/19-hardware-isolation-layers.md` |
| Signing pipeline (current) | `scripts/sign-cell.py`, `scripts/lib-sign-cells.sh`, `kernel/src/signing.rs` |
| Capability model / manifest | `docs/specs/01-core.md` |
| Hypervisor tier (silo) | `.agents/260607-1420-h-ext-hypervisor-cap/` (plan record) |
