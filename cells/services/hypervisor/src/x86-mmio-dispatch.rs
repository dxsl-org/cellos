//! Fixed x86 VirtIO-MMIO slot dispatch.

extern crate alloc;

use crate::{
    virtio_9p::P9Dev,
    virtio_blk::BlkDisk,
    virtio_console::Console,
    virtio_gpu::GpuDev,
    virtio_input::InputDev,
    virtio_mmio::{self, VirtioMmio},
    virtio_net::NetDev,
    virtio_snd::SndDev,
};
use ostd::io::println;

/// Mutable device bundle passed to the write dispatcher. `gpu` is `None` on
/// a thread that does not hold the GPU (`x86_machine`): its accesses to the
/// GPU's slot go to the thread that does.
pub struct MmioDevicesMut<'a> {
    pub block: &'a mut BlkDisk,
    pub block_mmio: &'a mut VirtioMmio,
    pub net: &'a mut NetDev,
    pub net_mmio: &'a mut VirtioMmio,
    pub gpu: Option<(&'a mut GpuDev, &'a mut VirtioMmio)>,
    pub input: &'a mut InputDev,
    pub input_mmio: &'a mut VirtioMmio,
    pub console: &'a mut Console,
    pub console_mmio: &'a mut VirtioMmio,
    pub p9: &'a mut P9Dev,
    pub p9_mmio: &'a mut VirtioMmio,
    pub snd: &'a mut SndDev,
    pub snd_mmio: &'a mut VirtioMmio,
}

/// Shared device bundle passed to the read dispatcher; `gpu` as in
/// [`MmioDevicesMut`].
pub struct MmioDevices<'a> {
    pub block: &'a BlkDisk,
    pub block_mmio: &'a VirtioMmio,
    pub net: &'a NetDev,
    pub net_mmio: &'a VirtioMmio,
    pub gpu: Option<(&'a GpuDev, &'a VirtioMmio)>,
    pub input: &'a InputDev,
    pub input_mmio: &'a VirtioMmio,
    pub console: &'a Console,
    pub console_mmio: &'a VirtioMmio,
    pub p9: &'a P9Dev,
    pub p9_mmio: &'a VirtioMmio,
    pub snd: &'a SndDev,
    pub snd_mmio: &'a VirtioMmio,
}

/// The virtio-mmio slot of the GPU.
pub const GPU_SLOT: usize = 3;

pub fn write(
    ipa: u64,
    size: u8,
    value: u32,
    vm_id: usize,
    vcpu_id: usize,
    dev: &mut MmioDevicesMut<'_>,
) -> bool {
    if !virtio_mmio::owns(ipa) {
        println(&alloc::format!(
            "[hv-x86] unhandled guest MMIO write gpa=0x{:x}",
            ipa
        ));
        return false;
    }
    let (slot, offset) = virtio_mmio::slot_and_offset(ipa);
    if size != 4 {
        if size == 1 && offset >= 0x100 {
            if slot == 4 {
                dev.input_mmio
                    .mmio_write(offset, value, dev.input, vm_id, vcpu_id);
            }
            return true;
        }
        println(&alloc::format!(
            "[hv-x86] unsupported MMIO write gpa=0x{:x} size={}",
            ipa,
            size
        ));
        return false;
    }
    match slot {
        0 => dev
            .block_mmio
            .mmio_write(offset, value, dev.block, vm_id, vcpu_id),
        1 => dev
            .net_mmio
            .mmio_write(offset, value, dev.net, vm_id, vcpu_id),
        2 => dev
            .console_mmio
            .mmio_write(offset, value, dev.console, vm_id, vcpu_id),
        3 => {
            if let Some((gpu, gpu_mmio)) = dev.gpu.as_mut() {
                gpu_mmio.mmio_write(offset, value, &mut **gpu, vm_id, vcpu_id);
            }
        }
        4 => dev
            .input_mmio
            .mmio_write(offset, value, dev.input, vm_id, vcpu_id),
        5 => dev
            .p9_mmio
            .mmio_write(offset, value, dev.p9, vm_id, vcpu_id),
        6 => dev
            .snd_mmio
            .mmio_write(offset, value, dev.snd, vm_id, vcpu_id),
        _ => {}
    }
    true
}

pub fn read(ipa: u64, size: u8, dev: &MmioDevices<'_>) -> Option<u32> {
    if !virtio_mmio::owns(ipa) {
        println(&alloc::format!(
            "[hv-x86] unhandled guest MMIO read gpa=0x{:x}",
            ipa
        ));
        return None;
    }
    let (slot, offset) = virtio_mmio::slot_and_offset(ipa);
    let aligned = match size {
        4 => offset,
        1 if offset >= 0x100 => offset & !3,
        // Device config words (`readw`) stay inside one 32-bit config word.
        2 if offset >= 0x100 && offset & 1 == 0 => offset & !3,
        _ => {
            println(&alloc::format!(
                "[hv-x86] unsupported MMIO read gpa=0x{:x} size={}",
                ipa,
                size
            ));
            return None;
        }
    };
    let raw = match slot {
        0 => {
            if offset == 0 {
                println("[hv-x86] virtio-mmio block probe");
            }
            dev.block_mmio.mmio_read(aligned, dev.block) as u32
        }
        1 => {
            if offset == 0 {
                println("[hv-x86] virtio-mmio net probe");
            }
            dev.net_mmio.mmio_read(aligned, dev.net) as u32
        }
        2 => {
            if offset == 0 {
                println("[hv-x86] virtio-mmio console probe");
            }
            dev.console_mmio.mmio_read(aligned, dev.console) as u32
        }
        3 => {
            if offset == 0 {
                println("[hv-x86] virtio-mmio gpu probe");
            }
            match dev.gpu {
                Some((gpu, gpu_mmio)) => gpu_mmio.mmio_read(aligned, gpu) as u32,
                None => 0,
            }
        }
        4 => {
            if offset == 0 {
                println("[hv-x86] virtio-mmio input probe");
            }
            dev.input_mmio.mmio_read(aligned, dev.input) as u32
        }
        5 => {
            if offset == 0 {
                println("[hv-x86] virtio-mmio 9p probe");
            }
            dev.p9_mmio.mmio_read(aligned, dev.p9) as u32
        }
        6 => {
            if offset == 0 {
                println("[hv-x86] virtio-mmio snd probe");
            }
            dev.snd_mmio.mmio_read(aligned, dev.snd) as u32
        }
        _ => 0,
    };
    let shift = (offset - aligned) * 8;
    Some(match size {
        1 => (raw >> shift) & 0xff,
        2 => (raw >> shift) & 0xffff,
        _ => raw,
    })
}
