# Generic x86_64 virtual machine (`x86_64-vm`)

Profile for running Cellos as a guest of KVM, VMware or another hypervisor
that emulates a PC (ADR-0031, owner decision D-VM, 2026-11-03).

## What selecting it means (D-VM)

**DMA isolation is optional under this profile.** A default guest has no
vIOMMU, so DMA-capable driver cells run with identity (untranslated) DMA. The
hypervisor still confines guest DMA to guest memory, but *inside* the guest a
compromised driver cell can reach any guest page through its device. This is
weaker than `x86_64-pc` (`Required`), where a DMA-capable driver runs only
behind a discovered VT-d remapper.

When the machine does have a vIOMMU (QEMU `-device intel-iommu`), the kernel
still activates it, `QueryPcieFunction` sets `FLAG_DMA_REMAPPED`, and the virtio
cells refuse any device that does not negotiate `VIRTIO_F_ACCESS_PLATFORM`
(otherwise its DMA would bypass the translation).

The kernel binds this profile only when **both** hold:

1. the Limine command line names it: `cellos.profile=x86_64-vm`;
2. CPUID reports a hypervisor.

It is never chosen from detected facts. Without the hypervisor bit, boot
refuses the profile. The boot log then names the profile and
`dma_isolation=Optional`, followed by
`[x86-gate] dma isolation optional (operator-selected VM profile)`.

## Drivers

The `x86_64-pc` set plus virtio over PCI (`/bin/virtio-net`, `/bin/block`
(virtio-blk), `/bin/virtio-scsi`). The virtio cells read their device
capabilities through syscall 434 and poll for completions. Interrupt delivery
(phase 04b) is deferred.

## Use

```bash
qemu-system-x86_64 -accel kvm -cpu host -machine q35,hpet=off -smp 2 -m 1G \
  -cdrom vicell-x86-vm.iso -device virtio-net-pci,netdev=n0 -netdev user,id=n0 \
  -drive file=disk.img,if=none,id=d0,format=raw -device virtio-blk-pci,drive=d0
```

The ISO's `limine.conf` must carry `cellos.profile=x86_64-vm`.

No hypervisor product is qualified by this file. Evidence for each lane is in
`.agents/261103-2218-x86-platform-breadth/reports/`.
