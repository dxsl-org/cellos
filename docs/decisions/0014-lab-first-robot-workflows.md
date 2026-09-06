# ADR-0014: Choose lab-first workflows with gated base and assembly extensions

**Date**: 2026-09-05  
**Status**: Accepted — product/workflow ordering and planning scope only  
**Decider**: Cellos maintainer, through the recorded user approval in this planning session

## Context

The owner has three opportunities, all still on paper: a modular wheeled two-leg
robot whose upper module can serve a laboratory, an organizational sovereign OS,
and a general military robot swarm requiring both robots and software. The lab
job is to assist researchers with precise, hazardous, or repetitive work.
The owner approved lab-first focus and requested a concrete workflow, explicit
base/leg and assembly workflows, and an amendment to the existing implementation
plan. This approval does not select hardware or prove any robot behavior.
The owner then narrowed organizational deployment to web/application/microservice
servers and ordinary office PCs with common basic software, not specialist devices.
That G2 scope is recorded separately; it does not change the selected lab-first WIP.

Cellos has trusted native Cells, ownership/lifecycle primitives, and existing
bench, VFS, hot-swap and QEMU fixtures. The approved SAS/LBI plan targets closure of
measurement validity, grant/quota ownership, stateful replacement and a real
VFS-backed native counter workload. Those outcomes remain required; a counter
is engineering evidence, not a physically successful laboratory operation.

[ADR-0007](./0007-development-first-hardware-constrained-execution.md) permits
useful development with available assets, not speculative procurement or robot
qualification. [ADR-0013](./0013-solo-first-development-independent-promotion.md)
permits solo development but does not replace accountable physical/safety or
independent production approval. [ADR-0006](./0006-block-production-root-pending-exact-product-evidence.md)
continues to govern production-root selection.

## Decision Drivers

- Deliver one bounded, observable laboratory job before general robot capability.
- Keep lab, base and assembled modes coherent without three competing programs.
- Separate observed physical completion from software command acknowledgement.
- Preserve uncertainty after interrupted physical actions; do not blindly replay.
- Reuse existing native SDK/bench/VFS fixtures rather than create generic robotics middleware.
- Keep software, exact-device hardware, safety and production evidence distinct.
- Make operational sovereignty testable through ownership, build, update and handover, not a promise of zero dependencies.

## Considered Options

### A. Lab first; base and assembly are explicit gated extensions — chosen

- **Pro:** A dry carrier-transfer job has a bounded input/output and an independently checkable result.
- **Pro:** Base and assembly contracts can be specified now, while scarce implementation work follows the lab outcome.
- **Pro:** Software-only development can proceed without pretending unavailable actuators exist.
- **Con:** Full physical outcomes remain blocked until exact mechanics, controllers, sensing, metrology and safety packages exist.
- **Chosen because:** It connects native lifecycle work to a real product task without making hardware breadth the first deliverable.

### B. Complete the humanoid form, locomotion and hot docking first — rejected

- **Pro:** Demonstrates the original combined-robot concept directly.
- **Con:** Couples new mechanics, stability, sensing, manipulation and OS work before one lab task is measured.
- **Rejected because:** Two legs, two arms and detachability are not yet demonstrated requirements of the selected lab task; motion/hot docking cannot be treated as routine software lifecycle.

### C. Start the general military swarm or universal Windows/Linux replacement — rejected as first program

- **Pro:** Addresses the broader opportunities directly.
- **Con:** Requires end-to-end system/application/hardware organizations and acceptance beyond current platform evidence.
- **Rejected because:** Neither is a necessary dependency of the lab workflow. No combat/swarm implementation or universal application-compatibility commitment is authorized by this decision.

### D. Call a simulation/counter demonstration the robot product — rejected

- **Pro:** Would produce an earlier apparent completion.
- **Con:** A model, command acknowledgement, or persisted counter cannot establish placement, stability, precision, physical safety or production qualification.
- **Rejected because:** It would silently replace the owner's end-to-end requirement with a smaller software PASS.

## Decision

1. **LAB-01 is the first product workflow.** Transfer one closed, inert dummy
   sample carrier with an identity from an indexed source slot in rack A to an
   indexed destination slot in rack B. Confirm the intended carrier's placement
   and preserve a traceable outcome. No carrier opening, liquids, hazardous
   specimens, clinical task, or autonomous experiment selection is included.
2. **BASE-01 is the base/leg extension.** Transport a secured tray of dummy
   carriers between two named handoff stations in a controlled flat test area.
   Physical execution requires the exact base's independently assessed stability,
   controller, braking/holding and protective mechanisms. This is not approval to
   develop walking, jumping or self-balancing algorithms, nor proof that an
   unsupported two-wheel/two-leg configuration can safely move.
3. **ASSEMBLY-01 is the integration extension.** Couple/uncouple the base and
   upper/lab module while stationary in a validated support/power arrangement;
   verify configuration, mechanical locking and safety eligibility before an
   intentional re-enable. Exercise LAB-01 with the assembled robot stationary,
   and verify the declared independent configurations separately. No hot docking
   or automatic motion after reconnect is authorized.
4. **Separate manipulation and transport in the first combined mode.** One
   active workflow job is shared across LAB-01, BASE-01 and ASSEMBLY-01; composed
   steps retain that job identity, not separate concurrent admissions. Other
   requests are rejected until completion or explicit reconciliation closes it.
   The arm does not manipulate while the base moves. Stationary/support/lock
   eligibility must remain fresh throughout manipulation until carrier retention
   and arm parking; independent protection handles eligibility loss. Tray handoff
   requires an eligible receiver and independently verified tray identity/securing.
5. **Physical truth outranks cached state.** Command acceptance is not completion.
   A crash, uncertain observation, power event or configuration change during an
   operation produces an indeterminate/reconciliation-required outcome unless
   the physical result is independently established. Never blindly replay a
   physical side effect from a memory snapshot or VFS checkpoint.
6. **Safety is independent of the general-purpose runtime.** Hardware/controller
   protection must reach the risk-assessed safe disposition if Cellos, Linux or
   communication fails. Removing power alone is not presumed safe for gravity
   loads or an unstable base. Software lifecycle/admission is not safety certification.
7. **One product outcome remains active.** Preserve SAS/LBI phases 01–05 and add
   phases 06–08. Contract/host-model work may proceed under local prerequisites;
   native QEMU milestones consume the real existing fixture outcomes. Missing
   physical milestones do not block independent software work, but the full
   workflow phase cannot be declared complete without its physical criteria.
8. **No blanket interface or procurement authority.** Public API/types/syscalls,
   wire layouts, KMS ABI and Manifest-v3 retain exact design/implementation gates.
   No generic robot ABI, production-driver placeholder, cross-board SAS pointer,
   sensor activation, motor actuation, procurement, remote/public C2C bypass or
   production promotion is authorized here. Physical LAB-01/BASE-01/ASSEMBLY-01
   operation or actuator energization first needs an exact hardware/interface/
   metrology/safety package and applicable approvals. Separately authorized
   exact-device, non-actuating development retains its own non-production ceiling;
   it does not wait for a complete robot package.
9. **Sovereignty is an operational contract.** The organizational target is
   control over data, keys, builds, updates, maintenance and supplier exit for
   the selected machines. A single-person knowledge dependency is not removed
   by writing a new kernel. A requirement to exclude Windows/Linux must still
   be met if contractual; a guest is not literal elimination of its OS.
10. **G2 has two bounded organizational profiles.** ORG-SRV-01 covers real
    web/app/microservice hosting; ORG-PC-01 covers local office/browser workflows.
    Their application/device acceptance matrix determines replacement for the
    selected organizational cohorts. Specialist equipment is not an entry
    requirement. These are scope-defined targets, not two newly active programs,
    and robot physical acceptance is not their technical prerequisite.

## Consequences

- LAB-01, BASE-01 and ASSEMBLY-01 share logical identity/handoff requirements,
  not a preapproved public wire format or distributed motion-control protocol.
- Exact carrier dimensions/mass, fixture geometry, sensors, controllers, numeric
  tolerances, measurement methods and safety responsibility are physical entry
  prerequisites. They are not invented from the product sketch or QEMU timing.
- Current Pi3 boards remain development assets, not robot or production qualification.
- Tier 2 has implemented RV64/QEMU substrate and cross-hart migration evidence;
  physical containment, DMA quarantine and production approvals remain open.
  This decision neither erases that work nor promotes it into a robotics safety boundary.
- Software rollback cannot undo a moved carrier or a mechanical coupling event.
  Physical recovery requires observation and the approved operator procedure.
- Future full-body mobility, hazardous lab tasks, arbitrary apps and additional
  deployment profiles need their own requirements and evidence; none is silently
  substituted for the selected dry-carrier workflow.

## Links

- [SAS/LBI and lab-first execution plan](../../.agents/260905-1139-sas-lbi-outcome-closure/plan.md)
- [LAB-01 phase](../../.agents/260905-1139-sas-lbi-outcome-closure/phase-06-lab-carrier-transfer.md)
- [BASE-01 phase](../../.agents/260905-1139-sas-lbi-outcome-closure/phase-07-base-tray-handoff.md)
- [ASSEMBLY-01 phase](../../.agents/260905-1139-sas-lbi-outcome-closure/phase-08-stationary-assembly.md)
- [Organizational server and office profiles](../../.agents/260905-1139-sas-lbi-outcome-closure/organization-deployment-profiles.md)
- [Opportunity analysis](../../.agents/reports/research-260905-1305-cellos-three-deployment-opportunities.md)
- [Product stages](../roadmap/product-stages.md#g1---robot--embedded)
- [Current focus](../roadmap/current-focus.md)
- [Capability lanes](../project-roadmap.md#capability-lanes)
