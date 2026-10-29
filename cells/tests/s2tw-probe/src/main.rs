//! Private Phase04 S2-T/S2-W fixture (`phase-04-s2tw-annex.md` §6). One binary is
//! packaged at a sandbox path (`/data/bin/s2tw-probe`, EMPTY launch ceiling) and at
//! a private `/bin/s2tw-probe` fixture row (`CapSet::LEGACY_OBSERVE_MUTATE`); the
//! output must differ only because of the path it was launched from.
//!
//! It declares `GetProcs`, `GetProcs2`, `TelemetrySnapshot`, `MemInfo` and
//! `VfsMutate`, tries each right once and writes, to its delegated stdout (the
//! shell's host stream, never the UART):
//!
//! ```text
//! S2TW getprocs <r>
//! S2TW getprocs2 <r>
//! S2TW telemetry <r>
//! S2TW meminfo <r>
//! S2TW vfs-write <r>
//! S2TW DONE
//! ```
//!
//! `<r>` is `denied` for a refusal and `other:<detail>` for anything else:
//! - telemetry: the syscall ABI encodes every refusal as the one generic error
//!   sentinel, and each call passes a valid, non-empty buffer, so an error return
//!   is the dispatch gate (`check_allowlist`) refusing it. `telemetry` is the
//!   Phase 06 S6-1 `TelemetrySnapshot` (X right, shared bit 55); an admitted
//!   snapshot must also decode, or it reads `other:malformed-<why>`;
//! - `vfs-write`: `WriteAt` of `/tmp/s2tw-probe-<tag>` through the cwd directory
//!   handle the shell delegated, answered `Err(ERR_DENIED)` by the VFS. `<tag>` is
//!   the first argument (`x` when absent). The handle route keeps the request
//!   reachable for this path-sealed child, so the only denial left is the missing
//!   attested `CALLER_FLAG_VFS_MUTATE`.
//!
//! With the first argument `trunc` it instead makes one capture with task rows requested
//! into a 192-byte buffer (room for the header and one Cell row only) and writes one line,
//! `S2TW telemetry-trunc <r>`: `other:cells-E/T-tasks-e/U-complete-C` counts the whole
//! rows `TelemetrySnapshot::parse` decoded (E Cells, e tasks) against the header totals
//! (T, U) with the `COMPLETE` flag (C = 1 or 0); `denied` or `other:<detail>` otherwise.
//! On any multi-Cell system it is the provider reporting truncation explicitly
//! (Phase 06 S6-5, D65-8 T-a).
//!
//! Exit statuses: 0 every line written; 2 missing stdout grant or bad tag; 3 stdout
//! failure. Nothing is retried. Never part of an ordinary image.

#![forbid(unsafe_code)]
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;
extern crate ostd;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use api::dir_handles::ViDirHandle;
use api::ipc::{VfsRequest, VfsResponse, IPC_BUF_SIZE};
use api::syscall::{ProcessInfo, ProcessInfoV2};
use api::telemetry::{header_flags, request, TelemetrySnapshot};
use ostd::child_context::{ChildContext, GrantSlot};
use ostd::service::VfsRef;
use ostd::syscall::{
    sys_exit, sys_get_procs, sys_get_procs2, sys_mem_info, sys_telemetry_snapshot,
    telemetry_bytes, SyscallError,
};
use terminal_sdk::stream::{Endpoint, EndpointRef, OperationRef};

api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = false
);

api::declare_syscalls![
    Send,
    Recv,
    TryRecv,
    RecvTimeout,
    Reply,
    Log,
    Exit,
    Yield,
    Heartbeat,
    LookupService,
    GetTime,
    SetTimer,
    GetProcs,
    GetProcs2,
    TelemetrySnapshot,
    MemInfo,
    VfsMutate,
];

ostd::cell_main!(cell_main);

const OK: i32 = 0;
const USAGE: i32 = 2;
const IO: i32 = 3;
/// No deadline: block until the reader takes the bytes, closes or cancels.
const FOREVER: u64 = u64::MAX;
/// The VFS wire code for `PermissionDenied` (`cells/services/vfs/src/paths.rs`).
const VFS_ERR_DENIED: u8 = 3;
/// Directory the write targets; the shell's cwd must be it or an ancestor.
const TARGET_DIR: &str = "/tmp";
const ROWS: usize = 64;
/// 16 KiB snapshot buffer: whole rows only; a fuller system reports truncation.
const TELEMETRY_WORDS: usize = 2048;
/// 192 bytes: the 96-byte header and one 96-byte Cell row (D65-8 T-a).
const TRUNC_WORDS: usize = (api::telemetry::HEADER_LEN + api::telemetry::CELL_LEN) / 8;

/// The committed endpoint a parent delegated into `slot`.
fn granted(context: &ChildContext, slot: GrantSlot) -> Option<Endpoint> {
    let operation = OperationRef::from_raw(context.operation()).ok()?;
    let grant = context.grants().find(|g| g.slot == slot && g.kind == 1)?;
    Some(Endpoint::new(EndpointRef::from_raw(grant.object).ok()?, operation))
}

/// Write every byte of `data`; the stream accepts at most 512 per call.
fn write_all(out: Endpoint, mut data: &[u8]) -> Result<(), i32> {
    while !data.is_empty() {
        match out.write(data, FOREVER) {
            Ok(count) if count > 0 && count <= data.len() => data = &data[count..],
            _ => return Err(IO),
        }
    }
    Ok(())
}

fn telemetry<E>(result: Result<usize, E>) -> String {
    match result {
        Ok(rows) => format!("other:rows-{rows}"),
        Err(_) => String::from("denied"),
    }
}

fn get_procs() -> String {
    let mut rows: Vec<ProcessInfo> = Vec::new();
    rows.resize(ROWS, ProcessInfo::default());
    telemetry(sys_get_procs(&mut rows))
}

fn get_procs2() -> String {
    let mut rows: Vec<ProcessInfoV2> = Vec::new();
    rows.resize(ROWS, ProcessInfoV2::default());
    telemetry(sys_get_procs2(&mut rows))
}

fn telemetry_snapshot() -> String {
    let mut out: Vec<u64> = Vec::new();
    out.resize(TELEMETRY_WORDS, 0);
    match sys_telemetry_snapshot(&mut out, request::TASKS) {
        Ok(written) => match TelemetrySnapshot::parse(telemetry_bytes(&out, written)) {
            Ok(snapshot) => {
                let header = snapshot.header();
                format!("other:cells-{}-tasks-{}", header.total_cells, header.total_tasks)
            }
            Err(why) => format!("other:malformed-{why:?}"),
        },
        Err(SyscallError::PermissionDenied) => String::from("denied"),
        Err(error) => format!("other:error-{error:?}"),
    }
}

/// `other:cells-E/T-tasks-e/U-complete-C` of snapshot `bytes`: the whole rows the parsed
/// view holds against the header totals, or `other:malformed-<why>`.
fn trunc_result(bytes: &[u8]) -> String {
    match TelemetrySnapshot::parse(bytes) {
        Ok(snapshot) => {
            let header = snapshot.header();
            format!(
                "other:cells-{}/{}-tasks-{}/{}-complete-{}",
                snapshot.cells().count(),
                header.total_cells,
                snapshot.tasks().count(),
                header.total_tasks,
                u8::from(header.has(header_flags::COMPLETE))
            )
        }
        Err(why) => format!("other:malformed-{why:?}"),
    }
}

/// One capture with task rows requested into a header + one Cell row buffer.
fn telemetry_trunc() -> String {
    let mut out = [0u64; TRUNC_WORDS];
    match sys_telemetry_snapshot(&mut out, request::TASKS) {
        Ok(written) => trunc_result(telemetry_bytes(&out, written)),
        Err(SyscallError::PermissionDenied) => String::from("denied"),
        Err(error) => format!("other:error-{error:?}"),
    }
}

fn mem_info() -> String {
    match sys_mem_info() {
        Ok(info) => format!("other:total-frames-{}", info.total_frames),
        Err(_) => String::from("denied"),
    }
}

/// Components below the cwd that lead to [`TARGET_DIR`], or `None` when the cwd is
/// neither the target nor one of its ancestors.
fn components_to_target(cwd: &str) -> Option<Vec<&'static str>> {
    let cwd = cwd.trim_end_matches('/');
    let rest = TARGET_DIR.strip_prefix(cwd)?;
    if !rest.is_empty() && !rest.starts_with('/') {
        return None;
    }
    Some(rest.split('/').filter(|c| !c.is_empty()).collect())
}

/// Detail for an `other:` result (never a refusal).
fn vfs_detail(response: &VfsResponse<'_>) -> String {
    match response {
        VfsResponse::Err(code) => format!("vfs-err-{code}"),
        VfsResponse::Ok => String::from("ok"),
        VfsResponse::DirHandle(handle) => format!("dir-{}", handle.0),
        _ => String::from("unexpected-response"),
    }
}

/// Create `/tmp/s2tw-probe-<tag>` through the delegated cwd handle.
fn vfs_write(context: &ChildContext, tag: &str) -> String {
    let Some(cwd_grant) = context.cwd_grant() else {
        return String::from("other:no-cwd-grant");
    };
    let Ok(cwd) = core::str::from_utf8(context.cwd()) else {
        return String::from("other:cwd-not-utf8");
    };
    let Some(components) = components_to_target(cwd) else {
        return format!("other:cwd-outside-{TARGET_DIR}");
    };
    let mut vfs = VfsRef::new();
    let mut dir = ViDirHandle(cwd_grant.id);
    for name in components {
        let mut reply = [0u8; IPC_BUF_SIZE];
        match vfs.call::<VfsRequest, VfsResponse>(&VfsRequest::OpenDir { dir, name }, &mut reply) {
            Ok(VfsResponse::DirHandle(handle)) => dir = handle,
            Ok(other) => return format!("other:opendir-{}", vfs_detail(&other)),
            Err(error) => return format!("other:opendir-ipc-{error:?}"),
        }
    }
    let name = format!("s2tw-probe-{tag}");
    let content = format!("s2tw-{tag}\n");
    let request = VfsRequest::WriteAt { dir, name: &name, content: content.as_bytes() };
    let mut reply = [0u8; IPC_BUF_SIZE];
    match vfs.call::<VfsRequest, VfsResponse>(&request, &mut reply) {
        Ok(VfsResponse::Err(VFS_ERR_DENIED)) => String::from("denied"),
        Ok(other) => format!("other:{}", vfs_detail(&other)),
        Err(error) => format!("other:ipc-{error:?}"),
    }
}

fn run() -> i32 {
    let context = match ChildContext::read_own() {
        Ok(context) => context,
        Err(_) => return USAGE,
    };
    let Some(output) = granted(&context, GrantSlot::Stdout) else {
        return USAGE;
    };
    let tag = context.argv().next().unwrap_or("x");
    if tag.is_empty() || !tag.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return USAGE;
    }
    if tag == "trunc" {
        return match write_all(output, format!("S2TW telemetry-trunc {}\n", telemetry_trunc()).as_bytes()) {
            Ok(()) => OK,
            Err(code) => code,
        };
    }
    let results = [
        ("getprocs", get_procs()),
        ("getprocs2", get_procs2()),
        ("telemetry", telemetry_snapshot()),
        ("meminfo", mem_info()),
        ("vfs-write", vfs_write(&context, tag)),
    ];
    for (right, result) in &results {
        if let Err(code) = write_all(output, format!("S2TW {right} {result}\n").as_bytes()) {
            return code;
        }
    }
    match write_all(output, b"S2TW DONE\n") {
        Ok(()) => OK,
        Err(code) => code,
    }
}

fn cell_main() {
    sys_exit(run() as u32 as usize);
}

#[cfg(test)]
mod tests {
    use super::trunc_result;
    use api::telemetry::{cell_state, header_flags, TelemetryCellV1, TelemetryHeaderV1, SCOPE_SYSTEM, TELEMETRY_VERSION};

    fn header(total_cells: u32, emitted_cells: u32, flags: u32) -> TelemetryHeaderV1 {
        TelemetryHeaderV1 {
            version: TELEMETRY_VERSION,
            flags,
            provider_epoch: 7,
            sample_id: 1,
            scope: SCOPE_SYSTEM,
            online_cpus: 1,
            total_cells,
            total_tasks: 40,
            emitted_cells,
            emitted_tasks: 0,
            ..TelemetryHeaderV1::default()
        }
    }

    fn cell() -> [u8; api::telemetry::CELL_LEN] {
        TelemetryCellV1 { cell_id: 1, generation: 3, root_tid: 9, task_count: 2, state: cell_state::LIVE, ..TelemetryCellV1::default() }
            .to_bytes()
    }

    #[test]
    fn truncated_capture_reports_whole_rows_against_totals() {
        let bytes = [&header(5, 1, 0).to_bytes()[..], &cell()[..]].concat();
        assert_eq!(trunc_result(&bytes), "other:cells-1/5-tasks-0/40-complete-0");
    }

    #[test]
    fn bytes_that_disagree_with_the_emitted_counts_are_malformed() {
        // A partial row past the one emitted Cell.
        let bytes = [&header(5, 1, 0).to_bytes()[..], &cell()[..], &[0u8; 8][..]].concat();
        assert_eq!(trunc_result(&bytes), "other:malformed-Length");
        // COMPLETE with fewer emitted than total Cells.
        let bytes = [&header(5, 1, header_flags::COMPLETE).to_bytes()[..], &cell()[..]].concat();
        assert_eq!(trunc_result(&bytes), "other:malformed-Malformed");
    }
}
