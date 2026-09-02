//! x86_64 Interrupt Descriptor Table with one generated entry per vector.

use core::arch::asm;

#[cfg(feature = "x86-idt-cpl3-test")]
mod cpl3_entry;
#[cfg(feature = "x86-idt-cpl3-test")]
mod cpl3_platform;
#[cfg(feature = "x86-idt-cpl3-test")]
mod cpl3_probe;
mod dispatch;
mod entry;
mod fatal;
pub mod policy;
#[cfg(feature = "x86-idt-cpl3-test")]
mod probe;
#[cfg(feature = "x86-idt-cpl3-test")]
mod probe_entry;
#[cfg(feature = "x86-idt-cpl3-test")]
mod probe_timer;

include!(concat!(env!("OUT_DIR"), "/x86_idt_generated.rs"));

unsafe extern "C" {
    static x86_64_idt_stub_table: [usize; X86_IDT_STUB_COUNT];
}

#[repr(C)]
#[derive(Copy, Clone)]
struct IdtEntry {
    off_lo: u16,
    selector: u16,
    ist: u8,
    attributes: u8,
    off_mid: u16,
    off_hi: u32,
    reserved: u32,
}

impl IdtEntry {
    const MISSING: Self = Self {
        off_lo: 0,
        selector: 0,
        ist: 0,
        attributes: 0,
        off_mid: 0,
        off_hi: 0,
        reserved: 0,
    };

    fn interrupt_gate(handler: usize, dpl: u8) -> Self {
        let handler = handler as u64;
        Self {
            off_lo: handler as u16,
            selector: 0x08,
            ist: 0,
            attributes: 0x8e | ((dpl & 3) << 5),
            off_mid: (handler >> 16) as u16,
            off_hi: (handler >> 32) as u32,
            reserved: 0,
        }
    }
}

#[repr(C, align(16))]
struct Idt([IdtEntry; X86_IDT_STUB_COUNT]);

#[repr(C, packed)]
struct IdtPointer {
    limit: u16,
    base: u64,
}

static mut IDT: Idt = Idt([IdtEntry::MISSING; X86_IDT_STUB_COUNT]);

pub fn init() {
    unsafe {
        let idt = core::ptr::addr_of_mut!(IDT);
        let mut previous = 0;
        for (vector, &handler) in x86_64_idt_stub_table.iter().enumerate() {
            debug_assert!(handler != 0 && (vector == 0 || handler > previous));
            previous = handler;
            let dpl = if vector == 0x80 { 3 } else { 0 };
            (*idt).0[vector] = IdtEntry::interrupt_gate(handler, dpl);
        }
        let pointer = IdtPointer {
            limit: (core::mem::size_of::<Idt>() - 1) as u16,
            base: core::ptr::addr_of!((*idt).0) as u64,
        };
        asm!("lidt [{pointer}]", pointer = in(reg) &pointer, options(nostack));
    }

    #[cfg(feature = "x86-idt-cpl3-test")]
    probe::run_exception_probes();
}

#[cfg(feature = "x86-idt-cpl3-test")]
pub fn require_cpl3_pku() {
    cpl3_platform::require_pku();
}

#[cfg(feature = "x86-idt-cpl3-test")]
pub fn cpl3_user_image() -> (&'static [u8], usize, usize, usize) {
    cpl3_platform::user_image()
}

#[cfg(feature = "x86-idt-cpl3-test")]
pub fn arm_cpl3_probe(code_base: usize, b_return_offset: usize) {
    cpl3_probe::arm(code_base, b_return_offset);
}

#[cfg(feature = "x86-idt-cpl3-test")]
pub fn handle_cpl3_probe_syscall(frame: &mut super::trap::ViTrapFrame) -> bool {
    cpl3_probe::handle_syscall(frame)
}

#[cfg(feature = "x86-idt-cpl3-test")]
pub fn cpl3_probe_fail() -> ! {
    probe::fail()
}

#[cfg(feature = "x86-idt-cpl3-test")]
pub const CPL3_PKRU_A: u32 = cpl3_probe::PKRU_A;
#[cfg(feature = "x86-idt-cpl3-test")]
pub const CPL3_PKRU_B: u32 = cpl3_probe::PKRU_B;
