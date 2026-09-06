# Product Stages

**Last updated**: 2026-09-05

## Execution Relationship

G1–G5 define product and release outcomes. They do not impose a global
G1→G2→G3 implementation order. Each capability is scheduled independently by
its documented dependency, `execution_class`, and `evidence_ceiling`; see the
[capability lanes](../project-roadmap.md#capability-lanes). A result may advance
only to the evidence class actually exercised. In particular, host/QEMU results
never satisfy physical, service, or production requirements.

## Development inventory and planning classes

[ADR-0007](../decisions/0007-development-first-hardware-constrained-execution.md)
authorizes development-first execution with QEMU, two owner-reported Raspberry
Pi 3 Model B+ boards, and incoming sensors, with no additional procurement now.

| Roadmap item | Planning class | Stage relationship |
|---|---|---|
| G1 QEMU, RPi3, sensor, and local-runtime integration | Current executable work | Advance now to the lane-specific software or development-hardware ceiling |
| Confirmed defects in currently supported paths | Current-scope technical debt | Track in the [open risk register](open-risk-register.md); do not use this label for all future work |
| G2 expansion, remote/public C2C, G3, G4, and G5 outcomes | Future capability | May be designed or implemented only through independently opened lanes; they are not defects in G1 |
| Unavailable exact boards, protected relay assets, cloud identity, and exact production-root evidence | External-gated prerequisite | Block only the milestone requiring that external evidence |
| Production admission and governed release closure | Production release gate | Remain disabled and fail-closed until every applicable security, hardware, evidence, approval, and ledger invariant passes |

QEMU provides software evidence only. RPi3 and sensor exercise may provide
development/hardware-integration evidence for the exact device, but RPi3 is
never a production-security qualification target or a qualified external
floor. No stock TPM or generic secure-element counter is selected as that floor.
Remote C2C,
protected relay identity, production KMS/root, secure/measured boot, a qualified
rollback-resistant external floor, physical hostile evidence, an authenticated
runner, required human approvals, and release-ledger closure remain mandatory
only for the applicable production-admission or production-release claim; they
do not serialize QEMU, RPi3, sensor, or local-runtime work.

## G1 - Robot & Embedded

Goal: a bounded, locally operated native platform for specialized laboratory
equipment on RV64/ARM64 SBC-class systems, with measured recovery and I/O behavior.

Required evidence:

- Real board boot evidence for promoted hardware lanes.
- Peripheral I/O through capability-gated driver cells or audited kernel
  integration paths.
- Bounded memory and stack posture per Cell.
- Clear separation between QEMU integration proof and physical hardware proof.

[ADR-0014](../decisions/0014-lab-first-robot-workflows.md) selects LAB-01 dry,
identified carrier transfer as the first product workflow. BASE-01 tray transport
and ASSEMBLY-01 stationary coupling are planned extensions, not simultaneous
active product programs. Their [execution plan](../../.agents/260905-1139-sas-lbi-outcome-closure/plan.md)
separates host/QEMU milestones from exact-device physical acceptance; robot
hardware, precision, safety and production remain unqualified by software results.

## G2 - Organization Servers & Office PCs

Goal: replace Windows/Linux for the organization's selected web/application/
microservice server and ordinary office-PC cohorts, verified against actual
applications, peripherals, security and operational requirements. Specialist
devices are not an entry requirement.

[ORG-SRV-01 and ORG-PC-01](../../.agents/260905-1139-sas-lbi-outcome-closure/organization-deployment-profiles.md)
define the functional floors and proposed reference applications. They are
scope-defined future profiles, not newly activated implementation programs or
proof of application compatibility. A Linux guest is a disclosed transition
dependency, not elimination of Linux; native, guest and remote claims remain
distinct. Their activation does not depend on completing physical robot workflows.

Current posture:

- x86_64 has implementation and QEMU/Ring-3 smoke evidence, but physical PC
  qualification remains target-specific.
- Untrusted Linux/POSIX application compatibility belongs in Tier 3 VM paths,
  not native Tier 1 cells.

## G3 - NPU-native Compute OS

Parked until hardware exists and the team has vendor API experience. The
contract for accelerators must be hardware-informed; avoid over-specifying
`ViAccelerator` before RKNN/Hailo/K230/P870-class evidence exists.

The first evidence target is RK3588/RKNN; X390 remains the second implementation
after usable silicon and software are available. The maintained readiness and
license gates are in [G3 Accelerator Evidence Envelope](../research/g3-accelerator-evidence.md).

## G4 - Full Rust std for Tier 1 Cells

Direction: a Tier 1 `rust-std` runtime profile using pure-Rust PAL plus a custom
`*-unknown-cellos` rustc target. Do not route native Tier 1 `std` through mlibc,
because that pulls C/POSIX assumptions into the trusted Tier 1 path.

The bounded kernel CWD/path lane is complete with paired fault-free release-boot
and immutable-FAT test-hooks marker evidence. It covers canonical
caller-attributed relative `open`, `remove`, `chdir`, exact non-NUL `getcwd`,
and VIFS1 FAT `stat`. Caller-scoped shell `cd`/`pwd`, the fixed-width
kind/access/size `fstat` contract, and typed VFS
`stat`/`unlink`/`rename`/`mkdir`/`rmdir` are also complete. These remain narrow
native contracts, not POSIX compatibility. Additional C wrappers, symlinks, new
ABI work, and broad POSIX support remain future capabilities.

## G5 - Virtualization Platform

Research/design overlay after G4. The intended shape is one VMM core with
profiled Tier 3 guest modes, not two separate codebases. Golden-frame poisoning
remains a named trust-anchor risk before production use.
