/// Architecture-Specific Context for RISC-V
/// This struct matches the register layout for context switching.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Context {
    pub ra: usize,      // 0
    pub sp: usize,      // 8
    pub s0: usize,      // 16
    pub s1: usize,      // 24
    pub s2: usize,      // 32
    pub s3: usize,      // 40
    pub s4: usize,      // 48
    pub s5: usize,      // 56
    pub s6: usize,      // 64
    pub s7: usize,      // 72
    pub s8: usize,      // 80
    pub s9: usize,      // 88
    pub s10: usize,     // 96
    pub s11: usize,     // 104
    pub mepc: usize,    // 112 - Cần cho context switch trong trap
    pub mstatus: usize, // 120 - Cần cho context switch trong trap
    pub gp: usize,      // 128
    pub tp: usize,      // 136
}

impl Default for Context {
    fn default() -> Self {
        Self {
            ra: 0, sp: 0,
            s0: 0, s1: 0, s2: 0, s3: 0, s4: 0, s5: 0,
            s6: 0, s7: 0, s8: 0, s9: 0, s10: 0, s11: 0,
            mepc: 0, mstatus: 0,
            gp: 0, tp: 0,
        }
    }
}

// Assembly implementation of Context Switch
#[cfg(target_arch = "riscv64")]
core::arch::global_asm!(
    ".section .text",
    ".global __context_switch",
    ".align 4",
    "__context_switch:",
    // Save current context
    "sd ra, 0(a0)",
    "sd sp, 8(a0)",
    "sd s0, 16(a0)",
    "sd s1, 24(a0)",
    "sd s2, 32(a0)",
    "sd s3, 40(a0)",
    "sd s4, 48(a0)",
    "sd s5, 56(a0)",
    "sd s6, 64(a0)",
    "sd s7, 72(a0)",
    "sd s8, 80(a0)",
    "sd s9, 88(a0)",
    "sd s10, 96(a0)",
    "sd s11, 104(a0)",
    "sd gp, 128(a0)",
    "sd tp, 136(a0)",

    // Restore next context
    "ld ra, 0(a1)",
    "ld sp, 8(a1)",
    "ld s0, 16(a1)",
    "ld s1, 24(a1)",
    "ld s2, 32(a1)",
    "ld s3, 40(a1)",
    "ld s4, 48(a1)",
    "ld s5, 56(a1)",
    "ld s6, 64(a1)",
    "ld s7, 72(a1)",
    "ld s8, 80(a1)",
    "ld s9, 88(a1)",
    "ld s10, 96(a1)",
    "ld s11, 104(a1)",
    "ld gp, 128(a1)",
    "ld tp, 136(a1)",
    
    "ret"
);

#[cfg(target_arch = "riscv64")]
extern "C" {
    fn __context_switch(current: *mut Context, next: *const Context);
}

impl Context {
    #[cfg(target_arch = "riscv64")]
    #[inline(never)] // Ensure we actually call the function
    pub unsafe fn switch(current: *mut Context, next: *const Context) {
        __context_switch(current, next);
    }

    #[cfg(not(target_arch = "riscv64"))]
    pub unsafe fn switch(_current: *mut Context, _next: *const Context) {}
}
