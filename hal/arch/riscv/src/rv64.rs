pub mod boot;

// Re-export common modules for convenience or trait impls
pub use crate::common::sbi;
pub use crate::common::timer;
pub use crate::common::uart_ns16550a as uart;

mod asm;
pub mod context;
pub mod domain;
pub mod paging;
pub mod trap;
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

        // Enable S-mode software + external interrupt delivery in SIE so the
        // kernel can receive both SSIP preemption nudges and PLIC-routed device
        // IRQs. Timer-interrupt enable stays on its existing lifecycle elsewhere.
        // SAFETY: this runs during RV64 arch init before normal interrupt
        // handling starts. `csrs sie, {mask}` only sets SSIE|SEIE (bits 1 and 9)
        // from S-mode and leaves STIE untouched, which is the required contract.
        #[cfg(target_arch = "riscv64")]
        unsafe {
            let mask = 0x202usize;
            core::arch::asm!("csrs sie, {mask}", mask = in(reg) mask);
        }
    }

    unsafe fn switch_context(&self, old: *mut Self::Context, new: *const Self::Context) {
        context::Context::switch(old, new);
    }

    fn enable_interrupts(&self) {
        unsafe {
            riscv::register::sstatus::set_sie();
        }
    }

    fn disable_interrupts(&self) {
        unsafe {
            riscv::register::sstatus::clear_sie();
        }
    }

    fn wait_for_interrupt(&self) {
        riscv::asm::wfi();
    }

    fn interrupts_enabled(&self) -> bool {
        riscv::register::sstatus::read().sie()
    }
}
