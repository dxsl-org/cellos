//! SVM vCPU state + host-side run loop (Tier 3b P03).
//!
//! [`SvmVcpu`] owns the guest GPR bank and a [`VmcbView`] over the kernel-owned
//! VMCB frame. [`SvmVcpu::run`] performs the world-switch ([`svm_vmrun`]),
//! services the internally-emulated exits (EFER-SVME preservation, CR0-write,
//! CPUID, physical INTR) in a bounded loop, and returns the first exit that must
//! be surfaced to the hypervisor cell as a HAL [`ViVmExit`].

use hal_hypervisor::ViVmExit;

use super::vmcb::{VmcbView, EFER_SVME, OFF_EXITCODE, OFF_EXITINFO1, OFF_EXITINFO2, OFF_NRIP, OFF_RAX, OFF_RIP};
use super::vmexit_decode::{
    decode, VMEXIT_CPUID, VMEXIT_CR0_WRITE, VMEXIT_HLT, VMEXIT_INTR, VMEXIT_INVALID, VMEXIT_IOIO,
    VMEXIT_MSR, VMEXIT_NPF, VMEXIT_VMMCALL,
};
use super::world_switch::svm_vmrun;

const MSR_EFER: u64 = 0xC000_0080;
/// Cap on internally-emulated exits per `run` call so a guest cannot spin the
/// kernel inside one syscall (mirror the ARM ID-reg resolve cap).
const MAX_INTERNAL_EXITS: u32 = 4096;
/// VMCB state-save offset for guest CR0 (write-back on a trapped CR0 write).
const OFF_CR0: usize = 0x558;
const OFF_EFER: usize = 0x4D0;

/// A single SVM vCPU.
pub struct SvmVcpu {
    /// Guest GPRs, x86 register-number indexed (0=RAX … 15=R15).
    pub gpr: [u64; 16],
    vmcb: VmcbView,
    vmcb_pa: u64,
    host_pa: u64,
}

// SAFETY: single-CPU kernel context in the current TCG bring-up (mirror
// AArch64Vcpu / NestedPageTable). The VmcbView raw pointer is not shared.
unsafe impl Send for SvmVcpu {}

impl SvmVcpu {
    /// Build a vCPU over kernel-allocated frames.
    ///
    /// # Safety
    /// `vmcb_va` must be the live kernel VA of the zeroed VMCB frame whose
    /// physical address is `vmcb_pa`; `host_pa` a distinct host-save frame;
    /// `iopm_pa`/`msrpm_pa` the all-ones permission bitmaps. All frames must
    /// outlive this vCPU (owned by the kernel VM entry).
    #[allow(clippy::too_many_arguments)] // reason: mirrors the flat VMCB frame set the kernel allocates
    pub unsafe fn new(
        vmcb_va: *mut u8,
        vmcb_pa: u64,
        host_pa: u64,
        entry_rip: u64,
        ncr3: u64,
        gdt_gpa: u64,
        iopm_pa: u64,
        msrpm_pa: u64,
    ) -> Self {
        // SAFETY: caller guarantees vmcb_va is a live 4 KiB VMCB frame.
        let mut vmcb = unsafe { VmcbView::new(vmcb_va) };
        vmcb.init(entry_rip, ncr3, gdt_gpa, iopm_pa, msrpm_pa);
        Self {
            gpr: [0; 16],
            vmcb,
            vmcb_pa,
            host_pa,
        }
    }

    /// Reset the guest program counter (for the register-isolation smoke loop).
    pub fn set_rip(&mut self, rip: u64) {
        self.vmcb.w64(OFF_RIP, rip);
    }

    /// World-switch into the guest, handling internal exits, and return the
    /// first surfaced [`ViVmExit`].
    ///
    /// # Safety
    /// SVM root operation must be active and interrupts masked (IF=0) — see
    /// [`svm_vmrun`]'s contract (the VMRUN→VMLOAD `gs:` window).
    pub unsafe fn run(&mut self) -> ViVmExit {
        let mut internal = 0u32;
        loop {
            // Sync caller-managed RAX/RSP into the VMCB before entry.
            self.vmcb.w64(OFF_RAX, self.gpr[0]);
            // (RSP slot 4 stays guest-owned; guest sets its own stack.)

            // SAFETY: root op active + IF=0 (caller contract); VMCB initialised;
            // gpr array lives for the call; frames kernel-owned.
            unsafe {
                svm_vmrun(self.vmcb_pa, self.gpr.as_mut_ptr(), self.host_pa);
            }

            // Recover caller-managed guest RAX from the VMCB save area.
            self.gpr[0] = self.vmcb.r64(OFF_RAX);

            let code = self.vmcb.r64(OFF_EXITCODE);
            let info1 = self.vmcb.r64(OFF_EXITINFO1);
            let info2 = self.vmcb.r64(OFF_EXITINFO2);
            let nrip = self.vmcb.r64(OFF_NRIP);

            if internal >= MAX_INTERNAL_EXITS {
                return ViVmExit::Unknown {
                    ec: code as u32,
                    iss: 0xE17E, // "exit loop" marker — internal-exit cap hit
                };
            }

            match code {
                // EFER WRMSR: force SVME back in, else the next VMRUN → INVALID.
                VMEXIT_MSR if info1 & 1 != 0 && self.gpr[1] as u32 as u64 == MSR_EFER => {
                    let value = ((self.gpr[2] & 0xFFFF_FFFF) << 32) | (self.gpr[0] & 0xFFFF_FFFF);
                    self.vmcb.w64(OFF_EFER, value | EFER_SVME);
                    self.advance(nrip);
                    internal += 1;
                    continue;
                }
                // CR0 write: apply the guest's intended value (decode-assist GPR#)
                // then resume; EFER.LMA is auto-derived by the CPU on SVM.
                VMEXIT_CR0_WRITE => {
                    let gpr_num = (info1 & 0xF) as usize;
                    if gpr_num < 16 {
                        self.vmcb.w64(OFF_CR0, self.gpr[gpr_num]);
                    }
                    self.advance(nrip);
                    internal += 1;
                    continue;
                }
                // CPUID: MVP stub — consume the instruction and resume. (Guest
                // feature probing is refined in P05; the smoke blob issues none.)
                VMEXIT_CPUID => {
                    self.advance(nrip);
                    internal += 1;
                    continue;
                }
                // Physical interrupt: budget/host-IRQ path. MVP surfaces Preempted
                // (the run loop re-enters or yields). Full host-timer budget is the
                // dossier §6 spike, deferred.
                VMEXIT_INTR => return ViVmExit::Preempted,
                // IOIO carries the next-instruction RIP in EXITINFO2 (valid even
                // when NRIPS is absent — e.g. QEMU TCG +svm, where nRIP reads 0).
                VMEXIT_IOIO => {
                    self.vmcb.w64(OFF_RIP, info2);
                    return decode(code, info1, info2, self.gpr[0], self.gpr[1], self.gpr[2]);
                }
                // Other consumed instructions advance via nRIP (requires NRIPS;
                // P05 adds an instruction-length fallback for no-NRIPS hosts).
                VMEXIT_MSR | VMEXIT_HLT | VMEXIT_VMMCALL => {
                    self.advance(nrip);
                    return decode(code, info1, info2, self.gpr[0], self.gpr[1], self.gpr[2]);
                }
                VMEXIT_NPF => {
                    return decode(code, info1, info2, self.gpr[0], self.gpr[1], self.gpr[2]);
                }
                VMEXIT_INVALID => {
                    return ViVmExit::Unknown {
                        ec: 0xFFFF_FFFF,
                        iss: info1 as u32,
                    };
                }
                _ => return decode(code, info1, info2, self.gpr[0], self.gpr[1], self.gpr[2]),
            }
        }
    }

    /// Advance guest RIP to the next-sequential RIP reported in the VMCB.
    #[inline]
    fn advance(&mut self, nrip: u64) {
        if nrip != 0 {
            self.vmcb.w64(OFF_RIP, nrip);
        }
    }
}
