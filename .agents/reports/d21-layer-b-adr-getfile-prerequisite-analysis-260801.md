# D21 — Layer B ADR timing and the `GetFile` prerequisite

**Status:** approved/applied 2026-08-01. No code or ABI changed.

## Finding

Layer B cannot safely inherit the current VFS contract:

- Spec 19 requires an ADR for grant mapping and the wire contract before implementation.
- Spec 18 §4 correctly says `DataPtr`/`GetFile` is unrepresentable across a Tier-2 domain.
- `GetFile -> DataPtr` remains live in `libs/api/src/services/ipc.rs:29,201`, VFS dispatch,
  shell, Lua, WASM, benchmarks, and the fast-IPC handler.
- The pointer is permanent, unrevocable authority in the shared Tier-1 mapping
  (`docs/specs/17-ipc-wire-contract.md:439-451`).

Writing the full Layer-B ADR in this decision pass would be premature: page-table domain
implementation is not in scope, and the ADR must choose concrete mapping lifetime,
revocation, copy thresholds, and failure semantics. But leaving `GetFile` as a mere
consequence permits a Tier-2 loader to land before its IPC boundary is representable.

## Recommended ruling [FINAL]

**Approve recommendation A: make raw-pointer removal a Tier-2 admission prerequisite,
but defer the detailed ADR to the Layer-B implementation window.**

1. Spec 18 must gate Tier-2 admission on replacing public `GetFile/DataPtr` with bounded
   copy or explicitly mapped/revocable Grant responses.
2. A Tier-1-only internal fast path may survive only if the type system/API prevents it
   from crossing into a Tier-2 domain.
3. The Layer-B ADR is mandatory before any page-table-domain code and must settle grant
   map/unmap ownership, revocation, task death, async pins, and Spec 17 framing.
4. The ADR and public API mutation remain Law-1 work requiring their own confirmation;
   D21 does not approve signatures or discriminant changes.
