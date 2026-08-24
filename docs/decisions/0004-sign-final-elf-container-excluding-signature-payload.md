# ADR-0004: Sign the final ELF container except the signature payload

**Date**: 2026-08-24  
**Status**: Accepted  
**Deciders**: Cellos maintainer

## Context

A Cell signature must bind the artifact that the loader will actually interpret,
not only the parts that are normally mapped into memory. The former verification
payload covered `PT_LOAD` bytes together with `__ViCell_manifest`. That left
load-affecting ELF representation outside the signed boundary, including ELF
headers, section headers and names, offsets, and `.rela.dyn` relocation
metadata. An attacker who could alter one of those unsigned bytes could change
how the artifact is interpreted without changing the signed payload.

`CELLOS-LOADER-SIG-001` records this as a Critical gap. The verifier constructs
its payload from the final ELF byte sequence with only the 64-byte payload of
the `__ViCell_sig` section omitted. The section header that locates and
describes that payload remains authenticated. The signing tool creates a
zero-filled placeholder, signs that stable final container excluding exactly
those 64 bytes, then overwrites only the placeholder with the resulting
signature.

This ADR records the signed-envelope boundary. It does not define key custody,
release provenance, or the policy by which an environment admits signed and
unsigned artifacts.

## Decision Drivers

- Bind all bytes that can influence loader interpretation of a final ELF
  container, not merely mapped segment contents.
- Solve the signature self-reference without leaving a larger mutable region.
- Keep verification byte-oriented and deterministic rather than depending on a
  second ELF serializer or a lossy normalization rule.
- Make modifications to relocation metadata, headers, section metadata, names,
  and offsets invalidate the signature.
- Reject malformed or ambiguous signature layouts rather than guessing which
  bytes or signature should be trusted.
- Keep artifact-integrity semantics separate from production key provisioning,
  provenance, and admission posture.

## Considered Options

### Option A: Sign `PT_LOAD` bytes plus `__ViCell_manifest` only

This model signs executable/loadable segment bytes and the manifest while
leaving the remainder of the ELF container outside the signature.

- **Pro**: The payload is relatively small and follows the most obvious runtime
  image boundary.
- **Pro**: It avoids self-reference if the signature section is not in a load
  segment.
- **Con**: ELF and program-header fields that affect loading are mutable.
  Section headers, section names, offsets, and `.rela.dyn` metadata may also
  change without invalidating the signature.
- **Rejected because**: The signed object is not the actual loader-interpreted
  container. This is the Critical exposure identified by
  `CELLOS-LOADER-SIG-001`.

### Option B: Exclude the entire signature section and its header, or use a detached signature

This model would omit all signature-section bytes, including the section header
that identifies the signature, or would store the signature outside the ELF
file as a detached companion artifact.

- **Pro**: Excluding an entire section makes self-reference straightforward;
  detached signatures avoid it entirely.
- **Pro**: A detached signature can be transported independently of the ELF.
- **Con**: Excluding the header makes the location, size, and interpretation of
  the signature section mutable. Excluding a whole section also creates a
  larger unauthenticated region than necessary.
- **Con**: A detached signature weakens the single-artifact binding and adds
  pairing, distribution, and substitution concerns between the ELF and its
  companion signature.
- **Rejected because**: The final ELF container itself must be the authenticated
  object. The signature metadata must therefore remain covered, and the
  self-reference exception must be no larger than the signature bytes that
  cannot sign themselves.

### Option C: Canonically reserialize selected ELF structures before signing

This model would parse selected headers, tables, and metadata, normalize or
reserialize them into a canonical form, and sign that derived representation.

- **Pro**: It could intentionally ignore byte-level layout differences deemed
  semantically irrelevant.
- **Pro**: It offers a structured representation rather than a raw-byte
  envelope.
- **Con**: It creates a second, security-critical ELF interpretation and
  serialization contract that must exactly track loader behavior.
- **Con**: Defining which structures are selected and which layout differences
  are harmless is error-prone; omitted or ambiguously normalized fields could
  recreate an unsigned influence path.
- **Rejected because**: The objective is to authenticate the exact final
  container, including its layout and metadata, rather than a separately
  reconstructed approximation of it.

### Option D (chosen): Sign the final ELF byte container except the one 64-byte `__ViCell_sig` payload

This model treats the final ELF file as the signed envelope. Verification uses
all final file bytes in their stored order except precisely the 64-byte payload
range of the single designated `__ViCell_sig` section. The section header and
every other byte, including ELF/program/section headers, section names,
offsets, `PT_LOAD` contents, the manifest, and relocation metadata, remain in
the signed input.

- **Pro**: It binds the complete artifact the loader consumes while removing
  only the unavoidable self-signature cycle.
- **Pro**: Any mutation of `.rela.dyn` or other relocation metadata invalidates
  the signature, as do changes to headers, names, offsets, and section
  descriptors.
- **Pro**: The signing and verification inputs are stable, byte-exact, and do
  not require canonical reserialization.
- **Con**: Producers and verifiers must enforce the exact one-section,
  exact-64-byte layout contract.
- **Chosen because**: It provides the narrowest practical exception to
  whole-container authentication and directly closes the mutable-metadata gap.

## Decision

Cellos will sign and verify the **final ELF container**, omitting only the
64-byte payload of exactly one `__ViCell_sig` section from the verification
input.

The signature section header is part of the signed envelope. Consequently, its
identity, location, declared size, and all surrounding ELF structure are
protected; the excluded range is only the 64-byte value whose cryptographic
contents cannot include a signature of themselves.

A signing producer must form the final container with a zero-filled 64-byte
signature placeholder, calculate the signature over that container with the
placeholder payload excluded, and replace only those 64 bytes. The resulting
signature is verified against the same byte sequence and exclusion rule.

Verification and admission must treat an absent, malformed, wrong-sized, or
duplicate `__ViCell_sig` layout as invalid rather than selecting a candidate or
silently enlarging the excluded area. Multiple possible signature payloads
would make the authenticated boundary ambiguous and are therefore not a valid
signed artifact.

## Consequences

### Positive

- The signed boundary now includes ELF headers and program headers, section
  headers and names, offsets, manifest data, segment contents, and relocation
  metadata.
- Altering `.rela.dyn` or other relocation data now changes authenticated bytes
  and invalidates the signature.
- The signature section's own metadata remains authenticated, preventing an
  attacker from relocating, resizing, or redefining the signature payload
  outside the verifier's contract.
- Signing and verification operate on the exact final bytes, avoiding a second
  canonical ELF model.

### Negative / Risks

- The signing pipeline is order-sensitive: post-signing transformations of any
  covered byte invalidate the signature.
- Tooling and loader validation must agree exactly on the one 64-byte exclusion
  and must reject malformed or duplicate signature layouts.
- This decision establishes artifact integrity only. It does not itself prove
  who authorized a signature or whether the artifact is admissible in a given
  environment.

### Non-goals

- **Production key provisioning is not completed by this decision.** Fleet key
  generation, storage, rotation, distribution, and trust-anchor provisioning
  remain separate production gates.
- **Provenance-envelope completion is not provided by this decision.** Build,
  source, release, and supply-chain provenance remain separate concerns.
- **Unsigned-development admission is unchanged.** This ADR does not change
  development-mode behavior or establish a signing-required production
  admission posture; requiring signatures for production remains a separate
  policy and rollout gate.

## References

- `kernel/src/signing.rs` — verifier construction of the payload from final ELF
  bytes excluding only the 64-byte `__ViCell_sig` payload.
- `scripts/sign-cell.py` — creation of the zero-filled signature placeholder,
  signing of the stable final container, and replacement of only its 64-byte
  payload.
- `docs/roadmap/open-risk-register.md` — `CELLOS-LOADER-SIG-001`, the Critical
  risk created by signing `PT_LOAD` data plus the manifest while leaving
  load-affecting ELF metadata mutable.
