#![no_std]
#![no_main]
#![forbid(unsafe_code)]
// The `main` symbol comes from `ostd::cell_main!`, whose expansion carries the
// `#[no_mangle]` the ELF loader needs without tripping `forbid(unsafe_code)`.

extern crate alloc;

mod framework;
mod scenarios;

api::declare_syscalls![
    Send,
    Recv,
    RecvTimeout,
    TryRecv,
    Log,
    Heartbeat,
    LookupService,
    ResolveCellOwner,
    GetTime,
    SetTimer,
    SpawnPinned,
    StateStash,
    StateRestore,
    Snapshot,
    MemInfo,
    Exit,
    Yield,
    VfsMutate
];
api::declare_manifest!(block_io = false, network = false, spawn = true);

use api::benchmark::{BenchReport, ViBenchmark};
use framework::{report, runner};
use ostd::io::println;
use scenarios::{
    context_switch::ContextSwitchBench, ipc_fastpath::IpcFastpathBench,
    ipc_send_recv::IpcSendRecvBench, memory_footprint::MemoryFootprintBench,
    syscall_yield::SyscallYieldBench,
};

const PROFILE: &str = "rv64-qemu-virt-2h-256m-v2";

/// PDR performance targets (nanoseconds). All checked against p99.
const TARGET_CTX_SWITCH_NS: u64 = 100_000;
const HARDWARE_TARGET_IPC_NS: u64 = 50_000;
const HARDWARE_TARGET_FASTPATH_IPC_NS: u64 = 10_000;
const TARGET_SYSCALL_NS: u64 = 40_000;
const TARGET_FOOTPRINT_BYTES: u64 = 10 * 1024 * 1024;

const SELF_PATH: &str = "/bin/bench-probe";
const LOAD_CELLS: usize = 3;

fn emit_invalid(invalid: &mut u32, scenario: &str, stage: &str) {
    println(&alloc::format!(
        "{{\"bench_event\":\"invalid\",\"scenario\":\"{}\",\"stage\":\"{}\"}}",
        scenario,
        stage
    ));
    *invalid = invalid.saturating_add(1);
}

fn emit_run_failure(invalid: &mut u32, scenario: &str, failure: runner::RunFailure) {
    println(&alloc::format!(
        "[bench] {} INVALID at {}: {:?}",
        scenario,
        failure.stage.as_str(),
        failure.error
    ));
    emit_invalid(invalid, scenario, failure.stage.as_str());
    if let Some(error) = failure.teardown_error {
        println(&alloc::format!(
            "[bench] {} cleanup also failed: {:?}",
            scenario,
            error
        ));
        emit_invalid(invalid, scenario, runner::RunStage::Teardown.as_str());
    }
}

fn run_metric<B: ViBenchmark>(
    scenario: &'static str,
    bench: &mut B,
    warmup: u32,
    iters: u32,
    invalid: &mut u32,
) -> Option<BenchReport> {
    match runner::run(bench, warmup, iters) {
        Ok(mut result) => {
            result.name = scenario;
            Some(result)
        }
        Err(failure) => {
            emit_run_failure(invalid, scenario, failure);
            None
        }
    }
}

fn print_latency(result: &BenchReport) {
    report::print_report(result);
    report::print_json(result);
}

fn record_target(passed_target: bool, passed: &mut u32, failed: &mut u32) {
    if passed_target {
        *passed = passed.saturating_add(1);
    } else {
        *failed = failed.saturating_add(1);
    }
}

fn spawn_role(role: &str, priority: u8) -> Option<usize> {
    if !ostd::syscall::sys_set_spawn_args(role) {
        return None;
    }
    match ostd::syscall::sys_spawn_pinned(SELF_PATH, priority, 0) {
        ostd::syscall::SyscallResult::Ok(tid) if tid != 0 => Some(tid),
        _ => None,
    }
}

/// Attempt every requested load-cell admission, retaining successful tids for cleanup.
fn spawn_load() -> ([usize; LOAD_CELLS], bool) {
    use api::task::TaskPriority;
    let mut tids = [0usize; LOAD_CELLS];
    let mut complete = true;
    for slot in &mut tids {
        match spawn_role("load", TaskPriority::Normal as u8) {
            Some(tid) => *slot = tid,
            None => complete = false,
        }
    }
    (tids, complete)
}

/// Attempt every relevant teardown even after an earlier teardown fails.
fn kill_all(tids: &[usize]) -> bool {
    let mut complete = true;
    for &tid in tids {
        if tid != 0
            && !matches!(
                ostd::syscall::sys_force_exit(tid),
                ostd::syscall::SyscallResult::Ok(_)
            )
        {
            complete = false;
        }
    }
    complete
}

fn run_rt_preempt(invalid: &mut u32, passed: &mut u32, failed: &mut u32) {
    use api::task::TaskPriority;

    let (load_tids, load_complete) = spawn_load();
    if !load_complete {
        println("[rt] preempt_latency INVALID — background load admission failed");
        emit_invalid(invalid, "preempt_latency", "setup");
        if !kill_all(&load_tids) {
            emit_invalid(invalid, "preempt_latency", "teardown");
        }
        return;
    }

    let Some(probe_tid) = spawn_role("rt-probe", TaskPriority::RealTime as u8) else {
        println("[rt] preempt_latency INVALID — probe admission failed");
        emit_invalid(invalid, "preempt_latency", "setup");
        if !kill_all(&load_tids) {
            emit_invalid(invalid, "preempt_latency", "teardown");
        }
        return;
    };
    for _ in 0..100 {
        ostd::task::yield_now();
    }

    let result = scenarios::preempt_latency::measure(probe_tid);
    let probe_cleanup = matches!(
        ostd::syscall::sys_force_exit(probe_tid),
        ostd::syscall::SyscallResult::Ok(_)
    );
    let load_cleanup = kill_all(&load_tids);

    match result {
        Err(error) => {
            println(&alloc::format!(
                "[rt] preempt_latency INVALID at measure: {:?}",
                error
            ));
            emit_invalid(invalid, "preempt_latency", "measure");
            if !probe_cleanup || !load_cleanup {
                emit_invalid(invalid, "preempt_latency", "teardown");
            }
        }
        Ok(_) if !probe_cleanup || !load_cleanup => {
            emit_invalid(invalid, "preempt_latency", "teardown");
        }
        Ok(result) => {
            result.print();
            result.print_json();
            let target_met = result.meets(200_000);
            record_target(target_met, passed, failed);
            if target_met {
                println("[rt] preempt_latency PASS");
            } else {
                println("[rt] preempt_latency FAIL (p99 over 200µs or deadline miss)");
            }
        }
    }
}

fn run_rt_control_loop(invalid: &mut u32, passed: &mut u32, failed: &mut u32) {
    use api::task::TaskPriority;

    let (load_tids, load_complete) = spawn_load();
    if !load_complete {
        println("[rt] control_loop INVALID — background load admission failed");
        emit_invalid(invalid, "control_loop", "setup");
        if !kill_all(&load_tids) {
            emit_invalid(invalid, "control_loop", "teardown");
        }
        return;
    }

    let Some(probe_tid) = spawn_role("ctl-loop", TaskPriority::RealTime as u8) else {
        println("[rt] control_loop INVALID — probe admission failed");
        emit_invalid(invalid, "control_loop", "setup");
        if !kill_all(&load_tids) {
            emit_invalid(invalid, "control_loop", "teardown");
        }
        return;
    };
    if !matches!(
        ostd::syscall::sys_notify_on_exit(probe_tid),
        ostd::syscall::SyscallResult::Ok(_)
    ) {
        println("[rt] control_loop INVALID — probe exit watch failed");
        emit_invalid(invalid, "control_loop", "setup");
        let probe_cleanup = matches!(
            ostd::syscall::sys_force_exit(probe_tid),
            ostd::syscall::SyscallResult::Ok(_)
        );
        let load_cleanup = kill_all(&load_tids);
        if !probe_cleanup || !load_cleanup {
            emit_invalid(invalid, "control_loop", "teardown");
        }
        return;
    }
    for _ in 0..100 {
        ostd::task::yield_now();
    }
    let mut result_buf = [0xa5u8; 64];
    let mut received_response = false;
    let result = if !matches!(
        ostd::syscall::sys_send(probe_tid, &[0u8]),
        ostd::syscall::SyscallResult::Ok(_)
    ) {
        Err(api::ViError::IO)
    } else {
        match ostd::syscall::sys_recv(probe_tid, &mut result_buf) {
            ostd::syscall::SyscallResult::Ok(sender) if sender == probe_tid => {
                received_response = true;
                scenarios::control_loop::decode_result(&result_buf)
            }
            ostd::syscall::SyscallResult::Ok(_) => Err(api::ViError::InvalidInput),
            ostd::syscall::SyscallResult::Err(_) => Err(api::ViError::IO),
        }
    };

    let probe_cleanup = if received_response {
        let mut exit_reason = [0u8; 8];
        matches!(
            ostd::syscall::sys_recv(probe_tid, &mut exit_reason),
            ostd::syscall::SyscallResult::Ok(sender) if sender == probe_tid
        )
    } else {
        matches!(
            ostd::syscall::sys_force_exit(probe_tid),
            ostd::syscall::SyscallResult::Ok(_)
        )
    };
    let load_cleanup = kill_all(&load_tids);
    match result {
        Err(error) => {
            println(&alloc::format!(
                "[rt] control_loop INVALID at measure: {:?}",
                error
            ));
            emit_invalid(invalid, "control_loop", "measure");
            if !probe_cleanup || !load_cleanup {
                emit_invalid(invalid, "control_loop", "teardown");
            }
        }
        Ok(_) if !probe_cleanup || !load_cleanup => {
            emit_invalid(invalid, "control_loop", "teardown")
        }
        Ok(result) => {
            result.print();
            result.print_json();
            let target_met = result.deadline_miss == 0;
            record_target(target_met, passed, failed);
            if target_met {
                println("[rt] control_loop PASS");
            } else {
                println("[rt] control_loop FAIL (deadline misses)");
            }
        }
    }
}

fn run_rt_under_load(invalid: &mut u32) {
    let ipc_idle = run_metric(
        "ipc_send_recv_idle",
        &mut IpcSendRecvBench::new(),
        runner::DEFAULT_WARMUP,
        runner::DEFAULT_ITERS,
        invalid,
    );
    if let Some(result) = ipc_idle.as_ref() {
        print_latency(result);
    }
    let syscall_idle = run_metric(
        "syscall_yield_idle",
        &mut SyscallYieldBench,
        runner::DEFAULT_WARMUP,
        runner::DEFAULT_ITERS,
        invalid,
    );
    if let Some(result) = syscall_idle.as_ref() {
        print_latency(result);
    }

    let (load_tids, load_complete) = spawn_load();
    if !load_complete {
        println("[rt] under_load INVALID — not all load cells were admitted");
        emit_invalid(invalid, "ipc_send_recv_load", "setup");
        emit_invalid(invalid, "syscall_yield_load", "setup");
        if !kill_all(&load_tids) {
            emit_invalid(invalid, "ipc_send_recv_load", "teardown");
            emit_invalid(invalid, "syscall_yield_load", "teardown");
        }
        return;
    }
    for _ in 0..100 {
        ostd::task::yield_now();
    }

    let ipc_load = run_metric(
        "ipc_send_recv_load",
        &mut IpcSendRecvBench::new(),
        runner::DEFAULT_WARMUP,
        runner::DEFAULT_ITERS,
        invalid,
    );
    let syscall_load = run_metric(
        "syscall_yield_load",
        &mut SyscallYieldBench,
        runner::DEFAULT_WARMUP,
        runner::DEFAULT_ITERS,
        invalid,
    );
    let load_cleanup = kill_all(&load_tids);

    if !load_cleanup {
        emit_invalid(invalid, "ipc_send_recv_load", "teardown");
        emit_invalid(invalid, "syscall_yield_load", "teardown");
        return;
    }

    if let Some(result) = ipc_load.as_ref() {
        print_latency(result);
    }
    if let Some(result) = syscall_load.as_ref() {
        print_latency(result);
    }
    if let (Some(idle), Some(load)) = (ipc_idle.as_ref(), ipc_load.as_ref()) {
        print_under_load("ipc_send_recv", idle.p99, load.p99);
    }
    if let (Some(idle), Some(load)) = (syscall_idle.as_ref(), syscall_load.as_ref()) {
        print_under_load("syscall_yield", idle.p99, load.p99);
    }
}

fn print_under_load(name: &str, idle_p99: u64, load_p99: u64) {
    let ratio = load_p99
        .saturating_mul(100)
        .checked_div(idle_p99)
        .unwrap_or(0);
    println(&alloc::format!(
        "[rt] {:14} idle_p99={}ns load_p99={}ns ratio={}.{:02}x",
        name,
        idle_p99,
        load_p99,
        ratio / 100,
        ratio % 100
    ));
}

fn run_breakdown<B: ViBenchmark>(scenario: &'static str, bench: &mut B, invalid: &mut u32) {
    if let Some(result) = run_metric(scenario, bench, 2, 10, invalid) {
        print_latency(&result);
    }
}

ostd::cell_main!(cell_main);

fn cell_main() {
    let argv = ostd::args();
    let role = argv.first().map(|arg| arg.as_str()).unwrap_or("");
    if role.starts_with("hotswap-cached-inc:") {
        scenarios::hotswap_supervisor::run_cached_sender_probe(role);
    }
    if role.starts_with("native-stateful-cached-inc:") {
        scenarios::native_stateful::run_cached_sender_probe(role);
    }
    match role {
        "c2c-broker-oracle" => scenarios::c2c_broker_oracle::run(),
        "load" => scenarios::rt_load::run_load(),
        "rt-probe" => scenarios::preempt_latency::run_probe(),
        "ctl-loop" => scenarios::control_loop::run_control_loop(),
        "ipc-echo" => {
            let mut buf = [0xa5u8; 64];
            loop {
                buf.fill(0xa5);
                let sender = match ostd::syscall::sys_recv(0, &mut buf) {
                    ostd::syscall::SyscallResult::Ok(sid) if sid != 0 => sid,
                    _ => continue,
                };
                let valid_request = buf[0] == 0x42 && buf[1..].iter().all(|&byte| byte == 0);
                let reply = if valid_request { 0 } else { 1 };
                let _ = ostd::syscall::sys_send(sender, &[reply]);
            }
        }
        "resp-echo" => scenarios::vfs_getfile_breakdown::run_resp_echo(),
        "smp-worker" => scenarios::smp::run_worker(),
        "peer-death-guard" => scenarios::smp::run_peer_death_guard(),
        "hotswap-cli-probe" => scenarios::hotswap_cli_probe::run(),
        "hotswap-supervisor" => scenarios::hotswap_supervisor::run(),
        "hotswap-unauthorized" => scenarios::hotswap_supervisor::run_unauthorized_probe(),
        "snapshot-authority" => scenarios::snapshot_authority::run(),
        "native-stateful" => scenarios::native_stateful::run(),
        "lab-carrier-transfer" => scenarios::lab_carrier_transfer::run(),
        "base-tray-handoff" => scenarios::base_tray_handoff::run(),
        "stationary-assembly" => scenarios::stationary_assembly::run(),
        _ => {}
    }

    println(&alloc::format!(
        "{{\"bench_event\":\"start\",\"profile\":\"{}\"}}",
        PROFILE
    ));
    println("[bench] Cellos Performance Benchmark Suite v0.1");
    println("[bench] gates: qemu ctx<100µs syscall<40µs; hardware ipc<50µs; mem<10MB");
    println("");

    let mut passed = 0u32;
    let mut failed = 0u32;
    let mut informational = 0u32;
    let mut invalid = 0u32;

    if let Some(result) = run_metric(
        "context_switch",
        &mut ContextSwitchBench,
        runner::DEFAULT_WARMUP,
        runner::DEFAULT_ITERS,
        &mut invalid,
    ) {
        print_latency(&result);
        let target_met = result.meets_target(TARGET_CTX_SWITCH_NS);
        record_target(target_met, &mut passed, &mut failed);
        if target_met {
            println("[bench] context_switch PASS");
        } else {
            println("[bench] context_switch FAIL (p99 exceeds 100µs target)");
        }
    }

    if let Some(result) = run_metric(
        "ipc_send_recv",
        &mut IpcSendRecvBench::new(),
        runner::DEFAULT_WARMUP,
        runner::DEFAULT_ITERS,
        &mut invalid,
    ) {
        print_latency(&result);
        informational = informational.saturating_add(1);
        if result.meets_target(HARDWARE_TARGET_IPC_NS) {
            println("[bench] ipc_send_recv HW-TARGET-MET (QEMU evidence only)");
        } else {
            println("[bench] ipc_send_recv HW-TARGET-MISS (QEMU evidence only)");
        }
    }

    if let Some(result) = run_metric(
        "ipc_fastpath",
        &mut IpcFastpathBench::new(),
        runner::DEFAULT_WARMUP,
        runner::DEFAULT_ITERS,
        &mut invalid,
    ) {
        print_latency(&result);
        informational = informational.saturating_add(1);
        if result.meets_target(HARDWARE_TARGET_FASTPATH_IPC_NS) {
            println("[bench] ipc_fastpath HW-TARGET-MET (P99 <= 10us)");
        } else {
            println("[bench] ipc_fastpath HW-TARGET-MISS (P99 > 10us)");
        }
    }

    if let Some(result) = run_metric(
        "syscall_yield",
        &mut SyscallYieldBench,
        runner::DEFAULT_WARMUP,
        runner::DEFAULT_ITERS,
        &mut invalid,
    ) {
        print_latency(&result);
        let target_met = result.meets_target(TARGET_SYSCALL_NS);
        record_target(target_met, &mut passed, &mut failed);
        if target_met {
            println("[bench] syscall_yield PASS");
        } else {
            println(
                "[bench] syscall_yield FAIL (p99 exceeds 40µs QEMU target; real-HW target: 10µs)",
            );
        }
    }

    {
        let mut memory = MemoryFootprintBench::new();
        match runner::run(&mut memory, 0, 1) {
            Err(failure) => emit_run_failure(&mut invalid, "memory_footprint", failure),
            Ok(_) => {
                let bytes = memory.bytes();
                println(&alloc::format!(
                    "[bench] allocator_committed_bytes={}",
                    bytes
                ));
                report::print_memory_json("memory_footprint", bytes);
                let target_met = bytes <= TARGET_FOOTPRINT_BYTES;
                record_target(target_met, &mut passed, &mut failed);
                if target_met {
                    println("[bench] memory_footprint PASS");
                } else {
                    println("[bench] memory_footprint FAIL (exceeds 10 MB target)");
                }
            }
        }
    }

    println("");
    println("[rt] Real-time latency suite (under load):");
    run_rt_preempt(&mut invalid, &mut passed, &mut failed);
    run_rt_control_loop(&mut invalid, &mut passed, &mut failed);
    run_rt_under_load(&mut invalid);

    println("");
    println("[smp] SMP throughput suite (2-hart work-stealing):");
    for outcome in scenarios::smp::run_smp_suite() {
        match outcome {
            scenarios::smp::SmpOutcome::Metric(metric) => {
                report::print_value_json(metric.name, metric.n, metric.value);
                record_target(metric.passed, &mut passed, &mut failed);
            }
            scenarios::smp::SmpOutcome::Invalid(failure) => {
                println(&alloc::format!(
                    "[smp] {} INVALID at {}",
                    failure.name,
                    failure.stage
                ));
                emit_invalid(&mut invalid, failure.name, failure.stage);
                if failure.teardown_failed {
                    emit_invalid(&mut invalid, failure.name, "teardown");
                }
            }
        }
    }

    {
        use scenarios::vfs_getfile_breakdown as vgb;
        println("");
        println("[breakdown] service call — per-stage cost (each sample = 1000 ops)");
        run_breakdown(
            "stage_encode_request_x1000",
            &mut vgb::EncodeRequestBench::new(),
            &mut invalid,
        );
        run_breakdown(
            "stage_decode_reply_x1000",
            &mut vgb::DecodeReplyBench::new(),
            &mut invalid,
        );
        run_breakdown(
            "stage_ecall_roundtrip_x1000",
            &mut vgb::TrapRoundTripBench,
            &mut invalid,
        );
        run_breakdown(
            "total_typed_roundtrip_x1000",
            &mut vgb::RoundTripBench::new(),
            &mut invalid,
        );
        println("[breakdown] done");
    }

    println("");
    println(&alloc::format!(
        "[bench] Results: {} PASS  {} FAIL  {} INFO  {} INVALID",
        passed,
        failed,
        informational,
        invalid
    ));
    if failed == 0 && invalid == 0 {
        println("[bench] ALL QEMU BENCHMARK GATES PASS");
    } else {
        println("[bench] BENCHMARK FAILURES OR INVALID EXPERIMENTS DETECTED");
    }
    // Compatibility marker consumed by the existing boot integration scenario.
    // Machine validity is determined exclusively by the strict JSON event below.
    println("[bench] BENCHMARK SUITE COMPLETE");
    println(&alloc::format!(
        "{{\"bench_event\":\"complete\",\"profile\":\"{}\",\"invalid\":{}}}",
        PROFILE,
        invalid
    ));
}
