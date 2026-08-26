# D24 — Spec 20 ratification and Law-1 sequencing

**Status:** approved/applied 2026-08-01. No code or ABI changed.

## Finding

Spec 20 is correctly Draft. Its own ratification checklist is not satisfied:

- kernel-attested local sender identity is absent (`docs/specs/20...:3-7,43`);
- `CellAddr`/`RemoteErr` are sketches, not public types;
- broker-scoped watch is a new kernel primitive and hard prerequisite (§2.4);
- broker connect/Noise handshake is not a yielding state machine (§3);
- no two-node request/reply/watch prototype exists.

Approving stable public types now would freeze names and error semantics before the
identity root and failure paths can falsify them. Ratifying the spec before phase 02 would
also make a normative authorization contract depend on an absent identity oracle.

## Recommended ruling [FINAL]

**Approve recommendation A: approve zero Law-1 ABI additions now and keep Spec 20 Draft.**

Sequence:

1. Apply D25's identity invariant as a docs/security rule without changing ABI.
2. Land and test phase 02 kernel-attested sender identity.
3. Prototype private/internal `CellAddr`, remote errors, yielding handshake, and watch
   safety classes on two nodes without migrating stable consumers.
4. Present one exact Law-1 package with discriminants, layouts, bounds, errors, and
   compatibility story for the required two confirmations.
5. Ratify Spec 20 only after the identity root and prototype gates pass, then migrate
   consumers.

This approves the architectural direction, not any public ABI shape.
