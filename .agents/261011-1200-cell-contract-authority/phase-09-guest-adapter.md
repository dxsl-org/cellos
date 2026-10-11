---
phase: 9
title: "Qualified Intel VMX and explicit guest C2C adapter"
status: pending
priority: P1
tier: thinking
dependencies: [4, 5, 7, 8]
---

# P09 — Guest participants without native authority inheritance

## Deliverable and requirements

AC-24. Intel VM guest interacts with native and remote services only through explicitly granted bridge exports. VMX/EPT qualification is a real prerequisite inside this phase, not a circular wait for a locked legacy plan. Design for that prerequisite may be prepared after P01; bridge activation waits for all listed dependencies.

## Related files / ownership

- Adopt exact unfinished Intel P09 scope from `.agents/260711-1917-tier3b-x86-vtx/plan.md` under new ownership, preserving its VM entry/exit, EPT, device and persistence criteria.
- Kernel VMM/HAL and `cells/services/hypervisor/` files enumerated in that handoff (do not assume AMD SVM implementation is Intel VMX).
- Existing guest bridge design from `.agents/260927-1100-c2c-anywhere-tier-aware/phase-09-guest-bridge.md`, native broker/SDK and approved bridge protocol.
- Intel VM test/build runners and guest image packaging discovered by P01/P09 recon; add behavior oracle to existing `tests/integration/` conventions.

## Implementation steps

1. Reconcile current Intel VMX status and carry remaining VMX/EPT entry/exit, interrupt/device, storage/network and teardown work into an exact owned sub-slice. Preserve AMD/ARM evidence as historical only; no alternate backend substituted for an Intel claim.
2. Qualify guest boot, I/O, reset and teardown with no host mapping/authority leakage. Make guest resource budgets/termination bounded by host Cell authority and safe device/DMA handling from P05.
3. Define bridge peer as an explicitly identified guest domain and incarnation. Guest UID 0, raw guest addresses and guest service names grant no host authority. Authenticate bridge context by host ownership, not caller payload.
4. Provision a bounded set of native service/export methods and resource ceilings to the bridge. Translate guest requests into ordinary C2C operations and copied or explicitly granted bounded buffers; never expose host handles or control opcodes wholesale.
5. Preserve deadlines, cancellation, peer-loss uncertainty and request correlation over guest-host and admitted remote paths. Guest/bridge restart invalidates old request/resource generations; do not revive old replies into new guest state.
6. Exercise a bounded guest client using native VFS/NET or stateful service and a remote exported service. This demonstrates participation, not transparent distribution of unmodified arbitrary programs or host POSIX emulation.
7. Remove any obsolete guest bridge prototype/alias only after consumers migrate; keep public reserved identifiers per ABI policy. Update Tier 3 profile evidence and admission docs.

## Verification / success criteria

- [ ] Intel VMX/EPT actual execution, contained hostile guest memory access, device I/O, guest reset/teardown and resource reclamation pass under the qualified environment. QEMU/TCG without VMX execution is not sufficient.
- [ ] Guest authorized request succeeds with exact response; ungranted method/resource, forged host Cell identity, stale bridge token and oversized guest buffer fail before host effect.
- [ ] Guest exit/bridge crash while remote request is dispatched returns uncertainty safely; old replies never settle a successor guest's request.
- [ ] All-tier runtime oracle connects native Tier 1, admitted Tier 2 C/C++ and Intel guest bridge to the same typed service contract, including a two-node path from P08.
- [ ] Hardware and production profile gates remain distinct; missing VMX environment keeps AC-24 open, not replaced with an AMD boot log.

## Security / rollback

Guest is not a native Cell principal and must not receive host root authority through an adapter. Disable bridge exports, quiesce guest/device mappings and cold-boot known-safe host on rollback. Guest disk mutations and remote effects are not reversed; reconcile as P06/P08. No unsafe virtual-device fallback to gain a passing guest boot.

## Assumptions and deviation log

Qualified Intel VMX environment may be unavailable; availability must be established before runtime qualification. The detailed VMX implementation checklist stays in the adopted legacy sub-slice, with this phase owning its release and contract mapping rather than duplicating architecture claims. Deviation log: none.
