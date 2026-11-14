// SPDX-License-Identifier: MPL-2.0
//! x86_64 SMP bring-up (Limine MP handoff, the four-step AP handshake) and its
//! test-hooks fixtures. Compiled as the `x86` child of `crate::sched::smp`
//! (`#[path]`), so it keeps reading that module's private state unchanged.

use super::*;

// x86 AP bring-up handshake (phase 07b: every MP CPU up to `MAX_HARTS`).
//
// The BSP starts APs one at a time, in Limine MP order, each through the same
// four-step exchange; the next AP is released only after the previous one is
// online, so each handshake word below names at most one AP in flight and is
// a logical id (0 = none yet), never a flag that a later AP could confuse with
// an earlier one's:
//   1. AP: own flush proof on the BSP (request_tlb_flush(0) acknowledged), then
//      `X86_READY = logical` (or `X86_AP_ROOT_REFUSED = logical` and park).
//   2. BSP: flush proof on the AP (request_tlb_flush(logical) acknowledged),
//      then `X86_QUALIFIED = logical`.
//   3. AP: `HART_ONLINE[logical] = true`.
//   4. BSP: one catch-up flush acknowledged by the AP (it was outside every
//      shootdown target set until step 3), then the next AP.
// A refusal (root operation) or a start failure (stack, topology) ends
// bring-up, so the online set is always the contiguous range 0..online and
// the RT hart (1) is online whenever any AP is. An AP that was released but
// never answers a step within its bounded budget panics the boot
// (`x86_bounded_wait`): fail-closed, never a silently missing CPU.
pub(super) static X86_READY: AtomicUsize = AtomicUsize::new(0);
/// Highest logical AP the BSP has qualified (step 2); monotone, because APs
/// are qualified in increasing logical order.
pub(super) static X86_QUALIFIED: AtomicUsize = AtomicUsize::new(0);
pub(super) static X86_BOOT_COMPLETE: AtomicBool = AtomicBool::new(false);
/// Logical id of the AP that answered step 1 with a refusal instead of
/// `X86_READY`: it cannot enter the x86 root operation the BSP latched, so it
/// parks and never goes online.
pub(super) static X86_AP_ROOT_REFUSED: AtomicUsize = AtomicUsize::new(0);
pub(super) static X86_PREEMPT: [AtomicBool; MAX_HARTS] =
    [const { AtomicBool::new(false) }; MAX_HARTS];

/// Budget of one bounded handshake wait (`x86_bounded_wait`).
pub(super) const X86_WAIT_STEPS: usize = 100_000_000;

pub fn x86_substrate_qualified() -> bool {
    // Every online AP passed step 2 before it published `HART_ONLINE`.
    X86_BOOT_COMPLETE.load(Ordering::Acquire)
        && (online_harts().count() == 1 || X86_QUALIFIED.load(Ordering::Acquire) != 0)
}

pub fn x86_scheduler_ready() -> bool {
    X86_BOOT_COMPLETE.load(Ordering::Acquire)
}

pub fn request_preemption(hart: usize) {
    if hart < MAX_HARTS {
        X86_PREEMPT[hart].store(true, Ordering::Release);
        send_ipi(hart);
    }
}

pub fn take_preemption(hart: usize) -> bool {
    hart < MAX_HARTS && X86_PREEMPT[hart].swap(false, Ordering::AcqRel)
}

/// What the BSP hands one AP through `LimineMpInfo.extra_argument`; read only
/// by `vi_x86_secondary_entry` (field offsets 0/8/16/24 are its contract).
/// `AtomicUsize` has the layout of `usize`, so the asm reads plain words.
#[repr(C)]
pub(super) struct X86SecondaryContext {
    stack_top: AtomicUsize,
    kernel_cr3: AtomicUsize,
    logical: AtomicUsize,
    lapic_id: AtomicUsize,
}

/// One context per logical AP slot, so no AP's handoff is ever rewritten for
/// another. Each slot is written once by the BSP before the Release store to
/// that AP's Limine `goto_address`.
pub(super) static X86_SECONDARY: [X86SecondaryContext; MAX_HARTS] = [const {
    X86SecondaryContext {
        stack_top: AtomicUsize::new(0),
        kernel_cr3: AtomicUsize::new(0),
        logical: AtomicUsize::new(0),
        lapic_id: AtomicUsize::new(0),
    }
}; MAX_HARTS];

// Limine enters with IF clear on its own permanent bootloader stack. Read all
// handoff fields before the root change, adopt our dedicated stack before Rust
// touches any stack, and never return to bootloader state. Both roots retain
// the loaded kernel mapping and HHDM.
core::arch::global_asm!(
    ".global vi_x86_secondary_entry",
    ".type vi_x86_secondary_entry,@function",
    "vi_x86_secondary_entry:",
    "endbr64",
    "cli",
    "mov rax, [rdi + 24]",
    "mov rdx, [rax]",
    "mov rcx, [rax + 8]",
    "mov rdi, [rax + 16]",
    "mov rsi, [rax + 24]",
    "mov cr3, rcx",
    "mov rsp, rdx",
    "and rsp, -16",
    "xor rbp, rbp",
    "mov rdx, rcx",
    "call {entry}",
    "ud2",
    entry = sym x86_secondary_main,
);

pub(super) fn x86_bounded_wait(done: impl FnMut() -> bool) {
    x86_bounded_wait_steps(X86_WAIT_STEPS, done);
}

pub(super) fn x86_bounded_wait_steps(steps: usize, mut done: impl FnMut() -> bool) {
    // A bounded instruction budget does not depend on timer interrupts being
    // delivered to the waiting CPU. Fail closed once a callback was released.
    for _ in 0..steps {
        if done() {
            return;
        }
        core::hint::spin_loop();
    }
    panic!("[x86-smp] active AP safety handshake timed out");
}

/// x86 IDT fixture only: it runs inside `task::init`, before
/// `start_secondaries`, on the BSP alone and never returns. Publish the same
/// uniprocessor boot-complete state that `start_secondaries` publishes when no
/// AP exists, so `yield_cpu` schedules the fixture's Ring-3 tasks. `task::init`
/// has already published the BSP in `HART_ONLINE`.
#[cfg(feature = "x86-idt-cpl3-test")]
pub(crate) fn x86_idt_fixture_uniprocessor_ready() {
    X86_BOOT_COMPLETE.store(true, Ordering::Release);
}

/// `processor_ids=` of the topology line: the BSP's LAPIC ID, then each online
/// AP's, in logical order.
pub(super) struct X86OnlineApicIds {
    bsp_id: u32,
}

impl core::fmt::Display for X86OnlineApicIds {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.bsp_id)?;
        for hart in online_harts().filter(|hart| *hart != 0) {
            if let Some(id) = logical_to_physical(hart) {
                write!(f, ";{}", id)?;
            }
        }
        Ok(())
    }
}

/// Bring every Limine MP CPU online, up to `MAX_HARTS` logical harts.
///
/// Logical 0 is the BSP; APs take logical 1, 2, … in Limine MP order (1 is
/// the RT hart, the rest are normal harts). APs are started one at a time
/// through the handshake documented at `X86_READY`; a CPU beyond `MAX_HARTS`
/// is logged `cpu N ignored: MAX_HARTS` and left in Limine's park loop, a
/// LAPIC ID above 255 stays offline (no interrupt remapping, phase 12), and
/// the first AP that refuses root operation or cannot be started ends
/// bring-up with the harts already online.
///
/// `task::quiesce` fails closed above 8 online harts; on x86 it already refuses
/// every multi-hart set (no park hook), so bring-up does not cap below
/// `MAX_HARTS` for it (phase 07b decision D2).
pub fn start_secondaries() {
    use crate::arch::boot::limine;
    let bsp_id = hal::x86_64::apic::current_id();
    #[cfg(feature = "test-hooks")]
    x86_handoff_fixture();
    let finish_up = || {
        X86_BOOT_COMPLETE.store(true, Ordering::Release);
        log::info!(
            "[x86-smp] topology online=1 processor_ids={} qualified=yes",
            bsp_id
        );
    };
    // Logical 0 is the BSP. `CPU_TOPOLOGY` is the kernel's logical -> LAPIC map;
    // the HAL keeps its own write-once copy (`apic::register_cpu`) because its
    // IPI path cannot read kernel state. Both refuse a LAPIC ID above 255 and a
    // conflicting pair, so either refusal keeps the system uniprocessor.
    if !CPU_TOPOLOGY.publish(0, bsp_id as usize) || !hal::x86_64::apic::register_cpu(0, bsp_id) {
        // UP does not use APIC-directed remote invalidations.
        finish_up();
        return;
    }
    let Some(response) = limine::get_mp_response() else {
        finish_up();
        return;
    };
    let count = response.cpu_count as usize;
    // Response flag bit 0 = Limine enabled x2APIC; it must match this CPU.
    let x2apic_flag = response.flags & 1 != 0;
    if response.flags & !1 != 0
        || x2apic_flag != hal::x86_64::apic::x2apic_active()
        || response.bsp_lapic_id != bsp_id
        || count == 0
        || count > 256
        || !limine::mp_record_valid(response.cpus, count)
    {
        log::warn!("[x86-smp] unsupported MP topology; no AP activated");
        finish_up();
        return;
    }
    // Validate every record before any AP is released: one bad record keeps
    // the whole machine uniprocessor, as before.
    let mut ids = [false; 256];
    let mut processors = [0u32; 256];
    let mut bsp_seen = false;
    let mut secondaries: [*mut limine::LimineMpInfo; 256] = [core::ptr::null_mut(); 256];
    let mut secondary_count = 0;
    for index in 0..count {
        let info = unsafe { *response.cpus.add(index) };
        if !limine::mp_record_valid(info, 1) {
            finish_up();
            return;
        }
        let cpu = unsafe { &*info };
        let id = cpu.lapic_id as usize;
        if id > 255 {
            // Needs interrupt remapping for IOAPIC/MSI destinations (phase 12).
            log::warn!(
                "[x86-smp] cpu apic_id={} offline: needs interrupt remapping",
                cpu.lapic_id
            );
            continue;
        }
        if ids[id]
            || processors[..index].contains(&cpu.processor_id)
            || (cpu.lapic_id != bsp_id && cpu.goto_address.load(Ordering::Acquire) != 0)
        {
            log::warn!("[x86-smp] invalid MP CPU record; no AP activated");
            finish_up();
            return;
        }
        ids[id] = true;
        processors[index] = cpu.processor_id;
        if cpu.lapic_id == bsp_id {
            bsp_seen = true;
        } else {
            secondaries[secondary_count] = info;
            secondary_count += 1;
        }
    }
    if !bsp_seen || secondary_count == 0 {
        finish_up();
        return;
    }
    let kernel_cr3 = hal::domain::kernel_cr3();
    assert!(kernel_cr3 != 0, "AP requires permanent shared kernel root");
    // The BSP must receive each AP's reciprocal proof even if boot entered this
    // routine with interrupts masked. Restore its original state afterwards.
    let flags: usize;
    unsafe {
        core::arch::asm!("pushfq", "pop {}", "sti", out(reg) flags);
    }
    for &info in &secondaries[..secondary_count] {
        let (ap_id, processor_id) = unsafe { ((*info).lapic_id, (*info).processor_id) };
        // The next free logical index; the pair is published before the AP is
        // released, so the AP's own check in `x86_secondary_main` reads a
        // complete mapping.
        let logical = CPU_TOPOLOGY.count();
        if logical >= MAX_HARTS {
            log::warn!(
                "[x86-smp] cpu {} ignored: MAX_HARTS (apic_id={})",
                processor_id,
                ap_id
            );
            continue;
        }
        if !x86_start_ap(info, logical, ap_id, kernel_cr3) {
            break;
        }
    }
    let online = online_hart_count();
    let broadcast_ns = (online > 1).then(x86_broadcast_flush_proof);
    #[cfg(feature = "test-hooks")]
    if online > 1 {
        x86_shootdown_fixture_primary();
    }
    X86_BOOT_COMPLETE.store(true, Ordering::Release);
    if let Some(ack_ns) = broadcast_ns {
        log::info!("[x86-smp] transport IPI-invalidate bidirectional=pass");
        log::info!(
            "[x86-smp] transport IPI-invalidate broadcast targets={} ack_ns={}",
            online - 1,
            ack_ns
        );
    }
    log::info!(
        "[x86-smp] topology online={} processor_ids={} qualified=yes",
        online,
        X86OnlineApicIds { bsp_id }
    );
    if flags & (1 << 9) == 0 {
        unsafe {
            core::arch::asm!("cli", options(nomem, nostack));
        }
    }
}

/// Start one AP as `logical` and run the four-step handshake (see
/// `X86_READY`). Returns `false`, leaving that AP offline, when it cannot be
/// started or refuses root operation; the caller then stops bring-up.
pub(super) fn x86_start_ap(
    info: *mut crate::arch::boot::limine::LimineMpInfo,
    logical: usize,
    ap_id: u32,
    kernel_cr3: usize,
) -> bool {
    let stack = match crate::task::stack::Stack::new_kernel(crate::task::STACK_PAGES) {
        Ok(stack) => stack,
        Err(_) => {
            log::warn!(
                "[x86-smp] AP {} stack allocation failed; bring-up stops",
                ap_id
            );
            return false;
        }
    };
    if !CPU_TOPOLOGY.publish(logical, ap_id as usize)
        || !hal::x86_64::apic::register_cpu(logical, ap_id)
    {
        log::warn!(
            "[x86-smp] AP {} refused as logical {}; bring-up stops",
            ap_id,
            logical
        );
        return false;
    }
    let stack_top = stack.top;
    core::mem::forget(stack);
    extern "C" {
        fn vi_x86_secondary_entry();
    }
    let context = &X86_SECONDARY[logical];
    // Relaxed: the Release store to `goto_address` below publishes these.
    context.stack_top.store(stack_top, Ordering::Relaxed);
    context.kernel_cr3.store(kernel_cr3, Ordering::Relaxed);
    context.logical.store(logical, Ordering::Relaxed);
    context.lapic_id.store(ap_id as usize, Ordering::Relaxed);
    unsafe {
        (*info).extra_argument = core::ptr::from_ref(context) as u64;
        (*info).goto_address.store(
            vi_x86_secondary_entry as *const () as usize,
            Ordering::Release,
        );
    }
    // Step 1 answered by this AP (no earlier AP can still write these words:
    // it is online or parked before this one is released).
    x86_bounded_wait(|| {
        X86_READY.load(Ordering::Acquire) == logical
            || X86_AP_ROOT_REFUSED.load(Ordering::Acquire) == logical
    });
    if X86_AP_ROOT_REFUSED.load(Ordering::Acquire) == logical {
        log::warn!(
            "[x86-smp] AP {} refused x86 root operation; it stays offline",
            ap_id
        );
        return false;
    }
    // Step 2: this AP answers the BSP's IPI-driven invalidation.
    let epoch = request_tlb_flush(logical);
    x86_bounded_wait(|| tlb_flush_completed(logical, epoch));
    X86_QUALIFIED.store(logical, Ordering::Release);
    // Step 3.
    x86_bounded_wait(|| HART_ONLINE[logical].load(Ordering::Acquire));
    // Step 4: catch-up invalidation. Until `HART_ONLINE[logical]` was visible,
    // every shootdown (another AP's, or the BSP's own) excluded this AP from its
    // target set, so a translation changed between step 2 and step 3 may still
    // be cached here. One more acknowledged flush, requested only after the
    // BSP observed the AP online, closes that window: every later shootdown
    // targets it.
    let epoch = request_tlb_flush(logical);
    x86_bounded_wait(|| tlb_flush_completed(logical, epoch));
    true
}

/// One broadcast invalidation to every online AP at once, the shape every
/// later shootdown takes (`tlb_shootdown::await_remote_invalidation`).
/// Returns its acknowledgement latency in clock nanoseconds (0 without a
/// clocksource). The APs still spin in their boot-complete wait with IF set.
pub(super) fn x86_broadcast_flush_proof() -> usize {
    let started = hal::x86_64::clock::now_ns();
    let requested = request_tlb_flush_set(online_harts().filter(|hart| *hart != 0));
    x86_bounded_wait(|| tlb_flush_set_outstanding(&requested).is_none());
    hal::x86_64::clock::now_ns().saturating_sub(started) as usize
}

pub(super) extern "C" fn x86_secondary_main(
    logical: usize,
    lapic_id: usize,
    kernel_cr3: usize,
) -> ! {
    // Any AP slot below MAX_HARTS is admissible, but only on the CPU whose LAPIC
    // ID the BSP published for it before releasing this one.
    assert!(
        logical != 0 && logical < MAX_HARTS && logical_to_physical(logical) == Some(lapic_id),
        "[x86-smp] AP logical {} lapic {} is not the published topology",
        logical,
        lapic_id
    );
    if !unsafe { hal::x86_64::init_secondary_cpu(logical, lapic_id as u32, kernel_cr3) } {
        panic!("[x86-smp] active AP feature/isolation initialization failed");
    }
    crate::sched::hart_local::install(logical);
    // HypervisorCap is system-wide once the BSP latched root operation, so this
    // CPU must enter the same root operation (own HSAVE/VMXON frame) before it
    // may go online. A refused AP parks with IF clear and is never published.
    if crate::cpu_features::has_x86_virt() {
        if let Err(reason) = crate::arch::boot::cpu_features::enter_x86_root_on_current_cpu() {
            log::warn!(
                "[x86-smp] AP {} x86 root operation refused: {}",
                lapic_id,
                reason
            );
            X86_AP_ROOT_REFUSED.store(logical, Ordering::Release);
            loop {
                unsafe {
                    core::arch::asm!("cli", "hlt", options(nomem, nostack));
                }
            }
        }
    }
    // Establish this CPU's own idle activation before enabling delivery.
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack));
    }
    // Per-AP clock state: `init_secondary_cpu` ran the TSC skew check, armed
    // this CPU's LAPIC/TSC-deadline tick and, when the BSP uses kvmclock,
    // registered this vCPU's own pvclock slot (the MSR is per vCPU).
    log::info!(
        "[x86-smp] AP logical={} apic_id={} pvclock={}",
        logical,
        lapic_id,
        if hal::x86_64::clock::kvmclock::registered_area().is_some() {
            "own"
        } else {
            "none"
        }
    );
    // Step 1 (see `X86_READY`).
    let epoch = request_tlb_flush(0);
    x86_bounded_wait(|| tlb_flush_completed(0, epoch));
    X86_READY.store(logical, Ordering::Release);
    x86_bounded_wait(|| X86_QUALIFIED.load(Ordering::Acquire) >= logical);
    // Step 3.
    HART_ONLINE[logical].store(true, Ordering::Release);
    // Later APs are brought up while this one waits here. Each later AP costs
    // the BSP at most four bounded waits, plus the closing broadcast and
    // shootdown fixture, so five budgets per hart slot bound this wait.
    x86_bounded_wait_steps(X86_WAIT_STEPS.saturating_mul(5 * MAX_HARTS), || {
        // The shootdown fixture pairs the BSP with the RT hart only; any other
        // AP just answers its IPIs from here.
        #[cfg(feature = "test-hooks")]
        if logical == HART_RT {
            x86_shootdown_fixture_secondary();
        }
        X86_BOOT_COMPLETE.load(Ordering::Acquire)
    });
    loop {
        crate::task::yield_cpu();
        unsafe {
            core::arch::asm!("sti", "hlt", options(nomem, nostack));
        }
    }
}

// This behavioral fixture caches a genuinely GLOBAL shared translation on the
// AP, changes its binding on the BSP, then forces the AP to contend on the same
// page-table lock with IF clear. Success requires lock-spin local invalidation;
// an ISR-only acknowledgement deadlocks or retains the old binding. A second
// round rebinds the page while the AP, IF still clear, only takes that lock
// *uncontended* — a long masked kernel stretch such as a spawn reading its
// image. The BSP's shootdown must still be acknowledged before its probe
// expires: every acquisition, not only a contended one, is a service point.
#[cfg(feature = "test-hooks")]
pub(super) const X86_PROBE_VA: usize = 0x0600_0000;
#[cfg(feature = "test-hooks")]
pub(super) static X86_PROBE_STAGE: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "test-hooks")]
pub(super) static X86_PROBE_BEFORE: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "test-hooks")]
pub(super) static X86_PROBE_AFTER: AtomicUsize = AtomicUsize::new(0);

#[cfg(feature = "test-hooks")]
pub(super) fn x86_shootdown_fixture_secondary() {
    if X86_PROBE_STAGE.load(Ordering::Acquire) != 1 {
        return;
    }
    X86_PROBE_BEFORE.store(
        unsafe { core::ptr::read_volatile(X86_PROBE_VA as *const usize) },
        Ordering::Release,
    );
    X86_PROBE_STAGE.store(2, Ordering::Release);
    x86_bounded_wait(|| X86_PROBE_STAGE.load(Ordering::Acquire) == 3);
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack));
    }
    X86_PROBE_STAGE.store(4, Ordering::Release);
    let root = crate::memory::paging::KERNEL_ROOT.lock();
    X86_PROBE_AFTER.store(
        unsafe { core::ptr::read_volatile(X86_PROBE_VA as *const usize) },
        Ordering::Release,
    );
    drop(root);
    X86_PROBE_STAGE.store(5, Ordering::Release);
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack));
    }
    // Round two: VA -> new is cached here. With IF clear again, take the lock
    // only uncontended until the BSP's shootdown of the rebinding returns.
    x86_bounded_wait(|| X86_PROBE_STAGE.load(Ordering::Acquire) == 6);
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack));
    }
    X86_PROBE_STAGE.store(7, Ordering::Release);
    x86_bounded_wait(|| {
        drop(crate::memory::paging::KERNEL_ROOT.lock());
        X86_PROBE_STAGE.load(Ordering::Acquire) == 8
    });
    // Round one's BEFORE value is already checked; reuse the slot.
    X86_PROBE_BEFORE.store(
        unsafe { core::ptr::read_volatile(X86_PROBE_VA as *const usize) },
        Ordering::Release,
    );
    X86_PROBE_STAGE.store(9, Ordering::Release);
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack));
    }
}

#[cfg(feature = "test-hooks")]
pub(super) fn x86_shootdown_fixture_primary() {
    use crate::memory::frame::phys_to_virt;
    use crate::memory::paging::{table_at, Flags, KERNEL_ROOT, PAGE_SIZE, SAS_TABLES};
    use hal::PageTableTrait;
    // A global leaf: the shootdown must reach translations a CR3 reload keeps.
    let flags = Flags::from_bits(Flags::VALID | Flags::READ | Flags::WRITE | Flags::GLOBAL);
    // Re-point the probe leaf in place under the held KERNEL_ROOT lock; the
    // fixture issues the invalidation itself.
    let repoint = |root: Option<usize>, frame: usize| {
        // SAFETY: the kernel root, edited under its lock (held by the caller).
        let table = unsafe { &mut *table_at(root.expect("fixture root")) };
        table
            .map_with(X86_PROBE_VA, frame, flags, SAS_TABLES, &mut || None)
            .expect("fixture leaf");
    };
    // The fixture leaf is new (asserted absent below), so mapping it needs no
    // shared invalidation; the batch stays empty.
    let mut batch = crate::memory::frame_run::Batch::new();
    let (old, new) = crate::memory::frame::with(|allocator| {
        let old = allocator.allocate_frame().expect("fixture old frame");
        let new = allocator
            .allocate_frame()
            .expect("fixture replacement frame");
        unsafe {
            core::ptr::write_volatile(phys_to_virt(old) as *mut usize, 0x1357);
            core::ptr::write_volatile(phys_to_virt(new) as *mut usize, 0x2468);
        }
        let root = KERNEL_ROOT.lock();
        // SAFETY: the kernel root, walked read-only under its lock.
        let table = unsafe { &*table_at(root.expect("fixture root")) };
        assert!(
            table.leaf_entry(X86_PROBE_VA).is_none(),
            "shootdown fixture VA already mapped"
        );
        drop(root);
        crate::memory::paging::map_page(allocator, &mut batch, X86_PROBE_VA, old, flags)
            .expect("fixture map");
        (old, new)
    })
    .expect("fixture frame allocator");
    assert!(batch.complete().is_confirmed(), "fixture map invalidation");
    let mut cap = crate::memory::frame::FlushCap::take().expect("fixture flush cap");
    X86_PROBE_STAGE.store(1, Ordering::Release);
    x86_bounded_wait(|| X86_PROBE_STAGE.load(Ordering::Acquire) == 2);
    {
        let root = KERNEL_ROOT.lock();
        repoint(*root, new);
        X86_PROBE_STAGE.store(3, Ordering::Release);
        x86_bounded_wait(|| X86_PROBE_STAGE.load(Ordering::Acquire) == 4);
        let outcome = crate::memory::tlb_shootdown::flush_range(&mut cap, X86_PROBE_VA, PAGE_SIZE);
        assert!(outcome.is_confirmed(), "fixture shootdown unconfirmed");
    }
    x86_bounded_wait(|| X86_PROBE_STAGE.load(Ordering::Acquire) == 5);
    assert_eq!(X86_PROBE_BEFORE.load(Ordering::Acquire), 0x1357);
    assert_eq!(X86_PROBE_AFTER.load(Ordering::Acquire), 0x2468);
    // Round two: rebind to `old` and shoot it down while the AP holds IF clear
    // and never contends. The fixture fails loudly if the AP stays silent.
    {
        let root = KERNEL_ROOT.lock();
        repoint(*root, old);
    }
    X86_PROBE_STAGE.store(6, Ordering::Release);
    x86_bounded_wait(|| X86_PROBE_STAGE.load(Ordering::Acquire) == 7);
    let outcome = crate::memory::tlb_shootdown::flush_range(&mut cap, X86_PROBE_VA, PAGE_SIZE);
    assert!(
        outcome.is_confirmed(),
        "fixture masked shootdown unconfirmed"
    );
    X86_PROBE_STAGE.store(8, Ordering::Release);
    x86_bounded_wait(|| X86_PROBE_STAGE.load(Ordering::Acquire) == 9);
    assert_eq!(X86_PROBE_BEFORE.load(Ordering::Acquire), 0x1357);
    let mut batch = crate::memory::frame_run::Batch::new();
    crate::memory::paging::unmap_page(&mut batch, X86_PROBE_VA).expect("fixture unmap");
    assert!(
        batch.complete().is_confirmed(),
        "fixture unmap invalidation"
    );
    crate::memory::frame::deallocate_frame(old);
    crate::memory::frame::deallocate_frame(new);
    // Both shootdowns went to every online AP; the RT hart is the one that
    // held IF clear and observed the binding.
    let harts = online_hart_count();
    log::info!("S22-X86-REMOTE-SHOOTDOWN: PASS harts={} global=true irq_masked_lock=true before=4951 after=9320", harts);
    log::info!(
        "S22-X86-MASKED-SHOOTDOWN: PASS harts={} irq_masked_uncontended_lock=true after=4951",
        harts
    );
}

#[cfg(feature = "test-hooks")]
pub(super) static mut X86_HANDOFF_CONTEXTS: [core::mem::MaybeUninit<hal::arch::Context>; 2] =
    [core::mem::MaybeUninit::uninit(); 2];
#[cfg(feature = "test-hooks")]
pub(super) static X86_HANDOFF_FIRST_EPOCH: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "test-hooks")]
pub(super) static X86_HANDOFF_LATE_EPOCH: AtomicUsize = AtomicUsize::new(0);

#[cfg(feature = "test-hooks")]
core::arch::global_asm!(
    ".global vi_x86_handoff_fixture_entry",
    "vi_x86_handoff_fixture_entry:",
    "endbr64",
    "jmp {entry}",
    entry = sym x86_handoff_fixture_incoming,
);

#[cfg(feature = "test-hooks")]
pub(super) extern "C" fn x86_handoff_fixture_incoming() -> ! {
    use crate::sched::hart_local::ready;
    assert_eq!(ready::selected_task_id_for(0), 0);
    assert_eq!(ready::executing_task_id_for(0), 90_002);
    assert_eq!(ready::outgoing_context_save_task_id_for(0), 0);
    assert!(!ready::owned_by_another_hart(1, 90_001));
    assert!(retirement_switch_completed(
        0,
        X86_HANDOFF_FIRST_EPOCH.load(Ordering::Acquire)
    ));
    assert!(!retirement_switch_completed(
        0,
        X86_HANDOFF_LATE_EPOCH.load(Ordering::Acquire)
    ));
    ready::steal_from_busiest(1);
    assert_eq!(ready::pick_local_eligible(1), Some(90_001));
    ready::begin_outgoing_context_save(0, 90_002);
    capture_retirement_boundary(0);
    let contexts = (&raw mut X86_HANDOFF_CONTEXTS).cast::<hal::arch::Context>();
    unsafe {
        hal::arch::Context::switch(contexts.add(1), contexts);
    }
    panic!("handoff fixture unexpectedly re-entered");
}

/// A real raw switch proves the outgoing Ready Context cannot be stolen before
/// save, becomes stealable on the incoming stack, and cannot acknowledge a
/// retirement requested after selection. Synthetic TIDs never enter SCHEDULER.
#[cfg(feature = "test-hooks")]
pub(super) fn x86_handoff_fixture() {
    use crate::sched::hart_local::ready;
    let stack = crate::task::stack::Stack::new_kernel(2).expect("handoff fixture stack");
    let flags = hal::arch::save_and_disable_interrupts();
    assert_eq!(ready::total_ready_count(), 0);
    let contexts = (&raw mut X86_HANDOFF_CONTEXTS).cast::<hal::arch::Context>();
    extern "C" {
        fn vi_x86_handoff_fixture_entry();
    }
    unsafe {
        contexts.write(hal::arch::Context::default());
        contexts.add(1).write(hal::arch::Context::default());
        (*contexts.add(1)).sp = (stack.top - 8) as u64;
        (*contexts.add(1)).rip = vi_x86_handoff_fixture_entry as *const () as u64;
        core::ptr::write_volatile((stack.top - 8) as *mut usize, 0);
    }
    ready::set_current_task_id(0, 90_001);
    ready::set_executing_task_id(0, 90_001);
    ready::begin_outgoing_context_save(0, 90_001);
    ready::push_on_hart(0, 90_001, abi::TaskPriority::Normal as u8);
    X86_HANDOFF_FIRST_EPOCH.store(request_retirement_switch(0), Ordering::Release);
    capture_retirement_boundary(0);
    ready::set_current_task_id(0, 90_002);
    ready::set_selected_task_id(0, 90_002);
    X86_HANDOFF_LATE_EPOCH.store(request_retirement_switch(0), Ordering::Release);
    ready::steal_from_busiest(1);
    assert_eq!(ready::pick_local_eligible(1), None);
    assert!(ready::owned_by_another_hart(1, 90_001));
    unsafe {
        hal::arch::Context::switch(contexts, contexts.add(1));
    }
    assert_eq!(ready::current_task_id_for(0), 0);
    assert_eq!(ready::executing_task_id_for(0), 0);
    assert!(retirement_switch_completed(
        0,
        X86_HANDOFF_LATE_EPOCH.load(Ordering::Acquire)
    ));
    assert_eq!(ready::total_ready_count(), 0);
    unsafe {
        hal::arch::restore_sstatus(flags);
    }
    drop(stack);
    log::info!("S22-X86-CONTEXT-HANDOFF: PASS raw_switch=true save_guard=true steal_after_save=true late_retirement_deferred=true");
}

// The HAL owns one GS record, GDT/TSS and IST set per logical CPU.
const _: () = assert!(MAX_HARTS <= hal::x86_64::syscall::MAX_CPUS);

pub(super) static RETIRE_SWITCH_INFLIGHT: [AtomicUsize; MAX_HARTS] =
    [const { AtomicUsize::new(0) }; MAX_HARTS];

/// Capture before scheduler selection: a later request may concern the incoming
/// task and must wait for that task's next switch-away.
pub fn capture_retirement_boundary(hart: usize) {
    RETIRE_SWITCH_INFLIGHT[hart].store(
        RETIRE_SWITCH_REQUEST[hart].load(Ordering::Acquire),
        Ordering::Release,
    );
}

pub fn complete_captured_retirement_boundary(hart: usize) {
    let epoch = RETIRE_SWITCH_INFLIGHT[hart].swap(0, Ordering::AcqRel);
    RETIRE_SWITCH_COMPLETE[hart].fetch_max(epoch, Ordering::Release);
}
