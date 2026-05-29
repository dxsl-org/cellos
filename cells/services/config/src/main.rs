#![no_std]
#![no_main]

extern crate alloc;
extern crate ostd;

use alloc::collections::BTreeMap;
use api::hotswap::ViStateTransfer;
use ostd::io::println;
use ostd::prelude::*;

// Singleton storage
struct ConfigStore {
    map: BTreeMap<String, String>,
}

impl ConfigStore {
    fn new() -> Self {
        let mut map = BTreeMap::new();
        // Default values
        map.insert(String::from("PATH"), String::from("/bin"));
        map.insert(String::from("OS"), String::from("ViOS"));
        Self { map }
    }
}

struct ConfigService {
    store: Mutex<ConfigStore>,
}


impl ConfigService {
    fn new() -> Self {
        Self {
            store: Mutex::new(ConfigStore::new()),
        }
    }
}

// Implement ViConfig trait
// Note: We implement this on the Service side struct.
// The `get` returns &str, which is tied to the lifetime of `self` (the service).
// This fits the SAS model.
impl ConfigService {
    fn get_value(&self, key: &str) -> Option<(usize, usize)> {
        let store = self.store.lock();
        if let Some(val) = store.map.get(key) {
            Some((val.as_ptr() as usize, val.len()))
        } else {
            None
        }
    }

    fn set_value(&self, key: &str, value: &str) {
        let mut store = self.store.lock();
        store.map.insert(String::from(key), String::from(value));
        // TODO: Notification
    }
}

#[no_mangle]
pub fn main() {
    println("Config Service: Starting...");

    let service = ConfigService::new();

    let mut buf = [0u8; 256];
    loop {
        match ostd::syscall::sys_recv(0, &mut buf) {
            ostd::syscall::SyscallResult::Ok(sender) if sender > 0 => {
                // Protocol:
                // 1: Get (Key) -> Ptr/Len
                // 2: Set (Key, Val) -> OK

                if buf[0] == 1 {
                    // Get
                    let key_len = buf[1] as usize;
                    if let Ok(key) = core::str::from_utf8(&buf[2..2 + key_len]) {
                        if let Some((ptr, len)) = service.get_value(key) {
                            let mut resp = [0u8; 16];
                            resp[0..8].copy_from_slice(&(ptr as u64).to_le_bytes());
                            resp[8..16].copy_from_slice(&(len as u64).to_le_bytes());
                            ostd::syscall::sys_send(sender, &resp);
                        } else {
                            ostd::syscall::sys_send(sender, b"");
                        }
                    }
                } else if buf[0] == 2 {
                    // Set
                    let key_len = buf[1] as usize;
                    let val_len = buf[2] as usize;
                    if let Ok(key) = core::str::from_utf8(&buf[3..3 + key_len]) {
                        if let Ok(val) =
                            core::str::from_utf8(&buf[3 + key_len..3 + key_len + val_len])
                        {
                            service.set_value(key, val);
                            ostd::syscall::sys_send(sender, b"OK");
                        }
                    }
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
        4 + 4 + self.map.iter().map(|(k, v)| 2 + k.len() + 2 + v.len()).sum::<usize>()
    }

    fn serialize_state(&self, buf: &mut [u8]) -> ViResult<usize> {
        let needed = self.state_size();
        if buf.len() < needed { return Err(ViError::InvalidArgument); }
        let mut pos = 0;
        buf[pos..pos+4].copy_from_slice(&SCHEMA_VERSION.to_le_bytes()); pos += 4;
        let count = self.map.len() as u32;
        buf[pos..pos+4].copy_from_slice(&count.to_le_bytes()); pos += 4;
        for (k, v) in &self.map {
            let kl = k.len() as u16;
            let vl = v.len() as u16;
            buf[pos..pos+2].copy_from_slice(&kl.to_le_bytes()); pos += 2;
            buf[pos..pos+k.len()].copy_from_slice(k.as_bytes()); pos += k.len();
            buf[pos..pos+2].copy_from_slice(&vl.to_le_bytes()); pos += 2;
            buf[pos..pos+v.len()].copy_from_slice(v.as_bytes()); pos += v.len();
        }
        Ok(pos)
    }

    fn deserialize_state(&mut self, buf: &[u8]) -> ViResult<()> {
        if buf.len() < 8 { return Err(ViError::InvalidInput); }
        let _version = u32::from_le_bytes([buf[0],buf[1],buf[2],buf[3]]);
        let count = u32::from_le_bytes([buf[4],buf[5],buf[6],buf[7]]) as usize;
        let mut pos = 8usize;
        self.map.clear();
        for _ in 0..count {
            if pos + 2 > buf.len() { return Err(ViError::InvalidInput); }
            let kl = u16::from_le_bytes([buf[pos], buf[pos+1]]) as usize; pos += 2;
            if pos + kl > buf.len() { return Err(ViError::InvalidInput); }
            let key = core::str::from_utf8(&buf[pos..pos+kl]).map_err(|_| ViError::InvalidInput)?;
            pos += kl;
            if pos + 2 > buf.len() { return Err(ViError::InvalidInput); }
            let vl = u16::from_le_bytes([buf[pos], buf[pos+1]]) as usize; pos += 2;
            if pos + vl > buf.len() { return Err(ViError::InvalidInput); }
            let val = core::str::from_utf8(&buf[pos..pos+vl]).map_err(|_| ViError::InvalidInput)?;
            pos += vl;
            self.map.insert(String::from(key), String::from(val));
        }
        Ok(())
    }
}
