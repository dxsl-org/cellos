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
#[path = "x86-irq-dispatch.rs"]
mod x86_irq_dispatch;
#[path = "x86-mmio-dispatch.rs"]
mod x86_mmio_dispatch;
#[path = "x86-port-dispatch.rs"]
mod x86_port_dispatch;

use crate::{
    cmos_rtc::CmosRtc, pic_8259::Pic8259, pit_8253::Pit8253, uart_16550::Uart16550,
    virtio_blk::BlkDisk, virtio_mmio::VirtioMmio, virtio_net::NetDev, vmm,
};
use api::hypervisor::ViVmExit;
use ostd::io::println;

pub enum RunOutcome {
    Shutdown,
}

/// Run the guest until it powers off or an unrecoverable exit.
pub fn run(
    vm_id: usize,
    vcpu_id: usize,
    disk_file: Option<(usize, api::vfs_file_handles::ViVfsFileHandle, u64)>,
) -> RunOutcome {
    let net_tid = ostd::syscall::sys_lookup_service(api::syscall::service::NET).unwrap_or(0);
    let mut blk = BlkDisk::new(disk_file, None);
    let mut blk_vmio = VirtioMmio::default();
    let mut net = NetDev::new(net_tid, None);
    let mut net_vmio = VirtioMmio::default();
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
                if !x86_port_dispatch::write(port, val, &mut uart, &mut pic, &mut pit, &mut rtc) {
                    return RunOutcome::Shutdown;
                }
            }
            // ── Port IN — device read; return value in guest (E)AX ────────────
            ViVmExit::PortIn { port, size, .. } => {
                let value = x86_port_dispatch::read(port, &mut uart, &pic, &mut pit, &mut rtc);
                write_rax(vm_id, vcpu_id, value, size);
            }
            // Guest idle: retry one pending virtual interrupt.
            ViVmExit::Hlt => x86_irq_dispatch::service_idle(
                vm_id,
                vcpu_id,
                &mut uart,
                &pit,
                &pic,
                &mut net,
                &blk_vmio,
                &mut net_vmio,
            ),

            // ── Host-interrupt preemption — the guest was mid-execution (maybe
            //    a pause-less spin loop): deliver the PIT tick here too, so
            //    jiffies advance at host-tick pace even when the guest never
            //    executes HLT or PAUSE. ──────────────────────────────────────
            ViVmExit::Preempted => x86_irq_dispatch::service_idle(
                vm_id,
                vcpu_id,
                &mut uart,
                &pit,
                &pic,
                &mut net,
                &blk_vmio,
                &mut net_vmio,
            ),

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

            ViVmExit::MmioWrite { ipa, size, val } => {
                if !x86_mmio_dispatch::write(
                    ipa,
                    size,
                    val as u32,
                    vm_id,
                    vcpu_id,
                    &mut blk,
                    &mut blk_vmio,
                    &mut net,
                    &mut net_vmio,
                ) {
                    return RunOutcome::Shutdown;
                }
            }

            ViVmExit::MmioRead { ipa, size, reg } => {
                let Some(value) =
                    x86_mmio_dispatch::read(ipa, size, &blk, &blk_vmio, &net, &net_vmio)
                else {
                    return RunOutcome::Shutdown;
                };
                write_gpr(vm_id, vcpu_id, reg, value, size);
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

fn write_gpr(vm_id: usize, vcpu_id: usize, reg: u8, val: u32, size: u8) {
    if reg >= 16 {
        return;
    }
    let mut rb = [0u64; 32];
    vmm::vcpu_regs(vm_id, vcpu_id, &mut rb, false);
    let old = rb[reg as usize];
    rb[reg as usize] = match size {
        1 => (old & !0xff) | (val as u64 & 0xff),
        2 => (old & !0xffff) | (val as u64 & 0xffff),
        _ => val as u64,
    };
    vmm::vcpu_regs(vm_id, vcpu_id, &mut rb, true);
}
