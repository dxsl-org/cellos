# Product Stages

**Last updated**: 2026-10-08 — G1–G5 retained as parked historical product definitions

## Execution Relationship

[ADR-0022](../decisions/0022-intel-x86-64-c2c-only-direction.md) replaces the
G1–G5 scheduling overlay with one program: **C2C Anywhere on Intel x86-64**.
The stage descriptions below preserve earlier goals and evidence requirements;
they are not active programs or an execution queue. A stage's local trigger
cannot reopen it. Only a named Intel C2C dependency may be admitted through the
[portfolio](../../.agents/plan-portfolio.md), with existing technical/ABI/security
gates intact. A change of direction needs an explicit new owner decision.

## Development inventory and planning classes

Use the [current focus](current-focus.md) for executable scope and milestone
projection. Existing ARM/RPi3/RISC-V/AMD assets and evidence are retained, not
expanded. Intel hardware procurement and physical qualification remain separate
gates; no physical Intel machine is qualified by this direction decision.

The three-tier target is not delayed until a hypothetical G4/G5 product release:
x86 Tier 2/C++ and Intel VMX are direct C2C dependencies, each requiring its own
qualification. GUI, robotics, AI and standalone runtime expansion are parked.

Host/QEMU results never establish physical, service or production qualification.
Protected relay identity, authenticated time, signing, production roots and
required approvals remain mandatory where their contracts require them.

## G1 - Robot & Embedded

Goal: a bounded, locally operated native platform for specialized laboratory
equipment on RV64/ARM64 SBC-class systems, with measured recovery and I/O behavior.

Required evidence:

- Real board boot evidence for promoted hardware lanes.
- Peripheral I/O through capability-gated driver cells or audited kernel
  integration paths.
- Bounded memory and stack posture per Cell.
- Clear separation between QEMU integration proof and physical hardware proof.

[ADR-0014](../decisions/0014-lab-first-robot-workflows.md) previously selected LAB-01
dry carrier transfer first, with BASE-01 and ASSEMBLY-01 as extensions. All are now
parked under ADR-0022. Their historical
[execution plan](../../.agents/260905-1139-sas-lbi-outcome-closure/plan.md)
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
- A native CPU inference path exists at the `host` and `qemu` ceilings
  ([Spec 24](../specs/24-ai-inference-architecture.md) CP-1..CP-3): the
  `/bin/ai` service answers typed-IPC inference requests and generates tokens
  from real GGUF weights with no Linux guest. This is a capability, not an
  application-compatibility or performance qualification result; NPU and GPU
  backends remain gated, and no organization cohort has been activated.

- x86_64 hardware prerequisites for this cohort are recorded in
  [hardware-tracks.md](hardware-tracks.md): a generic `x86_64-pc` descriptor and
  the HCL model landed in phase 01, but there is no machine-specific descriptor
  and no HCL machine row; the AHCI/SATA storage family landed in phases 02a/02b
  (`ahci-x86` 5/5 with a two-boot persistence oracle); xHCI is being reopened for
  this lane by the owner's 2026-10-05 decision; e1000 binding 82540EM only and
  fail-closing all other Ethernet classes; no ACPI DMAR discovery; and no Intel
  VMX backend. What is witnessed is the q35 software lane reaching a COM1 shell;
  a SATA-only industrial PC is expected to do the same but is unqualified, and
  would still have no persistent storage, no real NIC, and no USB input.
- A 16550-compatible COM1 (`0x3F8`, IRQ 4) is currently the only working x86
  log and input path, so it is a must-have row for any PC/server HCL, including
  BMC serial-over-LAN on servers.
- Firmware must allow disabling Secure Boot. Cellos has no signed or measured
  x86 boot path (secure/measured boot is a production-release-gate requirement;
  code-signing/secure-boot belongs to the Security track), so a board whose
  firmware locks Secure Boot on is not compatible and cannot be worked around.

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
