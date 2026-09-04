use alloc::format;
use core::ffi::{c_char, c_int, c_long};
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
    #[link_name = "_stat"]
    fn stat(name: *const c_char, st: *mut Stat) -> i32;
    #[link_name = "_unlink"]
    fn unlink(name: *const c_char) -> i32;
    #[link_name = "_rename"]
    fn rename(old: *const c_char, new: *const c_char) -> i32;
    #[link_name = "_close"]
    fn close(fd: i32) -> i32;
    #[link_name = "_mkdir"]
    fn mkdir(name: *const c_char, mode: c_int) -> c_int;
    #[link_name = "_rmdir"]
    fn rmdir(name: *const c_char) -> c_int;
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

pub(super) fn test_stat() {
    const PATH: &[u8] = b"/BIN/INIT\0";
    let mut actual = filled_stat(0xA5);
    let ret = unsafe { stat(PATH.as_ptr() as *const c_char, actual.as_mut_ptr()) };
    if ret != 0 {
        println("[posix-shim] POSIX-STAT: FAIL ret != 0");
        return;
    }
    let actual_ref = unsafe { &*actual.as_ptr() };
    if actual_ref.st_mode != 0o100000 || actual_ref.st_size == 0 {
        println(&format!(
            "[posix-shim] POSIX-STAT: FAIL mode={:#o} size={}",
            actual_ref.st_mode, actual_ref.st_size
        ));
        return;
    }

    // Negative tests:
    let mut invalid = filled_stat(0xA5);
    let null_path = unsafe { stat(core::ptr::null(), invalid.as_mut_ptr()) };
    let null_buf = unsafe { stat(PATH.as_ptr() as *const c_char, core::ptr::null_mut()) };
    let non_existent = unsafe {
        stat(
            b"/NONEXISTENT_FILE_12345\0".as_ptr() as *const c_char,
            invalid.as_mut_ptr(),
        )
    };

    if null_path == -1
        && null_buf == -1
        && non_existent == -1
        && stat_bytes(&invalid).iter().all(|byte| *byte == 0xA5)
    {
        println("[posix-shim] POSIX-STAT: OK");
    } else {
        println("[posix-shim] POSIX-STAT: FAIL negative checks");
    }
}

pub(super) fn test_unlink() {
    // Calling unlink on NULL or non-existent file must fail closed (-1)
    let null_ret = unsafe { unlink(core::ptr::null()) };
    if null_ret != -1 {
        println("[posix-shim] POSIX-UNLINK: FAIL null_ret != -1");
        return;
    }

    let non_existent =
        unsafe { unlink(b"/tmp/nonexistent_posix_unlink_12345\0".as_ptr() as *const c_char) };
    if non_existent != -1 {
        println("[posix-shim] POSIX-UNLINK: FAIL non_existent != -1");
        return;
    }

    // Positive create -> stat (exists) -> unlink (success) -> stat (gone) -> unlink (fails)
    const SMOKE_PATH: &str = "/tmp/posix_unlink_smoke.txt";
    const SMOKE_CSTR: &[u8] = b"/tmp/posix_unlink_smoke.txt\0";

    let mut vfs = ostd::clients::VfsClient::new();
    if vfs
        .write_file(SMOKE_PATH, b"posix unlink smoke data\n")
        .is_err()
    {
        println("[posix-shim] POSIX-UNLINK: FAIL create file");
        return;
    }

    // Verify stat confirms file exists
    let mut st = filled_stat(0);
    let stat_before = unsafe { stat(SMOKE_CSTR.as_ptr() as *const c_char, st.as_mut_ptr()) };
    if stat_before != 0 {
        println("[posix-shim] POSIX-UNLINK: FAIL stat before unlink");
        return;
    }

    // Call unlink -> must return 0
    let unlink_ret = unsafe { unlink(SMOKE_CSTR.as_ptr() as *const c_char) };
    if unlink_ret != 0 {
        println(&format!(
            "[posix-shim] POSIX-UNLINK: FAIL unlink ret={unlink_ret}"
        ));
        return;
    }

    // Verify stat confirms file is gone
    let mut st_after = filled_stat(0xA5);
    let stat_after = unsafe { stat(SMOKE_CSTR.as_ptr() as *const c_char, st_after.as_mut_ptr()) };
    if stat_after != -1 {
        println("[posix-shim] POSIX-UNLINK: FAIL file still exists after unlink");
        return;
    }

    // Second unlink on same file must fail closed (-1)
    let unlink_again = unsafe { unlink(SMOKE_CSTR.as_ptr() as *const c_char) };
    if unlink_again != -1 {
        println("[posix-shim] POSIX-UNLINK: FAIL second unlink succeeded");
        return;
    }

    println("[posix-shim] POSIX-UNLINK: OK");
}

pub(super) fn test_mkdir_rmdir() {
    const PATH: &[u8] = b"/srv/posix_mkdir_rmdir_smoke\0";
    const MISSING: &[u8] = b"/srv/posix_mkdir_rmdir_missing\0";
    const INVALID_UTF8: &[u8] = b"/srv/posix_mkdir_rmdir_\xFF\0";

    if unsafe { mkdir(core::ptr::null(), 0o755) } != -1 || unsafe { rmdir(core::ptr::null()) } != -1
    {
        println("[posix-shim] POSIX-MKDIR-RMDIR: FAIL null path succeeded");
        return;
    }
    if unsafe { mkdir(INVALID_UTF8.as_ptr() as *const c_char, 0o755) } != -1
        || unsafe { rmdir(INVALID_UTF8.as_ptr() as *const c_char) } != -1
    {
        println("[posix-shim] POSIX-MKDIR-RMDIR: FAIL invalid UTF-8 succeeded");
        return;
    }
    if unsafe { rmdir(MISSING.as_ptr() as *const c_char) } != -1 {
        println("[posix-shim] POSIX-MKDIR-RMDIR: FAIL missing directory removed");
        return;
    }
    const FILE_PATH: &str = "/srv/posix_mkdir_rmdir_regular_file";
    const FILE_CSTR: &[u8] = b"/srv/posix_mkdir_rmdir_regular_file\0";
    let mut vfs = ostd::clients::VfsClient::new();
    if vfs
        .write_file(FILE_PATH, b"rmdir must not delete this file\n")
        .is_err()
    {
        println("[posix-shim] POSIX-MKDIR-RMDIR: FAIL create regular file");
        return;
    }
    if unsafe { rmdir(FILE_CSTR.as_ptr() as *const c_char) } != -1 {
        println("[posix-shim] POSIX-MKDIR-RMDIR: FAIL rmdir regular file succeeded");
        return;
    }
    let mut regular = filled_stat(0xA5);
    if unsafe { stat(FILE_CSTR.as_ptr() as *const c_char, regular.as_mut_ptr()) } != 0
        || unsafe { (*regular.as_ptr()).st_mode } != 0o100000
    {
        println("[posix-shim] POSIX-MKDIR-RMDIR: FAIL regular file was removed");
        return;
    }
    if unsafe { unlink(FILE_CSTR.as_ptr() as *const c_char) } != 0 {
        println("[posix-shim] POSIX-MKDIR-RMDIR: FAIL cleanup regular file");
        return;
    }
    const NONEMPTY_DIR_CSTR: &[u8] = b"/srv/posix_mkdir_rmdir_nonempty\0";
    const NONEMPTY_CHILD_PATH: &str = "/srv/posix_mkdir_rmdir_nonempty/child";
    const NONEMPTY_CHILD_CSTR: &[u8] = b"/srv/posix_mkdir_rmdir_nonempty/child\0";
    if unsafe { mkdir(NONEMPTY_DIR_CSTR.as_ptr() as *const c_char, 0o755) } != 0 {
        println("[posix-shim] POSIX-MKDIR-RMDIR: FAIL mkdir non-empty directory");
        return;
    }
    if vfs
        .write_file(
            NONEMPTY_CHILD_PATH,
            b"rmdir must not remove non-empty directories\n",
        )
        .is_err()
    {
        println("[posix-shim] POSIX-MKDIR-RMDIR: FAIL create non-empty directory child");
        return;
    }
    if unsafe { mkdir(NONEMPTY_DIR_CSTR.as_ptr() as *const c_char, 0o755) } != -1 {
        println("[posix-shim] POSIX-MKDIR-RMDIR: FAIL duplicate mkdir succeeded");
        return;
    }
    if unsafe { rmdir(NONEMPTY_DIR_CSTR.as_ptr() as *const c_char) } != -1 {
        println("[posix-shim] POSIX-MKDIR-RMDIR: FAIL rmdir non-empty directory succeeded");
        return;
    }
    let mut child = filled_stat(0xA5);
    if unsafe {
        stat(
            NONEMPTY_CHILD_CSTR.as_ptr() as *const c_char,
            child.as_mut_ptr(),
        )
    } != 0
        || unsafe { (*child.as_ptr()).st_mode } != 0o100000
    {
        println("[posix-shim] POSIX-MKDIR-RMDIR: FAIL non-empty directory child was removed");
        return;
    }
    if unsafe { unlink(NONEMPTY_CHILD_CSTR.as_ptr() as *const c_char) } != 0
        || unsafe { rmdir(NONEMPTY_DIR_CSTR.as_ptr() as *const c_char) } != 0
    {
        println("[posix-shim] POSIX-MKDIR-RMDIR: FAIL cleanup non-empty directory");
        return;
    }
    if unsafe { mkdir(PATH.as_ptr() as *const c_char, 0o755) } != 0 {
        println("[posix-shim] POSIX-MKDIR-RMDIR: FAIL mkdir");
        return;
    }
    let mut before = filled_stat(0xA5);
    if unsafe { stat(PATH.as_ptr() as *const c_char, before.as_mut_ptr()) } != 0
        || unsafe { (*before.as_ptr()).st_mode } != 0o040000
    {
        println("[posix-shim] POSIX-MKDIR-RMDIR: FAIL directory stat");
        return;
    }

    if unsafe { rmdir(PATH.as_ptr() as *const c_char) } != 0 {
        println("[posix-shim] POSIX-MKDIR-RMDIR: FAIL rmdir");
        return;
    }
    let mut after = filled_stat(0xA5);
    if unsafe { stat(PATH.as_ptr() as *const c_char, after.as_mut_ptr()) } != -1
        || !stat_bytes(&after).iter().all(|byte| *byte == 0xA5)
    {
        println("[posix-shim] POSIX-MKDIR-RMDIR: FAIL removed directory stat");
        return;
    }
    if unsafe { rmdir(PATH.as_ptr() as *const c_char) } != -1 {
        println("[posix-shim] POSIX-MKDIR-RMDIR: FAIL second rmdir succeeded");
        return;
    }

    println("[posix-shim] POSIX-MKDIR-RMDIR: OK");
}

pub(super) fn test_rename() {
    // Calling rename on NULL pointers must fail closed (-1)
    let null_both = unsafe { rename(core::ptr::null(), core::ptr::null()) };
    if null_both != -1 {
        println("[posix-shim] POSIX-RENAME: FAIL null_both != -1");
        return;
    }

    let non_existent_src = unsafe {
        rename(
            b"/srv/nonexistent_src_12345\0".as_ptr() as *const c_char,
            b"/srv/nonexistent_dst_12345\0".as_ptr() as *const c_char,
        )
    };
    if non_existent_src != -1 {
        println("[posix-shim] POSIX-RENAME: FAIL non_existent_src != -1");
        return;
    }

    // Calling rename on non-/srv path must fail closed (-1) (VFS backend constraint)
    let non_srv = unsafe {
        rename(
            b"/tmp/non_srv_src\0".as_ptr() as *const c_char,
            b"/tmp/non_srv_dst\0".as_ptr() as *const c_char,
        )
    };
    if non_srv != -1 {
        println("[posix-shim] POSIX-RENAME: FAIL non_srv != -1");
        return;
    }

    // Positive create -> stat (exists) -> rename -> stat old (gone) -> stat new (exists) -> unlink (cleanup)
    const RN_SRC_PATH: &str = "/srv/posix_rename_src.txt";
    const RN_SRC_CSTR: &[u8] = b"/srv/posix_rename_src.txt\0";
    const RN_DST_CSTR: &[u8] = b"/srv/posix_rename_dst.txt\0";

    let mut vfs = ostd::clients::VfsClient::new();
    if vfs
        .write_file(RN_SRC_PATH, b"posix rename smoke data\n")
        .is_err()
    {
        println("[posix-shim] POSIX-RENAME: FAIL create src file");
        return;
    }

    // Verify stat confirms src file exists
    let mut st = filled_stat(0);
    let stat_before = unsafe { stat(RN_SRC_CSTR.as_ptr() as *const c_char, st.as_mut_ptr()) };
    if stat_before != 0 {
        println("[posix-shim] POSIX-RENAME: FAIL stat before rename");
        return;
    }

    // Call rename -> must return 0
    let rn_ret = unsafe {
        rename(
            RN_SRC_CSTR.as_ptr() as *const c_char,
            RN_DST_CSTR.as_ptr() as *const c_char,
        )
    };
    if rn_ret != 0 {
        println(&format!(
            "[posix-shim] POSIX-RENAME: FAIL rename ret={rn_ret}"
        ));
        return;
    }

    // Verify stat confirms src file is gone
    let mut st_old = filled_stat(0xAA);
    let stat_old = unsafe { stat(RN_SRC_CSTR.as_ptr() as *const c_char, st_old.as_mut_ptr()) };
    if stat_old != -1 {
        println("[posix-shim] POSIX-RENAME: FAIL src file still exists after rename");
        return;
    }

    // Verify stat confirms dst file exists
    let mut st_new = filled_stat(0x55);
    let stat_new = unsafe { stat(RN_DST_CSTR.as_ptr() as *const c_char, st_new.as_mut_ptr()) };
    if stat_new != 0 {
        println("[posix-shim] POSIX-RENAME: FAIL dst file does not exist after rename");
        return;
    }

    // Cleanup dst file via unlink
    let unlink_ret = unsafe { unlink(RN_DST_CSTR.as_ptr() as *const c_char) };
    if unlink_ret != 0 {
        println("[posix-shim] POSIX-RENAME: FAIL cleanup unlink dst");
        return;
    }

    println("[posix-shim] POSIX-RENAME: OK");
}

/// Exercise the published raw Rename ABI. Its kernel RPC must retain this
/// cell's attested VfsMutate identity; a queue acknowledgement is insufficient.
pub(super) fn test_raw_rename() {
    const SRC_PATH: &str = "/srv/raw_rename_src.txt";
    const SRC_CSTR: &[u8] = b"/srv/raw_rename_src.txt\0";
    const DST_CSTR: &[u8] = b"/srv/raw_rename_dst.txt\0";

    let mut vfs = ostd::clients::VfsClient::new();
    if ostd::syscall::sys_rename(
        "/srv/raw_rename_missing.txt",
        "/srv/raw_rename_missing_dst.txt",
    )
    .is_ok()
    {
        println("[posix-shim] RAW-RENAME: FAIL missing source succeeded");
        return;
    }

    if vfs
        .write_file(SRC_PATH, b"raw rename smoke data\n")
        .is_err()
    {
        println("[posix-shim] RAW-RENAME: FAIL create src file");
        return;
    }
    if ostd::syscall::sys_rename(SRC_PATH, "/srv/raw_rename_dst.txt").is_err() {
        println("[posix-shim] RAW-RENAME: FAIL syscall");
        return;
    }

    let mut old = filled_stat(0xAA);
    if unsafe { stat(SRC_CSTR.as_ptr() as *const c_char, old.as_mut_ptr()) } != -1 {
        println("[posix-shim] RAW-RENAME: FAIL src file still exists");
        return;
    }
    let mut new = filled_stat(0x55);
    if unsafe { stat(DST_CSTR.as_ptr() as *const c_char, new.as_mut_ptr()) } != 0 {
        println("[posix-shim] RAW-RENAME: FAIL dst file missing");
        return;
    }
    if unsafe { unlink(DST_CSTR.as_ptr() as *const c_char) } != 0 {
        println("[posix-shim] RAW-RENAME: FAIL cleanup");
        return;
    }
    println("[posix-shim] RAW-RENAME: OK");
}
