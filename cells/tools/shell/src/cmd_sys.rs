//! System-information shell built-ins: pwd, uname, date, free, env.

use ostd::prelude::*;
use ostd::syscall;

/// `pwd` — print the current working directory.
///
/// ViCell v1.0 has no per-cell CWD tracking; always prints `/` until
/// Phase 17a adds a proper chdir/getcwd implementation.
pub fn cmd_pwd(_args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    crate::executor::shell_println("/");
    Ok(())
}

/// `uname [-a]` — print system identification.
pub fn cmd_uname(mut args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let all = args.any(|a| a == "-a");
    if all {
        crate::executor::shell_println(&alloc::format!(
            "{} {} {} {}",
            ostd::system_info::OS_NAME,
            ostd::system_info::KERNEL_NAME,
            ostd::system_info::KERNEL_VERSION,
            ostd::system_info::ARCH,
        ));
    } else {
        crate::executor::shell_println(ostd::system_info::OS_NAME);
    }
    Ok(())
}

fn frames_to_kib(frames: u64, page_size: u64) -> Option<u64> {
    frames.checked_mul(page_size).map(|bytes| bytes / 1024)
}

fn decimal(mut value: u64, buffer: &mut [u8; 20]) -> &str {
    let mut cursor = buffer.len();
    loop {
        cursor -= 1;
        buffer[cursor] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    core::str::from_utf8(&buffer[cursor..]).expect("decimal digits are valid UTF-8")
}

/// `free` — print one physical-frame allocator snapshot in KiB.
pub fn cmd_free(_args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let info = syscall::sys_mem_info().map_err(|_| {
        crate::executor::shell_println("free: MemInfo denied or unavailable");
        ViError::Unknown
    })?;
    let values = (
        frames_to_kib(info.total_frames, info.page_size),
        frames_to_kib(info.used_frames, info.page_size),
        frames_to_kib(info.free_frames, info.page_size),
    );
    let (Some(total_kib), Some(used_kib), Some(free_kib)) = values else {
        crate::executor::shell_println("free: MemInfo denied or unavailable");
        return Err(ViError::Unknown);
    };
    if used_kib.checked_add(free_kib) != Some(total_kib) {
        crate::executor::shell_println("free: MemInfo denied or unavailable");
        return Err(ViError::Unknown);
    }

    let mut total_buf = [0u8; 20];
    let mut used_buf = [0u8; 20];
    let mut free_buf = [0u8; 20];
    let total = decimal(total_kib, &mut total_buf);
    let used = decimal(used_kib, &mut used_buf);
    let free = decimal(free_kib, &mut free_buf);

    crate::executor::shell_println("              total        used        free");
    crate::executor::shell_print("Mem (KiB):    ");
    crate::executor::shell_print(total);
    crate::executor::shell_print("      ");
    crate::executor::shell_print(used);
    crate::executor::shell_print("      ");
    crate::executor::shell_println(free);
    Ok(())
}

/// `env` — list all environment key=value pairs from the Config Cell.
pub fn cmd_env(_args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    crate::executor::shell_println("PATH=/bin");
    crate::executor::shell_println("SHELL=/bin/shell");
    crate::executor::shell_println("OS=Cellos");
    Ok(())
}

/// `uptime` — print time since boot in seconds.
///
/// Reads the kernel monotonic timer; converts ticks to seconds at 10 MHz.
pub fn cmd_uptime(_args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let ticks = syscall::sys_get_time();
    let secs = ticks / 10_000_000; // 10 MHz mtime
    crate::executor::shell_print(&alloc::format!("up {} seconds\n", secs));
    Ok(())
}

/// `shutdown` — cleanly power off the system via SBI SRST. Does not return.
///
/// Routes through raw kernel syscall 502 (SBI System Reset Extension) which
/// calls OpenSBI from S-mode, powering off the machine.
pub fn cmd_shutdown() -> ViResult<()> {
    ostd::io::println("System shutting down...");
    syscall::sys_shutdown()
}

/// `sleep <seconds>` — pause execution for the given number of seconds.
///
/// Uses the kernel monotonic timer (mtime at 10 MHz on QEMU RV64).
/// Yields on each iteration so other tasks keep running during the delay.
pub fn cmd_sleep(mut args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    const TIMER_HZ: u64 = 10_000_000; // 10 MHz mtime
    let secs: u64 = match args.next().and_then(|s| {
        let mut n = 0u64;
        for ch in s.bytes() {
            if !ch.is_ascii_digit() {
                return None;
            }
            n = n.saturating_mul(10).saturating_add((ch - b'0') as u64);
        }
        Some(n)
    }) {
        Some(n) => n,
        None => {
            ostd::io::println("Usage: sleep <seconds>");
            return Ok(());
        }
    };
    let deadline = syscall::sys_get_time().saturating_add(secs.saturating_mul(TIMER_HZ));
    while syscall::sys_get_time() < deadline {
        ostd::task::yield_now();
    }
    Ok(())
}

/// `blktest` — attempt a raw block read from the shell cell (a non-VFS cell).
///
/// Prints `"blkio: denied"` when Phase G's capability gate correctly rejects the
/// call, or `"blkio: ALLOWED (BUG)"` if the gate is missing. Used exclusively
/// by the `block_io_denied_non_vfs` integration test.
pub fn cmd_blkio_test(_args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let mut buf = [0u8; 512];
    if syscall::sys_blk_read(0, &mut buf) {
        ostd::io::println("blkio: ALLOWED (BUG)");
    } else {
        ostd::io::println("blkio: denied");
    }
    Ok(())
}
