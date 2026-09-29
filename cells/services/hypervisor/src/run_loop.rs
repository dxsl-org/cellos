//! VmExit dispatch loop.
//!
//! C2 (Red Team): run_vcpu uses a 10ms preempt budget so VFS/Net Cells stay live.
//! m2 (Red Team): default arm logs unregistered IPAs and continues (never silent).

extern crate alloc;

#[cfg(feature = "board-rpi3")]
use crate::gicc::Gicc;
use crate::{
    gicd::Gicd,
    net_backend,
    pl011::Pl011,
    psci, timer,
    virtio_blk::BlkDisk,
    virtio_console::Console,
    virtio_gpu::GpuDev,
    virtio_input::{InputDev, INPUT_SPI},
    virtio_mmio::{self, VirtioMmio},
    virtio_net::NetDev,
    vmm,
};
use api::hypervisor::ViVmExit;
use api::syscall::service;
use ostd::io::println;
use ostd::syscall::{sys_get_resolution, sys_lookup_service};

pub enum RunOutcome {
    Shutdown,
}

/// Main VMM run loop. Runs until the guest PSCI SYSTEM_OFF or an unrecoverable exit.
pub fn run(
    vm_id: usize,
    vcpu_id: usize,
    disk_file: Option<(usize, api::vfs_file_handles::ViVfsFileHandle, u64)>,
) -> RunOutcome {
    let compositor_tid = sys_lookup_service(service::COMPOSITOR).unwrap_or(0);
    let (width, height) = sys_get_resolution();

    let mut pl011 = Pl011::new();
    let mut gicd = Gicd::new();
    #[cfg(feature = "board-rpi3")]
    let mut gicc = Gicc::new();
    #[cfg(feature = "board-rpi3")]
    let mut timer_regs = [0u64; 32];
    let mut console = Console::new();
    let mut vmio = VirtioMmio::default();
    let mut blk = BlkDisk::new(disk_file, Some(17));
    let mut blk_vmio = VirtioMmio::default();
    let mut net = NetDev::new(sys_lookup_service(service::NET).unwrap_or(0), Some(18));
    let mut net_vmio = VirtioMmio::default();
    let mut gpu = GpuDev::new(
        compositor_tid,
        if width == 0 { 1024 } else { width },
        if height == 0 { 768 } else { height },
    );
    let mut gpu_vmio = VirtioMmio::default();
    let mut input = InputDev::new(Some(INPUT_SPI));
    let mut input_vmio = VirtioMmio::default();
    gpu.bring_up();
    let mut exit = ViVmExit::Unknown { ec: 0, iss: 0 };
    #[cfg(feature = "board-rpi3")]
    let mut diag = [0u64; 3]; // total exits, timer preemptions, WFI

    loop {
        gpu.poll_damage();
        #[cfg(feature = "board-rpi3")]
        {
            gicd.collect_device_irqs();
            if vmm::guest_timer_regs(vm_id, vcpu_id, &mut timer_regs) == usize::MAX {
                println("[hv] guest timer snapshot failed");
                gpu.shutdown();
                return RunOutcome::Shutdown;
            }
            gicd.set_level(
                timer::VIRT_TIMER_PPI,
                timer::guest_timer_asserted(timer_regs[0], timer_regs[1]),
            );
            gicd.set_level(crate::pl011::PL011_SPI, pl011.irq_pending());
            if let Some(intid) = gicc.pending_irq(&gicd) {
                if vmm::request_virtual_irq(vm_id, vcpu_id, intid) == usize::MAX {
                    println("[hv] InjectIrq failed");
                    gpu.shutdown();
                    return RunOutcome::Shutdown;
                }
            }
        }
        let ret = vmm::run_vcpu(vm_id, vcpu_id, &mut exit);
        if ret == usize::MAX {
            println("[hv] run_vcpu kernel error — aborting");
            gpu.shutdown();
            return RunOutcome::Shutdown;
        }

        #[cfg(feature = "board-rpi3")]
        {
            diag[0] += 1;
            if matches!(exit, ViVmExit::Preempted) {
                diag[1] += 1;
            }
            if matches!(exit, ViVmExit::Wfi) {
                diag[2] += 1;
            }
            if diag[0] <= 8 || diag[0] % 128 == 0 {
                let mut rb = [0u64; 32];
                vmm::vcpu_regs(vm_id, vcpu_id, &mut rb, false);
                println(&alloc::format!(
                    "[hv-diag] exits={} preempt={} wfi={} pc={:#x} lr={:#x} x0={:#x} x19={:#x} x20={:#x} ctl={:#x} cval={:#x} last={:?}",
                    diag[0], diag[1], diag[2], rb[31], rb[30], rb[0], rb[19], rb[20], timer_regs[0], timer_regs[1], exit
                ));
            }
        }
        match exit {
            // ── HVC (PSCI + unknown) ──────────────────────────────────────────
            ViVmExit::Hvc { imm: 0, mut regs } => match psci::dispatch(&mut regs) {
                psci::PsciAction::Return(result) => {
                    let mut rb = [0u64; 32];
                    vmm::vcpu_regs(vm_id, vcpu_id, &mut rb, false);
                    rb[0] = result;
                    vmm::vcpu_regs(vm_id, vcpu_id, &mut rb, true);
                }
                psci::PsciAction::SystemOff | psci::PsciAction::SystemReset => {
                    println("[hv] PSCI SYSTEM_OFF");
                    gpu.shutdown();
                    return RunOutcome::Shutdown;
                }
            },
            ViVmExit::Hvc { imm, regs: _ } => {
                // HVC returns NOT_SUPPORTED; ELR_EL2 already points past HVC.
                println(&alloc::format!("[hv] unknown HVC imm={}", imm));
                let mut rb = [0u64; 32];
                vmm::vcpu_regs(vm_id, vcpu_id, &mut rb, false);
                rb[0] = u64::MAX; // SMCCC NOT_SUPPORTED = -1
                vmm::vcpu_regs(vm_id, vcpu_id, &mut rb, true);
            }

            // ── MMIO write ───────────────────────────────────────────────────
            ViVmExit::MmioWrite { ipa, size, val } => {
                if Pl011::owns(ipa) {
                    pl011.write(ipa - crate::pl011::PL011_BASE_IPA, val);
                } else if Gicd::owns_gicd(ipa) {
                    gicd.write(ipa - crate::gicd::GICD_BASE_IPA, val, size);
                } else if Gicd::owns_gicc(ipa) {
                    #[cfg(feature = "board-rpi3")]
                    gicc.write(&mut gicd, ipa - crate::gicd::GICC_BASE_IPA, val);
                } else if virtio_mmio::owns(ipa) {
                    let (slot, off) = virtio_mmio::slot_and_offset(ipa);
                    match slot {
                        0 => vmio.mmio_write(off, val as u32, &mut console, vm_id, vcpu_id),
                        1 => blk_vmio.mmio_write(off, val as u32, &mut blk, vm_id, vcpu_id),
                        2 => net_vmio.mmio_write(off, val as u32, &mut net, vm_id, vcpu_id),
                        3 => gpu_vmio.mmio_write(off, val as u32, &mut gpu, vm_id, vcpu_id),
                        4 => input_vmio.mmio_write(off, val as u32, &mut input, vm_id, vcpu_id),
                        _ => {}
                    }
                } else {
                    println(&alloc::format!(
                        "[hv] unknown MMIO write ipa=0x{:x} val=0x{:x}",
                        ipa,
                        val
                    ));
                }
                advance_pc(vm_id, vcpu_id);
            }

            // ── MMIO read ────────────────────────────────────────────────────
            ViVmExit::MmioRead { ipa, size, reg } => {
                let val = if Pl011::owns(ipa) {
                    pl011.read(ipa - crate::pl011::PL011_BASE_IPA)
                } else if Gicd::owns_gicd(ipa) {
                    gicd.read(ipa - crate::gicd::GICD_BASE_IPA, size)
                } else if Gicd::owns_gicc(ipa) {
                    #[cfg(feature = "board-rpi3")]
                    {
                        gicc.read(&mut gicd, ipa - crate::gicd::GICC_BASE_IPA)
                    }
                    #[cfg(not(feature = "board-rpi3"))]
                    {
                        0u64
                    } // QEMU virt uses hardware GICV.
                } else if virtio_mmio::owns(ipa) {
                    let (slot, off) = virtio_mmio::slot_and_offset(ipa);
                    match slot {
                        0 => vmio.mmio_read(off, &console),
                        1 => blk_vmio.mmio_read(off, &blk),
                        2 => net_vmio.mmio_read(off, &net),
                        3 => gpu_vmio.mmio_read(off, &gpu),
                        4 => input_vmio.mmio_read(off, &input),
                        _ => 0,
                    }
                } else {
                    println(&alloc::format!("[hv] unknown MMIO read ipa=0x{:x}", ipa));
                    0u64
                };
                let mut rb = [0u64; 32];
                vmm::vcpu_regs(vm_id, vcpu_id, &mut rb, false);
                if (reg as usize) < 31 {
                    rb[reg as usize] = val;
                }
                vmm::vcpu_regs(vm_id, vcpu_id, &mut rb, true);
                advance_pc(vm_id, vcpu_id);
            }

            // ── WFI — poll guest RX; Pi timer IRQ follows its deadline ───────
            ViVmExit::Wfi => {
                #[cfg(not(feature = "board-rpi3"))]
                timer::inject_timer_irq(vm_id, vcpu_id);
                gpu.reconnect_compositor(sys_lookup_service(service::COMPOSITOR).unwrap_or(0));
                if let Some(frame) = net_backend::try_receive(&mut net.backend) {
                    if net.push_rx_frame(&frame, vm_id, vcpu_id, &net_vmio) {
                        net_vmio.signal_used();
                    }
                }
                forward_input_events(&mut input, &mut input_vmio, vm_id, vcpu_id);
                #[cfg(feature = "board-rpi3")]
                drain_host_serial(&mut pl011);
                #[cfg(feature = "board-rpi3")]
                ostd::task::yield_now();
            }

            // ── Preemption budget expired (C2 yield) — poll RX before re-enter
            ViVmExit::Preempted => {
                gpu.reconnect_compositor(sys_lookup_service(service::COMPOSITOR).unwrap_or(0));
                if let Some(frame) = net_backend::try_receive(&mut net.backend) {
                    if net.push_rx_frame(&frame, vm_id, vcpu_id, &net_vmio) {
                        net_vmio.signal_used();
                    }
                }
                forward_input_events(&mut input, &mut input_vmio, vm_id, vcpu_id);
                #[cfg(feature = "board-rpi3")]
                drain_host_serial(&mut pl011);
                ostd::task::yield_now();
            }

            // ── Guest shutdown ────────────────────────────────────────────────
            ViVmExit::Shutdown => {
                println("[hv] ViVmExit::Shutdown");
                gpu.shutdown();
                return RunOutcome::Shutdown;
            }

            // ── Sysreg trap — guest timer is physical on Cortex-A53 ─────────
            ViVmExit::SysReg { rt, is_write, .. } => {
                if !is_write && (rt as usize) < 31 {
                    let mut rb = [0u64; 32];
                    vmm::vcpu_regs(vm_id, vcpu_id, &mut rb, false);
                    rb[rt as usize] = 0;
                    vmm::vcpu_regs(vm_id, vcpu_id, &mut rb, true);
                }
                advance_pc(vm_id, vcpu_id);
            }

            // ── Unknown exit ─────────────────────────────────────────────────
            ViVmExit::Unknown { ec, iss } => {
                // Guest PC pinpoints WHERE the guest faulted — without it an
                // ec=0x20 (guest instruction abort) is undebuggable.
                let mut rb = [0u64; 32];
                vmm::vcpu_regs(vm_id, vcpu_id, &mut rb, false);
                println(&alloc::format!(
                    "[hv] unknown vmexit ec=0x{:x} iss=0x{:x} pc=0x{:x} x0=0x{:x} x30=0x{:x}",
                    ec,
                    iss,
                    rb[31],
                    rb[0],
                    rb[30],
                ));
                gpu.shutdown();
                return RunOutcome::Shutdown;
            }

            // ── x86-only exits (SVM/VT-x) — never emitted on this aarch64 cell;
            //    the x86 personality (P05) handles them in its own run loop. ──
            ViVmExit::PortIn { .. }
            | ViVmExit::PortOut { .. }
            | ViVmExit::Hlt
            | ViVmExit::Msr { .. } => {
                println("[hv] unexpected x86 vmexit on aarch64 — shutting down VM");
                gpu.shutdown();
                return RunOutcome::Shutdown;
            }
        }
    }
}

#[cfg(feature = "board-rpi3")]
fn drain_host_serial(pl011: &mut Pl011) {
    let mut buf = [0u8; 32];
    while let Ok(n) = ostd::syscall::sys_read(0, &mut buf) {
        if n == 0 {
            break;
        }
        for &byte in &buf[..n] {
            pl011.push_rx(byte);
        }
        if n < buf.len() {
            break;
        }
    }
}

fn forward_input_events(
    input: &mut InputDev,
    input_vmio: &mut VirtioMmio,
    vm_id: usize,
    vcpu_id: usize,
) {
    for ev in ostd::input::poll_events(16) {
        match ev {
            api::input::InputEvent::Key(ke) => {
                let pressed = ke.state == api::input::KeyState::Pressed
                    || ke.state == api::input::KeyState::Repeated;
                let code = if ke.scancode > 0 {
                    ke.scancode as u16
                } else {
                    ke.keysym as u16
                };
                input.push_key(code, pressed);
            }
            api::input::InputEvent::MouseMove { dx, dy, .. } => {
                input.push_mouse_move(dx, dy);
            }
            api::input::InputEvent::MouseButton { button, state } => {
                let pressed = state == api::input::KeyState::Pressed;
                let btn = match button {
                    api::input::MouseButton::Left => crate::virtio_input::BTN_LEFT,
                    api::input::MouseButton::Right => crate::virtio_input::BTN_RIGHT,
                    api::input::MouseButton::Middle => crate::virtio_input::BTN_MIDDLE,
                    _ => crate::virtio_input::BTN_LEFT,
                };
                input.push_mouse_button(btn, pressed);
            }
            api::input::InputEvent::MouseScroll { dy, .. } => {
                input.push_mouse_scroll(dy);
            }
        }
    }
    if input.flush_events(&input_vmio.queue_cfg(0), vm_id, vcpu_id) {
        input_vmio.signal_used();
    }
}

/// Advance guest PC by 4 bytes past the trapped instruction.
/// reg_buf layout: x0..x30 at [0..30], PC at [31].
fn advance_pc(vm_id: usize, vcpu_id: usize) {
    let mut rb = [0u64; 32];
    vmm::vcpu_regs(vm_id, vcpu_id, &mut rb, false);
    rb[31] = rb[31].wrapping_add(4);
    vmm::vcpu_regs(vm_id, vcpu_id, &mut rb, true);
}
