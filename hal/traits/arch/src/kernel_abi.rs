//! Single-source Rust-ABI hooks shared by HAL trap/syscall code and the kernel.
//!
//! These declarations live in one crate so every architecture imports the same
//! function signatures instead of re-declaring `extern "Rust"` blocks by hand.

/// Native-word trap/syscall frame shared by rv64, x86_64, and the ARM64 bridge.
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct ViTrapFrame {
    pub regs: [usize; 32],
    pub sstatus: usize,
    pub sepc: usize,
    pub stval: usize,
    pub scause: usize,
}

impl ViTrapFrame {
    pub const fn new() -> Self {
        Self {
            regs: [0; 32],
            sstatus: 0,
            sepc: 0,
            stval: 0,
            scause: 0,
        }
    }
}

/// RV32 trap/syscall frame with 32-bit register slots.
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct ViTrapFrame32 {
    pub regs: [u32; 32],
    pub sstatus: u32,
    pub sepc: u32,
    pub stval: u32,
    pub scause: u32,
}

impl ViTrapFrame32 {
    pub const fn new() -> Self {
        Self {
            regs: [0; 32],
            sstatus: 0,
            sepc: 0,
            stval: 0,
            scause: 0,
        }
    }
}

const _: () = assert!(core::mem::size_of::<ViTrapFrame>() == 36 * core::mem::size_of::<usize>());
const _: () = assert!(core::mem::size_of::<ViTrapFrame32>() == 144);

pub type TimerTick = extern "Rust" fn();
pub type TerminateOnFault = extern "Rust" fn(cause: usize, pc: usize, fault_addr: usize);
pub type CurrentCellId = extern "Rust" fn() -> usize;
pub type HandleUartIrq = extern "Rust" fn();

#[cfg(not(target_arch = "riscv32"))]
pub type SyscallDispatch = extern "Rust" fn(&mut ViTrapFrame);
#[cfg(target_arch = "riscv32")]
pub type SyscallDispatch = extern "Rust" fn(&mut ViTrapFrame32);

#[cfg(target_arch = "riscv64")]
pub type RiscvPlicContext = extern "Rust" fn() -> usize;
#[cfg(target_arch = "riscv64")]
pub type HandleRiscvExternalIrq = extern "Rust" fn(irq: u32);
#[cfg(target_arch = "riscv64")]
pub type TlbShootdownTestFault = extern "Rust" fn(&mut ViTrapFrame) -> bool;

#[cfg(target_arch = "aarch64")]
pub type TerminateOnFaultAarch64 =
    extern "Rust" fn(cause: usize, pc: usize, fault_addr: usize, spsr: usize, vector_kind: usize);
#[cfg(target_arch = "aarch64")]
pub type HandleVirtioIrq = extern "Rust" fn(irq: u32);
#[cfg(target_arch = "aarch64")]
pub type GpioNotifyIrq = extern "Rust" fn();

#[cfg(target_arch = "x86_64")]
pub type HandlePageFault =
    extern "Rust" fn(va: usize, error_code: u64, rip: u64, cs: u64, rsp: u64);

extern "Rust" {
    pub fn vi_timer_tick();
    pub fn vi_terminate_on_fault(cause: usize, pc: usize, fault_addr: usize);
    pub fn vi_current_cell_id() -> usize;
    pub fn vi_handle_uart_irq();
}

#[cfg(not(target_arch = "riscv32"))]
unsafe extern "Rust" {
    pub safe fn ViCell_syscall_dispatch(frame: &mut ViTrapFrame);
}

#[cfg(target_arch = "riscv32")]
unsafe extern "Rust" {
    pub safe fn ViCell_syscall_dispatch(frame: &mut ViTrapFrame32);
}

#[cfg(target_arch = "riscv64")]
extern "Rust" {
    pub fn vi_riscv_plic_context() -> usize;
    pub fn vi_handle_riscv_external_irq(irq: u32);
    pub fn vi_tlb_shootdown_test_fault(frame: &mut ViTrapFrame) -> bool;
}

#[cfg(target_arch = "aarch64")]
extern "Rust" {
    pub fn vi_terminate_on_fault_aarch64(
        cause: usize,
        pc: usize,
        fault_addr: usize,
        spsr: usize,
        vector_kind: usize,
    );
    pub fn vi_handle_virtio_irq(irq: u32);
    pub fn vi_gpio_notify_irq();
}

#[cfg(target_arch = "x86_64")]
extern "Rust" {
    pub fn vi_handle_page_fault(va: usize, error_code: u64, rip: u64, cs: u64, rsp: u64);
}
