#![no_std]
#![no_main]
#![forbid(unsafe_code)]
extern crate ostd;

/// Convert Unix epoch seconds to (year, month, day, hour, min, sec) in UTC.
fn epoch_to_datetime(mut secs: u64) -> (u64, u8, u8, u8, u8, u8) {
    fn is_leap(y: u64) -> bool {
        (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
    }
    fn days_in_month(m: u8, y: u64) -> u64 {
        match m {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if is_leap(y) {
                    29
                } else {
                    28
                }
            }
            _ => 0,
        }
    }
    let mut year = 1970u64;
    loop {
        let year_secs = if is_leap(year) {
            366 * 86400
        } else {
            365 * 86400
        };
        if secs < year_secs {
            break;
        }
        secs -= year_secs;
        year += 1;
    }
    let mut month = 1u8;
    loop {
        let m_secs = days_in_month(month, year) * 86400;
        if secs < m_secs {
            break;
        }
        secs -= m_secs;
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

fn decimal(mut value: u64, width: usize, buffer: &mut [u8; 20]) -> &[u8] {
    let mut cursor = buffer.len();
    loop {
        cursor -= 1;
        buffer[cursor] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 && buffer.len() - cursor >= width {
            break;
        }
    }
    &buffer[cursor..]
}

ostd::cell_main!(cell_main);

/// date — print wall-clock time from hardware RTC (UTC).
fn cell_main() {
    let output = ostd::io::ProcessOutput::read_own()
        .unwrap_or_else(|_| ostd::syscall::sys_exit(1));
    let epoch = ostd::syscall::sys_get_wall_secs();
    if epoch == 0 {
        // RTC absent — fall back to uptime display.
        let ticks = ostd::syscall::sys_get_time();
        let secs = ticks / 10_000_000; // 10 MHz mtime
        let mins = secs / 60;
        let hrs = mins / 60;
        let mut hrs_buf = [0u8; 20];
        let mut mins_buf = [0u8; 20];
        let mut secs_buf = [0u8; 20];
        emit(
            output.stdout(),
            &[
                b"Uptime: ",
                decimal(hrs, 1, &mut hrs_buf),
                b"h ",
                decimal(mins % 60, 1, &mut mins_buf),
                b"m ",
                decimal(secs % 60, 1, &mut secs_buf),
                b"s  (no RTC)\n",
            ],
        );
    } else {
        let (y, mo, d, h, mi, s) = epoch_to_datetime(epoch);
        // Format: 2026-06-07 15:30:42 UTC
        let mut year_buf = [0u8; 20];
        let mut month_buf = [0u8; 20];
        let mut day_buf = [0u8; 20];
        let mut hour_buf = [0u8; 20];
        let mut min_buf = [0u8; 20];
        let mut sec_buf = [0u8; 20];
        emit(
            output.stdout(),
            &[
                decimal(y, 4, &mut year_buf),
                b"-",
                decimal(mo as u64, 2, &mut month_buf),
                b"-",
                decimal(d as u64, 2, &mut day_buf),
                b" ",
                decimal(h as u64, 2, &mut hour_buf),
                b":",
                decimal(mi as u64, 2, &mut min_buf),
                b":",
                decimal(s as u64, 2, &mut sec_buf),
                b" UTC\n",
            ],
        );
    }
    ostd::syscall::sys_exit(0);
}

fn emit(stream: &ostd::io::OutputStream, parts: &[&[u8]]) {
    let deadline = match ostd::session::monotonic_ns()
        .ok()
        .and_then(|now| now.checked_add(30_000_000_000))
    {
        Some(deadline) if deadline != u64::MAX => deadline,
        _ => ostd::syscall::sys_exit(1),
    };
    for bytes in parts {
        if stream.write_all(bytes, deadline).is_err() {
            ostd::syscall::sys_exit(1);
        }
    }
}
