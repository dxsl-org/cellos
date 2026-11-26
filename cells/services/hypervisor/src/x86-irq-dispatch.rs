//! Raising the x86 guest's interrupts.
//!
//! The kernel keeps a raised vector until the guest can take it (`InjectIrq`),
//! so every source is raised as soon as it has something: the PIT when a tick
//! is due, a virtio device on the rising edge of its interrupt line. Only the
//! UART is level-polled, at the guest's idle and preemption boundaries.

use crate::{
    net_backend, pic_8259::Pic8259, pit_8253::Pit8253, uart_16550::Uart16550,
    virtio_mmio::VirtioMmio, virtio_net::NetDev, vmm,
};

/// ISA line shared by virtio-gpu, virtio-input, the clipboard virtio-console,
/// the Downloads virtio-9p and virtio-snd (Linux's virtio-mmio IRQs are
/// `IRQF_SHARED`).
/// The emulated PIC has no slave, so the free master lines are 3 and 7, and 7
/// is the 8259 spurious line.
pub const DISPLAY_IRQ: u8 = 3;
const BLOCK_IRQ: u8 = 5;
const NET_IRQ: u8 = 6;

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

/// Raise `line`'s vector on the rising edge of any of `devices`. A line the
/// guest has masked keeps its edge for when it is unmasked.
fn raise_virtio(
    vm_id: usize,
    vcpu_id: usize,
    pic: &Pic8259,
    line: u8,
    devices: &mut [&mut VirtioMmio],
) {
    let Some(vector) = pic.irq(line) else {
        return;
    };
    let mut edge = false;
    for device in devices.iter_mut() {
        edge |= device.take_interrupt_edge();
    }
    if edge {
        vmm::inject_irq(vm_id, vcpu_id, u32::from(vector));
    }
}

/// Raise what the timer and the virtio devices have for the guest. Run before
/// every entry: a tick or a completion then reaches the guest at once instead
/// of at its next idle boundary. `display_mmio` are the devices on
/// [`DISPLAY_IRQ`] the caller holds (the GPU stays with the first vCPU's
/// thread).
pub fn raise_due(
    vm_id: usize,
    vcpu_id: usize,
    pit: &mut Pit8253,
    pic: &Pic8259,
    block_mmio: &mut VirtioMmio,
    net_mmio: &mut VirtioMmio,
    display_mmio: &mut [&mut VirtioMmio],
) {
    if let Some(vector) = pic.irq0() {
        if pit.take_irq0_due() {
            vmm::inject_irq(vm_id, vcpu_id, u32::from(vector));
        }
    }
    raise_virtio(vm_id, vcpu_id, pic, BLOCK_IRQ, &mut [block_mmio]);
    raise_virtio(vm_id, vcpu_id, pic, NET_IRQ, &mut [net_mmio]);
    raise_virtio(vm_id, vcpu_id, pic, DISPLAY_IRQ, display_mmio);
}

/// What only an idle or preempted guest is polled for: the host console, the
/// UART's interrupt level and one inbound network frame. Returns whether a
/// frame arrived.
pub fn service_idle(
    vm_id: usize,
    vcpu_id: usize,
    uart: &mut Uart16550,
    pic: &Pic8259,
    net_device: &mut NetDev,
    net_mmio: &mut VirtioMmio,
    host_console: bool,
) -> bool {
    // The host serial input is one system-wide queue. Only the console guest
    // (`hv`) is the operator's terminal; a launcher's guest (Chrome) has no
    // interactive console and must not receive what is typed at CellOS.
    if host_console {
        drain_host_input(uart);
    }
    if uart.irq_pending() {
        if let Some(vector) = pic.irq(4) {
            vmm::inject_irq(vm_id, vcpu_id, u32::from(vector));
        }
    }
    let Some(frame) = net_backend::try_receive(&mut net_device.backend) else {
        return false;
    };
    if net_device.push_rx_frame(&frame, vm_id, vcpu_id, net_mmio) {
        net_mmio.signal_used();
    }
    true
}
