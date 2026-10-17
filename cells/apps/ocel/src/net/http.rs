// SPDX-License-Identifier: MIT
//! Bounded, cooperative HTTP/Gemini transport. Every IPC is an exact operation.
extern crate alloc;
use alloc::{format, string::{String, ToString}, vec::Vec};
use api::ipc::{NetRequest, NetResponse, IPC_BUF_SIZE};
use core::task::Poll;
use ostd::ipc::{self, IpcOpId, IpcSubmitError, IpcTakeResult, IpcTerminal};
use ostd::syscall::{sys_get_time_ms, sys_lookup_service};

const HEADER_LIMIT: usize = 16 * 1024;
const URL_LIMIT: usize = 2048;
const LOAD_TICKS: u64 = 45_000; // Normalized monotonic milliseconds, never CPU-loop counts.
const CLOSE_TICKS: u64 = 3_000;
const REDIRECT_LIMIT: usize = 5;

#[derive(Clone, Debug)]
pub struct Url { pub full: String, pub scheme: String, pub host: String, pub port: u16, pub path: String, authority: String, fragment: Option<String> }
impl Url {
    pub fn parse(input: &str) -> Result<Self, String> {
        if input.len() > URL_LIMIT || input.bytes().any(|b| b <= 32 || b == 127) { return Err("Invalid or overlong network URL".into()); }
        let fragment = input.split_once('#').map(|(_, fragment)| String::from(fragment));
        let input = input.split('#').next().unwrap_or(input);
        let (scheme, rest) = input.split_once("://").ok_or("Missing URL scheme")?;
        let default = match scheme { "http" => 80, "https" => 443, "gemini" => 1965, _ => return Err("Unsupported network scheme".into()) };
        let split = rest.find(|c| c == '/' || c == '?').unwrap_or(rest.len());
        let authority = &rest[..split];
        let (host, port) = match authority.split_once(':') {
            Some((host, port)) => (host, port.parse::<u16>().map_err(|_| "Invalid URL port")?), None => (authority, default),
        };
        if port == 0 || host.is_empty() || host.len() > 253 || !host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.') {
            return Err("Invalid hostname (IPv4/DNS names only; no credentials or IPv6)".into());
        }
        let path = match &rest[split..] { "" => String::from("/"), p if p.starts_with('?') => format!("/{}", p), p => String::from(p) };
        let host = host.to_ascii_lowercase();
        let authority = if port == default { host.clone() } else { format!("{}:{}", host, port) };
        Ok(Self { full: format!("{}://{}{}", scheme, authority, path), scheme: scheme.into(), host, port, path, authority, fragment })
    }
    fn tls(&self) -> bool { self.scheme != "http" }
    pub fn origin(&self) -> String { format!("{}://{}", self.scheme, self.authority) }
    fn document_url(&self) -> String { match &self.fragment { Some(fragment) => format!("{}#{}", self.full, fragment), None => self.full.clone() } }
    pub fn redirect(&self, location: &str) -> Result<Self, String> {
        let target = if location.contains("://") { String::from(location) }
            else if location.starts_with("//") { format!("{}:{}", self.scheme, location) }
            else {
                let mut target = crate::resources::resolve(&self.full, location)?;
                if let Some((_, fragment)) = location.split_once('#') { target.push('#'); target.push_str(fragment); }
                target
            };
        let mut next = Self::parse(&target)?;
        if !location.contains('#') { next.fragment = self.fragment.clone(); }
        if self.scheme == "https" && next.scheme != "https" || self.scheme == "gemini" && next.scheme != "gemini" || self.scheme != "gemini" && next.scheme == "gemini" {
            return Err("Unsafe redirect protocol change".into());
        }
        Ok(next)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Framing { Headers, Fixed(usize), Eof, ChunkSize, ChunkData(usize), ChunkCrlf, Trailers, Complete }
/// Incremental decoder: never treats a malformed or short response as success.
pub struct ResponseDecoder {
    framing: Framing, pending: Vec<u8>, pub body: Vec<u8>, limit: usize,
    pub status: u16, pub location: Option<String>, pub media_type: String,
    gemini: bool, header_bytes: usize, interim: usize,
}
impl ResponseDecoder {
    pub fn new(limit: usize, gemini: bool) -> Self { Self { framing: Framing::Headers, pending: Vec::new(), body: Vec::new(), limit, status: 0, location: None, media_type: String::new(), gemini, header_bytes: 0, interim: 0 } }
    fn append_pending(&mut self, count: usize) -> Result<(), String> {
        if count > self.limit.saturating_sub(self.body.len()) { return Err("Response exceeds document/resource byte budget".into()); }
        let needed = self.body.len() + count;
        if needed > self.body.capacity() {
            let capacity = needed.next_power_of_two().min(self.limit);
            self.body.reserve_exact(capacity - self.body.len());
        }
        self.body.extend_from_slice(&self.pending[..count]);
        self.pending.drain(..count);
        Ok(())
    }
    pub fn complete(&self) -> bool { self.framing == Framing::Complete }
    pub fn feed(&mut self, bytes: &[u8]) -> Result<(), String> {
        // Each service reply is <=4096; incomplete framing lines are separately bounded.
        if self.pending.len().saturating_add(bytes.len()) > HEADER_LIMIT + 4096 { return Err("Response framing exceeds header budget".into()); }
        let needed = self.pending.len() + bytes.len();
        if needed > self.pending.capacity() {
            let capacity = needed.next_power_of_two().min(HEADER_LIMIT + 4096);
            self.pending.reserve_exact(capacity - self.pending.len());
        }
        self.pending.extend_from_slice(bytes);
        loop {
            match self.framing {
                Framing::Headers => {
                    let delimiter: &[u8] = if self.gemini { b"\r\n" } else { b"\r\n\r\n" };
                    let Some(end) = self.pending.windows(delimiter.len()).position(|w| w == delimiter) else {
                        if self.pending.len() > if self.gemini { 1024 } else { HEADER_LIMIT } { return Err("Response headers too large".into()); }
                        return Ok(());
                    };
                    if end + delimiter.len() > if self.gemini { 1024 } else { HEADER_LIMIT } { return Err("Response headers too large".into()); }
                    let header = core::str::from_utf8(&self.pending[..end]).map_err(|_| "Non-UTF8 protocol header")?.to_string();
                    self.pending.drain(..end + delimiter.len());
                    self.headers(&header)?;
                }
                Framing::Fixed(left) => {
                    let count = left.min(self.pending.len());
                    self.append_pending(count)?;
                    self.framing = if left == count { Framing::Complete } else { Framing::Fixed(left - count) };
                    if left != count { return Ok(()); }
                }
                Framing::Eof => { self.append_pending(self.pending.len())?; return Ok(()); }
                Framing::ChunkSize => {
                    let Some(end) = self.pending.windows(2).position(|w| w == b"\r\n") else {
                        if self.pending.len() > 1024 { return Err("Chunk size line too large".into()); } return Ok(());
                    };
                    if end > 1024 { return Err("Chunk size line too large".into()); }
                    let line = core::str::from_utf8(&self.pending[..end]).map_err(|_| "Invalid chunk size")?;
                    let size = line.split(';').next().unwrap_or("");
                    if size.is_empty() || !size.bytes().all(|b| b.is_ascii_hexdigit()) { return Err("Invalid chunk size".into()); }
                    let count = usize::from_str_radix(size, 16).map_err(|_| "Chunk size overflow")?;
                    if count > self.limit.saturating_sub(self.body.len()) { return Err("Chunk exceeds body budget".into()); }
                    self.pending.drain(..end + 2);
                    self.framing = if count == 0 { Framing::Trailers } else { Framing::ChunkData(count) };
                }
                Framing::ChunkData(left) => {
                    let count = left.min(self.pending.len());
                    self.append_pending(count)?;
                    self.framing = if left == count { Framing::ChunkCrlf } else { Framing::ChunkData(left - count) };
                    if left != count { return Ok(()); }
                }
                Framing::ChunkCrlf => {
                    if self.pending.len() < 2 { return Ok(()); }
                    if &self.pending[..2] != b"\r\n" { return Err("Missing chunk delimiter".into()); }
                    self.pending.drain(..2); self.framing = Framing::ChunkSize;
                }
                Framing::Trailers => {
                    let Some(end) = self.pending.windows(2).position(|w| w == b"\r\n") else {
                        if self.header_bytes.saturating_add(self.pending.len()) > HEADER_LIMIT { return Err("Chunk trailers too large".into()); }
                        return Ok(());
                    };
                    self.header_bytes += end + 2;
                    if self.header_bytes > HEADER_LIMIT { return Err("Chunk trailers too large".into()); }
                    if end != 0 {
                        let line = core::str::from_utf8(&self.pending[..end]).map_err(|_| "Invalid trailer")?;
                        if !line.contains(':') || line.starts_with(' ') || line.starts_with('\t') { return Err("Malformed chunk trailer".into()); }
                    }
                    self.pending.drain(..end + 2);
                    if end == 0 { self.framing = Framing::Complete; }
                }
                Framing::Complete => { if !self.pending.is_empty() { return Err("Unexpected bytes after framed response".into()); } return Ok(()); }
            }
        }
    }
    fn headers(&mut self, header: &str) -> Result<(), String> {
        if self.gemini {
            let bytes = header.as_bytes();
            if bytes.len() < 3 || bytes[2] != b' ' || !bytes[..2].iter().all(u8::is_ascii_digit) { return Err("Malformed Gemini status".into()); }
            self.status = ((bytes[0] - b'0') as u16) * 10 + (bytes[1] - b'0') as u16;
            let meta = &header[3..];
            if (30..40).contains(&self.status) { self.location = Some(meta.into()); self.pending.clear(); self.framing = Framing::Complete; }
            else if (20..30).contains(&self.status) { self.media_type = meta.into(); self.framing = Framing::Eof; }
            else { return Err(format!("Gemini status {}: {}", self.status, meta)); }
            return Ok(());
        }
        let mut lines = header.split("\r\n");
        let status = lines.next().ok_or("Missing HTTP status")?;
        let mut fields = status.splitn(3, ' ');
        if !matches!(fields.next(), Some("HTTP/1.0" | "HTTP/1.1")) { return Err("Unsupported/malformed HTTP version".into()); }
        let code = fields.next().ok_or("Missing HTTP status code")?;
        if code.len() != 3 || !code.bytes().all(|b| b.is_ascii_digit()) { return Err("Malformed HTTP status code".into()); }
        self.status = code.parse().map_err(|_| "Invalid HTTP status")?;
        let mut length: Option<usize> = None; let mut transfer = false;
        for line in lines {
            if line.starts_with(' ') || line.starts_with('\t') { return Err("Folded HTTP headers unsupported".into()); }
            let (name, value) = line.split_once(':').ok_or("Malformed HTTP header")?;
            if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)) || value.bytes().any(|b| b < 32 && b != 9 || b == 127) { return Err("Invalid HTTP header".into()); }
            let value = value.trim();
            if name.eq_ignore_ascii_case("content-length") {
                if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) { return Err("Invalid Content-Length".into()); }
                let n = value.parse::<usize>().map_err(|_| "Content-Length overflow")?;
                if length.is_some_and(|old| old != n) { return Err("Conflicting Content-Length".into()); } length = Some(n);
            } else if name.eq_ignore_ascii_case("transfer-encoding") {
                if transfer || !value.eq_ignore_ascii_case("chunked") { return Err("Unsupported Transfer-Encoding".into()); } transfer = true;
            } else if name.eq_ignore_ascii_case("location") {
                if self.location.is_some() || value.is_empty() { return Err("Invalid duplicate/empty redirect location".into()); } self.location = Some(value.into());
            } else if name.eq_ignore_ascii_case("content-type") { self.media_type = value.into(); }
            else if name.eq_ignore_ascii_case("content-encoding") && !value.eq_ignore_ascii_case("identity") { return Err("Compressed HTTP content unsupported".into()); }
        }
        if transfer && length.is_some() { return Err("Ambiguous HTTP framing".into()); }
        if (100..200).contains(&self.status) {
            self.interim += 1; if self.interim > 4 || self.status == 101 { return Err("Unsupported HTTP interim response".into()); }
            self.location = None; self.media_type.clear(); self.framing = Framing::Headers; return Ok(());
        }
        if matches!(self.status, 301 | 302 | 303 | 307 | 308) {
            if self.location.is_none() { return Err("Redirect missing Location".into()); }
            self.pending.clear(); self.framing = Framing::Complete; return Ok(());
        }
        if !(200..300).contains(&self.status) { return Err(format!("HTTP status {}", self.status)); }
        self.location = None; // Location on a successful response is not a redirect.
        if length.is_some_and(|n| n > self.limit) { return Err("Content-Length exceeds body budget".into()); }
        self.framing = if self.status == 204 || self.status == 205 { Framing::Complete }
            else if transfer { Framing::ChunkSize } else if let Some(n) = length { if n == 0 { Framing::Complete } else { Framing::Fixed(n) } } else { Framing::Eof };
        Ok(())
    }
    pub fn eof(&mut self) -> Result<(), String> {
        if self.framing == Framing::Eof { self.framing = Framing::Complete; Ok(()) }
        else if self.complete() { Ok(()) } else { Err("Truncated protocol response before EOF".into()) }
    }
}

/// Narrow exact-operation transport, monomorphized in production and scripted in
/// regressions so timeouts/cancellation never depend on live network timing.
pub trait Transport {
    fn now(&self) -> u64;
    fn endpoint(&mut self) -> Option<usize>;
    fn submit(&mut self, endpoint: usize, bytes: &[u8]) -> Result<IpcOpId, IpcSubmitError>;
    fn take(&mut self, operation: IpcOpId, reply: &mut [u8]) -> Result<IpcTakeResult, ipc::IpcError>;
    fn cancel(&mut self, operation: IpcOpId);
    fn close(&mut self, endpoint: usize, cap: u64, tls: bool);
    fn cleanup(&mut self);
    fn cleanup_busy(&self) -> bool { false }
    fn cleanup_error(&self) -> Option<&'static str> { None }
}
pub struct KernelTransport { admitted: bool }
static ACTIVE_JOBS: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
impl KernelTransport { fn new() -> Self { Self { admitted: false } } }
impl Drop for KernelTransport {
    fn drop(&mut self) { if self.admitted { ACTIVE_JOBS.fetch_sub(1, core::sync::atomic::Ordering::Relaxed); } }
}
impl Transport for KernelTransport {
    fn now(&self) -> u64 { sys_get_time_ms().unwrap_or(u64::MAX) }
    fn endpoint(&mut self) -> Option<usize> {
        sys_get_time_ms()?;
        poll_cleanup();
        if closing_count() >= 16 { return None; }
        let endpoint = sys_lookup_service(api::syscall::service::NET)?;
        if ACTIVE_JOBS.fetch_update(core::sync::atomic::Ordering::Relaxed, core::sync::atomic::Ordering::Relaxed,
            |count| (count < crate::budget::MAX_TABS).then_some(count + 1)).is_err() { return None; }
        self.admitted = true;
        Some(endpoint)
    }
    fn submit(&mut self, endpoint: usize, bytes: &[u8]) -> Result<IpcOpId, IpcSubmitError> { ipc::submit(endpoint, bytes) }
    fn take(&mut self, operation: IpcOpId, reply: &mut [u8]) -> Result<IpcTakeResult, ipc::IpcError> { ipc::take(operation, reply) }
    fn cancel(&mut self, operation: IpcOpId) { let _ = ipc::cancel(operation); }
    fn close(&mut self, endpoint: usize, cap: u64, tls: bool) { enqueue_close(endpoint, cap, tls); }
    fn cleanup(&mut self) { poll_cleanup(); }
    fn cleanup_busy(&self) -> bool { closing_count() >= 16 }
    fn cleanup_error(&self) -> Option<&'static str> { cleanup_error() }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage { Resolve, Connect, WaitConnect, Send, Read, Done }
#[derive(Debug)]
pub struct FetchResult { pub url: String, pub body: Vec<u8>, pub media_type: String, pub gemini: bool }
pub struct FetchJob<T: Transport = KernelTransport> {
    url: Url, endpoint: usize, deadline: u64, stage: Stage, pending: Option<IpcOpId>, cap: Option<u64>,
    request: Vec<u8>, sent: usize, sent_chunk: usize, decoder: ResponseDecoder, visited: Vec<String>,
    same_origin: Option<String>,
    address: Option<[u8; 4]>,
    io: T,
}
impl FetchJob<KernelTransport> {
    pub fn new(url: &str, limit: usize) -> Result<Self, String> {
        recover_unaccepted_cleanup();
        Self::with_transport(url, limit, None, KernelTransport::new())
    }
    pub fn same_origin(url: &str, limit: usize, origin: &str) -> Result<Self, String> { Self::with_transport(url, limit, Some(origin.into()), KernelTransport::new()) }
}
impl<T: Transport> FetchJob<T> {
    fn with_transport(url: &str, limit: usize, same_origin: Option<String>, mut io: T) -> Result<Self, String> {
        let url = Url::parse(url)?;
        if same_origin.as_ref().is_some_and(|origin| *origin != url.origin()) { return Err("Cross-origin resource request blocked".into()); }
        let endpoint = match io.endpoint() {
            Some(endpoint) => endpoint,
            None => return Err(io.cleanup_error().unwrap_or("Network service unavailable or cleanup queue busy; retry navigation after cleanup drains").into()),
        };
        let mut job = Self { decoder: ResponseDecoder::new(limit, url.scheme == "gemini"), visited: alloc::vec![url.full.clone()], url, endpoint, deadline: io.now().saturating_add(LOAD_TICKS), stage: Stage::Resolve, pending: None, cap: None, request: Vec::new(), sent: 0, sent_chunk: 0, same_origin, address: None, io };
        job.make_request(); Ok(job)
    }
    fn make_request(&mut self) {
        self.request = if self.url.scheme == "gemini" { format!("{}\r\n", self.url.full).into_bytes() }
            else { format!("GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: Ocel/0.2 (CellOS)\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n", self.url.path, self.url.authority).into_bytes() };
        self.sent = 0;
    }
    pub fn progress(&self) -> &'static str { match self.stage { Stage::Resolve => "Resolving host", Stage::Connect | Stage::WaitConnect => "Connecting", Stage::Send => "Sending request", Stage::Read => "Receiving document", Stage::Done => "Finished" } }
    pub fn resident_bytes(&self) -> usize {
        self.request.capacity() + self.decoder.body.capacity() + self.decoder.pending.capacity()
            + self.visited.iter().map(String::capacity).sum::<usize>()
            + self.url.full.capacity() + self.url.host.capacity() + self.url.path.capacity() + 4096
    }
    pub fn set_memory_budget(&mut self, bytes: usize) {
        let overhead = self.resident_bytes().saturating_sub(self.decoder.body.capacity());
        self.decoder.limit = self.decoder.limit.min(bytes.saturating_sub(overhead + HEADER_LIMIT + 4096));
    }
    fn request_bytes(&mut self, addr: Option<[u8; 4]>, buf: &mut [u8; IPC_BUF_SIZE]) -> Result<usize, String> {
        let cap = self.cap.unwrap_or(0);
        if self.stage == Stage::Connect && self.url.tls() {
            buf[0] = 0x30;
            buf[9..13].copy_from_slice(&addr.ok_or("Missing resolved address")?);
            buf[13..15].copy_from_slice(&self.url.port.to_le_bytes()); buf[15..17].copy_from_slice(&(self.url.host.len() as u16).to_le_bytes()); buf[17..17 + self.url.host.len()].copy_from_slice(self.url.host.as_bytes());
            return Ok(17 + self.url.host.len());
        }
        if self.url.tls() && self.stage == Stage::Send {
            self.sent_chunk = (self.request.len() - self.sent).min(501);
            buf[0] = 0x31; buf[1..9].copy_from_slice(&cap.to_le_bytes()); buf[9..11].copy_from_slice(&(self.sent_chunk as u16).to_le_bytes()); buf[11..11 + self.sent_chunk].copy_from_slice(&self.request[self.sent..self.sent + self.sent_chunk]);
            return Ok(11 + self.sent_chunk);
        }
        if self.url.tls() && self.stage == Stage::Read {
            buf[0] = 0x32; buf[1..9].copy_from_slice(&cap.to_le_bytes()); buf[9..13].copy_from_slice(&4094u32.to_le_bytes()); return Ok(13);
        }
        let req = match self.stage {
            Stage::Resolve => NetRequest::Resolve { hostname: &self.url.host },
            Stage::Connect => NetRequest::TcpConnect { addr: addr.ok_or("Missing address")?, port: self.url.port },
            Stage::WaitConnect => NetRequest::SocketState { cap_id: cap as u32 },
            Stage::Send => { self.sent_chunk = (self.request.len() - self.sent).min(480); NetRequest::TcpSendReady { cap_id: cap as u32, data: &self.request[self.sent..self.sent + self.sent_chunk] } },
            Stage::Read => NetRequest::TcpRecvReady { cap_id: cap as u32, buf_len: 500 },
            Stage::Done => return Err("Load already finished".into()),
        };
        api::ipc::encode(&req, buf).map(|b| b.len()).map_err(|_| "Cannot encode network request".into())
    }
    fn submit(&mut self) -> Result<(), String> {
        let mut bytes = [0u8; IPC_BUF_SIZE];
        let len = self.request_bytes(self.address, &mut bytes)?;
        match self.io.submit(self.endpoint, &bytes[..len]) {
            Ok(operation) => self.pending = Some(operation),
            Err(IpcSubmitError::Busy) => {},
            Err(error) => return Err(format!("Network IPC admission failed: {:?}", error)),
        }
        Ok(())
    }
    pub fn poll(&mut self) -> Poll<Result<FetchResult, String>> {
        self.io.cleanup();
        if self.stage == Stage::Done { return Poll::Pending; }
        if let Some(error) = self.io.cleanup_error() { return self.fail(error.into()); }
        if self.io.now() >= self.deadline { return self.fail("Network load deadline exceeded".into()); }
        if self.stage == Stage::Resolve && self.pending.is_none() && self.io.cleanup_busy() { return Poll::Pending; }
        if self.pending.is_none() { if let Err(e) = self.submit() { return self.fail(e); } return Poll::Pending; }
        let mut reply = [0u8; IPC_BUF_SIZE];
        let op = self.pending.unwrap();
        let len = match self.io.take(op, &mut reply) {
            Ok(IpcTakeResult::Pending) => return Poll::Pending,
            Ok(IpcTakeResult::Terminal { status: IpcTerminal::Reply, len }) => { self.pending = None; len },
            Ok(IpcTakeResult::Terminal { status, .. }) => { self.pending = None; return self.fail(format!("Network IPC terminated: {:?}", status)); },
            Err(e) => return self.fail(format!("Network IPC take failed: {:?}", e)),
        };
        if let Err(e) = self.response(&reply[..len]) { return self.fail(e); }
        if self.decoder.complete() {
            if let Some(location) = self.decoder.location.take() {
                if self.visited.len() > REDIRECT_LIMIT { return self.fail("Too many redirects".into()); }
                let next = match self.url.redirect(&location) { Ok(next) => next, Err(e) => return self.fail(e) };
                if self.same_origin.as_ref().is_some_and(|origin| *origin != next.origin()) { return self.fail("Cross-origin resource redirect blocked".into()); }
                if self.visited.contains(&next.full) { return self.fail("Redirect loop".into()); }
                self.close(); self.visited.push(next.full.clone()); self.url = next;
                self.decoder = ResponseDecoder::new(self.decoder.limit, self.url.scheme == "gemini"); self.make_request(); self.stage = Stage::Resolve;
                return Poll::Pending;
            }
            self.close(); self.stage = Stage::Done;
            return Poll::Ready(Ok(FetchResult { url: self.url.document_url(), body: core::mem::take(&mut self.decoder.body), media_type: core::mem::take(&mut self.decoder.media_type), gemini: self.url.scheme == "gemini" }));
        }
        Poll::Pending
    }
    fn response(&mut self, bytes: &[u8]) -> Result<(), String> {
        if self.stage == Stage::Resolve {
            let addr = match api::ipc::decode::<NetResponse>(bytes) { Ok(NetResponse::Addr(addr)) => addr, _ => return Err(format!("DNS resolution failed for {}", self.url.host)) };
            self.address = Some(addr); self.stage = Stage::Connect; return Ok(());
        }
        if self.stage == Stage::Connect {
            let cap = if self.url.tls() {
                u64::from_le_bytes(bytes.try_into().map_err(|_| "Malformed TLS connect reply")?)
            } else { match api::ipc::decode::<NetResponse>(bytes) { Ok(NetResponse::CapId(cap)) => cap as u64, _ => return Err("TCP connect failed".into()) } };
            if cap == 0 { return Err("TLS connection failed (network/certificate/time policy)".into()); }
            self.cap = Some(cap); self.stage = if self.url.tls() { Stage::Send } else { Stage::WaitConnect }; return Ok(());
        }
        if self.stage == Stage::WaitConnect {
            match api::ipc::decode::<NetResponse>(bytes) {
                Ok(NetResponse::State(0x03)) => self.stage = Stage::Send,
                Ok(NetResponse::State(0x01 | 0x02)) | Ok(NetResponse::NotReady) => {},
                _ => return Err("TCP connection failed before establishment".into()),
            }
            return Ok(());
        }
        if self.stage == Stage::Send {
            let count = if self.url.tls() { u32::from_le_bytes(bytes.try_into().map_err(|_| "Malformed TLS send reply")?) as usize }
                else { match api::ipc::decode::<NetResponse>(bytes) { Ok(NetResponse::WriteProgress(n)) => n as usize, Ok(NetResponse::NotReady) => return Ok(()), _ => return Err("TCP write failed".into()) } };
            if count > self.sent_chunk || count == 0 { return Err("Invalid/failed network write progress".into()); }
            self.sent += count; if self.sent == self.request.len() { self.stage = Stage::Read; } return Ok(());
        }
        if self.url.tls() {
            if bytes.len() < 2 { return Err("Malformed TLS read reply".into()); }
            let count = u16::from_le_bytes([bytes[0], bytes[1]]) as usize;
            if count == u16::MAX as usize { return Err("TLS read failed".into()); }
            if count == u16::MAX as usize - 1 { return Ok(()); }
            if bytes.len() != count + 2 { return Err("Truncated TLS read IPC reply".into()); }
            if count == 0 { self.decoder.eof() } else { self.decoder.feed(&bytes[2..]) }
        } else {
            match api::ipc::decode::<NetResponse>(bytes) {
                Ok(NetResponse::Data(data)) if !data.is_empty() => self.decoder.feed(data),
                Ok(NetResponse::Eof) => self.decoder.eof(), Ok(NetResponse::NotReady) => Ok(()),
                _ => Err("TCP read failed (not EOF)".into()),
            }
        }
    }
    fn fail(&mut self, error: String) -> Poll<Result<FetchResult, String>> { self.cancel(); Poll::Ready(Err(error)) }
    fn close(&mut self) { if let Some(cap) = self.cap.take() { self.io.close(self.endpoint, cap, self.url.tls()); } }
    pub fn cancel(&mut self) {
        if let Some(op) = self.pending.take() {
            self.io.cancel(op); let mut buf = [0u8; IPC_BUF_SIZE];
            // A connect reply may have won the cancellation race: reclaim its cap.
            if let Ok(IpcTakeResult::Terminal { status: IpcTerminal::Reply, len }) = self.io.take(op, &mut buf) {
                if self.stage == Stage::Connect && self.cap.is_none() {
                    let cap = if self.url.tls() { buf[..len].try_into().ok().map(u64::from_le_bytes) }
                        else { match api::ipc::decode::<NetResponse>(&buf[..len]) { Ok(NetResponse::CapId(cap)) => Some(cap as u64), _ => None } };
                    self.cap = cap.filter(|cap| *cap != 0);
                }
            }
        }
        self.close(); self.stage = Stage::Done;
    }
}
impl<T: Transport> Drop for FetchJob<T> { fn drop(&mut self) { self.cancel(); } }

struct Closing {
    endpoint: usize, cap: u64, tls: bool, op: Option<IpcOpId>, deadline: u64,
    // true: no request was accepted, safe explicit navigation recovery.
    // false: terminal outcome uncertain; no accepted close is retried.
    quarantine: Option<bool>,
}
const CLOSE_SLOTS: usize = 16 + crate::budget::MAX_TABS;
struct CloseQueue { slots: [Option<Closing>; CLOSE_SLOTS], cursor: usize }
static CLOSING: ostd::sync::Mutex<CloseQueue> = ostd::sync::Mutex::new(CloseQueue { slots: [const { None }; CLOSE_SLOTS], cursor: 0 });
fn closing_count() -> usize { CLOSING.lock().slots.iter().filter(|slot| slot.is_some()).count() }
fn cleanup_error() -> Option<&'static str> {
    let queue = CLOSING.lock();
    if queue.slots.iter().flatten().any(|job| job.quarantine == Some(false)) {
        Some("Network socket cleanup outcome is uncertain; restart Ocel before further network navigation")
    } else if queue.slots.iter().flatten().any(|job| job.quarantine == Some(true)) {
        Some("Network socket cleanup admission timed out; retry navigation to recover retained sockets")
    } else { None }
}
fn recover_unaccepted_cleanup() {
    let now = sys_get_time_ms().unwrap_or(u64::MAX);
    for job in CLOSING.lock().slots.iter_mut().flatten() { job.recover(now); }
}
impl Closing {
    fn recover(&mut self, now: u64) {
        if self.quarantine == Some(true) {
            self.quarantine = None; self.deadline = now.saturating_add(CLOSE_TICKS);
        }
    }
    /// true only after the cap is known closed or its provider has died.
    fn advance<T: Transport>(&mut self, io: &mut T) -> bool {
        if self.quarantine.is_some() { return false; }
        let now = io.now();
        if let Some(operation) = self.op {
            if now >= self.deadline { io.cancel(operation); }
            let mut reply = [0u8; IPC_BUF_SIZE];
            return match io.take(operation, &mut reply) {
                Ok(IpcTakeResult::Pending) => false,
                Ok(IpcTakeResult::Terminal { status: IpcTerminal::Reply | IpcTerminal::PeerGone, .. }) => true,
                _ => { self.op = None; self.quarantine = Some(false); false },
            };
        }
        if now >= self.deadline {
            self.quarantine = Some(true); return false;
        }
        let mut bytes = [0u8; IPC_BUF_SIZE];
        let len = if self.tls { bytes[0] = 0x15; bytes[1..9].copy_from_slice(&self.cap.to_le_bytes()); 9 }
            else { match api::ipc::encode(&NetRequest::TcpClose { cap_id: self.cap as u32 }, &mut bytes) {
                Ok(bytes) => bytes.len(),
                Err(_) => { self.quarantine = Some(false); return false; }
            } };
        match io.submit(self.endpoint, &bytes[..len]) {
            Ok(operation) => {
                self.op = Some(operation);
                // Submission, not queue entry, starts the response deadline.
                self.deadline = now.saturating_add(CLOSE_TICKS);
                false
            }
            Err(IpcSubmitError::Busy) => false,
            Err(IpcSubmitError::PeerGone) => true,
            Err(_) => { self.quarantine = Some(false); false }
        }
    }
}
fn enqueue_close(endpoint: usize, cap: u64, tls: bool) {
    {
        let mut queue = CLOSING.lock();
        if queue.slots.iter().flatten().any(|job| job.endpoint == endpoint && job.cap == cap) { return; }
        // Pre-connect admission reserves a slot for every live job, including
        // cancellation races; no socket can be created when the reserve is full.
        let slot = queue.slots.iter_mut().find(|slot| slot.is_none()).expect("close admission invariant");
        *slot = Some(Closing { endpoint, cap, tls, op: None, deadline: sys_get_time_ms().unwrap_or(0).saturating_add(CLOSE_TICKS), quarantine: None });
    }
    poll_cleanup();
}
pub fn poll_cleanup() {
    let mut queue = CLOSING.lock();
    // Four slots per UI turn, fixed storage and round-robin fairness.
    for _ in 0..4 {
        let index = queue.cursor; queue.cursor = (index + 1) % CLOSE_SLOTS;
        let Some(job) = queue.slots[index].as_mut() else { continue; };
        if job.advance(&mut KernelTransport::new()) { queue.slots[index] = None; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn url_query_fragment_and_redirect_policy() {
        let url = Url::parse("https://Example.com:443?q=1#fragment").unwrap();
        assert_eq!(url.full, "https://example.com/?q=1");
        assert_eq!(url.redirect("../next").unwrap().full, "https://example.com/next");
        assert!(url.redirect("http://example.com/").is_err());
        for bad in ["http://host:0/", "http://u:p@host/", "http://host/\r\nx", "ftp://host/"] { assert!(Url::parse(bad).is_err()); }
    }
    #[test] fn incremental_chunked_with_trailers() {
        let response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4;ext=yes\r\nWiki\r\n5\r\npedia\r\n0\r\nX-End: yes\r\n\r\n";
        let mut decoder = ResponseDecoder::new(32, false);
        for byte in response { decoder.feed(core::slice::from_ref(byte)).unwrap(); }
        assert!(decoder.complete()); assert_eq!(decoder.body, b"Wikipedia");
    }
    #[test] fn content_length_requires_exact_completion() {
        let mut decoder = ResponseDecoder::new(32, false);
        decoder.feed(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nabc").unwrap();
        assert!(!decoder.complete()); assert!(decoder.eof().is_err());
        decoder.feed(b"de").unwrap(); assert!(decoder.complete());
        assert!(decoder.feed(b"x").is_err());
    }
    #[test] fn malformed_status_framing_and_budget_fail_closed() {
        for response in ["oops\r\n\r\nbody", "HTTP/1.1 404 Missing\r\n\r\n", "HTTP/1.1 200 OK\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\n", "HTTP/1.1 200 OK\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\n", "HTTP/1.1 200 OK\r\nContent-Length: 33\r\n\r\n", "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nZ\r\n"] {
            assert!(ResponseDecoder::new(32, false).feed(response.as_bytes()).is_err(), "{}", response);
        }
    }
    #[test] fn eof_and_gemini_headers_are_explicit() {
        let mut decoder = ResponseDecoder::new(8, false); decoder.feed(b"HTTP/1.0 200 OK\r\n\r\nhello").unwrap();
        assert!(!decoder.complete()); decoder.eof().unwrap(); assert!(decoder.complete());
        let mut gemini = ResponseDecoder::new(8, true); gemini.feed(b"20 text/gemini\r\nhi").unwrap(); gemini.eof().unwrap(); assert_eq!(gemini.body, b"hi");
        assert!(ResponseDecoder::new(8, true).feed(b"51 Not found\r\n").is_err());
        let mut redirect = ResponseDecoder::new(8, false); redirect.feed(b"HTTP/1.1 302 Found\r\nLocation: /new\r\n\r\n").unwrap(); assert!(redirect.complete()); assert_eq!(redirect.location.as_deref(), Some("/new"));
    }
    struct CloseIo {
        now: u64, busy: bool, submits: usize, cancels: usize, terminal: Option<IpcTerminal>,
    }
    impl Transport for CloseIo {
        fn now(&self) -> u64 { self.now }
        fn endpoint(&mut self) -> Option<usize> { Some(7) }
        fn submit(&mut self, _: usize, _: &[u8]) -> Result<IpcOpId, IpcSubmitError> {
            self.submits += 1;
            if self.busy { Err(IpcSubmitError::Busy) } else { Ok(42) }
        }
        fn take(&mut self, _: IpcOpId, _: &mut [u8]) -> Result<IpcTakeResult, ipc::IpcError> {
            Ok(match self.terminal { Some(status) => IpcTakeResult::Terminal { status, len: 0 }, None => IpcTakeResult::Pending })
        }
        fn cancel(&mut self, _: IpcOpId) { self.cancels += 1; self.terminal = Some(IpcTerminal::Indeterminate); }
        fn close(&mut self, _: usize, _: u64, _: bool) { panic!("unexpected nested close"); }
        fn cleanup(&mut self) {}
    }
    fn closing() -> Closing {
        Closing { endpoint: 7, cap: 99, tls: false, op: None, deadline: CLOSE_TICKS, quarantine: None }
    }
    #[test]
    fn busy_cleanup_quarantines_at_admission_deadline_and_explicitly_recovers() {
        let mut close = closing();
        let mut io = CloseIo { now: 0, busy: true, submits: 0, cancels: 0, terminal: None };
        assert!(!close.advance(&mut io)); assert_eq!(io.submits, 1);
        io.now = CLOSE_TICKS;
        assert!(!close.advance(&mut io)); assert_eq!(close.quarantine, Some(true));
        assert_eq!(io.submits, 1); assert_eq!(close.cap, 99);
        io.now += 100_000;
        io.busy = false;
        assert!(!close.advance(&mut io)); assert_eq!(io.submits, 1);
        close.recover(io.now);
        assert!(!close.advance(&mut io)); assert_eq!(close.op, Some(42));
        assert_eq!(close.deadline, io.now + CLOSE_TICKS);
        assert_eq!(io.cancels, 0);
        io.now += CLOSE_TICKS - 1;
        assert!(!close.advance(&mut io)); assert_eq!(io.cancels, 0);
        io.terminal = Some(IpcTerminal::Reply);
        assert!(close.advance(&mut io)); assert_eq!(io.submits, 2);
    }
    #[test]
    fn late_close_admission_gets_fresh_response_deadline() {
        let mut close = closing();
        let mut io = CloseIo { now: CLOSE_TICKS - 1, busy: false, submits: 0, cancels: 0, terminal: None };
        assert!(!close.advance(&mut io));
        io.now = CLOSE_TICKS;
        assert!(!close.advance(&mut io)); assert_eq!(io.cancels, 0);
        io.now = close.deadline;
        assert!(!close.advance(&mut io)); assert_eq!(io.cancels, 1);
        assert_eq!(close.quarantine, Some(false)); assert_eq!(close.cap, 99);
        close.recover(io.now + CLOSE_TICKS);
        assert!(!close.advance(&mut io)); assert_eq!(io.submits, 1);
        assert_eq!(close.quarantine, Some(false)); // Never retry an accepted uncertain close.
    }
}

#[cfg(test)]
#[path = "http_tests.rs"]
mod job_tests;
