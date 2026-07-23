//! SVM `#VMEXIT` EXITCODE → HAL [`ViVmExit`] decoder (Tier 3b P03).
//!
//! Codes from AMD APM Vol.2 Appendix C. Only the exits surfaced to the
//! hypervisor cell are decoded here; the run loop ([`super::svm_vcpu`]) handles
//! the internally-emulated exits (CR0 write, EFER WRMSR, CPUID, physical INTR)
//! before consulting this decoder.

use hal_hypervisor::ViVmExit;

// ── SVM exit codes (APM Appendix C) ──────────────────────────────────────────
pub const VMEXIT_CR0_WRITE: u64 = 0x10;
pub const VMEXIT_INTR: u64 = 0x60;
pub const VMEXIT_CPUID: u64 = 0x72;
pub const VMEXIT_HLT: u64 = 0x78;
pub const VMEXIT_IOIO: u64 = 0x7B;
pub const VMEXIT_MSR: u64 = 0x7C;
pub const VMEXIT_SHUTDOWN: u64 = 0x7F;
pub const VMEXIT_VMMCALL: u64 = 0x81;
pub const VMEXIT_NPF: u64 = 0x400;
pub const VMEXIT_INVALID: u64 = u64::MAX; // -1

// IOIO EXITINFO1 bit layout.
const IOIO_TYPE_IN: u64 = 1 << 0; // 0 = OUT, 1 = IN
const IOIO_STR: u64 = 1 << 2;
const IOIO_SZ8: u64 = 1 << 4;
const IOIO_SZ16: u64 = 1 << 5;
const IOIO_SZ32: u64 = 1 << 6;

// NPF EXITINFO1: bit1 = write, bit32 = fault during guest page-table walk.
const NPF_WRITE: u64 = 1 << 1;
const NPF_IN_PT_WALK: u64 = 1 << 32;

/// Decode a surfaced SVM exit into a HAL [`ViVmExit`].
///
/// `guest_rax` is the guest's RAX at exit (the `OUT` data source and the low
/// half of a WRMSR value); `guest_rcx`/`guest_rdx` supply the MSR index and the
/// high WRMSR half.
pub fn decode(
    code: u64,
    info1: u64,
    info2: u64,
    guest_rax: u64,
    guest_rcx: u64,
    guest_rdx: u64,
) -> ViVmExit {
    match code {
        VMEXIT_IOIO => {
            let port = ((info1 >> 16) & 0xFFFF) as u16;
            let size = ioio_size(info1);
            if info1 & IOIO_STR != 0 {
                // String INS/OUTS not modelled in the MVP (P05 8250 UART is
                // single-byte OUT); surface as Unknown so the run loop fails loud.
                return ViVmExit::Unknown {
                    ec: code as u32,
                    iss: info1 as u32,
                };
            }
            if info1 & IOIO_TYPE_IN != 0 {
                ViVmExit::PortIn { port, size }
            } else {
                let mask = size_mask(size);
                ViVmExit::PortOut {
                    port,
                    size,
                    val: (guest_rax & mask) as u32,
                }
            }
        }
        VMEXIT_MSR => {
            let is_write = info1 & 1 != 0;
            let value = ((guest_rdx & 0xFFFF_FFFF) << 32) | (guest_rax & 0xFFFF_FFFF);
            ViVmExit::Msr {
                index: guest_rcx as u32,
                is_write,
                value,
            }
        }
        VMEXIT_HLT => ViVmExit::Hlt,
        VMEXIT_NPF => {
            // Fault during a *guest* page-table walk → the GPA is a guest-PT
            // address, not an MMIO target (x86 analog of ARM's S1PTW guard).
            if info1 & NPF_IN_PT_WALK != 0 {
                return ViVmExit::Unknown {
                    ec: code as u32,
                    iss: info1 as u32,
                };
            }
            // MMIO size/reg require decode-assist bytes (P05/P06 refine); the MVP
            // surfaces the faulting GPA with size 0 so the cell can log/model it.
            if info1 & NPF_WRITE != 0 {
                ViVmExit::MmioWrite {
                    ipa: info2,
                    size: 0,
                    val: 0,
                }
            } else {
                ViVmExit::MmioRead {
                    ipa: info2,
                    size: 0,
                    reg: 0,
                }
            }
        }
        VMEXIT_SHUTDOWN => ViVmExit::Shutdown,
        _ => ViVmExit::Unknown {
            ec: code as u32,
            iss: info1 as u32,
        },
    }
}

#[inline]
fn ioio_size(info1: u64) -> u8 {
    if info1 & IOIO_SZ8 != 0 {
        1
    } else if info1 & IOIO_SZ16 != 0 {
        2
    } else if info1 & IOIO_SZ32 != 0 {
        4
    } else {
        1
    }
}

#[inline]
fn size_mask(size: u8) -> u64 {
    match size {
        1 => 0xFF,
        2 => 0xFFFF,
        _ => 0xFFFF_FFFF,
    }
}
