//! SVM vCPU state + host-side run loop (Tier 3b P03).
//!
//! [`SvmVcpu`] owns the guest GPR bank and a [`VmcbView`] over the kernel-owned
//! VMCB frame. [`SvmVcpu::run`] performs the world-switch ([`svm_vmrun`]),
//! services the internally-emulated exits (EFER-SVME preservation, CR0-write,
//! CPUID, physical INTR) in a bounded loop, and returns the first exit that must
//! be surfaced to the hypervisor cell as a HAL [`ViVmExit`].

use hal_hypervisor::ViVmExit;

use super::vmcb::{
    VmcbView, EFER_SVME, OFF_CR0, OFF_CR3, OFF_CR4, OFF_EVENTINJ, OFF_EXITCODE, OFF_EXITINFO1,
    OFF_EXITINFO2, OFF_INT_SHADOW, OFF_NRIP, OFF_RAX, OFF_RFLAGS, OFF_RIP,
};
use super::vmexit_decode::{
    decode, decode_mmio, is_mmio_data_npf, VMEXIT_CPUID, VMEXIT_HLT, VMEXIT_INTR, VMEXIT_INVALID,
    VMEXIT_IOIO, VMEXIT_MSR, VMEXIT_NPF, VMEXIT_PAUSE, VMEXIT_SHUTDOWN, VMEXIT_VMMCALL,
};
use super::world_switch::svm_vmrun;

const MSR_EFER: u32 = 0xC000_0080;
const MSR_APIC_BASE: u32 = 0x1B;
/// APIC_BASE read value: LAPIC @0xFEE00000, BSP, global-enable — **xAPIC** mode
/// (no EXTD bit). x2APIC needs IRQ remapping (VT-d IR) this VMM has no reason to
/// emulate; instead the guest drives the LAPIC through the RAM-backed 0xFEE00000
/// MMIO window (mapped by the kernel), which needs no per-access decode.
const APIC_BASE_VAL: u64 = 0xFEE0_0000 | (1 << 8) | (1 << 11);

/// Cap on internally-emulated exits per `run` call so a guest cannot spin the
/// kernel inside one syscall (mirror the ARM ID-reg resolve cap).
const MAX_INTERNAL_EXITS: u32 = 65536;
const OFF_EFER: usize = 0x4D0;
const OFF_INSN_LEN: usize = 0x0D0;
const OFF_INSN_BYTES: usize = 0x0D1;

// ── RAM-backed xAPIC register offsets polled on HLT/PAUSE ────────────────────
const APIC_SVR: usize = 0x0F0; // spurious-interrupt vector reg (bit8 = enable)
const APIC_LVT_TIMER: usize = 0x320; // bit16 mask; bits[7:0] vector
const APIC_TIMER_INIT: usize = 0x380; // initial count (0 = disarmed)
const SVR_ENABLE: u32 = 1 << 8;
const LVT_MASKED: u32 = 1 << 16;

/// Minimum real (HPET) time between PAUSE-path tick deliveries. 1 ms supports
/// guest HZ up to 1000; a slower guest HZ merely sees time run fast, which is
/// harmless for the jiffy-count wait loops this exists to unblock.
const PAUSE_TICK_NS: u64 = 1_000_000;
/// No-HPET fallback pace: a tick is due every N swallowed PAUSE exits.
const PAUSE_TICK_FALLBACK: u32 = 4096;

/// A single SVM vCPU.
pub struct SvmVcpu {
    /// Guest GPRs, x86 register-number indexed (0=RAX … 15=R15).
    pub gpr: [u64; 16],
    vmcb: VmcbView,
    vmcb_pa: u64,
    host_pa: u64,
    /// Kernel VA of the RAM-backed xAPIC frame (0xFEE00000), or null. Polled on
    /// HLT/PAUSE to deliver the guest LAPIC timer without trapping every MMIO
    /// access.
    apic_va: *mut u8,
    /// Host-visible contiguous guest RAM used only to fetch a faulting instruction.
    guest_ram: *const u8,
    guest_ram_len: usize,
    /// HPET timestamp of the last PAUSE-path tick delivery (pacing state).
    last_pause_tick_ns: u64,
    /// Swallowed-PAUSE counter for the no-HPET fallback pace.
    pause_backlog: u32,
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
    /// `iopm_pa`/`msrpm_pa` the all-ones permission bitmaps. `apic_va` is the
    /// live kernel VA of the RAM-backed xAPIC frame (or null). All frames must
    /// outlive this vCPU (owned by the kernel VM entry).
    #[allow(clippy::too_many_arguments)] // reason: mirrors the flat VMCB frame set the kernel allocates
    pub unsafe fn new(
        vmcb_va: *mut u8,
        vmcb_pa: u64,
        host_pa: u64,
        entry_rip: u64,
        ncr3: u64,
        asid: u32,
        gdt_gpa: u64,
        iopm_pa: u64,
        msrpm_pa: u64,
        apic_va: *mut u8,
        guest_ram: *const u8,
        guest_ram_len: usize,
    ) -> Self {
        // SAFETY: caller guarantees vmcb_va is a live 4 KiB VMCB frame.
        let mut vmcb = unsafe { VmcbView::new(vmcb_va) };
        vmcb.init(entry_rip, ncr3, asid, gdt_gpa, iopm_pa, msrpm_pa);
        Self {
            gpr: [0; 16],
            vmcb,
            vmcb_pa,
            host_pa,
            apic_va,
            guest_ram,
            guest_ram_len,
            last_pause_tick_ns: 0,
            pause_backlog: 0,
        }
    }

    /// True when the guest can accept a maskable external interrupt right now
    /// (RFLAGS.IF set and not in a STI/MOV-SS interrupt shadow). EVENTINJ
    /// delivery bypasses the hardware IF check, so the VMM must enforce it
    /// before injecting on a busy-wait exit (a PAUSE inside an IRQ-off spinlock
    /// section must NOT receive an interrupt).
    fn irq_window_open(&self) -> bool {
        self.vmcb.r64(OFF_RFLAGS) & (1 << 9) != 0 && self.vmcb.r64(OFF_INT_SHADOW) & 1 == 0
    }

    /// Rate-limit PAUSE-path tick delivery to one per [`PAUSE_TICK_NS`] of real
    /// (HPET) time, falling back to an exit-count pace when HPET reads 0.
    fn pause_tick_due(&mut self) -> bool {
        let now = super::hpet::now_ns();
        if now == 0 {
            self.pause_backlog += 1;
            if self.pause_backlog >= PAUSE_TICK_FALLBACK {
                self.pause_backlog = 0;
                return true;
            }
            return false;
        }
        if now.wrapping_sub(self.last_pause_tick_ns) >= PAUSE_TICK_NS {
            self.last_pause_tick_ns = now;
            return true;
        }
        false
    }

    /// Poll the RAM-backed xAPIC timer registers; return the vector to inject if
    /// the timer is armed (LAPIC software-enabled, LVT-timer unmasked, non-zero
    /// initial count), else `None`. The count is not decremented (no timebase in
    /// the poll model) — the guest calibrates against the PIT and, if it rejects
    /// the LAPIC timer, masks it, so this returns `None` and the PIT tick (cell)
    /// carries the boot.
    fn apic_timer_vector(&self) -> Option<u8> {
        if self.apic_va.is_null() {
            return None;
        }
        // SAFETY: apic_va is the live kernel VA of the 4 KiB xAPIC frame; the
        // offsets are within it and 4-byte aligned.
        unsafe {
            let svr = (self.apic_va.add(APIC_SVR) as *const u32).read_volatile();
            let lvt = (self.apic_va.add(APIC_LVT_TIMER) as *const u32).read_volatile();
            let init = (self.apic_va.add(APIC_TIMER_INIT) as *const u32).read_volatile();
            if svr & SVR_ENABLE == 0 || lvt & LVT_MASKED != 0 || init == 0 {
                return None;
            }
            Some((lvt & 0xFF) as u8)
        }
    }

    /// Reset the guest program counter (for the register-isolation smoke loop).
    pub fn set_rip(&mut self, rip: u64) {
        self.vmcb.w64(OFF_RIP, rip);
    }

    /// `(rip, rflags, cr0)` snapshot for triple-fault diagnostics.
    pub fn shutdown_diagnostics(&self) -> (u64, u64, u64) {
        (
            self.vmcb.r64(OFF_RIP),
            self.vmcb.r64(OFF_RFLAGS),
            self.vmcb.r64(OFF_CR0),
        )
    }
    /// `(fault metadata, guest physical address, RIP)` snapshot for NPF diagnostics.
    pub fn npf_diagnostics(&self) -> (u64, u64, u64) {
        (
            self.vmcb.r64(OFF_EXITINFO1),
            self.vmcb.r64(OFF_EXITINFO2),
            self.vmcb.r64(OFF_RIP),
        )
    }

    /// Queue a maskable external interrupt for delivery on the next `VMRUN`.
    ///
    /// Writes the VMCB `EVENTINJ` field (type 0 = external interrupt, valid
    /// bit set). The CPU delivers it on entry and clears the valid bit on the
    /// resulting exit. The caller must only queue this when the guest can take
    /// an IRQ (e.g. on a HLT idle exit, where `RFLAGS.IF=1`), since EVENTINJ
    /// delivery bypasses the guest interrupt-enable check.
    pub fn inject_ext_irq(&mut self, vector: u8) {
        const EVENTINJ_VALID: u64 = 1 << 31; // type field 0 = external interrupt
        self.vmcb.w64(OFF_EVENTINJ, EVENTINJ_VALID | vector as u64);
    }

    /// [`Self::inject_ext_irq`], but dropped when the guest cannot take a
    /// maskable interrupt right now (IF=0 or interrupt shadow). This is the
    /// safe form for the cell-driven tick path: a Preempted exit can interrupt
    /// the guest anywhere — including IRQ-off critical sections and pre-IDT
    /// early boot, where a forced injection would corrupt or triple-fault the
    /// guest. A dropped tick is delivered by a later exit instead.
    pub fn inject_ext_irq_gated(&mut self, vector: u8) -> bool {
        if !self.irq_window_open() {
            return false;
        }
        self.inject_ext_irq(vector);
        true
    }
    fn fault_instruction(&self) -> Option<([u8; 15], usize)> {
        let assisted_len = self.vmcb.r8(OFF_INSN_LEN) as usize;
        if (1..=15).contains(&assisted_len) {
            let mut bytes = [0u8; 15];
            for (index, byte) in bytes[..assisted_len].iter_mut().enumerate() {
                *byte = self.vmcb.r8(OFF_INSN_BYTES + index);
            }
            return Some((bytes, assisted_len));
        }

        let rip = self.vmcb.r64(OFF_RIP);
        let mut bytes = [0u8; 15];
        let mut available = 0;
        for (index, byte) in bytes.iter_mut().enumerate() {
            let Some(gpa) = rip
                .checked_add(index as u64)
                .and_then(|address| self.translate_guest_virtual(address))
            else {
                break;
            };
            let Some(value) = self.read_guest_byte(gpa) else {
                break;
            };
            *byte = value;
            available += 1;
        }
        (available != 0).then_some((bytes, available))
    }

    fn translate_guest_virtual(&self, address: u64) -> Option<u64> {
        if self.vmcb.r64(OFF_CR0) & (1 << 31) == 0 {
            return (address < self.guest_ram_len as u64).then_some(address);
        }
        if self.vmcb.r64(OFF_CR4) & (1 << 12) != 0 {
            return None; // LA57 is outside the pinned x86 guest contract.
        }

        let mut table = self.vmcb.r64(OFF_CR3) & 0x000f_ffff_ffff_f000;
        for (level, shift) in [(4u8, 39u8), (3, 30), (2, 21), (1, 12)] {
            let index = (address >> shift) & 0x1ff;
            let entry = self.read_guest_u64(table.checked_add(index * 8)?)?;
            if entry & 1 == 0 {
                return None;
            }
            if entry & (1 << 7) != 0 {
                return match level {
                    3 => Some((entry & 0x000f_ffff_c000_0000) | (address & ((1 << 30) - 1))),
                    2 => Some((entry & 0x000f_ffff_ffe0_0000) | (address & ((1 << 21) - 1))),
                    _ => None,
                };
            }
            table = entry & 0x000f_ffff_ffff_f000;
        }
        Some(table | (address & 0xfff))
    }

    fn read_guest_byte(&self, gpa: u64) -> Option<u8> {
        let offset: usize = gpa.try_into().ok()?;
        if offset >= self.guest_ram_len {
            return None;
        }
        // SAFETY: constructor contract pins `guest_ram` for this vCPU lifetime;
        // the bounds check keeps the read inside the contiguous guest carve.
        Some(unsafe { core::ptr::read(self.guest_ram.add(offset)) })
    }

    fn read_guest_u64(&self, gpa: u64) -> Option<u64> {
        let mut bytes = [0u8; 8];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = self.read_guest_byte(gpa.checked_add(index as u64)?)?;
        }
        Some(u64::from_le_bytes(bytes))
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
                // MSR access — fully emulated in-kernel so guest boot never
                // round-trips to the cell for the hundreds of MSR ops Linux does.
                // WRMSR EFER re-asserts SVME (else the next VMRUN → INVALID);
                // every other WRMSR is dropped; RDMSR of an intercepted MSR
                // returns 0. Guest-context MSRs (STAR/LSTAR/…/FS_BASE/GS_BASE)
                // are passthrough via the MSRPM allowlist and never reach here.
                // CPUID: execute the real leaf, then clear the x2APIC feature
                // bit (leaf 1 ECX[21]) so the guest stays in xAPIC mode and
                // drives the LAPIC through the RAM-backed 0xFEE00000 MMIO window
                // (x2APIC would need IRQ remapping this VMM does not provide).
                VMEXIT_CPUID => {
                    let leaf = self.gpr[0] as u32;
                    let sub = self.gpr[1] as u32;
                    let r = core::arch::x86_64::__cpuid_count(leaf, sub);
                    let (a, b, mut c, d) = (r.eax, r.ebx, r.ecx, r.edx);
                    if leaf == 1 {
                        c &= !(1 << 21); // clear ECX[21] X2APIC (keep xAPIC, EDX[9])
                    }
                    self.gpr[0] = a as u64; // RAX = eax
                    self.gpr[3] = b as u64; // RBX = ebx
                    self.gpr[1] = c as u64; // RCX = ecx
                    self.gpr[2] = d as u64; // RDX = edx
                    self.advance(code, info1, nrip);
                    internal += 1;
                    continue;
                }
                VMEXIT_MSR => {
                    let is_write = info1 & 1 != 0;
                    let msr = self.gpr[1] as u32; // RCX[31:0]
                    if is_write {
                        if msr == MSR_EFER {
                            let value =
                                ((self.gpr[2] & 0xFFFF_FFFF) << 32) | (self.gpr[0] & 0xFFFF_FFFF);
                            self.vmcb.w64(OFF_EFER, value | EFER_SVME);
                        }
                        // else: drop the write (unmodelled platform MSR). The
                        // guest LAPIC is xAPIC MMIO, so no x2APIC WRMSR arrives.
                    } else if msr == MSR_APIC_BASE {
                        // xAPIC enabled @0xFEE00000 (no EXTD) → guest uses MMIO.
                        self.gpr[0] = APIC_BASE_VAL & 0xFFFF_FFFF;
                        self.gpr[2] = APIC_BASE_VAL >> 32;
                    } else {
                        // RDMSR stub → 0 in EDX:EAX.
                        self.gpr[0] = 0;
                        self.gpr[2] = 0;
                    }
                    self.advance(code, info1, nrip);
                    internal += 1;
                    continue;
                }
                // Physical interrupt: a HOST interrupt fired during VMRUN (e.g.
                // the scheduler tick). Must return so the host services it after
                // sti — but first queue the guest LAPIC timer if armed: this is
                // the preemption channel that reaches even pause-less guest spin
                // loops (EVENTINJ delivers on the next VMRUN from the cell).
                VMEXIT_INTR => {
                    if self.irq_window_open() {
                        if let Some(vec) = self.apic_timer_vector() {
                            self.inject_ext_irq(vec);
                        }
                    }
                    return ViVmExit::Preempted;
                }
                // IOIO carries the next-instruction RIP in EXITINFO2 (valid even
                // when NRIPS is absent — e.g. QEMU TCG +svm, where nRIP reads 0).
                VMEXIT_IOIO => {
                    self.vmcb.w64(OFF_RIP, info2);
                    return decode(code, info1, info2, self.gpr[0], self.gpr[1], self.gpr[2]);
                }
                // HLT: guest idle. Advance past it (nRIP when present, else the
                // fixed length — TCG +svm has no NRIPS so nRIP reads 0). If the
                // guest LAPIC timer is armed, deliver its vector kernel-side and
                // re-enter without bothering the cell; otherwise surface Hlt so
                // the cell injects the 8259 PIT IRQ0 (pre-LAPIC boot tick).
                VMEXIT_HLT => {
                    self.advance(code, info1, nrip);
                    if let Some(vec) = self.apic_timer_vector() {
                        self.inject_ext_irq(vec);
                        internal += 1;
                        continue;
                    }
                    return decode(code, info1, info2, self.gpr[0], self.gpr[1], self.gpr[2]);
                }
                // VMMCALL consumed by the cell: advance past it (3-byte fallback).
                VMEXIT_VMMCALL => {
                    self.advance(code, info1, nrip);
                    return decode(code, info1, info2, self.gpr[0], self.gpr[1], self.gpr[2]);
                }
                // PAUSE — guest busy-wait (cpu_relax). Linux's calibration loops
                // spin on jiffies with PAUSE and never reach HLT, so the tick
                // must also be delivered here: at most once per PAUSE_TICK_NS of
                // real time and only when the guest can take an IRQ. An armed
                // LAPIC timer is injected kernel-side; otherwise the exit
                // surfaces as Hlt so the cell delivers the 8259 PIT tick (the
                // same "idle — deliver a tick" contract as a real HLT exit).
                VMEXIT_PAUSE => {
                    self.advance(code, info1, nrip);
                    internal += 1;
                    if self.irq_window_open() && self.pause_tick_due() {
                        if let Some(vec) = self.apic_timer_vector() {
                            self.inject_ext_irq(vec);
                        } else {
                            return ViVmExit::Hlt;
                        }
                    }
                    continue;
                }
                VMEXIT_NPF => {
                    if !is_mmio_data_npf(info1, info2) {
                        return ViVmExit::Unknown {
                            ec: VMEXIT_NPF as u32,
                            iss: info1 as u32,
                        };
                    }
                    let Some((instruction, available)) = self.fault_instruction() else {
                        return ViVmExit::Unknown {
                            ec: VMEXIT_NPF as u32,
                            iss: 0x4d4d_1001,
                        };
                    };
                    let Some((exit, length)) =
                        decode_mmio(info1, info2, &instruction[..available], &self.gpr)
                    else {
                        return ViVmExit::Unknown {
                            ec: VMEXIT_NPF as u32,
                            iss: 0x4d4d_1002,
                        };
                    };
                    let rip = self.vmcb.r64(OFF_RIP);
                    self.vmcb.w64(OFF_RIP, rip.wrapping_add(length as u64));
                    return exit;
                }
                VMEXIT_SHUTDOWN => {
                    // Triple fault inside the guest: surface immediately so the
                    // registry can snapshot the VMCB for diagnostics.
                    return ViVmExit::Shutdown;
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

    /// Advance guest RIP past a consumed instruction.
    ///
    /// Prefers the VMCB next-RIP (requires the NRIPS feature); when it reads 0
    /// (no-NRIPS host, e.g. QEMU TCG +svm) falls back to a fixed length keyed on
    /// the exit code. Only the exits the run loop consumes need a fallback:
    /// HLT (0xF4, 1B), RDMSR/WRMSR (0F 32 / 0F 30, 2B), PAUSE (F3 90, 2B),
    /// VMMCALL (0F 01 D9, 3B). Linux emits these without prefixes, so the
    /// lengths are exact.
    #[inline]
    fn advance(&mut self, code: u64, _info1: u64, nrip: u64) {
        if nrip != 0 {
            self.vmcb.w64(OFF_RIP, nrip);
            return;
        }
        let len: u64 = match code {
            VMEXIT_HLT => 1,
            VMEXIT_MSR | VMEXIT_CPUID | VMEXIT_PAUSE => 2, // 0F 30/32, 0F A2, F3 90
            VMEXIT_VMMCALL => 3,
            _ => 0,
        };
        let rip = self.vmcb.r64(OFF_RIP);
        self.vmcb.w64(OFF_RIP, rip.wrapping_add(len));
    }
}
