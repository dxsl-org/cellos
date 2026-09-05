# PCIe Multi-Bus ECAM Plan

Status: completed

## Goal

Close the software gate for PCIe endpoints above bus 0 on x86 q35 without weakening BDF ownership or VT-d DMA isolation.

## Scope Contract

### Deliverables
- Checked MCFG range admission and ECAM window sizing.
- x86 paging and Platform Cell claims covering the admitted bus range.
- Platform enumeration and canonical requester IDs for every admitted bus.
- Bus-relative ECAM access when MCFG starts above zero.
- Per-bus VT-d context tables, corrected invalidation encodings/order, cleanup-safe ownership, and collision regression coverage.
- q35 NVMe-behind-root-port proof, including DMA completion from the bus-1 requester under VT-d.
- Correct canonical BDF documentation and roadmap evidence.

### Boundaries
- Preserve `bus[15:8] | dev[7:3] | function[2:0]` across syscalls, ownership, and IOMMU code.
- Reject malformed/reversed/overflowing ECAM ranges before mapping or claiming.
- Do not claim NIC Tx/Rx/DHCP completion; this slice proves enumeration, ownership, bus mastering, and existing driver DMA initialization.
- Do not alter production signing or physical-hardware gates.

### Blast Radius
- `kernel/src/acpi.rs`, `kernel/src/main.rs`, `kernel/src/memory/paging.rs`.
- `kernel/src/task/drivers/pcie_ecam.rs`, `kernel/src/task/drivers/iommu_x86.rs`.
- `cells/services/platform/src/main.rs`, `cells/services/platform/src/scan.rs`.
- PCI ABI comments, integration runner/tests, and roadmap/changelog evidence.

## Phases

1. [x] VT-d per-bus isolation and invalidation safety.
2. [x] Range mapping and scanner contracts, landed atomically with phase 1.
3. [x] Host and q35 regression proof using bus-1 NVMe DMA.
4. [x] Review, documentation, and ship preparation.

## Precedent

- `003ee5c0` — MCFG launch argv and Platform Cell x86 discovery.
- `18ea5eeb` — canonical requester-ID decoding.
- `47237477` — host BAR probing harness.

## Completion Evidence

- Platform and kernel host suites passed 9/9 and 100/100; the `driver-nvme`
  cross-check passed.
- A fresh cells → kernel → ISO build passed.
- Strict q35 multibus passed 2/2, strict `nic-x86` passed 2/2, and strict
  `nvme-x86` passed 3/3.
- Runtime registered bus-1 NVMe and ordered `VT-d ACTIVE` before exact BDF
  `01:00.0` DMA authorization before block-driver registration.
- The raw MCFG base remains bus-0-relative and is normalized by `bus_start`;
  the frozen syscall ABI is unchanged.
- Review returned SHIP. Physical-x86 qualification, production signing, ACPI
  DMAR discovery, and real NIC Tx/Rx/DHCP remain open.
