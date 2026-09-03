use alloc::format;
use core::ffi::{c_char, c_long};
use ostd::io::println;

#[repr(C)]
struct Stat {
    st_dev: i32,
    st_ino: i32,
    st_mode: i32,
    st_nlink: i32,
    st_uid: i32,
    st_gid: i32,
    st_rdev: i32,
    st_size: c_long,
    st_atime: c_long,
    st_mtime: c_long,
    st_ctime: c_long,
    st_blksize: c_long,
    st_blocks: c_long,
}

extern "C" {
    #[link_name = "_open"]
    fn open(name: *const c_char, flags: i32, mode: i32) -> i32;
    #[link_name = "_fstat"]
    fn fstat(fd: i32, st: *mut Stat) -> i32;
    #[link_name = "_close"]
    fn close(fd: i32) -> i32;
}

fn filled_stat(byte: u8) -> core::mem::MaybeUninit<Stat> {
    let mut value = core::mem::MaybeUninit::<Stat>::uninit();
    unsafe {
        core::ptr::write_bytes(
            value.as_mut_ptr() as *mut u8,
            byte,
            core::mem::size_of::<Stat>(),
        );
    }
    value
}

fn stat_bytes(value: &core::mem::MaybeUninit<Stat>) -> &[u8] {
    unsafe {
        core::slice::from_raw_parts(value.as_ptr() as *const u8, core::mem::size_of::<Stat>())
    }
}

fn validate_open_fstat(fd: i32) -> bool {
    let mut wire = api::syscall::ViFstatV1::default();
    if ostd::syscall::sys_fstat(fd as usize, &mut wire) != Ok(api::syscall::VI_FSTAT_V1_LEN)
        || wire.kind != api::syscall::VI_FSTAT_KIND_REGULAR
        || wire.access != api::syscall::VI_FSTAT_ACCESS_READ
        || wire.reserved != [0; 2]
        || wire.size == 0
    {
        return false;
    }
    let Ok(expected_size) = c_long::try_from(wire.size) else {
        return false;
    };

    let mut actual = filled_stat(0xA5);
    if unsafe { fstat(fd, actual.as_mut_ptr()) } != 0 {
        return false;
    }
    let actual_ref = unsafe { &*actual.as_ptr() };
    if actual_ref.st_mode != 0o100000 || actual_ref.st_size != expected_size {
        return false;
    }

    let mut expected = filled_stat(0);
    unsafe {
        core::ptr::addr_of_mut!((*expected.as_mut_ptr()).st_mode).write(0o100000);
        core::ptr::addr_of_mut!((*expected.as_mut_ptr()).st_size).write(expected_size);
    }
    if stat_bytes(&actual) != stat_bytes(&expected) {
        return false;
    }

    let mut invalid = filled_stat(0xA5);
    let invalid_ret = unsafe { fstat(-1, invalid.as_mut_ptr()) };
    invalid_ret == -1
        && stat_bytes(&invalid).iter().all(|byte| *byte == 0xA5)
        && unsafe { fstat(fd, core::ptr::null_mut()) } == -1
}

pub(super) fn test_fstat() {
    const PATH: &[u8] = b"/BIN/INIT\0";
    let fd = unsafe { open(PATH.as_ptr() as *const c_char, 0, 0) };
    if fd < 0 {
        println("[posix-shim] POSIX-FSTAT-OPEN: FAIL");
        return;
    }
    println("[posix-shim] POSIX-FSTAT-OPEN: OK");

    let valid = validate_open_fstat(fd);
    let close_ret = unsafe { close(fd) };
    if valid && close_ret == 0 {
        println("[posix-shim] POSIX-FSTAT: OK");
    } else {
        println(&format!(
            "[posix-shim] POSIX-FSTAT: FAIL valid={valid} close={close_ret}"
        ));
    }
}
