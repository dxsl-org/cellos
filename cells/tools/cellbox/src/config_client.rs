use alloc::boxed::Box;
use alloc::string::String;
use api::config::ViConfig;
use api::caller_identity::CALLER_IDENTITY_LEN;
use api::ipc::{ConfigRequest, ConfigResponse, CONFIG_ERR_DENIED, IPC_BUF_SIZE};
use ostd::prelude::*;
use ostd::sync::Mutex;

/// Client for the Config service.
///
/// Uses typed postcard IPC (`ConfigRequest` / `ConfigResponse`) matching the
/// config service v0.4 protocol (attested writes: a `Set` outside the caller's
/// Q right is answered `Err(CONFIG_ERR_DENIED)`).  Resolves the live Config
/// endpoint via the Service Registry on each call, so it transparently
/// reconnects when the supervisor respawns Config.
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
        let encoded = encode_set(key, value, &mut req_buf)?;

        if !matches!(
            ostd::syscall::sys_send(sid, encoded),
            ostd::syscall::SyscallResult::Ok(_)
        ) {
            return Err(ViError::IO);
        }

        let mut ack = [0u8; 64];
        let raw = ostd::ipc::recv_from(sid, &mut ack).map_err(|_| ViError::IO)?;
        set_reply(raw)
    }
}

/// Encode a `Set`. service-config receives with an attested caller trailer in
/// the last `CALLER_IDENTITY_LEN` bytes of its `IPC_BUF_SIZE` buffer, so a
/// request reaching into that tail would be overwritten: refuse it here.
fn encode_set<'b>(key: &str, value: &str, buf: &'b mut [u8; IPC_BUF_SIZE]) -> ViResult<&'b [u8]> {
    let req = ConfigRequest::Set { key, value };
    let encoded = api::ipc::encode(&req, &mut buf[..]).map_err(|_| ViError::InvalidArgument)?;
    if encoded.len() > IPC_BUF_SIZE - CALLER_IDENTITY_LEN {
        return Err(ViError::InvalidArgument);
    }
    Ok(encoded)
}

/// Map service-config's reply to a `Set`. Only the service decides admission;
/// `CONFIG_ERR_DENIED` is a refusal, anything else unexpected is unavailability.
fn set_reply(raw: &[u8]) -> ViResult<()> {
    match api::ipc::decode::<ConfigResponse>(raw) {
        Ok(ConfigResponse::Ok) => Ok(()),
        Ok(ConfigResponse::Err(CONFIG_ERR_DENIED)) => Err(ViError::PermissionDenied),
        _ => Err(ViError::IO),
    }
}

/// `$?` and the stderr diagnostic for `export KEY=VALUE` given the Config
/// result (Phase 01 CP1 §4.3).
pub fn export_outcome(key: &str, result: ViResult<()>) -> (i32, Option<String>) {
    match result {
        Ok(()) => (0, None),
        Err(ViError::PermissionDenied) => (1, Some(alloc::format!("export: {key}: permission denied"))),
        Err(_) => (1, Some(alloc::format!("export: {key}: config unavailable"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(response: ConfigResponse<'_>) -> alloc::vec::Vec<u8> {
        let mut out = [0u8; 64];
        api::ipc::encode(&response, &mut out).expect("encodes").to_vec()
    }

    #[test]
    fn set_reply_distinguishes_refusal_from_unavailability() {
        assert_eq!(set_reply(&reply(ConfigResponse::Ok)), Ok(()));
        assert_eq!(
            set_reply(&reply(ConfigResponse::Err(CONFIG_ERR_DENIED))),
            Err(ViError::PermissionDenied)
        );
        for other in [ConfigResponse::Err(0xFF), ConfigResponse::NotFound, ConfigResponse::Value("x")] {
            assert_eq!(set_reply(&reply(other)), Err(ViError::IO));
        }
        assert_eq!(set_reply(&[]), Err(ViError::IO));
    }

    #[test]
    fn export_outcome_reports_status_and_message() {
        assert_eq!(export_outcome("P01Q_OK", Ok(())), (0, None));
        assert_eq!(
            export_outcome("PATH", Err(ViError::PermissionDenied)),
            (1, Some(String::from("export: PATH: permission denied")))
        );
        assert_eq!(
            export_outcome("K", Err(ViError::IO)),
            (1, Some(String::from("export: K: config unavailable")))
        );
    }

    #[test]
    fn set_never_reaches_the_attested_tail() {
        let mut buf = [0u8; IPC_BUF_SIZE];
        assert!(encode_set("K", "v", &mut buf).is_ok());
        let long = "v".repeat(IPC_BUF_SIZE - CALLER_IDENTITY_LEN);
        assert_eq!(encode_set("K", &long, &mut buf), Err(ViError::InvalidArgument));
    }
}
