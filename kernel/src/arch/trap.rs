/// RISC-V Trap Handler
/// 
/// Handles interrupts and exceptions in machine mode.
/// This is the core of preemptive multitasking - timer interrupts
/// will trigger context switches.

use log::warn;

/// Trap cause codes for RISC-V
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum TrapCause {
    /// Machine timer interrupt
    MachineTimer = 0x8000_0000_0000_0007,
    /// Machine software interrupt
    MachineSoftware = 0x8000_0000_0000_0003,
    /// Machine external interrupt
    MachineExternal = 0x8000_0000_0000_000B,
    /// Unknown/unsupported trap
    Unknown,
}

impl From<usize> for TrapCause {
    fn from(value: usize) -> Self {
        match value {
            0x8000_0000_0000_0007 => TrapCause::MachineTimer,
            0x8000_0000_0000_0003 => TrapCause::MachineSoftware,
            0x8000_0000_0000_000B => TrapCause::MachineExternal,
            _ => TrapCause::Unknown,
        }
    }
}

#[cfg(target_arch = "riscv64")]
core::arch::global_asm!(
    r#"
    .section .text.trap
    .global _trap_entry
    .align 4
_trap_entry:
    # 1. Save all registers to stack
    addi sp, sp, -256
    
    sd ra, 0(sp)
    sd t0, 8(sp)
    sd t1, 16(sp)
    sd t2, 24(sp)
    sd t3, 32(sp)
    sd t4, 40(sp)
    sd t5, 48(sp)
    sd t6, 56(sp)
    
    sd a0, 64(sp)
    sd a1, 72(sp)
    sd a2, 80(sp)
    sd a3, 88(sp)
    sd a4, 96(sp)
    sd a5, 104(sp)
    sd a6, 112(sp)
    sd a7, 120(sp)
    
    sd s0, 128(sp)
    sd s1, 136(sp)
    sd s2, 144(sp)
    sd s3, 152(sp)
    sd s4, 160(sp)
    sd s5, 168(sp)
    sd s6, 176(sp)
    sd s7, 184(sp)
    sd s8, 192(sp)
    sd s9, 200(sp)
    sd s10, 208(sp)
    sd s11, 216(sp)
    
    sd gp, 224(sp)
    sd tp, 232(sp)
    
    # 2. Call Rust handler with SP as argument
    mv a0, sp
    call trap_handler
    
    # 3. Restore all registers
    ld ra, 0(sp)
    ld t0, 8(sp)
    ld t1, 16(sp)
    ld t2, 24(sp)
    ld t3, 32(sp)
    ld t4, 40(sp)
    ld t5, 48(sp)
    ld t6, 56(sp)
    
    ld a0, 64(sp)
    ld a1, 72(sp)
    ld a2, 80(sp)
    ld a3, 88(sp)
    ld a4, 96(sp)
    ld a5, 104(sp)
    ld a6, 112(sp)
    ld a7, 120(sp)
    
    ld s0, 128(sp)
    ld s1, 136(sp)
    ld s2, 144(sp)
    ld s3, 152(sp)
    ld s4, 160(sp)
    ld s5, 168(sp)
    ld s6, 176(sp)
    ld s7, 184(sp)
    ld s8, 192(sp)
    ld s9, 200(sp)
    ld s10, 208(sp)
    ld s11, 216(sp)
    
    ld gp, 224(sp)
    ld tp, 232(sp)
    
    addi sp, sp, 256
    
    # 4. Return from Exception/Interrupt
    mret
    "#
);

extern "C" {
    fn _trap_entry();
}

pub unsafe fn init() {
    let trap_addr = _trap_entry as usize;
    #[cfg(target_arch = "riscv64")]
    {
        core::arch::asm!("csrw mtvec, {0}", in(reg) trap_addr);
        let mstatus_mask: usize = 0x8 | 0x2000;
        core::arch::asm!("csrs mstatus, {0}", in(reg) mstatus_mask);
        let mie_mask: usize = 0x80;
        core::arch::asm!("csrw mie, {0}", in(reg) mie_mask);
    }
}

pub unsafe fn enable_interrupts() {
    #[cfg(target_arch = "riscv64")]
    core::arch::asm!("csrsi mstatus, 0x8"); 
}

pub unsafe fn disable_interrupts() {
    #[cfg(target_arch = "riscv64")]
    core::arch::asm!("csrci mstatus, 0x8"); 
}

#[repr(C)]
struct TrapFrame {
    ra: usize, t0: usize, t1: usize, t2: usize, t3: usize, t4: usize, t5: usize, t6: usize,
    a0: usize, a1: usize, a2: usize, a3: usize, a4: usize, a5: usize, a6: usize, a7: usize,
    s0: usize, s1: usize, s2: usize, s3: usize, s4: usize, s5: usize, s6: usize, s7: usize,
    s8: usize, s9: usize, s10: usize, s11: usize, gp: usize, tp: usize,
}

#[no_mangle]
#[allow(dead_code)]
pub unsafe extern "C" fn trap_handler(tf_ptr: *mut TrapFrame) {
    let mcause: usize;
    let mepc: usize;
    #[cfg(target_arch = "riscv64")]
    {
        core::arch::asm!("csrr {0}, mcause", out(reg) mcause);
        core::arch::asm!("csrr {0}, mepc", out(reg) mepc);
    }
    #[cfg(not(target_arch = "riscv64"))]
    { mcause = 0; mepc = 0; }
    
    let cause = TrapCause::from(mcause);
    let tf = &mut *tf_ptr;

    match cause {
        TrapCause::MachineTimer => {
            handle_timer_interrupt();
        }
        TrapCause::Unknown => {
            let is_interrupt = (mcause >> 63) != 0;
            if !is_interrupt {
                if mcause == 8 || mcause == 9 || mcause == 11 {
                     // ADVANCE MEPC BEFORE SYSCALL
                     // This ensures that if the syscall causes a context switch,
                     // the saved context has the incremented PC.
                     #[cfg(target_arch = "riscv64")]
                     core::arch::asm!("csrw mepc, {0}", in(reg) mepc + 4);

                     let ret = crate::process::syscall::handle_software_trap(tf.a0, tf.a1, tf.a2, tf.a3, tf.a4);
                     tf.a0 = ret as usize;
                } else {
                     warn!("--- EXCEPTION 0x{:X} at 0x{:X} ---", mcause, mepc);
                     warn!("mcause: {} (0x{:X}), is_interrupt: {}", mcause, mcause, is_interrupt);
                     warn!("ra: 0x{:X}, sp: 0x{:X}", tf.ra, (tf_ptr as usize) + 256);
                     warn!("a0: 0x{:X}, a1: 0x{:X}, a2: 0x{:X}", tf.a0, tf.a1, tf.a2);
                     warn!("s0: 0x{:X}, s1: 0x{:X}", tf.s0, tf.s1);
                     
                     loop {}
                }
            }
        }
        _ => {}
    }
}

pub unsafe fn handle_timer_interrupt() {
    #[cfg(target_arch = "riscv64")]
    {
        extern crate hal_riscv;
        hal_riscv::timer::set_timer_ms(10);
        
        // Poll Console Input
        crate::process::drivers::console_drv::CONSOLE.lock().poll();

        // Processing
        crate::process::tick();
        crate::process::yield_cpu();
    }
}

pub unsafe fn read_mcause() -> usize {
    let mcause: usize;
    #[cfg(target_arch = "riscv64")]
    core::arch::asm!("csrr {0}, mcause", out(reg) mcause);
    #[cfg(not(target_arch = "riscv64"))]
    { mcause = 0; }
    mcause
}

pub unsafe fn read_mepc() -> usize {
    let mepc: usize;
    #[cfg(target_arch = "riscv64")]
    core::arch::asm!("csrr {0}, mepc", out(reg) mepc);
    #[cfg(not(target_arch = "riscv64"))]
    { mepc = 0; }
    mepc
}

pub unsafe fn read_mstatus() -> usize {
    let mstatus: usize;
    #[cfg(target_arch = "riscv64")]
    core::arch::asm!("csrr {0}, mstatus", out(reg) mstatus);
    #[cfg(not(target_arch = "riscv64"))]
    { mstatus = 0; }
    mstatus
}
