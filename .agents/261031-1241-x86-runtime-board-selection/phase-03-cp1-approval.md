# Phase 03 CP1 + lock handoff — approval record

2026-11-02: the user replied "duyệt mặc định" to the decision list in [phase-03-cp1.md](phase-03-cp1.md) §10.

Approved as recommended:
- D1: Option A, `QueryMachineFacts` (syscall 433) and `ViMachineFactsV1`.
- D2: the Law-1 delta (syscall 433, `ViMachineFactsV1`, service id 20 `MACHINE_PIN`, the cellbox↔init frame, the ostd wrapper, docs/abi rows).
- Q3: record path `/srv/.cellos-machine/pin`.
- Q4: an unknown-version or corrupt record enters safe mode.
- Q5: no pin store means safe mode under the PC profile, and a warning then continue under q35.
- Q6: safe mode runs `input` and `cellbox` only (no serial, no xhci).
- Q7: confirm is accepted only in safe mode, only from init's shell, and only for the digest shown; then reboot.
- Q8: inputs as recommended (profile source, hypervisor bit, PCI count multiset); no SMBIOS BIOS version.
- Q9: `MACHINE_PIN` registration is accepted only from trusted init.
- Q10: the QEMU test goes into `ci.yml`.
- Q11: the HCL gets a profile/pin-digest column and a note under R3.

The lock handoff in [phase-03-lock-handoff-draft.md](phase-03-lock-handoff-draft.md) is effective with these answers.
CP2 (the evidence) still needs approval before commit.
