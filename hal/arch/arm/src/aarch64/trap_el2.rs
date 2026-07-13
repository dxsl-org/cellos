//! ESR_EL2 decoder: maps raw trap-syndrome registers into `ViVmExit`.
//!
//! Called from `AArch64Vcpu::decode_exit` after a guest trap returns
//! to the `run_vcpu_impl` host-side.  All inputs come directly from the
//! CPU registers saved by the `vt_vcpu_trap` assembly trampoline.
//!
//! # Red-Team guards
//! - **m1:** S1PTW=1 data-abort → `Unknown` immediately (not MMIO dispatch).
//! - **m2:** ISV=0 data-abort → `Unknown` (no instruction syndrome to decode).
//! - Advances ELR_EL2 by 4 is the caller's responsibility (see `run_vcpu_impl`).

use hal_hypervisor::ViVmExit;

/// Decode an ARM64 Stage-2 VM exit.
///
/// # Parameters
/// - `esr`   — ESR_EL2 snapshot (exception syndrome register).
/// - `elr`   — ELR_EL2 snapshot (guest PC at trap time).
/// - `far`   — FAR_EL2 snapshot (faulting virtual address).
/// - `hpfar` — HPFAR_EL2 snapshot: bits[47:4] = IPA[47:12] of the fault.
/// - `gp`    — guest GP register bank x0-x30 (for `MmioWrite` source value).
///
/// # IPA reconstruction
/// `ipa = (hpfar << 8) | (far & 0xFFF)`
/// because HPFAR_EL2[47:4] = IPA[47:12], so shift-left 8 places the page frame
/// at its correct position, and FAR[11:0] provides the in-page byte offset.
pub fn decode_vmexit(
    esr: u64,
    _elr: u64,
    far: u64,
    hpfar: u64,
    gp: &[u64; 31],
) -> ViVmExit {
    let ec  = ((esr >> 26) & 0x3F) as u32;
    let iss = esr & 0x01FF_FFFF;

    match ec {
        // EC 0x24 — Data Abort from lower EL (guest EL1 or EL0).
        0x24 => {
            // m1 guard: stage-1 page-table walk fault — do NOT dispatch as MMIO.
            let s1ptw = (iss >> 7) & 1;
            if s1ptw != 0 {
                return ViVmExit::Unknown { ec, iss: iss as u32 };
            }
            // m2 guard: ISV=0 means no instruction syndrome — size/reg unknown.
            let isv = (iss >> 24) & 1;
            if isv == 0 {
                return ViVmExit::Unknown { ec, iss: iss as u32 };
            }
            let wnr  = ((iss >> 6) & 1) != 0;
            let sas  = (iss >> 22) & 0x3;          // 0=byte, 1=halfword, 2=word, 3=doubleword
            let srt  = ((iss >> 16) & 0x1F) as u8; // source/target register index
            let size = 1u8 << sas;

            // IPA = HPFAR_EL2[47:4] << 12  |  FAR_EL2[11:0]
            //     = hpfar << 8  |  far & 0xFFF  (bits[3:0] of HPFAR are always 0)
            let ipa = (hpfar << 8) | (far & 0xFFF);

            if wnr {
                // Store: read the source register value from guest GP bank.
                // XZR (index 31) always reads as zero.
                let val = if srt < 31 { gp[srt as usize] } else { 0 };
                ViVmExit::MmioWrite { ipa, size, val }
            } else {
                ViVmExit::MmioRead { ipa, size, reg: srt }
            }
        }

        // EC 0x16 — HVC instruction executed at EL1.
        0x16 => {
            let imm = (iss & 0xFFFF) as u16;
            let mut regs = [0u64; 8];
            // Copy guest x0-x7 as the SMCCC/PSCI argument registers.
            regs.copy_from_slice(&gp[..8]);
            ViVmExit::Hvc { imm, regs }
        }

        // EC 0x01 — WFI or WFE instruction (guest yielding).
        0x01 => ViVmExit::Wfi,

        // EC 0x18 — MSR/MRS/SYS trapped at EL2.
        0x18 => ViVmExit::Unknown { ec, iss: iss as u32 },

        // Anything else is unhandled.
        _ => ViVmExit::Unknown { ec, iss: iss as u32 },
    }
}
