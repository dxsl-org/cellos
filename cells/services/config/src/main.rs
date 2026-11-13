#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![forbid(unsafe_code)]

extern crate alloc;
extern crate ostd;

use abi::caller_identity::CallerIdentity;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use ostd::hotswap::ViStateTransfer;
use ostd::prelude::*;
use wire::ipc::{ConfigRequest, ConfigResponse};
use wire::proto::config::ConfigError;

mod rights;

abi::declare_manifest!(block_io = false, network = false, spawn = false);
abi::declare_syscalls![
    Send,
    SendGather,
    Recv,
    TryRecv,
    Log,
    Heartbeat,
    LookupServiceBound,
    StateStash,
    StateRestore,
    StateStashClear,
    HotSwapReady
];

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
        return encode(&ConfigResponse::Err(ConfigError::Malformed as u8), out);
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
            list = store
                .map
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join("\n");
            ConfigResponse::Keys(list.as_str())
        }
    };
    let encoded = wire::ipc::encode(&response, out)
        .ok()
        .map(|slice| slice.len());
    match encoded {
        Some(len) => Some(&out[..len]),
        None => encode(&ConfigResponse::Err(ConfigError::Malformed as u8), out),
    }
}

fn encode<'o>(response: &ConfigResponse<'_>, out: &'o mut [u8]) -> Option<&'o [u8]> {
    wire::ipc::encode(response, out).ok().map(|slice| &*slice)
}

#[cfg(target_os = "none")]
ostd::cell_main!(cell_main);

#[cfg(target_os = "none")]
fn cell_main() {
    use wire::ipc::IPC_BUF_SIZE;

    ostd::io::println("[config] Config Service v0.4 (typed IPC, attested writes)");

    let mut service = ConfigService {
        store: ConfigStore::new(),
        resp_buf: [0u8; IPC_BUF_SIZE],
    };
    ostd::service::serve(ostd::service::ServeConfig::with_heartbeat(), &mut service);
}

#[cfg(target_os = "none")]
struct ConfigService {
    store: ConfigStore,
    resp_buf: [u8; wire::ipc::IPC_BUF_SIZE],
}

#[cfg(target_os = "none")]
impl ostd::service::Handler for ConfigService {
    fn on_request(&mut self, request: ostd::service::Request<'_>) {
        let decoded = wire::ipc::decode::<ConfigRequest>(request.payload()).ok();
        if let Some(reply) = handle(
            &mut self.store,
            decoded,
            Some(request.caller()),
            &mut self.resp_buf,
        ) {
            let _ = request.reply(reply);
        }
    }

    /// The supervisor swaps this incarnation out: hand over the whole store.
    fn on_snapshot(&mut self) -> Option<Vec<u8>> {
        let state = ostd::hotswap::encode_transfer(&self.store).ok();
        ostd::io::println(match state {
            Some(_) => "[config] state handed to the hot-swap replacement",
            None => "[config] snapshot failed: state not handed over",
        });
        state
    }

    /// This incarnation is the replacement: take the previous store whole.
    fn on_restore(&mut self, state: &[u8]) -> bool {
        match ostd::hotswap::decode_transfer::<ConfigStore>(state) {
            Ok(store) => {
                ostd::io::println(&alloc::format!(
                    "[config] state restored from the previous incarnation ({} keys)",
                    store.map.len()
                ));
                self.store = store;
                true
            }
            Err(_) => {
                ostd::io::println("[config] restore refused: unreadable state");
                false
            }
        }
    }
}

// ─── Hot-swap state transfer ──────────────────────────────────────────────────
//
// The schema version travels in `ostd::hotswap`'s header. Payload (little-endian):
//   [count: u32] then per entry [key_len: u16][key bytes][val_len: u16][val bytes]

impl ViStateTransfer for ConfigStore {
    const SCHEMA_VERSION: u32 = 1;

    fn serialize(&self) -> Result<Box<[u8]>, ViError> {
        let count = u32::try_from(self.map.len()).map_err(|_| ViError::InvalidInput)?;
        let mut out = Vec::new();
        out.extend_from_slice(&count.to_le_bytes());
        for field in self.map.iter().flat_map(|(key, value)| [key, value]) {
            let len = u16::try_from(field.len()).map_err(|_| ViError::InvalidInput)?;
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(field.as_bytes());
        }
        Ok(out.into_boxed_slice())
    }

    fn deserialize(version: u32, bytes: &[u8]) -> Result<Self, ViError> {
        if version != Self::SCHEMA_VERSION {
            return Err(ViError::InvalidInput);
        }
        let (count, mut rest) = bytes
            .split_first_chunk::<4>()
            .ok_or(ViError::InvalidInput)?;
        let mut map = BTreeMap::new();
        for _ in 0..u32::from_le_bytes(*count) {
            let key = take_str(&mut rest)?;
            let value = take_str(&mut rest)?;
            map.insert(String::from(key), String::from(value));
        }
        if !rest.is_empty() {
            return Err(ViError::InvalidInput);
        }
        Ok(Self { map })
    }
}

/// One `[len: u16][utf-8 bytes]` field off the front of `rest`.
fn take_str<'b>(rest: &mut &'b [u8]) -> Result<&'b str, ViError> {
    let (len, tail) = rest.split_first_chunk::<2>().ok_or(ViError::InvalidInput)?;
    let (field, tail) = tail
        .split_at_checked(usize::from(u16::from_le_bytes(*len)))
        .ok_or(ViError::InvalidInput)?;
    *rest = tail;
    core::str::from_utf8(field).map_err(|_| ViError::InvalidInput)
}

#[cfg(test)]
mod tests {
    use super::*;
    use abi::caller_identity::{CALLER_FLAG_CONFIG_WRITE, CALLER_FLAG_VFS_MUTATE};
    use wire::ipc::IPC_BUF_SIZE;

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
    fn call(
        store: &mut ConfigStore,
        request: ConfigRequest<'_>,
        caller: Option<&CallerIdentity>,
    ) -> String {
        let mut out = [0u8; IPC_BUF_SIZE];
        let reply = handle(store, Some(request), caller, &mut out).expect("reply encodes");
        alloc::format!(
            "{:?}",
            wire::ipc::decode::<ConfigResponse>(reply).expect("reply decodes")
        )
    }

    fn snapshot(store: &ConfigStore) -> BTreeMap<String, String> {
        store.map.clone()
    }

    #[test]
    fn denied_writes_leave_the_store_unchanged() {
        let mut store = ConfigStore::new();
        let before = snapshot(&store);
        let denied = alloc::format!("{:?}", ConfigResponse::Err(ConfigError::Denied as u8));
        for (request, caller) in [
            (
                ConfigRequest::Set {
                    key: "P01Q_PROBE",
                    value: "x",
                },
                Some(&PROBE),
            ),
            (
                ConfigRequest::Set {
                    key: "P01Q_PROBE",
                    value: "x",
                },
                None,
            ),
            (ConfigRequest::Delete("OS"), Some(&PROBE)),
            (ConfigRequest::Delete("OS"), Some(&SHELL)),
            (
                ConfigRequest::Set {
                    key: "PATH",
                    value: "/tmp",
                },
                Some(&SHELL),
            ),
            (
                ConfigRequest::Set {
                    key: "a.b",
                    value: "1",
                },
                Some(&SHELL),
            ),
        ] {
            assert_eq!(call(&mut store, request, caller), denied);
            assert_eq!(snapshot(&store), before);
        }
    }

    #[test]
    fn granted_writes_mutate_and_reads_stay_open() {
        let mut store = ConfigStore::new();
        assert_eq!(
            call(
                &mut store,
                ConfigRequest::Set {
                    key: "P01Q_OK",
                    value: "1"
                },
                Some(&SHELL)
            ),
            "Ok"
        );
        // Reads need no identity and no right.
        assert_eq!(
            call(&mut store, ConfigRequest::Get("P01Q_OK"), None),
            "Value(\"1\")"
        );
        assert_eq!(
            call(&mut store, ConfigRequest::Get("PATH"), Some(&PROBE)),
            "Value(\"/bin\")"
        );
        assert_eq!(
            call(&mut store, ConfigRequest::List, None),
            "Keys(\"OS\\nP01Q_OK\\nPATH\")"
        );
        assert_eq!(
            call(&mut store, ConfigRequest::Delete("P01Q_OK"), Some(&SHELL)),
            "Ok"
        );
        assert_eq!(
            call(&mut store, ConfigRequest::Get("P01Q_OK"), None),
            "NotFound"
        );
        // Deleting an absent namespace key stays idempotent.
        assert_eq!(
            call(&mut store, ConfigRequest::Delete("P01Q_OK"), Some(&SHELL)),
            "Ok"
        );
    }

    #[test]
    fn malformed_request_is_answered_with_the_generic_error() {
        let mut store = ConfigStore::new();
        let mut out = [0u8; IPC_BUF_SIZE];
        let reply = handle(&mut store, None, Some(&SHELL), &mut out).expect("reply encodes");
        assert!(matches!(
            wire::ipc::decode::<ConfigResponse>(reply),
            Ok(ConfigResponse::Err(code)) if code == ConfigError::Malformed as u8
        ));
    }

    #[test]
    fn the_whole_store_survives_a_hot_swap_transfer() {
        let mut store = ConfigStore::new();
        store
            .map
            .insert(String::from("P13KEY"), String::from("swapped-value"));
        store.map.insert(String::from("EMPTY"), String::new());
        store.map.insert(String::from("ünï"), String::from("cödé"));
        let packet = ostd::hotswap::encode_transfer(&store).expect("encodes");
        // The transfer header: schema version, then the payload length.
        let payload_len = (packet.len() - 8) as u32;
        assert_eq!(&packet[..4], &1u32.to_le_bytes());
        assert_eq!(&packet[4..8], &payload_len.to_le_bytes());
        let restored = ostd::hotswap::decode_transfer::<ConfigStore>(&packet).expect("decodes");
        assert_eq!(snapshot(&restored), snapshot(&store));
    }

    #[test]
    fn a_packet_whose_header_disagrees_is_refused() {
        let packet = ostd::hotswap::encode_transfer(&ConfigStore::new()).expect("encodes");
        let refused = |bytes: &[u8]| ostd::hotswap::decode_transfer::<ConfigStore>(bytes).is_err();
        for len in [0, 7, 8, packet.len() - 1] {
            assert!(
                refused(&packet[..len]),
                "a {len}-byte packet must not restore"
            );
        }
        let mut padded = packet.clone();
        padded.push(0);
        assert!(refused(&padded));
        let mut newer = packet;
        newer[0] = 2;
        assert!(refused(&newer));
    }

    #[test]
    fn a_damaged_transfer_is_refused_whole() {
        let store = ConfigStore::new();
        let payload = store.serialize().expect("serializes");
        assert!(ConfigStore::deserialize(2, &payload).is_err());
        for len in 0..payload.len() {
            assert!(
                ConfigStore::deserialize(1, &payload[..len]).is_err(),
                "a {len}-byte prefix must not restore"
            );
        }
        let mut trailing = payload.to_vec();
        trailing.push(0);
        assert!(ConfigStore::deserialize(1, &trailing).is_err());
        let mut not_utf8 = payload.to_vec();
        not_utf8[6] = 0xFF;
        assert!(ConfigStore::deserialize(1, &not_utf8).is_err());
    }
}
