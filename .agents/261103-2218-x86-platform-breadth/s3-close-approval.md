# Slice S3 (phases 08, 09) — closing checkpoint

Recorded 2026-11-06. The owner (the user) answered **"ký đóng S3 và merge vào main"** to the S3 closing checkpoint (H-S3-13: the phase-09 checkpoint with an independent DMA/ring review of the e1000e, vmxnet3 and PVSCSI code).

**Delivered at these evidence ceilings: QEMU 8.2.2 TCG, local KVM (nested on WSL2), VMware Workstation 25 local (`x86_64-vm` only).** Not claimed: VMware production guests, physical Intel machines, bare-metal VMX. The VMware vIOMMU never activated on this host (A3 false, `GCAP=0xffffffffffffffff`, H-S3-9), so no VMware production-guest result is claimed.

- **08:** `/bin/e1000` (82545EM) and `/bin/e1000e` (82574L). q35's default NIC binds `/bin/e1000e` in CI ([H-S3-7](s3-handoff-approval.md)).
- **09.1–09.2:** `/bin/vmxnet3` (`15ad:07b0`) and `/bin/pvscsi` (`15ad:07c0`). Claim-time memory decode; bus mastering only at GrantDma ([record of fixes S1–S4](s3-v0-findings-grant.md)).
- **Review fixes:** review 1 (DMA/ring) fixed; review 2 (`S3DeltaReview`) fixed in `d986a555b` (e1000e keeps one TX slot empty) and `8eeb1d4e3` (kernel refuses PVSCSI `15ad:07c0` while VT-d is active, because QEMU's PVSCSI model bypasses the IOMMU; VMware is unaffected).
- **Evidence:**
  - Final checkpoint `eab0755c6`: QEMU TCG and KVM suites 72/72 per accelerator; OVMF 1/2/4 GiB reach prompt; VMware 39/39 (run `20261106T022001Z-7ce6b8`, [JSON](reports/s3-final-vmware-matrix.json)).
  - Post-review-2 `98b670640`: QEMU 22/22 per accelerator; VMware 23/23 (run `20261106T025345Z-235cef`, [JSON](reports/s3-post-review2-vmware-matrix.json)); no `15ad:07c0` refusal on VMware.
  - Kernel host suite 512 passed.
  - Phase evidence: [08](reports/phase-08-evidence.md), [09](reports/phase-09-evidence.md).

**Open items carried out of S3 (not closed by this record):**
1. The net service uses a fixed MAC `52:54:00:12:34:56` and all NICs run promiscuous. This blocks C2C over LAN between two nodes. Recommended as the next slice; owner decision pending.
2. OVMF with ≥2 CPUs panics at `smp.rs:1100` (frame-0 root).
3. The memory BAR gate applies only at claim; GrantDma and MSI paths are unchecked (reviewer MINOR). Bus-0 Platform BAR probe race (reviewer MINOR).
4. Syscall 236 cannot distinguish `Ok(1)` from `FileNotFound` through `libs/ostd` (reviewer MINOR).
5. One-time machine-pin change for bus ≥1 Ethernet functions. Reviewer found it undocumented; documented in the changelog entry.
6. VMware serial occasionally drops log lines (none judged, not a result failure).

**Effect:** S3 is closed. Its file-bounded exception ends with the slice. The Intel WIP lane is released once `feat/x86-s3-integration` is merged into `main`. The merge is done by the integrator; the security session's uncommitted files in `/home/dmin/cellos` were left untouched. All 38 lock relations keep their whole-plan scope.
