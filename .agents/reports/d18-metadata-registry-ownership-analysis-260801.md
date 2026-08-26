# D18 — Metadata Registry: monolith or focused ownership registries?

**Status:** approved/applied 2026-08-01. No code or ABI changed.

## Finding

The docket premise is partly stale. The universal address-range registry described by
Spec 02 §3 is absent, but the safety-critical pieces are no longer absent:

- `kernel/src/memory/pin.rs:1-26` implements a bounded async pin registry with
  quarantine-on-owner-death and explicit acknowledgement.
- `kernel/src/task/syscall.rs:60-88` owns active page-grant and registered-grant tables.
- `kernel/src/task/syscall.rs:223-345` reaps grants and withholds pinned frames.
- `kernel/src/resource_registry.rs` separately owns MMIO/PCIe resources.
- `kernel/src/snapshot.rs:80-100` snapshots allocated frames without consulting a
  universal metadata table, so D4 is not blocked by D18 in the shipped implementation.

The remaining Spec 02 design — one hash table mapping every address range to
`{OwnerID, State}` — would duplicate these authorities and create lock-order and stale
metadata risks. It also cannot deliver Spec 08's generic pointer swizzling: Rust/ELF
memory does not identify every raw pointer at runtime.

## Recommended ruling [FINAL]

**Approve recommendation A: withdraw the monolithic Metadata Registry.**

1. Spec 02 owns the ownership invariants, not a universal table implementation.
2. Grant ownership remains in the kernel grant tables; in-flight reachability remains in
   the pin registry; MMIO/PCIe ownership remains in the resource registry; frame/quota
   accounting remains with their existing owners.
3. Amend Specs 03, 07, and 10 to cite the specific authority they depend on.
4. Withdraw Spec 08's generic pointer-swizzling claim. A future hibernate format must use
   typed, subsystem-owned serialization rather than scanning arbitrary memory for pointers.
5. Do not create a G2 monolith merely to satisfy an obsolete name. Add a new registry only
   when an uncovered invariant has a concrete caller and test.

### Remaining gap

Pin quarantine has focused unit tests (`kernel/src/memory/pin_tests.rs`), but the combined
grant-transfer/death/DMA-ack path still needs an end-to-end runtime gate before it is called
fully qualified.
