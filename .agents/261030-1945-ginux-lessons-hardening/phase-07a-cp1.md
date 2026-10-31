# Phase 07A CP1 — CSpace derive: destination admission, delegability, issuer preservation (Law-1 semantic)

Status: CP1 and CP2 approved by the user 2026-10-31. Complete.

## 1. Facts (verified 2026-10-31)

- `dispatch_handle_control` (`kernel/src/cell/cspace.rs:702-786`) and `dispatch_cspace_control` (`:789-886`) are **not reachable from any syscall**. Their only callers are `cspace.rs` host tests and the mirror `tests/unit_cspace_smoke.rs`. Gaps #1/#2 are latent: no userspace exploit today, but any future wiring would inherit them.
- Derive takes `destination_cell/generation` (`:759-762`) or `target_cell/generation` (`:852-855`) as data only. No liveness, ceiling or receive-policy check. The child slot is allocated in the **source** authority, so the destination cannot use it (`verify_ticket` looks only at the caller's own table).
- `issuer` is overwritten with the delegating Cell (`:763`, `:856`) instead of the resource's issuer. Any live ticket is derivable: there is no delegable right.
- Rights subset, expiry narrowing, depth ≤ 8 and admission-cut already hold (`allocate_slot`, `:426-462`).
- Done without Law-1 in slice 07A: a root mint exists only as `mint_kernel_root` (issuer `Kernel`, holder = owning incarnation); `allocate_slot` is private.

## 2. Proposed semantics

D1. **Issuer-local records.** A grant stays in its issuer's (delegator's) table, so lineage and revoke stay bounded and local, and no cross-table parent pointers exist. The holder field names who may exercise it.

D2. **Destination admission at derive.** The destination `(cell, generation)` must be a live, non-revoking `CELL_AUTHORITIES` entry; otherwise `Stale`. The destination's admission ceiling must cover the requested rights for that kind; otherwise `Denied`. Checks take the same `CELL_AUTHORITIES` lock already held, with no allocation or scan.

D3. **Cross-cell receipt is fail-closed until a receive design exists.** A derive whose destination differs from the source incarnation returns `Denied`. Allowed: local attenuation (destination = self), which keeps equal-or-narrower rights. Spec 25 §3.1 post-start receipt then has nothing to bypass. A future receive/exercise path, where the holder uses the grant through the issuer's table, needs its own CP1.

D4. **Delegability.** A new slot attribute `delegable: bool`. Kernel roots are delegable only when the minting site says so; `mint_kernel_root` gains that parameter, and the BootFS file root is `false`. Deriving from a non-delegable slot returns `Denied`. A child's delegable flag is ≤ its parent's. The attribute is internal: no wire field changes, and the request's own rights stay as before.

D5. **Issuer preservation.** The child keeps the parent's `issuer`. The delegator is recorded through `lineage_parent`, which already exists, so no new field is needed.

## 3. Compatibility / Law-1 delta

- Wire layouts of `HandleControlRequest`/`CSpaceRequest` and `ControlResultHeader` are unchanged.
- Semantic changes: derive to another Cell becomes `Denied` (it was `Success` with an unusable record); stale or dead destination returns `Stale`; non-delegable parent returns `Denied`; `issuer` of derived slots changes meaning.
- No production consumer exists (§1), so no caller migrates. The host tests and the mirror module are updated.
- Lock register: no new lock and no order change (single `CELL_AUTHORITIES` lock).

## 4. Evidence plan

- Host tests: cross-cell derive is denied with no slot created; a stale or nonexistent destination returns `Stale`; local equal-rights derive succeeds (positive control); derive from a non-delegable parent is denied; a child's issuer equals the root issuer; existing excessive-rights, expiry, depth and wrong-type negatives still hold.
- Boot self-test (`cap_file_selftest`): a BootFS root is non-delegable.
- Builds with `-D warnings` on x86 and RV64.
- The private 1/2-CPU witness of Phase 07 step 3 is **not** in 07A; it belongs to the slice that wires a real issuer path.

## 5. Not decided here

- Shape of the receive/exercise path (holder-side lookup into the issuer table, or kernel-mediated transfer).
- Legacy `CAP_TABLE` keyed by `CellId` only (census finding #4).
- The signed-boot-manifest ADR (Phase 07 step 5).

## 6. Result (2026-10-31)

CP1 approved by the user ("Duyệt CP1"). Implemented in `kernel/src/cell/cspace.rs` and `kernel/src/task/cap_file_selftest.rs`:
- D2/D3: `AuthorityTable::derive_destination` runs under the already-held `CELL_AUTHORITIES` lock in both dispatchers. A non-live or revoking destination returns `Stale`; any other Cell returns `Denied`; nothing is recorded.
- D4: `SlotEntry::delegable`. `mint_kernel_root(kind, rights, delegable)`; the BootFS file root (OpenCap) is `false`. A child inherits its parent's flag, and a non-delegable parent returns `Denied`.
- D5: a derived slot takes the parent's `issuer`. The `issuer` argument applies to roots only.
- D2 ceiling check: with D3 every admitted destination is the source itself, and the child is already ⊆ the parent. A per-kind destination-ceiling map is therefore not implemented. It belongs to the future receive-path CP1, where a different destination becomes possible.
- `tests/unit_cspace_smoke.rs` is not referenced by any Cargo target or script (never compiled) and tests only its own copy, so it was left unchanged rather than updated.

Evidence: `cspace` host tests 11/11, including the new `derive_admits_only_live_self_destination_and_preserves_issuer` (cross-Cell `Denied` with no slot, stale or unregistered destination `Stale`, revoking destination `Stale` through the CSpace dispatcher, non-delegable root `Denied`, equal-rights local derive `Success` with issuer `Kernel`) and `child_is_never_more_delegable_than_parent`; x86 and RV64 release builds with `-D warnings`.

CP2 approved by the user 2026-10-31 ("Duyệt CP2 + commit"). RV64 boot: `cap-file self-test PASS` (incl. non-delegable BootFS root), reaches `Cellos >`.
