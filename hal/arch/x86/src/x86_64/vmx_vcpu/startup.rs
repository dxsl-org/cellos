// SPDX-License-Identifier: MIT
//! Mail from other threads (`VcpuShared`), and how an application processor
//! starts (SDM Vol. 3A 9.4, "Multiple-Processor Initialization"; Vol. 3C
//! 26.3.1, the guest-state checks for an unrestricted guest).
//!
//! An application processor begins waiting for INIT and a start-up IPI. INIT
//! resets its local APIC and puts it back to waiting; a start-up IPI with
//! vector `V` starts a waiting one in real mode at `V << 12` (CS `V << 8`,
//! IP 0). The guest state is written here, so the vCPU never runs before it
//! has been started.

use types::ViResult;

use super::super::pending_irqs::PendingIrqs;
use super::super::vmcs::*;
use super::super::world_switch::VmxGuestRegs;
use super::cr_access::flush_translations;
use super::shared::{vectors, VcpuShared};
use super::{VmxVcpu, CR0_CD, CR0_NW, CR0_PE, CR0_PG};

/// Real-mode access rights: a present, accessed code segment (read/execute)
/// and a present, accessed data segment (read/write); a busy TSS for TR.
const AR_CODE: u64 = 0x9B;
const AR_DATA: u64 = 0x93;
const AR_BUSY_TSS: u64 = 0x8B;
const AR_UNUSABLE: u64 = 1 << 16;
/// CR0 after INIT as the guest reads it: CD, NW and ET.
const CR0_RESET: u64 = 0x6000_0010;
const CR0_ET: u64 = 1 << 4;

impl VmxVcpu {
    /// Interrupts other threads raised: VMM lines into the pending set, other
    /// vCPUs' fixed interrupts into this local APIC.
    pub(super) fn collect_interrupts(&mut self, shared: &VcpuShared) {
        for vector in vectors(shared.take_lines()) {
            self.pending.raise(vector);
        }
        for vector in vectors(shared.take_ipis()) {
            self.lapic.receive(vector);
        }
    }

    /// INIT and start-up IPIs (an application processor's only), then the
    /// interrupts. INIT goes first when both came, as they were sent.
    ///
    /// # Safety
    /// This vCPU's VMCS is current; the guest's MSRs are not loaded.
    pub(super) unsafe fn collect_start(&mut self, shared: &VcpuShared) -> ViResult<()> {
        let (init, start_up) = shared.take_start();
        if self.is_ap {
            if init {
                self.lapic.reset();
                self.pending = PendingIrqs::default();
                self.waiting_for_sipi = true;
            }
            if let Some(vector) = start_up.filter(|_| self.waiting_for_sipi) {
                self.start_at(vector)?;
                self.waiting_for_sipi = false;
            }
        }
        self.collect_interrupts(shared);
        Ok(())
    }

    /// The state after INIT, in real mode at `vector << 12`.
    ///
    /// # Safety
    /// This vCPU's VMCS is current; the guest's MSRs are not loaded.
    unsafe fn start_at(&mut self, vector: u8) -> ViResult<()> {
        let segment = u64::from(vector) << 8;
        vmwrite(GUEST_CS_SELECTOR, segment)?;
        vmwrite(GUEST_CS_BASE, segment << 4)?;
        vmwrite(GUEST_CS_LIMIT, 0xFFFF)?;
        vmwrite(GUEST_CS_AR_BYTES, AR_CODE)?;
        for (selector, base, limit, rights) in [
            (
                GUEST_DS_SELECTOR,
                GUEST_DS_BASE,
                GUEST_DS_LIMIT,
                GUEST_DS_AR_BYTES,
            ),
            (
                GUEST_ES_SELECTOR,
                GUEST_ES_BASE,
                GUEST_ES_LIMIT,
                GUEST_ES_AR_BYTES,
            ),
            (
                GUEST_SS_SELECTOR,
                GUEST_SS_BASE,
                GUEST_SS_LIMIT,
                GUEST_SS_AR_BYTES,
            ),
            (
                GUEST_FS_SELECTOR,
                GUEST_FS_BASE,
                GUEST_FS_LIMIT,
                GUEST_FS_AR_BYTES,
            ),
            (
                GUEST_GS_SELECTOR,
                GUEST_GS_BASE,
                GUEST_GS_LIMIT,
                GUEST_GS_AR_BYTES,
            ),
        ] {
            vmwrite(selector, 0)?;
            vmwrite(base, 0)?;
            vmwrite(limit, 0xFFFF)?;
            vmwrite(rights, AR_DATA)?;
        }
        vmwrite(GUEST_TR_SELECTOR, 0)?;
        vmwrite(GUEST_TR_BASE, 0)?;
        vmwrite(GUEST_TR_LIMIT, 0xFFFF)?;
        vmwrite(GUEST_TR_AR_BYTES, AR_BUSY_TSS)?;
        vmwrite(GUEST_LDTR_SELECTOR, 0)?;
        vmwrite(GUEST_LDTR_BASE, 0)?;
        vmwrite(GUEST_LDTR_LIMIT, 0)?;
        vmwrite(GUEST_LDTR_AR_BYTES, AR_UNUSABLE)?;
        vmwrite(GUEST_GDTR_BASE, 0)?;
        vmwrite(GUEST_GDTR_LIMIT, 0xFFFF)?;
        vmwrite(GUEST_IDTR_BASE, 0)?;
        vmwrite(GUEST_IDTR_LIMIT, 0xFFFF)?;

        // The host keeps the VMX fixed-1 CR0 bits (NE) and CD/NW; the guest
        // reads its reset value from the shadow. Unrestricted guest: PE and
        // PG are the guest's.
        let host_owned = vmread(CR0_GUEST_HOST_MASK)? & !(CR0_CD | CR0_NW);
        vmwrite(GUEST_CR0, (host_owned & !(CR0_PE | CR0_PG)) | CR0_ET)?;
        vmwrite(CR0_READ_SHADOW, CR0_RESET)?;
        vmwrite(GUEST_CR4, vmread(CR4_GUEST_HOST_MASK)?)?;
        vmwrite(CR4_READ_SHADOW, 0)?;
        vmwrite(GUEST_CR3, 0)?;
        vmwrite(GUEST_IA32_EFER, 0)?;
        vmwrite(GUEST_RFLAGS, 0x2)?;
        vmwrite(GUEST_RIP, 0)?;
        vmwrite(GUEST_RSP, 0)?;
        vmwrite(GUEST_DR7, 0x400)?;
        vmwrite(GUEST_SYSENTER_CS, 0)?;
        vmwrite(GUEST_SYSENTER_ESP, 0)?;
        vmwrite(GUEST_SYSENTER_EIP, 0)?;
        vmwrite(GUEST_PENDING_DBG_EXCEPTIONS, 0)?;
        vmwrite(GUEST_INTERRUPTIBILITY_INFO, 0)?;
        vmwrite(GUEST_ACTIVITY_STATE, 0)?;
        vmwrite(VM_ENTRY_INTR_INFO_FIELD, 0)?;
        self.gpr = [0; 16];
        self.guest_regs = VmxGuestRegs::default();
        // Translations cached under this VPID before INIT are not the new
        // guest state's.
        flush_translations()
    }
}
