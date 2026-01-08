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

// Implement ViConfig trait (conceptual, but here we handle IPC loop)
impl ViConfig for ConfigService {
    fn get(&self, key: &str) -> ViResult<(usize, usize)> {
        let store = self.store.borrow();
        if let Some(val) = store.map.get(key) {
            // Return address and length of the string in THIS cell's memory.
            // SAS Assumption: Other cells can read this address if we grant it.
            // Or if protection is lax.
            Ok((val.as_ptr() as usize, val.len()))
        } else {
            Err(ViError::NotFound)
        }
    }

    fn set(&self, key: &str, value: &str) -> ViResult<()> {
        let mut store = self.store.borrow_mut();
        store.map.insert(String::from(key), String::from(value));
        // Todo: Notify subscribers
        Ok(())
    }

    fn subscribe(&self, _key: &str, _subscriber_cell_id: usize) -> ViResult<()> {
        // Todo: Implement subscription logic
        Ok(())
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
                // Handle Message
                if buf[0] == 1 { // Get
                    let key_len = buf[1] as usize;
                    if let Ok(key) = core::str::from_utf8(&buf[2..2+key_len]) {
                        if let Ok((ptr, len)) = service.get(key) {
                            // Reply with Pointer and Length (8 bytes each, little endian)
                            // We construct a response buffer: [Ptr(8) | Len(8)]
                            let mut resp = [0u8; 16];
                            unsafe {
                                let ptr_bytes = (ptr as u64).to_le_bytes();
                                let len_bytes = (len as u64).to_le_bytes();
                                resp[0..8].copy_from_slice(&ptr_bytes);
                                resp[8..16].copy_from_slice(&len_bytes);
                            }

                            // Send reply.
                            // In strict SAS with MPU, we should call sys_grant(sender, ptr, len, READ) here.
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
                             let _ = service.set(key, val);
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
