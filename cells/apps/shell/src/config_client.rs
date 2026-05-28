use api::config::ViConfig;
use ostd::prelude::*;

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

        if let ostd::syscall::SyscallResult::Ok(_) = ostd::syscall::sys_send(self.service_id, &msg)
        {
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

                    // SAFETY: In the SAS model all cells share one address space, so a
                    // pointer returned by the config service is directly readable here.
                    // The config service stores strings in its static BTreeMap and never
                    // frees them, so the memory is valid for the lifetime of the OS session.
                    // We construct a &str pointing directly into the service's allocation
                    // and cast it to `&'self str` so it satisfies the ViConfig trait bound.
                    // TODO: redesign ViConfig::get to return ViResult<String> (owned) to
                    // eliminate this unsafe block — tracking issue: Law-1 API change.
                    unsafe {
                        let slice = core::slice::from_raw_parts(ptr as *const u8, len);
                        let s = core::str::from_utf8(slice).map_err(|_| ViError::InvalidInput)?;
                        // Extend lifetime from the slice's implicit 'static to &'self str.
                        // Soundness relies on the SAS invariant stated above.
                        let extended: &str = &*(s as *const str);
                        Ok(extended)
                    }
                }
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
