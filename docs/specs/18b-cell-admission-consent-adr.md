# ADR: Cell Admission — Build-Time Attestation vs Install-Time Consent

**Date**: 2026-08-01 | **Status**: Accepted design, NOT implemented | **Authors**: Cellos core team

Amends `docs/specs/18-cell-trust-tiers.md` §2.1 and §4.

---

## Decision

Split the single mechanism currently called "signing" into two, because it is being
asked to carry two claims that no one party can make:

1. **Safety and provenance are attested at build time**, by a publisher key held in
   CI/KMS. `cellos-sign` stays what it is — a source-tree policy checker — and does
   not move onto target machines.
2. **Consent is recorded at install time**, by the machine owner, as a hash-pinned
   admission record. It is *not* a second signature embedded in the ELF.
3. **The local decision may only narrow, never widen.** No valid publisher signature
   means no Tier 1, whatever the owner clicks.
4. **Owner authority gets its own trust anchor**, distinct from the fleet root key,
   and the owner's private key does not have to live on the device.
5. **The Tier-1/Tier-2 install choice is gated on Spec 19 Layer B** and must not be
   presented before that mechanism exists.

---

## Context

Spec 18 §2.1 defines the platform signature as meaning *"approved by a pipeline that
enforced F1/F5"*. That is a claim about a property of the artifact, and Spec 18 §1.2
already establishes the hard limit: no algorithm verifies the memory-safety of a
compiled ELF. The property is checkable only where source is visible — at build time.

Two questions are being answered by one signature:

| | Claim A — safety / provenance | Claim B — authorization |
|---|---|---|
| Assertion | built under F1/F5; no `unsafe` outside the reviewed allowlist | the owner of *this* machine permits this binary in *their* SAS |
| Who can assert it | only a party holding the source (CI/KMS) | only the machine owner |
| When it is checkable | build | install |
| Key | one publisher key | per machine |

Tier 1 needs both. A, because the SAS is shared-fate: one `unsafe` byte in one cell
reaches every other cell's heap and stack (Spec 19 §2 Layer A closes code and
constants only). B, because in a single-address-space system the supplier is not the
party entitled to decide what shares an address space with the owner's data.

Two facts in the current tree make the gap concrete:

- `kernel/src/policy.rs` describes itself as *"the headless consent mechanism"*, but
  `/POLICY.BIN` is verified against `FLEET_ROOT_PUBKEY` (`policy.rs:78-87`). An owner
  without the fleet private key cannot author a policy. Today's consent belongs to the
  fleet, not to the owner. It is also keyed by path, which cannot express "this exact
  binary".
- Both trust anchors are compile-time constants (`signing.rs:29-33`,
  `policy.rs:84-87`). Rotation or revocation means rebuilding and reflashing the
  kernel on every board.

---

## Point 1 — Build time is the only time Claim A is checkable

`cellos-sign` reaches its verdict by parsing the source tree: `policy.check()` scans
crate roots and tracked files under `cells/` for the F1 attribute and `unsafe` tokens,
and `toolchain.check()` reads `rust-toolchain.toml` for F5. Neither input exists on a
target machine.

Any device-side tool that keeps the name "sign" but drops those inputs produces a
signature that attests nothing — which is precisely the failure Spec 18 §1.1 opens
with: *a signature proves only possession of its key*. Moving signing to install time
does not relocate the safety claim; it deletes it and leaves the ceremony behind.

The confidential-build path (Spec 18 §2.3) remains the only route by which a
third-party developer obtains Claim A without disclosing source. That is unchanged by
this ADR.

---

## Point 2 — Consent is an admission record, not a second signature

The owner's decision is recorded as an entry in a machine-local admission store keyed
by `SHA-256(elf_bytes)` — the digest the kernel already computes at
`measurement_log.rs:56`. The artifact is not modified.

Re-signing on the device was the obvious alternative and it loses on four counts:

- **Measurement divergence.** `measure()` hashes the whole ELF file, not just PT_LOAD.
  Embedding a per-machine signature with `objcopy` therefore changes the measured
  digest on every machine, and the rolling aggregate (`agg = SHA256(agg || hash)`)
  diverges across the fleet. Remote attestation loses the thing it exists to
  establish: a known-good software set. Note that `__ViCell_sig` sitting outside
  PT_LOAD keeps the *signature payload* stable — it does not keep the *measurement*
  stable, and it is the measurement that attestation signs.
- **A signing secret on the attacked machine.** Spec 19 §1 records the hardware:
  VF2, Pioneer and RK3588 have MMU and ASIDs, no MTE, no PKU, and no secure element
  worth anchoring a key in beyond RK3588's eFuse. A local private key is protected by
  little more than secure boot and the kernel's own memory.
- **A writable artifact is required.** `objcopy` at install rules out read-only and
  dm-verity-style images, and adds a write path to a binary that has already been
  verified.
- **Revocation stays awkward.** Un-signing means rewriting the ELF. Deleting a row
  does not.

The store buys back what the per-machine key was wanted for: the record is local, so a
binary admitted on machine A carries no authority on machine B.

---

## Point 3 — Narrow, never widen

Admission is `publisher signature ∧ owner record`. The owner's entry can withhold Tier
1 from a validly signed binary; it can never grant Tier 1 to a binary that has no
valid publisher signature.

Stating it as an invariant is not pedantry — the inverted design is an easy mistake
with a severe payload. If the installer signed with a local key and the kernel held
only the local public key, the publisher signature would drop out of the admission
decision entirely. The machine would then admit any ELF the owner clicked through,
`unsafe` and all, straight into the shared SAS. Acceptable for an owner-chosen Tier 2
behind an MMU wall; catastrophic as Tier 1 admission.

The system already works this way elsewhere: effective capabilities are
`manifest ∩ spawner ∩ policy` (`policy.rs:1-6`), and a spawned cell's authority only
ever narrows. Admission joins that rule rather than inventing a second one.

---

## Point 4 — The owner anchor

The kernel gains a third trust anchor, provisioned rather than compiled: an owner
Ed25519 public key, separate from `CELL_SIGNER_PUBKEY` (publisher) and
`FLEET_ROOT_PUBKEY` (fleet policy). The admission store is a blob signed by the owner
key, verified with the same discipline `policy.rs` already applies — verify first on
length alone, then parse, malformed means fail-closed, never panic on the boot path.

The owner's private key does not need to be on the device. It can live on the
administrator's machine or removable media, and the store blob is authored there and
delivered. The device then holds **no signing secret at all**, which is the property
that made on-device re-signing unattractive in Point 2. What is per-machine is the
*store*, and the owner key that authorizes it — not a key the running system must
defend.

Three anchors is one more than today, and that is the point: each names a distinct
authority. Collapsing publisher and owner into one key is what produced the gap in the
Context section.

---

## Point 5 — Sequencing, and the false-choice hazard

Tier 2 has no mechanism. The kernel holds one root page table
(`memory/paging.rs:38`) and no context switch writes `satp`/`TTBR0`/`CR3`. "Install
unsigned to run at Tier 2" today resolves to either *admitted to the shared SAS with
no wall* (default G1 features) or *denied* (`signing-required`). There is no third
outcome to select.

An installer that offers "Tier 1 or Tier 2" before Spec 19 Layer B lands is therefore
worse than one that offers nothing: the owner believes they chose containment and
received the SAS. Informed consent obtained against a false description of the
mechanism is not consent.

Until Layer B, the honest prompt has two outcomes: admit to the SAS with the
shared-fate consequence stated plainly, or decline to install. The Tier-2 branch is
compiled out, not greyed out.

---

## Tooling split

| Tool | Runs | Holds | Answers |
|------|------|-------|---------|
| `cellos-sign` | CI | publisher key (KMS) | Claim A — F1/F5 held over this source tree |
| `cellos-install` | target device | nothing secret | Claim B — the owner permits this digest |

`cellos-install` verifies the publisher signature, reads `__ViCell_manifest` and
displays the capabilities the cell requests, then records the owner's decision. The
manifest is inside the signed payload (`signing.rs:109-114`), so displaying it is
honest: an attacker cannot alter the request without invalidating the signature that
gates the prompt.

## Update streams

A digest-pinned entry re-prompts on every cell update, which is correct by default and
tiring in a fleet. The escape is opt-in and narrow: the owner may admit *publisher key
+ cell identity from the manifest* above a version floor, for one named cell. This is
a deliberate loosening and the prompt must say so — it delegates future decisions to
the publisher for that cell.

---

## Rejected alternatives

- **Per-machine re-signing of the ELF at install (`objcopy` on device).** Rejected on
  the four grounds in Point 2, the decisive one being that `measure()` digests the
  whole file, so per-machine signatures fork the attestation aggregate and destroy the
  fleet's known-good set.
- **An install-time signature replacing the build-time one.** Rejected: it deletes
  Claim A. The kernel would hold only a local key, and owner consent alone would admit
  unverified native code to the shared SAS (Point 3).
- **Status quo extended — one fleet key, no owner consent.** Rejected: a single leaked
  signed artifact becomes a fleet-wide SAS admission ticket; revocation requires
  reflashing every board because the anchor is a `const`; and the owner has no way to
  refuse a binary their supplier approved.
- **Extending `/POLICY.BIN` instead of a separate owner-anchored store.** Rejected:
  one blob under one anchor cannot express two authorities — the fleet operator's
  capability ceiling and the owner's admission decision have different authors,
  different rotation schedules, and different consequences when absent. `POLICY.BIN`
  is also path-keyed, and a path cannot name a specific binary.
- **Making `cellos-sign` the installer.** Rejected: it requires either shipping source
  to the device (Spec 18 §3 rejects the source-disclosure requirement for third
  parties) or removing the checks that give its signature meaning (Point 1).

---

## Consequences

- **Blocking prerequisite for everything here and for Spec 18 §4:** the kernel's trust
  anchors must become boot-provisioned data rather than `const` values
  (`signing.rs:29-33`, `policy.rs:84-87`). Neither this design nor the fleet posture in
  Spec 18 is deployable without it.
- A new kernel module owns the admission store: verify-then-parse over an
  owner-signed blob, digest lookup, fail-closed on malformed input.
- The loader gate at `loader.rs:119-153` becomes two-stage — publisher signature, then
  admission lookup. The digest is needed at the gate, so `SHA-256(elf_bytes)` moves
  ahead of the current `measurement_log::measure` call at `loader.rs:202` and is passed
  in rather than recomputed.
- New audit events for owner-admitted and owner-denied spawns, alongside the existing
  `CellSignatureVerified` / `CellSignatureFailed` / `CellMeasure`.
- Negative tests to add: unsigned, wrong publisher key, tampered, valid publisher
  signature with no admission entry, admission entry with a stale digest, unsigned
  admission store, admission store signed by the wrong owner key, malformed store.
- Spec 18 §2.1 and §4 are amended to reference this ADR and to name Claim A and Claim B
  separately. `docs/security-model.md` — the STRIDE Tampering row and the `hc-adr`
  consent language — needs revision when this lands, not before.
- `docs/README.md`'s spec index stops at 17 and already omits Specs 18–21; that gap is
  pre-existing and out of scope here.

---

## Cross-references

| Topic | Document |
|-------|----------|
| Trust tiers, what the signature attests | `docs/specs/18-cell-trust-tiers.md` |
| Layer B (per-domain page tables — the Tier-2 mechanism) | `docs/specs/19-hardware-isolation-layers.md` |
| rustc as TCB, policies F1–F7 | `docs/specs/16-rustc-tcb.md` |
| Capability model / manifest | `docs/specs/01-core.md` |
| Current signing pipeline | `scripts/cellos_sign/`, `scripts/sign-cell.py`, `kernel/src/signing.rs` |
| Fleet operator policy | `kernel/src/policy.rs`, `scripts/sign-policy.py` |
| Measurement log and attestation aggregate | `kernel/src/measurement_log.rs`, `libs/attestation/` |
