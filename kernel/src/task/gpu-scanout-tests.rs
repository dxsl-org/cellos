use super::{cursor, flush, SyscallError};
use crate::cell::service_registry::{clear_tid, register};

/// H-26: only the registered compositor may scan out; any other caller is
/// refused before its pointer (here a kernel-half address) is looked at.
#[test]
fn scanout_is_refused_to_everyone_but_the_compositor() {
    const COMPOSITOR: usize = 0x5_6C01;
    const INTRUDER: usize = 0x5_6C02;
    const KERNEL_PTR: usize = 0xFFFF_8000_0000_0000;
    let geometry = (1 << 16) | 1;
    assert_eq!(
        flush(INTRUDER, KERNEL_PTR, 4, 0, geometry),
        Err(SyscallError::PermissionDenied)
    );
    assert!(register(
        abi::syscall::service::COMPOSITOR,
        COMPOSITOR,
        0x6C,
        1
    ));
    assert_eq!(
        flush(INTRUDER, KERNEL_PTR, 4, 0, geometry),
        Err(SyscallError::PermissionDenied)
    );
    assert_eq!(
        cursor(INTRUDER, 0, KERNEL_PTR, 0, 0),
        Err(SyscallError::PermissionDenied)
    );
    clear_tid(COMPOSITOR);
}
