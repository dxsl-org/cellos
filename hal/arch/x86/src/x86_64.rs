//! x86_64 HAL facade.
//!
//! Sub-modules containing x86_64 assembly are gated on
//! `#[cfg(target_arch = "x86_64")]` to keep other targets' builds clean.

use hal_arch_trait::Arch;

#[cfg(target_arch = "x86_64")]
pub mod apic;
#[cfg(target_arch = "x86_64")]
pub mod boot;
#[cfg(target_arch = "x86_64")]
pub mod cet;
#[cfg(target_arch = "x86_64")]
pub mod clock;
#[cfg(target_arch = "x86_64")]
pub mod context;
#[cfg(target_arch = "x86_64")]
pub mod domain;
#[cfg(target_arch = "x86_64")]
pub mod gdt;
#[cfg(target_arch = "x86_64")]
pub mod guest_cpuid;
#[cfg(target_arch = "x86_64")]
pub mod guest_mem;
#[cfg(target_arch = "x86_64")]
pub mod hpet;
#[cfg(target_arch = "x86_64")]
pub mod idt;
#[cfg(target_arch = "x86_64")]
pub mod invept;
#[cfg(target_arch = "x86_64")]
pub mod ioapic;
#[cfg(target_arch = "x86_64")]
pub mod ist;
#[cfg(target_arch = "x86_64")]
pub mod msi;
#[cfg(target_arch = "x86_64")]
pub mod msr;
#[cfg(target_arch = "x86_64")]
pub mod paging;
#[cfg(target_arch = "x86_64")]
pub mod pku;
#[cfg(target_arch = "x86_64")]
pub mod rtc;
#[cfg(target_arch = "x86_64")]
pub mod svm;
#[cfg(target_arch = "x86_64")]
pub mod svm_vcpu;
#[cfg(target_arch = "x86_64")]
pub mod syscall;
#[cfg(target_arch = "x86_64")]
pub mod timer;
#[cfg(target_arch = "x86_64")]
pub mod trap;
#[cfg(target_arch = "x86_64")]
pub mod uart_16550;
#[cfg(target_arch = "x86_64")]
pub mod vmcb;
#[cfg(target_arch = "x86_64")]
pub mod vmcs;
#[cfg(target_arch = "x86_64")]
pub mod vmexit_decode;
#[cfg(target_arch = "x86_64")]
pub mod vmx;
#[cfg(target_arch = "x86_64")]
pub mod vmx_vcpu;
#[cfg(target_arch = "x86_64")]
pub mod world_switch;

#[cfg(target_arch = "x86_64")]
pub use context::CpuContext as Context;
#[cfg(target_arch = "x86_64")]
pub use paging::PageTable;
#[cfg(target_arch = "x86_64")]
pub use paging::PAGE_SIZE;

/// Mirrors the rv64 `pub mod arch { ... }` consumed by kernel/src/task*.rs.
///
/// `init()` is intentionally a no-op: GDT/IDT/syscall/APIC are initialised
/// once via `ARCH.init()` in kmain. `task_entry_point` calls this per-task
/// to match the RISC-V per-hart stvec setup convention; on x86_64 the IDT is
/// global so no per-task re-init is needed.
#[cfg(target_arch = "x86_64")]
pub mod arch {
    pub use super::context::CpuContext as Context;
    pub use super::context::{
        restore_sstatus, save_and_disable_interrupts, set_kernel_stack_for, switch, IrqSaved, Root,
        SwitchPlan,
    };
    pub use super::set_kernel_stack;
    pub use super::trap::{get_gp_tp, ViTrapFrame};

    /// Per-task arch init — no-op on x86_64 (global IDT set up in kmain).
    pub fn init() {}

    pub fn enable_interrupts() {
        // SAFETY: sti is a Ring-0 instruction; no memory invariants affected.
        unsafe {
            core::arch::asm!("sti", options(nomem, nostack));
        }
    }

    extern "C" {
        /// Assembly trampoline that bootstraps new x86_64 threads.
        /// Defined in hal/arch/x86/src/x86_64/boot.rs (Phase 02).
        pub fn thread_trampoline();
    }
}

#[cfg(target_arch = "x86_64")]
use core::sync::atomic::{AtomicUsize, Ordering};

/// Runtime HPET MMIO base parsed from ACPI; 0 means no HPET. HPET is one
/// clocksource candidate (`clock::select`); the G1 private lane requires it.
///
/// Set by `set_hpet_base` (called from kmain after ACPI parse) before
/// `init_timers` is invoked.
#[cfg(target_arch = "x86_64")]
static HPET_MMIO_BASE: AtomicUsize = AtomicUsize::new(0);

// Nonzero policy snapshot is published by the BSP before AP startup.
#[cfg(target_arch = "x86_64")]
static CPU_FEATURE_POLICY: AtomicUsize = AtomicUsize::new(0);

#[cfg(target_arch = "x86_64")]
fn cpu_feature_policy() -> usize {
    let leaf1 = core::arch::x86_64::__cpuid(1);
    let cet = cet::detect();
    let pku = pku::detect();
    let protect = protect::detect();
    1 | ((leaf1.edx & ((1 << 24) | (1 << 25) | (1 << 26)) == ((1 << 24) | (1 << 25) | (1 << 26)))
        as usize)
        << 1
        | (cet.ibt as usize) << 2
        | (cet.shstk as usize) << 3
        | (pku.pku as usize) << 4
        | (pku.pks as usize) << 5
        | (domain::pcid_supported() as usize) << 6
        | (domain::invpcid_supported() as usize) << 7
        | (protect.smep as usize) << 8
        | (protect.smap as usize) << 9
        | (protect.umip as usize) << 10
}

/// Opt-in supervisor protections, one cargo feature each (`x86-smep`,
/// `x86-smap`, `x86-umip`); all off by default.
///
/// They are defense in depth, not a substitute for LBI or for authority
/// checks. A requested protection the CPU lacks refuses the CPU (boot or AP
/// admission) instead of running unprotected. SMAP is only selectable for an
/// image whose kernel never touches U=1 memory outside a `user_access` window.
#[cfg(target_arch = "x86_64")]
pub mod protect {
    use core::sync::atomic::{AtomicU8, Ordering};

    const CR4_UMIP: u64 = 1 << 11;
    const CR4_SMEP: u64 = 1 << 20;
    const CR4_SMAP: u64 = 1 << 21;

    /// Read by the IDT entry: nonzero means CLAC is valid and required.
    ///
    /// Every CPU stores it from `init_cpu` while others read it, so it is
    /// atomic; the entry asm's `cmpb` byte load relies on the one-byte layout.
    #[used]
    #[export_name = "ViCell_smap_active"]
    pub static SMAP_ACTIVE: AtomicU8 = AtomicU8::new(0);
    const _: () = assert!(core::mem::size_of::<AtomicU8>() == 1);

    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    pub struct Caps {
        pub smep: bool,
        pub smap: bool,
        pub umip: bool,
    }

    pub fn detect() -> Caps {
        if core::arch::x86_64::__cpuid(0).eax < 7 {
            return Caps::default();
        }
        let leaf7 = core::arch::x86_64::__cpuid_count(7, 0);
        Caps {
            smep: leaf7.ebx & (1 << 7) != 0,
            smap: leaf7.ebx & (1 << 20) != 0,
            umip: leaf7.ecx & (1 << 2) != 0,
        }
    }

    pub const fn requested() -> Caps {
        Caps {
            smep: cfg!(feature = "x86-smep"),
            smap: cfg!(feature = "x86-smap"),
            umip: cfg!(feature = "x86-umip"),
        }
    }

    /// Protections currently set in this CPU's CR4.
    pub fn active() -> Caps {
        let cr4: u64;
        unsafe { core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack)) };
        Caps {
            smep: cr4 & CR4_SMEP != 0,
            smap: cr4 & CR4_SMAP != 0,
            umip: cr4 & CR4_UMIP != 0,
        }
    }

    /// Enable the requested protections on this CPU. Returns false, changing
    /// nothing, when the CPU lacks one of them.
    pub fn init_cpu() -> bool {
        let want = requested();
        let have = detect();
        if (want.smep && !have.smep) || (want.smap && !have.smap) || (want.umip && !have.umip) {
            return false;
        }
        let bits = if want.smep { CR4_SMEP } else { 0 }
            | if want.smap { CR4_SMAP } else { 0 }
            | if want.umip { CR4_UMIP } else { 0 };
        if bits != 0 {
            unsafe {
                let mut cr4: u64;
                core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack));
                core::arch::asm!("mov cr4, {}", in(reg) cr4 | bits, options(nomem, nostack));
                if want.smap {
                    // CR4.SMAP is set; CLAC is now a valid instruction.
                    SMAP_ACTIVE.store(1, Ordering::Release);
                }
            }
        }
        active() == want
    }

    /// Run `f` with supervisor access to U=1 pages allowed (SMAP profile).
    ///
    /// Interrupts stay masked for the window, so no handler or context switch
    /// runs inside it; AC is cleared before interrupts return. `f` must not
    /// unwind. Without SMAP active this is just `f()`.
    pub fn user_access<R>(f: impl FnOnce() -> R) -> R {
        if SMAP_ACTIVE.load(Ordering::Acquire) == 0 {
            return f();
        }
        let flags = super::context::save_and_disable_interrupts();
        // No `nomem`: STAC/CLAC must not be reordered with `f`'s user accesses.
        unsafe { core::arch::asm!("stac", options(nostack)) };
        let result = f();
        unsafe {
            core::arch::asm!("clac", options(nostack));
            super::context::restore_sstatus(flags);
        }
        result
    }
}

/// Publish the BSP feature policy after its local initialization completes.
#[cfg(target_arch = "x86_64")]
pub fn publish_boot_cpu_policy() {
    let bsp_id = apic::current_id();
    apic::register_cpu(0, bsp_id);
    let policy = cpu_feature_policy();
    assert!(policy & 2 != 0, "x86 requires FXSAVE, SSE and SSE2");
    CPU_FEATURE_POLICY.store(policy, Ordering::Release);
}

/// Initialize an AP under the shared kernel root, leaving interrupts masked.
///
/// # Safety
/// The logical slot is dedicated to this CPU, its permanent stack and root are
/// mapped, and the BSP has finished feature policy, IDT and timer initialization.
#[cfg(target_arch = "x86_64")]
pub unsafe fn init_secondary_cpu(logical: usize, lapic_id: u32, kernel_cr3: usize) -> bool {
    core::arch::asm!("cli", options(nomem, nostack));
    if logical == 0
        || logical >= syscall::MAX_CPUS
        || kernel_cr3 == 0
        || kernel_cr3 & 0xfff != 0
        || domain::kernel_cr3() != kernel_cr3
    {
        return false;
    }
    // No per-task SSP migration substrate exists: never share the BSP SSP.
    if cfg!(feature = "cet-shadow-stack") {
        return false;
    }
    let policy = CPU_FEATURE_POLICY.load(Ordering::Acquire);
    if policy == 0 || policy & 2 == 0 || policy != cpu_feature_policy() {
        return false;
    }
    core::arch::asm!("mov cr3, {}", in(reg) kernel_cr3, options(nostack));
    syscall::install_cpu(logical);
    init_sse();
    gdt::init();
    if !idt::load_current_cpu() {
        return false;
    }
    cet::init_kernel_cet();
    if !pku::init_secondary() || !domain::init_secondary_pcid() || !protect::init_cpu() {
        return false;
    }
    syscall::init();
    apic::register_cpu(logical, lapic_id) && clock::start_secondary(lapic_id)
}

/// Store the HPET MMIO base address from ACPI for use by `init_timers`.
///
/// Zero (the default) means no HPET; call with a validated base before
/// `init_timers()`.
#[cfg(target_arch = "x86_64")]
pub fn set_hpet_base(base: usize) {
    HPET_MMIO_BASE.store(base, Ordering::Relaxed);
}

/// Install the FADT PM timer (port I/O block, 24 or 32 bits) as a
/// clocksource candidate. Call before `init_timers()`.
#[cfg(target_arch = "x86_64")]
pub fn set_pm_timer(port: u16, bits: u8) {
    clock::pmtimer::init(port, bits);
}

/// The clocksource and tick chosen by [`init_timers`].
#[cfg(target_arch = "x86_64")]
#[derive(Clone, Copy, Debug)]
pub struct TimerSetup {
    pub source: clock::ClockSource,
    pub clockevent: clock::Clockevent,
    /// LAPIC registers are accessed through x2APIC MSRs.
    pub x2apic: bool,
}

/// Post-paging timer init for every non-G1 boot: choose the clocksource
/// (`clock::select`), then enable the LAPIC and start the tick
/// (`clock::start_clockevent`). `None` means no clocksource exists and the
/// LAPIC was left untouched, so the caller keeps the timer gate closed.
///
/// Must be called AFTER init_kernel_paging_x86, which identity-maps IOAPIC,
/// HPET, and LAPIC at addresses parsed from validated ACPI tables. Resets the
/// APIC HHDM_BASE to 0 so lapic_base() == physical LAPIC address.
#[cfg(target_arch = "x86_64")]
pub fn init_timers() -> Option<TimerSetup> {
    let hpet_base = HPET_MMIO_BASE.load(Ordering::Relaxed);
    if hpet_base != 0 {
        // SAFETY: hpet_base is identity-mapped by init_kernel_paging_x86.
        unsafe { hpet::init(hpet_base) };
    }
    let hpet_ready = hpet_base != 0 && hpet::present();
    let source = clock::select(hpet_ready);
    if source == clock::ClockSource::None {
        return None;
    }
    apic::set_hhdm_base(0);
    let x2apic = apic::init_mode();
    apic::init_lapic();
    let clockevent = clock::start_clockevent();
    // Verify the timer fires from ring-0 before entering user mode.
    // SAFETY: sti/hlt/cli are Ring-0 instructions; no memory side effects.
    for _ in 0..2u8 {
        unsafe { core::arch::asm!("sti", "hlt", "cli", options(nomem, nostack)) };
    }
    Some(TimerSetup {
        source,
        clockevent,
        x2apic,
    })
}

/// Actual BSP calibration outcome, without a second calibration or fallback claim.
#[cfg(target_arch = "x86_64")]
#[derive(Clone, Copy)]
pub struct TimerCalibration {
    pub ticks_per_ms: u64,
    pub hpet_used: bool,
    pub initial_count: u32,
    pub lvt: u32,
}

#[cfg(target_arch = "x86_64")]
pub fn init_timers_observed() -> Option<TimerCalibration> {
    init_timers_inner(true)
}

#[cfg(target_arch = "x86_64")]
fn init_timers_inner(observe: bool) -> Option<TimerCalibration> {
    let hpet_base = HPET_MMIO_BASE.load(Ordering::Relaxed);
    if hpet_base == 0 {
        return None;
    }

    // Switch LAPIC/IOAPIC accesses to the identity-mapped PAs we set up in our PML4.
    apic::set_hhdm_base(0);
    apic::init_mode();

    // Enable LAPIC (SVR) and set an initial periodic timer config.
    apic::init_lapic();
    uart_16550::putchar(b'A'); // LAPIC enabled

    // Init HPET using the validated runtime base from ACPI.
    // If hardware absent, HPET_PERIOD_FS stays 0 and calibrate_lapic falls back
    // to the 8254 PIT path automatically.
    // SAFETY: hpet_base is identity-mapped by init_kernel_paging_x86.
    unsafe {
        hpet::init(hpet_base);
    }
    uart_16550::putchar(b'H'); // after HPET init (H=HPET attempted)

    let ticks_per_ms = hpet::calibrate_lapic();
    let hpet_used = observe && hpet::checked_sample().is_ok();
    uart_16550::putchar(b'C'); // calibration done
                               // '0'=zero (timer disabled!), '1'=ok.
    uart_16550::putchar(if ticks_per_ms == 0 { b'0' } else { b'1' });
    // 'O'=overflow (>u32::MAX/10 → count wraps to tiny value → ISR storm), 'o'=ok.
    uart_16550::putchar(if ticks_per_ms > (u32::MAX as u64 / 10) {
        b'O'
    } else {
        b'o'
    });

    apic::init_lapic_calibrated(ticks_per_ms);

    // Read back LVT_TIMER and initial count to verify LAPIC is armed.
    // 'K'=unmasked+periodic, 'M'=masked, '?'=unexpected.
    let lvt = apic::read_lvt_timer();
    let init_cnt = apic::read_initial_count();
    let lvt_masked = (lvt >> 16) & 1 != 0; // bit 16 = mask
    let lvt_mode = (lvt >> 17) & 0x3; // bits 18:17 = timer mode
    uart_16550::putchar(if lvt_masked {
        b'M'
    } else if lvt_mode == 1 {
        b'K'
    } else {
        b'?'
    });
    // Print initial count as a range probe:
    // '!'=0 (stopped), 'l'=<10000, 's'=<100000, 'n'=<1000000, 'b'=>=1000000
    uart_16550::putchar(match init_cnt {
        0 => b'!',
        1..=9_999 => b'l',
        10_000..=99_999 => b's',
        100_000..=999_999 => b'n',
        _ => b'b',
    });

    // 'x' = xAPIC (MMIO) / 'X' = x2APIC (MSR) register access.
    uart_16550::putchar(if apic::x2apic_active() { b'X' } else { b'x' });

    // Verify the timer fires from ring-0 before entering user mode.
    // Enable interrupts + halt twice.  'V' probes from x86_64_timer_handler appear
    // before '~' if the LAPIC is delivering correctly from ring-0.
    // SAFETY: sti/hlt/cli are Ring-0 instructions; no memory side effects.
    for _ in 0..2u8 {
        unsafe {
            core::arch::asm!(
                "sti",
                "hlt", // sleep until next interrupt (LAPIC timer or other)
                "cli",
                options(nomem, nostack)
            );
        }
        uart_16550::putchar(b'~'); // '~' = returned from this hlt iteration
    }
    Some(TimerCalibration {
        ticks_per_ms,
        hpet_used,
        initial_count: init_cnt,
        lvt,
    })
}

/// x86_64 architecture implementation.
pub struct X86_64Arch;

pub type PlatformArch = X86_64Arch;
pub static ARCH: PlatformArch = X86_64Arch;

// ── Stub impl for non-x86_64 targets ──────────────────────────────────────────
#[cfg(not(target_arch = "x86_64"))]
impl Arch for X86_64Arch {
    type Context = usize;
    fn init(&self) {}
    unsafe fn switch_context(&self, _: *mut Self::Context, _: *const Self::Context) {}
    fn enable_interrupts(&self) {}
    fn disable_interrupts(&self) {}
    fn wait_for_interrupt(&self) {}
    fn interrupts_enabled(&self) -> bool {
        false
    }
}

// ── Full implementation for x86_64 ────────────────────────────────────────────
#[cfg(target_arch = "x86_64")]
impl Arch for X86_64Arch {
    type Context = context::CpuContext;

    /// Initialise x86_64 hardware: GDT, IDT, APIC, syscall MSRs.
    fn init(&self) {
        init_sse();
        gdt::init();
        idt::init();
        cet::init_kernel_cet(); // LAYER2-CET-INIT — must follow idt (registers #CP vector 21)
        pku::init(); // LAYER2-PKU-INIT — requires IBT (checked inside)
        syscall::init();
        assert!(
            protect::init_cpu(),
            "x86: requested SMEP/SMAP/UMIP profile unsupported by CPU"
        );
        apic::init_lapic();
        publish_boot_cpu_policy();
    }

    /// # Safety
    /// Both pointers must point to valid, aligned `CpuContext` structs.
    unsafe fn switch_context(&self, old: *mut Self::Context, new: *const Self::Context) {
        // SAFETY: invariant upheld by caller.
        unsafe {
            context::CpuContext::switch(old, new);
        }
    }

    fn enable_interrupts(&self) {
        // SAFETY: sti is a standard Ring-0 instruction; no memory invariants affected.
        unsafe {
            core::arch::asm!("sti", options(nomem, nostack));
        }
    }

    fn disable_interrupts(&self) {
        // SAFETY: cli is a standard Ring-0 instruction.
        unsafe {
            core::arch::asm!("cli", options(nomem, nostack));
        }
    }

    fn wait_for_interrupt(&self) {
        // SAFETY: hlt halts the CPU until the next interrupt; no memory side effects.
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack));
        }
    }

    fn interrupts_enabled(&self) -> bool {
        let rflags: u64;
        // SAFETY: pushfq/popfq read RFLAGS without modifying any visible state.
        unsafe {
            core::arch::asm!(
                "pushfq",
                "pop {f}",
                f = out(reg) rflags,
                options(nomem),
            );
        }
        rflags & (1 << 9) != 0 // IF bit
    }
}

/// Set TSS.rsp0 and CPU_LOCAL.kernel_rsp for the selected Ring-3 task.
#[cfg(target_arch = "x86_64")]
pub fn set_kernel_stack(sp: usize) {
    // Context switches run in kernel GS state and deliberately preserve both
    // GS-base MSRs; only the two stack destinations change here.
    gdt::set_kernel_stack(sp as u64);
    syscall::set_kernel_stack(sp as u64);
}

#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn init_sse() {
    unsafe {
        let mut cr0: u64;
        core::arch::asm!("mov {}, cr0", out(reg) cr0, options(nomem, nostack));
        cr0 &= !((1 << 2) | (1 << 3)); // EM=0, TS=0: eager FPU ownership
        cr0 |= 1 << 1;
        core::arch::asm!("mov cr0, {}", in(reg) cr0, options(nomem, nostack));

        let mut cr4: u64;
        core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack));
        cr4 |= (1 << 9) | (1 << 10);
        core::arch::asm!("mov cr4, {}", in(reg) cr4, options(nomem, nostack));
        core::arch::asm!("fninit", options(nostack));

        let mxcsr: u32 = 0x1F80;
        core::arch::asm!("ldmxcsr [{}]", in(reg) &mxcsr, options(nostack));
    }
}

#[cfg(all(target_arch = "x86_64", not(target_os = "none")))]
pub fn init_sse() {}
