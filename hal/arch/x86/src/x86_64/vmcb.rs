//! AMD SVM VMCB (Virtual Machine Control Block) layout + setup (Tier 3b P03).
//!
//! One 4 KiB page split into a **Control Area** (0x000–0x3FF) and a
//! **State-Save Area** (0x400–0xFFF). `VMRUN`'s operand (RAX) is the VMCB
//! physical address, which must be 4 KiB-aligned.
//!
//! Field offsets follow AMD APM Vol.2 Appendix B. This type is a **view** over
//! a caller-owned frame — it neither allocates nor frees memory. The kernel
//! (which owns `FRAME_ALLOCATOR`) allocates the VMCB + IOPM/MSRPM frames, hands
//! their virtual/physical addresses here, and frees them on VM teardown. Layout
//! knowledge (offsets, intercept bits, PVH entry state) lives here in the HAL.

/// Base of the control-area exit fields the world-switch reads each exit.
// ── Control-area offsets ─────────────────────────────────────────────────────
const OFF_CR_INTERCEPT: usize = 0x000;
const OFF_INTERCEPT1: usize = 0x00C;
const OFF_INTERCEPT2: usize = 0x010;
const OFF_IOPM_BASE: usize = 0x040;
const OFF_MSRPM_BASE: usize = 0x048;
const OFF_ASID: usize = 0x058;
const OFF_TLB_CONTROL: usize = 0x05C;
const OFF_VINTR: usize = 0x060;
pub const OFF_EXITCODE: usize = 0x070;
pub const OFF_EXITINFO1: usize = 0x078;
pub const OFF_EXITINFO2: usize = 0x080;
pub const OFF_EXITINTINFO: usize = 0x088;
/// Guest interrupt-shadow state (bit 0 set = in a STI/MOV-SS shadow).
pub const OFF_INT_SHADOW: usize = 0x068;
const OFF_NP_ENABLE: usize = 0x090;
pub const OFF_EVENTINJ: usize = 0x0A8;
const OFF_NCR3: usize = 0x0B0;
pub const OFF_NRIP: usize = 0x0C8;

// ── State-save-area offsets ──────────────────────────────────────────────────
const OFF_ES: usize = 0x400;
const OFF_CS: usize = 0x410;
const OFF_SS: usize = 0x420;
const OFF_DS: usize = 0x430;
const OFF_GDTR: usize = 0x460;
const OFF_IDTR: usize = 0x480;
const OFF_CPL: usize = 0x4CB;
const OFF_EFER: usize = 0x4D0;
const OFF_CR4: usize = 0x548;
const OFF_CR3: usize = 0x550;
pub(crate) const OFF_CR0: usize = 0x558;
pub const OFF_RFLAGS: usize = 0x570;
pub const OFF_RIP: usize = 0x578;
const OFF_RSP: usize = 0x5D8;
pub const OFF_RAX: usize = 0x5F8;
const OFF_GPAT: usize = 0x680;

// ── Intercept bits ───────────────────────────────────────────────────────────
const INT1_INTR: u32 = 1 << 0;
const INT1_CPUID: u32 = 1 << 18;
const INT1_PAUSE: u32 = 1 << 23;
const INT1_HLT: u32 = 1 << 24;
const INT1_IOIO: u32 = 1 << 27;
const INT1_MSR: u32 = 1 << 28;
const INT2_VMRUN: u32 = 1 << 0; // MANDATORY or VMRUN → VMEXIT_INVALID

const VINTR_MASKING: u64 = 1 << 24; // physical INTR governed by host IF
const NP_ENABLE_BIT: u64 = 1 << 0;
const TLB_FLUSH_ASID: u8 = 3; // flush this guest's TLB entries on VMRUN (MVP)

/// Guest EFER must keep SVME set or VMRUN fails its consistency check.
pub const EFER_SVME: u64 = 1 << 12;
/// Power-on-default PAT — a zero G_PAT is illegal when NP_ENABLE=1.
const GPAT_DEFAULT: u64 = 0x0007_0406_0007_0406;

// Flat-segment packed attributes (AMD 12-bit form).
const ATTR_CODE32: u16 = 0xC9B; // P,S,code(exec/read),DB=1,G=1,L=0
const ATTR_DATA: u16 = 0xC93; // P,S,data(read/write),DB=1,G=1

/// A view over a caller-owned, 4 KiB-aligned VMCB frame.
pub struct VmcbView {
    base: *mut u8,
}

impl VmcbView {
    /// Wrap the kernel virtual address of a zeroed VMCB frame.
    ///
    /// # Safety
    /// `vmcb_va` must be a live, writable, 4 KiB VMCB frame owned by the caller
    /// for the lifetime of this view.
    pub unsafe fn new(vmcb_va: *mut u8) -> Self {
        Self { base: vmcb_va }
    }

    /// Program all create-once fields for a guest entering at `entry_rip`
    /// (32-bit protected, PVH contract), nested paging rooted at `ncr3`, IOPM /
    /// MSRPM at the given physical addresses (all-ones bitmaps → intercept all),
    /// and a flat GDT at `gdt_gpa` (0 for the M1 smoke blob).
    pub fn init(&mut self, entry_rip: u64, ncr3: u64, gdt_gpa: u64, iopm_pa: u64, msrpm_pa: u64) {
        // Control area. No CR-write intercept: on SVM the guest drives its own
        // paging/long-mode through NPT (CR0.PG trapping is a VMX-only need).
        // CPUID IS intercepted so the run loop can clear the x2APIC feature bit
        // (leaf 1 ECX[21]) — the guest then drives the LAPIC through the
        // RAM-backed 0xFEE00000 xAPIC window instead of x2APIC MSRs. PAUSE is
        // intercepted so guest busy-waits (jiffies calibration loops that spin
        // on cpu_relax and never HLT) still receive paced timer ticks.
        self.w32(OFF_CR_INTERCEPT, 0);
        self.w32(
            OFF_INTERCEPT1,
            INT1_INTR | INT1_CPUID | INT1_PAUSE | INT1_HLT | INT1_IOIO | INT1_MSR,
        );
        self.w32(OFF_INTERCEPT2, INT2_VMRUN);
        self.w64(OFF_IOPM_BASE, iopm_pa);
        self.w64(OFF_MSRPM_BASE, msrpm_pa);
        self.w32(OFF_ASID, 1); // ASID 0 → VMEXIT_INVALID
        self.w8(OFF_TLB_CONTROL, TLB_FLUSH_ASID);
        self.w64(OFF_VINTR, VINTR_MASKING);
        self.w64(OFF_NP_ENABLE, NP_ENABLE_BIT);
        self.w64(OFF_NCR3, ncr3);

        // State-save area (PVH / smoke entry).
        self.set_segment(OFF_CS, 0x08, ATTR_CODE32);
        self.set_segment(OFF_DS, 0x10, ATTR_DATA);
        self.set_segment(OFF_ES, 0x10, ATTR_DATA);
        self.set_segment(OFF_SS, 0x10, ATTR_DATA);
        self.set_dtr(OFF_GDTR, gdt_gpa, 0x17);
        self.set_dtr(OFF_IDTR, 0, 0);
        self.w8(OFF_CPL, 0);
        self.w64(OFF_EFER, EFER_SVME);
        self.w64(OFF_CR4, 0);
        self.w64(OFF_CR3, 0);
        self.w64(OFF_CR0, 0x11); // PE | ET, PG=0
        self.w64(OFF_RFLAGS, 0x2);
        self.w64(OFF_RIP, entry_rip);
        self.w64(OFF_RSP, 0);
        self.w64(OFF_RAX, 0);
        self.w64(OFF_GPAT, GPAT_DEFAULT);
    }

    /// Volatile qword read (exit fields).
    #[inline]
    pub fn r64(&self, off: usize) -> u64 {
        // SAFETY: off is a documented in-page VMCB offset; base is a live frame.
        unsafe { core::ptr::read_volatile(self.base.add(off) as *const u64) }
    }

    /// Volatile qword write (EVENTINJ / RIP advance / EFER re-assert).
    #[inline]
    pub fn w64(&mut self, off: usize, val: u64) {
        // SAFETY: as `r64`; write to the caller-owned frame.
        unsafe { core::ptr::write_volatile(self.base.add(off) as *mut u64, val) }
    }

    #[inline]
    fn w32(&mut self, off: usize, val: u32) {
        // SAFETY: dword-aligned control field in the owned frame.
        unsafe { core::ptr::write_volatile(self.base.add(off) as *mut u32, val) }
    }

    #[inline]
    fn w16(&mut self, off: usize, val: u16) {
        // SAFETY: word-aligned field in the owned frame.
        unsafe { core::ptr::write_volatile(self.base.add(off) as *mut u16, val) }
    }

    #[inline]
    fn w8(&mut self, off: usize, val: u8) {
        // SAFETY: byte field in the owned frame.
        unsafe { core::ptr::write_volatile(self.base.add(off), val) }
    }

    fn set_segment(&mut self, off: usize, selector: u16, attrib: u16) {
        self.w16(off, selector);
        self.w16(off + 2, attrib);
        self.w32(off + 4, 0xFFFF_FFFF); // flat 4 GiB limit (G=1)
        self.w64(off + 8, 0);
    }

    fn set_dtr(&mut self, off: usize, base: u64, limit: u32) {
        self.w32(off + 4, limit);
        self.w64(off + 8, base);
    }
}
