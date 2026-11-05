# Apple Silicon phase 02a — owner checkpoint

Recorded 2026-11-05. The owner (the user) accepted phase 02a (slice A1) at the QEMU 8.2.2 TCG ceiling ([evidence](reports/phase-02a-evidence.md)), including file-set deviations D1–D7 in the phase file: the VHE variant is selected by `CELLOS_AARCH64_HOST_REGIME` rather than a Cargo feature, and `scripts/qemu-hypervisor-smoke.sh` gained the default-preserving `HV_SMOKE_CPU` knob.

**Effect:** slice A1 is closed; the Apple WIP slot is free. A2 (phases 02b, 02c) needs its own handoff. Not claimed: KVM, hardware, the "CPU forces E2H" refusal path (QEMU 8.2.2 cannot model it), and the new CI steps on GitHub. No Mac purchase is approved.
