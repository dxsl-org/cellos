//! S-mode trap handling for ViCell RV32 Nano.
//!
//! Mirrors `rv64/trap.rs` with RV32-specific differences:
//! - `ViTrapFrame32`: 32-bit register slots (144 bytes vs 288 bytes on RV64)
//! - Interrupt bit is bit 31 of scause (not bit 63)
//! - No PLIC claim in Phase 31 Nano (timer/software interrupts only)

/// Trap frame saved on the stack by `__trap_entry32`.
///
/// # Layout invariant
/// Offsets MUST match `rv32/asm/trap.S` exactly.
/// regs[N] is at byte offset N*4; CSR fields follow at offset 128+.
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct ViTrapFrame32 {
    /// General-purpose registers x0..x31 (4 bytes each).
    pub regs: [u32; 32],
    pub sstatus: u32,
    pub sepc: u32,
    pub stval: u32,
    pub scause: u32,
}

const _: () = assert!(core::mem::size_of::<ViTrapFrame32>() == 144);

extern "C" {
    fn __trap_entry32();
}

/// Set `stvec` to the RV32 trap entry and clear `sscratch`.
///
/// Must be called once during kernel init before enabling interrupts.
pub fn init() {
    unsafe {
        let entry = __trap_entry32 as *const () as usize;
        // Direct mode: all traps go to __trap_entry32.
        // SAFETY: csrw stvec with a valid function pointer is safe in S-mode.
        core::arch::asm!("csrw stvec, {}", in(reg) entry, options(nomem, nostack));
        // sscratch == 0 signals "trap came from S-mode" (Phase-31: always true).
        core::arch::asm!("csrw sscratch, zero", options(nomem, nostack));
    }
}

/// Rust-level trap handler invoked from `__trap_entry32`.
///
/// `frame` points to the `ViTrapFrame32` on the interrupted task's stack.
#[no_mangle]
pub extern "C" fn vi_trap_handler32(frame: &mut ViTrapFrame32) {
    let scause = frame.scause;
    // RV32: interrupt bit is bit 31 (not bit 63 like RV64).
    let is_interrupt = (scause >> 31) != 0;
    let code = scause & 0x7FFF_FFFF;

    if is_interrupt {
        match code {
            1 => {
                // S-mode software interrupt (SSIP) — zero-latency RT preemption.
                // SAFETY: csrci on sip.SSIP is valid from S-mode (priv spec §4.1.3).
                unsafe {
                    core::arch::asm!("csrci sip, 0x2", options(nomem, nostack));
                }
                unsafe {
                    vi_timer_tick();
                }
            }
            5 => {
                // S-mode timer interrupt (STIP) — scheduler preemption point.
                unsafe {
                    vi_timer_tick();
                }
            }
            _ => {
                // Unknown interrupt — ignore silently to avoid nested fault.
            }
        }
    } else {
        match code {
            8 | 9 => {
                // ecall from U-mode (8) or S-mode (9).
                vi_handle_syscall(frame);
                frame.sepc += 4; // advance past ecall instruction
            }
            _ => {
                // Exception: illegal instruction, page fault, misalign, etc.
                // If a Cell is running, kill it; otherwise panic.
                let cell_id = unsafe { vi_current_cell_id() };
                if cell_id != 0 {
                    unsafe {
                        vi_terminate_on_fault(code as usize, frame.sepc as usize);
                    }
                    // vi_terminate_on_fault calls yield_cpu() — we should not reach here.
                } else {
                    panic!(
                        "ViCell/RV32: kernel exception scause={} sepc={:#x} stval={:#x}",
                        code, frame.sepc, frame.stval
                    );
                }
            }
        }
    }
}

fn vi_handle_syscall(frame: &mut ViTrapFrame32) {
    extern "Rust" {
        // Same symbol name as riscv64; kernel provides this via cfg-gated stub.
        fn ViCell_syscall_dispatch(frame: &mut ViTrapFrame32);
    }
    unsafe {
        ViCell_syscall_dispatch(frame);
    }
}

extern "Rust" {
    /// Called on every S-mode timer interrupt. Defined in `kernel::task`.
    fn vi_timer_tick();
    /// Terminate the currently-executing Cell on hardware fault. Defined in `kernel::task`.
    fn vi_terminate_on_fault(scause: usize, sepc: usize);
    /// Returns CURRENT_CELL_ID (0 = kernel, nonzero = a Cell). Defined in `kernel::task`.
    fn vi_current_cell_id() -> usize;
}
