pub mod boot;

// Re-export common modules for convenience or trait impls
// Re-export common modules for convenience or trait impls
pub use crate::common::sbi;
pub use crate::common::timer;
pub use crate::common::uart_ns16550a as uart;

pub mod context;
pub mod trap;
mod asm; 
pub mod paging;
pub use paging::*; 

pub mod arch {
    pub use crate::rv64::context::*;
    pub use crate::rv64::trap::*;
    
    extern "C" {
        pub fn thread_trampoline();
    }
}

pub use hal_arch_trait::*;

pub use types::*;

/// RISC-V architecture implementation.
pub struct RiscVArch;

pub type PlatformArch = RiscVArch;

pub static ARCH: PlatformArch = RiscVArch;

impl Arch for RiscVArch {
    type Context = context::Context;

    fn init(&self) {
        // Initialize trap handling (set stvec)
        trap::init();
    }
    
    unsafe fn switch_context(&self, old: *mut Self::Context, new: *const Self::Context) {
        context::Context::switch(old, new);
    }
    
    fn enable_interrupts(&self) {
        unsafe { riscv::register::sstatus::set_sie(); }
    }
    
    fn disable_interrupts(&self) {
        unsafe { riscv::register::sstatus::clear_sie(); }
    }
    
    fn wait_for_interrupt(&self) {
        unsafe { riscv::asm::wfi(); }
    }

    fn interrupts_enabled(&self) -> bool {
        riscv::register::sstatus::read().sie()
    }
}
