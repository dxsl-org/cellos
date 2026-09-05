# Phase 03 — Host and QEMU Verification

Status: completed

## Change

- Extend the q35 runner with a root port and place the DMA-backed NVMe endpoint on bus 1.
- Add focused cases proving `01:00.0` discovery and NVMe queue/Identify completion from that same requester.
- Exercise both ordinary and Intel VT-d boots; e1000 registration is regression evidence only.
- Correct stale ABI comments and update roadmap/changelog only after evidence exists.

## Acceptance

- Platform registration reports NVMe at a nonzero-bus BDF.
- The bus-1 NVMe Driver Cell registers without VT-d.
- Under VT-d, the same bus-1 NVMe requester completes DMA-backed controller initialization after the isolation marker.
- Strict q35 multibus passes 2/2, `nic-x86` passes 2/2, and `nvme-x86` passes 3/3.
- No claim is made for real NIC Tx/Rx/DHCP.

## Evidence

- Platform and kernel host suites passed 9/9 and 100/100; the `driver-nvme`
  cross-check passed.
- A fresh cells → kernel → ISO build passed.
- Strict q35 multibus passed 2/2, strict `nic-x86` passed 2/2, and strict
  `nvme-x86` passed 3/3.
- Runtime registered bus-1 NVMe and ordered `VT-d ACTIVE` before exact BDF
  `01:00.0` DMA authorization before block-driver registration.
- Review returned SHIP.
- Physical-x86 qualification, production signing, ACPI DMAR discovery, and
  real NIC Tx/Rx/DHCP remain open.
