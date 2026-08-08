//! Runtime witness that Snapshot allowlist access does not bypass SupervisorCap.

use ostd::io::println;
use ostd::syscall::{sys_exit, sys_snapshot, SyscallError, SyscallResult};

pub fn run() {
    println("[snapshot-authority-runtime] START");
    match sys_snapshot() {
        SyscallResult::Err(SyscallError::PermissionDenied) => {
            println("[snapshot-authority-runtime] PASS (allowlisted bench caller denied: no SupervisorCap)");
            sys_exit(0);
        }
        SyscallResult::Ok(frames) => {
            println(&alloc::format!(
                "[snapshot-authority-runtime] FAIL (unexpected snapshot success: {frames} frame(s))"
            ));
        }
        SyscallResult::Err(error) => {
            println(&alloc::format!(
                "[snapshot-authority-runtime] FAIL (unexpected snapshot error: {:?})",
                error
            ));
        }
    }
    sys_exit(1);
}
