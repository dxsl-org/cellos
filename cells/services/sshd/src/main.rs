//! SSH-2.0 Server daemon for Cellos.
//! Listens on TCP port 22 and provides an authenticated remote shell session.

#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![forbid(unsafe_code)]

extern crate alloc;
use alloc::vec::Vec;
use api::ipc::{NetRequest, NetResponse, IPC_BUF_SIZE};
use api::syscall::service;
use ostd::io::println;
use ostd::syscall::{sys_get_procs, sys_get_scheduler_ticks, sys_lookup_service, sys_yield};

mod cipher;
mod kex;
mod session;
mod ssh_packet;

use session::{SessionPhase, SshSession};

api::declare_syscalls![Send, Recv, Log, LookupService, GetTime, GetProcs];

#[cfg(target_os = "none")]
ostd::declare_custom_heap!(6 * 1024 * 1024);

const SSH_PORT: u16 = 22;

#[cfg(not(test))]
ostd::cell_main!(cell_main);

fn cell_main() {
    #[cfg(target_os = "none")]
    init_custom_heap();

    println("[sshd] Cellos SSH-2.0 daemon starting...");

    // 1. Resolve network service
    let net_tid = loop {
        if let Some(tid) = sys_lookup_service(service::NET) {
            break tid;
        }
        sys_yield();
    };
    println("[sshd] Network service resolved");

    // 2. Open TCP listening socket on port 22
    let mut req_buf = [0u8; IPC_BUF_SIZE];
    let mut resp_buf = [0u8; IPC_BUF_SIZE];

    let listen_req = NetRequest::TcpListen { port: SSH_PORT };
    let listen_cap = loop {
        let resp = ostd::ipc::service_call_typed::<NetRequest<'_>, NetResponse<'_>>(
            net_tid,
            &listen_req,
            &mut req_buf,
            &mut resp_buf,
        );
        match resp {
            Ok(NetResponse::CapId(cap)) => {
                println("[sshd] TCP listener active on 0.0.0.0:22");
                break cap;
            }
            _ => {
                sys_yield();
            }
        }
    };

    // 3. Client connection slot
    let mut active_client: Option<(u32, SshSession)> = None;

    // 4. Reactor loop
    let accept_req = NetRequest::TcpAccept { cap_id: listen_cap };

    loop {
        let mut work_done = false;

        // Try accepting new connection if slot is empty
        if active_client.is_none() {
            let resp = ostd::ipc::service_call_typed::<NetRequest<'_>, NetResponse<'_>>(
                net_tid,
                &accept_req,
                &mut req_buf,
                &mut resp_buf,
            );
            if let Ok(NetResponse::CapId(client_cap)) = resp {
                println("[sshd] Client connected! Starting SSH handshake...");
                let mut session = SshSession::new();
                let initial = session.initial_outbound();

                // Send version banner: SSH-2.0-Cellos_1.0\r\n
                let send_req = NetRequest::TcpSend {
                    cap_id: client_cap,
                    data: &initial,
                };
                let _ = ostd::ipc::service_call_typed::<NetRequest<'_>, NetResponse<'_>>(
                    net_tid,
                    &send_req,
                    &mut req_buf,
                    &mut resp_buf,
                );

                active_client = Some((client_cap, session));
                work_done = true;
            }
        }

        // Process active client
        if let Some((client_cap, ref mut session)) = active_client {
            let recv_req = NetRequest::TcpRecv {
                cap_id: client_cap,
                buf_len: 2048,
            };
            let resp = ostd::ipc::service_call_typed::<NetRequest<'_>, NetResponse<'_>>(
                net_tid,
                &recv_req,
                &mut req_buf,
                &mut resp_buf,
            );

            match resp {
                Ok(NetResponse::Data(chunk)) if !chunk.is_empty() => {
                    work_done = true;
                    let outbound = session.process_incoming(chunk);
                    if !outbound.is_empty() {
                        // Send in chunks <= 2048
                        for slice in outbound.chunks(2048) {
                            let send_req = NetRequest::TcpSend {
                                cap_id: client_cap,
                                data: slice,
                            };
                            let _ = ostd::ipc::service_call_typed::<NetRequest<'_>, NetResponse<'_>>(
                                net_tid,
                                &send_req,
                                &mut req_buf,
                                &mut resp_buf,
                            );
                        }
                    }

                    if session.phase == SessionPhase::Closed {
                        println("[sshd] Session closed, closing socket");
                        let close_req = NetRequest::TcpClose { cap_id: client_cap };
                        let _ = ostd::ipc::service_call_typed::<NetRequest<'_>, NetResponse<'_>>(
                            net_tid,
                            &close_req,
                            &mut req_buf,
                            &mut resp_buf,
                        );
                        active_client = None;
                    }
                }
                Ok(NetResponse::Eof) | Ok(NetResponse::Err(_)) | Err(_) => {
                    println("[sshd] Client disconnected or socket closed");
                    let close_req = NetRequest::TcpClose { cap_id: client_cap };
                    let _ = ostd::ipc::service_call_typed::<NetRequest<'_>, NetResponse<'_>>(
                        net_tid,
                        &close_req,
                        &mut req_buf,
                        &mut resp_buf,
                    );
                    active_client = None;
                    work_done = true;
                }
                _ => {}
            }
        }

        if !work_done {
            sys_yield();
        }
    }
}

#[cfg(test)]
fn main() {}
