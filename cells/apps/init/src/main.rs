#![no_std]
#![no_main]

extern crate ostd;

use ostd::io::{print, println};
use ostd::string::ToString;

// Embed Service Binaries
// We need to launch these first.
static VFS_ELF: &[u8] = include_bytes!("../../../../kernel/src/embedded/vfs");
static CONFIG_ELF: &[u8] = include_bytes!("../../../../kernel/src/embedded/config");

#[no_mangle]
pub extern "C" fn main() {
    println("Init: Starting ViOS Orchestrator...");
    
    // 1. Spawn Config Service
    println("Init: Spawning Config Service...");
    if let ostd::syscall::SyscallResult::Ok(_) = ostd::syscall::sys_spawn_from_mem(CONFIG_ELF, "config") {
        println("Init: Config Service spawned.");
    } else {
        println("Init: Failed to spawn Config Service.");
    }

    // 2. Spawn VFS Service
    println("Init: Spawning VFS Service...");
    if let ostd::syscall::SyscallResult::Ok(_) = ostd::syscall::sys_spawn_from_mem(VFS_ELF, "vfs") {
        println("Init: VFS Service spawned.");
    } else {
        println("Init: Failed to spawn VFS Service.");
    }

    // Yield to let services start
    ostd::task::yield_now();
    ostd::task::yield_now();

    // 3. Load Shell
    // Now we can try to get Shell from VFS via IPC?
    // Wait, IPC from `init` to `vfs`?
    // Current `service-vfs` IPC protocol (OpCode 1) returns Ptr/Len via Reply.
    // We assume `vfs` is running.
    // But `vfs` might not be Cell ID 2 if we spawned config first.
    // Config=Cell 2, VFS=Cell 3? (Init=1).
    // Let's assume VFS is Cell 3.

    let vfs_cell_id = 3;

    println("Init: Requesting Shell from VFS...");

    // Send "Open /bin/shell" to VFS
    // Msg: [1 (Open), len, path...]
    let path = "/bin/shell";
    let mut msg = ostd::vec::Vec::new();
    msg.push(1); // OpCode Open
    msg.push(path.len() as u8);
    msg.extend_from_slice(path.as_bytes());

    if let ostd::syscall::SyscallResult::Ok(_) = ostd::syscall::sys_send(vfs_cell_id, &msg) {
        // Wait for reply
        let mut resp = [0u8; 16];
        match ostd::syscall::sys_recv(0, &mut resp) {
            ostd::syscall::SyscallResult::Ok(sender) if sender == vfs_cell_id => {
                // Parse Ptr/Len
                let ptr = u64::from_le_bytes(resp[0..8].try_into().unwrap()) as usize;
                let len = u64::from_le_bytes(resp[8..16].try_into().unwrap()) as usize;

                if ptr != 0 && len > 0 {
                    println("Init: Got shell binary from VFS.");

                    // Spawn Shell
                    // We need a slice from that pointer.
                    // SAFETY: In current SAS model, we assume we can read it.
                    let shell_data = unsafe { core::slice::from_raw_parts(ptr as *const u8, len) };

                    if let ostd::syscall::SyscallResult::Ok(_) = ostd::syscall::sys_spawn_from_mem(shell_data, "shell") {
                        println("Init: Shell spawned successfully.");
                    } else {
                        println("Init: Failed to spawn shell.");
                    }
                } else {
                    println("Init: VFS returned empty/null for shell.");
                }
            },
            _ => println("Init: VFS did not reply."),
        }
    } else {
        println("Init: Failed to send to VFS.");
    }

    // Keep init alive
    loop {
        ostd::task::yield_now();
    }
}
