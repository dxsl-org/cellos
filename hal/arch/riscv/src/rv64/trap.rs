//! Trap frame structures and S-mode trap handling for ViCell.
//! Uses Vi prefix per project conventions (Luật 6).
//! TrapFrame uses borrowing (&mut) per Luật 8.

/// Trap frame saved on stack during exception/interrupt.
/// Must match the layout in trap.S exactly!
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct ViTrapFrame {
    pub regs: [usize; 32], // x0-x31 (x0 always 0 but slot exists)
    pub sstatus: usize,
    pub sepc: usize,
    pub stval: usize,
    pub scause: usize,
}

impl ViTrapFrame {
    pub fn new() -> Self {
        Self::default()
    }
}

// External assembly functions
extern "C" {
    fn __trap_entry();
    fn __trap_entry_hart0();
    fn __trap_entry_hart1();
    pub fn vi_set_sscratch(kernel_stack_top: usize);
}

/// Initialize trap handling by setting stvec
pub fn init() {
    init_for_hart(0);
}

/// Install the direct trap vector for one logical hart.
pub fn init_for_hart(logical_hart: usize) {
    unsafe {
        let trap_entry = match logical_hart {
            0 => __trap_entry_hart0 as *const () as usize,
            1 => __trap_entry_hart1 as *const () as usize,
            _ => __trap_entry as *const () as usize,
        };
        // Set stvec to direct mode (all traps go to __trap_entry)
        core::arch::asm!("csrw stvec, {}", in(reg) trap_entry);
        // Initialize sscratch to 0 (indicates S-mode context)
        core::arch::asm!("csrw sscratch, zero");
    }
}

/// No-op on RISC-V since the nested-safe sscratch protocol (bug #7 fix).
///
/// INVARIANT: sscratch == 0 for the entire time a hart runs in S-mode; the
/// trap-exit path loads the task's kernel-stack top (frame base + frame size)
/// right before `sret`. The scheduler used to call this mid-S-mode with the
/// next task's SAVED sp — a mid-stack pointer — so any nested trap (timer IRQ
/// after a context with SIE=1 was restored) swapped that pointer in as "the
/// kernel stack" and sprayed a trap frame over live memory.
///
/// x86 (TSS RSP0 / syscall MSR) and ARM (TPIDR) still need their versions —
/// the multi-arch call site in `task::yield_cpu` stays.
pub fn set_kernel_stack(_kernel_stack_top: usize) {}

pub fn enable_interrupts() {
    unsafe {
        #[cfg(target_arch = "riscv64")]
        core::arch::asm!("csrsi sstatus, 0x2"); // SIE
    }
}

/// Rust trap handler called from assembly (vi_trap_handler)
/// Uses borrowed &mut ViTrapFrame per Luật 8
/// This function handles all traps: syscalls, interrupts, exceptions
#[no_mangle]
pub extern "C" fn vi_trap_handler(frame: &mut ViTrapFrame) {
    let scause = frame.scause;
    let is_interrupt = (scause >> 63) != 0;
    let code = scause & 0x7FFF_FFFF_FFFF_FFFF;

    if is_interrupt {
        // Handle interrupts
        match code {
            1 => {
                // S-mode software interrupt — zero-latency RT preemption.
                // Cleared here before yield so it does not re-fire immediately.
                // SAFETY: csrci on sip.SSIP is permitted from S-mode (priv spec §4.1.3).
                unsafe { core::arch::asm!("csrci sip, 0x2") };
                // Reuse the timer tick path: just run the scheduler.
                unsafe {
                    vi_timer_tick();
                }
            }
            5 => {
                // S-mode timer interrupt — preemption point.
                // SAFETY: vi_timer_tick is defined in kernel::task and linked
                // via extern "Rust".  It increments the tick counter, rearmed
                // the timer, and calls yield_cpu() to preempt if needed.
                unsafe {
                    vi_timer_tick();
                }
            }
            9 => {
                // S-mode external interrupt (PLIC)
                // Claim first, dispatch handler, complete AFTER handler per PLIC spec.
                if let Some((context, irq)) = plic_claim() {
                    // SAFETY: the kernel-owned dispatcher is linked via `extern "Rust"`
                    // and must stay allocation-free in interrupt-adjacent paths.
                    unsafe {
                        vi_handle_riscv_external_irq(irq);
                    }
                    // PLIC complete must come AFTER the device handler has run.
                    plic_complete(context, irq);
                }
            }
            _ => {
                // Unknown interrupt - log but don't panic
                // log::warn!("Unknown interrupt: {}", code);
            }
        }
    } else {
        // Handle exceptions
        match code {
            8 => {
                // Environment call from U-mode (syscall)
                vi_handle_syscall(frame);
                // Advance PC past ecall instruction (4 bytes)
                frame.sepc += 4;
            }
            9 => {
                // Environment call from S-mode (should not happen normally)
                frame.sepc += 4;
            }
            _ => {
                // Illegal instruction, page faults, or other unhandled exception.
                //
                // Distinguish U-mode Cell faults from S-mode kernel faults using SPP
                // (sstatus bit 8): SPP=0 means the trap came from U-mode (a Cell).
                // Checking only `cell_id != 0` is insufficient — if the kernel faults
                // while servicing a Cell's syscall, CURRENT_CELL_ID is still non-zero
                // but the CPU is in S-mode.  Misclassifying that as a Cell fault
                // silently kills the Cell and hides the kernel bug.
                //
                // SAFETY: vi_current_cell_id and vi_terminate_on_fault are defined
                // in kernel::task and linked via extern "Rust".
                if code == 15 && unsafe { vi_tlb_shootdown_test_fault(frame) } {
                    return;
                }
                let from_user = (frame.sstatus & 0x100) == 0; // SPP bit: 0=U-mode
                let cell_id = unsafe { vi_current_cell_id() };
                if from_user && cell_id != 0 {
                    // Genuine U-mode Cell fault — terminate the Cell, let kernel continue.
                    unsafe {
                        vi_terminate_on_fault(code, frame.sepc, frame.stval);
                    }
                    // vi_terminate_on_fault calls yield_cpu() which switches away.
                    // We should not reach here, but return safely if we do.
                } else {
                    // True kernel fault (S-mode) or U-mode fault without a registered Cell.
                    panic!(
                        "Cellos: Kernel exception: scause={} sepc={:#x} stval={:#x} sstatus={:#x}",
                        code, frame.sepc, frame.stval, frame.sstatus
                    );
                }
            }
        }
    }
}

fn plic_claim() -> Option<(usize, u32)> {
    // SAFETY: the kernel exports a no-allocation hart-local lookup. `usize::MAX`
    // is its fail-closed sentinel when no SoC context mapping is available.
    let context = unsafe { vi_riscv_plic_context() };
    if context == usize::MAX {
        return None;
    }
    crate::common::plic::claim(context).map(|irq| (context, irq))
}

fn plic_complete(context: usize, irq: u32) {
    crate::common::plic::complete(context, irq);
}

/// Handle syscall from userspace (Vi prefix per Luật 6)
fn vi_handle_syscall(frame: &mut ViTrapFrame) {
    extern "Rust" {
        fn ViCell_syscall_dispatch(frame: &mut ViTrapFrame);
    }
    unsafe {
        ViCell_syscall_dispatch(frame);
    }
}

extern "Rust" {
    fn vi_riscv_plic_context() -> usize;
    fn vi_handle_riscv_external_irq(irq: u32);
    /// Called on every S-mode timer interrupt.  Defined in `kernel::task`.
    fn vi_timer_tick();
    /// Terminate the currently-executing Cell on hardware fault.  Defined in `kernel::task`.
    fn vi_terminate_on_fault(cause: usize, pc: usize, fault_addr: usize);
    /// Returns CURRENT_CELL_ID (0 = kernel, nonzero = a Cell).
    fn vi_current_cell_id() -> usize;
    /// Returns true only when a test-hooks shootdown probe consumed its exact
    /// expected S-mode store fault; production kernels always return false.
    fn vi_tlb_shootdown_test_fault(frame: &mut ViTrapFrame) -> bool;
}
