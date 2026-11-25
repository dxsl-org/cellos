//! The emulated devices every vCPU's exits reach, and the loop of the vCPUs
//! after the first.
//!
//! Each vCPU has its own thread. The devices sit behind one lock that a
//! thread holds while it serves an exit, never across `RunVcpu`, so the
//! vCPUs run at the same time and only their device accesses take turns. The
//! first vCPU's thread (the BSP's) also serves the host side — the network,
//! the console, the launcher session — and is the only one the 8259's lines
//! interrupt (no IOAPIC: virtual-wire mode).
//!
//! The GPU stays with the BSP's thread: its scanout Grant and its messages to
//! the compositor or launcher belong to the task that made them (the kernel
//! keys a Grant by its registering task). Another vCPU's access to the GPU's
//! slot is handed to the BSP's thread ([`Machine::handed_over`]), which
//! serves it at its next turn; the vCPU waits for the answer.

use core::sync::atomic::{AtomicBool, Ordering};

use super::{x86_irq_dispatch, x86_mmio_dispatch, x86_port_dispatch};
use crate::{
    acpi_pm::AcpiPm, cmos_rtc::CmosRtc, pic_8259::Pic8259, pit_8253::Pit8253,
    uart_16550::Uart16550, virtio_9p::P9Dev, virtio_blk::BlkDisk, virtio_console::Console,
    virtio_gpu::GpuDev, virtio_input::InputDev, virtio_mmio::VirtioMmio, virtio_net::NetDev, vmm,
    yield_lock,
};
use abi::hypervisor::ViVmExit;
use alloc::vec::Vec;
use ostd::io::println;
use ostd::sync::Mutex;
use x86_mmio_dispatch::{MmioDevices, MmioDevicesMut, GPU_SLOT};

/// Set when the guest stops (power-off, a fatal exit, the launcher's end):
/// every vCPU thread leaves its loop at its next return.
pub(super) static STOPPED: AtomicBool = AtomicBool::new(false);

/// The GPU and its virtio-mmio state, held by the BSP's thread only.
pub(super) type Gpu<'a> = (&'a mut GpuDev, &'a mut VirtioMmio);

pub(super) struct Machine {
    pub blk: BlkDisk,
    pub blk_vmio: VirtioMmio,
    pub net: NetDev,
    pub net_vmio: VirtioMmio,
    pub input: InputDev,
    pub input_vmio: VirtioMmio,
    /// The clipboard console (slot 2); its text reaches the launcher from
    /// the BSP's thread (`run_loop_x86::serve_host`).
    pub console: Console,
    pub console_vmio: VirtioMmio,
    /// The Downloads share (slot 5).
    pub p9: P9Dev,
    pub p9_vmio: VirtioMmio,
    pub uart: Uart16550,
    pub pic: Pic8259,
    pub pit: Pit8253,
    pub rtc: CmosRtc,
    pub pm: AcpiPm,
    /// Polls left at the short interval (`run_budget_ns`).
    pub eager_polls: u32,
    /// GPU accesses of other vCPUs, `(vcpu_id, exit)`, for the BSP's thread.
    pub handed_over: Vec<(usize, ViVmExit)>,
    /// Their outcomes, `(vcpu_id, guest still running)`.
    pub answered: Vec<(usize, bool)>,
    /// GPU accesses handed over so far (reported when the guest ends).
    pub handed_over_count: u64,
}

/// Polls kept at the short interval after the outside world last talked to
/// the guest: a frame from the network, input from the host, or the guest's
/// own traffic on its network device.
pub(super) const EAGER_POLLS: u32 = 64;

impl Machine {
    /// Serve a port or MMIO exit of vCPU `vcpu_id`, with the GPU when the
    /// caller holds it. `None` for any other exit; `Some(false)` when the
    /// guest must stop.
    pub fn serve_device(
        &mut self,
        vm_id: usize,
        vcpu_id: usize,
        exit: &ViVmExit,
        gpu: Option<Gpu>,
    ) -> Option<bool> {
        let running = match *exit {
            // RIP is already advanced kernel-side.
            ViVmExit::PortOut { port, size: _, val } => x86_port_dispatch::write(
                port,
                val,
                &mut self.uart,
                &mut self.pic,
                &mut self.pit,
                &mut self.rtc,
                &mut self.pm,
            ),
            ViVmExit::PortIn { port, size, .. } => {
                let value = x86_port_dispatch::read(
                    port,
                    &mut self.uart,
                    &self.pic,
                    &mut self.pit,
                    &mut self.rtc,
                    &self.pm,
                );
                write_rax(vm_id, vcpu_id, value, size);
                true
            }
            ViVmExit::MmioWrite { ipa, size, val } => {
                // The guest is sending or refilling its receive queue: an
                // answer from the network is likely to follow.
                if crate::virtio_mmio::owns(ipa) && crate::virtio_mmio::slot_and_offset(ipa).0 == 1
                {
                    self.eager_polls = EAGER_POLLS;
                }
                x86_mmio_dispatch::write(
                    ipa,
                    size,
                    val as u32,
                    vm_id,
                    vcpu_id,
                    &mut MmioDevicesMut {
                        block: &mut self.blk,
                        block_mmio: &mut self.blk_vmio,
                        net: &mut self.net,
                        net_mmio: &mut self.net_vmio,
                        gpu,
                        input: &mut self.input,
                        input_mmio: &mut self.input_vmio,
                        console: &mut self.console,
                        console_mmio: &mut self.console_vmio,
                        p9: &mut self.p9,
                        p9_mmio: &mut self.p9_vmio,
                    },
                )
            }
            ViVmExit::MmioRead { ipa, size, reg } => {
                let value = x86_mmio_dispatch::read(
                    ipa,
                    size,
                    &MmioDevices {
                        block: &self.blk,
                        block_mmio: &self.blk_vmio,
                        net: &self.net,
                        net_mmio: &self.net_vmio,
                        gpu: gpu.map(|(gpu, gpu_mmio)| (&*gpu, &*gpu_mmio)),
                        input: &self.input,
                        input_mmio: &self.input_vmio,
                        console: &self.console,
                        console_mmio: &self.console_vmio,
                        p9: &self.p9,
                        p9_mmio: &self.p9_vmio,
                    },
                );
                if let Some(value) = value {
                    write_gpr(vm_id, vcpu_id, reg, value, size);
                }
                value.is_some()
            }
            _ => return None,
        };
        Some(running)
    }

    /// Serve the GPU accesses other vCPUs handed over (BSP's thread).
    pub fn serve_handed_over(&mut self, vm_id: usize, gpu: &mut GpuDev, gpu_vmio: &mut VirtioMmio) {
        for (vcpu_id, exit) in core::mem::take(&mut self.handed_over) {
            let running = self
                .serve_device(vm_id, vcpu_id, &exit, Some((&mut *gpu, &mut *gpu_vmio)))
                .unwrap_or(false);
            self.answered.push((vcpu_id, running));
        }
    }

    /// Raise on vCPU `bsp` what the timer and the virtio devices have; the
    /// GPU's interrupt only where the caller holds it.
    pub fn raise_due(&mut self, vm_id: usize, bsp: usize, gpu_vmio: Option<&mut VirtioMmio>) {
        let (input, console, p9) = (
            &mut self.input_vmio,
            &mut self.console_vmio,
            &mut self.p9_vmio,
        );
        let mut all;
        let mut held;
        let display: &mut [&mut VirtioMmio] = match gpu_vmio {
            Some(gpu) => {
                all = [gpu, input, console, p9];
                &mut all
            }
            None => {
                held = [input, console, p9];
                &mut held
            }
        };
        x86_irq_dispatch::raise_due(
            vm_id,
            bsp,
            &mut self.pit,
            &self.pic,
            &mut self.blk_vmio,
            &mut self.net_vmio,
            display,
        );
    }
}

/// Whether `exit` is an access to the GPU's virtio-mmio slot.
fn reaches_gpu(exit: &ViVmExit) -> bool {
    let (ViVmExit::MmioWrite { ipa, .. } | ViVmExit::MmioRead { ipa, .. }) = *exit else {
        return false;
    };
    crate::virtio_mmio::owns(ipa) && crate::virtio_mmio::slot_and_offset(ipa).0 == GPU_SLOT
}

/// Hand a GPU access of vCPU `vcpu_id` to the BSP's thread and wait for its
/// outcome (whether the guest still runs; `false` if the guest stopped
/// meanwhile). The BSP's runs are short while accesses are handed over
/// (`eager_polls`), so the wait is about one of them.
fn hand_over(vcpu_id: usize, exit: ViVmExit, machine: &Mutex<Machine>) -> bool {
    {
        let mut machine = yield_lock::lock(machine);
        machine.handed_over.push((vcpu_id, exit));
        machine.handed_over_count += 1;
        machine.eager_polls = EAGER_POLLS;
    }
    loop {
        ostd::task::yield_now();
        if STOPPED.load(Ordering::Acquire) {
            return false;
        }
        let mut machine = yield_lock::lock(machine);
        if let Some(at) = machine.answered.iter().position(|(id, _)| *id == vcpu_id) {
            return machine.answered.swap_remove(at).1;
        }
    }
}

/// How long one run of a vCPU other than the first may hold its host CPU. The
/// kernel keeps an idle vCPU halted inside the guest for the whole run, IRQs
/// off, where no other task can have the CPU; with a scheduler slice (10 ms)
/// four idle vCPUs held four CPUs of five and the first vCPU, which serves
/// every device, waited 120 s of a 185 s lane run for a CPU.
const AP_RUN_NS: u64 = 1_000_000;

/// Run vCPU `vcpu_id` (not the first) until the guest stops. It waits in the
/// kernel until the guest starts it, then leaves the kernel for device
/// accesses and every [`AP_RUN_NS`].
pub(super) fn run_ap(vm_id: usize, vcpu_id: usize, bsp: usize, machine: &Mutex<Machine>) {
    let mut exit = ViVmExit::Unknown { ec: 0, iss: 0 };
    while !STOPPED.load(Ordering::Acquire) {
        if vmm::run_vcpu(vm_id, vcpu_id, AP_RUN_NS, &mut exit) == usize::MAX {
            println(&alloc::format!(
                "[hv-x86] vcpu {vcpu_id}: run_vcpu kernel error"
            ));
            break;
        }
        // `Hlt`: not started yet. `Preempted`: the run's time is up. Either
        // way other tasks on this CPU (the BSP's thread, the compositor) get
        // it first.
        if matches!(exit, ViVmExit::Hlt | ViVmExit::Preempted) {
            ostd::task::yield_now();
            continue;
        }
        let running = if reaches_gpu(&exit) {
            Some(hand_over(vcpu_id, exit, machine))
        } else {
            let mut machine = yield_lock::lock(machine);
            let running = machine.serve_device(vm_id, vcpu_id, &exit, None);
            if running == Some(true) {
                machine.raise_due(vm_id, bsp, None);
            }
            running
        };
        match running {
            Some(true) => {}
            Some(false) => break,
            None => {
                println(&alloc::format!(
                    "[hv-x86] vcpu {vcpu_id}: guest stopped ({exit:?})"
                ));
                break;
            }
        }
    }
    STOPPED.store(true, Ordering::Release);
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
