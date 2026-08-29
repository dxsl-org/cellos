//! Fixed x86 VirtIO-MMIO slot dispatch.

extern crate alloc;

use crate::{
    virtio_blk::BlkDisk,
    virtio_mmio::{self, VirtioMmio},
    virtio_net::NetDev,
};
use ostd::io::println;

pub fn write(
    ipa: u64,
    size: u8,
    value: u32,
    vm_id: usize,
    vcpu_id: usize,
    block: &mut BlkDisk,
    block_mmio: &mut VirtioMmio,
    net: &mut NetDev,
    net_mmio: &mut VirtioMmio,
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
            return true; // Current device-specific config fields are read-only.
        }
        println(&alloc::format!(
            "[hv-x86] unsupported MMIO write gpa=0x{:x} size={}",
            ipa,
            size
        ));
        return false;
    }
    match slot {
        0 => block_mmio.mmio_write(offset, value, block, vm_id, vcpu_id),
        1 => net_mmio.mmio_write(offset, value, net, vm_id, vcpu_id),
        _ => {}
    }
    true
}

pub fn read(
    ipa: u64,
    size: u8,
    block: &BlkDisk,
    block_mmio: &VirtioMmio,
    net: &NetDev,
    net_mmio: &VirtioMmio,
) -> Option<u32> {
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
            block_mmio.mmio_read(aligned, block) as u32
        }
        1 => {
            if offset == 0 {
                println("[hv-x86] virtio-mmio net probe");
            }
            net_mmio.mmio_read(aligned, net) as u32
        }
        _ => 0,
    };
    Some(if size == 1 {
        (raw >> ((offset - aligned) * 8)) & 0xff
    } else {
        raw
    })
}
