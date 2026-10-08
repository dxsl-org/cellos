//! Single-owner, bounded HTTP reactor. No operation may park this owner except the
//! timed IPC wait when there is no runnable connection work.
use alloc::{format, string::{String, ToString}, vec::Vec};
use api::dir_handles::ViDirHandle;
use api::ipc::{net_ready, NetRequest, NetResponse, VfsRequest, VfsResponse, IPC_BUF_SIZE};
use api::vfs_file_handles::ViVfsFileHandle;
use ostd::ipc::{self, IpcSubmitError};
use ostd::syscall::{sys_get_scheduler_ticks, sys_lookup_service};
use crate::{handlers::{self, Response}, net_ipc::{self, Op, RequestReadError}, router::{self, Route}};

const STATIC_FILE_MAX_BYTES: usize = 64 * 1024;
const MAX_CONNECTIONS: usize = 257; // 256 held clients and one fast probe.
const MAX_OPERATIONS: usize = 48; // 64 IPC slots, reserve capacity for cleanup/readiness.
const MAX_BACKENDS: usize = 8;
const READ_TIMEOUT: u64 = 3_000; // 30 seconds (scheduler ticks are 10ms).
const BACKEND_TIMEOUT: u64 = 6_000;
const SEND_TIMEOUT: u64 = 3_000;
const CLOSE_TIMEOUT: u64 = 1_000;
const IDLE_TICKS: u64 = 2;
const TCP_CHUNK: usize = 2048;

fn push_interest(out: &mut [u8; 256 * 5], used: &mut usize, cap: u32, mask: u8) {
    if *used == out.len() { return; }
    out[*used..*used + 4].copy_from_slice(&cap.to_le_bytes());
    out[*used + 4] = mask;
    *used += 5;
}

#[derive(Clone, Copy)]
enum NetStep { Read, Write, Close }
#[derive(Clone, Copy)]
enum PendingKind { Network(NetStep), Backend }
#[derive(Clone, Copy)]
struct Pending { op: Op, kind: PendingKind }

struct Connection {
    cap: u32,
    phase: Phase,
    pending: Option<Pending>,
    ready: u8,
    deadline: u64,
    // Buffered only while receiving. 512 bytes initially, at most 4096.
    request: Vec<u8>,
    response: Option<Response>,
    sent_header: usize,
    sent_body: usize,
    disconnected: bool,
    close_retries: u8,
}

enum Phase {
    Reading,
    File(FileSession),
    Listing(String),
    #[cfg(target_os = "none")]
    Infer(InferSession),
    Writing,
    Closing,
}

struct FileSession {
    path: String,
    components: Vec<String>,
    dir_index: usize,
    dirs: Vec<ViDirHandle>,
    file: Option<ViVfsFileHandle>,
    bytes: Vec<u8>,
    stage: FileStep,
    outcome: u16,
}
#[derive(Clone, Copy)]
enum FileStep { Preflight, Root, Dir, Open, Stat, Read, CloseFile, CloseDir, Done }

impl FileSession {
    fn new(path: &str) -> Result<Self, ()> {
        api::dir_name::validate_dir_path(path.as_bytes()).map_err(|_| ())?;
        let components = path[1..].split('/').map(|part| {
            api::dir_name::validate_dir_component(part.as_bytes()).map(|_| part.to_string())
        }).collect::<Result<Vec<_>, _>>().map_err(|_| ())?;
        if components.is_empty() { return Err(()); }
        Ok(Self { path: path.to_string(), components, dir_index: 0, dirs: Vec::new(),
            file: None, bytes: Vec::new(), stage: FileStep::Preflight, outcome: 200 })
    }
    fn cleanup(&mut self, status: u16) {
        self.outcome = status;
        self.stage = if self.file.is_some() { FileStep::CloseFile } else { FileStep::CloseDir };
    }
    fn request(&self) -> Option<VfsRequest<'_>> {
        match self.stage {
            FileStep::Preflight => Some(VfsRequest::Stat(&self.path)),
            FileStep::Root => Some(VfsRequest::OpenRootDir { path: "/" }),
            FileStep::Dir => self.dirs.last().map(|&dir| VfsRequest::OpenDir { dir, name: &self.components[self.dir_index] }),
            FileStep::Open => Some(VfsRequest::OpenFileAt {
                dir: *self.dirs.last()?,
                name: self.components.last()?.as_str(),
            }),
            FileStep::Stat => Some(VfsRequest::StatAt {
                dir: *self.dirs.last()?,
                name: self.components.last()?.as_str(),
            }),
            FileStep::Read => self.file.map(|file| VfsRequest::ReadFileHandle {
                file, offset: self.bytes.len() as u64,
                max: STATIC_FILE_MAX_BYTES.saturating_sub(self.bytes.len()).min(4000).max(1) as u32,
            }),
            FileStep::CloseFile => self.file.map(|file| VfsRequest::CloseFile { file }),
            FileStep::CloseDir => self.dirs.last().copied().map(|dir| VfsRequest::CloseDir { dir }),
            FileStep::Done => None,
        }
    }
    fn complete(&mut self, result: Option<VfsResponse<'_>>) {
        let missing = matches!(result, Some(VfsResponse::Err(1)));
        match self.stage {
            FileStep::Preflight => match result {
                Some(VfsResponse::Stat { is_dir: false, .. }) => self.stage = FileStep::Root,
                _ => self.cleanup(if missing {404} else {500}),
            },
            FileStep::Root | FileStep::Dir => match result {
                Some(VfsResponse::DirHandle(dir)) => {
                    self.dirs.push(dir);
                    if matches!(self.stage, FileStep::Dir) { self.dir_index += 1; }
                    self.stage = if self.dir_index + 1 < self.components.len() {FileStep::Dir} else {FileStep::Open};
                }
                _ => self.cleanup(if missing {404} else {500}),
            },
            FileStep::Open => match result {
                Some(VfsResponse::FileHandle(file)) => { self.file = Some(file); self.stage = FileStep::Stat; }
                _ => self.cleanup(if missing {404} else {500}),
            },
            FileStep::Stat => { self.stage = FileStep::Read; }, // stat is advisory; reads are bounded regardless.
            FileStep::Read => match result {
                Some(VfsResponse::Data(data)) => {
                    let remaining = STATIC_FILE_MAX_BYTES.saturating_sub(self.bytes.len());
                    let requested = remaining.min(4000).max(1);
                    if data.len() > remaining { self.cleanup(500); }
                    else {
                        self.bytes.extend_from_slice(data);
                        if data.len() < requested { self.cleanup(200); }
                    }
                }
                _ => self.cleanup(if missing {404} else {500}),
            },
            FileStep::CloseFile => {
                if !matches!(result, Some(VfsResponse::Ok)) { self.outcome = 500; }
                self.file = None;
                self.stage = FileStep::CloseDir;
            },
            FileStep::CloseDir => {
                if !matches!(result, Some(VfsResponse::Ok)) { self.outcome = 500; }
                self.dirs.pop();
                if self.dirs.is_empty() { self.stage = FileStep::Done; }
            },
            FileStep::Done => {},
        }
        if matches!(self.stage, FileStep::CloseDir) && self.dirs.is_empty() { self.stage = FileStep::Done; }
    }
}

#[cfg(target_os = "none")]
struct InferSession {
    prompt: String, max_tokens: u16, model: String, text: String, count: usize,
    polls: usize, id: Option<u32>, peer: Option<usize>, stage: InferStep, failure: Option<String>,
}
#[cfg(target_os = "none")]
#[derive(Clone, Copy)]
enum InferStep { Describe, Submit, Poll, Cancel, Done }
#[cfg(target_os = "none")]
impl InferSession {
    fn request(&self) -> ai_proto::AiRequest<'_> {
        use ai_proto::{AiRequest, DeviceTarget, InferSubmit};
        match self.stage {
            InferStep::Describe => AiRequest::Describe,
            InferStep::Submit => AiRequest::InferSubmit(InferSubmit {
                prompt: &self.prompt, max_tokens: self.max_tokens, temperature_milli: 0,
                top_k: 0, seed: 0, device: DeviceTarget::Auto,
            }),
            InferStep::Poll => AiRequest::InferStreamPoll { request_id: self.id.unwrap_or(0), max_tokens: 16 },
            InferStep::Cancel | InferStep::Done => AiRequest::InferCancel { request_id: self.id.unwrap_or(0) },
        }
    }
    fn complete(&mut self, result: Option<ai_proto::AiResponse<'_>>, disconnected: bool) -> Option<Response> {
        use ai_proto::AiResponse;
        match self.stage {
            InferStep::Describe => {
                if disconnected { self.stage = InferStep::Done; }
                else {
                    if let Some(AiResponse::Description(info)) = result { self.model = info.model.to_string(); }
                    self.stage = InferStep::Submit;
                }
            }
            InferStep::Submit => match result {
                Some(AiResponse::Accepted { request_id, .. }) => {
                    self.id = Some(request_id);
                    self.stage = if disconnected {InferStep::Cancel} else {InferStep::Poll};
                }
                Some(AiResponse::Failed { error, .. }) => return Some(handlers::infer_failed(&format!("{:?}", error))),
                _ => return Some(handlers::infer_failed("Transport")),
            },
            InferStep::Poll => match result {
                Some(AiResponse::TokenChunk { request_id, text, tokens, done, finish, .. })
                    if self.id == Some(request_id) && tokens.len() % 4 == 0 => {
                    self.count += tokens.len() / 4;
                    self.text.push_str(text);
                    if done {
                        self.stage = InferStep::Done;
                        return Some(handlers::infer_success(&self.model, self.prompt.len(), self.count, finish, &self.text));
                    }
                    self.polls += 1;
                    if disconnected || self.polls >= 96 { self.stage = InferStep::Cancel; }
                }
                Some(AiResponse::Failed { error, .. }) => {
                    self.failure = Some(format!("{:?}", error));
                    self.stage = InferStep::Cancel;
                }
                _ => { self.failure = Some("Transport".to_string()); self.stage = InferStep::Cancel; },
            },
            InferStep::Cancel | InferStep::Done => {
                self.stage = InferStep::Done;
                return Some(handlers::infer_failed(self.failure.as_deref().unwrap_or("poll limit exceeded")));
            }
        }
        None
    }
}

impl Connection {
    fn new(cap: u32, now: u64) -> Self {
        Self { cap, phase: Phase::Reading, pending: None, ready: 0,
            deadline: now + READ_TIMEOUT, request: Vec::with_capacity(512),
            response: None, sent_header: 0, sent_body: 0, disconnected: false, close_retries: 0 }
    }
    fn respond(&mut self, response: Response, now: u64) {
        if self.disconnected { self.phase = Phase::Closing; return; }
        self.response = Some(response);
        self.request = Vec::new(); // release request allocation before outbound payload.
        self.sent_header = 0;
        self.sent_body = 0;
        self.phase = Phase::Writing;
        self.ready = net_ready::WRITE;
        self.deadline = now + SEND_TIMEOUT;
    }
    fn interest(&self) -> u8 {
        if self.disconnected || matches!(self.phase, Phase::Closing) { return 0; }
        match self.phase {
            Phase::Reading if self.pending.is_none() => net_ready::READ,
            Phase::Writing if self.pending.is_none() => net_ready::WRITE,
            Phase::File(_) | Phase::Listing(_) => net_ready::EOF,
            #[cfg(target_os = "none")]
            Phase::Infer(_) => net_ready::EOF,
            _ => 0,
        }
    }
    fn backend(&self) -> bool { matches!(self.phase, Phase::File(_) | Phase::Listing(_))
        || { #[cfg(target_os = "none")] { matches!(self.phase, Phase::Infer(_)) }
             #[cfg(not(target_os = "none"))] { false } } }
    fn on_disconnect(&mut self) {
        self.disconnected = true;
        self.ready = 0;
        match &mut self.phase {
            Phase::File(file) if self.pending.is_none() => file.cleanup(500),
            Phase::File(_) => {},
            #[cfg(target_os = "none")]
            Phase::Infer(infer) if self.pending.is_none() && infer.id.is_some() => infer.stage = InferStep::Cancel,
            #[cfg(target_os = "none")]
            Phase::Infer(_) if self.pending.is_some() => {},
            _ => { self.phase = Phase::Closing; },
        }
    }
}

struct Reactor<'a> {
    sockets: Vec<Option<Connection>>,
    listener: u32, net: usize, vfs: usize, file: Option<&'a str>,
    accepting: Option<Op>, ready_op: Option<Op>, cursor: u16,
    outstanding: usize, next: usize, accepted_peak: usize, refused: usize,
}

pub(crate) fn listen(net: usize, port: u16) -> Option<u32> {
    let op = match net_ipc::send_net(net, &NetRequest::TcpListen { port }) {
        Ok(op) => op,
        Err(error) => {
            ostd::io::println(&alloc::format!("httpd: TcpListen submit refused: {error:?}"));
            return None;
        }
    };
    let mut buf = [0u8; IPC_BUF_SIZE];
    // Bounded: a listener that never completes must fail loudly, not park the
    // server with a silently dead port.
    for _ in 0..500 {
        if let Some(result) = net_ipc::take(op, &mut buf) {
            return match result {
                Ok(bytes) => match net_ipc::net(bytes) {
                    Some(NetResponse::CapId(cap)) => Some(cap),
                    Some(other) => {
                        ostd::io::println(&alloc::format!("httpd: TcpListen refused: {other:?}"));
                        None
                    }
                    None => {
                        ostd::io::println("httpd: TcpListen reply undecodable");
                        None
                    }
                },
                Err(status) => {
                    ostd::io::println(&alloc::format!("httpd: TcpListen terminal: {status:?}"));
                    None
                }
            };
        }
        ipc::wait(10);
    }
    ostd::io::println("httpd: TcpListen timed out");
    None
}

pub(crate) fn run(listener: u32, net: usize, vfs: usize, file: Option<&str>) -> ! {
    let mut reactor = Reactor { sockets: Vec::new(), listener, net, vfs, file,
        accepting: None, ready_op: None, cursor: 0, outstanding: 0,
        next: 0, accepted_peak: 0, refused: 0 };
    loop { reactor.turn(); }
}

impl Reactor<'_> {
    fn now() -> u64 { sys_get_scheduler_ticks().unwrap_or(0) }
    fn active(&self) -> usize { self.sockets.iter().filter(|c| c.is_some()).count() }
    fn submit_net(&mut self, request: &NetRequest<'_>) -> Result<Op, IpcSubmitError> {
        if self.outstanding >= MAX_OPERATIONS { return Err(IpcSubmitError::Busy); }
        let op = net_ipc::send_net(self.net, request)?;
        self.outstanding += 1;
        Ok(op)
    }
    fn submit_vfs(&mut self, request: &VfsRequest<'_>) -> Result<Op, IpcSubmitError> {
        if self.outstanding >= MAX_OPERATIONS { return Err(IpcSubmitError::Busy); }
        let op = net_ipc::send_vfs(self.vfs, request)?;
        self.outstanding += 1;
        Ok(op)
    }
    fn turn(&mut self) {
        let now = Self::now();
        let mut work = false;
        let mut accept_ready = false;
        if let Some(op) = self.ready_op {
            let mut buf = [0u8; IPC_BUF_SIZE];
            if let Some(result) = net_ipc::take(op, &mut buf) {
                self.ready_op = None;
                self.outstanding -= 1;
                if let Ok(raw) = result {
                    if let Some(NetResponse::TcpReady { events, next_cursor }) = net_ipc::net(raw) {
                        self.cursor = next_cursor;
                        if events.len() % 5 == 0 {
                            for event in events.chunks_exact(5) {
                                let cap = u32::from_le_bytes([event[0], event[1], event[2], event[3]]);
                                let flags = event[4];
                                if cap == self.listener { accept_ready |= flags & net_ready::ACCEPT != 0; }
                                else if let Some(conn) = self.sockets.iter_mut().flatten().find(|c| c.cap == cap) {
                                    conn.ready |= flags;
                                }
                            }
                        }
                    }
                }
            }
        }
        if accept_ready && self.accepting.is_none() && self.active() < MAX_CONNECTIONS {
            if let Ok(op) = self.submit_net(&NetRequest::TcpAccept {cap_id: self.listener}) {
                self.accepting = Some(op);
                work = true;
            }
        }
        if let Some(op) = self.accepting {
            let mut buf = [0u8; IPC_BUF_SIZE];
            if let Some(result) = net_ipc::take(op, &mut buf) {
                self.accepting = None; self.outstanding -= 1; work = true;
                if let Ok(raw) = result {
                    if let Some(NetResponse::CapId(cap)) = net_ipc::net(raw) {
                        let conn = Connection::new(cap, now);
                        if let Some(slot) = self.sockets.iter_mut().find(|s| s.is_none()) { *slot = Some(conn); }
                        else if self.sockets.len() < MAX_CONNECTIONS { self.sockets.push(Some(conn)); }
                        else { self.refused += 1; }
                        self.accepted_peak = self.accepted_peak.max(self.active());
                    }
                }
            }
        }
        let len = self.sockets.len();
        let start = self.next;
        for offset in 0..len {
            let i = (start + offset) % len;
            let Some(mut conn) = self.sockets[i].take() else { continue; };
            work |= self.advance(&mut conn, now);
            if matches!(conn.phase, Phase::Closing) && conn.pending.is_none() && conn.cap == 0 {
                // Graceful close was acknowledged: net owns FIN draining and cap reaping.
            } else { self.sockets[i] = Some(conn); }
        }
        if len > 0 { self.next = (start + 1) % len; }
        if work && self.ready_op.is_some() {
            // Readiness waits capture an immutable interest snapshot. Drain the
            // cancelled token before registering a new snapshot after state changes.
            let op = self.ready_op.unwrap();
            let _ = ipc::cancel(op);
            let mut bytes = [0u8; IPC_BUF_SIZE];
            if net_ipc::take(op, &mut bytes).is_some() {
                self.ready_op = None;
                self.outstanding -= 1;
            }
        }
        // The net wire accepts at most 256 interests. Rotate omitted readers;
        // include the listener and pending writers first so neither can starve.
        if self.ready_op.is_none() && self.outstanding < MAX_OPERATIONS {
            let mut interests = [0u8; 256 * 5];
            let mut used = 0usize;
            // The listener is armed even while an accept is in flight: a
            // snapshot that omits it cannot report the next connection, and the
            // retained wait would then hide every later client until some other
            // connection happened to become ready.
            if self.active() < MAX_CONNECTIONS {
                push_interest(&mut interests, &mut used, self.listener, net_ready::ACCEPT);
            }
            for c in self.sockets.iter().flatten() {
                if c.interest() == net_ready::WRITE {
                    push_interest(&mut interests, &mut used, c.cap, net_ready::WRITE);
                }
            }
            let priority = used / 5;
            let mut readers = 0;
            let start = self.next;
            for offset in 0..self.sockets.len() {
                let idx = (start + offset) % self.sockets.len();
                if let Some(c) = &self.sockets[idx] {
                    let interest = c.interest();
                    if interest == net_ready::READ || interest == net_ready::EOF {
                        readers += 1;
                        push_interest(&mut interests, &mut used, c.cap, interest);
                    }
                }
            }
            if used != 0 {
                let truncated = readers + priority > 256;
                if let Ok(op) = self.submit_net(&NetRequest::TcpReady {
                    interests: &interests[..used], cursor: self.cursor, wait: !work && !truncated,
                }) { self.ready_op = Some(op); }
            }
        }
        // No busy spin when all sockets are awaiting either the net cell or backends.
        if !work { ipc::wait(IDLE_TICKS); }
    }

    fn advance(&mut self, conn: &mut Connection, now: u64) -> bool {
        if now >= conn.deadline && !matches!(conn.phase, Phase::Closing) && !conn.disconnected {
            if conn.backend() { conn.on_disconnect(); }
            else if matches!(conn.phase, Phase::Reading) && conn.pending.is_none() {
                conn.respond(handlers::response(408, "text/plain", b"Request Timeout", false), now);
            } else { conn.on_disconnect(); }
            if let Some(Pending { op, kind: PendingKind::Network(_) }) = conn.pending {
                let _ = ipc::cancel(op);
            }
        }
        if conn.ready & (net_ready::EOF | net_ready::ERROR) != 0 && !conn.disconnected
            && !matches!(conn.phase, Phase::Closing) {
            conn.on_disconnect();
        }
        if let Some(pending) = conn.pending {
            let mut reply = [0u8; IPC_BUF_SIZE];
            let mut result = net_ipc::take(pending.op, &mut reply);
            if result.is_none() && now >= conn.deadline
                && matches!(pending.kind, PendingKind::Network(NetStep::Close)) {
                let _ = ipc::cancel(pending.op);
                result = net_ipc::take(pending.op, &mut reply);
            }
            if let Some(result) = result {
                conn.pending = None;
                self.outstanding -= 1;
                match pending.kind {
                    PendingKind::Network(step) => self.net_completion(conn, step, result.ok().and_then(net_ipc::net), now),
                    PendingKind::Backend => self.backend_completion(conn, result.ok(), now),
                }
                return true;
            }
            return false;
        }
        match &mut conn.phase {
            Phase::Reading if conn.ready & net_ready::READ != 0 => {
                conn.ready &= !net_ready::READ;
                let cap = conn.cap;
                if let Ok(op) = self.submit_net(&NetRequest::TcpRecvReady {cap_id: cap, buf_len: 512}) {
                    conn.pending = Some(Pending { op, kind: PendingKind::Network(NetStep::Read) });
                    return true;
                }
            }
            Phase::Writing if conn.ready & net_ready::WRITE != 0 => {
                let Some(response) = conn.response.as_ref() else { conn.phase = Phase::Closing; return true; };
                let (bytes, offset) = if conn.sent_header < response.header.len() {
                    (&response.header[..], conn.sent_header)
                } else { (&response.body[..], conn.sent_body) };
                if offset == bytes.len() { conn.phase = Phase::Closing; return true; }
                let end = bytes.len().min(offset + TCP_CHUNK);
                let cap = conn.cap;
                match self.submit_net(&NetRequest::TcpSendReady { cap_id: cap, data: &bytes[offset..end] }) {
                    Ok(op) => { conn.pending = Some(Pending {op, kind: PendingKind::Network(NetStep::Write)}); conn.ready &= !net_ready::WRITE; return true; }
                    Err(IpcSubmitError::Busy) => {},
                    Err(_) => conn.on_disconnect(),
                }
            }
            Phase::Listing(path) => {
                if self.backend_count() >= MAX_BACKENDS { return false; }
                match self.submit_vfs(&VfsRequest::ListDir(path)) {
                    Ok(op) => { conn.pending = Some(Pending {op, kind: PendingKind::Backend}); return true; }
                    Err(IpcSubmitError::Busy) => {},
                    Err(_) => conn.respond(handlers::response(500, "text/plain", b"500 Internal Server Error", false), now),
                }
            }
            Phase::File(file) => {
                let request = file.request();
                if let Some(request) = request {
                    match self.submit_vfs(&request) {
                        Ok(op) => { conn.pending = Some(Pending {op, kind: PendingKind::Backend}); return true; }
                        Err(IpcSubmitError::Busy) => {},
                        Err(IpcSubmitError::PeerGone) => {
                            conn.respond(handlers::response(503, "text/plain", b"Service Unavailable", false), now);
                            return true;
                        }
                        Err(_) => {
                            if matches!(file.stage, FileStep::CloseFile | FileStep::CloseDir) {
                                conn.respond(handlers::response(500, "text/plain", b"500 Internal Server Error", false), now);
                                return true;
                            }
                            file.cleanup(500);
                        }
                    }
                } else if matches!(file.stage, FileStep::Done) {
                    let status = file.outcome;
                    let resp = if status == 200 {
                        handlers::response_owned(200, handlers::mime_from_ext(&file.path), core::mem::take(&mut file.bytes))
                    } else if status == 404 { handlers::not_found() }
                    else { handlers::response(500, "text/plain", b"500 Internal Server Error", false) };
                    conn.respond(resp, now);
                    return true;
                }
            }
            #[cfg(target_os = "none")]
            Phase::Infer(infer) => {
                if matches!(infer.stage, InferStep::Done) { conn.phase = Phase::Closing; return true; }
                if self.backend_count() >= MAX_BACKENDS { return false; }
                if let Some(ai) = sys_lookup_service(api::syscall::service::AI) {
                    if infer.peer.is_some_and(|peer| peer != ai) {
                        conn.respond(handlers::infer_failed("ServiceNotFound"), now);
                        return true;
                    }
                    let mut bytes = [0u8; IPC_BUF_SIZE];
                    if let Ok(encoded) = api::ipc::encode(&infer.request(), &mut bytes) {
                        if self.outstanding < MAX_OPERATIONS {
                            match ipc::submit(ai, encoded) {
                                Ok(op) => {
                                    infer.peer = Some(ai);
                                    self.outstanding += 1;
                                    conn.pending = Some(Pending {op, kind: PendingKind::Backend});
                                    return true;
                                }
                                Err(IpcSubmitError::Busy) => {},
                                Err(_) => conn.respond(handlers::infer_failed("Transport"), now),
                            }
                        }
                    } else { conn.respond(handlers::infer_failed("MessageTooLarge"), now); }
                } else { conn.respond(handlers::infer_failed("ServiceNotFound"), now); }
            }
            Phase::Closing => {
                if conn.cap == 0 { return false; }
                let cap = conn.cap;
                let request = if conn.disconnected || conn.close_retries != 0 { NetRequest::TcpClose {cap_id: cap} }
                    else { NetRequest::TcpCloseGraceful {cap_id: cap} };
                match self.submit_net(&request) {
                    Ok(op) => { conn.pending = Some(Pending {op, kind: PendingKind::Network(NetStep::Close)});
                        conn.deadline = now + CLOSE_TIMEOUT; return true; },
                    Err(IpcSubmitError::Busy) => {},
                    Err(_) => { conn.cap = 0; return true; },
                }
            }
            _ => {},
        }
        false
    }

    fn net_completion(&mut self, conn: &mut Connection, step: NetStep, result: Option<NetResponse<'_>>, now: u64) {
        if conn.disconnected && !matches!(step, NetStep::Close) {
            conn.phase = Phase::Closing;
            return;
        }
        match step {
            NetStep::Read => match result {
                Some(NetResponse::Data(data)) if !data.is_empty() => {
                    let remain = net_ipc::MAX_HTTP_REQUEST_BYTES.saturating_sub(conn.request.len());
                    if data.len() > remain { conn.respond(handlers::response(413, "text/plain", b"Payload Too Large", false), now); return; }
                    conn.request.extend_from_slice(data);
                    match net_ipc::request_complete_len(&conn.request) {
                        Ok(Some(expected)) if conn.request.len() >= expected => self.dispatch(conn, now),
                        Ok(_) => {},
                        Err(RequestReadError::TooLarge) => conn.respond(handlers::response(413, "text/plain", b"Payload Too Large", false), now),
                        Err(_) => conn.respond(handlers::response(400, "text/plain", b"Bad Request", false), now),
                    }
                }
                Some(NetResponse::NotReady) => {},
                Some(NetResponse::Eof) => conn.on_disconnect(),
                _ => conn.on_disconnect(),
            },
            NetStep::Write => match result {
                Some(NetResponse::WriteProgress(n)) if n > 0 => {
                    let response = conn.response.as_ref().unwrap();
                    let remaining = if conn.sent_header < response.header.len() {
                        response.header.len() - conn.sent_header
                    } else { response.body.len() - conn.sent_body };
                    if n as usize > remaining.min(TCP_CHUNK) { conn.on_disconnect(); return; }
                    if conn.sent_header < response.header.len() {
                        conn.sent_header += n as usize;
                    } else {
                        conn.sent_body += n as usize;
                    }
                    conn.deadline = now + SEND_TIMEOUT;
                    if conn.sent_header == response.header.len() && conn.sent_body == response.body.len() {
                        conn.response = None; conn.phase = Phase::Closing;
                    } else { conn.ready |= net_ready::WRITE; }
                }
                Some(NetResponse::NotReady) => {},
                _ => conn.on_disconnect(),
            },
            NetStep::Close => match result {
                Some(NetResponse::Ok) | Some(NetResponse::Err(_)) => conn.cap = 0,
                _ => {
                    conn.close_retries = conn.close_retries.saturating_add(1);
                    conn.disconnected = true;
                    conn.deadline = now + CLOSE_TIMEOUT;
                }
            },
        }
    }

    fn dispatch(&mut self, conn: &mut Connection, now: u64) {
        let route = router::classify(&conn.request, self.file);
        let result = match route {
            Ok(Route::Index) => Some(handlers::index()),
            Ok(Route::Status) => Some(handlers::status_page()),
            Ok(Route::ApiStatus) => Some(handlers::api_status(self.active() + 1, self.accepted_peak, self.refused)),
            Ok(Route::ApiCells) => Some(handlers::api_cells()),
            Ok(Route::NotFound) => Some(handlers::not_found()),
            Ok(Route::Restart) => Some(handlers::api_restart()),
            Ok(Route::Files(path)) => {
                if self.backend_count() >= MAX_BACKENDS {
                    self.refused += 1;
                    Some(handlers::response(503, "text/plain", b"Service Unavailable", false))
                }
                else { conn.phase = Phase::Listing(path.to_string()); conn.deadline = now + BACKEND_TIMEOUT; None }
            }
            Ok(Route::File(path)) => {
                if self.backend_count() >= MAX_BACKENDS {
                    self.refused += 1;
                    Some(handlers::response(503, "text/plain", b"Service Unavailable", false))
                }
                else { match FileSession::new(path) {
                    Ok(session) => { conn.phase = Phase::File(session); conn.deadline = now + BACKEND_TIMEOUT; None }
                    Err(_) => Some(handlers::response(500, "text/plain", b"500 Internal Server Error", false)),
                } }
            }
            #[cfg(target_os = "none")]
            Ok(Route::Infer { body, max_tokens }) => {
                if self.backend_count() >= MAX_BACKENDS || self.ai_count() >= ai_proto::MAX_SESSIONS as usize {
                    self.refused += 1;
                    Some(handlers::response(503, "text/plain", b"Service Unavailable", false))
                }
                else { match handlers::infer_prompt(body) {
                    Ok(prompt) => { conn.phase = Phase::Infer(InferSession {
                        prompt: prompt.to_string(), max_tokens, model: String::new(), text: String::new(), count: 0,
                        polls: 0, id: None, peer: None, stage: InferStep::Describe, failure: None,
                    }); conn.deadline = now + BACKEND_TIMEOUT; None },
                    Err(response) => Some(response),
                } }
            }
            Err(RequestReadError::TooLarge) => Some(handlers::response(413, "text/plain", b"Payload Too Large", false)),
            _ => Some(handlers::response(400, "text/plain", b"Bad Request", false)),
        };
        if let Some(response) = result { conn.respond(response, now); }
        else { conn.request = Vec::new(); }
    }

    fn backend_count(&self) -> usize { self.sockets.iter().flatten().filter(|c| c.backend()).count() }
    #[cfg(target_os = "none")]
    fn ai_count(&self) -> usize {
        self.sockets.iter().flatten().filter(|c| matches!(c.phase, Phase::Infer(_))).count()
    }

    fn backend_completion(&mut self, conn: &mut Connection, raw: Option<&[u8]>, now: u64) {
        match &mut conn.phase {
            Phase::Listing(path) => {
                let resp = match raw.and_then(net_ipc::vfs) {
                    Some(VfsResponse::Data(data)) => handlers::api_files(path, data),
                    _ => handlers::api_files(path, &[]), // historical listing failure response.
                };
                conn.respond(resp, now);
            }
            Phase::File(file) => {
                file.complete(raw.and_then(net_ipc::vfs));
                if conn.disconnected && !matches!(file.stage, FileStep::Done) {
                    file.cleanup(500);
                }
            },
            #[cfg(target_os = "none")]
            Phase::Infer(infer) => {
                let result = raw.and_then(|bytes| api::ipc::decode::<ai_proto::AiResponse<'_>>(bytes).ok());
                if let Some(resp) = infer.complete(result, conn.disconnected) {
                    conn.respond(resp, now);
                }
            }
            _ => {},
        }
    }
}

#[cfg(test)]
mod state_tests {
    use super::{Connection, FileSession, FileStep, Phase, READ_TIMEOUT, SEND_TIMEOUT};
    use api::dir_handles::ViDirHandle;
    use api::ipc::VfsResponse;
    use api::vfs_file_handles::ViVfsFileHandle;
    use crate::handlers;

    #[test]
    fn read_deadline_is_elapsed_time_not_iteration_count() {
        let conn = Connection::new(7, 500);
        assert_eq!(conn.deadline, 500 + READ_TIMEOUT);
        assert!(matches!(conn.phase, Phase::Reading));
    }

    #[test]
    fn response_releases_request_and_restarts_send_deadline() {
        let mut conn = Connection::new(7, 500);
        conn.request.extend_from_slice(b"GET / HTTP/1.1\r\n\r\n");
        conn.respond(handlers::not_found(), 1000);
        assert!(matches!(conn.phase, Phase::Writing));
        assert!(conn.request.is_empty());
        assert_eq!(conn.deadline, 1000 + SEND_TIMEOUT);
        assert_eq!(conn.sent_header, 0);
        assert_eq!(conn.sent_body, 0);
    }
    #[test]
    fn existing_empty_file_is_not_a_missing_file() {
        let mut file = FileSession::new("/readme.txt").unwrap();
        file.stage = FileStep::Read;
        file.file = Some(ViVfsFileHandle(9));
        file.dirs.push(ViDirHandle(2));
        file.complete(Some(VfsResponse::Data(&[])));
        assert!(matches!(file.stage, FileStep::CloseFile));
        file.complete(Some(VfsResponse::Ok));
        assert!(matches!(file.stage, FileStep::CloseDir));
        file.complete(Some(VfsResponse::Ok));
        assert!(matches!(file.stage, FileStep::Done));
        assert!(file.bytes.is_empty());
        assert_eq!(file.outcome, 200);
    }

    #[test]
    fn missing_and_invalid_file_preflight_do_not_open_a_handle() {
        let mut missing = FileSession::new("/missing").unwrap();
        missing.complete(Some(VfsResponse::Err(1)));
        assert!(matches!(missing.stage, FileStep::Done));
        assert_eq!(missing.outcome, 404);
        let mut invalid = FileSession::new("/directory").unwrap();
        invalid.complete(Some(VfsResponse::Stat { size: 0, is_dir: true }));
        assert!(matches!(invalid.stage, FileStep::Done));
        assert_eq!(invalid.outcome, 500);
    }

    #[test]
    fn failed_file_read_closes_acquired_handles() {
        let mut file = FileSession::new("/readme.txt").unwrap();
        file.stage = FileStep::Read;
        file.file = Some(ViVfsFileHandle(9));
        file.dirs.push(ViDirHandle(2));
        file.complete(None);
        assert!(matches!(file.stage, FileStep::CloseFile));
        file.complete(Some(VfsResponse::Ok));
        file.complete(Some(VfsResponse::Ok));
        assert!(matches!(file.stage, FileStep::Done));
        assert_eq!(file.outcome, 500);
    }

}
