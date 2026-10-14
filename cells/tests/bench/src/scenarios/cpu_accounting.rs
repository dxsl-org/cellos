//! Selected-image G1 action schedule; markers are pacing observations, never CPU proof.
use ostd::syscall::{self, SyscallResult};
use core::sync::atomic::{AtomicU8, Ordering};

static STARTED: AtomicU8 = AtomicU8::new(0);

const WORKERS: usize = 2;
const MAX_CHUNKS: usize = 1_000_000;

/// One bounded syscall per record, with no shared userspace heap access from
/// concurrent workers. Formatting directly into stack storage also keeps
/// measurement bookkeeping out of the workload's allocator behavior.
fn record(args: core::fmt::Arguments<'_>) {
    struct Buffer { bytes: [u8; 256], len: usize }
    impl core::fmt::Write for Buffer {
        fn write_str(&mut self, text: &str) -> core::fmt::Result {
            let end = self.len.checked_add(text.len()).ok_or(core::fmt::Error)?;
            if end > self.bytes.len() { return Err(core::fmt::Error); }
            self.bytes[self.len..end].copy_from_slice(text.as_bytes());
            self.len = end;
            Ok(())
        }
    }
    let mut buffer = Buffer { bytes: [0; 256], len: 0 };
    if core::fmt::write(&mut buffer, args).is_err()
        || core::fmt::Write::write_str(&mut buffer, "\n").is_err() {
        syscall::sys_exit(1);
    }
    let text = core::str::from_utf8(&buffer.bytes[..buffer.len])
        .unwrap_or_else(|_| syscall::sys_exit(1));
    let _ = syscall::sys_log(text);
}

fn invalid(reason: &str) -> ! {
    record(format_args!("G1-WORKLOAD invalid={}", reason));
    syscall::sys_exit(1)
}

fn now() -> u64 {
    syscall::sys_get_time_ms().unwrap_or_else(|| invalid("pacing-clock-unavailable"))
}

fn worker(index: usize, yields: bool) {
    // One blocking release, bound to its actual sender by the existing trailer.
    // The decoder joins that sender to this generation's measured root endpoint.
    let mut release = [0u8; 64];
    let sender = match syscall::sys_recv_attested(0, &mut release) {
        SyscallResult::Ok(sender) if sender != 0 => sender,
        _ => invalid("release-receive"),
    };
    let caller = api::caller_identity::CallerIdentity::from_recv_buf(&release)
        .unwrap_or_else(|| invalid("release-sender-unavailable"));
    if caller.sender_tid != sender as u64 || release[0] != 0x71 || release[1] != index as u8 {
        invalid("release-sender-token");
    }
    let mut start_gate = [0u8; 3];
    if !matches!(syscall::sys_recv(0, &mut start_gate), SyscallResult::Ok(0))
        || start_gate[0] != 0x74 || !matches!(start_gate[1], 1 | 2) || start_gate[2] != 0 {
        invalid("compute-barrier");
    }
    if start_gate[1] == 2 {
        STARTED.fetch_or(1 << index, Ordering::AcqRel);
        let mut both_started = false;
        for _ in 0..MAX_CHUNKS {
            if STARTED.load(Ordering::Acquire) == 3 {
                both_started = true;
                break;
            }
            core::hint::spin_loop();
        }
        if !both_started { invalid("distinct-hart-start-bound"); }
    }
    let start = now();
    record(format_args!("G1-ACTION worker={} action=start sender_tid={} sender_cell={} sender_generation={} ms={}", index, sender, caller.cell_id, caller.generation, start));
    if yields {
        for action in 0..4 {
            record(format_args!("G1-ACTION worker={} action=yield ordinal={} ms={}", index, action, now()));
            ostd::task::yield_now();
        }
    }
    let duration = if yields { 1 } else { 16 };
    let mut value = index as u64 + 1;
    let mut ended = false;
    for _ in 0..MAX_CHUNKS {
        for _ in 0..256 {
            value = value.wrapping_mul(6364136223846793005).wrapping_add(1);
            core::hint::black_box(value);
        }
        if now().checked_sub(start).unwrap_or_else(|| invalid("pacing-regression")) >= duration {
            ended = true;
            break;
        }
    }
    if !ended {
        invalid("work-bound");
    }
    // Wait's existing API does not retain an outcome after the reaper removes
    // the TID. The private kernel handshake releases this receive only after
    // the real root waiter is installed; exit and quiescent reaping stay real.
    let mut join = [0u8; 2];
    if !matches!(syscall::sys_recv(0, &mut join), SyscallResult::Ok(0))
        || join != [0x73, 0] {
        invalid("join-registration");
    }
    record(format_args!("G1-ACTION worker={} action=exit ms={}", index, now()));
}

pub fn run(role: &str) -> ! {
    let mode = role.strip_prefix("cpu-accounting:").unwrap_or_else(|| invalid("role"));
    if !matches!(mode, "overlap" | "yield" | "retention") {
        invalid("mode");
    }
    record(format_args!("G1-WORKLOAD mode={} workers=2 explicit_yields={} delay_source=hpet-period16 max_chunks={} status=start", mode, if mode == "yield" { 8 } else { 0 }, MAX_CHUNKS));
    let mut tids = [0; WORKERS];
    for (index, tid) in tids.iter_mut().enumerate() {
        let yields = mode == "yield";
        *tid = match ostd::task::spawn(move || worker(index, yields)) {
            SyscallResult::Ok(tid) if tid != 0 => tid,
            _ => invalid("thread-admission"),
        };
        record(format_args!("G1-ACTION worker={} action=admit tid={} ms={}", index, tid, now()));
    }
    for (index, &tid) in tids.iter().enumerate() {
        record(format_args!("G1-ACTION worker={} action=release tid={} ms={}", index, tid, now()));
        if !matches!(syscall::sys_send(tid, &[0x71, index as u8]), SyscallResult::Ok(_)) {
            invalid("release-send");
        }
    }
    for (index, tid) in tids.into_iter().enumerate() {
        match syscall::sys_wait(tid) {
            SyscallResult::Ok(0) => {},
            _ => invalid("thread-terminal"),
        }
        record(format_args!("G1-ACTION worker={} action=joined tid={} ms={}", index, tid, now()));
    }
    record(format_args!("G1-ACTION worker=2 action=delay-start ms={}", now()));
    let mut message = [0u8; 2];
    // One existing blocking receive. Only the selected private kernel producer
    // releases this wait at checked HPET period 16; software ticks are pacing
    // observations, not a conversion certificate for the charge clock.
    if !matches!(syscall::sys_recv(0, &mut message), SyscallResult::Ok(0))
        || message != [0x72, 16] {
        invalid("delay-terminal");
    }
    record(format_args!("G1-ACTION worker=2 action=delay-end ms={}", now()));
    record(format_args!("G1-WORKLOAD mode={} workers=2 status=terminal ms={}", mode, now()));
    syscall::sys_exit(0)
}
