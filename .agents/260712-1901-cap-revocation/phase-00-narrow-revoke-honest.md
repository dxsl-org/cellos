# Phase 00 (G1) — Narrow `sys_cap_revoke` to what it can truly revoke

## Context Links
- Plan: [plan.md](plan.md)
- Design authority: `.agents/260712-1836-mythos-g123-analysis/dossier-3-revocation-sas.md` §"Two-speed recommendation" #1
- Depends on: P-TRUST (`.agents/260712-1100`) — shares trust theme + same file (`syscall.rs`)

## Overview
- **Priority:** P1 (makes a shipped-but-unsound mechanism honest). **Speed:** G1. **Status:** pending.
- **Law 1:** none. **Effort:** ~20 LOC + 1 negative test.
- Make `sys_cap_revoke` refuse to revoke any surface it cannot actually tear down, instead of silently label-changing the TCB field while the derived authority stays live. This closes the "revocation is a lie" gap by *refusing the lie* until G2 teardown lands. Standalone-shippable ahead of the G2 phases.

## Key Insights
- `sys_cap_revoke` today (`syscall.rs:1420-1474`): clears `block_io/network/spawn/hypervisor` cap fields and ANDs `mmio_devices` / `block_regions` masks; logs `CapRevoked` audit; does nothing to derived authority.
- Already safe: `block_io`/`network` are refused by the Gate-2 system-cell guard (`syscall.rs:1442`, returns `PermissionDenied`). These are Class-1 anyway (lazy re-check).
- Class-1 lazy-safe bit that IS reachable: `SPAWN` (next spawn syscall fails closed). Keep allowing it.
- The reachable **Class-2** bit encodable today is the `mmio_devices` mask (`MMIO_SHIFT=8`, `libs/api/src/abi/syscall.rs:368`). Clearing `task.mmio_devices` stops *future* `sys_request_mmio` but leaves the already-mapped MMIO window live (x86: user PTE from `map_mmio_user_x86`; riscv/aarch64: boot-mapped user). So this bit must be **rejected** until P03/P04.
- `pcie_driver`/`platform`/`supervisor` have **no `cap_mask` bit** today → not encodable → not reachable via this syscall yet. P05 adds the bits (Law 1) *with* their teardown. P00 documents this so the widening is deliberate.
- `SyscallError::NotSupported` exists (`syscall.rs:253`) and maps to `ViError::NotSupported` (`libs/types/src/lib.rs:27`).

## Requirements
- **F1:** revoking any `mmio_devices` bit (`cap_mask & MMIO_MASK != 0`) → return `NotSupported` + audit event; make no partial change.
- **F2:** revoking `HYPERVISOR` → `NotSupported` (ambient CSR authority, no teardown; open-question default). Flag for caller confirmation.
- **F3:** `SPAWN` revoke continues to succeed (Class-1 lazy).
- **F4:** `block_io`/`network` continue to be refused by the existing Gate-2 guard (unchanged).
- **NF1:** no new mechanism, no new state, no Law 1.

## Architecture
Insert an **early reject** inside the `Syscall::CapRevoke` arm, after Gate-1/Gate-2, before the field-clear block (`syscall.rs:1451`):
```
if cap_mask & CM::MMIO_MASK != 0 || cap_mask & CM::HYPERVISOR != 0 {
    audit(CapRevokeRejected, target_tid, cap_mask);   // distinct from CapRevoked
    return Err(SyscallError::NotSupported);
}
```
Rejection is all-or-nothing: if the caller mixes a rejected bit with `SPAWN`, the whole call is refused (no partial revoke — a caller must not believe MMIO was revoked because SPAWN succeeded). Data flow: caller mask → gate checks → reject-filter → (surviving Class-1 bits only) field clear → audit `CapRevoked`.

## Related Code Files
- **Modify:** `kernel/src/task/syscall.rs` (CapRevoke arm ~1451) — add reject-filter.
- **Modify (maybe):** `kernel/src/audit.rs` — add `CapRevokeRejected` variant if a distinct audit code is wanted (else reuse `CapRevoked` with a marker).
- **Read-only:** `libs/api/src/abi/syscall.rs:357-376` (cap_mask), `kernel/src/task/tcb.rs:207-224` (cap fields).

## Implementation Steps
1. Add `CM::MMIO_MASK` + `CM::HYPERVISOR` reject-filter early in the CapRevoke arm; return `NotSupported`.
2. Emit an audit event on rejection (new `CapRevokeRejected` code or `CapRevoked` + sentinel `cap_mask` high bit).
3. Confirm `SPAWN`-only revoke still passes and clears `spawn_cap`.
4. Add doc comment: "MMIO/hypervisor/privileged caps are not revocable until Class-2 teardown (P01-P05) lands — refusing is the honest behavior."

## Todo List
- [ ] Reject-filter for `MMIO_MASK | HYPERVISOR` → `NotSupported`
- [ ] Rejection audit event
- [ ] Doc comment referencing this plan
- [ ] Negative test: revoke MMIO bit → `NotSupported`, `task.mmio_devices` unchanged
- [ ] Regression test: revoke SPAWN → Ok, `spawn_cap == None`

## Success Criteria
- Revoking an MMIO bit returns `NotSupported`; the target's `mmio_devices` is byte-for-byte unchanged (observable via a follow-up probe or test hook).
- Revoking `SPAWN` still succeeds and the next `sys_spawn_*` from the target fails closed.
- 3-arch boot suite stays green (no boot-path caller exercises revoke; the change is inert at boot).

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|-----------|
| A real (future) caller expects MMIO revoke to work | Low×Med | It never worked; `NotSupported` is strictly safer than the silent lie. P03/P04 restore it correctly. |
| Rejecting `HYPERVISOR` breaks a VMM-lifecycle flow | Low×Med | No caller found today; open-question flagged. If one appears, revisit before merge. |
| Partial-revoke expectation (mask mixes bits) | Low×Low | All-or-nothing reject documented; callers must issue Class-1 and Class-2 masks separately. |

## Security Considerations
Fail-closed is the entire intent: a capability system that reports success while leaving derived authority live is worse than one that admits it cannot revoke. This phase converts a latent J-Kernel stale-authority hole into an explicit, audited refusal. No authority is *granted* here; only refusals are added.

## Next Steps
- Unblocks nothing (standalone); G2 phases P01-P05 replace each `NotSupported` with real teardown, then re-widen.
- Rollback: delete the reject-filter — reverts to prior (unsound) behavior with zero cascading effects.
