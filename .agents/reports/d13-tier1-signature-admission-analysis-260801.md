# D13 — Tier-1 trust: signature, path, or deployment posture?

**Ruling:** Recommendation A approved and applied 2026-08-01. Specs 12/18, the security
model, roadmap, system architecture, project changelog, and decision docket now separate
the shipped dev/test verification hook from future fleet-secure admission. No runtime,
ABI, key, Cargo-feature, or secure-boot implementation changed.

**Date**: 2026-08-01 · **Question from the docket**: is Spec 12's statement that
"trusted" means a `/bin/` path superseded by the Ed25519 spawn gate, and does the current
dev-seed key satisfy Spec 18's "Tier 1 = signed only" admission argument? · **Method**:
inspect the common loader gate, build features, key source, signing tools, image lanes,
manifest/capability logic, Tier-2 status, CI references, secure-boot status, and relevant
specifications and history.

## Answer first

**Spec 12 is partly superseded, but the replacement claim in Spec 18 and the security
model is too strong. The current system is not a production-grade "Tier 1 = signed only"
system, and the dev-seed key does not satisfy that security premise.**

The repository now has real Ed25519 verification in the common spawn gate, so signing is
no longer "spec-only." However:

1. `signing-required` is off by default, and no checked-in CI/build profile enables it;
   an ELF with no signature section is admitted to the shared SAS.
2. Removing the signature section converts a tampered signed ELF into an unsigned ELF,
   which the default posture permits. Signing therefore does not prevent disk-image
   replacement in the default build.
3. The dev private seed is public and reproducible. Anyone can mint a kernel-valid dev
   signature, including through the explicit unchecked-dev test route.
4. Disabling the dev-key feature selects `[0u8; 32]`, not a provisioned fleet key. With
   `signing-required` this fails closed by refusing every cell; it is not a usable
   production posture.
5. Signature validity does not select a memory tier. Every admitted native cell currently
   enters the same SAS; Tier 2 is not implemented.
6. `/bin/` still controls privilege classification, legacy grants, operator-policy lookup,
   and trusted-core paths. It is an authorization label, not cryptographic provenance.

The honest current claim is: **the signature mechanism and checked image-building workflow
exist; default G1 admission still relies on operational trust in every native cell.**

## 1. Four distinct decisions are being conflated

```text
ELF bytes
  |
  +-- Signature present? ---- invalid -> DENY
  |         |
  |         +-- absent + signing-required -> DENY
  |         +-- absent + default build    -> ALLOW
  |
  +-- Path label (/bin or /mem) -> capability/legacy-policy ceiling
  |
  +-- Manifest tier ------------> x86 PKRU value only (no PTE enforcement)
  |
  +-- Memory mapping -----------> shared SAS for every admitted native cell
```

A signature answers which key signed the covered bytes. `/bin/` answers which path-based
authority may be considered. F1 is a source/build-pipeline property. Tier 1 versus Tier 2
answers which page table exposes memory. Current documents sometimes treat those four
answers as one decision, but the implementation does not.

## 2. What the loader actually enforces

All current spawn sources converge on `loader::spawn_gated`:

- `spawn_from_path` reads boot/P2-table bytes and calls it
  (`kernel/src/loader.rs:95-100`);
- caller-supplied `SpawnFromMem` bytes are reduced to a `/mem/` label and call the same
  gate (`kernel/src/loader/mem_spawn_gate.rs:30-64`).

At `kernel/src/loader.rs:119-153` the decision is:

- signature section present and invalid: always deny;
- section absent and `signing-required` enabled: deny;
- section absent and default posture: permit.

`kernel/src/signing.rs:35-40` defines `signing_required()` solely from a Cargo feature.
`kernel/Cargo.toml:58-79` keeps that feature outside defaults. The default feature set is
`dev-policy-key`, `dev-signing-key`, and `dev-weak-rng`.

Repository-wide search finds no checked-in workflow or ordinary image build enabling
`signing-required`. The July 28 changelog likewise records that no CI job enabled it and
only describes a manual verification build (`docs/project-changelog.md:263-300`). Thus the
security model's unqualified statement that Tier-1 "signed cells only" is enforced is
false for the normal build posture.

## 3. Default signing is downgradeable to unsigned

Image lanes now sign their embedded cells through `cellos-sign`, which is useful: an
unchanged signature catches accidental corruption and an invalid signature is denied.
But absence is a separate branch.

An attacker able to replace a disk or ramdisk ELF can remove `__ViCell_sig` entirely.
`extract_sig` then returns `None`, and the default loader takes the explicit "unsigned
cell permitted" branch (`kernel/src/loader.rs:139-152`). Therefore the current default
does not mitigate malicious ELF replacement even though the images happen to contain
signatures when built normally.

This directly contradicts `docs/security-model.md:34`, which marks disk-image ELF
injection as mitigated by the unified signature gate without qualifying the
`signing-required` feature.

## 4. Why the dev seed is not a trust anchor

The dev signer seed is the literal public value `[0x43; 32]`
(`scripts/sign-cell.py:61-63`). The generated public key exactly matches the kernel's
default `DEV_CELL_SIGNER_PUBKEY` (`kernel/src/signing.rs:19-30`). This is intentional for
reproducible QEMU/dev images, but it means anyone with the repository holds the private
key material.

The low-level signer even exposes `--unchecked-dev-signature`
(`scripts/sign-cell.py:261-300`). Its warning correctly says such a signature attests
nothing. The kernel cannot distinguish that signature from one emitted after an F1/F5
check: both verify under the same dev public key.

Consequences:

- enabling `signing-required` while retaining `dev-signing-key` proves only "signed by a
  publicly known test key";
- it does not prove first-party provenance;
- it does not prove the F1/F5 checks ran;
- it does not keep a hostile developer or disk-image modifier out of Tier 1.

The build scripts' convention that unchecked signatures "must never reach an image" is a
useful accident guard for trusted developers, not a security boundary against an attacker.

## 5. There is no usable production key configuration

With `dev-signing-key`, the kernel embeds the reproducible dev public key. Without it,
`CELL_SIGNER_PUBKEY` is `[0u8; 32]` with `TODO(prod): provisioned fleet key`
(`kernel/src/signing.rs:26-33`). No build script, generated source, immutable config, or
KMS export path replaces that constant.

The signing CLI can accept a production private seed through `--seed-hex`, but that only
changes the signer. It does not inject the corresponding public key into the kernel.
Therefore the currently described production combinations are:

- dev key + `signing-required`: boots, but is forgeable;
- zero placeholder + `signing-required`: rejects both unsigned and legitimately
  production-signed cells;
- zero placeholder without `signing-required`: admits unsigned cells but rejects any
  present non-matching signature.

None is a deployable fleet trust chain. Full secure boot is also explicitly open
(`docs/security-model.md:224-230`), so even a real cell key would not by itself establish
an end-to-end immutable root of trust for the kernel containing that key.

## 6. `/bin/` remains authorization, not provenance

Spec 12's old wording is wrong in one direction: cryptographic verification now exists.
But `/bin/` has not disappeared.

`kernel/src/loader.rs:155-180` still treats a non-`/bin/` path as a user cell that may not
declare privileged capabilities. The path also drives legacy grants, operator-policy
lookup, and trusted-core recovery. Caller-supplied bytes are deliberately relabelled
`/mem/...` so a caller cannot forge `/bin/vfs` authority
(`kernel/src/loader/mem_spawn_gate.rs:1-20`, `:66-76`).

Thus `/bin/` means "eligible for path-scoped authority," not "cryptographically trusted."
In the default posture, an unsigned ELF reached through a legitimate `/bin/` path can still
enter the SAS. Conversely, a validly signed `/mem/` ELF does not inherit `/bin/` privilege.

## 7. Signature validity does not assign Tier 1 or Tier 2

Spec 18 says the loader assigns a tier from signature verification and that an unsigned
ELF never sees another cell's pages (`docs/specs/18-cell-trust-tiers.md:30-54`). Neither is
implemented.

After the signature branch, `spawn_gated` calls the ordinary `spawn_from_mem`
(`kernel/src/loader.rs:183-192`), which installs the cell into the shared address space.
There is no signature result retained and no selection between a SAS mapper and a domain
mapper. Manifest v2's tier field only derives an x86 PKRU value
(`kernel/src/loader.rs:310-325`); D12 established that all PTEs remain key 0, so this does
not create a memory tier.

Spec 18 correctly says Tier 2 is not implemented, but its invariant is still phrased as
current fact. Actual current behavior is:

- default build: signed or unsigned native ELF may enter SAS;
- `signing-required` build: only a valid signature enters SAS;
- no build routes unsigned ELF into a private domain.

## 8. The F1 attestation wording also needs narrowing

`cellos-sign` does run F1/F5 checks before its normal signing call
(`scripts/cellos_sign/cli.py:79-105`). This is a real and valuable trusted-pipeline guard.
However, Spec 18 overstates what the tool itself binds:

- the F1 scan covers repository `cells/`; `libs/*` is trusted TCB, not scanned under the
  same rule (`scripts/cellos_sign/__init__.py:21-38`);
- `run_sign` accepts arbitrary target ELF paths after checking the current source tree;
  it does not rebuild them or cryptographically prove they came from that checked tree
  (`scripts/cellos_sign/cli.py:87-96`).

Therefore "built by a pipeline that enforced F1" is valid only when a controlled CI/KMS
pipeline also guarantees artifact provenance. It is not an intrinsic property established
by the CLI or by a dev-key signature.

## 9. Security consequence

The current documentation upgrades an implemented crypto primitive into a trust guarantee
that the deployment configuration does not provide. In the normal G1 build, every admitted
native cell must still be treated as operationally trusted because any unsigned native ELF
enters the shared SAS. A known dev key does not improve that adversarial boundary.

This does not make the signing work useless. It already provides:

- a common verification hook for every spawn source;
- tamper detection when a signature is present and not stripped;
- reproducible image-lane testing;
- a sound place to enforce a future production key policy.

It is plumbing ready for a trust anchor, not the trust anchor itself.

## 10. Recommended ruling

**Approve option A: distinguish current dev admission from a future fleet-secure posture.**

1. Amend Spec 12 §2: signing is implemented, but `/bin/` remains an authorization class;
   default G1 permits unsigned native cells, so "Tier 1 = signed only" is not current fact.
2. Amend Spec 18:
   - mark signed-only Tier-1 admission as a fleet target, not shipped default behavior;
   - state that the loader does not yet assign memory tiers from signature status;
   - keep unsigned third-party native code at "Tier 3 or refused" until Layer B ships;
   - narrow the F1 claim to a controlled pipeline and the actual `cells/` scan boundary.
3. Downgrade the security-model disk-tampering row and signed-only statement from
   `Mitigated/enforced` to partial: verification exists, but absence is allowed by default.
4. Describe the dev seed solely as a test fixture. Remove any implication that a dev-key
   signature proves provenance or F1 against an adversary.
5. Define production acceptance criteria before claiming signed-only:
   - immutable/provisioned fleet public key, with no zero placeholder;
   - `signing-required` and `policy-required` in a named production profile;
   - dev key and weak RNG features absent;
   - controlled build-to-artifact provenance before KMS signing;
   - negative CI/runtime tests for unsigned, signature-stripped, wrong-key, dev-key,
     tampered, and unchecked-dev-signed ELFs;
   - secure-boot/root-of-trust plan for the kernel that embeds the cell key.

No code change was made by the D13 ruling. Key provisioning, production feature bundling,
and Tier-2 routing need an implementation plan and explicit security review.
