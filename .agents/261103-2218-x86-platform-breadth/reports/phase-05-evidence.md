# Phase 05 evidence: virtio-pci cells and the x86_64-vm profile (2026-11-04)

Base HEAD `fb79d0fae`; the working tree is uncommitted. Evidence ceiling: host tests, plus QEMU 8.2.2 under TCG and under KVM (local). No VMware host and no physical machine was used. Lane excerpts: [tcg](phase-05-virtio-tcg.txt), [kvm](phase-05-virtio-kvm.txt).

## `virtio-x86` lanes, 4/4 under TCG and 4/4 under KVM

| Lane | Image | Result |
|---|---|---|
| `kvm_default_vm_acceptance_x86`: `-accel kvm -cpu host -machine q35,hpet=off -smp 2 -m 1G`, virtio-net/blk/scsi only, no vIOMMU | `limine-vm.conf` (`cellos.profile=x86_64-vm`) | Passed. See the first list below. |
| `virtio_pci_cells_bind_dhcp_and_persist_x86`: default q35 lane | `limine.conf` (`qemu-q35-x86_64`) | All three cells ready, DHCP, two-boot persistence on `/mnt/sd` (virtio-blk) and `/mnt/disk/pci-00-04-0` (virtio-scsi) |
| `virtio_pci_binds_with_access_platform_under_vtd_x86`: `intel-iommu` and `iommu_platform=on,disable-legacy=on` | `limine-pc.conf` (`x86_64-pc`, Required) | Passed. See the second list below. |
| `virtio_pci_refuses_devices_without_access_platform_under_vtd_x86`: `intel-iommu`, no `iommu_platform` | `x86_64-pc` | Passed. See the third list below. |

**`kvm_default_vm_acceptance_x86`**, from the boot log:
- `board=x86_64-vm … dma_isolation=Optional`;
- `dma isolation optional (operator-selected VM profile)`;
- `timer: clock=kvmclock clockevent=TscDeadline … apic=x2apic`;
- all three cells `ready`: transitional IDs `1af4:1000`, `1001` and `1004`;
- `[net] DHCP acquired`;
- boot 1 `machine pin: first boot`, boot 2 `match`;
- a file written on each disk in boot 1 reads back in boot 2.

**`virtio_pci_binds_with_access_platform_under_vtd_x86`**:
- modern IDs `1041`, `1042` and `1048`;
- VT-d active;
- DHCP;
- pin first boot, then match;
- persistence on both disks.

**`virtio_pci_refuses_devices_without_access_platform_under_vtd_x86`**:
- the kernel logs `[pcie] refusing 00:03.0 1af4:1001: virtio without VIRTIO_F_ACCESS_PLATFORM would bypass VT-d`, and the same for `00:04.0`, once each;
- no cell reaches `ready`;
- with both disks refused there is no pin store, so init enters `SAFE MODE` (virtio-net is withheld);
- the shell `Cellos >` still comes up.

Success criterion 1 asks for `irq=msix`. That does not apply here: 04b is deferred, and the cells log `irq=polled reason=04b-deferred`.

## Regression (clean image with every phase 05 change)

| Suite | tcg | kvm |
|---|---|---|
| serial-x86 | 1/1 | 1/1 |
| nvme-x86 | 11/11 | 11/11 |
| ahci-x86 | 15/15 | 15/15 |
| nic-x86 | 2/2 | 2/2 |
| igb-x86 | 2/2 | 2/2 |
| xhci-x86 | 4/4 | 4/4 |
| iommu-dmar-x86 | 3/3 | 3/3 |
| pcie-multibus-x86 | 2/2 | 2/2 |
| machine-pin-x86 | 5/5 | 5/5 |
| local-service-lifecycle-x86 | 3/3 | 3/3 |
| x86_64-boot | 7/9 (pre-existing prod-refusal witness) | 7/9 |

serial, xhci, machine-pin and lifecycle are from the run before the review fixes. The other suites were re-run after the fixes.

**RISC-V virtio-MMIO** (QEMU riscv64 `boot` suite, rebuilt cells):
- `fat_filesystem_mounts` and `network_dhcp_acquires_ip` pass;
- `boots_to_shell_prompt` and `vfs_fat16_reboot_persistence` fail with `Init: shell spawn failed`. That message is not in the current init source; see the phase 05 deviation log.

## Host and lint

- **Kernel:** 421 host tests pass. New tests: the x86_64-vm binding rule and the virtio-net Ethernet gate.
- **Libraries and crates:** api 190, hal-x86 25, hal-soc-x86 7, cellos-boards 15, app-init 41, virtio-pci-transport 49, scsi-block 49, driver-virtio-scsi 14.
- **Clippy (`-D warnings`):** clean for:
  - the kernel on x86_64-unknown-none and riscv64;
  - app-init and the three virtio cells on x86_64-unknown-none.
- **Formatting:** `cargo fmt --check` is clean for every touched crate.
- **Python:** `scripts/test_assemble_x86_image.py` 40/40 and `scripts/test_cellos_sign.py` 64/64.

## Review

An independent `haily-reviewer` pass found 1 HIGH, 3 MED and 6 LOW issues. All are fixed except the documented flush residual. The phase 05 deviation log lists each one. The HIGH fix is a kernel refusal of virtio functions that would bypass VT-d. The fail-closed lane above exercises it.
