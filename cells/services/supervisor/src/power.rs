//! Orderly shutdown for a `PowerRequest` (owner decision H-S2-8 (i)).
//!
//! Contract: freeze (413) every Cell except this supervisor and the storage
//! stack (the VFS and block-driver services and every live block-route
//! provider), give the storage stack a bounded settle window to finish the
//! requests it already holds, then call `SystemPower` (436). Nothing is
//! flushed or synced, so the markers report only freeze counts and the window.
//! A second power-button press ends the window at once. If 436 returns, every
//! Cell this sweep froze is resumed and the 436 error is returned.

use api::syscall::{service, ProcessInfo};
use ostd::io::print_fmt;
use ostd::service::provider_tid;
use ostd::syscall::{
    sys_freeze_cell, sys_get_procs, sys_lookup_block_route, sys_recv_timeout, sys_resume_cell,
    sys_system_power, SyscallResult, SystemPowerAction, SystemPowerError,
};

use api::services::proto::supervisor::POWER_BUTTON_REQUEST;

/// Who asked: the `/bin/shutdown` worker or the kernel's power-button relay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Operator,
    PowerButton,
}

const PROCESS_TABLE_ROWS: usize = 64;
const STATE_TERMINATED: usize = 3;
/// Upper bound of the settle window, in 10 ms scheduler ticks.
const SETTLE_TICKS: u64 = 50;
const SETTLE_POLL_TICKS: u64 = 5;
const KEEP_MAX: usize = 2 + api::block_route::MAX_BLOCK_ROUTES;

fn action_name(action: SystemPowerAction) -> &'static str {
    match action {
        SystemPowerAction::Reboot => "reboot",
        SystemPowerAction::PowerOff => "poweroff",
    }
}

/// Freeze, settle, call 436; on its return resume and report the error.
pub fn shut_down(action: SystemPowerAction, origin: Origin) -> SystemPowerError {
    let mut keep = [0usize; KEEP_MAX];
    let kept = storage_stack(&mut keep);
    let own = provider_tid(service::SUPERVISOR);
    let mut rows = [ProcessInfo::default(); PROCESS_TABLE_ROWS];
    let count = sys_get_procs(&mut rows)
        .unwrap_or(0)
        .min(PROCESS_TABLE_ROWS);
    let mut frozen = [0usize; PROCESS_TABLE_ROWS];
    let (mut froze, mut refused) = (0usize, 0usize);
    for row in &rows[..count] {
        if row.id == 0
            || Some(row.id) == own
            || row.state == STATE_TERMINATED
            || keep[..kept].contains(&row.id)
        {
            continue;
        }
        if sys_freeze_cell(row.id).is_ok() {
            frozen[froze] = row.id;
            froze += 1;
        } else {
            refused += 1;
        }
    }
    print_fmt(format_args!(
        "[supervisor] power: {} ({:?}) froze={} refused={} kept={} settle<={}ms\n",
        action_name(action),
        origin,
        froze,
        refused,
        kept,
        SETTLE_TICKS * 10
    ));
    if settle() {
        print_fmt(format_args!(
            "[supervisor] power: second power-button press ended the settle window\n"
        ));
    }
    print_fmt(format_args!(
        "[supervisor] power: requesting {}\n",
        action_name(action)
    ));
    let error = sys_system_power(action);
    for &tid in &frozen[..froze] {
        let _ = sys_resume_cell(tid);
    }
    print_fmt(format_args!(
        "[supervisor] power: {} refused ({:?}); resumed {} cells\n",
        action_name(action),
        error,
        froze
    ));
    error
}

/// Tids the storage stack runs as: VFS, the block-driver service and every
/// live block-route provider. Returns how many were written to `keep`.
fn storage_stack(keep: &mut [usize; KEEP_MAX]) -> usize {
    let mut kept = 0;
    let mut add = |tid: usize| {
        if tid != 0 && !keep[..kept].contains(&tid) && kept < KEEP_MAX {
            keep[kept] = tid;
            kept += 1;
        }
    };
    for id in [service::VFS, service::BLOCK_DRIVER] {
        if let Some(tid) = provider_tid(id) {
            add(tid);
        }
    }
    for index in 0..api::block_route::MAX_BLOCK_ROUTES as u32 {
        match sys_lookup_block_route(index) {
            Ok(Some(route)) => {
                if route.flags & api::block_route::FLAG_PROVIDER_LIVE != 0 {
                    add(route.provider_tid as usize);
                }
            }
            _ => break,
        }
    }
    kept
}

/// Wait out the settle window; `true` when a second power-button press ended
/// it. Other messages are dropped: every sender but the storage stack is
/// frozen, and the window only gets shorter for them.
fn settle() -> bool {
    let mut buf = [0u8; 64];
    let mut waited = 0;
    while waited < SETTLE_TICKS {
        buf.fill(0);
        let result = sys_recv_timeout(0, &mut buf, SETTLE_POLL_TICKS);
        waited += SETTLE_POLL_TICKS;
        // The relay's sender id is 0, which a timeout also returns; only the
        // relay writes these bytes into the cleared buffer.
        if matches!(result, SyscallResult::Ok(0))
            && buf[..POWER_BUTTON_REQUEST.len()] == POWER_BUTTON_REQUEST
        {
            return true;
        }
    }
    false
}
