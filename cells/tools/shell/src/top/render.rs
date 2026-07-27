use text_engine::top::{
    format_bytes, sort_label, state_label, SortKey, TopRow, MAX_ROWS, TIMER_HZ,
};

pub fn render_frame(rows: &[TopRow], sort: SortKey, batch: bool, remaining: usize) {
    if !batch {
        ostd::io::print("\x1b[2J\x1b[1;1H");
    }
    let uptime = ostd::syscall::sys_get_time() / TIMER_HZ;
    crate::executor::shell_print(&alloc::format!(
        "top: uptime {}s  tasks {}  sort {}{}{}\n",
        uptime,
        rows.len(),
        sort_label(sort),
        if batch { "  batch" } else { "  interactive" },
        if batch {
            alloc::format!("  remaining {}", remaining)
        } else {
            alloc::string::String::from("  q to quit")
        }
    ));
    crate::executor::shell_println("  PID  STATE      CPU%   HEAP      MEM       NAME");
    crate::executor::shell_println("  ---  ---------  -----  --------  --------  ----------------");
    for row in rows.iter().take(MAX_ROWS) {
        crate::executor::shell_print(&alloc::format!(
            "  {:<4} {:<10} {:>3}.{:01}  {:>8}  {:>8}  {}\n",
            row.pid,
            state_label(row.state),
            row.cpu_tenths / 10,
            row.cpu_tenths % 10,
            format_bytes(row.heap_bytes),
            format_bytes(row.mem_bytes),
            row.name
        ));
    }
}
