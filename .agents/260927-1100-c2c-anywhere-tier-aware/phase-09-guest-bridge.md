---
phase: 9
title: "Tier-3 VM guest IPC bridge"
status: pending
priority: P2
effort: "separate guest capability"
dependencies: [1, 2]
tier: thinking
---

# Phase 09: Tier-3 VM guest IPC bridge

## Overview

A Tier-3 Linux/Android/Windows guest is an isolated VM, **not** a native SAS Cell and not a Tier-2 domain. Give a guest an explicitly scoped, copy-based host-service bridge only if a real VM/runtime and a concrete exported service need it. Do not make a guest bridge a precondition for *native* two-node C2C Anywhere.

## Requirements and architecture

- Choose and ratify an explicit guest transport supported by the existing VM monitor (e.g. a bounded virtio message queue). A guest/host message names a guest instance principal, host export and bounded request ID; VM-exit/virtqueue descriptor provenance is verified at the host boundary. The guest cannot mint `CellEndpoint::Local`, SAS ring/grant handles, `DomainGrant`, kernel sender ID or a remote NodeId.
- Host copies a validated request out of guest-owned memory into an owned broker/service request; host copies response to validated writable guest memory **only while the same VM instance/generation and descriptor remain live**. Validate descriptor chain length, pointer arithmetic, read/write flags, overlap, request/response bound, guest memory mapping and restart before touching a buffer. An untrusted guest must never supply a host pointer to reuse after VM death.
- Export admission is explicit per guest principal/service/method and host policy. Guest-origin calls do not inherit Tier-1 `Local` permissions or bypass Tier-2 private-root copied IPC. Cross-node access, if ever needed, becomes an independently authorized host-broker remote export under Phase-01/05 rules; do not claim that a guest is a third `Cell` transport route.
- Completion retains the same bounded, definite-not-submitted versus uncertain-after-dispatch rules. Terminating a VM revokes outstanding queue mappings and delivery permissions; no queued response can attach to a new guest incarnation. Start with one concrete host service and a real QEMU guest fixture, not an inert adapter.

## Related files

- Inspect current VM monitor, guest I/O/virtqueue driver and host service export policy before selecting files to modify. Contract sources: `docs/decisions/0015-dual-mode-hybrid-architecture.md`, `docs/specs/17-ipc-wire-contract.md`, `docs/specs/20-unified-ipc-contract.md`, `docs/specs/22-native-domain-cell-implementation-gate.md`. The host-to-guest bridge is not a change to native Spec-17 caller attestation.

## Implementation steps

1. Inventory the actual VM monitor/guest device model and choose a concrete exported host service; if neither supports a bounded guest-facing queue, record the missing dependency and leave capability disabled rather than simulating a native endpoint.
2. Ratify the guest endpoint/addressing, admission and queue format with security review and Law-1 confirmations if any public host ABI is affected; implement descriptor validator, owned copy, reply routing and VM-generation revocation.
3. Boot a real QEMU guest, invoke allowed host service and confirm reply. Then exercise forged principal, unauthorized method, out-of-range/overlapping descriptor, cyclic chain, oversized payload, read-only response page, guest death/respawn, full queue and response-after-restart.
4. Prove the guest cannot call an unexported Tier-1/Tier-2 method or submit an SAS pointer/DomainGrant; separately verify that native local IPC and the remote relay oracle remain unaffected.

## Success criteria

- [ ] An actual guest VM invokes exactly one explicitly exported host service through a validated, bounded copied bridge with an attributable response; no native Cell identity is synthesized.
- [ ] Malformed/hostile descriptor and permission tests fail closed without host out-of-bounds access, stale-VM delivery or Tier-1 privilege escalation.
- [ ] Docs and feature admission label guest support separately from native two-node remote operation; absent reviewed VM/device capability, guest bridge stays disabled with no claimed success.

## Assumptions

- **Claim:** The target VM monitor exposes a suitable guest/host queue with a maintainable driver. **Confidence:** low. **Verify:** source inventory and a real guest boot before locking the ABI.

## Security considerations

Guest RAM and queues are attacker-controlled input. Do not grant a guest direct host memory or direct relay credentials; host service policy is not delegated to a device emulator merely because it sits on the same node.

## Risk assessment and rollback

Descriptor bugs cross the VM isolation boundary. Disable the bridge and revoke guest export policy/queue mappings while keeping native IPC and remote relay operational. Host actions already dispatched from a terminated guest can have effects; return/record `Indeterminate` rather than replaying them to the next guest.

## Deviation log

None.
