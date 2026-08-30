# ADR-0010: Use deterministic CBOR and COSE_Sign1 for VF2 root-stream manifests

- **Status:** Accepted
- **Date:** 2026-08-29

## Context

The selected `DEV_REFERENCE` authority lane boots a VisionFive 2 v1.3B through
immutable JH7110 UART/XMODEM into a bounded SRAM loader. The lane must accept
exactly one authenticated OpenSBI/DTB/Cellos/VIFS bundle and reject substitution,
truncation, overlap, overflow, stale or repeated authority requests, unknown
metadata, and trailing transfer data before copying or executing a component.

The repository has no root-stream bundle format. It does have a verify-only
Ed25519 precedent in the kernel, but no CBOR or COSE dependency. The loader is a
constrained `no_std`, no-allocation parser, so selecting an extensible encoding
without freezing a strict profile would create multiple encodings, unbounded
input, and ambiguous signature preimages.

The loader has no independent durable replay floor. Freshness is therefore a
joint control: the protected authority owns monotonic boot/request state and is
the sole electrical UART sender; the signed payload binds that state; physical
evidence must prove that no other sender can replay an older valid object. A host
parser or signature test cannot claim physical replay resistance.

## Decision Drivers

- Use a standard authenticated object format with one byte-exact signature input.
- Keep hostile parsing bounded, deterministic, no-allocation, and panic-free.
- Bind the exact approved loader, authority/device, boot request, component
  order, addresses, lengths, and SHA-256 digests.
- Preserve an explicit `DEV_REFERENCE` marker and evidence ceiling.
- Separate signed-manifest framing from component bytes without a self-hash.
- Reuse verify-only Ed25519 rather than introduce boot-path ECDSA/DER rules.
- Make every unsupported CBOR/COSE feature fail closed; no negotiation.

## Considered Options

### Option A: Fixed binary manifest with Ed25519

- **Pro:** Smallest loader parser and simplest exact bounds.
- **Pro:** Reuses the repository's verify-only Ed25519 precedent.
- **Con:** Every additive field requires a new binary version and coordinated
  offset changes.
- **Rejected because:** The selected lane needs a standard externally inspectable
  envelope while retaining a frozen constrained profile.

### Option B (chosen): Deterministic CBOR payload in tagged COSE_Sign1 with Ed25519

- **Pro:** RFC-defined signature structure and deterministic encoding rules.
- **Pro:** Integer-keyed schema remains compact and inspectable with standard
  tooling.
- **Con:** Requires a strict CBOR/COSE parser and negative coverage for alternate
  encodings, tags, maps, and lengths.
- **Chosen because:** A closed profile gives standards-based interoperability
  without admitting generic CBOR or algorithm agility into the loader.

### Option C: Fixed binary manifest with P-256

- **Pro:** Shares an algorithm family with the protected relay identity.
- **Con:** Adds a second boot-path verifier shape and requires fixed `r||s` or DER,
  curve, and low-S rules unrelated to the UART bundle.
- **Rejected because:** Algorithm-family reuse does not offset parser and key-role
  coupling; boot authorization is an independent capability.

### Option D: Unsigned manifest with component hashes

- **Pro:** Deterministic and easy to reproduce.
- **Con:** An attacker can replace both components and their hashes.
- **Rejected because:** Hashes provide integrity only after an authenticated root
  binds them.

## Decision

### Outer bundle

The byte stream after the immutable SRAM loader is:

```text
cose_length:u32be || tagged_cose_sign1[cose_length] || component_region
```

`cose_length` is untrusted framing only. It must be nonzero and within the
compile-time manifest bound before buffering. The loader receives the stream as
one new XMODEM-1K transfer after BootROM's loader transfer. The logical stream
length is `4 + cose_length + component_region_length`; bytes from that boundary
to the end of its final 1,024-byte data block must all be `0x1a`, no later data
block is accepted, and the receiver must complete the XMODEM EOT handshake
before component copy or execution. Missing EOT, a non-padding trailing byte, or
an additional data block fails sealed. Bytes after a completed EOT are outside
the admitted transfer and are excluded by the physical sole-sender gate, not by
a loader replay claim.

The signed payload carries `component_region_length`; the component region is
exactly that many logical bytes and has no trailing data.

Component offsets are relative to the start of `component_region`, avoiding any
manifest-length or signature self-reference. Descriptors are contiguous in the
required order: the first offset is zero and every later offset equals the prior
`offset + length`.

### COSE profile

1. The object is tagged `COSE_Sign1` with CBOR tag 18. Untagged objects fail.
2. The `COSE_Sign1` array has exactly four elements: protected bstr,
   unprotected map, embedded payload bstr, and signature bstr.
3. The protected map is deterministic CBOR and contains exactly:
   `{1: -8, 4: key_id}` where `-8` is COSE EdDSA and `key_id` is the full
   32-byte `SHA-256` digest of exactly the 32-octet RFC 8032 encoded Ed25519
   public key, with no DER, COSE_Key, length prefix, or other wrapper.
4. The unprotected map is empty. Detached payloads, countersignatures, critical
   headers, alternate algorithms, and additional headers fail.
5. The signature is exactly 64 bytes and uses pure Ed25519 per RFC 8032/9053.
6. The `Sig_structure` is the RFC 9052 `Signature1` structure over the exact
   protected bstr and payload bytes with external AAD
   `cellos.vf2-root-stream.manifest/v1`.
7. The loader contains exactly one approved public key and expected `key_id`.
   There is no key search, algorithm negotiation, or fallback.

### Deterministic CBOR payload

The embedded payload is one RFC 8949 core-deterministic map. Preferred shortest
integer/length encodings are mandatory; maps use bytewise lexicographic ordering
of deterministically encoded keys; indefinite lengths, floats, negative payload
integers, duplicate/unknown keys, optional fields, extra tags, and trailing CBOR
items fail.

The top-level map contains exactly these integer keys:

| Key | Value |
|---:|---|
| 1 | schema version `1` |
| 2 | text string `DEV_REFERENCE` |
| 3 | `device_id` bstr, 32 bytes |
| 4 | `authority_id` bstr, 32 bytes |
| 5 | nonzero `boot_epoch` uint64 |
| 6 | nonzero `request_id` uint64 |
| 7 | `approved_loader_sha256` bstr, 32 bytes |
| 8 | `component_region_length` uint64 |
| 9 | `entry_address` uint64 |
| 10 | array of exactly four component maps |

Each component map contains exactly:

| Key | Value |
|---:|---|
| 1 | kind: `1=OpenSBI`, `2=DTB`, `3=Cellos`, `4=VIFS` |
| 2 | region-relative `offset` uint64 |
| 3 | nonzero `length` uint64 |
| 4 | `load_address` uint64 |
| 5 | SHA-256 digest bstr, 32 bytes |

The array kind order is exactly `1,2,3,4`. All arithmetic is checked before use.
Each range must fit the compile-time kind-specific address/size limits, ranges
must not overlap or wrap, `entry_address` must equal the admitted OpenSBI entry,
and the final descriptor end must equal `component_region_length`.

### Quarantine staging contract

The loader never streams component bytes directly into their final load
addresses. After DRAM initialization, one immutable `StagingLimits` value in the
reviewed loader defines the successfully initialized usable-DRAM aperture
`[usable_dram_base, usable_dram_end)`, a page-aligned half-open quarantine range
`[staging_base, staging_base + staging_size)`, a nonzero
`max_transfer_blocks`, and the manifest bound. The aperture is nonempty; every
addition and `max_transfer_blocks * 1024` is checked for overflow; `staging_size`
is a multiple of 1,024 and must cover the maximum concatenated XMODEM data-block
payload; and the complete staging range must be contained within the usable-DRAM
aperture before any pre-clear or receive write. A hardware loader build is
inadmissible until measured DRAM initialization tests freeze these values. Host
tests inject explicitly labeled `SOFTWARE_HARNESS` limits and cannot promote
them.

Before requesting the second transfer, the loader zeroizes the complete staging
range. Each accepted XMODEM data-block payload is then written once at
`staging_base + block_index * 1024` after the monotonic block number and checked
end offset pass. The COSE envelope, manifest payload, component region, and
canonical final padding remain in this range; parsers and hashers borrow bounded
slices rather than copying attacker-controlled lengths.

The staging range must be disjoint from the SRAM loader image, stack, manifest
scratch, final OpenSBI/DTB/Cellos/VIFS ranges, and every admitted entry address.
All pairs are checked as half-open physical ranges with checked arithmetic before
the transfer is accepted. The loader never jumps to, exposes, or treats staging
as a final component address.

Only after signature, manifest semantics, transfer completion, padding, and all
four component digests pass may the loader copy the exact verified component
slices from staging to their disjoint final ranges. It copies all four before
handoff, then volatile-zeroizes the complete staging range and bounded manifest,
signature, digest, and XMODEM scratch.

`StagingLimits` also fixes one reviewed cleanup profile for the exact hardware:
either the quarantine/scratch accesses are demonstrably uncached and
device-visible, or the loader cleans every touched cache line to the required
point of coherency with the platform's reviewed primitive. Both profiles finish
with a RISC-V architectural `fence rw,rw`; a compiler fence alone is
insufficient. Handoff or reset release occurs only after that visibility barrier.
If the exact VF2 cannot provide and physically evidence one profile, the lane
stops.

Validation failures before usable-DRAM aperture, staging range, and cleanup
profile acceptance leave reset asserted and must not touch the untrusted
quarantine address. Once those checks pass and pre-clear begins, every
software-detected failure performs the same full visible zeroization before
returning to the authority-controlled sealed/reset state. A power cut may
interrupt cleanup; reset remains asserted, no interrupted boot resumes, and the
next loader boot completes the same validated full visible pre-clear before
requesting or executing mutable bundle bytes.

### Verification order

1. Initialize DRAM, validate the immutable staging/final ranges and cleanup
   profile, and complete the full device-visible quarantine pre-clear before
   requesting bundle bytes.
2. Receive only a bounded, monotonic XMODEM-1K block sequence into quarantine;
   bound `cose_length` and parse only the exact COSE envelope needed to obtain
   borrowed protected, payload, and signature slices.
3. Require the exact protected/unprotected profile and verify the Ed25519
   `Sig_structure` before semantically parsing manifest fields.
4. Parse the payload under the deterministic profile; re-encoding is not an
   acceptance path.
5. Check exact device, authority, loader digest, nonzero boot/request binding,
   component order, arithmetic, final/staging disjointness, limits, entry
   address, and total logical length.
6. Receive exactly the remaining declared component bytes, require canonical
   final-block padding and a successful EOT handshake, and reject any additional
   XMODEM data block.
7. Hash and verify all four complete quarantined components.
8. Copy the exact verified slices to disjoint final ranges, visibly zeroize
   quarantine and bounded scratch through the frozen cleanup profile, execute
   `fence rw,rw`, and only then hand off. Authentication, transfer-completion,
   copy, cleanup, or visibility-barrier failure leaves execution sealed.

### Freshness and key custody

- The protected authority signs only the current monotonic
  `{device_id, authority_id, boot_epoch, request_id}` and refuses stale, repeated,
  or forked requests before emitting the bundle.
- The loader verifies the signed tuple and exact compiled identity but does not
  claim an independent persistent replay floor.
- Real replay resistance requires both authority protected-state evidence and the
  Phase 3 physical sole-sender gate.
- Deterministic development signing material may exist only in test fixtures
  marked `DEV_REFERENCE`. Real authority private keys never enter the repository,
  AP, bundle, logs, or retained captures.

## Consequences

### Positive

- Signature preimages and hostile-input rejection are byte-exact and testable.
- Component offsets avoid signature or manifest self-reference.
- Standard COSE tooling can inspect host artifacts without expanding the loader's
  accepted profile.
- The ASCII lane marker remains directly observable to production scanners.

### Negative / Risks

- A constrained CBOR/COSE parser is larger than a fixed binary parser.
- Canonical encoding, protected-header, and duplicate-key negatives become
  security-critical tests.
- Physical bounds remain provisional until exact VF2 measurements; host constants
  are `SOFTWARE_HARNESS` limits, not hardware evidence.
- Loader verification alone cannot defeat replay from another valid electrical
  sender; failure of the sole-sender gate stops the lane.

## Verification

Host `SOFTWARE_HARNESS` evidence must prove:

- two clean builds from identical inputs produce byte-identical COSE and bundle;
- tagged/untagged, protected/unprotected, algorithm, key-id, signature, external
  AAD, detached-payload, and extra-header negatives fail;
- non-preferred integers/lengths, indefinite items, reordered/duplicate/unknown
  keys, extra tags/items, wrong types, and logical trailing bytes fail;
- wrong lane/device/authority/loader digest, zero/stale modeled boot/request,
  component order, offset, overlap, wrap, size/address, entry, region length,
  component digest, truncation, non-`0x1a` final padding, missing EOT, and an
  additional XMODEM data block fail;
- empty/invalid usable-DRAM apertures and staging ranges below the aperture base
  or above its end fail before pre-clear; undersized, unaligned, overflowing, or
  overlapping staging limits fail before receive; block writes cannot escape
  quarantine; no final-range write occurs before all signature/manifest/
  transfer/digest checks pass;
- host tests prove that pre-validation failures perform no quarantine write and
  that success plus every post-validation failure performs logical volatile
  clearing in cleanup-hook order, but claim no cache/store-buffer/DRAM visibility;
- parser/verifier paths do not panic or allocate and respect compile-time input
  and stack bounds; and
- every output is marked `SOFTWARE_HARNESS` and `DEV_REFERENCE` and claims no
  BootROM, electrical, physical replay, physical zeroization, or production
  evidence.

Physical evidence remains owned by the Phase 3 failure matrix and must prove the
sole sender, fixed straps, reset control, BootROM limits, no alternate-media
execution, replay refusal backed by protected authority state, and complete
quarantine/scratch zero visibility at the required coherency point before
handoff or reset release.

## Links

- [ADR-0006](./0006-block-production-root-pending-exact-product-evidence.md) — no production root or hardware qualification is implied.
- [ADR-0007](./0007-development-first-hardware-constrained-execution.md) — software harness evidence cannot promote the lane.
- [VF2 root-stream phase](../../.agents/260826-1605-phase4-dev-reference-authority/phase-03-vf2-uart-root-stream-boot.md) — implementation and physical evidence owner.
- [RFC 8949](https://www.rfc-editor.org/rfc/rfc8949) — CBOR and core deterministic encoding.
- [RFC 9052](https://www.rfc-editor.org/rfc/rfc9052) — COSE structures and `COSE_Sign1`.
- [RFC 9053](https://www.rfc-editor.org/rfc/rfc9053) — COSE EdDSA algorithm profile.
