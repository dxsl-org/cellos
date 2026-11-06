#![no_std]
#![no_main]
#![allow(dead_code)] // orchestrator-only items in shared modules are unreachable from this binary
#![forbid(unsafe_code)]

extern crate alloc;

mod echo;
mod framework;
mod scenarios;

// Probe/load roles do not spawn further children.
api::declare_syscalls![
    Send,
    Recv,
    RecvTimeout,
    TryRecv,
    Log,
    GetTime,
    Heartbeat,
    LookupService,
    ResolveCellOwner,
    SetTimer,
    // Phase 03d multi-source caller (`c2c-msw-caller`): test-cell authority for
    // the completion source; bit 42 is shared with WaitForEvent.
    WaitCompletion,
    StateRestore,
    Exit,
    Yield,
    // `fastpath-echo` resolves the host's channel Grant (owner-checked).
    GrantSlice
];
api::declare_manifest!(block_io = false, network = false, spawn = false);

ostd::cell_main!(cell_main);

fn cell_main() {
    let argv = ostd::args();
    let role = argv.first().map(|arg| arg.as_str()).unwrap_or("");
    // G1 concurrency is exclusively same-Cell threads in bench, never probe processes.
    #[cfg(feature = "g1-cpu-workload")]
    if role.starts_with("cpu-accounting:") {
        ostd::io::println("G1-WORKLOAD invalid=process-substitution");
        ostd::syscall::sys_exit(1);
    }
    ostd::io::println(&alloc::format!(
        "[bench-probe] Started with role: '{}'",
        role
    ));
    if role.starts_with("hotswap-cached-inc:") {
        scenarios::hotswap_supervisor::run_cached_sender_probe(role);
    }
    if role.starts_with("native-stateful-cached-inc:") {
        scenarios::native_stateful::run_cached_sender_probe(role);
    }
    if role.starts_with("c2c-sync-caller:") {
        scenarios::local_service_lifecycle::run_sync_caller(role);
    }
    if role.starts_with("c2c-doomed-caller:") {
        scenarios::local_service_lifecycle::run_doomed_caller(role);
    }
    if role.starts_with("c2c-full-caller:") {
        scenarios::local_service_lifecycle::run_full_caller(role);
    }
    if role.starts_with("c2c-pressure-caller:") {
        scenarios::local_service_lifecycle::run_pressure_caller(role);
    }
    if role.starts_with("c2c-sync-expiry-caller:") {
        scenarios::local_service_lifecycle::run_sync_expiry_caller(role);
    }
    if role.starts_with("c2c-echo-reverse:") {
        scenarios::async_lifecycle::run_echo_reverse(role);
    }
    match role {
        "parked-footprint-probe" => scenarios::parked_footprint::run_probe(),
        "c2c-client" => scenarios::c2c_broker_oracle::run_client(),
        "c2c-provider" => scenarios::local_service_lifecycle::run_provider(),
        "c2c-slow-parked" => scenarios::async_lifecycle::run_slow_parked(),
        "c2c-msw-caller" => scenarios::async_lifecycle::run_msw_caller(),
        "c2c-msw-echo" => scenarios::async_lifecycle::run_msw_echo(),
        "c2c-msw-slow" => scenarios::async_lifecycle::run_msw_slow(),
        "c2c-msw-hold" => scenarios::async_lifecycle::run_msw_hold(),
        "c2c-msw-raw" => scenarios::async_lifecycle::run_msw_raw(),
        "c2c-race-provider" => scenarios::async_lifecycle::run_race_provider(),
        "c2c-sync-hold" => scenarios::async_lifecycle::run_sync_hold(),
        "c2c-drop-provider" => scenarios::async_lifecycle::run_drop_provider(),
        "c2c-sync-expiry-holder" => scenarios::local_service_lifecycle::run_sync_expiry_holder(),
        "c2c-nested-reply" => scenarios::local_service_lifecycle::run_nested_provider(),
        "c2c-reply-loop" => scenarios::local_service_lifecycle::run_reply_loop(),
        "c2c-cancel-reply" => scenarios::local_service_lifecycle::run_cancel_provider(),
        "c2c-saturation" => scenarios::local_service_lifecycle::run_saturation_provider(),
        "c2c-deadline" => scenarios::local_service_lifecycle::run_deadline_provider(),
        "c2c-caller-death" => scenarios::local_service_lifecycle::run_caller_death_provider(),
        "c2c-restart-old" => scenarios::local_service_lifecycle::run_restart_old(),
        "c2c-restart-new" => scenarios::local_service_lifecycle::run_restart_new(),
        "c2c-event-provider" => scenarios::local_service_lifecycle::run_event_provider(),
        "c2c-raw-event" => scenarios::local_service_lifecycle::run_raw_event_source(),
        "c2c-input-provider" => scenarios::local_service_lifecycle::run_concurrent_input_provider(),
        "c2c-input-source" => scenarios::local_service_lifecycle::run_concurrent_input_source(),
        "c2c-multi-provider" => scenarios::local_service_lifecycle::run_multi_caller_provider(),
        "c2c-pressure-provider" => scenarios::local_service_lifecycle::run_pressure_provider(),
        "load" => scenarios::rt_load::run_load(),
        "rt-probe" => scenarios::preempt_latency::run_probe(),
        "ctl-loop" => scenarios::control_loop::run_control_loop(),
        "ipc-echo" => echo::run(),
        "fastpath-echo" => {
            let mut token_buf = [0u8; 8];
            let sender = match ostd::syscall::sys_recv(0, &mut token_buf) {
                ostd::syscall::SyscallResult::Ok(sid) if sid != 0 => sid,
                _ => return,
            };
            // The Grant must be owned by the kernel-stamped sender, so a token
            // from anyone else resolves to nothing.
            let token = u64::from_le_bytes(token_buf);
            let client = match ostd::ring_channel::ChannelClient::connect(token, sender) {
                Some(c) => c,
                None => return,
            };
            // Send ACK to orchestrator
            let _ = ostd::syscall::sys_send(sender, &[0x55]);

            let endpoint = client.endpoint();
            let mut req_buf = [0u8; 64];
            let mut resp_buf = [0u8; 1];

            while let Ok(meta) = endpoint.rx.recv_blocking(&mut req_buf) {
                let valid = req_buf[0] == 0x42 && req_buf[1..meta.len].iter().all(|&b| b == 0);
                resp_buf[0] = if valid { 0 } else { 1 };
                if endpoint.tx.send_blocking(&resp_buf, meta.seq, 0).is_err() {
                    break;
                }
            }
        }
        "resp-echo" => scenarios::vfs_getfile_breakdown::run_resp_echo(),
        "smp-worker" => scenarios::smp::run_worker(),
        "heartbeat-peer" => scenarios::smp::run_heartbeat_peer(),
        _ => {} // unknown role: exit cleanly
    }
}
