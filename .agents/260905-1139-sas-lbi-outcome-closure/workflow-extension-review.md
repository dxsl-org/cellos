# Workflow Extension — Review and Validation

## Scope and evidence ceiling

2026-09-05 amendment: LAB-01, BASE-01, ASSEMBLY-01, ADR-0014 and bounded ORG-SRV-01/ORG-PC-01 scope; original phases01–05 remain required. Four read-only reviewers examined the planning contracts and relevant sources in parallel. Main adjudicated and amended the documents; no second reviewer approval, exploit reproduction, runtime test, physical trial or production qualification is claimed.

## Accepted reviewer findings

| ID | Reviewer / priority | Finding | Disposition |
|---|---|---|---|
| S1 | WorkflowSafetyReview / P1 | BASE and LAB could admit separate active jobs | ADR, master and phases06–08 now share one active workflow job. Composed steps retain its identity; another admission is rejected until completion or explicit reconciliation closes it. Phase08 requires a competing-admission negative case. |
| S2 | WorkflowSafetyReview / P1 | Stationary eligibility was admission-only | Require fresh stationary/support/lock eligibility throughout manipulation through carrier retention and arm parking. Phase08 A/B and authorized C matrices cover mid-job loss, independent inhibition, custody, reconciliation and explicit re-enable. No software timing safety claim. |
| S3 | WorkflowSafetyReview / P2 | Robot package gate could block unrelated physical development | ADR-0014 limits this gate to physical robot workflows/actuator energization; separately authorized non-actuating exact-device development retains its own ceiling. |
| O1 | OrganizationScopeReview / P2 | PDR retained conflicting dated G2 overlay | Replaced the lower overlay with ORG-SRV/ORG-PC scope and marked the dated technical timeline historical, not current scheduling/capability requirements. |
| Q1 | WorkflowSecurityReview / P1 | VFS helper accepted wildcard replies and ignored syscall errors | Source-confirmed in `libs/ostd/src/fs.rs:280-291`; existing sender-bound/error-propagating precedent is `libs/ostd/src/ipc.rs:39-64`. Phase05 now owns the source fix and foreign-reply/error witnesses; 06B consumes them before claiming acknowledgement. The source bug remains to be fixed during implementation; documentation is not remediation evidence. |
| Q2 | WorkflowSecurityReview / P1 | Tier2 guide implied an available application deployment route | Source-confirmed internal/default-off admission in `kernel/src/loader/domain_admission.rs:1-5`. Guide retains implemented RV64/QEMU substrate/migration evidence but explicitly states test-hooks-only, no public application loader/admission. Untrusted work needs a qualified guest boundary or remains blocked; no Tier1 fallback. Master/ORG profile reflect that distinction. |
| D1 | WorkflowDependencyReview / P2 | Independent A acceptance could duplicate unfrozen shared contracts | 07A implementation/acceptance consumes accepted 06A; 08A consumes accepted 06A+07A. Drafting can overlap under WIP without whole physical-phase dependencies. Master rows, milestone gates, phase instructions and handoff agree. |

Seven reviewer findings accepted: four P1 and three P2. Planning corrections and future implementation gates resolve the document findings; they do not claim the underlying VFS runtime defect or physical risks are already resolved.

## Additional integration corrections

- Preserve the owner's combined wheeled two-leg concept, not a forced legs-versus-wheels choice. Exact mechanism/controller/support remains unselected.
- Phase06C explicitly applies ADR0006 only to production claims; authorized physical development is not gated on wholesale production closure.
- G2 remains scope-defined and unactivated, independent of robot physical completion. Reference applications are proposals, not current compatibility results; all must-have cohort rows remain required.

## Validation method and result

The companion [validation.json](./validation.json) records the executed planning-only checks: all eight phase numbers/templates and line limits; whole-phase and milestone dependency graphs; relative file links and heading fragments; explicit source-file references versus nine proposed source paths. The validation is a throwaway inspection, not a permanent source-text test or program smoke test.

Physical packages/authorizations and production gates are separate from DAG edges. Phase04 supplies the Phase05-consumed native-workload budget rows without gating on unrelated scorecard rows. A/B software acceptance does not complete a phase with an unexercised C outcome.

## User decisions retained

- LAB first: one identified dry inert closed carrier, indexed rack A to B, independently observed placement/release plus trace acknowledgement/readback.
- BASE extension: secured tray H-A to H-B on an exact approved configuration, no new walking/self-balance algorithm.
- ASSEMBLY extension: stationary coupling/decoupling plus standalone upper LAB, standalone BASE and assembled stationary LAB outcomes.
- Organization target: real web/app/microservice servers and ordinary local-office PCs; specialized devices excluded from entry scope. Actual applications, migration, security and operational control determine cohort acceptance.
- Execution handoff: [master plan](./plan.md); Phase01 and first product milestone06A under WIP. No procurement, actuator/sensor activation, public-interface change, remote enablement or production approval follows from this planning amendment.
