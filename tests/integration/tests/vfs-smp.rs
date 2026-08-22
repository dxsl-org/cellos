//! Two-hart VFS integration regression.
//!
//! The VFS test client must receive request/reply traffic only from VFS. This
//! runner keeps that transport boundary covered under concurrent SMP traffic;
//! the single-hart quota runner remains the one-hart VFS contract.

use std::path::PathBuf;

use vicell_integration_tests::{qemu_binary, QemuRunner};

const VFS_TEST_CLIENT: &str = include_str!("../../../cells/tests/vfs-test/src/main.rs");
const RV64_SWITCH_ASM: &str = include_str!("../../../hal/arch/riscv/src/rv64/asm/switch.S");
const RV64_CONTEXT: &str = include_str!("../../../hal/arch/riscv/src/rv64/context.rs");
const KERNEL_TASK: &str = include_str!("../../../kernel/src/task.rs");
const SCHEDULER: &str = include_str!("../../../kernel/src/task/scheduler.rs");
const HART_LOCAL: &str = include_str!("../../../kernel/src/task/hart_local.rs");
const RETIREMENT_SELFTEST: &str =
    include_str!("../../../kernel/src/task/retirement_selftest.rs");
const CONTEXT_HANDOFF_SELFTEST: &str =
    include_str!("../../../kernel/src/task/context_handoff_selftest.rs");
const ATOMIC_PUBLICATION_CASES: &str =
    include_str!("../../../kernel/src/loader/atomic_publication_tests/cases.rs");


fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

fn test_hooks_kernel() -> String {
    repo_root()
        .join("target/riscv64gc-unknown-none-elf/release/cellos-kernel-test-hooks")
        .to_string_lossy()
        .into_owned()
}

fn prerequisites_ok() -> bool {
    let kernel = PathBuf::from(test_hooks_kernel());
    let qemu_ok = std::process::Command::new(qemu_binary())
        .arg("--version")
        .output()
        .is_ok();
    if !kernel.exists() {
        eprintln!(
            "SKIP: test-hooks kernel not found ({}). Run scripts/build-test-hooks-cells.ps1 first.",
            test_hooks_kernel()
        );
    }
    if !qemu_ok {
        eprintln!("SKIP: qemu-system-riscv64 not on PATH");
    }
    vicell_integration_tests::ci_guard(kernel.exists() && qemu_ok)
}

fn wait_for_or_dump(runner: &QemuRunner, pattern: &str) {
    runner.wait_for(pattern, 60).unwrap_or_else(|error| {
        panic!("{error}\n--- serial output ---\n{}\n---", runner.dump());
    });
}

#[test]
fn rv64_yield_masks_interrupts_before_selection_and_preserves_switch_status() {
    let yield_mask = KERNEL_TASK
        .find("let outgoing_sstatus = crate::hal::arch::save_and_disable_interrupts()")
        .expect("RV64 yield captures and masks the outgoing status");
    let pick = KERNEL_TASK[yield_mask..]
        .find("sched.pick_next(hart_id)")
        .map(|offset| yield_mask + offset)
        .expect("scheduler selection follows the RV64 mask");
    let no_switch_restore = KERNEL_TASK[pick..]
        .find("restore_sstatus(outgoing_sstatus)")
        .map(|offset| pick + offset)
        .expect("no-switch path restores the captured status");
    let post_pick_hook = KERNEL_TASK[pick..]
        .find("hold_after_selection_before_switch(hart_id)")
        .map(|offset| pick + offset)
        .expect("forced-SSIP hook runs after selection");
    let switch = KERNEL_TASK[post_pick_hook..]
        .find("Context::switch_with_saved_sstatus(")
        .map(|offset| post_pick_hook + offset)
        .expect("RV64 switch receives the pre-selection status");

    assert!(
        yield_mask < pick
            && pick < no_switch_restore
            && no_switch_restore < post_pick_hook
            && post_pick_hook < switch,
        "RV64 must mask before scheduler publication, restore on no-switch, and remain masked through the raw switch"
    );

    assert!(
        RV64_CONTEXT.contains("csrrci {saved}, sstatus, 0x2")
            && RV64_CONTEXT.contains("outgoing_sstatus: usize")
            && RV64_CONTEXT.contains("__switch(old, new, outgoing_sstatus)"),
        "RV64 Context ABI must forward the complete pre-mask outgoing sstatus"
    );
    assert!(
        !RV64_SWITCH_ASM.contains("csrrci"),
        "assembly-only masking is the rejected control: it leaves scheduler publication interruptible"
    );

    let save_ra = RV64_SWITCH_ASM
        .find("sd ra,  0*8(a0)")
        .expect("RV64 switch saves outgoing return address");
    let save_sstatus = RV64_SWITCH_ASM
        .find("sd a2, 15*8(a0)")
        .expect("RV64 switch saves the pre-selection outgoing sstatus argument");
    let callback = RV64_SWITCH_ASM
        .find("call vi_context_switch_complete")
        .expect("RV64 switch invokes incoming completion callback");
    let restore_s11 = RV64_SWITCH_ASM
        .rfind("ld s11,13*8(s11)")
        .expect("RV64 switch restores incoming s11");
    let restore_sstatus = RV64_SWITCH_ASM
        .rfind("csrw sstatus, t0")
        .expect("RV64 switch restores incoming sstatus");
    let ret = RV64_SWITCH_ASM[restore_sstatus..]
        .find("\n    ret")
        .map(|offset| restore_sstatus + offset)
        .expect("RV64 switch returns after restoring incoming sstatus");

    assert!(
        save_ra < save_sstatus
            && save_sstatus < callback
            && callback < restore_s11
            && restore_s11 < restore_sstatus
            && restore_sstatus < ret,
        "RV64 Context::switch must save the original status and defer incoming SIE restoration until register completion"
    );
}

#[test]
fn rv64_task_to_idle_retains_identity_until_boot_switch_completion() {
    for (branch, return_statement) in [
        (
            "if self.zombies.iter().any(|t| t.id == cid)",
            "return Some((c, core::ptr::null()))",
        ),
        (
            "Live blocked task with no peer ready to run.",
            "return Some((curr_ctx, core::ptr::null()))",
        ),
    ] {
        let start = SCHEDULER.find(branch).expect("task-to-idle branch exists");
        let return_to_boot = SCHEDULER[start..]
            .find(return_statement)
            .map(|offset| start + offset)
            .expect("task-to-idle branch selects the boot context");
        let source = &SCHEDULER[start..return_to_boot];
        assert!(
            !source.contains("set_current_task_id")
                && !source.contains("set_current_cell_id")
                && !source.contains("set_current_cell_context"),
            "outgoing task identity must survive until the incoming boot context"
        );
    }

    let boot_switch = KERNEL_TASK
        .find("let switched_to_boot = pinned == 0")
        .expect("incoming completion recognizes task-to-boot switch");
    let complete_selected = KERNEL_TASK[boot_switch..]
        .find("complete_selected_switch(hart, selected)")
        .map(|offset| boot_switch + offset)
        .expect("incoming completion publishes boot execution");
    let clear_task = KERNEL_TASK[complete_selected..]
        .find("set_current_task_id(hart, 0)")
        .map(|offset| complete_selected + offset)
        .expect("boot completion clears current task");
    let clear_cell = KERNEL_TASK[clear_task..]
        .find("set_current_cell_context(0, 0)")
        .map(|offset| clear_task + offset)
        .expect("boot completion clears CellId attribution");
    assert!(
        boot_switch < complete_selected && complete_selected < clear_task && clear_task < clear_cell,
        "only the proven incoming boot completion may clear the identity tuple"
    );

    let selected_hold = RETIREMENT_SELFTEST
        .find("stage=selected-pre-executing-hold")
        .expect("retirement regression retains the selected-before-executing stage marker");
    let selected_reap = RETIREMENT_SELFTEST
        .find("stage=selected-context-blocked-retirement-and-reap")
        .expect("selected Context regression runs and blocks the real retirement/reap pass");
    let selected_window_proof = RETIREMENT_SELFTEST
        .find("if !selected_window_blocked")
        .expect("retirement regression checks selected Context retention before execution");
    let aggregate_pass = RETIREMENT_SELFTEST
        .find("SMP-RETIREMENT: PASS (selected Context + zombie switch completion gate owner release + CellId reuse)")
        .expect("retirement regression retains its aggregate PASS marker");
    assert!(
        selected_hold < selected_reap
            && selected_reap < selected_window_proof
            && selected_window_proof < aggregate_pass,
        "selected-before-executing retirement/reap proof must precede aggregate PASS"
    );
    assert!(
        RETIREMENT_SELFTEST
            .contains("stage=idle-attribution-cleared current=0 executing=0 selected=0 cell=0"),
        "retirement regression must expose exact idle attribution before PASS"
    );
}

#[test]
fn rv64_remote_cell_fault_waits_for_scheduler_owned_retirement() {
    let trap_entry = KERNEL_TASK
        .find("pub extern \"Rust\" fn vi_terminate_on_user_trap_fault")
        .expect("RISC-V exposes the trap-proven Cell fault entry");
    let fault_funnel_end = KERNEL_TASK[trap_entry..]
        .find("\n/// Core scheduling logic")
        .map(|offset| trap_entry + offset)
        .expect("trap-proven Cell fault funnel has a bounded source region");
    let source = &KERNEL_TASK[trap_entry..fault_funnel_end];
    assert!(
        source.contains("const _: crate::hal::TerminateOnUserTrapFault = vi_terminate_on_user_trap_fault;"),
        "the RISC-V trap ABI must bind only to the exported trap-proven fault entry"
    );
    assert!(
        source.contains("TrapProvenUserFault::new()")
            && source.contains("DeferredFault::from_user_trap("),
        "only the validated trap wrapper may mint fault provenance for the fixed deferred record"
    );
    assert!(
        !source.contains("force_unlock") && !source.contains("task.name.clone()"),
        "recoverable Cell faults must neither clear a global lock nor clone a heap-backed task name"
    );
    let deferred = source
        .find("hart_local::defer_fault(fault);")
        .expect("fault trap stores a fixed per-hart record");
    let kernel_attribution = source[deferred..]
        .find("hart_local::set_current_cell_id(0);")
        .map(|offset| deferred + offset)
        .expect("fault trap switches allocation attribution to kernel");
    let switch_away = source[kernel_attribution..]
        .find("yield_cpu();")
        .map(|offset| trap_entry + kernel_attribution + offset)
        .expect("fault handoff enters the scheduler before switching away");
    let retirement = KERNEL_TASK
        .find("scheduler.retire_deferred_fault(fault)")
        .expect("scheduler drains the deferred record in its locked phase");
    let funnel_attempt = KERNEL_TASK
        .find("retirement_selftest::observe_fault_scheduler_funnel_attempt(fault.tid);")
        .expect("deferred fault publishes its real scheduler-funnel pre-lock attempt");
    let funnel_lock = KERNEL_TASK[funnel_attempt..]
        .find("let mut guard = SCHEDULER.lock();")
        .map(|offset| funnel_attempt + offset)
        .expect("pre-lock attempt immediately precedes the scheduler guard");
    assert!(
        funnel_attempt < funnel_lock && funnel_lock < retirement,
        "the H0 guard proof must wait for H1's actual scheduler pre-lock handoff"
    );
    let deferred_global = trap_entry + deferred;
    let kernel_attribution_global = trap_entry + kernel_attribution;
    let selection = KERNEL_TASK[retirement..]
        .find("sched.pick_next(hart_id)")
        .map(|offset| retirement + offset)
        .expect("deferred fault retirement precedes scheduler selection");
    assert!(
        deferred_global < kernel_attribution_global
            && kernel_attribution_global < switch_away
            && switch_away < retirement
            && retirement < selection,
        "trap-proven fault capture must hand off kernel attribution before yield_cpu drains and retires it before selecting a successor"
    );
    assert!(
        HART_LOCAL.contains("pub struct DeferredFault")
            && HART_LOCAL.contains("deferred_retirement_pending")
            && HART_LOCAL.contains("current_cell_generation"),
        "each hart must retain a fixed scalar retirement record paired with Cell generation"
    );
    let direct_trigger = RETIREMENT_SELFTEST
        .find("observe_direct_fault_trigger();")
        .expect("selftest records the real synthetic trigger before the trap boundary");
    let trap_boundary = RETIREMENT_SELFTEST[direct_trigger..]
        .find("terminate_test_hook_trap_proven_user_fault")
        .map(|offset| direct_trigger + offset)
        .expect("selftest reaches the trap-proven fault boundary after the trigger record");
    assert!(
        direct_trigger < trap_boundary,
        "direct-fault provenance must publish before trap entry validates and commits its deferred record"
    );
    assert!(
        RETIREMENT_SELFTEST
            .contains("stage=quota-exhausted-user-fault-confirmed")
            && RETIREMENT_SELFTEST
                .contains("stage=quota-exhausted-fault-handoff-kernel-attribution"),
        "two-hart regression must exhaust the user quota and prove the deferred handoff uses kernel attribution"
    );

    let pre_lock = RETIREMENT_SELFTEST
        .find("stage=hart1-scheduler-funnel-pre-lock-attempt-published")
        .expect("two-hart fault regression publishes H1's real scheduler pre-lock attempt");
    let blocked = RETIREMENT_SELFTEST
        .find("stage=hart0-scheduler-owner-retained-hart1-retirement-blocked")
        .expect("two-hart fault regression proves the remote guard is retained");
    let released = RETIREMENT_SELFTEST
        .find("stage=hart0-scheduler-guard-released-worker-unblocked")
        .expect("two-hart fault regression marks the H0 guard release");
    let resumed = RETIREMENT_SELFTEST
        .find("stage=worker-retirement-resumed-after-scheduler-owner-release")
        .expect("two-hart fault regression proves retirement resumes after guard release");
    let quiesced = RETIREMENT_SELFTEST
        .find("stage=hart1-fault-retirement-quiesced")
        .expect("faulted worker must reach terminal quiescence before PASS");
    let worker_retired = RETIREMENT_SELFTEST
        .find("stage=hart1-worker-retired-by-scheduler")
        .expect("two-hart fault regression records scheduler-owned worker retirement");
    let aggregate = RETIREMENT_SELFTEST
        .find("SMP-FAULT-RETIREMENT: PASS")
        .expect("two-hart fault regression retains its aggregate PASS marker");
    assert!(
        worker_retired < aggregate,
        "scheduler-owned worker retirement must be source-ordered before the stable fault-retirement aggregate PASS"
    );
    assert!(
        released < aggregate,
        "H0's guard-release stage must be emitted before the stable fault-retirement aggregate PASS"
    );
    assert!(
        pre_lock < blocked
            && blocked < released
            && released < resumed
            && resumed < quiesced
            && quiesced < aggregate,
        "H1's pre-lock handoff must precede H0 guard retention, release, retirement, terminal quiescence, and aggregate PASS"
    );
}

#[test]
fn atomic_publication_aggregate_follows_prerequisite_in_source() {
    let prerequisite = ATOMIC_PUBLICATION_CASES
        .find("ATOMIC_PUBLICATION_PREREQUISITE_COMPLETE / PHASE07_BLOCKED")
        .expect("atomic-publication source retains the prerequisite terminal");
    let aggregate = ATOMIC_PUBLICATION_CASES[prerequisite..]
        .find("ATOMIC_PUBLICATION_ALL: PASS")
        .map(|offset| prerequisite + offset)
        .expect("atomic-publication source retains the aggregate terminal");
    assert!(
        prerequisite < aggregate,
        "the stable aggregate terminal must remain source-ordered after its prerequisite"
    );
}

#[test]
fn rv64_ipc_block_before_yield_wake_handoff_is_marked_and_gated() {
    let send_arm = KERNEL_TASK
        .find("arm_ipc_block_handoff(caller_id);")
        .expect("blocking IPC send arms its outgoing Context before publication");
    let sending = KERNEL_TASK[send_arm..]
        .find("caller.state = TaskState::Sending")
        .map(|offset| send_arm + offset)
        .expect("blocking IPC send publishes Sending after arming");
    let recv_arm = KERNEL_TASK[sending..]
        .find("arm_ipc_block_handoff(caller_id);")
        .map(|offset| sending + offset)
        .expect("blocking IPC receive arms its outgoing Context before publication");
    let receiving = KERNEL_TASK[recv_arm..]
        .find("caller.state = TaskState::Recv")
        .map(|offset| recv_arm + offset)
        .expect("blocking IPC receive publishes Recv after arming");
    assert!(
        send_arm < sending && recv_arm < receiving,
        "an externally wakeable IPC state must never precede its Context handoff"
    );

    assert!(
        SCHEDULER.contains("rl::pick_local_eligible(hart_id)")
            && SCHEDULER.contains("rl::begin_outgoing_context_save(hart_id, cid);"),
        "selection and task-to-idle must retain the outgoing save handoff"
    );
    for ownership in [
        "current_task_id_for(owner_hart) == task_id",
        "selected_task_id_for(owner_hart) == task_id",
        "executing_task_id_for(owner_hart) == task_id",
        "outgoing_context_save_task_id_for(owner_hart) == task_id",
    ] {
        assert!(
            include_str!("../../../kernel/src/task/hart_local/ready.rs").contains(ownership),
            "remote selection must reject the {ownership} ownership window"
        );
    }

    let stages = [
        (
            "CTX00",
            "stage=blocked-before-yield hart={} marker=CTX-HANDOFF-00",
        ),
        (
            "CTX01",
            "stage=remote-wake-deferred marker=CTX-HANDOFF-01",
        ),
        (
            "CTX02",
            "stage=origin-context-saved hart={} marker=CTX-HANDOFF-02",
        ),
        ("CTX03", "PASS marker=CTX-HANDOFF-03 hart=0"),
    ];
    let aggregate = CONTEXT_HANDOFF_SELFTEST
        .find("SMP-CONTEXT-HANDOFF: PASS aggregate=CTX00-03")
        .expect("deterministic two-hart handoff regression must retain its aggregate PASS");
    for (stage, marker) in stages {
        let position = CONTEXT_HANDOFF_SELFTEST
            .find(marker)
            .unwrap_or_else(|| panic!("deterministic two-hart handoff regression must retain {stage}"));
        assert!(
            position < aggregate,
            "{stage} source stage must precede the stable aggregate PASS"
        );
    }
}

#[test]
fn vfs_test_client_never_wildcard_receives_replies() {
    assert!(
        !VFS_TEST_CLIENT.contains("sys_recv(0"),
        "VFS request/reply transport must not wildcard receive:\n{VFS_TEST_CLIENT}",
    );
}

#[test]
fn riscv64_vfs_smp_all_pass() {
    if !prerequisites_ok() {
        return;
    }

    let runner = QemuRunner::boot_rv64_smp(&test_hooks_kernel(), 2);
    wait_for_or_dump(&runner, "[smp] hart 1 online");
    // Per-hart UART writes can interleave stage records. The source-order
    // contract above proves the H0 release stage precedes this stable
    // aggregate lifecycle proof.
    wait_for_or_dump(
        &runner,
        "[selftest] SMP-CONTEXT-HANDOFF: PASS aggregate=CTX00-03",
    );
    wait_for_or_dump(
        &runner,
        "[selftest] SMP-FAULT-RETIREMENT: stage=hart1-scheduler-funnel-pre-lock-attempt-published",
    );
    wait_for_or_dump(
        &runner,
        "[selftest] SMP-FAULT-RETIREMENT: stage=hart0-scheduler-owner-retained-hart1-retirement-blocked",
    );
    wait_for_or_dump(&runner, "[selftest] SMP-FAULT-RETIREMENT: PASS");
    wait_for_or_dump(
        &runner,
        "[selftest] SMP-FAULT-RETIREMENT: stage=hart1-fault-retirement-quiesced",
    );
    wait_for_or_dump(
        &runner,
        "[selftest] SMP-RETIREMENT: stage=selected-context-blocked-retirement-and-reap",
    );
    // The retirement boundary is a separate, stable post-switch proof; its
    // per-hart progress records need not be contiguous in the UART stream.
    wait_for_or_dump(
        &runner,
        "[selftest] SMP-RETIREMENT: stage=rv64-switch-boundary hart=1",
    );
    wait_for_or_dump(
        &runner,
        "[selftest] SMP-RETIREMENT: stage=idle-attribution-cleared current=0 executing=0 selected=0 cell=0",
    );
    wait_for_or_dump(
        &runner,
        "[selftest] SMP-RETIREMENT: PASS (selected Context + zombie switch completion gate owner release + CellId reuse)",
    );
    wait_for_or_dump(
        &runner,
        "[selftest] HEARTBEAT-TERMINAL-IDENTITY: PASS (heartbeat retirement retained nonzero caller through boot switch; ReadLog denied)",
    );
    // Init must survive enough two-hart scheduling pressure to finish its
    // VFS/config launch sequence before the VFS client exercises the service.
    wait_for_or_dump(&runner, "Init: services spawned.");
    wait_for_or_dump(&runner, "Init: service registry verified.");


    for marker in [
        "ATOMIC_PUBLICATION_AP-00: PASS",
        "ATOMIC_PUBLICATION_AP-01: PASS",
        "ATOMIC_PUBLICATION_AP-02: PASS",
        "ATOMIC_PUBLICATION_AP-03: PASS",
        "ATOMIC_PUBLICATION_AP-04: PASS",
        "ATOMIC_PUBLICATION_AP-05: PASS",
        "ATOMIC_PUBLICATION_AP-06: PASS",
        "ATOMIC_PUBLICATION_AP-07: PASS",
        "ATOMIC_PUBLICATION_AP-08: PASS",
        "ATOMIC_PUBLICATION_AP-09: PASS",
        "ATOMIC_PUBLICATION_AP-10: PASS",
        "ATOMIC_PUBLICATION_AP-11: PASS",
        "ATOMIC_PUBLICATION_AP-12: PASS",
        "ATOMIC_PUBLICATION_AP-13: PASS",
        "ATOMIC_PUBLICATION_AP-14: PASS",
        "ATOMIC_PUBLICATION_AP-15: PASS",
        "ATOMIC_PUBLICATION_ALL: PASS",
    ] {
        wait_for_or_dump(&runner, marker);
    }
    wait_for_or_dump(
        &runner,
        "[selftest] VFS-LIFETIME: stage=dead-owner-pending-revoke-exact-release",
    );
    wait_for_or_dump(
        &runner,
        "[selftest] VFS-LIFETIME: stage=smp-stale-context-denied-capacity-preserved",
    );
    wait_for_or_dump(
        &runner,
        "[selftest] VFS-LIFETIME: PASS (exact lease + quarantine + owner watch + SMP stale-install denial)",
    );

    wait_for_or_dump(&runner, "[vfs-test] ALL TESTS PASSED");

    let serial = runner.dump();
    assert!(
        !serial.lines().any(|line| line.contains("[FAIL]")),
        "two-hart VFS runner must have no VFS failure lines:\n{serial}",
    );
    assert!(
        !serial.contains("SMP-CONTEXT-HANDOFF: FAIL"),
        "two-hart VFS runner must not report a failed Context handoff:\n{serial}",
    );
    assert!(
        !serial.contains("[vfs-test] FAILURES DETECTED"),
        "two-hart VFS runner must not report test failures:\n{serial}",
    );
}
