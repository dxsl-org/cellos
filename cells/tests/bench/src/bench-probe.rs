#![no_std]
#![no_main]
#![allow(dead_code)] // orchestrator-only items in shared modules are unreachable from this binary
#![forbid(unsafe_code)]

extern crate alloc;

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
    StateRestore,
    Exit,
    Yield
];
api::declare_manifest!(block_io = false, network = false, spawn = false);

ostd::cell_main!(cell_main);

fn cell_main() {
    let argv = ostd::args();
    let role = argv.first().map(|arg| arg.as_str()).unwrap_or("");
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
    if role.starts_with("c2c-echo-reverse:") {
        scenarios::async_lifecycle::run_echo_reverse(role);
    }
    match role {
        "c2c-client" => scenarios::c2c_broker_oracle::run_client(),
        "c2c-provider" => scenarios::local_service_lifecycle::run_provider(),
        "c2c-slow-parked" => scenarios::async_lifecycle::run_slow_parked(),
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
        "c2c-multi-provider" => scenarios::local_service_lifecycle::run_multi_caller_provider(),
        "c2c-pressure-provider" => scenarios::local_service_lifecycle::run_pressure_provider(),
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
        "fastpath-echo" => {
            let mut addr_buf = [0u8; 8];
            let sender = match ostd::syscall::sys_recv(0, &mut addr_buf) {
                ostd::syscall::SyscallResult::Ok(sid) if sid != 0 => sid,
                _ => return,
            };
            let handle = u64::from_le_bytes(addr_buf);
            let client = match ostd::ring_channel::ChannelClient::connect(handle) {
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
