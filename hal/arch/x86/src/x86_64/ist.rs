//! Per-CPU Interrupt Stack Table stacks for NMI, #DF and #MC.
//!
//! These three vectors can arrive while RSP is not a trustworthy kernel
//! stack: in the SYSCALL window between `swapgs` and the kernel-stack load,
//! after SYSRET's user RSP restore, or after a stack overflow. In 64-bit mode
//! an IST gate always switches to its own stack, so delivery never writes
//! through the interrupted RSP.
//!
//! Layout per CPU, all in kernel `.bss` (mapped in every CR3 through the
//! shared higher half): `[G][NMI][G][#DF][G][#MC][G]`. Every stack has an
//! unmapped guard page on both ends once [`install_guards`] has run, which
//! also unmaps the page below the BSP boot stack (`kernel/linker-x86-64.ld`).
//!
//! Size: the deepest path is the shared entry (160-byte frame, 16 bytes of
//! alignment and the 528-byte FXSAVE area) plus the fatal dispatch and serial
//! writer, about 1 KiB. 8 KiB leaves an 8x margin; the IDT fixture reports the
//! measured high-water mark (`ist-hw=`).
//!
//! Count: one slot per logical CPU up to `syscall::MAX_CPUS` (64), 40 KiB
//! each, so the region is 2.5 MiB of `.bss` whether or not a CPU comes online.
//! It stays static (plan 261103-2218 phase 07a): a guard needs a 4 KiB leaf,
//! which the kernel image mapping provides, while a frame-allocator page is
//! reached through the HHDM, which Limine maps with 1 GiB leaves that
//! [`install_guards`] refuses to split.

use super::syscall::MAX_CPUS;
use core::sync::atomic::{AtomicUsize, Ordering};

static GUARDS_INSTALLED: AtomicUsize = AtomicUsize::new(0);

/// Guard pages verified unmapped by [`install_guards`] (0 before it runs).
pub fn guards_installed() -> usize {
    GUARDS_INSTALLED.load(Ordering::Acquire)
}

/// Guard pages the layout requires: four per CPU slot plus the BSP boot-stack
/// guard.
pub const GUARD_PAGES: usize = (IST_COUNT + 1) * MAX_CPUS + 1;

/// TSS IST slot numbers (1-based, as encoded in the IDT gate).
pub const IST_NMI: u8 = 1;
pub const IST_DF: u8 = 2;
pub const IST_MC: u8 = 3;
pub const IST_COUNT: usize = 3;

const PAGE: usize = 4096;
const STACK_PAGES: usize = 2;
/// One guard page below each stack, plus one above the last.
const CPU_PAGES: usize = IST_COUNT * (STACK_PAGES + 1) + 1;
pub const STACK_BYTES: usize = STACK_PAGES * PAGE;

#[repr(C, align(4096))]
struct Region([u8; CPU_PAGES * PAGE * MAX_CPUS]);

static mut REGION: Region = Region([0; CPU_PAGES * PAGE * MAX_CPUS]);

unsafe extern "C" {
    /// Linker symbol: the page directly below the 64 KiB BSP boot stack.
    static __boot_stack_guard: u8;
}

/// The guard page directly below the BSP boot stack (`.stack`).
pub fn boot_stack_guard() -> usize {
    core::ptr::addr_of!(__boot_stack_guard) as usize
}

fn cpu_base(cpu: usize) -> usize {
    assert!(cpu < MAX_CPUS);
    core::ptr::addr_of!(REGION) as usize + cpu * CPU_PAGES * PAGE
}

/// Lowest usable byte of the stack for IST slot `ist` (1-based).
pub fn stack_bottom(cpu: usize, ist: u8) -> usize {
    let slot = usize::from(ist);
    assert!((1..=IST_COUNT).contains(&slot));
    cpu_base(cpu) + (slot - 1) * (STACK_PAGES + 1) * PAGE + PAGE
}

/// Initial RSP for IST slot `ist`: the first byte above the stack, 16-aligned.
pub fn stack_top(cpu: usize, ist: u8) -> u64 {
    (stack_bottom(cpu, ist) + STACK_BYTES) as u64
}

fn guard_pages() -> impl Iterator<Item = usize> {
    (0..MAX_CPUS)
        .flat_map(|cpu| (0..=IST_COUNT).map(move |k| cpu_base(cpu) + k * (STACK_PAGES + 1) * PAGE))
        .chain(core::iter::once(boot_stack_guard()))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuardError {
    /// The guard VA sits inside a 1 GiB or 2 MiB leaf; it cannot be split here.
    HugeLeaf(usize),
    /// A table level on the path is absent, so the stack itself is unmapped.
    NotMapped(usize),
    /// The leaf was cleared but a re-walk still finds it present.
    StillMapped(usize),
}

const PRESENT: u64 = 1;
const HUGE: u64 = 1 << 7;

/// Leaf PTE slot for a 4 KiB mapping of `va` under `pml4`, refusing huge leaves.
///
/// # Safety
/// `pml4` is the active root, readable and writable through the HHDM.
unsafe fn leaf_slot(pml4: *mut u64, va: usize) -> Result<*mut u64, GuardError> {
    let mut table = pml4;
    for shift in [39, 30, 21] {
        let entry = unsafe { core::ptr::read_volatile(table.add((va >> shift) & 0x1ff)) };
        if entry & PRESENT == 0 {
            return Err(GuardError::NotMapped(va));
        }
        if shift != 39 && entry & HUGE != 0 {
            return Err(GuardError::HugeLeaf(va));
        }
        table = super::paging::phys_to_virt_ptr((entry & super::paging::PTE_ADDR_MASK) as usize)
            as *mut u64;
    }
    Ok(unsafe { table.add((va >> 12) & 0x1ff) })
}

/// Unmap every guard page in the live root and verify each is gone.
///
/// Call once on the BSP before the TSS/IST gates are loaded. The BSP is then
/// running on the boot stack; its guard lies below `__stack_bottom`, outside
/// the live stack. Later roots (the kernel PML4, AP and domain roots) copy the
/// higher-half PML4 entries and so share these page tables; only the local TLB
/// needs flushing. Returns the number of guard pages installed.
///
/// # Safety
/// Kernel mode, interrupts in any state, HHDM offset set, no other CPU running.
pub unsafe fn install_guards() -> Result<usize, GuardError> {
    let root = (unsafe { super::paging::read_cr3() } & super::paging::PTE_ADDR_MASK) as usize;
    let pml4 = super::paging::phys_to_virt_ptr(root) as *mut u64;
    let mut count = 0;
    for va in guard_pages() {
        let slot = unsafe { leaf_slot(pml4, va) }?;
        unsafe { core::ptr::write_volatile(slot, 0) };
        super::paging::flush_tlb_page(va);
        match unsafe { leaf_slot(pml4, va) } {
            Ok(slot) if unsafe { core::ptr::read_volatile(slot) } & PRESENT == 0 => count += 1,
            Ok(_) => return Err(GuardError::StillMapped(va)),
            // The PT itself is still present; an absent level is also unmapped.
            Err(GuardError::NotMapped(_)) => count += 1,
            Err(error) => return Err(error),
        }
    }
    GUARDS_INSTALLED.store(count, Ordering::Release);
    Ok(count)
}

/// True when `rsp` lies inside the usable stack of IST slot `ist` on `cpu`.
pub fn contains(cpu: usize, ist: u8, rsp: u64) -> bool {
    let bottom = stack_bottom(cpu, ist) as u64;
    (bottom..=stack_top(cpu, ist)).contains(&rsp)
}

#[cfg(feature = "x86-idt-cpl3-test")]
pub(crate) mod watermark {
    use super::*;

    const PATTERN: u8 = 0xa5;

    /// Fill this CPU's stacks with a pattern before the probes run.
    pub(crate) fn fill(cpu: usize) {
        for ist in 1..=IST_COUNT as u8 {
            unsafe {
                core::ptr::write_bytes(stack_bottom(cpu, ist) as *mut u8, PATTERN, STACK_BYTES);
            }
        }
    }

    /// Bytes of stack slot `ist` ever written since [`fill`].
    pub(crate) fn used(cpu: usize, ist: u8) -> usize {
        let bottom = stack_bottom(cpu, ist) as *const u8;
        let untouched = (0..STACK_BYTES)
            .take_while(|&i| unsafe { core::ptr::read_volatile(bottom.add(i)) } == PATTERN)
            .count();
        STACK_BYTES - untouched
    }
}
