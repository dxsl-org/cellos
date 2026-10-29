//! System-information shell built-ins: pwd, uname, date, free, env.

use ostd::prelude::*;
use ostd::syscall;

pub use crate::cmd_cwd::cmd_pwd;

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
fn validated_kib(
    total_frames: u64,
    used_frames: u64,
    free_frames: u64,
    page_size: u64,
) -> Option<(u64, u64, u64)> {
    if used_frames.checked_add(free_frames) != Some(total_frames) || page_size == 0 {
        return None;
    }
    Some((
        frames_to_kib(total_frames, page_size)?,
        frames_to_kib(used_frames, page_size)?,
        frames_to_kib(free_frames, page_size)?,
    ))
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
    let Some((total_kib, used_kib, free_kib)) = validated_kib(
        info.total_frames,
        info.used_frames,
        info.free_frames,
        info.page_size,
    ) else {
        crate::executor::shell_println("free: MemInfo denied or unavailable");
        return Err(ViError::Unknown);
    };

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
    crate::executor::shell_println("SHELL=/bin/cellbox");
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

/// `shutdown` — refused. Raw syscall 502 is permitted by the kernel only for the retained
/// `/bin/shutdown` worker, so the shell's own attempt is denied (logged by the kernel as
/// `raw opcode 502 denied for cellbox`) and returns here; the shell stays alive.
pub fn cmd_shutdown() -> i32 {
    syscall::sys_shutdown_denied();
    crate::executor::shell_stderrln("shutdown: use /bin/shutdown");
    crate::commands::REFUSED_STATUS
}

/// `sleep <seconds>` — pause execution for the given number of whole seconds.
///
/// Real seconds on the monotonic clock on every architecture, parked in one-tick
/// `SetTimer` steps (`top::wait_until`), so the shell is off the run queue while it waits.
/// Ctrl-C on the session ends the wait with `^C` and status 130 (S10-3b); without a
/// session (a one-shot) the kernel's operation cancel is the only stop.
pub fn cmd_sleep(args: &[String]) -> i32 {
    let Some(delay_ns) = args.first().and_then(|operand| sleep_delay_ns(operand)) else {
        crate::executor::shell_stderrln("Usage: sleep <seconds>");
        return 0;
    };
    let latch = crate::executor::job_latch();
    sleep_status(crate::top::wait_until(delay_ns, &mut || latch.poll_now()))
}

/// Status of a finished `sleep`: 0 when the time elapsed, `^C` and 130 when interrupted.
fn sleep_status(waited: crate::top::Waited) -> i32 {
    match waited {
        crate::top::Waited::Elapsed => 0,
        crate::top::Waited::Interrupted => {
            crate::executor::shell_console("^C\n");
            crate::typed::INTERRUPTED_STATUS
        }
    }
}

/// Nanoseconds requested by a `sleep` operand of ASCII digits (whole seconds,
/// saturating); `None` for any other operand.
fn sleep_delay_ns(operand: &str) -> Option<u64> {
    let mut secs = 0u64;
    for ch in operand.bytes() {
        if !ch.is_ascii_digit() {
            return None;
        }
        secs = secs.saturating_mul(10).saturating_add((ch - b'0') as u64);
    }
    Some(secs.saturating_mul(1_000_000_000))
}

/// `blktest` — attempt a raw block read from the shell cell (a non-VFS cell).
///
/// Prints `"blkio: denied"` when Phase G's capability gate correctly rejects the
/// call, or `"blkio: ALLOWED (BUG)"` if the gate is missing. Used exclusively
/// by the `block_io_denied_non_vfs` integration test.
pub fn cmd_blkio_test(_args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let mut buf = [0u8; 512];
    if syscall::sys_blk_read(0, &mut buf) {
        crate::executor::shell_println("blkio: ALLOWED (BUG)");
    } else {
        crate::executor::shell_println("blkio: denied");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{sleep_delay_ns, sleep_status, validated_kib};

    #[test]
    fn sleep_returns_130_when_interrupted_and_0_when_elapsed() {
        assert_eq!(sleep_status(crate::top::Waited::Elapsed), 0);
        assert_eq!(sleep_status(crate::top::Waited::Interrupted), 130);
    }

    #[test]
    fn valid_frame_snapshot_survives_independent_kib_flooring() {
        assert_eq!(validated_kib(2, 1, 1, 1536), Some((3, 1, 1)));
    }

    #[test]
    fn invalid_zero_sized_or_overflowing_frame_snapshot_is_rejected() {
        assert_eq!(validated_kib(3, 1, 1, 4096), None);
        assert_eq!(validated_kib(2, 1, 1, 0), None);
        assert_eq!(validated_kib(u64::MAX, u64::MAX, 1, 1), None);
        assert_eq!(validated_kib(u64::MAX, u64::MAX, 0, 2), None);
    }

    #[test]
    fn sleep_operand_is_whole_real_seconds_in_nanoseconds() {
        // One second is 1e9 monotonic ns on every architecture (it was 10 ms of
        // raw x86 HPET ticks), so the shell is parked for the full second.
        assert_eq!(sleep_delay_ns("1"), Some(1_000_000_000));
        assert_eq!(sleep_delay_ns("0"), Some(0));
        assert_eq!(sleep_delay_ns("12"), Some(12_000_000_000));
        assert_eq!(sleep_delay_ns("99999999999999999999"), Some(u64::MAX));
    }

    #[test]
    fn sleep_operand_other_than_digits_is_refused() {
        assert_eq!(sleep_delay_ns("1.5"), None);
        assert_eq!(sleep_delay_ns("-1"), None);
        assert_eq!(sleep_delay_ns("1s"), None);
    }
}

/// `ifconfig` / `ip` — query and print the network interface IP address from service-net.
pub fn cmd_ifconfig(_args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    use api::ipc::{NetRequest, NetResponse};
    use ostd::syscall::sys_lookup_service;

    let Some(net_tid) = sys_lookup_service(api::syscall::service::NET) else {
        crate::executor::shell_println("ifconfig: network service (/bin/net) is not running");
        return Ok(());
    };

    let mut send = [0u8; 512];
    let mut reply = [0u8; 512];
    let len = api::ipc::encode(&NetRequest::GetLocalIp, &mut send)
        .map(|b| b.len())
        .unwrap_or(0);
    ostd::syscall::sys_send(net_tid, &send[..len]);

    if ostd::ipc::recv_from(net_tid, &mut reply).is_ok() {
        if let Ok(NetResponse::Addr(ip)) = api::ipc::decode::<NetResponse>(&reply) {
            if ip == [0, 0, 0, 0] {
                crate::executor::shell_println("eth0: link up, waiting for DHCP lease...");
            } else {
                crate::executor::shell_println(&alloc::format!(
                    "eth0: inet {}.{}.{}.{}  netmask 255.255.255.0  (DHCP)",
                    ip[0],
                    ip[1],
                    ip[2],
                    ip[3]
                ));
            }
            return Ok(());
        }
    }
    crate::executor::shell_println("ifconfig: no response from network service");
    Ok(())
}

// ─── date ─────────────────────────────────────────────────────────────────────

/// Convert Unix epoch seconds to calendar fields (UTC, proleptic Gregorian).
fn epoch_to_datetime(mut secs: u64) -> (u64, u8, u8, u8, u8, u8) {
    fn is_leap(y: u64) -> bool {
        (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
    }
    fn days_in_month(m: u8, y: u64) -> u64 {
        match m {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if is_leap(y) => 29,
            2 => 28,
            _ => 30,
        }
    }
    let mut year = 1970u64;
    loop {
        let days = if is_leap(year) { 366 } else { 365 };
        if secs < days * 86400 {
            break;
        }
        secs -= days * 86400;
        year += 1;
    }
    let mut month = 1u8;
    loop {
        let d = days_in_month(month, year) * 86400;
        if secs < d {
            break;
        }
        secs -= d;
        month += 1;
    }
    let day = (secs / 86400 + 1) as u8;
    secs %= 86400;
    let hour = (secs / 3600) as u8;
    secs %= 3600;
    let min = (secs / 60) as u8;
    let sec = (secs % 60) as u8;
    (year, month, day, hour, min, sec)
}

fn pad2(buf: &mut [u8; 2], n: u8) -> &str {
    buf[0] = b'0' + n / 10;
    buf[1] = b'0' + n % 10;
    core::str::from_utf8(buf).unwrap_or("??")
}

/// `date` — print the current UTC date and time from the hardware RTC.
///
/// Falls back to the monotonic timer if no RTC is present (epoch = 0).
pub fn cmd_date(_args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let epoch_secs = ostd::syscall::sys_get_wall_secs();
    if epoch_secs == 0 {
        // No RTC available — show uptime instead
        let ticks = syscall::sys_get_time();
        let secs = ticks / 10_000_000;
        crate::executor::shell_println(&alloc::format!("date: no RTC — uptime {} seconds", secs));
        return Ok(());
    }

    let month_names = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    let (year, month, day, hour, min, sec) = epoch_to_datetime(epoch_secs);
    let month_name = month_names
        .get((month as usize).saturating_sub(1))
        .copied()
        .unwrap_or("???");

    let mut h2 = [0u8; 2];
    let mut m2 = [0u8; 2];
    let mut s2 = [0u8; 2];
    let mut d2 = [0u8; 2];

    // Output: "Sep 03 14:35:22 UTC 2026"
    let out = alloc::format!(
        "{} {} {}:{}:{} UTC {}",
        month_name,
        pad2(&mut d2, day),
        pad2(&mut h2, hour),
        pad2(&mut m2, min),
        pad2(&mut s2, sec),
        year,
    );
    crate::executor::shell_println(&out);
    Ok(())
}
