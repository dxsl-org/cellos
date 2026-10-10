//! x86_64 GDT + TSS. Selectors: null=0 kCS=0x08 kDS=0x10 uDS=0x18 uCS=0x20 TSS=0x28.
//!
//! GDT index → selector → RPL3 variant:
//!   [1] code(0)  → 0x08 (kernel CS)
//!   [2] data(0)  → 0x10 (kernel SS/DS)
//!   [3] data(3)  → 0x18, RPL3 = 0x1B (user DS/SS for iretq)
//!   [4] code(3)  → 0x20, RPL3 = 0x23 (user CS for iretq / SYSRET)
//!   [5] tss_low  → 0x28 (TSS — ltr)
use core::arch::asm;

// ── Canonical GDT selector constants ────────────────────────────────────────
/// Kernel code segment selector (CPL 0).
pub const SEL_KERNEL_CODE: u16 = 0x08;
/// Kernel data/stack segment selector (CPL 0).
pub const SEL_KERNEL_DATA: u16 = 0x10;
/// User data/stack segment selector (CPL 3, RPL=3) — used in `iretq` frame as SS.
pub const SEL_USER_DATA: u16 = 0x1B; // 0x18 | RPL3
/// User code segment selector (CPL 3, RPL=3) — used in `iretq` frame as CS.
pub const SEL_USER_CODE: u16 = 0x23; // 0x20 | RPL3
/// TSS selector (loaded via `ltr`).
pub const SEL_TSS: u16 = 0x28;

/// Minimal TSS storing only the kernel-stack pointer (RSP0).
#[repr(C, packed)]
pub struct Tss {
    _r0: u32,
    pub rsp0: u64,
    _rest: [u8; 92],
}
impl Tss {
    pub const fn new() -> Self {
        Self {
            _r0: 0,
            rsp0: 0,
            _rest: [0; 92],
        }
    }
}
impl Default for Tss {
    fn default() -> Self {
        Self::new()
    }
}

#[repr(transparent)]
#[derive(Copy, Clone)]
struct GdtEntry(u64);
impl GdtEntry {
    const NULL: Self = Self(0);
    const fn code(dpl: u8) -> Self {
        Self((1u64 << 43) | (1 << 44) | ((dpl as u64) << 45) | (1 << 47) | (1 << 53))
    }
    const fn data(dpl: u8) -> Self {
        Self((1u64 << 41) | (1 << 44) | ((dpl as u64) << 45) | (1 << 47))
    }
    fn tss_low(base: u64, limit: u32) -> Self {
        let b = ((base & 0xFF) << 16)
            | ((base >> 8 & 0xFF) << 24)
            | ((base >> 16 & 0xFF) << 32)
            | ((base >> 24 & 0xFF) << 56);
        let l = (limit as u64 & 0xFFFF) | ((limit as u64 >> 16) << 48);
        Self(l | b | (0x9u64 << 40) | (1 << 47))
    }
    fn tss_high(base: u64) -> Self {
        Self((base >> 32) & 0xFFFF_FFFF)
    }
}

#[repr(C, align(16))]
struct Gdt {
    entries: [GdtEntry; 8],
}
#[repr(C, packed)]
struct GdtPtr {
    limit: u16,
    base: u64,
}

#[repr(C, align(64))]
struct CpuTables {
    gdt: Gdt,
    tss: Tss,
}
static mut TABLES: [CpuTables; super::syscall::MAX_CPUS] = [
    const { CpuTables { gdt: Gdt { entries: [GdtEntry::NULL; 8] }, tss: Tss::new() } };
    super::syscall::MAX_CPUS
];
const _: () = {
    assert!(core::mem::size_of::<Tss>() == 104);
    assert!(core::mem::offset_of!(Tss, rsp0) == 4);
};

/// Build this CPU's private GDT and TSS after installing kernel GS.
pub fn init() {
    unsafe {
        let tables = core::ptr::addr_of_mut!(TABLES).cast::<CpuTables>()
            .add(super::syscall::current_cpu_id());
        let gdt = core::ptr::addr_of_mut!((*tables).gdt);
        let tss = core::ptr::addr_of_mut!((*tables).tss);
        // I/O bitmap starts beyond the TSS limit, denying Ring-3 port access.
        core::ptr::write_unaligned(tss.cast::<u8>().add(102).cast::<u16>(), 104);
        (*gdt).entries[1] = GdtEntry::code(0);
        (*gdt).entries[2] = GdtEntry::data(0);
        (*gdt).entries[3] = GdtEntry::data(3);
        (*gdt).entries[4] = GdtEntry::code(3);
        let b = tss as u64;
        let l = (core::mem::size_of::<Tss>() - 1) as u32;
        (*gdt).entries[5] = GdtEntry::tss_low(b, l);
        (*gdt).entries[6] = GdtEntry::tss_high(b);
        let ptr = GdtPtr {
            limit: (core::mem::size_of::<Gdt>() - 1) as u16,
            base: gdt as u64,
        };
        asm!(
            // SAFETY: GDT pointer is valid; lgdt + far jmp reload CS; ltr loads TSS.
            "lgdt [{p}]",
            "push 0x08",
            "lea {t}, [rip+99f]",
            "push {t}",
            "retfq",
            "99:",
            "mov ax, 0x10",
            "mov ds, ax", "mov es, ax", "mov ss, ax",
            "mov ax, 0x28", "ltr ax",
            p = in(reg) &ptr, t = lateout(reg) _, // Intel syntax
        );
    }
}
/// Set RSP0 (kernel stack for Ring3->Ring0 transition).
pub fn set_kernel_stack(sp: u64) {
    unsafe {
        let tables = core::ptr::addr_of_mut!(TABLES).cast::<CpuTables>()
            .add(super::syscall::current_cpu_id());
        core::ptr::write_unaligned(core::ptr::addr_of_mut!((*tables).tss.rsp0), sp);
    }
}
