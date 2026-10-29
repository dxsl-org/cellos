#![no_std]
#![no_main]
#![forbid(unsafe_code)]
extern crate ostd;

api::declare_syscalls![Log, MemInfo, GetTime];

ostd::cell_main!(cell_main);

/// free — print one physical-frame allocator snapshot in KiB.
fn cell_main() {
    let output = ostd::io::ProcessOutput::read_own()
        .unwrap_or_else(|_| ostd::syscall::sys_exit(1));
    let info = match ostd::syscall::sys_mem_info() {
        Ok(info) => info,
        Err(_) => {
            emit(output.stderr(), &[b"free: MemInfo denied or unavailable\n"]);
            ostd::syscall::sys_exit(1);
        }
    };

    let Some((total_kib, used_kib, free_kib)) = validated_kib(
        info.total_frames,
        info.used_frames,
        info.free_frames,
        info.page_size,
    ) else {
        emit(output.stderr(), &[b"free: MemInfo denied or unavailable\n"]);
        ostd::syscall::sys_exit(1);
    };

    let mut total_buf = [0u8; 20];
    let mut used_buf = [0u8; 20];
    let mut free_buf = [0u8; 20];
    let total = decimal(total_kib, &mut total_buf);
    let used = decimal(used_kib, &mut used_buf);
    let free = decimal(free_kib, &mut free_buf);

    emit(
        output.stdout(),
        &[
            b"              total        used        free\n",
            b"Mem (KiB):    ",
            total.as_bytes(),
            b"      ",
            used.as_bytes(),
            b"      ",
            free.as_bytes(),
            b"\n",
        ],
    );
    ostd::syscall::sys_exit(0);
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
