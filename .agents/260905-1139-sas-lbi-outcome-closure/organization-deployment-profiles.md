# Organization Deployment Profiles — ORG-SRV-01 and ORG-PC-01

**Decision date:** 2026-09-05  
**Status:** Product scope defined; implementation lanes not yet activated  
**Owner:** Main for profile integration; accountable deployment/acceptance owners must be named before activation

## Approved Scope

The owner narrowed organizational Windows/Linux replacement to web/application/
microservice servers and ordinary office PCs running common basic applications.
Highly specialized devices are not an entry requirement. This supersedes the
previous suggestion that organizational deployment should first target specialist
machine controllers; the lab robot remains a separate first product workflow.

This document fixes the functional floor and a proposed reference application set.
It does not claim any named application currently runs on Cellos, authorize a
runtime/ABI port, or declare these two G2 profiles active alongside LAB-01.
G1 and G2 are independent product lanes rather than sequential stages: neither
requires completion of the other. LAB remains the current product WIP until an
explicit scheduling decision activates a bounded G2 profile; shared platform
changes must still obey their own dependencies and file ownership.

## ORG-SRV-01 — Web, Application and Microservice Server

### Functional floor

- Serve a real HTTP(S) site through a front-end/reverse proxy and two separately
  managed application services communicating through documented APIs.
- Complete a client-visible transaction that persists data and can be read back
  after the relevant service restart and after approved backup/restore exercises.
- Run the selected application runtime and database, not just a native echo
  endpoint or two Cells exchanging messages. Microservice deployment is an
  application/operations contract, not a synonym for kernel IPC.
- Demonstrate deploy/update, configuration/secrets handling, identity/TLS,
  logging, access control, resource limits, recovery and operator handover on the
  chosen hardware/cohort. Service disruption and data semantics must be explicit;
  no universal zero-downtime or exactly-once claim is implied.
- Prove the selected isolation boundary for the deployed services. Arbitrary
  third-party runtime/extension code is not automatically trusted Tier1 SAS code.

### Reference application proposal

Use **Nginx + two Node.js application services + PostgreSQL** as the initial
compatibility target unless the organization's actual must-have inventory
requires another supported runtime/database. Pin exact versions, extensions,
application source, client transaction corpus, configuration and licence terms
before implementation. This is a proposed target, not an existing Cellos stack.
A Java/.NET/Python requirement cannot silently pass through a Node-only witness.

No Docker/Kubernetes parity is inferred or required by the words “microservice”.
If the organization requires those interfaces, record them as explicit workload
requirements; do not mark the profile accepted while omitting them.

## ORG-PC-01 — Ordinary Office Workstation

### Functional floor

- Local graphical login/session, file operations, normal display/keyboard/mouse
  and networking on an identified commodity PC configuration.
- Browser use for the organization's actual internal/web applications, downloads,
  uploads, authentication and webmail if that is the chosen mail workflow.
- Locally open, edit, save, reopen and exchange ordinary documents, spreadsheets
  and presentations; view/export PDF and print through the selected ordinary
  office printer path. Preserve the required document content and formulas.
- Correct fonts, keyboard/input method, local-language text and required
  accessibility. Corporate identity, certificate/token needs are included when
  they are ordinary prerequisites of the selected users, not dismissed as
  specialized equipment.
- Updates, backup/restore, user separation, application isolation and recovery
  are part of acceptance. A compositor/counter demo is not an office workstation.

### Reference application proposal

Use **Firefox + LibreOffice Writer/Calc/Impress**, with browser-based PDF viewing
and webmail where applicable. Pin exact versions and a real representative corpus
of organizational sites/documents, including required interchange formats and
spreadsheet calculations. These applications are not claimed to run on Cellos.
If Microsoft-specific macros, add-ins or signatures are mandatory, explicitly
port/replace/bridge them under an approved disposition or fail that workload;
calling them “non-basic” after a failed test is not acceptance.

No games, CAD, specialist lab/industrial peripherals or universal GPU/driver
coverage are initial targets. A web-only terminal or remote desktop is not a
substitute for this local-office profile without an explicit scope amendment.

## Replacement and Sovereignty Claims

For every application/workload, record one of these execution dispositions:

| Disposition | Truthful claim | Not implied |
|---|---|---|
| Native Cellos with qualified application boundary | Named workload runs on Cellos without a Windows/Linux guest | All applications, drivers or machines are replaced |
| Cellos host + explicitly approved Linux guest | Host platform replacement with declared Linux compatibility dependency | Elimination of Linux from the runtime stack |
| Explicit external/remote compatibility service | Named user workflow remains available through a disclosed dependency | Local native compatibility or independence from that service |

Guest or remote dispositions require purchaser acceptance. A strict no-Windows/
no-Linux runtime requirement rejects those dispositions for that requirement.
Current Tier3 Linux work is not Windows guest support, a desktop application
compatibility result, or permission to ignore application/guest licence terms.

Sovereignty acceptance covers organization-controlled data/keys, build materials
and rights, update/revocation authority, operation without an external licensing
or cloud dependency where required, an SBOM/dependency register and independent
maintenance/handover. A kernel rewrite alone does not satisfy these outcomes.
Security, operational autonomy and application compatibility need separate proof.

## Activation and Implementation Handoff

1. Freeze the actual user/service cohorts, exact hardware, must-have applications,
   workflows and migration/rollback owners against the functional floors above.
2. Complete a source-grounded compatibility matrix per application: runtime/ABI,
   graphics/filesystem/network/driver dependencies, execution tier, licence,
   proposed port or disclosed bridge, and observable pass/fail criteria.
3. Inventory actual gaps before choosing a port sequence. Native Rust std,
   POSIX/FFI breadth, browser/runtime dependencies and physical x86 qualification
   are not supplied merely by accepting this profile. Tier2 has RV64/QEMU
   substrate and cross-hart migration in internal test-hooks, but no public
   application admission/loader route; physical containment, DMA quarantine and
   production approval remain open (`docs/project-roadmap.md`, Current Codebase Facts).
4. Obtain exact Law1/runtime/driver/security checkpoints for affected interfaces,
   then activate a separately owned phase plan for the chosen profile. Reuse
   applicable SAS/LBI baseline/ownership evidence. G2 does not depend on G1 or
   LAB/BASE/ASSEMBLY physical success; conversely, G1 does not depend on G2
   application compatibility. Shared platform evidence is consumed only where
   its exact contract/profile applies.
5. Start with one identified server or office-PC configuration per activated
   profile; hardware variety follows validated demand. No procurement or fleet
   rollout is authorized by this scope decision.
6. Measure real application behavior, isolation, load/resource behavior and
   recovery on the exact image and hardware; freeze workload-specific numerical
   budgets before runs. QEMU results retain their software-only ceiling.
7. Complete cohort-level pilot, migration/rollback, security, production gates and
   operator handover before claiming organizational replacement for that cohort.
   Rejected/unsupported must-have rows remain failures, not silently removed scope.

## Planning Touchpoints and Risk

Use the existing [product-stage overlay](../../docs/roadmap/product-stages.md),
[application guide](../../docs/app-development-guide.md), runtime/platform tracks,
Tier3 platform lanes and [main plan](./plan.md). No new implementation source
locations are selected before the compatibility matrix and exact design review.
Do not turn these scope-defined targets into two additional active engineering
programs by automatically opening G2/G4/G5 wholesale.

Rollback a pilot through its tested backup/restore and prior deployment route;
software rollback alone does not repair already transformed organizational data.
Retain licence, security and data-integrity evidence through a migration reversal.
The full organization is replaced only when its in-scope cohorts and every
must-have workflow pass; the profile names themselves are not that evidence.
