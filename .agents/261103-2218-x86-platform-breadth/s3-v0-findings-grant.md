# S3 — file-set extension for the V0 VMware findings

Recorded 2026-11-06. The owner (the user) chose **"Cả bốn trong S3"**: all four defects found by the V0 VMware tooling smoke ([evidence](reports/s3-v0-vmware-tooling-evidence.md)) are fixed inside S3, before the phase-09 VMware matrix.

| ID | Defect | Added scope |
|---|---|---|
| S1 | VMware places e1000e/vmxnet3 behind PCIe root ports (bus ≥ 1); the kernel scans bus 0 only and the platform Cell skips every Ethernet function except `8086:100E` (`cells/services/platform/src/scan.rs:222-229`) | `cells/services/platform/src/scan.rs` (and its tests): register Ethernet functions by the same exact-ID table the kernel gate admits, print the ID on refusal |
| S2 | With VMware's legacy PCI bridge (`pciBridge0`) AHCI lands on bus 2 and reports `no usable MMIO BAR (base=0x0 len=0x2000)` | the PCIe discovery / BAR path that produces the zero BAR (kernel `pcie*` or platform scan) and the AHCI cell, located by symbol after root cause |
| S3 | EFI with ≥ 1 GiB RAM panics at `kernel/src/main.rs:1219` ("frame allocator bitmap and early paging registration must succeed") | the boot memory registration path that panics (`kernel/src/main.rs` boot block and the frame allocator / early paging code it calls), located by symbol after root cause |
| S4 | The static `\_S5_` scan stops at `ExtOpPrefix` (`0x5b`) in VMware's EFI DSDT, so poweroff is refused | `kernel/src/acpi/power.rs` scanner and its fixtures/tests (`tests/fixtures/acpi/`) |

Rules: root cause first, then the smallest fix; no Law-1 delta (stop rule applies); `SystemPower = 436` and its envelope stay frozen; each fix gets a host test where feasible and a QEMU regression plus the VMware cell that failed. Recorded as S3 deviations; the phase-09 independent review also covers S2/S3 changes. All 38 lock relations keep their whole-plan scope.
