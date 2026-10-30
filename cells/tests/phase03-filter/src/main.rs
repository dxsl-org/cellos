//! Private Phase03 stdin/stdout filter fixture. Mode is `argv[0]` (the first
//! word after the program name); bytes move through the real delegated stream
//! endpoints in the child context (Stdin / Stdout grants), never through a
//! terminal session (only `tui-abandon` touches one, for its raw-mode lease).
//!
//! Modes (exact byte semantics, nothing is interpreted or re-framed):
//! - `cat`        copy stdin to stdout until end of stream, exit 0
//! - `upper`      like `cat`, ASCII a-z mapped to A-Z, every other byte unchanged
//! - `noread`     exit 0 at once without touching any endpoint
//! - `exit N`     exit with status `N` (any `i32`) without touching any endpoint
//! - `gen`        write the fixed 6 bytes `00 ff 41 fe 80 7a` (NUL, non-UTF-8, no
//!                trailing newline) to stdout, exit 0
//! - `hex`        read stdin to end of stream and print
//!                `p03-hex len=<total> hex=<first 64 bytes as lowercase hex>\n`
//! - `endless`    write 512-byte blocks until the consumer closes (exit 141) or
//!                the operation is cancelled (exit 130)
//! - `rows`       write the fixed plain JSONL `{"a":1}\n{"a":2}\n` (an external
//!                head for `from jsonl`), exit 0
//! - `typed`      write the fixed typed JSONL records `pid` 7 and `pid` 9 in the
//!                `cellbox` encoding (an external head for `from jsonl --typed`)
//! - `net-probe`  send each rights-gated `NetRequest` (Resolve, TcpConnect,
//!                UdpCreate, UdpBind, TcpListen, MulticastJoin, L2Send) once to the
//!                live net service and print one `NETPROBE <op> <result>` line per
//!                request, then `NETPROBE DONE`; `<result>` is `denied` for
//!                `Err(NET_ERR_DENIED)` and `other:<detail>` for anything else.
//!                This Cell holds no network right (its launch edge has an empty
//!                ceiling), so every line is expected to read `denied`.
//! - `config-probe get KEY` / `config-probe set KEY VALUE` /
//!   `config-probe delete KEY`
//!                send one `ConfigRequest` to the live config service and print
//!                `CFGPROBE <op> <KEY> <result>`; `<result>` is `ok`, `denied`
//!                (`Err(CONFIG_ERR_DENIED)`), `value:<v>`, `notfound` or
//!                `other:<detail>`. This Cell holds no Q right (its edge ceiling
//!                lacks `config_write`), so `set`/`delete` are expected to read
//!                `denied`; `get` is ungated (Phase 01 CP1).
//! - `tui-abandon` simulate a killed TUI: write the `vitui` enter bytes
//!                `ESC[?1049h ESC[?25l ESC[?2004h` followed by `p03-tui-abandon\n` to
//!                stdout and exit 0 WITHOUT restoring anything. As the serial session's
//!                foreground reader it first takes the raw-mode lease (best effort) and
//!                keeps it to exit; as a pipeline stage it is no reader (no session
//!                binding, stdin end of file) and the bytes alone are the observable.
//!
//! Exit statuses: 0 ok; 2 missing grant / bad argument / unknown mode; 3 other
//! stream failure; 141 stdout `BrokenPipe`; 130 `Cancelled`.
//!
//! Never part of an ordinary image: reachable only through the exact
//! `phase03-filter-profile` launch row.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;
extern crate ostd;

use alloc::format;
use alloc::string::String;
use api::cell_session::Status;
use api::ipc::{
    ConfigRequest, ConfigResponse, NetRequest, NetResponse, CONFIG_ERR_DENIED, IPC_BUF_SIZE,
    NET_ERR_DENIED,
};
use ostd::child_context::{ChildContext, GrantSlot};
use ostd::service::{ConfigRef, NetRef};
use ostd::syscall::sys_exit;
use terminal_sdk::raw_input::RawInput;
use terminal_sdk::session::TerminalSession;
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
    MemInfo,
];

ostd::cell_main!(cell_main);

const OK: i32 = 0;
const USAGE: i32 = 2;
const IO: i32 = 3;
const CANCELLED: i32 = 130;
const BROKEN_PIPE: i32 = 141;
/// No deadline: block until data, end of stream, close or cancellation.
const FOREVER: u64 = u64::MAX;
const GEN_BYTES: [u8; 6] = [0x00, 0xff, 0x41, 0xfe, 0x80, 0x7a];
const ROWS_JSONL: &[u8] = b"{\"a\":1}\n{\"a\":2}\n";
const ROWS_TYPED: &[u8] = b"{\"cellbox\":1,\"value\":[\"record\",[[\"pid\",[\"uint\",\"7\"]]]]}\n\
{\"cellbox\":1,\"value\":[\"record\",[[\"pid\",[\"uint\",\"9\"]]]]}\n";
const HEX_PREVIEW: usize = 64;
/// `vitui` enter sequence (alternate screen, cursor hidden, bracketed paste) and a
/// line of text; no restore follows.
const TUI_ABANDON: &[u8] = b"\x1b[?1049h\x1b[?25l\x1b[?2004hp03-tui-abandon\n";

/// The committed endpoint a parent delegated into `slot`.
fn granted(context: &ChildContext, slot: GrantSlot) -> Option<Endpoint> {
    let operation = OperationRef::from_raw(context.operation()).ok()?;
    let grant = context.grants().find(|g| g.slot == slot && g.kind == 1)?;
    Some(Endpoint::new(EndpointRef::from_raw(grant.object).ok()?, operation))
}

fn read_failure(status: Status) -> i32 {
    match status {
        Status::Cancelled | Status::Revoked => CANCELLED,
        _ => IO,
    }
}

fn write_failure(status: Status) -> i32 {
    match status {
        Status::BrokenPipe => BROKEN_PIPE,
        Status::Cancelled | Status::Revoked => CANCELLED,
        _ => IO,
    }
}

/// Write every byte of `data`; the stream accepts at most 512 per call.
fn write_all(out: Endpoint, mut data: &[u8]) -> Result<(), i32> {
    while !data.is_empty() {
        match out.write(data, FOREVER) {
            Ok(count) if count > 0 && count <= data.len() => data = &data[count..],
            Ok(_) => return Err(IO),
            Err(status) => return Err(write_failure(status)),
        }
    }
    Ok(())
}

/// Copy stdin to stdout, mapping each chunk with `map`, until end of stream.
fn copy(context: &ChildContext, map: fn(&mut [u8])) -> i32 {
    let (Some(input), Some(output)) = (
        granted(context, GrantSlot::Stdin),
        granted(context, GrantSlot::Stdout),
    ) else {
        return USAGE;
    };
    let mut buffer = [0u8; 512];
    loop {
        match input.read(&mut buffer, FOREVER) {
            Ok(count) => {
                map(&mut buffer[..count]);
                if let Err(code) = write_all(output, &buffer[..count]) {
                    return code;
                }
            }
            Err(Status::Eof) => return OK,
            Err(status) => return read_failure(status),
        }
    }
}

fn identity(_: &mut [u8]) {}

fn upper(bytes: &mut [u8]) {
    bytes.make_ascii_uppercase();
}

fn generate(context: &ChildContext) -> i32 {
    emit(context, &GEN_BYTES)
}

/// Write the fixed `bytes` to stdout, exit 0.
fn emit(context: &ChildContext, bytes: &[u8]) -> i32 {
    let Some(output) = granted(context, GrantSlot::Stdout) else {
        return USAGE;
    };
    match write_all(output, bytes) {
        Ok(()) => OK,
        Err(code) => code,
    }
}

/// A TUI that dies without restoring the terminal. The lease (when this Cell is the
/// session's foreground reader) is deliberately never released: the input owner's
/// foreground restoration after exit is what takes it back, as for a killed TUI.
fn tui_abandon(context: &ChildContext) -> i32 {
    if let Ok(mut session) = TerminalSession::from_own_context(context) {
        let _lease = RawInput::enter(&mut session);
    }
    emit(context, TUI_ABANDON)
}

fn hex(context: &ChildContext) -> i32 {
    let (Some(input), Some(output)) = (
        granted(context, GrantSlot::Stdin),
        granted(context, GrantSlot::Stdout),
    ) else {
        return USAGE;
    };
    let mut preview = [0u8; HEX_PREVIEW];
    let mut kept = 0usize;
    let mut total = 0u64;
    let mut buffer = [0u8; 512];
    loop {
        match input.read(&mut buffer, FOREVER) {
            Ok(count) => {
                total += count as u64;
                let take = count.min(HEX_PREVIEW - kept);
                preview[kept..kept + take].copy_from_slice(&buffer[..take]);
                kept += take;
            }
            Err(Status::Eof) => break,
            Err(status) => return read_failure(status),
        }
    }
    // "p03-hex len=" + up to 20 digits + " hex=" + 128 hex digits + "\n"
    let mut line = [0u8; 12 + 20 + 5 + 2 * HEX_PREVIEW + 1];
    let mut at = 0usize;
    let mut push = |bytes: &[u8]| {
        line[at..at + bytes.len()].copy_from_slice(bytes);
        at += bytes.len();
    };
    push(b"p03-hex len=");
    let mut digits = [0u8; 20];
    let mut start = digits.len();
    let mut value = total;
    loop {
        start -= 1;
        digits[start] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    push(&digits[start..]);
    push(b" hex=");
    for byte in &preview[..kept] {
        push(&[nibble(byte >> 4), nibble(byte & 15)]);
    }
    push(b"\n");
    match write_all(output, &line[..at]) {
        Ok(()) => OK,
        Err(code) => code,
    }
}

fn nibble(value: u8) -> u8 {
    b"0123456789abcdef"[value as usize]
}

fn endless(context: &ChildContext) -> i32 {
    let Some(output) = granted(context, GrantSlot::Stdout) else {
        return USAGE;
    };
    let block = [b'x'; 512];
    loop {
        if let Err(code) = write_all(output, &block) {
            return code;
        }
    }
}

/// One 60-byte Ethernet frame between two locally administered unicast MACs with the
/// IEEE local-experimental EtherType 0x88B5: inert on any segment if it were ever sent.
const PROBE_FRAME: [u8; 60] = {
    let mut frame = [0u8; 60];
    let header = [
        0x02, 0, 0, 0, 0, 0x01, // destination
        0x02, 0, 0, 0, 0, 0x02, // source
        0x88, 0xb5, // EtherType
    ];
    let mut i = 0;
    while i < header.len() {
        frame[i] = header[i];
        i += 1;
    }
    frame
};

/// Send `request` once to the live net service and classify the reply. Nothing is
/// retried: an admitted request is a probe failure the line reports, not an error to
/// hide (any socket it created is owner-scoped and reclaimed when this Cell exits).
fn probe(net: &mut NetRef, request: &NetRequest) -> String {
    let mut reply = [0u8; IPC_BUF_SIZE];
    match net.call::<NetRequest, NetResponse>(request, &mut reply) {
        Ok(NetResponse::Err(NET_ERR_DENIED)) => String::from("denied"),
        Ok(NetResponse::Err(code)) => format!("other:err-0x{code:02x}"),
        Ok(NetResponse::CapId(id)) => format!("other:cap-{id}"),
        Ok(NetResponse::Addr([a, b, c, d])) => format!("other:addr-{a}.{b}.{c}.{d}"),
        Ok(NetResponse::Ok) => String::from("other:ok"),
        Ok(_) => String::from("other:unexpected-response"),
        Err(error) => format!("other:ipc-{error:?}"),
    }
}

fn net_probe(context: &ChildContext) -> i32 {
    let Some(output) = granted(context, GrantSlot::Stdout) else {
        return USAGE;
    };
    let requests: [(&str, NetRequest); 7] = [
        ("resolve", NetRequest::Resolve { hostname: "10.0.2.2" }),
        ("tcp-connect", NetRequest::TcpConnect { addr: [10, 0, 2, 2], port: 9 }),
        ("udp-create", NetRequest::UdpCreate),
        ("udp-bind", NetRequest::UdpBind { cap_id: 0, port: 0 }),
        ("tcp-listen", NetRequest::TcpListen { port: 9 }),
        ("multicast-join", NetRequest::MulticastJoin { cap_id: 0, group: [239, 255, 0, 1] }),
        ("l2-send", NetRequest::L2Send { data: &PROBE_FRAME }),
    ];
    let mut net = NetRef::new();
    for (op, request) in &requests {
        let line = format!("NETPROBE {op} {}\n", probe(&mut net, request));
        if let Err(code) = write_all(output, line.as_bytes()) {
            return code;
        }
    }
    match write_all(output, b"NETPROBE DONE\n") {
        Ok(()) => OK,
        Err(code) => code,
    }
}

fn config_probe(context: &ChildContext) -> i32 {
    let Some(output) = granted(context, GrantSlot::Stdout) else {
        return USAGE;
    };
    let mut args = context.argv().skip(1);
    let (op, key) = match (args.next(), args.next()) {
        (Some(op), Some(key)) => (op, key),
        _ => return USAGE,
    };
    let value = args.next();
    let request = match (op, value) {
        ("get", None) => ConfigRequest::Get(key),
        ("set", Some(value)) => ConfigRequest::Set { key, value },
        ("delete", None) => ConfigRequest::Delete(key),
        _ => return USAGE,
    };
    let mut reply = [0u8; IPC_BUF_SIZE];
    let result = match ConfigRef::new().call::<ConfigRequest, ConfigResponse>(&request, &mut reply) {
        Ok(ConfigResponse::Ok) => String::from("ok"),
        Ok(ConfigResponse::Err(CONFIG_ERR_DENIED)) => String::from("denied"),
        Ok(ConfigResponse::Value(v)) => format!("value:{v}"),
        Ok(ConfigResponse::NotFound) => String::from("notfound"),
        Ok(ConfigResponse::Err(code)) => format!("other:err-0x{code:02x}"),
        Ok(ConfigResponse::Keys(_)) => String::from("other:keys"),
        Err(error) => format!("other:ipc-{error:?}"),
    };
    match write_all(output, format!("CFGPROBE {op} {key} {result}\n").as_bytes()) {
        Ok(()) => OK,
        Err(code) => code,
    }
}

fn run() -> i32 {
    let context = match ChildContext::read_own() {
        Ok(context) => context,
        Err(_) => return USAGE,
    };
    let mut args = context.argv();
    match args.next().unwrap_or("") {
        "cat" => copy(&context, identity),
        "upper" => copy(&context, upper),
        "noread" => OK,
        "exit" => match args.next().and_then(|n| n.parse::<i32>().ok()) {
            Some(code) => code,
            None => USAGE,
        },
        "gen" => generate(&context),
        "rows" => emit(&context, ROWS_JSONL),
        "typed" => emit(&context, ROWS_TYPED),
        "hex" => hex(&context),
        "endless" => endless(&context),
        "net-probe" => net_probe(&context),
        "config-probe" => config_probe(&context),
        "tui-abandon" => tui_abandon(&context),
        _ => USAGE,
    }
}

fn cell_main() {
    sys_exit(run() as u32 as usize);
}
