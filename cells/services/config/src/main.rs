#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![forbid(unsafe_code)]

extern crate alloc;
extern crate ostd;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use api::caller_identity::CallerIdentity;
use api::hotswap::ViStateTransfer;
use api::ipc::{ConfigRequest, ConfigResponse};
use ostd::prelude::*;

mod rights;

api::declare_manifest!(block_io = false, network = false, spawn = false);
api::declare_syscalls![
    Send,
    Recv,
    TryRecv,
    Log,
    Heartbeat,
    LookupService,
    StateStash,
    StateRestore
];

/// `ConfigResponse::Err` code for a malformed request or a reply too large to
/// encode.
const ERR_MALFORMED: u8 = 0xFF;

// Singleton storage
struct ConfigStore {
    map: BTreeMap<String, String>,
}

impl ConfigStore {
    fn new() -> Self {
        let mut map = BTreeMap::new();
        map.insert(String::from("PATH"), String::from("/bin"));
        map.insert(String::from("OS"), String::from("Cellos"));
        Self { map }
    }
}

/// Answer one request into `out` and return the encoded reply.
///
/// `request` is `None` when the payload did not decode. `caller` is the
/// kernel-attested identity (`None` = unknown). Admission (`rights::admit`)
/// runs before the store is read or written, so a refused `Set`/`Delete`
/// leaves the store untouched.
fn handle<'o>(
    store: &mut ConfigStore,
    request: Option<ConfigRequest<'_>>,
    caller: Option<&CallerIdentity>,
    out: &'o mut [u8],
) -> Option<&'o [u8]> {
    let Some(request) = request else {
        return encode(&ConfigResponse::Err(ERR_MALFORMED), out);
    };
    if let Err(code) = rights::admit(&request, caller) {
        return encode(&ConfigResponse::Err(code), out);
    }
    let list;
    let response = match request {
        ConfigRequest::Get(key) => match store.map.get(key) {
            Some(value) => ConfigResponse::Value(value.as_str()),
            None => ConfigResponse::NotFound,
        },
        ConfigRequest::Set { key, value } => {
            store.map.insert(String::from(key), String::from(value));
            ConfigResponse::Ok
        }
        ConfigRequest::Delete(key) => {
            store.map.remove(key);
            ConfigResponse::Ok
        }
        ConfigRequest::List => {
            // Newline-separated key list, sent inline.
            list = store.map.keys().map(String::as_str).collect::<Vec<_>>().join("\n");
            ConfigResponse::Keys(list.as_str())
        }
    };
    let encoded = api::ipc::encode(&response, out).ok().map(|slice| slice.len());
    match encoded {
        Some(len) => Some(&out[..len]),
        None => encode(&ConfigResponse::Err(ERR_MALFORMED), out),
    }
}

fn encode<'o>(response: &ConfigResponse<'_>, out: &'o mut [u8]) -> Option<&'o [u8]> {
    api::ipc::encode(response, out).ok().map(|slice| &*slice)
}

#[cfg(target_os = "none")]
ostd::cell_main!(cell_main);

#[cfg(target_os = "none")]
fn cell_main() {
    use api::ipc::IPC_BUF_SIZE;

    ostd::io::println("[config] Config Service v0.4 (typed IPC, attested writes)");

    let mut store = ConfigStore::new();
    let mut buf = [0u8; IPC_BUF_SIZE];
    let mut resp_buf = [0u8; IPC_BUF_SIZE];

    loop {
        // Attested receive: the kernel writes the sender's identity into the
        // last `CALLER_IDENTITY_LEN` bytes of `buf` (Phase 01 CP1, Q right).
        match ostd::syscall::sys_recv_attested(0, &mut buf) {
            ostd::syscall::SyscallResult::Ok(sender) if sender > 0 => {
                let caller = CallerIdentity::from_recv_buf(&buf);
                let request = api::ipc::decode::<ConfigRequest>(&buf).ok();
                if let Some(reply) = handle(&mut store, request, caller.as_ref(), &mut resp_buf) {
                    ostd::syscall::sys_send(sender, reply);
                }
            }
            _ => {
                ostd::task::yield_now();
            }
        }
    }
}

// ─── Hot-swap state transfer ──────────────────────────────────────────────────
//
// Wire format (little-endian):
//   [count: u32][key_len: u16][key bytes][val_len: u16][val bytes]...
//
// Schema version 1 is prepended as a u32 for forward compatibility.

const SCHEMA_VERSION: u32 = 1;

impl ViStateTransfer for ConfigStore {
    fn state_size(&self) -> usize {
        // version(4) + count(4) + per-entry overhead(4) + key+val bytes
        4 + 4
            + self
                .map
                .iter()
                .map(|(k, v)| 2 + k.len() + 2 + v.len())
                .sum::<usize>()
    }

    fn serialize_state(&self, buf: &mut [u8]) -> ViResult<usize> {
        let needed = self.state_size();
        if buf.len() < needed {
            return Err(ViError::InvalidArgument);
        }
        let mut pos = 0;
        buf[pos..pos + 4].copy_from_slice(&SCHEMA_VERSION.to_le_bytes());
        pos += 4;
        let count = self.map.len() as u32;
        buf[pos..pos + 4].copy_from_slice(&count.to_le_bytes());
        pos += 4;
        for (k, v) in &self.map {
            let kl = k.len() as u16;
            let vl = v.len() as u16;
            buf[pos..pos + 2].copy_from_slice(&kl.to_le_bytes());
            pos += 2;
            buf[pos..pos + k.len()].copy_from_slice(k.as_bytes());
            pos += k.len();
            buf[pos..pos + 2].copy_from_slice(&vl.to_le_bytes());
            pos += 2;
            buf[pos..pos + v.len()].copy_from_slice(v.as_bytes());
            pos += v.len();
        }
        Ok(pos)
    }

    fn deserialize_state(&mut self, buf: &[u8]) -> ViResult<()> {
        if buf.len() < 8 {
            return Err(ViError::InvalidInput);
        }
        let _version = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let count = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;
        let mut pos = 8usize;
        self.map.clear();
        for _ in 0..count {
            if pos + 2 > buf.len() {
                return Err(ViError::InvalidInput);
            }
            let kl = u16::from_le_bytes([buf[pos], buf[pos + 1]]) as usize;
            pos += 2;
            if pos + kl > buf.len() {
                return Err(ViError::InvalidInput);
            }
            let key =
                core::str::from_utf8(&buf[pos..pos + kl]).map_err(|_| ViError::InvalidInput)?;
            pos += kl;
            if pos + 2 > buf.len() {
                return Err(ViError::InvalidInput);
            }
            let vl = u16::from_le_bytes([buf[pos], buf[pos + 1]]) as usize;
            pos += 2;
            if pos + vl > buf.len() {
                return Err(ViError::InvalidInput);
            }
            let val =
                core::str::from_utf8(&buf[pos..pos + vl]).map_err(|_| ViError::InvalidInput)?;
            pos += vl;
            self.map.insert(String::from(key), String::from(val));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use api::caller_identity::{CALLER_FLAG_CONFIG_WRITE, CALLER_FLAG_VFS_MUTATE};
    use api::ipc::{CONFIG_ERR_DENIED, IPC_BUF_SIZE};

    const SHELL: CallerIdentity = CallerIdentity {
        flags: CALLER_FLAG_CONFIG_WRITE | CALLER_FLAG_VFS_MUTATE,
        cell_id: 5,
        generation: 1,
        sender_tid: 5,
    };
    const PROBE: CallerIdentity = CallerIdentity {
        flags: CALLER_FLAG_VFS_MUTATE,
        cell_id: 6,
        generation: 1,
        sender_tid: 6,
    };

    /// Run `request` through the real handler and return the decoded reply.
    fn call(store: &mut ConfigStore, request: ConfigRequest<'_>, caller: Option<&CallerIdentity>) -> String {
        let mut out = [0u8; IPC_BUF_SIZE];
        let reply = handle(store, Some(request), caller, &mut out).expect("reply encodes");
        alloc::format!("{:?}", api::ipc::decode::<ConfigResponse>(reply).expect("reply decodes"))
    }

    fn snapshot(store: &ConfigStore) -> BTreeMap<String, String> {
        store.map.clone()
    }

    #[test]
    fn denied_writes_leave_the_store_unchanged() {
        let mut store = ConfigStore::new();
        let before = snapshot(&store);
        let denied = alloc::format!("{:?}", ConfigResponse::Err(CONFIG_ERR_DENIED));
        for (request, caller) in [
            (ConfigRequest::Set { key: "P01Q_PROBE", value: "x" }, Some(&PROBE)),
            (ConfigRequest::Set { key: "P01Q_PROBE", value: "x" }, None),
            (ConfigRequest::Delete("OS"), Some(&PROBE)),
            (ConfigRequest::Delete("OS"), Some(&SHELL)),
            (ConfigRequest::Set { key: "PATH", value: "/tmp" }, Some(&SHELL)),
            (ConfigRequest::Set { key: "a.b", value: "1" }, Some(&SHELL)),
        ] {
            assert_eq!(call(&mut store, request, caller), denied);
            assert_eq!(snapshot(&store), before);
        }
    }

    #[test]
    fn granted_writes_mutate_and_reads_stay_open() {
        let mut store = ConfigStore::new();
        assert_eq!(
            call(&mut store, ConfigRequest::Set { key: "P01Q_OK", value: "1" }, Some(&SHELL)),
            "Ok"
        );
        // Reads need no identity and no right.
        assert_eq!(call(&mut store, ConfigRequest::Get("P01Q_OK"), None), "Value(\"1\")");
        assert_eq!(call(&mut store, ConfigRequest::Get("PATH"), Some(&PROBE)), "Value(\"/bin\")");
        assert_eq!(call(&mut store, ConfigRequest::List, None), "Keys(\"OS\\nP01Q_OK\\nPATH\")");
        assert_eq!(call(&mut store, ConfigRequest::Delete("P01Q_OK"), Some(&SHELL)), "Ok");
        assert_eq!(call(&mut store, ConfigRequest::Get("P01Q_OK"), None), "NotFound");
        // Deleting an absent namespace key stays idempotent.
        assert_eq!(call(&mut store, ConfigRequest::Delete("P01Q_OK"), Some(&SHELL)), "Ok");
    }

    #[test]
    fn malformed_request_is_answered_with_the_generic_error() {
        let mut store = ConfigStore::new();
        let mut out = [0u8; IPC_BUF_SIZE];
        let reply = handle(&mut store, None, Some(&SHELL), &mut out).expect("reply encodes");
        assert!(matches!(
            api::ipc::decode::<ConfigResponse>(reply),
            Ok(ConfigResponse::Err(ERR_MALFORMED))
        ));
    }
}
