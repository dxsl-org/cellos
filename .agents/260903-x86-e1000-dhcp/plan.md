# x86 q35 e1000 DHCP Gate

Status: completed

## Goal

Close the q35-only e1000 data-plane software gate by proving that the net service uses the e1000 Driver Cell for DHCP Tx/Rx with and without Intel VT-d.

## Scope

### Deliverables
- Package `/bin/net` in the x86 VIFS1 image using the existing `service-net` crate.
- Extend `nic-x86` with bounded ordinary and VT-d DHCP data-plane assertions.
- Fix only a defect reproduced by the new strict oracle.
- Record QEMU-only evidence in active roadmap/changelog docs.

### Boundaries
- Require e1000 registration, first bridge Tx with `accepted=true`, first bridge Rx, and DHCP acquisition.
- Under VT-d, require `VT-d ACTIVE` before the e1000 bridge/DHCP sequence.
- Preserve existing SLIRP `restrict=on`; no outbound network or credentials.
- Do not claim physical LAN, physical x86, ACPI DMAR discovery, production signing, or DHCP on hardware.
- Do not redesign NIC interrupts, network IPC, or the DHCP state machine unless the strict oracle proves that path broken.

### Blast Radius
- `scripts/build-x86_64-cells.ps1` for fatal fresh service-net build, required
  packaging, and FAT-layout assertion.
- `tests/integration/tests/nic-x86.rs` plus a shared bounded serial-accept helper
  in `tests/integration/src/lib.rs`.
- Proven e1000/net implementation defect only: `cells/services/net/src/interface.rs` or `cells/drivers/e1000/*`.
- Targeted roadmap/changelog evidence after verification.

## Baseline

- Strict `nic-x86` registration suite: 2/2 pass before implementation.
- Current x86 image builder does not build or package `service-net`; runtime therefore skips `/bin/net`, so no DHCP oracle can exist yet.

## Precedent

- `c83adcc6` — x86 Platform/NVMe/e1000 Driver-Cell image packaging.
- `9c8c303d` — existing Driver-Cell DHCP/TCP data-path restoration.
- `93657fa3` — q35 PCIe multi-bus evidence boundary.

## Phases

1. Package the existing net service in the x86 image.
2. Add bounded runner startup plus ordinary and VT-d strict DHCP data-plane oracles.
3. Repair only a reproduced data-plane failure, if any.
4. Run adjacent PCIe/NVMe and full x86 boot regressions, review, document, and prepare for commit.

## Outcome

- No e1000/net source defect reproduced; existing implementation completed the
  new ordinary and VT-d DHCP oracles once `/bin/net` was packaged.
- Fresh build and strict regression evidence: `nic-x86` 2/2,
  `pcie-multibus-x86` 2/2, `nvme-x86` 3/3, and `x86_64-boot` 7/7.
- Focused review approved the stale-artifact invalidation, bounded QEMU startup,
  reachable ASCII success marker, and settled final fault scan.
