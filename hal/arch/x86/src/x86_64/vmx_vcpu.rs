// SPDX-License-Identifier: MIT
//! Intel VMX vCPU state management and execution loop.
//!
//! Owns the guest general-purpose registers (GPRs) and the VMCS frame.
//! Drives `vmx_vmrun` to execute the guest and decodes VM-exits into HAL [`ViVmExit`].
//! The VMCS is current only inside `vmcs_scope::with_vmcs` (load → use →
//! VMCLEAR), and the host-state area is rewritten from the executing CPU on
//! every run (`host_state`), so the owner task may run the vCPU on any hart.

mod cr_access;
mod host_state;
mod interrupts;
mod lapic;
mod mmio_access;
mod msr_access;
mod run_timer;
mod shared;
mod startup;
mod stats;
mod vmcs_scope;
mod xsave;

pub use lapic::{Ipi, IpiKind, IpiTarget};
pub use shared::VcpuShared;
pub use stats::{ExitClass, VcpuStats};

use hal_hypervisor::ViVmExit;
use types::{ViError, ViResult};

use super::clock;
use super::guest_cpuid;
use super::guest_mem::GuestRam;
use super::pending_irqs::PendingIrqs;
use super::vmcs::*;
use super::vmx;
use super::world_switch::{vmx_vmrun, xgetbv0, xsetbv0, VmxGuestRegs};
use cr_access::CrOutcome;
use msr_access::MsrOutcome;
use vmcs_scope::with_vmcs;

/// What a run needs from outside the vCPU.
pub struct RunContext<'a> {
    /// When the guest is taken off the CPU at the latest, on the
    /// [`clock::now_ns`] base; `None` for no bound (see [`VmxVcpu::run`]).
    pub deadline_ns: Option<u64>,
    /// This vCPU's mailbox. The caller has published the CPU it runs on.
    pub shared: &'a VcpuShared,
    /// vCPUs in the VM, for the guest's view of the processor topology.
    pub vcpus: u8,
    /// Delivers an interrupt this vCPU's local APIC sent to other vCPUs.
    pub send: &'a mut dyn FnMut(Ipi),
}

/// Primary controls the isolation model depends on; refused if the CPU's
/// allowed-1 settings cannot provide them. Unconditional I/O exiting keeps the
/// guest off host ports; CR8 load/store exiting keeps it off the physical TPR.
const PROC_REQUIRED: u32 = PROC_UNCONDITIONAL_IO
    | PROC_CR8_LOAD_EXITING
    | PROC_CR8_STORE_EXITING
    | PROC_ACTIVATE_SECONDARY_CONTROLS;

/// I/O-instruction exit qualification: string instruction (INS/OUTS).
const IO_STRING: u64 = 1 << 4;
/// VM-exit reason bit 31: the exit is a failed VM entry.
const EXIT_REASON_ENTRY_FAILURE: u64 = 1 << 31;
const CR0_PE: u64 = 1 << 0;
const CR0_PG: u64 = 1 << 31;
const CR0_NW: u64 = 1 << 29;
const CR0_CD: u64 = 1 << 30;
/// `IA32_VMX_MISC[6]`: VM entry into the HLT activity state is supported.
const MISC_ACTIVITY_HLT: u64 = 1 << 6;

/// What [`program_pvh`] settled between the VMCS and the CPU's capabilities.
struct Programmed {
    /// The primary processor-based and the VM-entry controls as written.
    proc_ctls: u32,
    entry_ctls: u32,
    run_timer: Option<run_timer::RunTimer>,
    halts_in_guest: bool,
}

/// An Intel VMX virtual CPU.
pub struct VmxVcpu {
    /// Guest GPRs, indexed 0=RAX, 1=RCX, 2=RDX, 3=RBX, 4=RSP, 5=RBP, 6=RSI, 7=RDI, 8..15=R8..R15.
    pub gpr: [u64; 16],
    pub vmcs_pa: u64,
    #[allow(dead_code)]
    vmcs_va: *mut u8,
    /// The guest's local APIC. Its task priority is also the guest's CR8 and
    /// never reaches the physical LAPIC.
    lapic: lapic::Lapic,
    /// An application processor (any vCPU but a VM's first): it starts
    /// waiting for INIT and a start-up IPI, and INIT puts it back there.
    is_ap: bool,
    /// Not started yet: a run returns `Hlt` without entering the guest.
    waiting_for_sipi: bool,
    /// An interrupt the local APIC sent to other vCPUs during the current
    /// exit, for [`RunContext::send`].
    outbox: Option<Ipi>,
    /// The guest sees XSAVE and AVX and has its own XCR0 and AVX state
    /// (`xsave`): the host turned XSAVE on and the CPU has AVX.
    avx: bool,
    /// The guest's XCR0; in the register for the whole of a run when `avx`.
    guest_xcr0: u64,
    /// Guest PKRU / KERNEL_GS_BASE / SYSCALL MSRs carried across exits by the world switch.
    guest_regs: VmxGuestRegs,
    /// Guest RAM, read only to fetch the instruction behind an MMIO exit.
    guest_ram: GuestRam,
    /// What the VMCS host-state area was last written from.
    host_stamp: Option<host_state::Stamp>,
    /// The CPU of the latest entry. With a VPID the guest's translations stay
    /// cached on that CPU, so an entry anywhere else must not trust what an
    /// earlier stay left there.
    last_cpu: Option<usize>,
    /// Ends a run at its deadline; `None` where the CPU has no VMX-preemption
    /// timer, and a run then ends at the first exit after the deadline.
    run_timer: Option<run_timer::RunTimer>,
    /// The primary processor-based controls the VMCS holds (`interrupts`).
    proc_ctls: u32,
    /// The VM-entry controls the VMCS holds (`sync_ia32e_mode`).
    entry_ctls: u32,
    /// A guest `HLT` may stay in the guest during a run that has a deadline:
    /// the control can be cleared, the CPU can re-enter a halted guest, and the
    /// run timer ends the wait. The vCPU then idles without leaving the guest
    /// and wakes at the deadline, at a host interrupt or by an injection.
    halts_in_guest: bool,
    /// External interrupts raised and not yet taken (`interrupts`).
    pending: PendingIrqs,
    /// The host's SYSCALL MSRs on the CPU named by its [`host_state::Stamp`]:
    /// read once per CPU instead of at every run.
    host_syscall: Option<(u64, [u64; 4])>,
    stats: VcpuStats,
}

unsafe impl Send for VmxVcpu {}

impl VmxVcpu {
    /// Create and initialize a new VMX vCPU over a caller-owned 4 KiB VMCS frame.
    ///
    /// # Safety
    /// `vmcs_pa` and `vmcs_va` must name an exclusively-owned, 4 KiB-aligned frame.
    pub unsafe fn new(
        vmcs_pa: u64,
        vmcs_va: *mut u8,
        guest_ram: *const u8,
        guest_ram_len: usize,
    ) -> ViResult<Self> {
        if vmcs_pa == 0 || vmcs_pa & 0xFFF != 0 || vmcs_va.is_null() {
            return Err(ViError::InvalidArgument);
        }

        // 1. Clear the frame
        core::ptr::write_bytes(vmcs_va, 0, 4096);

        // 2. Stamp revision ID in the first 4 bytes
        let rev = vmx::vmxon_revision();
        core::ptr::write_volatile(vmcs_va as *mut u32, rev);

        // 3. Initialise the launch state; the VMCS is loaded only per access.
        vmclear(vmcs_pa)?;

        Ok(Self {
            gpr: [0; 16],
            vmcs_pa,
            vmcs_va,
            lapic: lapic::Lapic::new(0),
            is_ap: false,
            waiting_for_sipi: false,
            outbox: None,
            avx: xsave::host_offers_avx(),
            guest_xcr0: xsave::XCR0_RESET,
            guest_regs: VmxGuestRegs::default(),
            host_stamp: None,
            last_cpu: None,
            run_timer: None,
            proc_ctls: 0,
            entry_ctls: 0,
            halts_in_guest: false,
            pending: PendingIrqs::default(),
            host_syscall: None,
            stats: VcpuStats::default(),
            // SAFETY: the caller's contract covers the guest carve.
            guest_ram: GuestRam::new(guest_ram, guest_ram_len),
        })
    }

    /// Physical address of the underlying VMCS frame.
    #[inline]
    pub fn vmcs_pa(&self) -> u64 {
        self.vmcs_pa
    }

    /// Configure the initial VMCS state for direct PVH boot (32-bit flat
    /// protected mode): a VM's first vCPU, APIC ID 0.
    ///
    /// # Arguments
    /// * `entry_pc` - 32-bit guest entry RIP (`PHYS32_ENTRY`).
    /// * `hvm_info_gpa` - physical address of `hvm_start_info` struct (passed in `%ebx`).
    /// * `eptp` - formatted EPTP control value from [`NestedPageTable::eptp`].
    /// * `vpid` - tag for this vCPU's cached translations where the CPU offers
    ///   VPIDs: nonzero and distinct among the vCPUs that run under `eptp`.
    ///   Translations are tagged by the EPT root as well, so vCPUs of
    ///   different VMs may share a value.
    /// * `host_cr3` - host PML4 root for the initial host-state area; every
    ///   [`Self::run`] replaces it with the live CR3 of the running CPU.
    ///
    /// Fails with [`ViError::NotSupported`] when the CPU cannot provide the
    /// exit controls the isolation model needs (external-interrupt exiting,
    /// unconditional I/O exiting, CR8 load/store exiting, EPT).
    ///
    /// # Safety
    /// The CPU must be in VMX root operation, since this loads this vCPU's VMCS
    /// and writes host state read from the live CR0/CR3/CR4 and segment
    /// registers. `eptp` must name an EPT hierarchy that stays valid while the
    /// guest runs, and `host_cr3` must map the kernel's VM-exit path.
    pub unsafe fn init_pvh(
        &mut self,
        entry_pc: u64,
        hvm_info_gpa: u64,
        eptp: u64,
        vpid: u16,
        host_cr3: u64,
    ) -> ViResult<()> {
        self.program(entry_pc, eptp, vpid, host_cr3)?;
        // PVH ABI: %ebx points to hvm_start_info
        self.gpr[3] = hvm_info_gpa; // index 3 = RBX
        Ok(())
    }

    /// Configure an application processor: APIC ID `apic_id`, waiting for
    /// INIT and a start-up IPI from another vCPU's local APIC. Its guest
    /// state is written when the start-up IPI arrives.
    ///
    /// # Safety
    /// As for [`Self::init_pvh`].
    pub unsafe fn init_ap(
        &mut self,
        eptp: u64,
        vpid: u16,
        apic_id: u8,
        host_cr3: u64,
    ) -> ViResult<()> {
        self.program(0, eptp, vpid, host_cr3)?;
        self.lapic = lapic::Lapic::new(apic_id);
        self.is_ap = true;
        self.waiting_for_sipi = true;
        Ok(())
    }

    /// # Safety
    /// As for [`Self::init_pvh`].
    unsafe fn program(
        &mut self,
        entry_pc: u64,
        eptp: u64,
        vpid: u16,
        host_cr3: u64,
    ) -> ViResult<()> {
        if vpid == 0 {
            return Err(ViError::InvalidArgument);
        }
        let programmed = with_vmcs(self.vmcs_pa, || {
            host_state::refresh(host_cr3, &mut self.host_stamp)?;
            program_pvh(entry_pc, eptp, vpid)
        })?;
        self.proc_ctls = programmed.proc_ctls;
        self.entry_ctls = programmed.entry_ctls;
        self.run_timer = programmed.run_timer;
        self.halts_in_guest = programmed.halts_in_guest;
        Ok(())
    }
}

/// Write the create-once VMCS fields: static host fields, controls, PVH guest
/// state.
///
/// # Safety
/// VMX root operation with the target VMCS current.
unsafe fn program_pvh(entry_pc: u64, eptp: u64, vpid: u16) -> ViResult<Programmed> {
    vmwrite(HOST_IA32_SYSENTER_CS, 0)?;
    vmwrite(HOST_SYSENTER_ESP, 0)?;
    vmwrite(HOST_SYSENTER_EIP, 0)?;
    // ── 2. Control Fields Setup ──────────────────────────────────────────
    let use_true = has_true_ctls();
    let pin_msr = if use_true {
        MSR_IA32_VMX_TRUE_PINBASED_CTLS
    } else {
        MSR_IA32_VMX_PINBASED_CTLS
    };
    let proc_msr = if use_true {
        MSR_IA32_VMX_TRUE_PROCBASED_CTLS
    } else {
        MSR_IA32_VMX_PROCBASED_CTLS
    };
    let exit_msr = if use_true {
        MSR_IA32_VMX_TRUE_EXIT_CTLS
    } else {
        MSR_IA32_VMX_EXIT_CTLS
    };
    let entry_msr = if use_true {
        MSR_IA32_VMX_TRUE_ENTRY_CTLS
    } else {
        MSR_IA32_VMX_ENTRY_CTLS
    };

    let pin_ctls = adjust_vmx_ctrl(PIN_EXTERNAL_INTERRUPT | PIN_VMX_PREEMPTION_TIMER, pin_msr);
    if pin_ctls & PIN_EXTERNAL_INTERRUPT == 0 {
        return Err(ViError::NotSupported);
    }
    vmwrite(PIN_BASED_VM_EXEC_CONTROL, pin_ctls as u64)?;
    let run_timer = run_timer::RunTimer::of(pin_ctls);
    if let Some(timer) = run_timer {
        // Never left at the field's reset value of 0, which would end every
        // entry before the guest ran an instruction.
        timer.arm(None)?;
    }

    let proc_ctls = adjust_vmx_ctrl(PROC_HLT_EXITING | PROC_REQUIRED, proc_msr);
    if proc_ctls & PROC_REQUIRED != PROC_REQUIRED || proc_ctls & PROC_USE_TPR_SHADOW != 0 {
        return Err(ViError::NotSupported);
    }
    // Interrupt-window exiting is how a raised interrupt reaches a guest that
    // cannot take it yet (`interrupts`).
    if adjust_vmx_ctrl(PROC_INTERRUPT_WINDOW, proc_msr) & PROC_INTERRUPT_WINDOW == 0 {
        return Err(ViError::NotSupported);
    }
    vmwrite(CPU_BASED_VM_EXEC_CONTROL, proc_ctls as u64)?;
    let halts_in_guest = adjust_vmx_ctrl(PROC_REQUIRED, proc_msr) & PROC_HLT_EXITING == 0
        && run_timer.is_some()
        && super::msr::read(MSR_IA32_VMX_MISC) & MISC_ACTIVITY_HLT != 0;

    // With a VPID, VM entries and exits stop flushing the guest's and the
    // host's cached translations; the registry invalidates when a vCPU moves
    // to another CPU instead (`VmxVcpu::last_cpu`).
    let sec_ctls = adjust_vmx_ctrl(
        SEC_ENABLE_EPT | SEC_ENABLE_RDTSCP | SEC_UNRESTRICTED_GUEST | SEC_ENABLE_VPID,
        MSR_IA32_VMX_PROCBASED_CTLS2,
    );
    // Unrestricted guest: the PVH entry runs with CR0.PG=0, and CR0.PE/PG are
    // left out of the host-owned mask below.
    const SEC_REQUIRED: u32 = SEC_ENABLE_EPT | SEC_UNRESTRICTED_GUEST;
    if sec_ctls & SEC_REQUIRED != SEC_REQUIRED {
        return Err(ViError::NotSupported);
    }
    vmwrite(SECONDARY_VM_EXEC_CONTROL, sec_ctls as u64)?;
    if sec_ctls & SEC_ENABLE_VPID != 0 {
        vmwrite(VIRTUAL_PROCESSOR_ID, u64::from(vpid))?;
    }

    // No EXIT_ACKNOWLEDGE_INTERRUPT: an external-interrupt exit leaves the
    // vector pending in the LAPIC IRR, so the host IDT takes it (and EOIs it)
    // once IF is re-enabled. Acking on exit would park it in the ISR with no
    // handler and no EOI, masking every lower-priority vector.
    // EFER is switched by the CPU: the guest's on entry, saved on exit, the
    // host's reloaded on exit. Without the exit load, the host would resume
    // with the guest's NXE/SCE; without the entry load, the guest could not
    // set LME (WRMSR is emulated into GUEST_IA32_EFER).
    const EXIT_EFER: u32 = EXIT_SAVE_IA32_EFER | EXIT_LOAD_IA32_EFER;
    let exit_ctls = adjust_vmx_ctrl(EXIT_HOST_ADDR_SPACE_SIZE | EXIT_EFER, exit_msr);
    if exit_ctls & EXIT_EFER != EXIT_EFER {
        return Err(ViError::NotSupported);
    }
    vmwrite(VM_EXIT_CONTROLS, exit_ctls as u64)?;

    // VM entry: 32-bit guest (IA-32e mode guest bit = 0); `sync_ia32e_mode`
    // tracks EFER.LMA after every exit.
    let entry_ctls = adjust_vmx_ctrl(ENTRY_LOAD_IA32_EFER, entry_msr);
    if entry_ctls & ENTRY_LOAD_IA32_EFER == 0 {
        return Err(ViError::NotSupported);
    }
    vmwrite(VM_ENTRY_CONTROLS, entry_ctls as u64)?;

    vmwrite(EPT_POINTER, eptp)?;
    vmwrite(VMCS_LINK_POINTER, !0u64)?;

    // Clear all event injection and MSR load/store counts
    vmwrite(EXCEPTION_BITMAP, 0)?;
    vmwrite(PAGE_FAULT_ERROR_CODE_MASK, 0)?;
    vmwrite(PAGE_FAULT_ERROR_CODE_MATCH, 0)?;
    vmwrite(CR3_TARGET_COUNT, 0)?;
    vmwrite(VM_EXIT_MSR_STORE_COUNT, 0)?;
    vmwrite(VM_EXIT_MSR_LOAD_COUNT, 0)?;
    vmwrite(VM_ENTRY_MSR_LOAD_COUNT, 0)?;
    vmwrite(VM_ENTRY_INTR_INFO_FIELD, 0)?;
    vmwrite(VM_ENTRY_EXCEPTION_ERROR_CODE, 0)?;
    vmwrite(VM_ENTRY_INSTRUCTION_LEN, 0)?;
    vmwrite(TPR_THRESHOLD, 0)?;
    vmwrite(TSC_OFFSET, 0)?;
    vmwrite(GUEST_DR7, 0x400)?; // Bit 10 is reserved (must be 1)
    vmwrite(GUEST_PENDING_DBG_EXCEPTIONS, 0)?;
    vmwrite(GUEST_SYSENTER_CS, 0)?;
    vmwrite(GUEST_SYSENTER_ESP, 0)?;
    vmwrite(GUEST_SYSENTER_EIP, 0)?;
    vmwrite(GUEST_IA32_DEBUGCTL, 0)?;
    vmwrite(GUEST_IA32_PAT, 0x0007_0406_0007_0406)?;
    vmwrite(GUEST_IA32_EFER, 0)?;
    vmwrite(GUEST_IA32_PERF_GLOBAL_CTRL, 0)?;

    vmwrite(HOST_IA32_PAT, 0x0007_0406_0007_0406)?;
    vmwrite(HOST_IA32_PERF_GLOBAL_CTRL, 0)?;
    // ── 3. Guest State Setup (PVH 32-bit Protected Mode) ─────────────────
    // Linux PVH entry: CR0.PE=1, CR0.ET=1, CR0.NE=1 (0x31), CR4=0, EFER=0, paging off
    vmwrite(GUEST_CR0, 0x31)?;
    vmwrite(CR0_READ_SHADOW, 0x31)?;
    // Host-owned CR0 bits: the VMX fixed-1 bits other than PE/PG (relaxed by
    // unrestricted guest), i.e. NE — Linux's PVH entry loads CR0 = PG|PE,
    // which would #GP natively — and CD/NW, which a VM exit does not reload
    // from HOST_CR0: a guest-set CD would leave the host running uncached.
    // The guest's values live in the read shadow (cr_access).
    let cr0_fixed1 = super::msr::read(MSR_IA32_VMX_CR0_FIXED0) & !(CR0_PE | CR0_PG);
    vmwrite(CR0_GUEST_HOST_MASK, cr0_fixed1 | CR0_CD | CR0_NW)?;
    vmwrite(GUEST_CR3, 0)?;
    // The VMX fixed-1 CR4 bits (VMXE) are host-owned: the guest reads its own
    // value from the shadow, so Linux writing a fresh CR4 without VMXE neither
    // #GPs nor clears it (cr_access).
    let cr4_f0 = super::msr::read(MSR_IA32_VMX_CR4_FIXED0);
    vmwrite(GUEST_CR4, cr4_f0)?;
    vmwrite(CR4_GUEST_HOST_MASK, cr4_f0)?;
    vmwrite(CR4_READ_SHADOW, 0)?;
    vmwrite(GUEST_RFLAGS, 0x2)?; // Bit 1 is always 1 in RFLAGS
    vmwrite(GUEST_RIP, entry_pc)?;
    vmwrite(GUEST_RSP, 0)?;
    vmwrite(GUEST_DR7, 0x400)?; // Bit 10 is reserved (must be 1)
    vmwrite(GUEST_PENDING_DBG_EXCEPTIONS, 0)?;
    vmwrite(GUEST_SYSENTER_CS, 0)?;
    vmwrite(GUEST_SYSENTER_ESP, 0)?;
    vmwrite(GUEST_SYSENTER_EIP, 0)?;
    // CS: 32-bit flat code segment
    vmwrite(GUEST_CS_SELECTOR, 0x10)?;
    vmwrite(GUEST_CS_BASE, 0)?;
    vmwrite(GUEST_CS_LIMIT, 0xFFFF_FFFF)?;
    vmwrite(GUEST_CS_AR_BYTES, 0xC09B)?; // Present, Code, Readable, 32-bit (DB=1), 4KB gran (G=1)

    // DS, ES, SS, FS, GS: 32-bit flat data segments
    for &sel in &[
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
        vmwrite(sel.0, 0x18)?;
        vmwrite(sel.1, 0)?;
        vmwrite(sel.2, 0xFFFF_FFFF)?;
        vmwrite(sel.3, 0xC093)?; // Present, Data, Writable, 32-bit (DB=1), 4KB gran (G=1)
    }

    // TR: busy 32-bit TSS (must not be null in VMX)
    vmwrite(GUEST_TR_SELECTOR, 0x20)?;
    vmwrite(GUEST_TR_BASE, 0)?;
    vmwrite(GUEST_TR_LIMIT, 0x67)?; // >= 0x67 required by Intel SDM for 32-bit TSS
    vmwrite(GUEST_TR_AR_BYTES, 0x008B)?; // Present, 32-bit TSS

    // LDTR: unusable
    vmwrite(GUEST_LDTR_SELECTOR, 0)?;
    vmwrite(GUEST_LDTR_BASE, 0)?;
    vmwrite(GUEST_LDTR_LIMIT, 0)?;
    vmwrite(GUEST_LDTR_AR_BYTES, 0x10000)?; // Bit 16 = unusable

    // GDTR & IDTR
    vmwrite(GUEST_GDTR_BASE, 0)?;
    vmwrite(GUEST_GDTR_LIMIT, 0)?;
    vmwrite(GUEST_IDTR_BASE, 0)?;
    vmwrite(GUEST_IDTR_LIMIT, 0)?;

    vmwrite(GUEST_INTERRUPTIBILITY_INFO, 0)?;
    vmwrite(GUEST_ACTIVITY_STATE, 0)?;
    Ok(Programmed {
        proc_ctls,
        entry_ctls,
        run_timer,
        halts_in_guest,
    })
}

/// Move the guest past an instruction the VMM completed for it.
///
/// Completing the instruction ends any STI/MOV-SS shadow it ran in
/// (interruptibility bits 0-1), as the CPU would; NMI/SMI blocking stays.
/// Linux idles with `sti; hlt`: left set after a HLT exit, the shadow keeps
/// every raised interrupt waiting for an interrupt window that the resumed
/// guest only reaches after it has halted again.
///
/// # Safety
/// This vCPU's VMCS is current.
unsafe fn complete_instruction(next_rip: u64) -> ViResult<()> {
    vmwrite(GUEST_RIP, next_rip)?;
    let interruptibility = vmread(GUEST_INTERRUPTIBILITY_INFO)?;
    if interruptibility & 0x3 != 0 {
        vmwrite(GUEST_INTERRUPTIBILITY_INFO, interruptibility & !0x3)?;
    }
    Ok(())
}

impl VmxVcpu {
    /// Run the guest vCPU until an exit requires hypervisor handling.
    ///
    /// Loads the VMCS, refreshes the host-state area from this CPU, runs, and
    /// VMCLEARs before returning, all with IF=0, so successive calls may come
    /// from different CPUs.
    ///
    /// `ctx.deadline_ns`, on the [`clock::now_ns`] base, is when the guest is
    /// taken off the CPU at the latest (`Preempted`): at that time where the
    /// CPU offers the VMX-preemption timer, otherwise at the first exit after
    /// it. The guest always gets to run before the deadline is looked at.
    ///
    /// An application processor that has not been started (INIT, start-up
    /// IPI) returns `Hlt` without entering the guest.
    pub fn run(&mut self, ctx: &mut RunContext) -> ViResult<ViVmExit> {
        let started = clock::now_ns();
        // SAFETY: VMX root is active whenever a VmxVcpu exists; the scope owns
        // VMCS currency and keeps IF=0 across refresh + world switch.
        let exit = unsafe {
            with_vmcs(self.vmcs_pa, || {
                host_state::refresh(host_state::live_cr3(), &mut self.host_stamp)?;
                self.collect_start(ctx.shared)?;
                if self.waiting_for_sipi {
                    return Ok(ViVmExit::Hlt);
                }
                // The guest's KERNEL_GS_BASE and SYSCALL MSRs stay in the
                // registers for every entry of this run.
                let cpu = self.host_stamp.map(|stamp| stamp.cpu());
                let known = self
                    .host_syscall
                    .filter(|(on, _)| Some(*on) == cpu)
                    .map(|(_, msrs)| msrs);
                let host = self.guest_regs.load_msrs(known);
                self.host_syscall = cpu.map(|cpu| (cpu, host.syscall));
                // The guest's XCR0 and x87/SSE/AVX state for every entry of
                // this run: XSETBV traps to an outer hypervisor, so it is not
                // switched around each entry.
                let host_xcr0 = self.avx.then(|| {
                    let host = xgetbv0();
                    if host != self.guest_xcr0 {
                        xsetbv0(self.guest_xcr0);
                    }
                    host
                });
                self.guest_regs.xstate.load(self.avx);
                let exit = self.run_loaded(ctx);
                self.guest_regs.xstate.save(self.avx, self.guest_xcr0);
                if let Some(host) = host_xcr0.filter(|host| *host != self.guest_xcr0) {
                    xsetbv0(host);
                }
                self.guest_regs.unload_msrs(host);
                exit
            })
        };
        let preempted = matches!(exit, Ok(ViVmExit::Preempted));
        self.stats.count_run(started, clock::now_ns(), preempted);
        exit
    }

    /// What this vCPU has cost so far.
    pub fn stats(&self) -> VcpuStats {
        self.stats
    }

    /// The CPU of the latest entry, if any. When it is not the CPU about to
    /// enter, what that CPU cached for the guest during an earlier stay may be
    /// stale (the guest flushes only the CPU it runs on): the caller
    /// invalidates the EPT context there before [`Self::run`].
    pub fn last_cpu(&self) -> Option<usize> {
        self.last_cpu
    }

    /// Record `cpu` as the CPU of the entry about to be made.
    pub fn set_last_cpu(&mut self, cpu: usize) {
        self.last_cpu = Some(cpu);
    }

    /// TSC ticks from `now_tsc` until the entry must end: the earlier of the
    /// run's deadline and the guest's APIC timer. `None` when neither bounds
    /// it (the run's deadline cannot while the TSC's rate is unknown).
    fn ticks_to_leave(&self, deadline_ns: Option<u64>, now_tsc: u64) -> Option<u64> {
        let run = deadline_ns
            .filter(|_| clock::tsc::hz() != 0)
            .map(|deadline| clock::tsc::ns_to_ticks(deadline.saturating_sub(clock::now_ns())));
        let apic = Some(self.lapic.deadline())
            .filter(|deadline| *deadline != 0)
            .map(|deadline| deadline.saturating_sub(now_tsc));
        match (run, apic) {
            (Some(run), Some(apic)) => Some(run.min(apic)),
            (run, apic) => run.or(apic),
        }
    }

    /// # Safety
    /// This vCPU's VMCS is current with a fresh host-state area, IF=0.
    unsafe fn run_loaded(&mut self, ctx: &mut RunContext) -> ViResult<ViVmExit> {
        let deadline_ns = ctx.deadline_ns;
        // The scope VMCLEARed the VMCS since the last run: launch state clear.
        let mut launched = false;
        loop {
            // An exit served here does not extend the run past its deadline,
            // and an INIT or start-up IPI waits for the next run (it replaces
            // the guest state, MSRs included).
            if launched
                && (deadline_ns.is_some_and(|deadline| clock::now_ns() >= deadline)
                    || ctx.shared.start_pending())
            {
                return Ok(ViVmExit::Preempted);
            }
            self.collect_interrupts(ctx.shared);
            // The guest's APIC timer and the caller's deadline share the run
            // timer: whichever is due first ends the entry.
            let now_tsc = clock::tsc::read();
            self.lapic.tick(now_tsc);
            if let Some(timer) = self.run_timer {
                timer.arm(self.ticks_to_leave(deadline_ns, now_tsc))?;
            }
            // With a deadline to end the wait, a halted guest waits in the guest.
            let halt_in_guest = deadline_ns.is_some() && self.halts_in_guest;
            self.set_proc_ctl(PROC_HLT_EXITING, !halt_in_guest)?;
            self.offer_pending()?;
            self.sync_ia32e_mode()?;
            let entered = clock::now_ns();
            let status = vmx_vmrun(self.gpr.as_mut_ptr(), launched as u64, &mut self.guest_regs);
            self.stats.guest_ns += clock::now_ns().saturating_sub(entered);
            if status != 0 {
                return Ok(ViVmExit::Shutdown);
            }
            launched = true;

            let raw_reason = vmread(VM_EXIT_REASON)?;
            if raw_reason & EXIT_REASON_ENTRY_FAILURE != 0 {
                // Invalid guest state / MSR load / #MC during entry: no
                // instruction ran, so RIP stays; the VM stops.
                return Ok(ViVmExit::Unknown {
                    ec: raw_reason as u32,
                    iss: 0,
                });
            }
            let exit_reason = (raw_reason & 0xFFFF) as u32;
            self.stats.count_exit(exit_reason);
            // Only an exit that finishes an instruction needs its length and
            // RIP: two fields the other exits leave unread.
            let advance = || {
                let length = vmread(VM_EXIT_INSTRUCTION_LEN).unwrap_or(0);
                complete_instruction(vmread(GUEST_RIP)?.wrapping_add(length))
            };

            match exit_reason {
                EXIT_REASON_HLT => {
                    advance()?;
                    return Ok(ViVmExit::Hlt);
                }
                // A host interrupt: the host must take it, outside this IF=0
                // scope.
                EXIT_REASON_EXTERNAL_INTERRUPT => return Ok(ViVmExit::Preempted),
                // The run timer fired for the run's deadline or for the
                // guest's APIC timer. The loop head tells them apart: it ends
                // the run at the former and raises the timer's vector at the
                // latter, without a trip to the caller.
                EXIT_REASON_VMX_PREEMPTION_TIMER => continue,
                // The guest can take an interrupt now: the next entry injects.
                EXIT_REASON_INTERRUPT_WINDOW => continue,
                EXIT_REASON_IO_INSTRUCTION => {
                    let qual = vmread(EXIT_QUALIFICATION)?;
                    if qual & IO_STRING != 0 {
                        // INS/OUTS are not modelled (as on SVM): stop loud,
                        // RIP unchanged.
                        return Ok(ViVmExit::Unknown {
                            ec: exit_reason,
                            iss: qual as u32,
                        });
                    }
                    let size = (qual & 0x7) as u8 + 1; // 0=1 byte, 1=2 bytes, 3=4 bytes
                    let is_in = (qual & (1 << 3)) != 0;
                    let port = ((qual >> 16) & 0xFFFF) as u16;
                    advance()?;

                    if is_in {
                        return Ok(ViVmExit::PortIn { port, size });
                    }
                    let mask = match size {
                        1 => 0xFF,
                        2 => 0xFFFF,
                        _ => 0xFFFF_FFFF,
                    };
                    let val = (self.gpr[0] & mask) as u32;
                    return Ok(ViVmExit::PortOut { port, size, val });
                }
                EXIT_REASON_EPT_VIOLATION => match self.handle_ept_violation()? {
                    Some(exit) => return Ok(exit),
                    None => {
                        // A local APIC access: what it sent to other vCPUs
                        // goes out, and they see its logical ID.
                        if let Some(ipi) = self.outbox.take() {
                            (ctx.send)(ipi);
                        }
                        ctx.shared.set_logical_id(self.lapic.logical_id());
                        continue;
                    }
                },
                EXIT_REASON_MSR_READ | EXIT_REASON_MSR_WRITE => {
                    let write = exit_reason == EXIT_REASON_MSR_WRITE;
                    if let MsrOutcome::Done = self.handle_msr(write)? {
                        advance()?;
                    }
                    continue;
                }
                EXIT_REASON_CPUID => {
                    let leaf = self.gpr[0] as u32;
                    let subleaf = self.gpr[1] as u32;
                    let regs = lapic::cpuid(
                        leaf,
                        subleaf,
                        guest_cpuid::query(leaf, subleaf),
                        self.run_timer.is_some(),
                        self.lapic.id(),
                        ctx.vcpus,
                    );
                    let [a, b, c, d] = if self.avx {
                        let osxsave = leaf == 1 && vmread(GUEST_CR4)? & (1 << 18) != 0;
                        xsave::cpuid(leaf, subleaf, regs, self.guest_xcr0, osxsave)
                    } else {
                        regs
                    };
                    self.gpr[0] = u64::from(a); // RAX
                    self.gpr[3] = u64::from(b); // RBX
                    self.gpr[1] = u64::from(c); // RCX
                    self.gpr[2] = u64::from(d); // RDX
                    advance()?;
                    continue;
                }
                EXIT_REASON_CR_ACCESS => {
                    let qual = vmread(EXIT_QUALIFICATION)?;
                    if let CrOutcome::Done = self.handle_cr_access(qual)? {
                        advance()?;
                    }
                    continue;
                }
                EXIT_REASON_XSETBV => {
                    if self.handle_xsetbv()? {
                        advance()?;
                    }
                    continue;
                }
                EXIT_REASON_TRIPLE_FAULT => {
                    return Ok(ViVmExit::Shutdown);
                }
                other => {
                    return Ok(ViVmExit::Unknown {
                        ec: other,
                        iss: vmread(VM_EXIT_INSTRUCTION_LEN).unwrap_or(0) as u32,
                    });
                }
            }
        }
    }

    /// Keep the "IA-32e mode guest" entry control equal to the guest's
    /// EFER.LMA (saved on exit): the CPU sets LMA when the guest enables paging
    /// with LME, and VM entry fails if the two disagree.
    ///
    /// # Safety
    /// This vCPU's VMCS is current.
    unsafe fn sync_ia32e_mode(&mut self) -> ViResult<()> {
        const EFER_LMA: u64 = 1 << 10;
        let wanted = if vmread(GUEST_IA32_EFER)? & EFER_LMA != 0 {
            self.entry_ctls | ENTRY_IA32E_MODE_GUEST
        } else {
            self.entry_ctls & !ENTRY_IA32E_MODE_GUEST
        };
        if wanted != self.entry_ctls {
            vmwrite(VM_ENTRY_CONTROLS, u64::from(wanted))?;
            self.entry_ctls = wanted;
        }
        Ok(())
    }
}

/// Queue #GP(0) for the next entry. Real mode (CR0.PE=0, reachable with
/// unrestricted guest) delivers exceptions without an error code, and
/// injecting one there fails the VM-entry checks.
///
/// # Safety
/// The target VMCS is current.
unsafe fn queue_gp() -> ViResult<()> {
    const GP: u64 = (1 << 31) | (3 << 8) | 13;
    const DELIVER_ERROR_CODE: u64 = 1 << 11;
    if vmread(GUEST_CR0)? & CR0_PE != 0 {
        vmwrite(VM_ENTRY_INTR_INFO_FIELD, GP | DELIVER_ERROR_CODE)?;
        vmwrite(VM_ENTRY_EXCEPTION_ERROR_CODE, 0)
    } else {
        vmwrite(VM_ENTRY_INTR_INFO_FIELD, GP)
    }
}
