#![no_std]
#![no_main]

extern crate alloc;
extern crate ostd;

use ostd::prelude::*;
use api::config::ViConfig;
use alloc::collections::BTreeMap;
use ostd::io::println;
use core::cell::RefCell;

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
    store: RefCell<ConfigStore>,
}

unsafe impl Sync for ConfigService {}

impl ConfigService {
    fn new() -> Self {
        Self {
            store: RefCell::new(ConfigStore::new())
        }
    }
}

// Implement ViConfig trait
// Note: We implement this on the Service side struct.
// The `get` returns &str, which is tied to the lifetime of `self` (the service).
// This fits the SAS model.
impl ConfigService {
    fn get_value(&self, key: &str) -> Option<(usize, usize)> {
        let store = self.store.borrow();
        if let Some(val) = store.map.get(key) {
            Some((val.as_ptr() as usize, val.len()))
        } else {
            None
        }
    }

    fn set_value(&self, key: &str, value: &str) {
        let mut store = self.store.borrow_mut();
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

                if buf[0] == 1 { // Get
                    let key_len = buf[1] as usize;
                    if let Ok(key) = core::str::from_utf8(&buf[2..2+key_len]) {
                        if let Some((ptr, len)) = service.get_value(key) {
                            let mut resp = [0u8; 16];
                            unsafe {
                                let ptr_bytes = (ptr as u64).to_le_bytes();
                                let len_bytes = (len as u64).to_le_bytes();
                                resp[0..8].copy_from_slice(&ptr_bytes);
                                resp[8..16].copy_from_slice(&len_bytes);
                            }
                            ostd::syscall::sys_send(sender, &resp);
                        } else {
                            ostd::syscall::sys_send(sender, b"");
                        }
                    }
                } else if buf[0] == 2 { // Set
                    let key_len = buf[1] as usize;
                    let val_len = buf[2] as usize;
                    if let Ok(key) = core::str::from_utf8(&buf[3..3+key_len]) {
                        if let Ok(val) = core::str::from_utf8(&buf[3+key_len..3+key_len+val_len]) {
                             service.set_value(key, val);
                             ostd::syscall::sys_send(sender, b"OK");
                        }
                    }
                }
            },
            _ => {
                ostd::task::yield_now();
            }
        }
    }
}
