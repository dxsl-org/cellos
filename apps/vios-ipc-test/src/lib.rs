#![no_std]
extern crate alloc;
use alloc::vec::Vec;
use ostd::prelude::*;

#[no_mangle]
pub fn app_main() -> ! {
    ostd::println!("=== ViOS IPC Phase 19: Grant Table Test ===");
    
    // 1. Spawn Receiver
    ostd::println!("Main: Spawning Receiver...");
    ostd::println!("Main: Entry Address = 0x{:X}", receiver_entry as usize);
    let receiver_id = match ostd::task::spawn(receiver_entry) {
        ostd::syscall::SyscallResult::Ok(id) => id,
        _ => {
            ostd::println!("Main: Spawn Failed!");
            loop { ostd::syscall::sys_yield(); }
        }
    };
    ostd::println!("Main: Receiver ID = {}", receiver_id);
    
    // 2. Alloc Data
    let mut data = alloc::vec![0u8; 128];
    for i in 0..128 { data[i] = (i % 255) as u8; }
    let ptr = data.as_ptr() as usize;
    let len = data.len();
    
    ostd::println!("Main: Data allocated at 0x{:X}", ptr);
    
    // 3. Grant Access
    // Flags: 3 = R/W
    match ostd::syscall::sys_grant(receiver_id, ptr, len, 3) {
        ostd::syscall::SyscallResult::Ok(gid) => {
            ostd::println!("Main: Grant Success! ID = {}", gid);
            
            // 4. Send Message (Grant Transfer)
            // Simplified: Send just the GID in byte 0.
            let msg = [gid as u8, 0, 0, 0];
            ostd::syscall::sys_send(receiver_id, &msg);
            
            // 5. Wait for signal that Receiver read it
            let mut buf = [0u8; 16];
            ostd::syscall::sys_recv(0, &mut buf);
            
            // 6. Verify Modification (Receiver should write 'A' at [0])
            // Since we are in SAS (Single Address Space), modification should be visible immediately
            // IF the Grant mapped to same physical memory (it does).
            if data[0] == b'A' {
               ostd::println!("Main: Peer modification verified! (Byte 0 = 'A')"); 
               ostd::println!("TEST PASSED: Zero-Copy IPC Works!");
            } else {
               ostd::println!("Main: Peer modification NOT found. Value: {}", data[0]);
               ostd::println!("TEST FAILED!");
            }
        },
        ostd::syscall::SyscallResult::Err(e) => {
            ostd::println!("Main: Grant Failed: {:?}", e);
        }
    }

    loop { ostd::syscall::sys_yield(); }
}

fn receiver_entry() {
    let mut buf = [0u8; 16];
    ostd::println!("Receiver: Waiting for Grant...");
    match ostd::syscall::sys_recv(0, &mut buf) {
        ostd::syscall::SyscallResult::Ok(sender) => {
             let gid = buf[0] as usize;
             ostd::println!("Receiver: Msg From {}. GrantID={}", sender, gid);
             
             // Map Grant
             match ostd::syscall::sys_map(gid) {
                 ostd::syscall::SyscallResult::Ok(ptr) => {
                     ostd::println!("Receiver: Mapped at 0x{:X}", ptr);
                     let slice = unsafe { core::slice::from_raw_parts_mut(ptr as *mut u8, 128) };
                     
                     // Read Check
                     ostd::println!("Receiver: Read Byte 1: {}", slice[1]);
                     
                     // Modify Check (Write 'A')
                     slice[0] = b'A';
                     ostd::println!("Receiver: Wrote 'A' to Byte 0.");
                 },
                 _ => ostd::println!("Receiver: Map Failed!"),
             }
             
             ostd::syscall::sys_reply(sender, 0);
        },
        _ => {}
    }
    loop { ostd::syscall::sys_yield(); }
}
