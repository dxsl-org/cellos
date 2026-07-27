//! x86 (SVM/VT-x) VmExit dispatch loop for the hypervisor cell.
//!
//! The x86 twin of the aarch64 [`crate::run_loop`]. It services the port-I/O
//! device models (16550 UART, 8259 PIC, 8253 PIT, CMOS RTC), injects the PIT
//! timer IRQ on guest idle so jiffies advance, forwards host keystrokes
//! (kernel UART ring, `sys_read(0)`) into the 16550 RX FIFO as guest IRQ4,
//! and surfaces guest shutdown. One external interrupt is injected per entry
//! (single EVENTINJ slot): UART RX wins over the PIT tick because RX pending
//! clears within one guest IRQ handler pass while ticks recur forever.
//! RIP advancing is done kernel-side (IOIO via EXITINFO2; HLT/MSR/PAUSE via
//! instruction-length), so the cell never touches guest PC here. The guest
//! LAPIC is the kernel's RAM-backed xAPIC window — its timer is injected
//! kernel-side; only the legacy PIT tick is delivered from here.

extern crate alloc;

use crate::{cmos_rtc::CmosRtc, pic_8259::Pic8259, pit_8253::Pit8253, uart_16550::Uart16550, vmm};
use api::hypervisor::ViVmExit;
use ostd::io::println;

pub enum RunOutcome {
    Shutdown,
}

/// Ports a guest write to which means "power off" (QEMU/firmware conventions).
fn is_exit_port(port: u16) -> bool {
    matches!(port, 0x604 | 0x501 | 0xB004)
}

/// Deliver the PIT tick if the guest has armed it and IRQ0 is deliverable.
fn deliver_pit_tick(vm_id: usize, vcpu_id: usize, pit: &Pit8253, pic: &Pic8259) {
    if pit.irq0_armed() {
        if let Some(vector) = pic.irq0() {
            vmm::inject_irq(vm_id, vcpu_id, vector as u32);
        }
    }
}

/// Drain pending host console bytes into the UART RX FIFO. The kernel's UART
/// ring (fd 0) is a single-consumer stream — this image ships no shell cell,
/// so the hypervisor is the sole reader and every keystroke reaches the guest.
fn drain_host_input(uart: &mut Uart16550) {
    let mut buf = [0u8; 32];
    while let Ok(n) = ostd::syscall::sys_read(0, &mut buf) {
        if n == 0 {
            break;
        }
        for &b in &buf[..n] {
            uart.push_rx(b);
        }
        if n < buf.len() {
            break;
        }
    }
}

/// Inject the UART IRQ4 vector if the 16550 asserts an interrupt and the PIC
/// gate is open. Returns true when the single per-entry injection slot was
/// consumed.
fn deliver_uart_irq(vm_id: usize, vcpu_id: usize, uart: &Uart16550, pic: &Pic8259) -> bool {
    if !uart.irq_pending() {
        return false;
    }
    match pic.irq(4) {
        Some(vector) => {
            vmm::inject_irq(vm_id, vcpu_id, vector as u32);
            true
        }
        None => false,
    }
}

/// Run the guest until it powers off or an unrecoverable exit.
pub fn run(vm_id: usize, vcpu_id: usize) -> RunOutcome {
    let mut uart = Uart16550::new();
    let mut pic = Pic8259::new();
    let mut pit = Pit8253::new();
    let mut rtc = CmosRtc::new();
    let mut exit = ViVmExit::Unknown { ec: 0, iss: 0 };

    loop {
        let ret = vmm::run_vcpu(vm_id, vcpu_id, &mut exit);
        if ret == usize::MAX {
            println("[hv-x86] run_vcpu kernel error — aborting");
            return RunOutcome::Shutdown;
        }

        match exit {
            // ── Port OUT — device write; RIP already advanced kernel-side ─────
            ViVmExit::PortOut { port, size: _, val } => {
                if Uart16550::owns(port) {
                    uart.write(port, val);
                } else if Pic8259::owns(port) {
                    pic.write(port, val);
                } else if Pit8253::owns(port) {
                    pit.write(port, val);
                } else if CmosRtc::owns(port) {
                    rtc.write(port, val);
                } else if is_exit_port(port) {
                    println("[hv-x86] guest power-off port write");
                    return RunOutcome::Shutdown;
                } else {
                    println(&alloc::format!(
                        "[hv-x86] unhandled OUT port=0x{:x} val=0x{:x}",
                        port,
                        val
                    ));
                }
            }

            // ── Port IN — device read; return value in guest (E)AX ────────────
            ViVmExit::PortIn { port, size, .. } => {
                let val = if Uart16550::owns(port) {
                    uart.read(port)
                } else if Pic8259::owns(port) {
                    pic.read(port)
                } else if Pit8253::owns(port) {
                    pit.read(port)
                } else if CmosRtc::owns(port) {
                    rtc.read(port)
                } else {
                    // Open bus: all-ones (unclaimed port reads float high).
                    0xFFFF_FFFF
                };
                write_rax(vm_id, vcpu_id, val, size);
            }

            // ── HLT — guest idle (a real HLT, or a paced PAUSE busy-wait the
            //    kernel surfaced as Hlt): deliver the PIT tick so jiffies
            //    advance. An armed LAPIC timer never reaches here — the kernel
            //    injects it from the RAM-backed xAPIC frame before surfacing. ──
            ViVmExit::Hlt => {
                drain_host_input(&mut uart);
                if !deliver_uart_irq(vm_id, vcpu_id, &uart, &pic) {
                    deliver_pit_tick(vm_id, vcpu_id, &pit, &pic);
                }
            }

            // ── Host-interrupt preemption — the guest was mid-execution (maybe
            //    a pause-less spin loop): deliver the PIT tick here too, so
            //    jiffies advance at host-tick pace even when the guest never
            //    executes HLT or PAUSE. ──────────────────────────────────────
            ViVmExit::Preempted => {
                drain_host_input(&mut uart);
                if !deliver_uart_irq(vm_id, vcpu_id, &uart, &pic) {
                    deliver_pit_tick(vm_id, vcpu_id, &pit, &pic);
                }
            }

            // ── MSR — never surfaced anymore (the kernel emulates all MSR exits
            //    internally); fail loud if one arrives so a regression is seen. ─
            ViVmExit::Msr {
                index, is_write, ..
            } => {
                println(&alloc::format!(
                    "[hv-x86] unexpected surfaced MSR idx=0x{:x} write={} — shutting down",
                    index,
                    is_write
                ));
                return RunOutcome::Shutdown;
            }

            // ── Unmodelled MMIO (NPT fault) — log; virtio window arrives P06 ──
            ViVmExit::MmioWrite { ipa, .. } | ViVmExit::MmioRead { ipa, .. } => {
                println(&alloc::format!(
                    "[hv-x86] unhandled guest MMIO gpa=0x{:x}",
                    ipa
                ));
                return RunOutcome::Shutdown;
            }

            ViVmExit::Shutdown => {
                println("[hv-x86] guest shutdown");
                return RunOutcome::Shutdown;
            }

            ViVmExit::Unknown { ec, iss } => {
                println(&alloc::format!(
                    "[hv-x86] unknown vmexit ec=0x{:x} iss=0x{:x}",
                    ec,
                    iss
                ));
                return RunOutcome::Shutdown;
            }

            // ── aarch64-only exits — never emitted on the x86 world-switch ────
            ViVmExit::Hvc { .. } | ViVmExit::Wfi | ViVmExit::SysReg { .. } => {
                println("[hv-x86] unexpected aarch64 vmexit — shutting down VM");
                return RunOutcome::Shutdown;
            }
        }
    }
}

/// Store a port-IN result in the guest's RAX (masked to the access size). RAX is
/// VMCB-managed; the kernel syncs gpr[0]→VMCB before the next entry.
fn write_rax(vm_id: usize, vcpu_id: usize, val: u32, size: u8) {
    let mut rb = [0u64; 32];
    vmm::vcpu_regs(vm_id, vcpu_id, &mut rb, false);
    let mask: u64 = match size {
        1 => 0xFF,
        2 => 0xFFFF,
        _ => 0xFFFF_FFFF,
    };
    rb[0] = val as u64 & mask;
    vmm::vcpu_regs(vm_id, vcpu_id, &mut rb, true);
}
