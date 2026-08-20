# Spec 22 — Tier 2 Native Domain Cell Implementation Gate (ADR)

> **Status**: Accepted design gate 2026-08-21. **Not implementation approval.**
> Tier 2 is accepted but unimplemented; this document defines the evidence required
> before it may be exposed by the loader or installer.

## 1. Context and current truth

Tier 1 Cells share the SAS page-table view and rely on Rust LBI. The native loader has
one CPU root, `memory::paging::KERNEL_ROOT`; ordinary mappings and grants are inserted
there. `GrantAlloc` returns an identity-mapped physical address, so a grant is directly
addressable by every SAS Cell once it is mapped USER. The scheduler saves and restores
register contexts, but has no address-space field or CPU-MMU activation step.

The loader currently verifies a present signature and, unless `signing-required` is
enabled, admits an absent signature. That result is an admission input, not containment:
the selected native execution view remains the SAS. Therefore **unsigned does not mean
contained**, and an unsigned native ELF must continue to be rejected by a fleet-secure
profile or run in Tier 3 until this gate is completed.

Tier 2 is the future native containment class: a Cell receives a private page-table view
with its own Cell pages, approved shared kernel/user ABI pages, and explicit grants. It
uses the same broad VA layout as the SAS only where that layout is safe to share. Absence
of peer mappings, rather than a signature, is the containment mechanism.

## 2. Decision

No Tier-2 loader branch, installer choice, or claim of native containment may ship until
all required design and negative-test gates below are approved in a separate implementation
plan. The first implementation must be feature-gated, default-off, and leave every
Tier-1-to-Tier-1 switch on the current no-MMU-switch fast path.

### 2.1 CPU page-table ownership and lifetime

Introduce an explicit kernel-owned `AddressSpace`/domain object, not a raw root physical
address stored ad hoc in a task. It owns its root and intermediate table frames, an
architecture context identifier, and a mapping ledger for private Cell frames, immutable
image frames, approved shared ABI pages, and temporary grants. The owning Cell's terminal
cleanup is the sole destruction authority.

The implementation plan must specify and test this order:

1. Allocate and zero a domain root plus all required intermediate tables. Map kernel code,
   kernel stacks/HHDM and trap/syscall requirements supervisor-only; map only the Cell's
   user image, stack, heap, and explicitly permitted shared pages USER.
2. Complete relocation and W^X before making the Cell runnable. Publish the domain to its
   TCB only after its root and mapping ledger are complete.
3. On explicit unshare, Cell exit, forced exit, fault, and failed spawn, transition the
   domain from `LIVE` to `DYING` under its generation lock. Remove every task naming that
   generation from all run queues before new scheduling decisions; reject new syscalls,
   grants, and admissions that would retain it. Every hart that reports the domain current
   receives a quiesce request and activates a safe root that has no dying-domain USER PTEs.
   Each acknowledgement includes the domain identity and generation and is accepted only
   after that hart no longer names the domain.
4. Only after all generation-matched acknowledgements arrive may teardown revoke USER PTEs,
   invalidate local and remote translations, and release leaf frames. Fault/exception paths
   queue this work after returning to the safe root; they must never free their active root
   inline. Free intermediate tables and the root last. A CPU or DMA mapping must never
   survive reuse of its frame.
5. Domain roots never enter the generic `KERNEL_ROOT` mapping API. Existing SAS mappings
   retain their contract; new domain-aware map/unmap APIs take an `AddressSpace` reference
   and cannot silently fall back to the global root.

Kernel global identity mappings cannot be copied wholesale into a user domain. In
particular, current boot code maps broad usable RAM and architecture MMIO in the root.
The domain builder must instead use an allowlist of supervisor mappings and a per-Cell
user mapping ledger. The system must fail closed on a mapping request it cannot classify.

### 2.2 Architecture feasibility and context identifiers

| Architecture | Feasible control | Required implementation proof | Limitation / disposition |
|---|---|---|---|
| RISC-V RV64 | Write `satp` with an Sv39 root PPN and allocated ASID; execute the architecture-required `sfence.vma` sequence. | ASID allocator with wrap generation, remote invalidation protocol, and a test that stale translations cannot cross a recycled ASID. | Viable on the supported paged RV64 lanes. RV32 bare-physical targets cannot implement Tier 2. |
| AArch64 | Write `TTBR0_EL1` with a private root and nonzero ASID; use `TLBI` plus required DSB/ISB ordering. | ASID generation/reuse protocol, multi-PE shootdown witness, and MAIR/TCR compatibility with the root. | Viable on the Armv8.2 deployment lane; MTE is not required. |
| x86_64 | Write `CR3` with a private PML4 and PCID when CPUID supports PCID; invalidate with `INVPCID`/CR3 semantics as appropriate. | CPUID-gated PCID allocation/reuse and a correct non-PCID full-flush fallback. | Feasible, but PCID is an optimization, not a prerequisite; no-PCID mode must remain correct. |

An architecture backend that lacks a safe private-root activation and invalidation protocol
does not advertise Tier 2. It must retain Tier-1/Tier-3 behaviour; it may not label an
SAS launch as a domain launch. ASID/PCID tags reduce TLB flush cost but never replace
unmapping, permission checks, or reuse invalidation.

### 2.3 Scheduling and the SAS fast-path invariant

Scheduling selects the next task first, then derives a transition from the current and
next address-space identities. The implementation must establish this transition contract:

| Transition | Required action |
|---|---|
| SAS → SAS | Restore normal task context only. No `satp`, `TTBR0`, or `CR3` write; no mandatory TLB invalidation. |
| SAS → domain / domain → SAS | Activate the destination root and its ASID/PCID under the architecture ordering rules before returning to user mode. |
| domain A → domain A | Restore normal task context only, provided both tasks retain the exact live address-space generation. |
| domain A → domain B | Activate B's root/tag before user return. |

Interrupt, exception, syscall, idle, task exit, migration, and nested preemption paths must
preserve the same rule. The kernel executes with mappings valid under every selected root;
per-hart current-domain state must be updated atomically with the switch decision. A task
may not run on a second hart while a grant revoke or address-space destruction is pending
unless the implementation proves the required shootdown acknowledgement. `DYING` domains
are not schedulable or migratable: quiescence switches every reporting hart to a safe root,
requires an identity-and-generation acknowledgement, then permits deferred destruction.

### 2.4 Domain-aware syscall user-memory boundary

Every syscall ABI path that accepts a user pointer or buffer must use domain-aware
`copy_from_user` and `copy_to_user`; it may not directly dereference a caller-controlled
address after Tier 2 exists. Each helper receives the active domain identity and generation,
validates the complete range page by page against that domain's USER mapping ledger and
declared permissions, and copies through a kernel-owned buffer. Null, overflow, kernel,
peer-domain, unmapped, and permission-mismatched addresses return the ABI's recoverable
invalid-address error without modifying destination state.

The helpers install a narrow recoverable-fault guard around each copy. A page fault from
the validated user copy resumes at that guard and returns an error; it must never take the
generic kernel-fault panic path. The mapping ledger holds a read-side reference for the
entire copy. Unmap/revoke first makes the range unavailable to new copies, then waits for
that reference to drain before removing PTEs and reusing frames. This contract applies to
all existing syscall arms, output pointers, IPC payloads, and future ABI additions; an
implementation may not qualify Tier 2 with a partial syscall allowlist unless admission
also denies every omitted syscall before the task becomes runnable.

### 2.5 IPC and explicit grant contract

Tier crossing defaults to copied IPC. Kernel validation copies a bounded wire message from
the sender's mapped memory into kernel-owned storage and then into the receiver's mapped
memory; raw `DataPtr`/identity-pointer conventions are not valid across a Tier-2 boundary.
The copy has no receiver access to sender pages after completion.

Zero-copy is an opt-in `DomainGrant`, not an extension of the current SAS `GrantShare` bit.
It must name an owner, exactly one mapped grantee (or an explicitly designed immutable
fan-out form), page-aligned range, permissions, mapping address, monotonic generation, and
revocation state. The kernel validates both Cell liveness and grant ownership before adding
the receiver PTE. A grant maps only its pages in the receiver's domain; it never exposes
the global SAS identity map.

Revoke is synchronous with respect to security: mark revoking, block new maps and new IPC
use, remove receiver PTEs, invalidate local and remote translations, await completion, then
return success. Owner/grantee death follows the same protocol before frames are reused.
Pinned DMA frames remain quarantined until the device/IOMMU teardown acknowledgement; CPU
revoke does not authorize recycling DMA-visible memory. The initial implementation should
prefer copied IPC and defer grants until this state machine and its race tests pass.

### 2.6 MMIO, DMA, and IOMMU confinement

A Tier-2 Cell starts with no user MMIO mappings. Resource-registry ownership is necessary
but not sufficient: the domain page table must map only the authorized MMIO range with the
architecture's device attributes and no broad boot MMIO window. A request that cannot be
represented as a range/attribute allowlist is denied.

DMA is separate from CPU mapping. Existing PCIe IOMMU domains and `sys_grant_dma` provide
a useful lifecycle precedent, but Tier 2 must bind a CPU domain grant, device assignment,
IOMMU translation entry, and frame pin to one teardown transaction. Teardown order is:
disable/unbind device access, invalidate IOMMU/device context and wait for fence, revoke CPU
domain mappings and flush, then release pins and frames. Virtio-MMIO is not IOMMU-covered
today; consequently a Tier-2 Cell with raw virtio-MMIO access is out of scope until an
IOPMP/WorldGuard or equivalent DMA confinement path is qualified. Tier 2 must not claim to
contain a device-capable native Cell on that lane.

### 2.7 Admission and manifest compatibility

Admission remains a policy decision independent of mechanism availability. Before Tier 2
is qualified, an absent/unverified signature either follows current developer SAS policy or
is denied by `signing-required`; it must not be silently redirected to a fictional domain.

When the feature is enabled, the loader may select a domain only after it has established:
architecture support, kernel feature enabled, domain resource quota, an eligible artifact
class, and an effective capability/MMIO/DMA ceiling that the domain builder can enforce.
Failure of any check denies the Tier-2 request; it does not downgrade to SAS. Fleet policy
must retain the rule that unverified native code never enters SAS.

This work deliberately does **not** introduce manifest v3 or repurpose existing manifest
bits. A future manifest-v3 proposal may carry a requested execution class or domain
requirements, but it must be separately versioned, consent-reviewed, backward-compatible,
and approved after the runtime mechanism is proven. The first feature-gated implementation
may use an internal kernel/boot-policy selection only; no persistent application metadata
is changed by this gate.

## 3. Required negative test matrix

The implementation plan must turn each case into an automated target-architecture test or
record a hardware-gated reason and a corresponding release block.

| Case | Required result |
|---|---|
| Tier-2 code reads/writes/executes an unmapped peer Cell page | CPU fault attributed to the offending domain; peer bytes and execution state remain unchanged. |
| Tier-2 code probes kernel-only RAM, page tables, HHDM, or unassigned MMIO | Fault/deny; no user-readable alias exists. |
| Syscall pointer is null, overflowing, unmapped, cross-page, kernel, peer-domain, or concurrently unmapped | `copy_from_user`/`copy_to_user` returns the recoverable ABI error; the kernel neither panics nor reads/writes outside the caller's domain. |
| SAS → SAS schedule loop | No MMU-root write or mandatory TLB flush, verified by instrumented backend counter. |
| Domain transition and same-domain task switch | Correct root/tag active before user return; same-domain switch has no redundant root write. |
| ASID/PCID reuse after domain exit | New domain cannot read stale translation or data from the old domain. |
| Grant map/revoke racing receiver execution on another hart | No access after revoke completes; frame is not recycled before CPU shootdown acknowledgement. |
| Owner/grantee kill with grant and pinned DMA | Grant/CPU mappings and IOMMU mappings are removed or quarantined before frame reuse. |
| Forced exit during syscall or cross-hart migration | Domain enters `DYING`; all run queues are drained, every hart acknowledges the matching generation from a safe root, and the root is freed last. |
| Invalid signature, no signature, malformed ELF, unsupported arch, exhausted tag/table/quota | Deny with no task/domain publication and no SAS fallback. |
| Tier-2 Cell requests unauthorized MMIO, PCIe DMA, or virtio-MMIO DMA | Deny; virtio-MMIO remains disallowed until hardware DMA confinement qualifies. |
| Feature disabled or rollback boot | No Tier-2 option; existing Tier-1 and Tier-3 launch behaviour remains unchanged. |

Successful boot, an ASID write, or a positive copied-message test alone does not satisfy
this gate. At least one hostile native test must demonstrate that a private root cannot
reach peer SAS memory.

## 4. Feature flag, rollout, and rollback

The kernel `native-domains` build feature (name provisional) means only that a qualified
architecture backend is compiled. It is not an admission decision and, by itself, cannot
make the loader offer Tier 2. A separate boot-provisioned `native-domain-admission` policy
is default-off and is the sole persisted enablement control. Enabling it requires a reboot
into a build with the backend capability; installer UI remains hidden unless both controls
and the required test suite are present.

An emergency runtime disable is deliberately one-way for the current boot: atomically
change the admission policy from `ENABLED` to `DRAINING`, which linearizes before new
admission publication. An admission in progress holds the policy read generation through
validation and task/domain publication; it either publishes completely before that
linearization or observes `DRAINING` and fails with no SAS fallback. `DRAINING` rejects all
new Tier-2 launches, begins the `DYING` safe-root protocol for existing domains, and becomes
`DISABLED` only after they drain. It does not alter the persisted boot policy; a reboot with
that policy off is required for durable rollback. Existing domain Cells are never converted
in place to SAS. Disabling the policy leaves Tier-1 SAS scheduling and Tier-3 guests
unchanged. No manifest v3 data is written, so no on-disk application migration is needed.
Initial enabled rollout is developer-only with audit events for admission policy generation,
domain create, switch, grant map, revoke, fault, quiescence, and teardown; fleet enablement
requires a separate release decision.

## 5. Failure modes and non-claims

- A private CPU page table does not make unsafe native code safe; it limits the CPU-memory
  blast radius to what is mapped.
- Tier 2 does not mitigate microarchitectural side channels, kernel bugs, privileged-device
  DMA outside the qualified IOMMU/IOPMP path, or an intentionally granted shared page.
- ASID/PCID availability does not prove a correct switch. Incorrect reuse, stale TLB entries,
  copied global USER mappings, or missing remote shootdown are containment failures.
- A domain root is not safe to free just because its task faulted or exited. Missing a
  `DYING` acknowledgement, a safe-root switch, or a syscall-copy reference is a use-after-
  free of paging state and blocks release.
- A private root cannot make raw kernel dereference of a hostile syscall pointer safe. The
  domain-aware copy and recoverable-fault contract is part of the containment boundary.
- An IOMMU domain does not grant or restrict MMIO by itself; CPU page-table mappings and
  resource ownership remain separate checks.
- A valid signature does not create a domain, and a missing signature does not create one.
  Until this mechanism ships, unsigned native code in the SAS is not contained.

## 6. Evidence and follow-on owners

| Current evidence | Consequence for Tier 2 |
|---|---|
| `kernel/src/memory/paging.rs:38`, `:430-440`, `:604-617` | CPU mapping and activation are global-root operations today; domain-aware ownership is new work. |
| `kernel/src/task/scheduler.rs:683-692` and `kernel/src/task/tcb.rs:141-149` | Scheduler/TCB carry task register context, not an address-space identity. |
| `kernel/src/loader.rs:115-153` and `kernel/src/loader/mem_spawn_gate.rs:30-64` | Governed native loader and syscall spawn paths share signature/manifest admission; no tier-to-root selection exists. Trusted bootstrap init directly calls `task::spawn_from_mem` at `kernel/src/main.rs:877` and is an explicit exception that a Tier-2 implementation must audit rather than silently inherit. |
| `kernel/src/task/syscall.rs:612-632`, `:2175-2185`; `hal/arch/riscv/src/rv64/trap.rs:124-151` | Current user-buffer validation is not a domain-aware recoverable copy boundary, and kernel-mode faults panic; Tier 2 must replace raw ABI-pointer access with the §2.4 contract. |
| `kernel/src/task/syscall.rs:98-165`, `:4232-4294`, `:4361-4386` | Grants are global SAS identity mappings with ownership bookkeeping, not receiver-domain PTE mappings or synchronous revoke. |
| `kernel/src/task/syscall.rs:4476-4503` | MMIO ownership currently adds user visibility to the SAS mapping path; a domain-specific mapping path is required. |
| `kernel/src/task/drivers/iommu_riscv.rs:393-456` and `iommu_x86.rs:419-456` | DMA teardown already requires IOTLB/context invalidation before reuse; Tier 2 must compose, not bypass, that contract. |

Spec 02 needs a memory-lifetime addendum, Spec 17 needs a cross-domain wire/grant addendum,
and the architecture backends need independently reviewed switch and shootdown designs.
Those are implementation dependencies, not approvals granted here.

## 7. Cross-references

| Topic | Document |
|---|---|
| Tier definitions and current admission truth | `docs/specs/18-cell-trust-tiers.md` |
| Layer-B placement in the isolation roadmap | `docs/specs/19-hardware-isolation-layers.md` |
| Present security posture and DMA residual gaps | `docs/security-model.md` |
| IPC wire contract to amend before grants | `docs/specs/17-ipc-wire-contract.md` |
