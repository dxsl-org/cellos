//! Fixed x86 VirtIO-MMIO slot dispatch.

extern crate alloc;

use crate::{
    virtio_blk::BlkDisk,
    virtio_input::InputDev,
    virtio_mmio::{self, VirtioMmio},
    virtio_net::NetDev,
};
use ostd::io::println;

/// Mutable device bundle passed to the write dispatcher.
pub struct MmioDevicesMut<'a> {
    pub block: &'a mut BlkDisk,
    pub block_mmio: &'a mut VirtioMmio,
    pub net: &'a mut NetDev,
    pub net_mmio: &'a mut VirtioMmio,
    pub input: &'a mut InputDev,
    pub input_mmio: &'a mut VirtioMmio,
}

/// Shared device bundle passed to the read dispatcher.
pub struct MmioDevices<'a> {
    pub block: &'a BlkDisk,
    pub block_mmio: &'a VirtioMmio,
    pub net: &'a NetDev,
    pub net_mmio: &'a VirtioMmio,
    pub input: &'a InputDev,
    pub input_mmio: &'a VirtioMmio,
}

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
        4 => dev
            .input_mmio
            .mmio_write(offset, value, dev.input, vm_id, vcpu_id),
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
        4 => {
            if offset == 0 {
                println("[hv-x86] virtio-mmio input probe");
            }
            dev.input_mmio.mmio_read(aligned, dev.input) as u32
        }
        _ => 0,
    };
    Some(if size == 1 {
        (raw >> ((offset - aligned) * 8)) & 0xff
    } else {
        raw
    })
}
