// SPDX-License-Identifier: MPL-2.0
//! Phase 14 A/B: the `yield_cpu` switch path as it was before the HAL's
//! `switch(SwitchPlan)`, kept verbatim for one commit range so the A/B fixture
//! can boot it beside the new path. Built only with `CELLOS_SWITCH_AB=legacy`
//! (`cfg(cellos_switch_legacy)`, refused in release posture); a child of
//! `crate::task`, it shares the housekeeping drain with the new path.

/// Core scheduling logic: picks next task and performs switch OUTSIDE of the lock.
pub fn yield_cpu() {
    #[cfg(target_arch = "x86_64")]
    if !crate::sched::smp::x86_scheduler_ready() {
        return;
    }
    // RV64 cooperative yields can enter with SIE set (not only from trap
    // context). Capture and clear it before any scheduler lock or publication;
    // masking only at `__switch` leaves current/selected scheduler state interruptible.
    #[cfg(target_arch = "riscv64")]
    let outgoing_sstatus = crate::hal::arch::save_and_disable_interrupts();

    #[cfg(feature = "test-hooks")]
    crate::loader::atomic_publication_tests::observe_schedule_attempt();

    // x86_64: disable interrupts for the entire scheduler critical section.
    // Without this, the LAPIC timer fires mid-lock and causes a nested
    // yield_cpu call that deadlocks on the same spinlock (IRQ-in-lock deadlock).
    // RISC-V/AArch64 automatically clear the interrupt-enable bit on trap entry,
    // so they don't have this problem when called from vi_timer_tick.
    #[cfg(target_arch = "x86_64")]
    let outgoing_rflags: usize = unsafe {
        let flags;
        core::arch::asm!("pushfq", "pop {}", "cli", out(reg) flags);
        flags
    };
    super::drain_before_selection();

    // AArch64 reaches this point from the boot idle loop with IRQs unmasked,
    // and the SCHEDULER guard restores that state when it drops. Mask from
    // selection through the raw switch: a timer IRQ in that window would
    // re-enter the scheduler after this hart published its selected and
    // outgoing-save identities but before the switch, and save the IRQ
    // activation into the selected task's Context. The incoming Context
    // restores its own DAIF; this activation restores the captured state when
    // it resumes, at the end of `yield_cpu`.
    #[cfg(target_arch = "aarch64")]
    let outgoing_daif = crate::hal::arch::save_and_disable_interrupts();
    let hart_id = crate::sched::hart_local::current_hart_id();
    #[cfg(all(feature = "native-domains", cellos_wide))]
    let switch_info = if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        sched.pick_next_domain(hart_id)
    } else {
        None
    };
    #[cfg(not(all(feature = "native-domains", cellos_wide)))]
    let switch_info = if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        sched.pick_next(hart_id)
    } else {
        None
    };
    // The scheduler lock is now released but the terminal Context is still
    // executing. Exercise the real trap admission boundary in this exact
    // pre-switch interval; the regression proves a heartbeat victim cannot
    // become caller 0 before boot owns the incoming Context.
    #[cfg(all(
        feature = "test-hooks",
        target_arch = "riscv64",
        feature = "native-domains"
    ))]
    if let Some(ref plan) = switch_info {
        if plan.incoming.is_null() {
            crate::arch::task::retirement_selftest::observe_heartbeat_terminal_current();
        }
    }
    #[cfg(all(
        feature = "test-hooks",
        target_arch = "riscv64",
        not(feature = "native-domains")
    ))]
    if let Some((_, next)) = switch_info {
        if next.is_null() {
            crate::arch::task::retirement_selftest::observe_heartbeat_terminal_current();
        }
    }
    if switch_info.is_none() {
        // Selection is normally published only with a concrete switch tuple.
        // Clear defensively on the no-switch/abort path so a stale pin cannot
        // retain a retired Context indefinitely.
        crate::sched::hart_local::ready::abort_selected_switch(hart_id);
    }
    #[cfg(target_arch = "riscv64")]
    if switch_info.is_none() {
        // No incoming Context will restore status for us. Preserve the caller's
        // exact SIE state instead of unconditionally enabling interrupts.
        unsafe {
            crate::hal::arch::restore_sstatus(outgoing_sstatus);
        }
    }

    #[cfg(all(feature = "native-domains", cellos_wide))]
    if let Some(plan) = switch_info {
        unsafe {
            let final_curr = if plan.outgoing.is_null() {
                #[cfg(target_arch = "riscv64")]
                {
                    &raw mut super::BOOT_CONTEXTS[hart_id]
                }
                #[cfg(not(target_arch = "riscv64"))]
                {
                    super::boot_context_for(hart_id)
                }
            } else {
                plan.outgoing
            };
            let final_next = if plan.incoming.is_null() {
                #[cfg(target_arch = "riscv64")]
                {
                    &raw const super::BOOT_CONTEXTS[hart_id]
                }
                #[cfg(not(target_arch = "riscv64"))]
                {
                    super::boot_context_for(hart_id) as *const crate::hal::arch::Context
                }
            } else {
                plan.incoming
            };
            if !plan.incoming.is_null() {
                #[cfg(feature = "bench-counters")]
                crate::bench_counters::context_switch();
                #[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
                super::note_task_dispatch(hart_id);
                #[cfg(target_arch = "riscv64")]
                crate::hal::arch::set_kernel_stack((&*plan.incoming).sp);
                #[cfg(target_arch = "aarch64")]
                crate::hal::arch::set_kernel_stack((&*plan.incoming).sp as usize);
                #[cfg(target_arch = "x86_64")]
                crate::hal::arch::set_kernel_stack((&*plan.incoming).kernel_trap_sp as usize);
                #[cfg(not(target_arch = "riscv64"))]
                super::tls::install_published_base();
            }
            #[cfg(target_arch = "riscv64")]
            {
                #[cfg(feature = "test-hooks")]
                crate::arch::task::retirement_selftest::hold_after_selection_before_switch(hart_id);
                let (root_ppn, asid) = plan.root_switch();
                crate::hal::arch::Context::switch_with_saved_sstatus(
                    final_curr,
                    final_next,
                    outgoing_sstatus,
                    root_ppn,
                    asid,
                );
            }
            #[cfg(not(target_arch = "riscv64"))]
            {
                let (root_addr, asid) = plan.root_switch();
                // The root is programmed *inside* the switch, after the outgoing
                // context is saved and before the incoming stack is adopted.
                // Programming it here instead would leave the incoming root live
                // while the outgoing context is still being saved on a stack the
                // new root does not map.
                crate::hal::arch::Context::switch_with_root(
                    final_curr, final_next, root_addr, asid,
                );
            }
        }
    }
    #[cfg(not(all(feature = "native-domains", cellos_wide)))]
    if let Some((curr, next)) = switch_info {
        unsafe {
            let final_curr = if curr.is_null() {
                #[cfg(target_arch = "riscv64")]
                {
                    &raw mut super::BOOT_CONTEXTS[hart_id]
                }
                #[cfg(not(target_arch = "riscv64"))]
                {
                    super::boot_context_for(hart_id)
                }
            } else {
                curr
            };

            let final_next = if next.is_null() {
                #[cfg(target_arch = "riscv64")]
                {
                    &raw const super::BOOT_CONTEXTS[hart_id]
                }
                #[cfg(not(target_arch = "riscv64"))]
                {
                    super::boot_context_for(hart_id) as *const crate::hal::arch::Context
                }
            } else {
                next
            };
            if !next.is_null() {
                #[cfg(feature = "bench-counters")]
                crate::bench_counters::context_switch();
                // Set TSS.rsp0 and CPU_LOCAL.kernel_rsp for the incoming task.
                // The physical GS_BASE/KERNEL_GS_BASE pair is kernel context
                // state and must remain untouched across this raw switch.
                // kernel_trap_sp is fixed at spawn, unlike the deep yield RSP.
                let next_ref = &*next;
                // aarch64 context stores sp as u64 (register-width field);
                // riscv64 already uses usize — keep it cast-free so clippy's
                // same-type-cast lint stays quiet on that target.
                #[cfg(target_arch = "aarch64")]
                crate::hal::arch::set_kernel_stack(next_ref.sp as usize);
                #[cfg(not(any(
                    target_arch = "x86_64",
                    target_arch = "aarch64",
                    target_arch = "riscv32"
                )))]
                crate::hal::arch::set_kernel_stack(next_ref.sp);
                #[cfg(target_arch = "riscv32")]
                crate::hal::arch::set_kernel_stack(next_ref.sp as usize);
                #[cfg(target_arch = "x86_64")]
                crate::hal::arch::set_kernel_stack(next_ref.kernel_trap_sp as usize);
                #[cfg(not(target_arch = "riscv64"))]
                super::tls::install_published_base();
            }
            #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
            crate::arch::task::retirement_selftest::hold_after_selection_before_switch(hart_id);

            #[cfg(target_arch = "riscv64")]
            crate::hal::arch::Context::switch_with_saved_sstatus(
                final_curr,
                final_next,
                outgoing_sstatus,
                0,
                0,
            );
            #[cfg(not(target_arch = "riscv64"))]
            crate::hal::arch::Context::switch(final_curr, final_next);
        }
    }

    // The incoming task is now the running one. Install its user thread pointer
    // (aarch64/x86_64; riscv64 carries it in the trap frame) from the hart-local
    // value the scheduler published, so this stays lock-free.
    super::tls::apply_on_resume();
    // This activation may resume on another CPU. Its captured flags belong to
    // the suspended context, not the selecting CPU; never enable IF inside an
    // IRQ activation that originally entered masked.
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!("push {}", "popfq", in(reg) outgoing_rflags);
    }
    // Same rule for DAIF: restore this activation's own pre-selection state.
    #[cfg(target_arch = "aarch64")]
    unsafe {
        crate::hal::arch::restore_sstatus(outgoing_daif);
    }
}
