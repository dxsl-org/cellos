use alloc::boxed::Box;
use alloc::string::String;
use api::config::ViConfig;
use api::ipc::{ConfigRequest, ConfigResponse, IPC_BUF_SIZE};
use ostd::prelude::*;
use ostd::sync::Mutex;

/// Client for the Config service.
///
/// Uses typed postcard IPC (`ConfigRequest` / `ConfigResponse`) matching the
/// config service v0.3 protocol.  Resolves the live Config endpoint via the
/// Service Registry on each call, so it transparently reconnects when the
/// supervisor respawns Config.
///
/// `ViConfig` requires `Sync`; the response buffer is therefore a `Mutex`, which
/// supplies that bound as a checked fact rather than the hand-written
/// `unsafe impl Sync` this type used to carry.
pub struct ConfigClient {
    /// Scratch space for the reply of the `get()` currently in flight.
    resp_buf: Mutex<[u8; IPC_BUF_SIZE]>,
}

impl ConfigClient {
    pub fn new() -> Self {
        Self {
            resp_buf: Mutex::new([0u8; IPC_BUF_SIZE]),
        }
    }

    fn endpoint() -> Option<usize> {
        for _ in 0..8 {
            if let Some(tid) = ostd::syscall::sys_lookup_service(api::syscall::service::CONFIG) {
                return Some(tid);
            }
            ostd::task::yield_now();
        }
        None
    }
}

impl Default for ConfigClient {
    fn default() -> Self {
        Self::new()
    }
}

impl ViConfig for ConfigClient {
    /// Fetch a value.
    ///
    /// Contract: the returned `&str` is **leaked** — it lives for the rest of the
    /// process. `ViConfig::get` hands back a borrow tied to `&self`, which cannot
    /// be produced from a locked buffer without laundering the lifetime through a
    /// raw pointer (what this used to do, unsoundly: the next `get()` overwrote
    /// the buffer a live `&str` still pointed at). Leaking is the honest way to
    /// satisfy the signature; callers must therefore not poll config in a loop.
    /// The shell currently never calls `get()`, so nothing leaks today — the fix
    /// is to change the trait to return `String`, which is a change to
    /// `libs/api` and out of this crate's scope.
    fn get(&self, key: &str) -> ViResult<&str> {
        let sid = Self::endpoint().ok_or(ViError::IO)?;

        let mut req_buf = [0u8; IPC_BUF_SIZE];
        let req = ConfigRequest::Get(key);
        let encoded = api::ipc::encode(&req, &mut req_buf).map_err(|_| ViError::IO)?;

        if let ostd::syscall::SyscallResult::Ok(_) = ostd::syscall::sys_send(sid, encoded) {
            let mut resp_buf = self.resp_buf.lock();
            // Masked recv: a wildcard here can consume a queued input key event
            // as the config reply while the shell holds input focus.
            match ostd::syscall::sys_recv(sid, &mut resp_buf[..]) {
                ostd::syscall::SyscallResult::Ok(sender) if sender == sid => {
                    match api::ipc::decode::<ConfigResponse>(&resp_buf[..]) {
                        Ok(ConfigResponse::Value(val)) => {
                            Ok(Box::leak(String::from(val).into_boxed_str()))
                        }
                        Ok(ConfigResponse::NotFound) => Err(ViError::NotFound),
                        _ => Err(ViError::IO),
                    }
                }
                _ => Err(ViError::IO),
            }
        } else {
            Err(ViError::IO)
        }
    }

    fn set(&mut self, key: &str, value: &str) -> ViResult<()> {
        let sid = Self::endpoint().ok_or(ViError::IO)?;

        let mut req_buf = [0u8; IPC_BUF_SIZE];
        let req = ConfigRequest::Set { key, value };
        let encoded = api::ipc::encode(&req, &mut req_buf).map_err(|_| ViError::IO)?;

        ostd::syscall::sys_send(sid, encoded);

        let mut ack = [0u8; 64];
        // Masked recv — see get().
        ostd::syscall::sys_recv(sid, &mut ack);
        Ok(())
    }
}
