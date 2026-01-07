//! Trap frame structures and S-mode trap handling for ViOS.
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
    pub fn vi_set_sscratch(kernel_stack_top: usize);
}

/// Initialize trap handling by setting stvec
pub fn init() {
    unsafe {
        let trap_entry = __trap_entry as *const () as usize;
        // Set stvec to direct mode (all traps go to __trap_entry)
        core::arch::asm!("csrw stvec, {}", in(reg) trap_entry);
        // Initialize sscratch to 0 (indicates S-mode context)
        core::arch::asm!("csrw sscratch, zero");
    }
}

/// Set sscratch to kernel stack top before switching to userspace
/// Call this before context switch to user mode
pub fn set_kernel_stack(kernel_stack_top: usize) {
    unsafe {
        vi_set_sscratch(kernel_stack_top);
    }
}

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
            5 => {
                // S-mode timer interrupt
                // TODO: Clear timer and handle scheduling
            }
            9 => {
                // S-mode external interrupt
                // TODO: Handle device interrupts
            }
            _ => {
                // Unknown interrupt - log but don't panic
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
            2 => {
                // Illegal instruction
                panic!("ViOS: Illegal instruction at 0x{:X}, stval=0x{:X}", 
                    frame.sepc, frame.stval);
            }
            12 => {
                // Instruction page fault
                panic!("ViOS: Instruction page fault at 0x{:X}, addr=0x{:X}", 
                    frame.sepc, frame.stval);
            }
            13 => {
                // Load page fault
                panic!("ViOS: Load page fault at 0x{:X}, addr=0x{:X}", 
                    frame.sepc, frame.stval);
            }
            15 => {
                // Store page fault
                panic!("ViOS: Store page fault at 0x{:X}, addr=0x{:X}", 
                    frame.sepc, frame.stval);
            }
            _ => {
                panic!("ViOS: Unhandled exception: scause={}, sepc=0x{:X}, stval=0x{:X}", 
                    code, frame.sepc, frame.stval);
            }
        }
    }
}

/// Handle syscall from userspace (Vi prefix per Luật 6)
fn vi_handle_syscall(frame: &mut ViTrapFrame) {
    extern "Rust" {
        fn vios_syscall_dispatch(frame: &mut ViTrapFrame);
    }
    unsafe {
        vios_syscall_dispatch(frame);
    }
}

