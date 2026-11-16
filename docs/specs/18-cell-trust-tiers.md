# Spec 18 — Cell Execution Tiers, Runtime Profiles, and Admission (ADR)

> **Status**: Accepted 2026-07-30; amended 2026-08-01 by D13, again 2026-08-01 by
> `18b-cell-admission-consent-adr.md` (§2.1, §4), and terminology-normalized on
> 2026-08-19 by ADR 0003. Supersedes the WASM runtime tier wherever older documents mention it.
> Tier 2 Paged Domain Engine is shipped and verified on 2026-09-06 (ADR-0015 Phase 02) via
> negative hardware page-fault test (`tests/integration/tests/tier2_fault_isolation.rs`).
> Amended 2026-11-16 (architecture remediation Phase 19): §2 admission truth and §4
> consequences corrected to the code; per-architecture status is the generated table
> [`docs/generated/arch-status.md`](../generated/arch-status.md).

## 1. Context

Cellos isolation is language-based (LBI): the Rust type system is the wall between
Cells sharing one address space (Spec 16). That wall only holds for code that provably
contains no `unsafe`. Two facts broke the previous story:

1. **A signature proves only possession of its key.** `scripts/sign-cell.py` signs
   PT_LOAD segments + manifest with Ed25519. The normal `cellos-sign` route now checks
   F1/F5 before signing, but that meaning depends on a controlled pipeline and key:
   the `forbid` scan covers `cells/` (with a reviewed allowlist), and the code cells link
   is held to reviewed ceilings and pins, not proven safe; the CLI reads each target
   ELF's manifest but does not prove the ELF came from the checked build; and the
   reproducible dev seed is public and forgeable.
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

Tier means the execution and isolation boundary only. It does **not** mean language,
standard library surface, SDK generation, or roadmap stage.

| Term | Meaning | Examples |
|------|---------|----------|
| **Tier** | Execution / isolation boundary | Tier 1 trusted SAS, Tier 2 native domain, Tier 3 VM guest |
| **Profile** | Language/runtime/toolchain surface inside a tier | Rust `no_std`, planned Rust `std`, POSIX/FFI, Lua |
| **SDK** | Shared API/tooling family | Native SDK Core (`ostd`), higher middleware, guest integration helpers |
| **Layer** | Internal implementation layer | Spec 19 Layer A/B/C, kernel/HAL/system layers |
| **Stage** | Product roadmap maturity overlay | G1..G5 |

The accepted destination is three execution tiers. Under the Dual-Mode Hybrid Architecture (ADR-0015),
the loader assigns unsigned and FFI native cells into Tier 2 Paged Domains (`PROTECTION_CLASS_UNTRUSTED`
and `PROTECTION_CLASS_FFI`), while signed Safe Rust cells execute in Tier 1 SAS.

| Tier | Status | Who | Isolation mechanism | Execution speed | IPC |
|------|--------|-----|---------------------|-----------------|-----|
| **1 — Trusted SAS cell** | SAS shipped; mandatory signing in Phase 01 ([ADR-0015](../decisions/0015-dual-mode-hybrid-architecture.md)) | 100% Safe Rust cells (`no_std` and in-tree pure-Rust `std`) + audited Driver Cells (e1000, NVMe, VirtIO with MMIO/DMA). Requires valid Ed25519 signature. | LBI: rustc `#![forbid(unsafe_code)]` + audited driver MMIO/DMA under IOMMU | Native zero-cost SAS boundary; no TLB flush | Zero-trap SPSC Ring Buffer / Zero-copy grants |
| **2 — Native domain cell** | **Shipped (2026-09-06, ADR-0015 Phase 02)** — verified via negative hardware Page Fault containment test; per-arch admission in [arch-status](../generated/arch-status.md) | All unsigned binaries (including unsigned Rust), all C/C++/Zig FFI profiles, Doom, Tetris-C, and Lua runtime | Hardware: private page-table (`satp`/`CR3`/`TTBR0`) — other cells' and kernel pages unmapped/NX | Native inside domain; ASID-tagged switch | Kernel-copied messages; `validate_user_buf` |
| **3 — VM guest** | Shipped where the arch has a hypervisor module ([arch-status](../generated/arch-status.md), Tier-3 column) | Whole legacy stacks, normally Linux guests (Alpine, Nginx, ROS2) | Stage-2 paging / hypervisor guest boundary | Native inside guest | virtio / proxy |

Profiles under the Dual-Mode Hybrid Architecture (ADR-0015):

- **Tier 1**: Strict Safe Rust (`no_std` and in-tree pure-Rust `std`) + audited Driver Cells. Signed-only.
- **Tier 2**: All C/C++/Zig FFI code (`posix-shim`, `mlibc`), the `cpp-freestanding` C++ subset
  profile ([ADR-0018](../decisions/0018-cell-native-portability-and-runtime-profiles.md) §2.3),
  the `cpp-hosted` C++17 profile (LLVM libc++ over mlibc, x86_64 only;
  [ADR-0034](../decisions/0034-hosted-cxx-runtime-profile.md)),
  dynamic runtimes (Lua), and all unsigned native binaries. Contained by hardware MMU. Admission
  is gated by the on-path policy ruled in
  [ADR-0019](../decisions/0019-tier2-admission-control-on-path.md).
- **Tier 3**: Full Linux guest userspace via Stage-2 hypervisor.

Do not define Tier 2 as "Tier 1 but unsigned". The absence of a trusted
signature is an admission input; Tier 2 is the hardware-domain execution class
that can contain that native code once implemented.

Silo is **not** its own execution tier. It is Tier-1-facing service/hypervisor
infrastructure that exposes a hardware-fenced API to trusted cells.

Tier 2 **adds** a hardware containment option under Spec 19 Layer B and Spec 22:
the kernel allocates an `AddressSpace` per domain cell with a private page table (`satp` on RV64,
`TTBR0_EL1` on AArch64, `CR3` on x86_64). During scheduling, transitions into or between domain
cells activate the respective hardware root and flush translations, while SAS-to-SAS switches
remain on the zero-cost fast path without MMU root writes. Unverified native code and C-FFI
runtimes are safely contained behind hardware page faults.
A note on an apparent conflict: `security-model.md` records a 2026-06-05 decision that
per-Cell SATP isolation is "explicitly NOT pursued". That decision is about **Tier 1**, where
a page-table switch per cell would destroy zero-copy IPC and the SAS economy. Tier 2 pays
that exact cost on purpose, and only for cells that have not been verified — which is why the
two decisions coexist rather than contradict.

Invariant: **an unsigned native ELF loaded through the governed spawn never runs inside the
shared SAS view.** The governed spawn denies it in the release posture and otherwise routes it
to Tier 2 (`kernel/src/loader/governed_spawn.rs:102-119`, `kernel/src/signing.rs:90-95`). Where
the boot leaves Tier-2 admission disabled (fleet profiles, AArch64 without `test-hooks`), a
domain-class artifact is denied rather than admitted elsewhere (`kernel/src/main.rs:414-447`;
Spec 22 §4). The one exception is `init`: it is embedded in the kernel image and published as
trusted with a digest, not a signature (`kernel/src/loader/governed_spawn.rs:333-345`).

### 2.1 Tier 1 admission — `cellos-sign`

Tier-1 admission carries two claims, and only one of them is a signature over the
artifact. `18b-cell-admission-consent-adr.md` separates them:

- **Claim A — safety/provenance**: this ELF was built under F1/F5. Checkable only where
  source is visible, so it is attested at **build** time by a publisher key in CI/KMS.
  That is what `cellos-sign` produces, and the rest of this section describes it.
- **Claim B — authorization**: the owner of this machine permits this binary in their
  SAS. Assertable only by the owner, so it is recorded at **install** time as a
  digest-pinned admission entry under a separate owner anchor — not as a second
  signature embedded in the ELF.

Admission is `A ∧ B`: the owner's record may withhold Tier 1 from a validly signed
binary and can never grant Tier 1 without one. The owner anchor
(`CELLOS_RELEASE_OWNER_PUBLIC_KEY`, provisioned at build time by `kernel/build.rs`) and the
gate (`admission::evaluate_owner_admission`, `kernel/src/admission/gate.rs:93`, called by every
governed spawn) exist. Dev and test kernels are permissive; a release kernel refuses every
spawn until a qualified backend installs an admitted A/B slot, and
`OwnerAdmissionRegistry::update` has no production caller yet (`kernel/src/admission/mod.rs:1-7`).
§2.1 as written below describes Claim A only.

In a controlled fleet pipeline, the platform signature is intended to mean **"approved
by a pipeline that enforced F1/F5"**. The normal `cellos-sign` route refuses to sign
unless its repository checks pass:

- Cell crate roots and tracked Rust files under `cells/` satisfy the F1 attribute/token
  checks outside `scripts/unsafe-allowlist.toml`;
- every file of an in-tree crate in the Tier-1 dependency closure (`libs/ostd`, `libs/wire`,
  the vendored `virtio-drivers`, … — `scripts/tier1-closure.generated.toml`, resolved by
  `cargo metadata --all-features` on the three bare-metal targets) stays within its
  reviewed ceilings of `unsafe` tokens and unsafe attributes (`global_asm!` included) in
  `scripts/tcb-unsafe-baseline.toml`, and so does the Tier-1 total of cells plus closure;
  third-party crates and proc-macros are pinned by name, version and full source string (a
  git dependency's revision included) in the generated file, which CI regenerates and diffs
  (Spec 16 F1b);
- a crate whose allowlist entries include `c-ffi` or `legacy-raw-ptr` declares manifest
  class FFI or UNTRUSTED, so it loads in Tier 2, unless a signed `[[tier_exception]]` names
  it and the manifest class it accepts (tier-class gate, Spec 16 F1c). The class is read
  from source on every run and, before signing, from each target ELF's
  `__ViCell_manifest` exactly as the loader reads it: the binary decides, so a
  `#[cfg]`-gated or never-expanded manifest, or a binary target without one, is refused.
  A target is attributed to its crate by file name (a `cells/` crate's binary target); a
  renamed copy (such as `init`) cannot be attributed and is only reported, a documented
  residual;
- the toolchain matches the pinned `rust-toolchain.toml` (policy F5);
- the controlled CI job binds the target ELF to the checked build before releasing the
  production signing key. The CLI itself accepts target paths and does not establish
  that build-to-artifact provenance.

The production signing key must live in CI/KMS, not on developer machines; key policy and
artifact provenance are the guarantee. The current `[0x43; 32]` dev seed and
`--unchecked-dev-signature` route are test fixtures: the kernel cannot distinguish their
signatures from a checked dev signature, so they establish no adversarial provenance.

Current admission behavior is explicit (2026-10-31, plan `261031-1200-test-release-key-separation`):

- signature present but invalid for this kernel's trust anchor: deny in every build;
- valid signature: Tier 1, or Tier 2 when the manifest declares FFI/UNTRUSTED;
- signature absent, dev/test posture: admit to Tier 2;
- signature absent, release posture (`release-signing-key`): deny. Every release cell is
  signed; its manifest class selects the tier.
- Trust anchors are exclusive per kernel: `dev-signing-key` (test images) or
  `release-signing-key` (provisioned public key file; private key is a CI secret). A
  kernel without either uses a `[0u8; 32]` placeholder that fails every verify. A
  dev-signed artifact therefore cannot run on a release kernel, and the signer no
  longer refuses names: test images sign every cell, `cellbox` included.

### 2.2 Tier 2 admission — nothing

That is the point of the accepted design. A third-party developer would build a normal
cell ELF with the public SDK, omit a platform signature, withhold source, and run at native
speed behind an MMU wall once Tier 2 exists. `unsafe` in a Tier-2 cell would be confined
to that domain's direct CPU-memory mappings, not made harmless: kernel bugs, granted pages,
DMA outside a qualified fence, and unsafe syscall-pointer handling remain separate attack
surfaces. The costs a Tier-2
cell pays, relative to Tier 1: address-space switch at its scheduling boundary
(ASID-tagged, no full TLB flush — VF2/Pioneer/RK3588 all have MMU+ASID; none has
MTE/PKU, which is why page tables are the mechanism), and copied IPC instead of
zero-copy grants. Explicit domain grants require their own mapping and revoke protocol;
they are not the current SAS `GrantShare` mechanism. Verification buys performance; it is
never a license to exist. The page-table, scheduler, architecture, IPC, DMA, test, and
rollback gates are defined by `docs/specs/22-native-domain-cell-implementation-gate.md`.

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
  provides with zero speed penalty and no new runtime to maintain. WASM may still exist
  as a tool/runtime-adjacent host path, but not as a first-class execution tier.
- **Client-side attestation without TEE** — forgeable by construction; see §1.
- **Mandatory build service for third parties** — rejected: source-disclosure
  requirement would strangle the ecosystem; Tier 2 removes the need.

## 4. Consequences

- The kernel loader has a tier decision point: the governed spawn verifies the signature and
  reads the manifest class (`kernel/src/loader/governed_spawn.rs`), and
  `task::launch::publish_prepared` admits a domain-class artifact into a private root through
  `domain_admission::admit_for_launch` or denies it with no SAS fallback (Spec 22 §4). Default
  (development) builds route an absent signature to Tier 2; release builds deny it.
- A production profile must provision the kernel's immutable cell-signing public key,
  enable `signing-required` and `policy-required`, remove dev-key/weak-RNG features, bind
  checked source to the signed artifact, and test unsigned, stripped, wrong-key,
  dev-key, tampered, and unchecked-dev-signed negative cases. The `release-posture` feature
  selects that set, and the build refuses any dev, test, recovery or fixture feature beside it
  (`kernel/src/posture.rs`). Its three trust anchors come from provisioned key files at build
  time (`CELLOS_RELEASE_{CELL,POLICY,OWNER}_PUBLIC_KEY`, `kernel/build.rs:115-175`), so they are
  compiled into the image, not boot-provisioned data.
- Owner consent is a separate mechanism from fleet policy. `/POLICY.BIN` is verified
  against the fleet root key, so it expresses the *operator's* capability ceiling, not the
  machine owner's admission decision; see `18b-cell-admission-consent-adr.md` §4.
- Spec 19 Layer B exists on the architectures the generated
  [arch-status](../generated/arch-status.md) table marks for Tier-2 admission. No installer
  exists; when one does, `18b-cell-admission-consent-adr.md` §5 governs its Tier-1/Tier-2 choice.
- Secure boot remains required to anchor the kernel and its embedded trust key.
- `GrantShare` to/from a Tier-2 cell maps the grant into the domain's table explicitly;
  `DataPtr`-style raw pointers (`GetFile`) are unrepresentable across the tier boundary
  — consistent with their planned removal (midori-lessons phase 06).
- The scheduler's context-switch path is tier-aware: `sched::domain_switch::RootDecision`
  writes a root only when crossing domains; SAS→SAS switches keep the no-root-write path.
- Spec 02 §6 and Spec 17 §12 carry the Tier-2 memory and IPC addenda.
- WASM crates are still present in the workspace. Their retain-vs-remove disposition and
  Tier-2/runtime qualification remain unresolved; do not describe removal as landed.

## 5. Cross-references

| Topic | Document |
|-------|----------|
| Build-time attestation vs install-time consent | `docs/specs/18b-cell-admission-consent-adr.md` |
| rustc as TCB, policies F1–F7 | `docs/specs/16-rustc-tcb.md` |
| Hardware isolation layers (W^X, MPK, domain tables) | `docs/specs/19-hardware-isolation-layers.md` |
| Tier-2 implementation gate | `docs/specs/22-native-domain-cell-implementation-gate.md` |
| Signing pipeline (current) | `scripts/sign-cell.py`, `scripts/lib-sign-cells.sh`, `kernel/src/signing.rs` |
| Capability model / manifest | `docs/specs/01-core.md` |
| Hypervisor tier (silo) | `.agents/260607-1420-h-ext-hypervisor-cap/` (plan record) |
