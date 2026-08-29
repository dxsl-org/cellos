//! Prioritized x86 interrupt delivery at guest idle/preemption boundaries.

use crate::{
    net_backend, pic_8259::Pic8259, pit_8253::Pit8253, uart_16550::Uart16550,
    virtio_mmio::VirtioMmio, virtio_net::NetDev, vmm,
};

fn drain_host_input(uart: &mut Uart16550) {
    let mut buf = [0u8; 32];
    while let Ok(n) = ostd::syscall::sys_read(0, &mut buf) {
        if n == 0 {
            break;
        }
        for &byte in &buf[..n] {
            uart.push_rx(byte);
        }
        if n < buf.len() {
            break;
        }
    }
}

fn deliver_uart(vm_id: usize, vcpu_id: usize, uart: &Uart16550, pic: &Pic8259) -> bool {
    if !uart.irq_pending() {
        return false;
    }
    if let Some(vector) = pic.irq(4) {
        vmm::inject_irq(vm_id, vcpu_id, u32::from(vector));
        true
    } else {
        false
    }
}

fn deliver_virtio(
    vm_id: usize,
    vcpu_id: usize,
    block: &VirtioMmio,
    net: &VirtioMmio,
    pic: &Pic8259,
) -> bool {
    let vector = block
        .interrupt_pending()
        .then(|| pic.irq(5))
        .flatten()
        .or_else(|| net.interrupt_pending().then(|| pic.irq(6)).flatten());
    if let Some(vector) = vector {
        vmm::inject_irq(vm_id, vcpu_id, u32::from(vector));
        true
    } else {
        false
    }
}

fn deliver_pit(vm_id: usize, vcpu_id: usize, pit: &Pit8253, pic: &Pic8259) {
    if pit.irq0_armed() {
        if let Some(vector) = pic.irq0() {
            vmm::inject_irq(vm_id, vcpu_id, u32::from(vector));
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn service_idle(
    vm_id: usize,
    vcpu_id: usize,
    uart: &mut Uart16550,
    pit: &Pit8253,
    pic: &Pic8259,
    net_device: &mut NetDev,
    block_mmio: &VirtioMmio,
    net_mmio: &mut VirtioMmio,
) {
    drain_host_input(uart);
    if let Some(frame) = net_backend::try_receive(&mut net_device.backend) {
        if net_device.push_rx_frame(&frame, vm_id, vcpu_id, net_mmio) {
            net_mmio.signal_used();
        }
    }
    if !deliver_uart(vm_id, vcpu_id, uart, pic)
        && !deliver_virtio(vm_id, vcpu_id, block_mmio, net_mmio, pic)
    {
        deliver_pit(vm_id, vcpu_id, pit, pic);
    }
}
