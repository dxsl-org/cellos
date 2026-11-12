// SPDX-License-Identifier: MIT
//! EPT violations on the VirtIO-MMIO window (`EXIT_REASON_EPT_VIOLATION`).
//!
//! VMX reports neither the access width nor the data register for an EPT
//! violation, and `VM_EXIT_INSTRUCTION_LEN` is undefined for it, so the
//! faulting instruction is fetched through the guest page tables and decoded
//! with the same `decode_mmio` the SVM backend uses. Anything else (a fault
//! outside the window, an instruction fetch, a page-walk access, an
//! undecodable instruction) is surfaced as `Unknown` with RIP unchanged.

use hal_hypervisor::ViVmExit;
use types::ViResult;

use super::super::vmcs::*;
use super::super::vmexit_decode::decode_mmio;
use super::VmxVcpu;

const QUAL_WRITE: u64 = 1 << 1;
const QUAL_FETCH: u64 = 1 << 2;
const QUAL_LINEAR_VALID: u64 = 1 << 7;
const QUAL_FINAL_TRANSLATION: u64 = 1 << 8;
/// The pinned VirtIO-MMIO window (same bounds as the SVM NPF predicate).
const MMIO_WINDOW: core::ops::Range<u64> = 0xd000_0000..0xd000_4000;
/// `Unknown.iss` markers, matching the SVM backend's.
const ISS_NO_INSTRUCTION: u32 = 0x4d4d_1001;
const ISS_UNDECODABLE: u32 = 0x4d4d_1002;
const ISS_DURING_EVENT_DELIVERY: u32 = 0x4d4d_1003;
const VECTORING_VALID: u64 = 1 << 31;

/// True for a final-translation data access to the VirtIO-MMIO window.
fn is_mmio_data_access(qual: u64, gpa: u64) -> bool {
    qual & QUAL_FETCH == 0
        && qual & (QUAL_LINEAR_VALID | QUAL_FINAL_TRANSLATION)
            == (QUAL_LINEAR_VALID | QUAL_FINAL_TRANSLATION)
        && MMIO_WINDOW.contains(&gpa)
}

impl VmxVcpu {
    /// Decode the EPT violation at the current exit; on success RIP is moved
    /// past the decoded instruction.
    ///
    /// # Safety
    /// This vCPU's VMCS is current (inside the run scope).
    pub(super) unsafe fn handle_ept_violation(&mut self) -> ViResult<ViVmExit> {
        let qual = vmread(EXIT_QUALIFICATION)?;
        let gpa = vmread(GUEST_PHYSICAL_ADDRESS)?;
        let unknown = |iss| ViVmExit::Unknown {
            ec: EXIT_REASON_EPT_VIOLATION,
            iss,
        };
        if !is_mmio_data_access(qual, gpa) {
            return Ok(unknown(qual as u32));
        }
        // A fault while delivering an event is not the instruction at RIP;
        // decoding it would advance RIP and lose the event.
        if vmread(IDT_VECTORING_INFO_FIELD)? & VECTORING_VALID != 0 {
            return Ok(unknown(ISS_DURING_EVENT_DELIVERY));
        }
        let rip = vmread(GUEST_RIP)?;
        let Some((instruction, available)) = self.guest_ram.fetch_instruction(
            vmread(GUEST_CR0)?,
            vmread(GUEST_CR3)?,
            vmread(GUEST_CR4)?,
            vmread(GUEST_IA32_EFER)?,
            rip,
        ) else {
            return Ok(unknown(ISS_NO_INSTRUCTION));
        };
        // `decode_mmio` reads the write flag from bit 1, where VMX keeps it too.
        let Some((exit, length)) =
            decode_mmio(qual & QUAL_WRITE, gpa, &instruction[..available], &mut self.gpr)
        else {
            return Ok(unknown(ISS_UNDECODABLE));
        };
        super::complete_instruction(rip.wrapping_add(u64::from(length)))?;
        Ok(exit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DATA: u64 = QUAL_LINEAR_VALID | QUAL_FINAL_TRANSLATION;

    #[test]
    fn only_final_data_accesses_inside_the_window_are_mmio() {
        assert!(is_mmio_data_access(DATA, 0xd000_0070));
        assert!(is_mmio_data_access(DATA | QUAL_WRITE, 0xd000_3ffc));
        assert!(!is_mmio_data_access(DATA, 0xd000_4000), "past the window");
        assert!(!is_mmio_data_access(DATA | QUAL_FETCH, 0xd000_0000), "fetch");
        assert!(!is_mmio_data_access(QUAL_LINEAR_VALID, 0xd000_0000), "page walk");
        assert!(!is_mmio_data_access(0, 0xd000_0000), "no linear address");
    }
}
