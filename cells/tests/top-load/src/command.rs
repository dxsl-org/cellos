//! Pure parts of the fixture: the line protocol, the bounded input line, the output line
//! writer and the bench arithmetic. Host-tested; nothing here touches a syscall.

use core::fmt;

/// Most workers one Cell can hold: `MAX_THREADS_PER_CELL` (32) counts the root.
pub const MAX_WORKERS: usize = 31;
/// Longest accepted command line in bytes.
pub const LINE_MAX: usize = 64;
/// Largest `bench N`.
pub const BENCH_MAX: u64 = 1_000_000;

/// One typed command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// The first `K` live workers spin, the rest park.
    Busy(usize),
    /// Every worker parks.
    Idle,
    /// The first `K` live workers return and are joined.
    Exit(usize),
    Ping,
    Quit,
    /// `N` timed `GetTime(0)` calls, then `N` timed `Yield` calls.
    Bench(u64),
}

/// A decimal without sign, leading `+` or blanks.
fn decimal(text: &str) -> Option<u64> {
    if text.is_empty() || text.len() > 19 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// The worker count argument: `1..=MAX_WORKERS`.
pub fn parse_workers(arg: &str) -> Option<usize> {
    decimal(arg).and_then(|n| usize::try_from(n).ok()).filter(|n| (1..=MAX_WORKERS).contains(n))
}

/// Parse one command line. Counts are range-checked against `live` (the workers not yet
/// exited): `busy`/`exit` need `1..=live`, `bench` needs `1..=BENCH_MAX`.
pub fn parse(line: &str, live: usize) -> Option<Command> {
    let mut words = line.split_ascii_whitespace();
    let verb = words.next()?;
    let count = words.next();
    if words.next().is_some() {
        return None;
    }
    let workers = |text: Option<&str>| {
        text.and_then(decimal).and_then(|n| usize::try_from(n).ok()).filter(|n| (1..=live).contains(n))
    };
    match (verb, count) {
        ("busy", _) => workers(count).map(Command::Busy),
        ("exit", _) => workers(count).map(Command::Exit),
        ("bench", Some(n)) => decimal(n).filter(|n| (1..=BENCH_MAX).contains(n)).map(Command::Bench),
        ("idle", None) => Some(Command::Idle),
        ("ping", None) => Some(Command::Ping),
        ("quit", None) => Some(Command::Quit),
        _ => None,
    }
}

/// The command line being typed: bounded, an over-long line is remembered as such.
pub struct InputLine {
    bytes: [u8; LINE_MAX],
    len: usize,
    overflow: bool,
}

impl InputLine {
    pub const fn new() -> InputLine {
        InputLine { bytes: [0; LINE_MAX], len: 0, overflow: false }
    }

    pub fn push(&mut self, text: &str) {
        for &byte in text.as_bytes() {
            if self.len == LINE_MAX {
                self.overflow = true;
            } else {
                self.bytes[self.len] = byte;
                self.len += 1;
            }
        }
    }

    /// Enter: the completed line (`None` when it overflowed or is not UTF-8); the buffer
    /// starts empty again either way.
    pub fn take(&mut self) -> Option<&str> {
        let len = core::mem::take(&mut self.len);
        let overflow = core::mem::take(&mut self.overflow);
        if overflow {
            return None;
        }
        core::str::from_utf8(&self.bytes[..len]).ok()
    }
}

/// One output line formatted into a fixed buffer (no heap).
pub struct OutputLine {
    bytes: [u8; 512],
    len: usize,
}

impl OutputLine {
    pub const fn new() -> OutputLine {
        OutputLine { bytes: [0; 512], len: 0 }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl fmt::Write for OutputLine {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let end = self.len.checked_add(text.len()).filter(|&end| end <= self.bytes.len()).ok_or(fmt::Error)?;
        self.bytes[self.len..end].copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}

/// `t1,t2,…` of `tids`.
pub struct Tids<'a>(pub &'a [usize]);

impl fmt::Display for Tids<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, tid) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_str(",")?;
            }
            write!(f, "{tid}")?;
        }
        Ok(())
    }
}

/// Nanoseconds per call of `calls` calls that took `ticks` timer ticks at `frequency` Hz.
pub fn per_call_ns(ticks: u64, frequency: u64, calls: u64) -> Option<u64> {
    if frequency == 0 || calls == 0 {
        return None;
    }
    let ns = u128::from(ticks) * 1_000_000_000 / u128::from(frequency) / u128::from(calls);
    u64::try_from(ns).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::fmt::Write;

    #[test]
    fn worker_counts_are_one_to_thirty_one() {
        assert_eq!(parse_workers("1"), Some(1));
        assert_eq!(parse_workers("31"), Some(31));
        for bad in ["0", "32", "", "-1", "+3", " 3", "3x", "99999999999999999999"] {
            assert_eq!(parse_workers(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn commands_parse_with_counts_bounded_by_live_workers() {
        assert_eq!(parse("busy 1", 31), Some(Command::Busy(1)));
        assert_eq!(parse("  busy   2 ", 31), Some(Command::Busy(2)));
        assert_eq!(parse("busy 31", 31), Some(Command::Busy(31)));
        assert_eq!(parse("busy 31", 30), None);
        assert_eq!(parse("busy 0", 31), None);
        assert_eq!(parse("busy", 31), None);
        assert_eq!(parse("exit 1", 1), Some(Command::Exit(1)));
        assert_eq!(parse("exit 1", 0), None);
        assert_eq!(parse("idle", 3), Some(Command::Idle));
        assert_eq!(parse("ping", 0), Some(Command::Ping));
        assert_eq!(parse("quit", 0), Some(Command::Quit));
        assert_eq!(parse("bench 10000", 0), Some(Command::Bench(10_000)));
        assert_eq!(parse("bench 0", 0), None);
        assert_eq!(parse("bench 1000001", 0), None);
    }

    #[test]
    fn unknown_or_extra_words_are_refused() {
        for bad in ["", "   ", "Busy 1", "idle now", "ping 1", "quit 0", "busy 1 2", "spin 1", "busy x"] {
            assert_eq!(parse(bad, 31), None, "{bad:?}");
        }
    }

    #[test]
    fn input_line_is_bounded_and_reset_on_take() {
        let mut line = InputLine::new();
        line.push("bu");
        line.push("sy 2");
        assert_eq!(line.take(), Some("busy 2"));
        assert_eq!(line.take(), Some(""));
        let long = "x".repeat(LINE_MAX + 1);
        line.push(&long);
        assert_eq!(line.take(), None);
        line.push("ping");
        assert_eq!(line.take(), Some("ping"));
        line.push(&"y".repeat(LINE_MAX));
        assert_eq!(line.take().map(str::len), Some(LINE_MAX));
    }

    #[test]
    fn output_line_formats_tid_lists_and_refuses_overflow() {
        let mut out = OutputLine::new();
        writeln!(out, "TOPLOAD busy tids={}", Tids(&[12, 7, 300])).unwrap();
        assert_eq!(out.as_bytes(), b"TOPLOAD busy tids=12,7,300\n");
        let mut out = OutputLine::new();
        write!(out, "{}", Tids(&[])).unwrap();
        assert_eq!(out.as_bytes(), b"");
        let mut out = OutputLine::new();
        assert!(out.write_str(&"z".repeat(513)).is_err());
        assert_eq!(out.as_bytes(), b"");
        let all: [usize; MAX_WORKERS] = core::array::from_fn(|i| 1_000_000 + i);
        let mut out = OutputLine::new();
        writeln!(out, "TOPLOAD ready workers={MAX_WORKERS} tids={}", Tids(&all)).unwrap();
        assert!(out.as_bytes().ends_with(b",1000030\n"));
    }

    #[test]
    fn per_call_cost_is_integer_nanoseconds() {
        // 1 GHz counter: one tick is one ns.
        assert_eq!(per_call_ns(10_000, 1_000_000_000, 1_000), Some(10));
        // 100 MHz HPET-like counter: 25 ticks = 250 ns over 10 calls.
        assert_eq!(per_call_ns(25, 100_000_000, 10), Some(25));
        assert_eq!(per_call_ns(1, 0, 1), None);
        assert_eq!(per_call_ns(1, 1, 0), None);
        assert_eq!(per_call_ns(u64::MAX, 1, 1), None);
    }
}
