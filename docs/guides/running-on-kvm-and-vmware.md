# Running Cellos on KVM and VMware

This guide records the exact hypervisor configurations, parameters, and evidence ceilings for running Cellos on **Linux KVM** (QEMU/libvirt) and **VMware Workstation**.

## Evidence Ceilings

| Platform | Profile | Tested Devices | Evidence Ceiling |
|---|---|---|---|
| QEMU (TCG) | `x86_64-vm`, `x86_64-pc` | VirtIO (net/blk/scsi), e1000, e1000e, igb, vmxnet3, pvscsi, AHCI, NVMe | Host unit & integration test gate in CI |
| Linux KVM | `x86_64-vm`, `x86_64-pc` | VirtIO modern, e1000e, vmxnet3, pvscsi, AHCI, NVMe, VT-d vIOMMU | Local Linux KVM test matrix |
| VMware Workstation | `x86_64-vm` | vmxnet3, e1000, e1000e, PVSCSI, AHCI (SATA), NVMe | Local VMware Workstation matrix (39/39 cells PASS) |

---

## 1. Running on Linux KVM (QEMU Direct CLI)

### 1.1 Standard VM Profile (`x86_64-vm`, Optional DMA Isolation)

For standard development and CI without vIOMMU:

```bash
qemu-system-x86_64 \
    -enable-kvm -cpu host -m 1G -smp 2 \
    -machine q35 \
    -cdrom build/cellos.iso \
    -boot order=d \
    -device virtio-net-pci,netdev=net0,disable-legacy=on \
    -netdev user,id=net0 \
    -drive file=disk.img,format=raw,if=none,id=hd0 \
    -device virtio-blk-pci,drive=hd0,disable-legacy=on \
    -serial stdio -display none
```

### 1.2 Strict PC Profile with Intel vIOMMU (`x86_64-pc`, Required DMA Isolation)

To exercise the VT-d driver, interrupt remapping, and DMA isolation in Cellos:

```bash
qemu-system-x86_64 \
    -enable-kvm -cpu host -m 1G -smp 4 \
    -machine q35,accel=kvm,kernel-irqchip=split \
    -device intel-iommu,intremap=on,caching-mode=on,eim=on \
    -cdrom build/cellos.iso \
    -boot order=d \
    -device e1000e,netdev=net0 \
    -netdev user,id=net0 \
    -drive file=disk.img,format=raw,if=none,id=nvme0 \
    -device nvme,drive=nvme0,serial=deadbeef01 \
    -serial stdio -display none
```

Key flags:
- `kernel-irqchip=split`: required for KVM interrupt remapping support.
- `intel-iommu,intremap=on,caching-mode=on,eim=on`: enables VT-d DMAR unit with Interrupt Remapping and Extended Interrupt Mode (EIM).

---

## 2. Libvirt Domain XML Configuration

When configuring a virtual machine in `virt-manager` or `virsh`:

```xml
<domain type='kvm'>
  <name>cellos-guest</name>
  <memory unit='GiB'>1</memory>
  <vcpu placement='static'>2</vcpu>
  <os>
    <type arch='x86_64' machine='q35'>hvm</type>
    <boot dev='cdrom'/>
  </os>
  <features>
    <acpi/>
    <apic/>
    <ioapic driver='qemu'/>
  </features>
  <cpu mode='host-passthrough' check='none'/>
  <devices>
    <!-- Intel vIOMMU with Interrupt Remapping -->
    <iommu model='intel'>
      <driver intremap='on' caching_mode='on' eim='on'/>
    </iommu>

    <!-- Paravirtual storage (VirtIO or PVSCSI) -->
    <disk type='file' device='disk'>
      <driver name='qemu' type='raw' iommu='on'/>
      <source file='/var/lib/libvirt/images/cellos-disk.img'/>
      <target dev='vda' bus='virtio'/>
    </disk>

    <!-- Paravirtual Network (VirtIO or vmxnet3) -->
    <interface type='user'>
      <mac address='52:54:00:12:34:56'/>
      <model type='virtio'/>
      <driver iommu='on'/>
    </interface>

    <serial type='pty'>
      <target type='isa-serial' port='0'>
        <model name='isa-serial'/>
      </target>
    </serial>
    <console type='pty'>
      <target type='serial' port='0'/>
    </console>
  </devices>
</domain>
```

---

## 3. Running on VMware Workstation / ESXi

The VMware matrix is verified on **VMware Workstation Pro 25** (`virtualHW.version = "22"`).

### 3.1 VMX Configuration File

A verified `.vmx` definition for booting Cellos under BIOS or UEFI:

```ini
.encoding = "UTF-8"
config.version = "8"
virtualHW.version = "22"
guestOS = "other-64"

# Compute resources
numvcpus = "2"
memsize = "1024"

# Firmware: "bios" or "efi"
firmware = "efi"

# Storage: PVSCSI controller and primary disk
scsi0.present = "TRUE"
scsi0.virtualDev = "pvscsi"
scsi0:0.present = "TRUE"
scsi0:0.fileName = "cellos-disk.vmdk"
scsi0:0.deviceType = "disk"

# CD-ROM ISO image
sata0.present = "TRUE"
sata0:0.present = "TRUE"
sata0:0.fileName = "cellos.iso"
sata0:0.deviceType = "cdrom-image"

# Networking: vmxnet3 adapter
ethernet0.present = "TRUE"
ethernet0.virtualDev = "vmxnet3"
ethernet0.connectionType = "nat"
ethernet0.addressType = "generated"

# Serial console over named pipe (Windows host / WSL relay)
serial0.present = "TRUE"
serial0.fileType = "pipe"
serial0.fileName = "\\.\pipe\cellos-serial"
serial0.tryNoRxLoss = "TRUE"
serial0.yieldOnMsrRead = "TRUE"

# Optional: Virtualize IOMMU (vIOMMU) on supported hosts
vtd.enable = "TRUE"
```

### 3.2 VMware Driver Support Notes
- **Network**: `vmxnet3` (`15AD:07B0`), `e1000` (`8086:100F`), and `e1000e` (`8086:10D3`) are fully supported.
- **Storage**: `pvscsi` (`15AD:07C0`), `ahci` (SATA), and `nvme` are verified for two-boot persistence across both BIOS and EFI.
- **Power Management**: Soft power-off from Cellos operator CLI commands (`poweroff`, `reboot`) issues ACPI S5 transitions recognized by VMware Workstation.
