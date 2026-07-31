# ADR: Directory Capabilities for the VFS

**Date**: 2026-07-31 | **Status**: Accepted | **Authors**: Cellos core team

---

## Decision

Move the VFS from path-string operations guarded by an access table to
directory-handle operations that cannot name a path outside the handle. Four
points that had to be settled before any code:

1. **Revoking a handle revokes everything derived from it.**
2. **Handles do not survive a hot-swap.** The new instance re-acquires.
3. **`ViDirHandle` is a new type, not a widened `CapId`.**
4. **The kernel carries a child's handle set through the spawn call.** This costs
   a second ABI change, accepted deliberately for the properties it buys.

---

## Context

Today a cell names any absolute path and the VFS decides whether to allow it.
That is an access check, and access checks carry the failure modes checks always
carry: confused deputies, traversal, and the gap between checking and using. The
capability form removes the question instead of answering it — a cell holding a
handle to one directory has no way to express a path outside it.

The device side of the system already works this way. A spawned cell's
capabilities are the intersection of what it asks for and what its spawner holds,
so authority only ever narrows. Filesystem authority is the part that never got
the same treatment, and this is what closes that asymmetry.

Point 4 was the open one. Authority that a child claims to have received from its
parent is worthless unless something can confirm the parent granted it — and
confirming that is exactly the confused-deputy problem this work exists to solve.

---

## Point 1 — Revocation is transitive

A derived handle is a strictly narrower share of its parent's authority. If
revoking the parent left the derived handle alive, revocation would not be
revocation: authority would survive the withdrawal of the authority it came from,
and a cell could preserve access indefinitely by deriving a subdirectory handle
and discarding the original.

Each entry therefore records the handle it was derived from, and revocation walks
the derived set. Revocation is rare and the sets are small, so the walk is not
worth avoiding.

The cost is real and accepted: a child's access can disappear because of a
decision about its parent. That is the same shape as capability intersection at
spawn, where a parent's ceiling already bounds a child it will never see again.

## Point 2 — Handles do not survive a hot-swap

A swapped cell re-acquires its handles rather than inheriting them.

The attested identity a service now receives carries a generation alongside the
cell id, and that field exists precisely so a service can tell a respawned cell
from the dead one it replaced. Honouring a predecessor's handles would discard
that distinction at the one moment it matters. A cell that implements state
transfer can request equivalent handles on the way back up; what it cannot do is
silently continue with authority granted to a previous instance.

This is the fail-closed direction. A cell that forgets to re-acquire loses access
and says so; the alternative fails silently and in the unsafe direction.

## Point 3 — A distinct handle type

`CapId` is a kernel-issued capability for a file opened through the kernel.
A directory handle is issued by the VFS, lives in the VFS's own table, and is
revoked on the VFS's authority. The two share a representation and nothing else.

Reusing `CapId` would make passing one where the other belongs a runtime
confusion rather than a compile error, across a boundary whose whole purpose is
that authority cannot be forged or mistaken. `ViDirHandle` is therefore its own
type. The cost is a conversion at the few points that legitimately bridge them.

## Point 4 — The kernel carries the handle set through spawn

Three mechanisms were available:

| Option | Mechanism | ABI cost |
|---|---|---|
| (a) | Kernel carries the handle set through the spawn call | **Second ABI change** |
| (b) | VFS issues a sealed token the parent hands to the child | Needs a carrier the spawn path lacks |
| (c) | Child asks, parent grants over IPC while both are alive | None |

**Chosen: (a).**

It is the model that matches how device authority already works. A spawned cell's
capabilities are fixed at spawn as an intersection with its spawner's, and nothing
about filesystem authority argues for a different shape. Making the two work the
same way is worth more than the ABI it costs, because the asymmetry is itself a
source of mistakes.

It is also the only option with no window. Under (c) a child holds no filesystem
authority between spawn and its first grant, and a parent that dies in between
leaves a child that can never acquire what it was meant to have — an
init-spawned service would need a live grantor. Under (a) a child starts with
exactly the authority it will ever have, which is both simpler to reason about and
simpler to audit: the handle set is a property of the spawn, not of a conversation
that happened afterwards.

(b) was never viable — the spawn-by-path call has no argument to carry a token.

**What this costs, stated plainly.** Two ABI changes rather than one:

- the VFS request enum gains handle-based operations;
- the spawn path gains a carrier for the handle set, and the task control block
  gains somewhere to record it, since neither exists today. The task structure has
  no parent or spawner field and the spawn-by-path call takes only a path pointer
  and length.

**The kernel is a courier, not the authority.** The distinction decides whether
(a) delivers anything. A spawn names handles the parent already holds; the kernel
copies that set to the child, records it against the child's task, and attests to
the VFS that the set came from that parent. It never interprets a handle. The VFS
then checks the parent genuinely held them, against its own table, before binding
them to the child.

The VFS therefore remains the single authority on filesystem authority — it
issues, validates and revokes — and the kernel's new responsibility is limited to
carrying and attesting, which is what it already does for caller identity.
Mirroring the handle set into the kernel as a second source of truth was
considered and rejected: the two copies can drift, and drift here is a silently
widened authority rather than a compile error.

Without some such confirmation, (a) degenerates into the confused-deputy problem
of an unauthenticated (c) with more machinery attached. **The confirmation path is
part of the ABI change, not a follow-up.**

Narrowing-only is enforced where the set is constructed, symmetric with the
capability intersection already performed at spawn. A spawn requesting a handle
its parent does not hold fails the spawn rather than silently dropping the handle,
so an over-broad request is visible instead of quietly downgraded.

**Why not (c), given it was free.** The attested caller identity added earlier in
this work does make (c) sound — a grant request now arrives with the granting
cell's identity established by the kernel rather than claimed by the sender, which
was the property (c) previously lacked. It was rejected on model grounds rather
than security grounds: authority that appears partway through a cell's life is
harder to audit than authority fixed at its creation, and it would leave the
filesystem as the one subsystem where delegation works differently from every
other.

---

## Consequences

- Phase 06 needs **two** ABI changes, confirmed together rather than separately:
  the VFS request enum, and the spawn carrier plus its task-side record. Splitting
  them would hide half the cost behind an approval already given.
- The access table becomes defence in depth rather than the primary control, for
  cells that have migrated.
- The raw pointer returned by the current whole-file read is incompatible with
  this model, because in a single address space it is authority that cannot be
  taken back. It becomes a time-bounded grant or it goes.
- Migration is per-cell and gated on a per-cell flag. Until a cell carries that
  flag, the guarantee does not hold for it, and the phase is not complete while
  the path-string operations still exist.

## Rejected

**Letting a derived handle outlive its parent's revocation.** Convenient for
long-lived children, and it makes revocation advisory.

**Handles surviving a hot-swap for continuity.** It reintroduces exactly what the
generation field was added to prevent.

**Reusing `CapId` to avoid a conversion.** Trades a compile-time distinction for a
runtime one on a security boundary.

**Granting handles over IPC after spawn**, which would have cost no ABI change and
is sound now that caller identity is attested. Rejected because authority that
arrives partway through a cell's life is harder to audit than authority fixed at
creation, and because it would leave filesystem delegation working differently
from every other kind. The saving was real; the inconsistency was judged to cost
more over time.
