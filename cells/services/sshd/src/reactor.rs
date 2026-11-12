//! The TCP side: listen on :22, serve one connection at a time.

use api::ipc::{NetRequest, NetResponse, IPC_BUF_SIZE};
use api::syscall::service;
use ed25519_compact::KeyPair;
use ostd::service::provider_tid;
use ostd::syscall::{sys_get_time_ms, sys_yield};

use crate::session::{SessionPhase, SshSession};
use crate::{authorized_keys, kex, log, LOGIN_GRACE_MS};

const SSH_PORT: u16 = 22;
const RECV_CHUNK: u32 = 2048;

struct Net {
    tid: usize,
    req: [u8; IPC_BUF_SIZE],
    resp: [u8; IPC_BUF_SIZE],
}

impl Net {
    fn call(&mut self, request: &NetRequest<'_>) -> Result<NetResponse<'_>, ()> {
        ostd::ipc::service_call_typed::<NetRequest<'_>, NetResponse<'_>>(
            self.tid,
            request,
            &mut self.req,
            &mut self.resp,
        )
        .map_err(|_| ())
    }

    fn send(&mut self, cap_id: u32, bytes: &[u8]) {
        for data in bytes.chunks(RECV_CHUNK as usize) {
            let _ = self.call(&NetRequest::TcpSend { cap_id, data });
        }
    }

    fn close(&mut self, cap_id: u32) {
        let _ = self.call(&NetRequest::TcpClose { cap_id });
    }
}

fn now_ms() -> u64 {
    sys_get_time_ms().unwrap_or(0)
}

struct Client {
    cap: u32,
    session: SshSession,
    accepted_ms: u64,
}

pub fn run(host_key: KeyPair) -> ! {
    let tid = loop {
        if let Some(tid) = provider_tid(service::NET) {
            break tid;
        }
        sys_yield();
    };
    let mut net = Net {
        tid,
        req: [0; IPC_BUF_SIZE],
        resp: [0; IPC_BUF_SIZE],
    };
    let listen_cap = loop {
        if let Ok(NetResponse::CapId(cap)) = net.call(&NetRequest::TcpListen { port: SSH_PORT }) {
            log("[sshd] TCP listener active on 0.0.0.0:22");
            break cap;
        }
        sys_yield();
    };

    let mut active: Option<Client> = None;
    loop {
        let mut work_done = false;

        if active.is_none() {
            if let Ok(NetResponse::CapId(cap)) =
                net.call(&NetRequest::TcpAccept { cap_id: listen_cap })
            {
                log("[sshd] client connected");
                // Re-read per connection so a key added or removed takes effect
                // for the next login without a restart.
                let session = SshSession::new(
                    host_key.clone(),
                    kex::kernel_entropy,
                    authorized_keys::load(),
                );
                net.send(cap, session.initial_outbound());
                active = Some(Client {
                    cap,
                    session,
                    accepted_ms: now_ms(),
                });
                work_done = true;
            }
        }

        let Some(client) = active.as_mut() else {
            sys_yield();
            continue;
        };
        let cap = client.cap;
        let mut done = false;
        match net.call(&NetRequest::TcpRecv {
            cap_id: cap,
            buf_len: RECV_CHUNK,
        }) {
            Ok(NetResponse::Data(chunk)) if !chunk.is_empty() => {
                work_done = true;
                let outbound = client.session.process_incoming(chunk);
                net.send(cap, &outbound);
                done = client.session.phase == SessionPhase::Closed;
            }
            Ok(NetResponse::Eof) | Ok(NetResponse::Err(_)) | Err(_) => {
                log("[sshd] client disconnected");
                done = true;
            }
            _ => {}
        }
        if !done
            && !client.session.is_authenticated()
            && now_ms().saturating_sub(client.accepted_ms) > LOGIN_GRACE_MS
        {
            log("[sshd] login grace time expired");
            done = true;
        }
        if done {
            net.close(cap);
            active = None;
            work_done = true;
        }
        if !work_done {
            sys_yield();
        }
    }
}
