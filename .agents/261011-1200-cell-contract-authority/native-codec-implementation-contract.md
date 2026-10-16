# Native DCO1 codec/proof implementation contract

## Scope

The user requests step2 implementation before the complete native CP1/runtime cutover in step3. Build a standalone no_std, allocation-free Rust library under `libs/durable-charge-codec/` with its own Cargo workspace root. This is a native-usable codec and cryptographic adapter, not a live native ledger or a producer of truthful resource/floor facts. No native runtime/public ABI callsite is switched before the exact native package gate. Historical DCO1 candidate bytes and approved document pins remain unchanged.

Inputs are `durable-charge-canonical-byte-candidate.md` r1 and `durable-charge-allocation-byte-successor.md` s1. All specified kinds1–11,32–36,48–58 must be handled, not an envelope-only subset. Typed borrowed record/payload views, exact fixed/variable lengths, finite limits before allocation/hashing, reserved/identity/schema rules, canonical lists, checked arithmetic and role/Ref binding checks are required. All header/payload/watermark/profile equalities and successor current-allocation rules must be expressed in code or explicitly remain admission checks requiring authenticated live state. Do not claim a pure parser has validated that live state.

Proof adapters use the already present native `ed25519-compact` verify mechanism, with a separately specified fixed canonical signed-fact carrier and exact domain separation. Bind finite role/purpose/lane/scope/journal/genesis/O/A/T, owner/fence/revision/input/output reference and roster/public pin. Reject wrong role/purpose/context/pin/signature and unknown/oversized carrier fields. Typed verified facts establish only the named assigned signer's statement. Live custody, current roster/floor, complete owner set and enforced gate admission must still be checked by native consumers after their implementation. No test key is a production key, no generic signed blob grants all roles, and no signature claims storage durability by itself.

## Observable criteria

- NC-01: All defined kind schemas validate exact packed sizes and reject unknown kinds/schema IDs, bad flags/reserved, lengths/overflow/trailing bytes, noncanonical identities/lists and bounded-work exhaustion.
- NC-02: Encode to caller-provided buffers and decode from borrowed slices; no heap or unsafe code. Valid records use the proposed domain-separated SHA256 trailer and exact schema name derivation.
- NC-03: Referent verification binds length/kind/schema/ledger/genesis/O/A/profile and role as applicable; missing/cross-obligation/wrong-role referents never grant permission. Dependency completeness/cycles require the bounded reference validator, not merely parse success.
- NC-04: Typed proof verification rejects changed signed bindings, unknown role/suite/version, wrong assigned public pin, forged signature, stale expected fence/revision and mismatched exact record/reference. Required native current-policy/completeness/gate facts are not faked.
- NC-05: Deterministic behavior-focused regressions include each variable boundary, schema substitution, vector/profile mismatch, duplicate keys/identity, overflow/exhaustion and valid-checksum malicious records. Native target compile and an actual host CLI encode/decode/verify smoke are observed.

## Admission boundary

The implementation supplies the deterministic byte/crypto part of step2. It cannot supply absent O1–O9 native producers, qualified native G2 owning-service backend/floor or G0 finite basis. Native adapters must not return usable permission while those prerequisites remain absent. Complete package CP1 and source lock handoffs precede step3 integration; actual native VMware state-machine/fault evidence and target/profile/CP2 follow, not inferred from codec tests.
