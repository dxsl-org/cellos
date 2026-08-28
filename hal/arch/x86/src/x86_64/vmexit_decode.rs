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
pub const VMEXIT_PAUSE: u64 = 0x77;
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

// NPF EXITINFO1: bit1 = write, bit32 = final guest translation, bit33 =
// fault during the guest page-table walk.
const NPF_WRITE: u64 = 1 << 1;
const NPF_FINAL_TRANSLATION: u64 = 1 << 32;
const NPF_IN_PT_WALK: u64 = 1 << 33;
const NPF_INSTRUCTION_FETCH: u64 = 1 << 4;

/// True when an NPF is a final guest data access to the pinned VirtIO-MMIO window.
pub(crate) fn is_mmio_data_npf(info1: u64, gpa: u64) -> bool {
    info1 & NPF_FINAL_TRANSLATION != 0
        && info1 & (NPF_IN_PT_WALK | NPF_INSTRUCTION_FETCH) == 0
        && (0xd000_0000..0xd000_4000).contains(&gpa)
}

/// Decode a surfaced SVM exit into a HAL [`ViVmExit`].
///
/// `guest_rax` is the guest's RAX at exit (the `OUT` data source and the low
/// half of a WRMSR value); `guest_rcx`/`guest_rdx` supply the MSR index and the
/// high WRMSR half.
pub fn decode(
    code: u64,
    info1: u64,
    _info2: u64,
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
        VMEXIT_NPF => ViVmExit::Unknown {
            ec: code as u32,
            iss: info1 as u32,
        },
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

/// Decode one x86 MMIO `MOV` instruction captured at an NPF.
///
/// VirtIO MMIO registers are 32-bit. Other operand widths, register-register
/// forms, and unsupported opcodes fail closed instead of advancing guest RIP.
pub fn decode_mmio(
    info1: u64,
    ipa: u64,
    instruction: &[u8],
    gprs: &[u64; 16],
) -> Option<(ViVmExit, u8)> {
    let mut cursor = 0usize;
    let mut rex = 0u8;
    while let Some(&byte) = instruction.get(cursor) {
        match byte {
            0x40..=0x4f => {
                rex = byte;
                cursor += 1;
            }
            0x66 | 0xf2 | 0xf3 => return None,
            _ => break,
        }
    }

    let opcode = *instruction.get(cursor)?;
    cursor += 1;
    let modrm = *instruction.get(cursor)?;
    cursor += 1;
    if modrm >> 6 == 3 {
        return None;
    }
    let reg = ((modrm >> 3) & 7) | ((rex & 4) << 1);
    if reg == 4 {
        return None; // RSP lives in the VMCB, not the caller-managed GPR bank.
    }
    cursor = consume_address(instruction, cursor, modrm, rex)?;
    let is_write_fault = info1 & NPF_WRITE != 0;

    let exit = match opcode {
        0x89 if is_write_fault && rex & 8 == 0 => ViVmExit::MmioWrite {
            ipa,
            size: 4,
            val: gprs[reg as usize] & 0xffff_ffff,
        },
        0x8b if !is_write_fault && rex & 8 == 0 => ViVmExit::MmioRead {
            ipa,
            size: 4,
            reg,
        },
        0xc7 if is_write_fault && rex & 8 == 0 && reg == 0 => {
            let immediate = instruction.get(cursor..cursor.checked_add(4)?)?;
            cursor += 4;
            ViVmExit::MmioWrite {
                ipa,
                size: 4,
                val: u32::from_le_bytes(immediate.try_into().ok()?) as u64,
            }
        }
        _ => return None,
    };
    Some((exit, cursor.try_into().ok()?))
}

fn consume_address(instruction: &[u8], mut cursor: usize, modrm: u8, rex: u8) -> Option<usize> {
    let mode = modrm >> 6;
    let rm = (modrm & 7) | ((rex & 1) << 3);
    if rm & 7 == 4 {
        let sib = *instruction.get(cursor)?;
        cursor += 1;
        if mode == 0 && sib & 7 == 5 {
            cursor = cursor.checked_add(4)?;
        }
    } else if mode == 0 && rm & 7 == 5 {
        cursor = cursor.checked_add(4)?;
    }
    cursor = cursor.checked_add(match mode {
        1 => 1,
        2 => 4,
        _ => 0,
    })?;
    (cursor <= instruction.len()).then_some(cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_register_write_and_read() {
        let mut gprs = [0u64; 16];
        gprs[8] = 0xfeed_beef_dead_cafe;
        assert!(matches!(
            decode_mmio(NPF_WRITE, 0xd000_0000, &[0x44, 0x89, 0x07], &gprs),
            Some((ViVmExit::MmioWrite { size: 4, val: 0xdead_cafe, .. }, 3))
        ));
        assert!(matches!(
            decode_mmio(0, 0xd000_0000, &[0x41, 0x8b, 0x04, 0x24], &gprs),
            Some((ViVmExit::MmioRead { size: 4, reg: 0, .. }, 4))
        ));
    }

    #[test]
    fn decodes_immediate_write_and_rejects_wrong_direction() {
        let gprs = [0u64; 16];
        assert!(matches!(
            decode_mmio(
                NPF_WRITE,
                0xd000_0070,
                &[0xc7, 0x47, 0x70, 0x04, 0x00, 0x00, 0x00],
                &gprs,
            ),
            Some((ViVmExit::MmioWrite { size: 4, val: 4, .. }, 7))
        ));
        assert!(decode_mmio(0, 0xd000_0000, &[0x89, 0x07], &gprs).is_none());
    }

    #[test]
    fn rejects_rsp_operands_outside_the_caller_managed_gpr_bank() {
        let gprs = [0u64; 16];
        assert!(decode_mmio(NPF_WRITE, 0xd000_0000, &[0x89, 0x23], &gprs).is_none());
        assert!(decode_mmio(0, 0xd000_0000, &[0x8b, 0x23], &gprs).is_none());
    }

    #[test]
    fn classifies_only_final_mmio_data_translations() {
        assert!(is_mmio_data_npf(
            NPF_FINAL_TRANSLATION | 4,
            0xd000_0000
        ));
        assert!(!is_mmio_data_npf(
            NPF_FINAL_TRANSLATION | NPF_IN_PT_WALK | 4,
            0xd000_0000
        ));
        assert!(!is_mmio_data_npf(
            NPF_FINAL_TRANSLATION | NPF_INSTRUCTION_FETCH,
            0xd000_0000
        ));
        assert!(!is_mmio_data_npf(0, 0xd000_0000));
        assert!(!is_mmio_data_npf(
            NPF_FINAL_TRANSLATION,
            0xcfff_ffff
        ));
    }
}
