# ADR-0022: Make Intel x86-64 Cell-to-Cell Anywhere the sole development direction

**Date**: 2026-10-08
**Status**: Accepted — direction and scheduling; not implementation or hardware qualification
**Decider**: Cellos maintainer, through explicit approval in the planning conversation

## Context

Cellos has one maintainer, no committed funding, and more platform and application
programs than can be completed together. The owner selected Cell-to-Cell Anywhere
(C2C) as the research focus, then explicitly chose Intel x86-64 as the main and
only hardware direction for the coming period. Every task must serve that focus.
This decision does not establish a commercial advantage over Linux or a funded
product requirement.

Existing evidence is uneven: RPi3 has physical development records but its native
Ethernet path remains unresolved; x86 controller-family lanes have QEMU evidence
but the physical HCL is empty. The x86 guest backend has AMD SVM/QEMU evidence,
not Intel VMX qualification. The x86 DMA path implements Intel VT-d; SVM does not
substitute for AMD-Vi. x86 Tier 2 admission remains test-image-only and the existing
C++ freestanding profile excludes x86. These are required gaps to close, not
capabilities supplied by choosing an ISA.

## Decision drivers

- Reduce simultaneous work to one outcome and one reproducible hardware target.
- Exercise native Cells, isolated C/C++ workloads and Linux guests in one C2C
  service model, with explicit authority, lifecycle and failure semantics.
- Keep enough memory, storage and network capacity to measure useful workloads
  and compare them with Linux on the same hardware.
- Preserve truthful evidence, security gates and existing code without continuing
  every historical roadmap as an independent program.

## Considered options

1. **Intel x86-64, one fixed headless configuration — chosen.** Aligns the target
   with the ACPI/VT-d and PCIe controller work already present, and permits a
   repeatable two-node setup. Costs include physical bring-up, Intel VMX, x86
   C/C++ runtime support and Tier 2 qualification; none is assumed complete.
2. **Existing ARM/RPi3 as the only long-term target — rejected.** Avoids immediate
   procurement and retains real-board guest evidence, but unresolved USB Ethernet
   and the Pi3 resource envelope can dominate multi-tier C2C experiments. Existing
   boards and evidence are retained; no new ARM board is opened as a workaround.
3. **AMD x86-64 because SVM exists — rejected.** The QEMU SVM result is not a
   qualified AMD machine, and the current Intel VT-d path does not supply AMD-Vi.
   It would add another platform/isolation program instead of narrowing scope.
4. **Continue robotics, GUI, AI and multi-architecture programs independently —
   rejected.** Useful individual features do not justify competing roadmaps for
   an unfunded solo effort. Availability of a board or an old ready checkbox is
   not sufficient to authorize new work.

## Decision

### One program and one task-admission rule

**C2C Anywhere on Intel x86-64 is the sole active development direction until the
owner explicitly changes it.** The research target is a headless Cellos node,
then two identified Intel machines communicating over a real network. A terminal
is an operational/debugging surface, not a separate terminal product. GUI is not
a prerequisite.

Every new or resumed task must record:

1. The named Intel C2C milestone or consumer it serves.
2. Whether it is a direct deliverable, necessary prerequisite, measured bottleneck
   or regression/security/build repair protecting that path.
3. Its observable acceptance scenario, execution environment and evidence ceiling.

If that connection cannot be stated, the task is parked. “Useful to an OS” or
“might be useful later” does not pass. Unrelated security/build failures may be
repaired only to preserve the integrity of the shared baseline needed by C2C;
this is not a side-feature budget. Work-in-progress is one implementation slice;
independent review/evidence preparation must not create competing file ownership.

The [current focus](../roadmap/current-focus.md) owns the milestone projection;
the [plan portfolio](../../.agents/plan-portfolio.md) owns scheduling of child
plans. A child plan's old `active`, `ready`, `next` or unchecked entry cannot
override this decision. Existing ABI review and implementation entry gates still
apply to an otherwise in-scope task.

### C2C scope and completion

- Keep the three execution tiers. Native Tier 1, isolated Tier 2 C/C++ consumers
  and Tier 3 Linux guest adapters are parts of the destination, not optional
  substitutes for each other. Language alone does not redefine a protection tier.
- Unify service contracts, identity/authority and lifecycle. Local calls, LAN and
  relay remain distinct transports with explicit costs and failure modes. Shared
  SAS addresses or ownership pointers never become network references.
- Exercise bounded requests, cancellation/timeouts, service restart, stale
  references, denied authority, disconnect/reconnect and indeterminate outcomes.
  Do not promise automatic distribution of arbitrary applications, transparent
  shared memory across hosts or safe retries of non-idempotent work.
- Preserve the existing tier-aware C2C plan's phase-local contracts. Its current
  guest bridge does not automatically authorize cross-node guest routing; any
  missing route needed for the all-tier destination needs explicit contract and
  authority review before implementation or a completion claim.
- Measure local overhead and end-to-end latency, throughput, resource use and
  recovery on identified workloads. Compare a relevant Linux implementation on
  the same hardware and semantics. No claim of universal or unmatched performance.

### Hardware and dependency order

1. Reuse existing x86 QEMU/controller evidence and qualify the local C2C contract.
   Fix shared kernel/network/storage defects only as dependencies of this path.
2. Select and qualify **one exact Intel machine configuration**: VT-x/EPT and
   VT-d exposed by firmware, supported wired PCIe NIC and storage, and the console,
   HPET and boot requirements in the [HCL](../hardware-compatibility-list.md).
   Keep one NIC/storage path; do not broaden device support for convenience.
3. Complete x86 Tier 2 admission/runtime support and Intel VMX prerequisites for
   the corresponding C2C consumers. SVM/TCG results remain reference evidence,
   never evidence that Intel guests work.
4. After the first machine's required bring-up gates pass, qualify a second Intel
   node and exercise real two-node C2C. Reusing the configuration is preferred
   to reduce support work, not required; a different model needs independent
   qualification and bounded driver scope. Software-only two-node QEMU work
   can precede procurement but cannot satisfy physical acceptance.
5. Close guest participation, remote/relay authority and performance milestones
   under their own prerequisites. The final destination remains all three tiers;
   passing a local native demo is not C2C Anywhere completion.

This is dependency ordering, not permission to block all software work on buying
hardware or all local work on production security. The exact next executable
slice is selected through the portfolio after its entry gates pass.

### Paused scope and preserved assets

New AMD, ARM, RISC-V and MCU bring-up; RPi3 NIC/peripheral expansion; robotics and
LAB/BASE/ASSEMBLY acceptance; desktop/ViUI/browser development; AI/NPU/GPU programs;
general office/server replacement; and standalone runtime breadth are parked.
Existing code, tests, board records and completed evidence are not deleted. Shared
changes may run existing non-Intel regressions when needed to protect C2C's
substrate, without promoting those platforms to active development targets.

A historical non-Intel security/authority plan may supply evidence or a necessary
C2C dependency, but it does not automatically authorize another platform program.
Identify the minimum required slice and obtain its existing approvals. An actual
change of the sole direction requires a new explicit owner decision; passing an
old plan trigger or obtaining a board is insufficient.

### Authorization and evidence boundaries

- No hardware purchase, cloud spend, irreversible provisioning, ABI change or
  implementation approval is implied by this strategy decision.
- No change to protected relay identity, authenticated time, replay protection,
  production roots, signing, secure/measured boot or independent approval gates.
  No insecure fallback may be introduced to make a demo appear complete.
- The evidence ladder remains `none → contract → host → QEMU → physical → service
  → production`. Buying an Intel CPU, QEMU DHCP or an SVM guest boot raises none
  of the other evidence classes.
- [ADR-0013](0013-solo-first-development-independent-promotion.md) remains valid:
  solo development is allowed; automated agents do not provide independent human
  ratification where that is required.

## Consequences and supersession

- Supersedes [ADR-0007](0007-development-first-hardware-constrained-execution.md)
  only for independent-lane scheduling and active hardware scope. Its truthful
  evidence and fail-closed production boundaries remain.
- Supersedes [ADR-0014](0014-lab-first-robot-workflows.md) as the current product
  workflow ordering. Its lab contracts/evidence remain historical, not a queue.
- Preserves [ADR-0015](0015-dual-mode-hybrid-architecture.md) as the tier baseline
  and [ADR-0008](0008-protected-relay-tls-endpoint-ownership.md) as the relay
  endpoint ownership boundary. It does not rewrite their implementation claims.
- Reduces parallel breadth but does not remove substantial Intel hardware,
  virtualization, C/C++ and distributed-systems work. No delivery date, hardware
  budget, production readiness or market demand is asserted.
- Revisit the direction only through an explicit owner decision based on a
  concrete funded requirement, demonstrated technical blocker or measured result;
  record the changed scope rather than silently reopening old programs.
