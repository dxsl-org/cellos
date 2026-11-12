//! Emulated MC146818 CMOS/RTC (index port 0x70, data port 0x71).
//!
//! Linux polls the RTC early (status register A's UIP bit, then the time
//! registers) and sets its wall clock from it. The time registers carry the
//! host's UTC wall time, read on every access: a guest that validates TLS
//! certificates (Chromium) refuses every site when its clock reads year 2000.
//! Without a host RTC the registers read zero and the guest still boots.

/// CMOS register indices.
const REG_SECONDS: u8 = 0x00;
const REG_MINUTES: u8 = 0x02;
const REG_HOURS: u8 = 0x04;
const REG_WEEKDAY: u8 = 0x06;
const REG_DAY: u8 = 0x07;
const REG_MONTH: u8 = 0x08;
const REG_YEAR: u8 = 0x09;
const REG_STATUS_A: u8 = 0x0A;
const REG_STATUS_B: u8 = 0x0B;
const REG_STATUS_C: u8 = 0x0C;
const REG_STATUS_D: u8 = 0x0D;
/// Century register (ACPI FADT default, which Linux uses when it has none).
const REG_CENTURY: u8 = 0x32;

/// Minimal RTC: just the selected index (NMI-disable bit masked off).
pub struct CmosRtc {
    index: u8,
}

impl CmosRtc {
    pub const fn new() -> Self {
        Self { index: 0 }
    }

    pub fn owns(port: u16) -> bool {
        matches!(port, 0x70 | 0x71)
    }

    pub fn write(&mut self, port: u16, val: u32) {
        if port == 0x70 {
            self.index = (val & 0x7F) as u8; // low 7 bits = register; bit7 = NMI mask
        }
        // Writes to 0x71 (setting RTC) are ignored — the guest's clock is virtual.
    }

    pub fn read(&self, port: u16) -> u32 {
        if port != 0x71 {
            return 0;
        }
        u32::from(register(self.index, ostd::syscall::sys_get_wall_secs()))
    }
}

/// Value of CMOS register `index` at `unix_secs` UTC (0: no wall clock).
fn register(index: u8, unix_secs: u64) -> u8 {
    let time = CivilTime::from_unix(unix_secs);
    match index {
        REG_STATUS_A => 0x26, // UIP=0 (not updating), 32 kHz base, rate 6
        REG_STATUS_B => 0x02, // 24-hour mode, BCD
        REG_STATUS_C => 0x00, // no interrupt flags pending
        REG_STATUS_D => 0x80, // battery/RAM valid
        _ if unix_secs == 0 => 0,
        REG_SECONDS => bcd(time.second),
        REG_MINUTES => bcd(time.minute),
        REG_HOURS => bcd(time.hour),
        REG_WEEKDAY => bcd(time.weekday),
        REG_DAY => bcd(time.day),
        REG_MONTH => bcd(time.month),
        REG_YEAR => bcd((time.year % 100) as u8),
        REG_CENTURY => bcd((time.year / 100) as u8),
        _ => 0,
    }
}

fn bcd(value: u8) -> u8 {
    ((value / 10) << 4) | (value % 10)
}

/// UTC calendar fields; `weekday` is 1 (Sunday) ..= 7, as the RTC counts.
#[derive(Debug, PartialEq, Eq)]
struct CivilTime {
    year: u32,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
    weekday: u8,
}

impl CivilTime {
    /// Days-to-civil conversion (H. Hinnant, "chrono-Compatible Low-Level Date
    /// Algorithms"), valid for every date from 1970 on.
    fn from_unix(secs: u64) -> Self {
        let days = secs / 86_400;
        let of_day = secs % 86_400;
        let z = days + 719_468;
        let era = z / 146_097;
        let doe = z % 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + u64::from(month <= 2);
        Self {
            year: year as u32,
            month: month as u8,
            day: day as u8,
            hour: (of_day / 3600) as u8,
            minute: (of_day / 60 % 60) as u8,
            second: (of_day % 60) as u8,
            // 1970-01-01 was a Thursday (5 in 1 = Sunday counting).
            weekday: ((days + 4) % 7 + 1) as u8,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_carry_the_wall_time_in_bcd() {
        // 2026-11-12 09:05:07 UTC, a Thursday.
        let secs = 1_794_474_307;
        let read = |index| register(index, secs);
        assert_eq!(
            [
                read(REG_CENTURY),
                read(REG_YEAR),
                read(REG_MONTH),
                read(REG_DAY),
                read(REG_HOURS),
                read(REG_MINUTES),
                read(REG_SECONDS),
                read(REG_WEEKDAY),
            ],
            [0x20, 0x26, 0x11, 0x12, 0x09, 0x05, 0x07, 0x05]
        );
    }

    #[test]
    fn leap_day_and_year_end_convert() {
        let leap = CivilTime::from_unix(1_709_164_800); // 2024-02-29 00:00:00
        assert_eq!((leap.year, leap.month, leap.day), (2024, 2, 29));
        let last = CivilTime::from_unix(1_735_689_599); // 2024-12-31 23:59:59
        assert_eq!(
            (
                last.year,
                last.month,
                last.day,
                last.hour,
                last.minute,
                last.second
            ),
            (2024, 12, 31, 23, 59, 59)
        );
    }

    #[test]
    fn no_wall_clock_reads_zero_time_with_valid_status() {
        assert_eq!(register(REG_YEAR, 0), 0);
        assert_eq!(register(REG_STATUS_D, 0), 0x80);
    }
}
