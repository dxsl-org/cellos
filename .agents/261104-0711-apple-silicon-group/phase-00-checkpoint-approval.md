# Apple Silicon phase 00 — owner checkpoint

Recorded 2026-11-05. The owner (the user) approved the [phase 00](phase-00-governance-and-provenance.md) checkpoint: ADR-0032 §2–§8 lane rules as written, and the phase 02a lock handoff, so that slice A1 runs beside the Intel WIP slice.

**Effect:**
- Slice A1 ([phase 02a](phase-02a-image-entry-and-vhe-host.md)) holds the Apple WIP slot from 2026-11-05.
- File set (exactly phase 00 "Related Code Files"): `hal/arch/arm/src/aarch64/{boot,el2,trap,trap_el2,vcpu,paging}.rs`, `hal/arch/arm/src/aarch64/backend/{mod,qemu_virt}.rs`, `kernel/linker-aarch64.ld`, the aarch64 image build step in `kernel/build.rs` or `scripts/`, `scripts/qemu-aarch64-test*.sh`, and the aarch64 CI job.
- Activation condition holds: the Intel WIP slice (x86 breadth 04b) holds none of these files. If an Intel slice later needs one, A1 yields it (ADR-0032 §3).

**Not changed:** no Mac purchase; A2–A4 each need their own handoff; all 38 lock relations keep their whole-plan scope.
