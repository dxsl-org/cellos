//! Single-source Rust-ABI hooks shared by HAL trap/syscall code and the kernel.
//!
//! These declarations live in one crate so every architecture imports the same
//! function signatures instead of re-declaring `extern "Rust"` blocks by hand.

/// Kernel syscall-dispatch frame shared by RV64, x86_64, and the AArch64 bridge.
///
/// This is not the architecture's hardware exception frame. Architecture HALs
/// translate their saved state into this stable dispatcher layout when needed.
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

/// Timer interrupt callback supplied by the kernel scheduler.
pub type TimerTick = unsafe extern "Rust" fn();
/// Trap-proven U-mode Cell-fault callback supplied by the kernel task layer.
///
/// The caller must establish the interrupted context was U-mode before
/// invoking this symbol; Cell accounting attribution alone is insufficient.
pub type TerminateOnUserTrapFault =
    unsafe extern "Rust" fn(cause: usize, pc: usize, fault_addr: usize);
/// Current-cell lookup supplied by the kernel scheduler.
pub type CurrentCellId = unsafe extern "Rust" fn() -> usize;
/// UART interrupt callback supplied by the kernel driver layer.
pub type HandleUartIrq = unsafe extern "Rust" fn();

#[cfg(not(target_arch = "riscv32"))]
/// Native-word syscall dispatcher supplied by the kernel.
pub type SyscallDispatch = extern "Rust" fn(&mut ViTrapFrame);
#[cfg(target_arch = "riscv32")]
/// RV32 syscall dispatcher supplied by the kernel.
pub type SyscallDispatch = extern "Rust" fn(&mut ViTrapFrame32);

#[cfg(target_arch = "riscv64")]
/// RV64 PLIC-context lookup supplied by the kernel platform layer.
pub type RiscvPlicContext = unsafe extern "Rust" fn() -> usize;
#[cfg(target_arch = "riscv64")]
/// RV64 external-interrupt callback supplied by the kernel driver layer.
pub type HandleRiscvExternalIrq = unsafe extern "Rust" fn(irq: u32);
#[cfg(target_arch = "riscv64")]
/// Test-hook callback that consumes an expected RV64 shootdown fault.
pub type TlbShootdownTestFault = unsafe extern "Rust" fn(&mut ViTrapFrame) -> bool;
#[cfg(target_arch = "riscv64")]
/// Recoverable user-copy guard-fault hook supplied by the kernel task layer.
///
/// Returns `true` when the faulting access belonged to an armed copy guard on
/// this hart and the saved PC was rewound to the copy helper's recoverable
/// error path. All other faults must stay on their existing fatal paths.
pub type UserCopyGuardFault = unsafe extern "Rust" fn(frame: &mut ViTrapFrame) -> bool;

#[cfg(target_arch = "aarch64")]
/// AArch64 cell-fault callback supplied by the kernel task layer.
pub type TerminateOnFaultAarch64 = unsafe extern "Rust" fn(
    cause: usize,
    pc: usize,
    fault_addr: usize,
    spsr: usize,
    vector_kind: usize,
);
#[cfg(target_arch = "aarch64")]
/// AArch64 VirtIO interrupt callback supplied by the kernel driver layer.
pub type HandleVirtioIrq = unsafe extern "Rust" fn(irq: u32);
#[cfg(target_arch = "aarch64")]
/// AArch64 GPIO interrupt callback supplied by the kernel driver layer.
pub type GpioNotifyIrq = unsafe extern "Rust" fn();

#[cfg(target_arch = "x86_64")]
/// x86_64 page-fault callback supplied by the kernel memory layer.
pub type HandlePageFault =
    unsafe extern "Rust" fn(va: usize, error_code: u64, rip: u64, cs: u64, rsp: u64);

extern "Rust" {
    pub fn vi_timer_tick();
    pub fn vi_terminate_on_user_trap_fault(cause: usize, pc: usize, fault_addr: usize);
    pub fn vi_current_cell_id() -> usize;
    pub fn vi_handle_uart_irq();
}

const _: TimerTick = vi_timer_tick;
const _: TerminateOnUserTrapFault = vi_terminate_on_user_trap_fault;
const _: CurrentCellId = vi_current_cell_id;
const _: HandleUartIrq = vi_handle_uart_irq;

#[cfg(not(target_arch = "riscv32"))]
unsafe extern "Rust" {
    pub safe fn ViCell_syscall_dispatch(frame: &mut ViTrapFrame);
}

#[cfg(target_arch = "riscv32")]
unsafe extern "Rust" {
    pub safe fn ViCell_syscall_dispatch(frame: &mut ViTrapFrame32);
}

const _: SyscallDispatch = ViCell_syscall_dispatch;

#[cfg(target_arch = "riscv64")]
extern "Rust" {
    pub fn vi_riscv_plic_context() -> usize;
    pub fn vi_handle_riscv_external_irq(irq: u32);
    pub fn vi_tlb_shootdown_test_fault(frame: &mut ViTrapFrame) -> bool;
    pub fn vi_user_copy_guard_fault(frame: &mut ViTrapFrame) -> bool;
}

#[cfg(target_arch = "riscv64")]
const _: RiscvPlicContext = vi_riscv_plic_context;
#[cfg(target_arch = "riscv64")]
const _: HandleRiscvExternalIrq = vi_handle_riscv_external_irq;
#[cfg(target_arch = "riscv64")]
const _: TlbShootdownTestFault = vi_tlb_shootdown_test_fault;
#[cfg(target_arch = "riscv64")]
const _: UserCopyGuardFault = vi_user_copy_guard_fault;
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

#[cfg(target_arch = "aarch64")]
const _: TerminateOnFaultAarch64 = vi_terminate_on_fault_aarch64;
#[cfg(target_arch = "aarch64")]
const _: HandleVirtioIrq = vi_handle_virtio_irq;
#[cfg(target_arch = "aarch64")]
const _: GpioNotifyIrq = vi_gpio_notify_irq;

#[cfg(target_arch = "x86_64")]
extern "Rust" {
    pub fn vi_handle_page_fault(va: usize, error_code: u64, rip: u64, cs: u64, rsp: u64);
}

#[cfg(target_arch = "x86_64")]
unsafe extern "Rust" {
    pub safe fn vi_x86_idt_cpl3_park_b();
    pub safe fn vi_x86_idt_cpl3_wake_b();
    pub safe fn vi_x86_idt_cpl3_switch_to_a();
}

#[cfg(target_arch = "x86_64")]
const _: HandlePageFault = vi_handle_page_fault;
