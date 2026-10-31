//! IDT-fixture probes for IST delivery and the opt-in SMEP/SMAP/UMIP profile.
//!
//! CPL0 phase (paging and LAPIC live, before any Ring-3 task):
//! - TSS IST1-3 and the NMI/#DF/#MC gates point at this CPU's own stacks and
//!   every guard page is unmapped (a guard read takes a not-present #PF),
//!   including the one below the BSP boot stack;
//! - `int $2` / `int $18` with a non-canonical RSP are delivered on IST1/IST3
//!   with the bad RSP saved in the frame (the SYSCALL/SYSRET window hazard);
//! - `int3` with a non-canonical RSP cannot be delivered, cascades into a real
//!   #DF, and that #DF runs on IST2 instead of triple-faulting;
//! - a real LAPIC NMI to this CPU arrives on IST1.
//! `int $18` is gate routing only, not machine-check evidence.
//!
//! Kernel phase inside the Ring-3 probe syscall: SMAP denies a supervisor read
//! of a U=1 page outside `user_access` and allows it inside; SMEP denies a
//! supervisor fetch from a U=1 page. CPL3 phase: UMIP turns `sgdt` into #GP.

use super::super::ist::{self, IST_DF, IST_MC, IST_NMI};
use super::super::protect;
use super::entry::EntryFrame;
use core::sync::atomic::{AtomicU64, AtomicU8, Ordering};

const IDLE: u8 = 0;
const ARMED_INT2: u8 = 1;
const ARMED_INT18: u8 = 2;
const ARMED_DF: u8 = 3;
const ARMED_NMI: u8 = 4;
const ARMED_READ_FAULT: u8 = 5;
const ARMED_EXEC_FAULT: u8 = 6;
const SAW: u8 = 7;

const BAD_RSP: u64 = 0x8000_0000_0000_0000;
const READ_FAULTED: u64 = 0xfa17_fa17_fa17_fa17;
const DF: u64 = 1 << 10;
const AC: u64 = 1 << 18;
const PF_PRESENT: u64 = 1;
const PF_FETCH: u64 = 1 << 4;

static STATE: AtomicU8 = AtomicU8::new(IDLE);
static FAULT_ADDR: AtomicU64 = AtomicU64::new(0);
static FAULT_ERROR: AtomicU64 = AtomicU64::new(0);
static UMIP_RIP: AtomicU64 = AtomicU64::new(0);
static UMIP_NEXT: AtomicU64 = AtomicU64::new(0);
static UMIP_TRAPS: AtomicU8 = AtomicU8::new(0);

#[no_mangle]
static mut X86_IST_SAVED_RSP: u64 = 0;

unsafe extern "C" {
    fn x86_ist_probe_int2();
    fn x86_ist_probe_int18();
    fn x86_ist_probe_df();
    fn x86_ist_probe_read(addr: u64) -> u64;
    fn x86_ist_probe_call(target: u64);
    static x86_ist_int2_resume: u8;
    static x86_ist_int18_resume: u8;
    static x86_ist_df_resume: u8;
    static x86_ist_read_fault: u8;
    static x86_ist_read_resume: u8;
    static x86_ist_call_resume: u8;
    static x86_idt_cpl3_user_start: u8;
    static x86_idt_cpl3_user_umip: u8;
    static x86_idt_cpl3_user_umip_end: u8;
}

fn fail() -> ! {
    super::probe::fail()
}

fn addr(symbol: &u8) -> u64 {
    symbol as *const u8 as u64
}

fn live_rflags() -> u64 {
    let flags: u64;
    unsafe { core::arch::asm!("pushfq", "pop {}", out(reg) flags, options(preserves_flags)) };
    flags
}

fn saved_rsp() -> u64 {
    unsafe { core::ptr::read_volatile(core::ptr::addr_of!(X86_IST_SAVED_RSP)) }
}

fn cr2() -> u64 {
    let value: u64;
    unsafe { core::arch::asm!("mov {}, cr2", out(reg) value, options(nomem, nostack)) };
    value
}

fn on_ist(frame: &EntryFrame, slot: u8) -> bool {
    ist::contains(super::super::syscall::current_cpu_id(), slot, frame as *const _ as u64)
}

fn arm(state: u8) {
    if STATE.compare_exchange(IDLE, state, Ordering::AcqRel, Ordering::Acquire).is_err() {
        fail();
    }
}

fn expect_seen() {
    if STATE.compare_exchange(SAW, IDLE, Ordering::AcqRel, Ordering::Acquire).is_err() {
        fail();
    }
}

/// Software INT through an IST gate with a non-canonical RSP. Resume at the
/// next instruction on the probe's own stack.
fn soft_ist(frame: &mut EntryFrame, vector: u64, slot: u8, resume: u64) -> bool {
    if frame.vector != vector
        || frame.cs != 0x08
        || frame.rip != resume
        || frame.old_rsp() != BAD_RSP
        || !on_ist(frame, slot)
        || live_rflags() & DF != 0
    {
        fail();
    }
    frame.set_old_rsp(saved_rsp());
    STATE.store(SAW, Ordering::Release);
    true
}

pub(super) fn handle_entry(frame: &mut EntryFrame) -> bool {
    match (STATE.load(Ordering::Acquire), frame.vector) {
        (ARMED_INT2, 2) => soft_ist(frame, 2, IST_NMI, addr(unsafe { &x86_ist_int2_resume })),
        (ARMED_INT18, 18) => soft_ist(frame, 18, IST_MC, addr(unsafe { &x86_ist_int18_resume })),
        (ARMED_DF, 8) => {
            // #DF is an abort: its saved CS:RIP is not a resume point.
            if frame.error != 0 || !on_ist(frame, IST_DF) || live_rflags() & DF != 0 {
                fail();
            }
            frame.rip = addr(unsafe { &x86_ist_df_resume });
            frame.cs = 0x08;
            frame.set_old_rsp(saved_rsp());
            STATE.store(SAW, Ordering::Release);
            true
        }
        (ARMED_NMI, 2) => {
            if frame.cs != 0x08 || !on_ist(frame, IST_NMI) || frame.old_rsp() == BAD_RSP {
                fail();
            }
            STATE.store(SAW, Ordering::Release);
            true
        }
        (ARMED_READ_FAULT, 14) => {
            if frame.cs != 0x08 || frame.rip != addr(unsafe { &x86_ist_read_fault }) {
                fail();
            }
            FAULT_ADDR.store(cr2(), Ordering::Release);
            FAULT_ERROR.store(frame.error, Ordering::Release);
            frame.rax = READ_FAULTED;
            frame.rip = addr(unsafe { &x86_ist_read_resume });
            STATE.store(SAW, Ordering::Release);
            true
        }
        (ARMED_EXEC_FAULT, 14) => {
            if frame.cs != 0x08 {
                fail();
            }
            FAULT_ADDR.store(cr2(), Ordering::Release);
            FAULT_ERROR.store(frame.error, Ordering::Release);
            frame.rip = addr(unsafe { &x86_ist_call_resume });
            frame.set_old_rsp(saved_rsp());
            STATE.store(SAW, Ordering::Release);
            true
        }
        (_, 13) if frame.cs == 0x23 && frame.rip == UMIP_RIP.load(Ordering::Acquire) => {
            if frame.error != 0 || !protect::active().umip {
                fail();
            }
            frame.rip = UMIP_NEXT.load(Ordering::Acquire);
            UMIP_TRAPS.fetch_add(1, Ordering::AcqRel);
            true
        }
        _ => false,
    }
}

/// Read `va` with a recoverable fault; `None` when the read faulted at `va`.
fn probe_read(va: u64) -> Option<u64> {
    arm(ARMED_READ_FAULT);
    let value = unsafe { x86_ist_probe_read(va) };
    match STATE.swap(IDLE, Ordering::AcqRel) {
        ARMED_READ_FAULT => Some(value),
        SAW if value == READ_FAULTED && FAULT_ADDR.load(Ordering::Acquire) == va => None,
        _ => fail(),
    }
}

fn hex(value: u64) {
    let digits = (64 - value.leading_zeros()).div_ceil(4).max(1);
    for shift in (0..digits).rev() {
        let nibble = ((value >> (shift * 4)) & 0xf) as u8;
        super::super::uart_16550::putchar(if nibble < 10 { b'0' + nibble } else { b'a' + nibble - 10 });
    }
}

fn send_self_nmi() {
    const LAPIC: usize = 0xfee0_0000;
    unsafe {
        let id = core::ptr::read_volatile((LAPIC + 0x20) as *const u32) >> 24;
        core::ptr::write_volatile((LAPIC + 0x310) as *mut u32, id << 24);
        // Delivery mode NMI (100b), level assert, no shorthand.
        core::ptr::write_volatile((LAPIC + 0x300) as *mut u32, (0b100 << 8) | (1 << 14));
    }
}

/// CPL0 IST phase. Runs before the Ring-3 tasks exist.
pub(super) fn run_cpl0() {
    let cpu = super::super::syscall::current_cpu_id();
    ist::watermark::fill(cpu);
    for (vector, slot) in [(2usize, IST_NMI), (8, IST_DF), (18, IST_MC)] {
        if super::super::idt::gate_ist(vector) != slot
            || super::super::gdt::ist_entry(slot) != ist::stack_top(cpu, slot)
        {
            fail();
        }
    }
    if ist::guards_installed() != ist::GUARD_PAGES {
        fail();
    }
    // Every guard is genuinely unmapped in the live root: both ends of NMI and
    // the top quadword below the BSP boot stack, where an overflow lands first.
    for guard in [
        ist::stack_bottom(cpu, IST_NMI) as u64 - 8,
        ist::stack_top(cpu, IST_NMI) + 8,
        ist::boot_stack_guard() as u64 + 0x1000 - 8,
    ] {
        if probe_read(guard).is_some() || FAULT_ERROR.load(Ordering::Acquire) & PF_PRESENT != 0 {
            fail();
        }
    }

    arm(ARMED_INT2);
    unsafe { x86_ist_probe_int2() };
    expect_seen();
    arm(ARMED_INT18);
    unsafe { x86_ist_probe_int18() };
    expect_seen();
    arm(ARMED_DF);
    unsafe { x86_ist_probe_df() };
    expect_seen();

    arm(ARMED_NMI);
    send_self_nmi();
    let mut spins = 0u32;
    while STATE.load(Ordering::Acquire) != SAW {
        spins += 1;
        if spins > 50_000_000 {
            fail();
        }
        core::hint::spin_loop();
    }
    expect_seen();

    super::super::uart_16550::puts(
        "\nX86-IST: PASS gates=2/8/18->ist1/2/3 tss=own-cpu guards=9 guard-read=pf boot-guard-read=pf \
         int2=ist1/bad-rsp int18=ist3/bad-rsp df=ist2/cascade nmi=ist1/lapic\n",
    );
    super::super::uart_16550::puts("X86-IST-HW: nmi=");
    hex(ist::watermark::used(cpu, IST_NMI) as u64);
    super::super::uart_16550::puts(" df=");
    hex(ist::watermark::used(cpu, IST_DF) as u64);
    super::super::uart_16550::puts(" mc=");
    hex(ist::watermark::used(cpu, IST_MC) as u64);
    super::super::uart_16550::puts(" limit=");
    hex(ist::STACK_BYTES as u64);
    super::super::uart_16550::putchar(b'\n');
    for slot in [IST_NMI, IST_DF, IST_MC] {
        // Leave at least half of each stack unused.
        if ist::watermark::used(cpu, slot) * 2 > ist::STACK_BYTES {
            fail();
        }
    }
}

/// Kernel phase, called from the Ring-3 probe syscall: `user_page` is the
/// fixture's U=1 code page and `expected` its first quadword.
pub(super) fn run_kernel_protection(user_page: u64, saved_flags: u64) {
    // SYSCALL entry: caller DF/AC preserved for SYSRET, cleared for Rust.
    if saved_flags & (DF | AC) != DF | AC || live_rflags() & (DF | AC) != 0 {
        fail();
    }
    let active = protect::active();
    if active != protect::requested() {
        fail();
    }
    let expected = unsafe { core::ptr::read_volatile(core::ptr::addr_of!(x86_idt_cpl3_user_start).cast::<u64>()) };

    // No SAS kernel profile qualifies for boot-time SMAP yet (task spawn
    // writes U=1 stack pages directly). When the CPU has SMAP, exercise the
    // mechanism with CR4.SMAP set only around these two reads.
    let smap_probe = !active.smap && protect::detect().smap;
    let flags = super::super::context::save_and_disable_interrupts();
    if smap_probe {
        set_smap(true);
    }
    let plain = probe_read(user_page);
    let windowed = protect::user_access(|| unsafe { core::ptr::read_volatile(user_page as *const u64) });
    let windowed_ac = live_rflags() & AC;
    if smap_probe {
        set_smap(false);
    }
    unsafe { super::super::context::restore_sstatus(flags) };
    if active.smap || smap_probe {
        if plain.is_some() || FAULT_ERROR.load(Ordering::Acquire) & (PF_PRESENT | PF_FETCH) != PF_PRESENT {
            fail();
        }
    } else if plain != Some(expected) {
        fail();
    }
    if windowed != expected || windowed_ac != 0 {
        fail();
    }

    if active.smep {
        arm(ARMED_EXEC_FAULT);
        unsafe { x86_ist_probe_call(user_page) };
        expect_seen();
        if FAULT_ADDR.load(Ordering::Acquire) != user_page
            || FAULT_ERROR.load(Ordering::Acquire) & (PF_PRESENT | PF_FETCH) != PF_PRESENT | PF_FETCH
        {
            fail();
        }
    }

    let start = addr(unsafe { &x86_idt_cpl3_user_start });
    UMIP_RIP.store(user_page + addr(unsafe { &x86_idt_cpl3_user_umip }) - start, Ordering::Release);
    UMIP_NEXT.store(user_page + addr(unsafe { &x86_idt_cpl3_user_umip_end }) - start, Ordering::Release);

    super::super::uart_16550::puts(if active.smap {
        "\nX86-PROTECT: smap=on read=pf window=ok"
    } else if smap_probe {
        "\nX86-PROTECT: smap=probe read=pf window=ok"
    } else {
        "\nX86-PROTECT: smap=off read=ok window=ok"
    });
    super::super::uart_16550::puts(if active.smep { " smep=on exec=pf" } else { " smep=off" });
    super::super::uart_16550::puts(" syscall-flags=df+ac-cleared\n");
}

/// Fixture-only SMAP toggle with interrupts masked by the caller. CLAC is
/// valid only while CR4.SMAP is set, so the entry flag follows it on enable
/// and leads it on disable.
fn set_smap(on: bool) {
    const CR4_SMAP: u64 = 1 << 21;
    unsafe {
        let cr4: u64;
        core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack));
        if on {
            core::arch::asm!("mov cr4, {}", in(reg) cr4 | CR4_SMAP, options(nomem, nostack));
            protect::SMAP_ACTIVE.store(1, Ordering::Release);
        } else {
            protect::SMAP_ACTIVE.store(0, Ordering::Release);
            core::arch::asm!("mov cr4, {}", in(reg) cr4 & !CR4_SMAP, options(nomem, nostack));
        }
    }
}

/// CPL3 completion: UMIP trapped `sgdt` exactly when the profile enabled it.
pub(super) fn finish_cpl3(user_rflags: u64) {
    let umip = protect::active().umip;
    let traps = UMIP_TRAPS.load(Ordering::Acquire);
    // SYSRET restored the caller's DF/AC; the IDT entry cleared live DF
    // (and AC under SMAP) before Rust.
    if user_rflags & (DF | AC) != DF | AC
        || live_rflags() & DF != 0
        || (protect::active().smap && live_rflags() & AC != 0)
        || traps != u8::from(umip)
    {
        fail();
    }
    super::super::uart_16550::puts(if umip {
        "\nX86-PROTECT: umip=on sgdt=gp\n"
    } else {
        "\nX86-PROTECT: umip=off sgdt=ran\n"
    });
}

core::arch::global_asm!(
    r#"
    .section .text.x86_ist_probe,"ax",@progbits

    # Each IST probe masks interrupts while RSP is invalid; only the probed
    # vector (or a non-maskable NMI) can arrive, and it uses an IST stack.
    .global x86_ist_probe_int2
x86_ist_probe_int2:
    .byte 0xf3,0x0f,0x1e,0xfa
    pushfq
    cli
    movq %rsp,X86_IST_SAVED_RSP(%rip)
    movabsq $0x8000000000000000,%rsp
    int $2
    .global x86_ist_int2_resume
x86_ist_int2_resume:
    popfq
    retq

    .global x86_ist_probe_int18
x86_ist_probe_int18:
    .byte 0xf3,0x0f,0x1e,0xfa
    pushfq
    cli
    movq %rsp,X86_IST_SAVED_RSP(%rip)
    movabsq $0x8000000000000000,%rsp
    int $18
    .global x86_ist_int18_resume
x86_ist_int18_resume:
    popfq
    retq

    # int3 through a non-IST gate cannot push to a non-canonical RSP; the
    # resulting fault cannot be delivered either, so the CPU raises #DF.
    .global x86_ist_probe_df
x86_ist_probe_df:
    .byte 0xf3,0x0f,0x1e,0xfa
    pushfq
    cli
    movq %rsp,X86_IST_SAVED_RSP(%rip)
    movabsq $0x8000000000000000,%rsp
    int3
    .global x86_ist_df_resume
x86_ist_df_resume:
    popfq
    retq

    .global x86_ist_probe_read
x86_ist_probe_read:
    .byte 0xf3,0x0f,0x1e,0xfa
    .global x86_ist_read_fault
x86_ist_read_fault:
    movq (%rdi),%rax
    .global x86_ist_read_resume
x86_ist_read_resume:
    retq

    .global x86_ist_probe_call
x86_ist_probe_call:
    .byte 0xf3,0x0f,0x1e,0xfa
    pushq %rbx
    movq %rsp,X86_IST_SAVED_RSP(%rip)
    callq *%rdi
    .global x86_ist_call_resume
x86_ist_call_resume:
    popq %rbx
    retq
"#,
    options(att_syntax)
);
