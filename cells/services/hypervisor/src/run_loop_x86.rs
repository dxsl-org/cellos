//! x86 (SVM/VT-x) VmExit dispatch loop for the hypervisor cell.
//!
//! The x86 twin of the aarch64 [`crate::run_loop`]. It services the port-I/O
//! device models (16550 UART, 8259 PIC, 8253 PIT, CMOS RTC), raises the PIT
//! tick and the virtio interrupts, forwards host keystrokes (kernel UART ring,
//! `sys_read(0)`) into the 16550 RX FIFO as guest IRQ4, serves the launcher
//! session of an app-mode hypervisor (`app_session_x86`: its window's input,
//! display resizes and termination), and surfaces guest shutdown. RIP
//! advancing is done kernel-side (IOIO via EXITINFO2; HLT/MSR/PAUSE via
//! instruction-length), so the cell never touches guest PC here. The guest
//! local APIC and its timer are the kernel's.
//!
//! A guest with more than one vCPU gets a thread per vCPU (`x86_machine`);
//! this loop is the first vCPU's.

extern crate alloc;
#[path = "x86-irq-dispatch.rs"]
mod x86_irq_dispatch;
#[path = "x86-machine.rs"]
mod x86_machine;
#[path = "x86-mmio-dispatch.rs"]
mod x86_mmio_dispatch;
#[path = "x86-port-dispatch.rs"]
mod x86_port_dispatch;

use crate::app_session_x86::{AppSession, SessionRequest};
use crate::{
    acpi_pm::AcpiPm, client_audio::AudioClient, client_clipboard, cmos_rtc::CmosRtc,
    exit_counts::ExitCounts, pic_8259::Pic8259, pit_8253::Pit8253, uart_16550::Uart16550,
    virtio_9p::P9Dev, virtio_blk::BlkDisk, virtio_console::Console, virtio_gpu::GpuDev,
    virtio_input::InputDev, virtio_mmio::VirtioMmio, virtio_net::NetDev, virtio_snd::SndDev, vmm,
    yield_lock,
};
use abi::hypervisor::ViVmExit;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;
use ostd::io::println;
use ostd::sync::Mutex;
use x86_machine::{Machine, EAGER_POLLS, STOPPED};

#[cfg(feature = "hostile-backend-recovery")]
const HOSTILE_PREEMPTION_PORT: u16 = 0x5050;
#[cfg(feature = "hostile-backend-recovery")]
const HOSTILE_PREEMPTION_ARM: u32 = 0x5052_4545;

pub enum RunOutcome {
    Shutdown,
}

/// Run the guest until it powers off or an unrecoverable exit. `vcpus` are
/// the guest's vCPU ids, the first one's first; the others get a thread each.
pub fn run(
    vm_id: usize,
    vcpus: &[usize],
    disk_file: Option<(
        crate::persistent_disk::PersistentDisk,
        crate::persistent_disk::DiskSpec,
    )>,
    launcher: Option<(AppSession, u32, u32)>,
) -> RunOutcome {
    let vcpu_id = vcpus[0];
    let net_tid = ostd::service::provider_tid(abi::syscall::service::NET).unwrap_or(0);
    let (mut gpu, mut session) = match launcher {
        Some((session, width, height)) => (
            GpuDev::for_client(session.client_tid(), width, height),
            Some(session),
        ),
        None => {
            let comp_tid =
                ostd::service::provider_tid(abi::syscall::service::COMPOSITOR).unwrap_or(0);
            let (width, height) = ostd::syscall::sys_get_resolution();
            let gpu = GpuDev::new(
                comp_tid,
                if width == 0 { 1024 } else { width },
                if height == 0 { 768 } else { height },
            );
            (gpu, None)
        }
    };
    gpu.bring_up();
    // x86: completions raise ISR status only; `x86_irq_dispatch` delivers them on DISPLAY_IRQ.
    let mut gpu_vmio = VirtioMmio::default();
    let machine = Arc::new(Mutex::new(Machine {
        blk: BlkDisk::new(disk_file, None),
        blk_vmio: VirtioMmio::default(),
        net: NetDev::new(net_tid, None),
        net_vmio: VirtioMmio::default(),
        input: InputDev::new(None),
        input_vmio: VirtioMmio::default(),
        console: Console::clipboard(),
        console_vmio: VirtioMmio::default(),
        p9: P9Dev::new(),
        p9_vmio: VirtioMmio::default(),
        snd: SndDev::new(),
        snd_vmio: VirtioMmio::default(),
        uart: Uart16550::new(),
        pic: Pic8259::new(),
        pit: Pit8253::new(),
        rtc: CmosRtc::new(),
        pm: AcpiPm::new(),
        eager_polls: 0,
        handed_over: Vec::new(),
        answered: Vec::new(),
        handed_over_count: 0,
    }));
    // With other vCPUs this thread alone serves the GPU for them: its runs
    // stay short so a handed-over access waits about one of them.
    let smp = vcpus.len() > 1;
    STOPPED.store(false, Ordering::Release);
    for &ap in &vcpus[1..] {
        let machine = machine.clone();
        let spawned = ostd::task::spawn(move || x86_machine::run_ap(vm_id, ap, vcpu_id, &machine));
        if matches!(spawned, ostd::syscall::SyscallResult::Err(_)) {
            println(&alloc::format!(
                "[hv-x86] vcpu {ap}: no thread — the guest waits for it in vain"
            ));
        }
    }
    let mut exit = ViVmExit::Unknown { ec: 0, iss: 0 };
    let mut counts = ExitCounts::new();
    let mut audio = AudioClient::new();
    #[cfg(feature = "hostile-backend-recovery")]
    let mut hostile_preemption_armed = false;

    while !STOPPED.load(Ordering::Acquire) {
        let budget_ns = {
            let mut machine = yield_lock::lock(&machine);
            machine.serve_handed_over(vm_id, &mut gpu, &mut gpu_vmio);
            gpu.poll_damage();
            machine.raise_due(vm_id, vcpu_id, Some(&mut gpu_vmio));
            run_budget_ns(&machine.pit, &machine.pic, smp || machine.eager_polls != 0)
        };
        counts.entering();
        let ret = vmm::run_vcpu(vm_id, vcpu_id, budget_ns, &mut exit);
        if ret == usize::MAX {
            println("[hv-x86] run_vcpu kernel error — aborting");
            break;
        }
        counts.record(&exit);

        let mut guard = yield_lock::lock(&machine);
        let m = &mut *guard;
        #[cfg(feature = "hostile-backend-recovery")]
        if let ViVmExit::PortOut { port, val, .. } = exit {
            if port == HOSTILE_PREEMPTION_PORT && val == HOSTILE_PREEMPTION_ARM {
                hostile_preemption_armed = true;
                continue;
            }
        }
        if let Some(running) =
            m.serve_device(vm_id, vcpu_id, &exit, Some((&mut gpu, &mut gpu_vmio)))
        {
            if !running {
                break;
            }
            continue;
        }
        match exit {
            // ── The guest is idle (`Hlt`, where the kernel reports it), or a
            //    host interrupt or the budget took it off the CPU (`Preempted`,
            //    which is also how a guest shows up that waits halted in the
            //    guest). Either way the outside world gets its turn. ──────────
            ViVmExit::Hlt | ViVmExit::Preempted => {
                let preempted = matches!(exit, ViVmExit::Preempted);
                #[cfg(feature = "hostile-backend-recovery")]
                if preempted && hostile_preemption_armed {
                    println("[hv-virtio-host] vcpu-preempted");
                    hostile_preemption_armed = false;
                }
                m.uart
                    .flush_tx_if_quiescent(!preempted, ostd::syscall::sys_get_time());
                let frame = x86_irq_dispatch::service_idle(
                    vm_id,
                    vcpu_id,
                    &mut m.uart,
                    &m.pic,
                    &mut m.net,
                    &mut m.net_vmio,
                    session.is_none(),
                );
                let served = serve_host(
                    m,
                    &mut gpu,
                    &mut gpu_vmio,
                    &mut session,
                    &mut audio,
                    vm_id,
                    vcpu_id,
                );
                if served.terminate {
                    break;
                }
                m.eager_polls = if frame || served.input {
                    EAGER_POLLS
                } else {
                    m.eager_polls.saturating_sub(1)
                };
                if preempted {
                    drop(guard);
                    ostd::task::yield_now();
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
                break;
            }

            ViVmExit::Shutdown => {
                println("[hv-x86] guest shutdown");
                break;
            }

            ViVmExit::Unknown { ec, iss } => {
                println(&alloc::format!(
                    "[hv-x86] unknown vmexit ec=0x{:x} iss=0x{:x}",
                    ec,
                    iss
                ));
                break;
            }

            // ── Device exits were served above; aarch64-only exits are never
            //    emitted on the x86 world-switch ────────────────────────────────
            _ => {
                println("[hv-x86] unexpected aarch64 vmexit — shutting down VM");
                break;
            }
        }
    }
    STOPPED.store(true, Ordering::Release);
    counts.report();
    // Files the guest left open are synced and closed with the guest.
    let (requests, errors) = yield_lock::lock(&machine).p9.end_session();
    println(&alloc::format!(
        "[hv-x86] 9p requests: {requests}, errors: {errors}"
    ));
    if smp {
        println(&alloc::format!(
            "[hv-x86] GPU accesses handed to the first vCPU: {}",
            yield_lock::lock(&machine).handed_over_count
        ));
    }
    audio.finish(&yield_lock::lock(&machine).snd);
    // Every way out ends the guest: stop presenting, which is how a bound
    // launcher learns of it.
    gpu.shutdown();
    RunOutcome::Shutdown
}

/// Work done at every guest idle/preemption boundary besides interrupt
/// delivery: the launcher session (or, without one, compositor reconnection),
/// flushing queued input into the guest, and the session's clipboard and the
/// guest's sound, whose Grants and calls must come from this (the BSP's)
/// thread.
fn serve_host(
    machine: &mut Machine,
    gpu: &mut GpuDev,
    gpu_vmio: &mut VirtioMmio,
    session: &mut Option<AppSession>,
    audio: &mut AudioClient,
    vm_id: usize,
    vcpu_id: usize,
) -> Served {
    audio.pump(&mut machine.snd, ostd::syscall::sys_get_time());
    let Machine {
        input,
        input_vmio,
        console,
        console_vmio,
        ..
    } = machine;
    let terminate = match session {
        Some(session) => matches!(
            session.poll(gpu, gpu_vmio, input),
            SessionRequest::Terminate
        ),
        None => {
            // Console (`hv`) mode. The compositor also posts lifecycle and
            // forwarded-input frames to this task; they must be drained here,
            // or the scanout bridge's next request reads one as its reply.
            for event in ostd::input::poll_events(16) {
                push_console_event(input, event);
            }
            gpu.reconnect_compositor(
                ostd::service::provider_tid(abi::syscall::service::COMPOSITOR).unwrap_or(0),
            );
            false
        }
    };
    let input = input.flush_events(&input_vmio.queue_cfg(0), vm_id, vcpu_id);
    if input {
        input_vmio.signal_used();
    }
    let clipboard = session
        .as_mut()
        .is_some_and(|session| client_clipboard::serve(session, console, console_vmio, vm_id));
    Served {
        terminate,
        input: input || clipboard,
    }
}

/// What [`serve_host`] found.
struct Served {
    /// The launcher ended the session.
    terminate: bool,
    /// Host input or clipboard text went into the guest.
    input: bool,
}

/// The short poll interval: what an inbound frame or a pointer move waits at
/// most while the guest is in a conversation.
const EAGER_POLL_NS: u64 = 1_000_000;
/// The shortest budget: 0 would mean "no bound" to the kernel.
const SOONEST_NS: u64 = 50_000;

/// How long the guest may run before this Cell wants control back: until the
/// next PIT tick is due, so the tick is raised on time, and no longer than the
/// pace at which the host console, the network and the launcher are polled —
/// one scheduler tick, or [`EAGER_POLL_NS`] while `eager`. Where the kernel
/// keeps a halted guest in the guest, this is also how long an idle guest
/// sleeps.
///
/// A tick whose line the guest has masked is not waited for: it cannot be
/// raised, and a tick that stays due would otherwise shrink every budget to
/// [`SOONEST_NS`].
fn run_budget_ns(pit: &Pit8253, pic: &Pic8259, eager: bool) -> u64 {
    let poll_ns = if eager {
        EAGER_POLL_NS
    } else {
        vmm::SCHED_TICK_BUDGET_NS
    };
    pic.irq0()
        .and_then(|_| pit.ns_until_irq0())
        .map_or(poll_ns, |tick_ns| tick_ns.min(poll_ns))
        .max(SOONEST_NS)
}

/// Host input with keyboard focus on the console `hv` reaches the guest's tablet
/// as keys, buttons and wheel; relative motion has no absolute position to map to.
fn push_console_event(input: &mut InputDev, event: wire::input::InputEvent) {
    match event {
        wire::input::InputEvent::Key(key) => {
            let pressed = matches!(
                key.state,
                wire::input::KeyState::Pressed | wire::input::KeyState::Repeated
            );
            if key.scancode > 0 {
                input.push_key(key.scancode as u16, pressed);
            }
        }
        wire::input::InputEvent::MouseButton { button, state } => {
            input.push_mouse_button(
                crate::virtio_input::button_code(button),
                state == wire::input::KeyState::Pressed,
            );
        }
        wire::input::InputEvent::MouseScroll { dy, .. } => input.push_mouse_scroll(dy),
        wire::input::InputEvent::MouseMove { .. } => {}
    }
}
