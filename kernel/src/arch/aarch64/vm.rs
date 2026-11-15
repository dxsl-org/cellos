// SPDX-License-Identifier: MPL-2.0
//! `vm`: the aarch64 VM registry behind `crate::hypervisor::registry` —
//! per-incarnation RAII store for Stage-2 tables, guest RAM, and vCPUs.
//!
//! # Lock order
//! `VM_REGISTRY` → `FRAME_ALLOCATOR` (same order as grant reaper; never reverse).

use crate::arch::hypervisor::pending_irqs::PendingIrqs;
use crate::hypervisor::registry::{next_vm_id, VmCreateError, VmKey};
use crate::memory::cell_quota::{Account, Charge, Reservation};
use crate::memory::stage2::Stage2Table;
use crate::sync::Spinlock;
use abi::hypervisor::ViVmExit as ApiVmExit;
use alloc::{collections::BTreeMap, vec::Vec};
use hal::aarch64::backend::{ArmEl2Monitor, ArmResources, Board, El2Monitor, VgicWindows};
use hal::aarch64::{
    id_regs::read_trapped_id_reg,
    stage2_regs::{
        clear_stage2_root, disable_stage2, enable_stage2, load_stage2, s2_tlb_flush_all, VmidPool,
    },
    vcpu::AArch64Vcpu,
};
use hal::aarch64::{vcpu::run_vcpu_impl, vgic};
use hal::ViVmExit as HalVmExit;
use types::{ViError, ViResult};

/// The host reaches EL2 through the board's EL2 monitor (the Pi: guest IRQs
/// via `HCR_EL2.VI`, no GICV) instead of running at EL2 itself (guest IRQs via
/// the GICH list registers, GICV passed through).
const PI_MONITOR: bool = El2Monitor::PRESENT;

/// The board's GICv2 virtualization windows; `None` leaves guests without GICH
/// list-register injection or GICV passthrough (the Pi 3 monitor uses
/// `HCR_EL2.VI` instead).
const VGIC: Option<VgicWindows> = <Board as ArmResources>::VGIC;

/// Guest IPA of the guest's GICC, as the hypervisor cell's DTB declares it.
const GUEST_GICC_IPA: u64 = 0x0801_0000;

/// Guest virtual-timer PPI (the guest DTB's `arm,armv8-timer` virtual entry).
/// Level-sensitive: it is sampled before every entry, never queued.
const GUEST_VTIMER_PPI: u32 = 27;

/// The guest's virtual timer line, against the virtual count with the current
/// `CNTVOFF_EL2` applied (see [`vgic::vtimer_line_asserted`]).
fn guest_vtimer_asserted(ctl: u64, cval: u64) -> bool {
    vgic::vtimer_line_asserted(ctl, cval, hal::aarch64::timer::virtual_count_now())
}

struct Vm {
    stage2: Stage2Table,
    guest_pa: u64,
    guest_pages: usize,
    vcpus: Vec<AArch64Vcpu>,
    /// Per-vCPU pending virtual IRQ set; intids set by inject_irq, drained into
    /// GICH LRs just before each run_vcpu_impl call (Phase 09). Fixed-size
    /// coalescing bitset — see `pending_irqs::PendingIrqs` for why this isn't
    /// a queue.
    vcpu_irqs: Vec<PendingIrqs>,
    /// Per-vCPU GICv2 virtual CPU interface state (VMCR, APR, Active LRs),
    /// installed in GICH around each guest run on hosts without the Pi monitor.
    vcpu_vgic: Vec<vgic::CpuIfState>,
    /// The VM's Stage-2 VMID, loaded into `VTTBR_EL2` with its root on every
    /// guest entry (directly at an EL2 host, by the Pi monitor's run op).
    /// Taken from [`VMIDS`]; `reap_vms_for` returns it.
    vmid: u16,
    /// Set once the guest RAM window has been cleaned to PoC and the I-cache
    /// invalidated, before the first vCPU entry.
    entry_flushed: bool,
}

// VM_REGISTRY is keyed by the owner incarnation and a boot-wide VM id
// (`registry::next_vm_id`).
static VM_REGISTRY: Spinlock<Option<BTreeMap<VmKey, Vm>>> =
    Spinlock::ranked(None, crate::sync::rank::VM_REGISTRY);

/// Guest VMIDs within the 8-bit width `VTCR_EL2` configures. A VMID goes back
/// only after `TLBI ALLE1IS` dropped every entry tagged with it, and binding it
/// again invalidates it once more (`enable_stage2`).
static VMIDS: VmidPool = VmidPool::new();

fn registry_lock() -> &'static Spinlock<Option<BTreeMap<VmKey, Vm>>> {
    &VM_REGISTRY
}

/// The guest charge `create_vm` takes for a VM of `guest_pages`, before any
/// frame: the guest RAM, the Stage-2 root pair, and the most L2/L3 tables the
/// guest-RAM window and the GICV passthrough can need. A vCPU's state is heap,
/// not frames, so vCPUs add nothing. `None` when the sum overflows, which no
/// budget covers.
pub(crate) fn guest_bytes(guest_pages: usize) -> Option<usize> {
    use crate::memory::paging::PAGE_SIZE;
    let gicv_tables = VGIC.map_or(0, |windows| {
        Stage2Table::table_bound(windows.gicv.size / PAGE_SIZE)
    });
    guest_pages
        .checked_add(Stage2Table::table_bound(guest_pages))?
        .checked_add(gicv_tables)?
        .checked_add(Stage2Table::ROOT_PAGES)?
        .checked_mul(PAGE_SIZE)
}

/// Guest VMIDs free now (the `VM-GUEST-BUDGET` fixture's tag snapshot).
#[cfg(not(cellos_posture = "release"))]
pub(crate) fn free_vm_tags() -> usize {
    VMIDS.free_count()
}

/// The host the kernel's VM fixtures run on: an EL2 host (not the Pi, whose
/// monitor runs guests on the kernel's behalf).
#[cfg(not(cellos_posture = "release"))]
pub(crate) fn guest_fixture_host() -> bool {
    !PI_MONITOR && hal::aarch64::el2::is_el2()
}

/// Boot fixture `VM-GUEST-BUDGET` (census §8.6(b)), shared with x86.
#[cfg(not(cellos_posture = "release"))]
#[path = "../guest_budget_selftest.rs"]
pub(crate) mod guest_budget_selftest;

/// Pi's EL1-host capability gate: an actual monitor round-trip must run guest
/// HVC, WFI, MMIO and software-injected virtual IRQ before any VM is admitted.
/// Called only when the board has an EL2 monitor (`El2Monitor::PRESENT`).
pub fn pi_monitor_smoke() {
    use crate::memory::frame::phys_to_virt;
    use hal::aarch64::cache::{
        clean_data_cache_range, invalidate_instruction_cache_all, sync_instruction_cache,
    };

    if !El2Monitor::is_ready() {
        log::warn!(
            "[pi-monitor] entry EL={} HVC init unavailable; HypervisorCap closed",
            El2Monitor::entry_el()
        );
        return;
    }
    const IPA: u64 = 0x4000_0000;
    // A kernel-owned VM: its guest charge is never limited or counted.
    let Some(mut table) = Stage2Table::new(Reservation::kernel(Account::Guest)) else {
        log::warn!("[pi-monitor] smoke: no Stage-2 root; HypervisorCap closed");
        return;
    };
    let Some(guest_pa) = table.carve_guest_ram(1) else {
        log::warn!("[pi-monitor] smoke: no guest page; HypervisorCap closed");
        return;
    };
    if table.map(IPA, guest_pa, 1, true).is_err() {
        log::warn!("[pi-monitor] smoke: guest map failed; HypervisorCap closed");
        return;
    }
    // The monitor binds and invalidates the VMID on every run (gateway op 5).
    let Some(vmid) = VMIDS.allocate() else {
        log::warn!("[pi-monitor] smoke: no free VMID; HypervisorCap closed");
        return;
    };
    let page = phys_to_virt(guest_pa as usize) as *mut u32;
    // MOVZ X0,#42; HVC #0; WFI; MOVZ X0,#0x900,LSL#16;
    // STR X1,[X0]; HVC #0. The final HVC bounds a failure if MMIO did not
    // trap instead of leaving an unbounded guest loop.
    let blob = [
        0xD280_0540,
        0xD400_0002,
        0xD503_201F,
        0xD2A1_2000,
        0xF900_0001,
        0xD400_0002,
    ];
    unsafe {
        core::ptr::copy_nonoverlapping(blob.as_ptr(), page, blob.len());
        // EL1h IRQ vector = VBAR_EL1+0x280; HVC reports that vector was reached.
        page.add(0x280 / 4).write(0xD400_0002);
        // +0x300: `B .` — a guest that never yields, for the preemption step.
        page.add(0x300 / 4).write(0x1400_0000);
        sync_instruction_cache(page as usize, page as usize, 4096);
        // The guest fetches this page through Stage-2, i.e. a different VA and
        // ASID, so the per-VA invalidate above does not cover the alias. Clean
        // the page to the point of coherency and drop the whole instruction
        // cache: on real hardware (not in TCG, which has no caches) a guest that
        // fetched a stale alias faulted at its first instruction while EL2's own
        // read of the same page showed the correct blob.
        clean_data_cache_range(page as usize, 4096);
        invalidate_instruction_cache_all();
    }
    let mut vcpu = AArch64Vcpu::new(IPA);
    // These markers distinguish an unobserved EL2 write from a genuine trap
    // whose syndrome and PC happen to be zero.
    vcpu.exit_esr = 0x0053_4d4f_4b45_5352;
    vcpu.exit_elr = 0x0053_4d4f_4b45_5043;
    // Boot has DAIF.I set: defer the existing, still-routed physical timers
    // for a bounded 100 ms guest smoke window. Otherwise their pre-boot
    // pending IRQs can turn a conditional WFI into a NOP. Production vCPU
    // runs retain the regular 10 ms host-tick preemption.
    let prepare = El2Monitor::prepare_smoke_window;
    // A physical tick can still race guest entry. Retry only genuine
    // preemptions, bounded: a permanently asserted unrelated IRQ fails closed.
    let run_step = |cpu: &mut AArch64Vcpu, vi: bool| {
        for _ in 0..16 {
            prepare();
            unsafe {
                El2Monitor::run(cpu, vmid, table.root_pa(), vi);
            }
            if !matches!(cpu.decode_exit(), HalVmExit::Preempted) {
                return;
            }
        }
    };
    let passed = {
        log::info!(
            "[pi-monitor] first-run EL1 entry={:#x} insn={:#x} esr-marker={:#x} elr-marker={:#x}",
            vcpu.g_elr_el2,
            unsafe { core::ptr::read_volatile(page) },
            vcpu.exit_esr,
            vcpu.exit_elr
        );
        prepare();
        let first = unsafe {
            El2Monitor::run_probed(&mut vcpu, vmid, table.root_pa(), false, IPA, guest_pa)
        };
        log::info!(
            "[pi-monitor] first-run root={:#x} page={:#x} EL2 entry={:#x} s2=[{:#x},{:#x},{:#x}] insn={:#x} exit=[{:#x},{:#x},irq={}] EL1 exit=[{:#x},{:#x},irq={}]",
            table.root_pa(), guest_pa, first.entry_pc, first.stage2_l1,
            first.stage2_l2, first.stage2_l3, first.instruction,
            first.exit_esr, first.exit_elr, first.exit_is_irq,
            vcpu.exit_esr, vcpu.exit_elr, vcpu.exit_is_irq
        );
        if matches!(vcpu.decode_exit(), HalVmExit::Preempted) {
            run_step(&mut vcpu, false);
        }
        let hvc = matches!(vcpu.decode_exit(), HalVmExit::Hvc { imm: 0, regs } if regs[0] == 42);
        if !hvc {
            // EL1's own view of the same words EL2 reported: if they differ the
            // two regimes disagree about memory; if they agree, the divergence is
            // in what the *hardware* walk/fetch saw, not in what software reads.
            let el1_root_leaf = unsafe {
                core::ptr::read_volatile(phys_to_virt(table.root_pa() as usize) as *const u64)
            };
            let el1_insn = unsafe { core::ptr::read_volatile(page) };
            log::warn!(
                "[pi-monitor] HVC exit={:?} pc={:#x} esr={:#x} EL1 leaf0={:#x} EL1 insn={:#x}",
                vcpu.decode_exit(),
                vcpu.exit_elr,
                vcpu.exit_esr,
                el1_root_leaf,
                el1_insn
            );
            false
        } else {
            // Step 2 runs the guest's WFI. Whether it produces a `Wfi` trap is
            // emulator/hardware dependent: QEMU's AArch64 WFI helper returns
            // before the HCR_EL2.TWI trap whenever `cpu_has_work()` is true
            // (target/arm/tcg/op_helper.c), so under QEMU-TCG the WFI can be an
            // in-block NOP. Measured on raspi3b: the WFI is a NOP and the guest
            // runs straight into the MMIO store. The gate therefore requires
            // forward progress past the WFI — either exit — and logs which one.
            run_step(&mut vcpu, false);
            let (mmio_ok, wfi_trapped) = match vcpu.decode_exit() {
                HalVmExit::Wfi => {
                    run_step(&mut vcpu, false);
                    (
                        matches!(
                            vcpu.decode_exit(),
                            HalVmExit::MmioWrite {
                                ipa: 0x0900_0000,
                                size: 8,
                                ..
                            }
                        ),
                        true,
                    )
                }
                HalVmExit::MmioWrite {
                    ipa: 0x0900_0000,
                    size: 8,
                    ..
                } => (true, false),
                _ => (false, false),
            };
            if !mmio_ok {
                log::warn!(
                    "[pi-monitor] WFI/MMIO exit={:?} pc={:#x} esr={:#x} far={:#x} hpfar={:#x}",
                    vcpu.decode_exit(),
                    vcpu.exit_elr,
                    vcpu.exit_esr,
                    vcpu.exit_far,
                    vcpu.exit_hpfar
                );
                let soc = hal_soc_bcm27xx::BCM2837;
                // SAFETY: both controller apertures are identity-mapped before
                // IRQs are enabled and the smoke runs after paging activation.
                let (
                    basic,
                    pending1,
                    pending2,
                    enable1,
                    enable2,
                    core_src,
                    core_fiq,
                    core_timers,
                    cntp_ctl,
                ) = unsafe {
                    (
                        core::ptr::read_volatile(soc.mmio.legacy_irq_base as *const u32),
                        core::ptr::read_volatile((soc.mmio.legacy_irq_base + 0x04) as *const u32),
                        core::ptr::read_volatile((soc.mmio.legacy_irq_base + 0x08) as *const u32),
                        core::ptr::read_volatile((soc.mmio.legacy_irq_base + 0x10) as *const u32),
                        core::ptr::read_volatile((soc.mmio.legacy_irq_base + 0x14) as *const u32),
                        core::ptr::read_volatile(
                            (soc.mmio.local_controller_base + 0x60) as *const u32,
                        ),
                        core::ptr::read_volatile(
                            (soc.mmio.local_controller_base + 0x70) as *const u32,
                        ),
                        core::ptr::read_volatile(
                            (soc.mmio.local_controller_base + 0x40) as *const u32,
                        ),
                        crate::arch::clock::cntp_ctl(),
                    )
                };
                log::warn!(
                    "[pi-monitor]   irq: basic={:#x} pend1={:#x} pend2={:#x} en1={:#x} en2={:#x}",
                    basic,
                    pending1,
                    pending2,
                    enable1,
                    enable2
                );
                log::warn!(
                    "[pi-monitor]   core: irq_src={:#x} fiq_src={:#x} timers={:#x} cntp_ctl={:#x}",
                    core_src,
                    core_fiq,
                    core_timers,
                    cntp_ctl
                );
                false
            } else {
                log::info!(
                    "[pi-monitor] WFI {}",
                    if wfi_trapped {
                        "trapped (TWI honoured)"
                    } else {
                        "NOP (emulator helper)"
                    }
                );
                let mut irq_cpu = AArch64Vcpu::new(IPA);
                irq_cpu.g_vbar_el1 = IPA;
                irq_cpu.g_spsr_el2 &= !(1 << 7); // unmask guest IRQ
                run_step(&mut irq_cpu, true);
                // The vIRQ must be taken before the guest's first instruction:
                // PC becomes VBAR_EL1 + 0x280 and that vector's `HVC #0` runs.
                // ELR_EL2 for a natively executed HVC is HVC+4, so the exit PC
                // is IPA + 0x284 — proof the guest entered its own IRQ vector.
                let irq = matches!(irq_cpu.decode_exit(), HalVmExit::Hvc { imm: 0, .. })
                    && irq_cpu.exit_elr == IPA + 0x284;
                if !irq {
                    log::warn!(
                        "[pi-monitor] VI exit={:?} pc={:#x} esr={:#x} spsr={:#x}",
                        irq_cpu.decode_exit(),
                        irq_cpu.exit_elr,
                        irq_cpu.exit_esr,
                        irq_cpu.g_spsr_el2
                    );
                    false
                } else {
                    // A guest that never yields must still be preempted by the
                    // host tick; otherwise a runaway guest owns the CPU.
                    let mut spin_cpu = AArch64Vcpu::new(IPA);
                    spin_cpu.g_elr_el2 = IPA + 0x300;
                    El2Monitor::arm_smoke_preemption();
                    unsafe {
                        El2Monitor::run(&mut spin_cpu, vmid, table.root_pa(), false);
                    }
                    let preempted = matches!(spin_cpu.decode_exit(), HalVmExit::Preempted);
                    if !preempted {
                        log::warn!(
                            "[pi-monitor] PREEMPT exit={:?} pc={:#x} esr={:#x}",
                            spin_cpu.decode_exit(),
                            spin_cpu.exit_elr,
                            spin_cpu.exit_esr
                        );
                    }
                    preempted
                }
            }
        }
    };
    // Flush before freeing the Stage-2 frames even on a failed smoke. The
    // monitor cleared VTTBR_EL2 after the last run, so `disable_stage2` flushes
    // no guest VMID: drop every VMID's entries before the frames and the VMID
    // go back.
    // SAFETY: the monitor is ready (checked on entry) and no vCPU is running.
    unsafe {
        disable_stage2();
        s2_tlb_flush_all();
    }
    VMIDS.release(vmid);
    // Restore the ordinary host tick cadence before Cells can run.
    El2Monitor::restore_host_tick();
    if passed {
        El2Monitor::mark_verified();
        log::info!("[pi-monitor] HVC/MMIO/VI/PREEMPT smoke PASS; HypervisorCap open");
    } else {
        log::warn!("[pi-monitor] HVC/MMIO/VI/PREEMPT smoke FAILED; HypervisorCap closed");
    }
}

// ── Registry operations (`crate::hypervisor::registry`) ──────────────────────

pub(crate) fn create_vm(owner: Charge, guest_pages: usize) -> Result<usize, VmCreateError> {
    build_vm(owner, guest_pages, false)
}

/// [`create_vm`]; `refusal_expected` (the VMID fixture exhausting the pool on
/// purpose) logs an exhausted pool at info, tagged `(fixture)`, not at warn.
fn build_vm(
    owner: Charge,
    guest_pages: usize,
    refusal_expected: bool,
) -> Result<usize, VmCreateError> {
    if PI_MONITOR && !El2Monitor::is_verified() {
        // The cell can only print one sentence for every failure, so the
        // reason has to be here: a refused boot monitor and a failed carve
        // are different bugs with the same symptom on the console.
        log::warn!(
            "[hv] create_vm refused: EL2 monitor not verified (ready={})",
            El2Monitor::is_ready()
        );
        return Err(ViError::NotSupported.into());
    }
    use crate::memory::paging::PAGE_SIZE;

    // The first effect: the owner's guest charge for every frame the VM can
    // take. A refusal allocates nothing; every later frame is drawn from it,
    // and an error below frees what was drawn and refunds the whole charge.
    let budget = guest_bytes(guest_pages)
        .and_then(|bytes| Reservation::take(owner, Account::Guest, bytes).ok())
        .ok_or_else(|| {
            log::warn!(
                "[hv] create_vm refused: {} guest pages exceed cell {}'s guest budget",
                guest_pages,
                owner.cell().0
            );
            VmCreateError::Budget
        })?;
    let mut table = Stage2Table::new(budget).ok_or_else(|| {
        log::error!("[hv] create_vm: stage-2 root allocation failed");
        ViError::OutOfMemory
    })?;
    let guest_pa = table
        .carve_guest_ram(guest_pages)
        .ok_or_else(|| {
            let (total_mib, used_mib, largest_mib) = {
                crate::memory::frame::with(|allocator| {
                    (
                        allocator.total_memory() / (1024 * 1024),
                        allocator.used_memory() / (1024 * 1024),
                        allocator.largest_free_run() / 256,
                    )
                })
                .unwrap_or((0, 0, 0))
            };
            log::error!(
                "[hv] create_vm: no contiguous guest run ({} pages, {} MiB; allocator {} MiB total, {} MiB used, largest free run {} MiB)",
                guest_pages,
                guest_pages / 256,
                total_mib,
                used_mib,
                largest_mib
            );
            ViError::OutOfMemory
        })?;
    // Map all guest RAM at IPA 0x40000000.
    table
        .map(0x4000_0000, guest_pa, guest_pages, true)
        .map_err(|error| {
            log::error!("[hv] create_vm: guest RAM stage-2 map failed: {:?}", error);
            ViError::OutOfMemory
        })?;
    // GICV is passed through only on boards that declare it (QEMU virt).
    // Pi GICC/GICD are software-MMIO devices and must remain unmapped
    // Stage-2 holes. GICV is writable: the guest enables its interface
    // (GICC_CTLR, GICC_PMR) and completes every interrupt (GICC_EOIR)
    // through it, and those writes reach only the running vCPU's GICH
    // state, which `vgic::CpuIfState` switches per vCPU.
    if let Some(windows) = VGIC {
        table
            .map_mmio_passthrough(
                GUEST_GICC_IPA,
                windows.gicv.base as u64,
                windows.gicv.size / PAGE_SIZE,
                true,
            )
            .map_err(|error| {
                log::error!("[hv] create_vm: GICV stage-2 map failed: {:?}", error);
                ViError::OutOfMemory
            })?;
    }

    if let Some(windows) = VGIC {
        // SAFETY: EL2 host; the board's GICH window is kernel-mapped.
        unsafe {
            vgic::enable(windows.gich_base);
        }
    }

    // The table, never bound, is freed on refusal; no TLB entry names it.
    let vmid = VMIDS.allocate().ok_or_else(|| {
        if refusal_expected {
            log::info!("[hv] create_vm refused (fixture): all 255 VMIDs in use");
        } else {
            log::warn!("[hv] create_vm refused: all 255 VMIDs in use");
        }
        ViError::OutOfMemory
    })?;
    let vm_id = {
        let mut guard = registry_lock().lock();
        // Invalidate whatever the VMID's previous VM left cached (its teardown
        // already did, before it returned the VMID), then leave this CPU
        // naming no root: every guest entry programs its own VM's root
        // (`run_vcpu`), and a root this CPU still named could be freed under
        // it. Under the registry lock (interrupts masked), so the clear runs on
        // the CPU the bind did.
        // SAFETY: table built and flushed; vmid in 1..=255; no vCPU of it
        // exists yet.
        unsafe {
            enable_stage2(vmid, table.root_pa());
            clear_stage2_root();
        }
        if guard.is_none() {
            *guard = Some(BTreeMap::new());
        }
        let map = guard.as_mut().unwrap();
        let id = next_vm_id();
        map.insert(
            VmKey::new(owner, id),
            Vm {
                stage2: table,
                guest_pa,
                guest_pages,
                vcpus: Vec::new(),
                vcpu_irqs: Vec::new(),
                vcpu_vgic: Vec::new(),
                vmid,
                entry_flushed: false,
            },
        );
        id
    };
    Ok(vm_id)
}

pub(crate) fn create_vcpu(owner: Charge, vm_id: usize, entry_pc: u64) -> ViResult<usize> {
    let mut guard = registry_lock().lock();
    let map = guard.as_mut().ok_or(ViError::NotFound)?;
    let vm = map
        .get_mut(&VmKey::new(owner, vm_id))
        .ok_or(ViError::NotFound)?;
    let vcpu_id = vm.vcpus.len() + 1;

    // test-hooks: write P04 HVC smoke blob so the test cell can verify Hvc exit.
    #[cfg(feature = "test-hooks")]
    {
        const MOVZ_X0_42: u32 = 0xD280_0540; // MOVZ X0, #42
        const HVC_0: u32 = 0xD400_0002; // HVC #0
        const B_DOT: u32 = 0x1400_0000; // B .
        const GUEST_IPA_BASE: u64 = 0x4000_0000;
        let offset = (entry_pc - GUEST_IPA_BASE) as usize;
        let blob_pa = vm.guest_pa as usize + offset;
        // SAFETY: guest RAM is kernel-allocated identity-mapped memory; no active vCPU yet.
        unsafe {
            let ptr = blob_pa as *mut u32;
            ptr.write(MOVZ_X0_42);
            ptr.add(1).write(HVC_0);
            ptr.add(2).write(B_DOT);
        }
    }

    vm.vcpus.push(AArch64Vcpu::new(entry_pc));
    vm.vcpu_irqs.push(PendingIrqs::new());
    vm.vcpu_vgic.push(vgic::CpuIfState::new());
    Ok(vcpu_id)
}

pub(crate) fn map_guest_memory(
    owner: Charge,
    vm_id: usize,
    ipa: u64,
    size: usize,
    writable: bool,
) -> ViResult<()> {
    use crate::memory::paging::PAGE_SIZE;
    let pages = size.div_ceil(PAGE_SIZE);
    let mut guard = registry_lock().lock();
    let map = guard.as_mut().ok_or(ViError::NotFound)?;
    let vm = map
        .get_mut(&VmKey::new(owner, vm_id))
        .ok_or(ViError::NotFound)?;
    // Extend guest RAM mapping to cover the requested IPA range.
    vm.stage2
        .map(ipa, vm.guest_pa, pages, writable)
        .map_err(|_| ViError::OutOfMemory)?;
    Ok(())
}

pub(crate) fn guest_window(owner: Charge, vm_id: usize) -> ViResult<(u64, usize)> {
    const GUEST_IPA_BASE: u64 = 0x4000_0000;
    let guard = registry_lock().lock();
    let map = guard.as_ref().ok_or(ViError::NotFound)?;
    let vm = map
        .get(&VmKey::new(owner, vm_id))
        .ok_or(ViError::NotFound)?;
    let bytes = vm
        .guest_pages
        .checked_mul(crate::memory::paging::PAGE_SIZE)
        .ok_or(ViError::InvalidInput)?;
    Ok((GUEST_IPA_BASE, bytes))
}

pub(crate) fn run_vcpu(
    owner: Charge,
    vm_id: usize,
    vcpu_id: usize,
    _budget_ns: u64,
) -> ViResult<abi::hypervisor::ViVmExit> {
    let hal_exit = {
        let mut guard = registry_lock().lock();
        let map = guard.as_mut().ok_or(ViError::NotFound)?;
        let vm = map
            .get_mut(&VmKey::new(owner, vm_id))
            .ok_or(ViError::NotFound)?;
        let vcpu_idx = vcpu_id.saturating_sub(1);

        // Pi's software GICC requests at most one edge per entry. Consume
        // one request now, assert HCR_EL2.VI only during this guest run and
        // clear VI in the EL2 exit trampoline before the host resumes.
        let mut virtual_irq = PI_MONITOR
            && vm
                .vcpu_irqs
                .get_mut(vcpu_idx)
                .and_then(PendingIrqs::take_lowest)
                .is_some();
        let mut num_loaded = 0usize;
        if let Some(windows) = VGIC {
            if let (Some(q), Some(cpu_if), Some(vcpu)) = (
                vm.vcpu_irqs.get_mut(vcpu_idx),
                vm.vcpu_vgic.get_mut(vcpu_idx),
                vm.vcpus.get(vcpu_idx),
            ) {
                // The guest's timer is level-triggered and only its saved
                // CNTV state says whether the line is up, so sample it at
                // every entry. Injecting it only on WFI starved a CPU-bound
                // guest of ticks: it never waited, so it never got one.
                if guest_vtimer_asserted(vcpu.g_cntv_ctl, vcpu.g_cntv_cval) {
                    q.set(GUEST_VTIMER_PPI);
                }
                // SAFETY: EL2; the board's GICH window is kernel-mapped; no
                // vCPU running.
                num_loaded = unsafe { cpu_if.load(windows.gich_base, || q.take_lowest()) };
            }
        }

        // ── Guest image: make it visible to the guest's own view ────────────
        //
        // The cell streams the guest kernel/initrd/DTB into guest RAM through
        // `write_guest_memory`, which copies through the kernel's mapping; the
        // guest fetches those pages through Stage-2, a different VA and ASID.
        // Cache maintenance by VA does not cover that alias, so the first
        // entry cleans the whole window to the point of coherency and drops
        // the instruction cache. Once per VM: the streaming happens before
        // the first run, and the guest owns the window afterwards.
        if !vm.entry_flushed {
            #[cfg(target_arch = "aarch64")]
            {
                let start = vm.guest_pa as usize;
                let len = vm.guest_pages * crate::memory::paging::PAGE_SIZE;
                hal::aarch64::cache::clean_data_cache_range(start, len);
                hal::aarch64::cache::invalidate_instruction_cache_all();
            }
            vm.entry_flushed = true;
        }

        // ── World-switch into guest ──────────────────────────────────────────
        let exit = {
            let vcpu = vm.vcpus.get_mut(vcpu_idx).ok_or(ViError::NotFound)?;

            // Resolve guest ID_AA64* reads (trapped by HCR_EL2.TID3) here:
            // `ViVmExit::SysReg` cannot carry a return value. When the
            // bounded batch ends, retry the faulting MRS on the next run
            // instead of fabricating a CPU feature value.
            const MAX_ID_REG_RESOLVES: u32 = 64;
            let mut resolved = 0u32;
            let exit = loop {
                let exit = if PI_MONITOR {
                    let vi = core::mem::replace(&mut virtual_irq, false);
                    unsafe {
                        El2Monitor::run(vcpu, vm.vmid, vm.stage2.root_pa(), vi);
                    }
                    vcpu.decode_exit()
                } else {
                    // `VTTBR_EL2` is per CPU and this vCPU may enter on any
                    // of them: name this VM's root for this entry only (the
                    // Pi monitor does the same inside its run op).
                    // SAFETY: EL2 host; the VM is registered (its root is
                    // live) and the registry lock is held across the run.
                    unsafe {
                        load_stage2(vm.vmid, vm.stage2.root_pa());
                        let exit = run_vcpu_impl(vcpu);
                        clear_stage2_root();
                        exit
                    }
                };
                if let HalVmExit::SysReg {
                    op0,
                    op1,
                    crn,
                    crm,
                    op2,
                    rt,
                    is_write,
                } = exit
                {
                    if !is_write {
                        if let Some(val) = read_trapped_id_reg(op0, op1, crn, crm, op2) {
                            if resolved == MAX_ID_REG_RESOLVES {
                                break HalVmExit::Preempted;
                            }
                            if (rt as usize) < 31 {
                                vcpu.gp[rt as usize] = val;
                            }
                            vcpu.g_elr_el2 = vcpu.exit_elr.wrapping_add(4);
                            resolved += 1;
                            continue;
                        }
                    }
                }
                break exit;
            };
            // Unhandled guest trap: dump the guest's own EL1 exception bank.
            // After a guest-internal exception these carry the ORIGINAL
            // syndrome (the EL2 exit only sees the follow-on vector-fetch
            // fault), which is the difference between a diagnosable failure
            // and "unknown vmexit".
            if let HalVmExit::Unknown { ec, iss } = exit {
                log::warn!(
                    "[hv] unhandled guest trap ec={:#x} iss={:#x} | guest ELR_EL1={:#x} ESR_EL1={:#x} FAR_EL1={:#x} VBAR_EL1={:#x} SCTLR_EL1={:#x} SPSR_EL1={:#x}",
                    ec, iss,
                    vcpu.g_elr_el1, vcpu.g_esr_el1, vcpu.g_far_el1,
                    vcpu.g_vbar_el1, vcpu.g_sctlr_el1, vcpu.g_spsr_el1,
                );
                log::warn!(
                    "[hv]   guest TCR_EL1={:#x} TTBR0_EL1={:#x} TTBR1_EL1={:#x} MAIR_EL1={:#x}",
                    vcpu.g_tcr_el1,
                    vcpu.g_ttbr0_el1,
                    vcpu.g_ttbr1_el1,
                    vcpu.g_mair_el1,
                );
            }
            exit
            // vcpu borrow ends here (NLL + nested block)
        };

        // ── Phase 09: take the vCPU's GICH state back after exit ────────────
        // Unacknowledged edge interrupts return to the pending set; the timer
        // line is re-sampled at the next entry instead. Active ones (guest
        // preempted mid-handler) stay with the vCPU's interface state.
        // SAFETY: no vCPU running; EL2; the board's GICH window is mapped.
        if let Some(windows) = VGIC {
            if let (Some(q), Some(cpu_if)) = (
                vm.vcpu_irqs.get_mut(vcpu_idx),
                vm.vcpu_vgic.get_mut(vcpu_idx),
            ) {
                unsafe {
                    cpu_if.save(
                        windows.gich_base,
                        num_loaded,
                        |intid| intid == GUEST_VTIMER_PPI,
                        |intid| q.set(intid),
                    )
                };
            }
        }

        exit
    };

    // Convert HAL ViVmExit → API ViVmExit (same fields, different crate paths).
    let api_exit = match hal_exit {
        HalVmExit::MmioRead { ipa, size, reg } => ApiVmExit::MmioRead { ipa, size, reg },
        HalVmExit::MmioWrite { ipa, size, val } => ApiVmExit::MmioWrite { ipa, size, val },
        HalVmExit::Hvc { imm, regs } => ApiVmExit::Hvc { imm, regs },
        HalVmExit::Wfi => ApiVmExit::Wfi,
        HalVmExit::SysReg {
            op0,
            op1,
            crn,
            crm,
            op2,
            rt,
            is_write,
        } => ApiVmExit::SysReg {
            op0,
            op1,
            crn,
            crm,
            op2,
            rt,
            is_write,
        },
        HalVmExit::Preempted => ApiVmExit::Preempted,
        HalVmExit::Shutdown => ApiVmExit::Shutdown,
        HalVmExit::Unknown { ec, iss } => ApiVmExit::Unknown { ec, iss },
        // x86-only exits (SVM/VT-x) never arise on the aarch64 world-switch.
        HalVmExit::PortIn { .. }
        | HalVmExit::PortOut { .. }
        | HalVmExit::Hlt
        | HalVmExit::Msr { .. } => ApiVmExit::Unknown { ec: 0, iss: 0 },
    };
    Ok(api_exit)
}

pub(crate) fn vcpu_regs(
    owner: Charge,
    vm_id: usize,
    vcpu_id: usize,
    regs: &mut [u64; 32],
    mode: usize,
) -> ViResult<usize> {
    let mut guard = registry_lock().lock();
    let map = guard.as_mut().ok_or(ViError::NotFound)?;
    let vm = map
        .get_mut(&VmKey::new(owner, vm_id))
        .ok_or(ViError::NotFound)?;
    let vcpu = vm
        .vcpus
        .get_mut(vcpu_id.saturating_sub(1))
        .ok_or(ViError::NotFound)?;
    let buf = regs;
    match mode {
        1 => {
            for (i, v) in buf[..31].iter().enumerate() {
                vcpu.gp[i] = *v;
            }
            vcpu.g_elr_el2 = buf[31];
        }
        0 => {
            for (i, v) in vcpu.gp.iter().enumerate() {
                buf[i] = *v;
            }
            buf[31] = vcpu.g_elr_el2;
        }
        2 if <hal::aarch64::vcpu::VcpuExt as hal::aarch64::backend::ArmVcpuExt>::REGS_MAX_MODE
            >= 2 =>
        {
            buf.fill(0);
            buf[0] = vcpu.g_cntv_ctl;
            buf[1] = vcpu.g_cntv_cval;
        }
        _ => return Err(ViError::InvalidArgument),
    }
    Ok(0)
}

pub(crate) fn write_guest_memory(
    owner: Charge,
    vm_id: usize,
    gpa: u64,
    src: &[u8],
) -> ViResult<usize> {
    use crate::memory::paging::PAGE_SIZE;
    const GUEST_IPA_BASE: u64 = 0x4000_0000;
    let len = src.len();

    let guard = registry_lock().lock();
    let map = guard.as_ref().ok_or(ViError::NotFound)?;
    let vm = map
        .get(&VmKey::new(owner, vm_id))
        .ok_or(ViError::NotFound)?;

    // Validate gpa is within the mapped guest-RAM window.
    let offset = gpa
        .checked_sub(GUEST_IPA_BASE)
        .ok_or(ViError::InvalidInput)? as usize;
    let end = offset.checked_add(len).ok_or(ViError::InvalidInput)?;
    if end > vm.guest_pages * PAGE_SIZE {
        return Err(ViError::InvalidInput);
    }

    // SAFETY: guest RAM is kernel-allocated identity-mapped memory and
    // `offset + len` lies inside it (checked above); `src` is a kernel
    // slice, disjoint from guest RAM.
    unsafe {
        let dst = (vm.guest_pa as usize + offset) as *mut u8;
        core::ptr::copy_nonoverlapping(src.as_ptr(), dst, len);
    }
    Ok(len)
}

pub(crate) fn read_guest_memory(
    owner: Charge,
    vm_id: usize,
    gpa: u64,
    dst: &mut [u8],
) -> ViResult<usize> {
    use crate::memory::paging::PAGE_SIZE;
    const GUEST_IPA_BASE: u64 = 0x4000_0000;
    let len = dst.len();

    let guard = registry_lock().lock();
    let map = guard.as_ref().ok_or(ViError::NotFound)?;
    let vm = map
        .get(&VmKey::new(owner, vm_id))
        .ok_or(ViError::NotFound)?;

    // Validate gpa is within the mapped guest-RAM window.
    let offset = gpa
        .checked_sub(GUEST_IPA_BASE)
        .ok_or(ViError::InvalidInput)? as usize;
    let end = offset.checked_add(len).ok_or(ViError::InvalidInput)?;
    if end > vm.guest_pages * PAGE_SIZE {
        return Err(ViError::InvalidInput);
    }

    // SAFETY: guest RAM is kernel-allocated identity-mapped memory and
    // `offset + len` lies inside it (checked above); `dst` is a kernel
    // slice, disjoint from guest RAM.
    unsafe {
        let src = (vm.guest_pa as usize + offset) as *const u8;
        core::ptr::copy_nonoverlapping(src, dst.as_mut_ptr(), len);
    }
    Ok(len)
}

pub(crate) fn inject_irq(
    owner: Charge,
    vm_id: usize,
    vcpu_id: usize,
    intid: u32,
) -> ViResult<usize> {
    let mut guard = registry_lock().lock();
    let map = guard.as_mut().ok_or(ViError::NotFound)?;
    let vm = map
        .get_mut(&VmKey::new(owner, vm_id))
        .ok_or(ViError::NotFound)?;
    let idx = vcpu_id.saturating_sub(1);
    if let Some(q) = vm.vcpu_irqs.get_mut(idx) {
        q.set(intid);
    }
    Ok(0)
}

/// Reclaim every VM `owner` holds: its tables, guest RAM, VMID and guest
/// charge. Called once per incarnation from the root-retirement funnel, after
/// every member quiesced (`hypervisor::registry::reap_vms_for`).
pub(crate) fn reap_vms_for(owner: Charge) {
    // Collect entries to drop outside the lock (Stage2Table::drop frees frames).
    let dead_vms: alloc::vec::Vec<Vm> = {
        let mut guard = registry_lock().lock();
        let Some(map) = guard.as_mut() else { return };
        let dead_keys: alloc::vec::Vec<VmKey> = map
            .keys()
            .filter(|key| key.owned_by(owner))
            .copied()
            .collect();
        dead_keys.iter().filter_map(|k| map.remove(k)).collect()
    };
    // Disable Stage-2 and drop the dying VMs' translations before their
    // tables and guest RAM go back to the allocator. `disable_stage2` flushes
    // only the VMID in this CPU's VTTBR_EL2 — none, since every guest exit
    // and `create_vm` clear it — so every VMID is flushed, on every CPU,
    // before the drop (census P13). No CPU names the dying root: VTTBR_EL2
    // holds a root only during a guest run, under the registry lock, and the
    // VM left the registry above.
    // One flush covers every dying VM (every VMID on every CPU), so a
    // teardown costs one broadcast, not one per VM. Each VMID goes back to
    // the pool only after that flush completed, so the VM that is handed it
    // next starts with no entry tagged with it.
    if dead_vms.is_empty() {
        return;
    }
    // SAFETY: no vCPU is running (the owner quiesced); safe to disable Stage-2.
    unsafe {
        disable_stage2();
        s2_tlb_flush_all();
    }
    for vm in dead_vms {
        VMIDS.release(vm.vmid);
        drop(vm); // Stage2Table::drop frees all frames and refunds their charge
    }
}

/// Boot self-test `S22-AARCH64-TWO-VM-STAGE2` (non-release builds; the
/// `virt-vhe` lane requires it): two live VMs, entered through `run_vcpu`,
/// each run against its own Stage-2 root, and this CPU naming no root outside
/// a guest run; then VMIDs that stay within their 8-bit width and are recycled.
///
/// Both VMs carry a guest at the same IPA that loads a VM-specific marker into
/// X0, traps with `HVC #0` and branches back, so every run exits with its
/// marker. VM B is created last, which used to leave this CPU's `VTTBR_EL2`
/// naming B's root; before every guest entry programmed it, running A executed
/// B's guest (X0 = B's marker). The fixture runs A, B, then A again, and tears
/// both down. `VTTBR_EL2` must read 0 after each create, each run and the
/// teardown: a CPU left naming a root ran the next VM created on another CPU
/// against that root once it was freed. A boot fixture cannot place work on a
/// chosen CPU, so this is checked on the CPU it runs on. EL2 hosts only: the
/// Pi kernel cannot read `VTTBR_EL2`, and its monitor programs it inside its
/// own run op.
///
/// VMIDs: one-page VMs are then created until `create_vm` refuses. Every VMID
/// must lie in 1..=255 and be held once, and the refusal must come from an
/// exhausted pool: VMIDs used to count up through 16 bits, so VM 256 shared
/// the host's TLB tag and set RES0 bits of `VTTBR_EL2`. After their teardown
/// VM C is created; it must be handed A's VMID again and run its own guest.
#[cfg(not(cellos_posture = "release"))]
pub(crate) fn two_vm_stage2_selftest() {
    use hal::aarch64::{backend::ArmIrqController, stage2_regs::VMID_COUNT};

    if PI_MONITOR || !hal::aarch64::el2::is_el2() {
        log::info!("S22-AARCH64-TWO-VM-STAGE2: SKIP (not an EL2 host)");
        return;
    }
    /// The kernel: its guest charge is never limited or counted, so the
    /// fixture can hold all 255 VMIDs.
    const OWNER: Charge = Charge::KERNEL;
    const ENTRY: u64 = 0x4008_0000;
    const GUEST_PAGES: usize = 512;
    const MARKERS: [u16; 2] = [0xa1, 0xb2];
    const MARKER_C: u16 = 0xc3;
    /// A host tick may exit a guest before its `HVC`; the next run resumes it.
    const MAX_PREEMPTIONS: usize = 8;
    /// Keeps the first nonzero `VTTBR_EL2` this CPU shows outside a guest run.
    fn note_vttbr(named: &mut Option<u64>) {
        // SAFETY: EL2 host (checked on entry to the fixture).
        let vttbr = unsafe { hal::aarch64::stage2_regs::monitor_read_vttbr() };
        if vttbr != 0 {
            named.get_or_insert(vttbr);
        }
    }
    fn run(named: &mut Option<u64>, (vm_id, vcpu): (usize, usize)) -> Option<ViResult<ApiVmExit>> {
        let mut exit = run_vcpu(OWNER, vm_id, vcpu, 0);
        for _ in 0..MAX_PREEMPTIONS {
            if !matches!(exit, Ok(ApiVmExit::Preempted)) {
                break;
            }
            exit = run_vcpu(OWNER, vm_id, vcpu, 0);
        }
        note_vttbr(named);
        Some(exit)
    }
    fn vmid_of(vm_id: usize) -> Option<u16> {
        registry_lock()
            .lock()
            .as_ref()
            .and_then(|map| map.get(&VmKey::new(OWNER, vm_id)))
            .map(|vm| vm.vmid)
    }
    let guest = |marker: u16| -> [u8; 12] {
        // MOVZ X0, #marker; HVC #0; B .-8 (back to the MOVZ)
        let words = [
            0xd280_0000 | (u32::from(marker) << 5),
            0xd400_0002,
            0x17ff_fffe,
        ];
        let mut bytes = [0u8; 12];
        for (chunk, word) in bytes.chunks_exact_mut(4).zip(words) {
            chunk.copy_from_slice(&word.to_le_bytes());
        }
        bytes
    };
    let mut named_root = None;
    let mut vms = [(0usize, 0usize); 2];
    let mut built = true;
    for (index, &marker) in MARKERS.iter().enumerate() {
        let Ok(vm_id) = create_vm(OWNER, GUEST_PAGES) else {
            built = false;
            break;
        };
        note_vttbr(&mut named_root);
        let Ok(vcpu) = create_vcpu(OWNER, vm_id, ENTRY) else {
            built = false;
            break;
        };
        vms[index] = (vm_id, vcpu);
        if write_guest_memory(OWNER, vm_id, ENTRY, &guest(marker)).is_err() {
            built = false;
            break;
        }
    }
    let vmid_a = vmid_of(vms[0].0);
    // Boot runs with host interrupts masked, so a pending host interrupt (a
    // VirtIO line the boot left asserted) would exit every guest entry at once
    // (`HCR_EL2.IMO`): hold them at the controller for the guest runs.
    let exits = if built {
        <Board as ArmIrqController>::with_irqs_held(|| {
            [
                run(&mut named_root, vms[0]),
                run(&mut named_root, vms[1]),
                run(&mut named_root, vms[0]),
            ]
        })
    } else {
        [None; 3]
    };
    reap_vms_for(OWNER);
    note_vttbr(&mut named_root);

    // Hold every VMID; `held` stops at the width so a VMID past it fails.
    let mut held = [false; VMID_COUNT];
    let mut count = 0usize;
    let mut each_once = true;
    while count < VMID_COUNT {
        let Ok(vm_id) = build_vm(OWNER, 1, true) else {
            break;
        };
        count += 1;
        match vmid_of(vm_id).map(usize::from) {
            Some(vmid) if vmid != 0 && vmid < VMID_COUNT && !held[vmid] => held[vmid] = true,
            _ => each_once = false,
        }
    }
    note_vttbr(&mut named_root);
    let exhausted = match VMIDS.allocate() {
        Some(vmid) => {
            VMIDS.release(vmid);
            false
        }
        None => true,
    };
    reap_vms_for(OWNER);
    note_vttbr(&mut named_root);

    // VM C takes the lowest free VMID, the one A ran under (and the first
    // one-page VM after it), and must run its own guest, not A's.
    let vm_c = create_vm(OWNER, GUEST_PAGES).ok().and_then(|vm_id| {
        let vcpu = create_vcpu(OWNER, vm_id, ENTRY).ok()?;
        write_guest_memory(OWNER, vm_id, ENTRY, &guest(MARKER_C)).ok()?;
        Some((vm_id, vcpu))
    });
    note_vttbr(&mut named_root);
    let vmid_c = vm_c.and_then(|(vm_id, _)| vmid_of(vm_id));
    let exit_c = vm_c
        .and_then(|vm| <Board as ArmIrqController>::with_irqs_held(|| run(&mut named_root, vm)));
    reap_vms_for(OWNER);
    note_vttbr(&mut named_root);

    let marker_of = |exit: Option<ViResult<ApiVmExit>>| match exit {
        Some(Ok(ApiVmExit::Hvc { regs, .. })) => Some(regs[0]),
        _ => None,
    };
    let seen = exits.map(marker_of);
    let seen_c = marker_of(exit_c);
    let expected = [MARKERS[0], MARKERS[1], MARKERS[0]].map(|marker| Some(u64::from(marker)));
    let vmids_ok = exhausted && each_once && count == VMID_COUNT - 1;
    let recycled = vmid_a.is_some() && vmid_c == vmid_a;
    if seen == expected
        && named_root.is_none()
        && vmids_ok
        && recycled
        && seen_c == Some(u64::from(MARKER_C))
    {
        log::info!(
            "S22-AARCH64-TWO-VM-STAGE2: PASS runs=A,B,A markers={:x?} vttbr-outside-runs=0 vmids={} then refused; C reran VMID {} with {:x}",
            MARKERS,
            count,
            vmid_c.unwrap_or(0),
            MARKER_C
        );
    } else {
        log::error!(
            "S22-AARCH64-TWO-VM-STAGE2: FAIL built={} seen={:x?} expected={:x?} vttbr-outside-runs={:#x} exits={:?} vmids={} exhausted={} each-once={} vmid-a={:?} vmid-c={:?} seen-c={:x?} exit-c={:?}",
            built,
            seen,
            expected,
            named_root.unwrap_or(0),
            exits,
            count,
            exhausted,
            each_once,
            vmid_a,
            vmid_c,
            seen_c,
            exit_c
        );
    }
}
