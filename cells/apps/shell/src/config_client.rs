use ostd::prelude::*;
use api::config::ViConfig;

pub struct ConfigClient {
    service_id: usize,
}

impl ConfigClient {
    pub fn new(service_id: usize) -> Self {
        Self { service_id }
    }
}

impl ViConfig for ConfigClient {
    fn get(&self, key: &str) -> ViResult<&str> {
        // Send IPC to Config Service
        let mut msg = Vec::new();
        msg.push(1); // Get
        msg.push(key.len() as u8);
        msg.extend_from_slice(key.as_bytes());

        if let ostd::syscall::SyscallResult::Ok(_) = ostd::syscall::sys_send(self.service_id, &msg) {
            let mut resp = [0u8; 16];
            // Wait for reply (OpCode is implicit)
            // Note: In real system, use recv specific to this transid
            match ostd::syscall::sys_recv(0, &mut resp) {
                ostd::syscall::SyscallResult::Ok(sender) if sender == self.service_id => {
                    let ptr = u64::from_le_bytes(resp[0..8].try_into().unwrap()) as usize;
                    let len = u64::from_le_bytes(resp[8..16].try_into().unwrap()) as usize;

                    if ptr == 0 {
                        return Err(ViError::NotFound);
                    }

                    // Zero-Copy Magic (SAS)
                    // We cast the pointer (from Service space) to &str (in our space)
                    // SAFETY: Assuming SAS allows read access.
                    unsafe {
                        let slice = core::slice::from_raw_parts(ptr as *const u8, len);
                        let s = core::str::from_utf8(slice).map_err(|_| ViError::InvalidInput)?;
                        // Leak the lifetime? Or force it to match &self?
                        // The Trait definition says `&str` returned has lifetime of `&self`.
                        // But `slice` is temporary.
                        // We must extend lifetime to 'self if we assume Service data is stable.
                        // Or we transmute.
                        // But wait, the Trait says `fn get(&self) -> &str`.
                        // So we just need to return a reference that lives as long as Client.
                        // Since `slice` is raw pointer based, it effectively has 'static lifetime potential if we say so.
                        Ok(core::mem::transmute(s))
                    }
                },
                _ => Err(ViError::IO),
            }
        } else {
            Err(ViError::IO)
        }
    }

    fn set(&mut self, key: &str, value: &str) -> ViResult<()> {
        let mut msg = Vec::new();
        msg.push(2); // Set
        msg.push(key.len() as u8);
        msg.push(value.len() as u8); // Simplification: 1 byte len for key/val
        msg.extend_from_slice(key.as_bytes());
        msg.extend_from_slice(value.as_bytes());

        ostd::syscall::sys_send(self.service_id, &msg);

        let mut buf = [0u8; 16];
        ostd::syscall::sys_recv(0, &mut buf); // Wait for OK
        Ok(())
    }
}
