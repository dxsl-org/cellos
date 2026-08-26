# D36 — Record reciprocal cross-plan precedence

**Status:** approved/applied 2026-08-01. Documentation/portfolio only.

## Finding

Both conflicts currently document precedence on only one side.

For package distribution, the old phase proposes name-based `pkg`/shell authorization and
conditionally writable `/bin`; this conflicts with the later directory-capability and
sealed-path model. `/bin` must remain non-writable through ambient path requests. A future
installer needs a dedicated authority/staging/commit contract and must preserve spawn
signature/policy checks.

For grant reclaim, Midori phase 07 already landed pin/quarantine foundations and requires
death/revoke paths to respect in-flight pins. Cap-revocation phase 02 still describes
immediate reclaim/fault semantics that can free a grant during DMA.

## Recommended ruling [FINAL]

**Approve A: write reciprocal precedence notes now.**

1. Package phase 01 is blocked on a capability-scoped installer design; forbid
   `allow_write_all`, caller-name authorization, and ambient `/bin` writes.
2. Revocation phase 02 must reuse pin/quarantine-aware reclaim and cannot return frames
   until cancellation/driver acknowledgement completes.
3. Midori remains the mechanism authority for pin/quarantine ordering; the revocation
   plan remains policy/trigger authority.
4. These notes change no runtime or ABI.
