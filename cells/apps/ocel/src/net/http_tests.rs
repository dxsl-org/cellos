// SPDX-License-Identifier: MIT
//! Deterministic cooperative transport regressions; no services or wall sleeps.
use super::*;
use alloc::{collections::{BTreeMap, BTreeSet, VecDeque}, rc::Rc};
use core::cell::RefCell;

#[derive(Default)]
struct Trace {
    now: u64, next: usize, cap: u32, hold: Option<Stage>,
    operations: BTreeMap<usize, Option<Vec<u8>>>, cancelled: BTreeSet<usize>,
    requests: Vec<Vec<u8>>, closes: Vec<u64>, reads: VecDeque<Vec<u8>>,
    tls_reads: VecDeque<Vec<u8>>, not_ready_once: bool, accepted: Vec<u8>,
    busy_once: bool,
}
#[derive(Clone, Default)]
struct Fake(Rc<RefCell<Trace>>);
fn typed(response: NetResponse<'_>) -> Vec<u8> {
    let mut bytes = [0u8; IPC_BUF_SIZE]; api::ipc::encode(&response, &mut bytes).unwrap().to_vec()
}
impl Transport for Fake {
    fn now(&self) -> u64 { self.0.borrow().now }
    fn endpoint(&mut self) -> Option<usize> { Some(7) }
    fn submit(&mut self, _: usize, bytes: &[u8]) -> Result<IpcOpId, IpcSubmitError> {
        let mut state = self.0.borrow_mut(); state.next += 1; let id = state.next;
        if state.busy_once { state.busy_once = false; return Err(IpcSubmitError::Busy); }
        state.requests.push(bytes.to_vec());
        let (stage, reply) = match bytes.first() {
            Some(0x30) => { state.cap += 1; (Stage::Connect, (state.cap as u64).to_le_bytes().to_vec()) }
            Some(0x31) => {
                let data = &bytes[11..]; let count = data.len().min(7); state.accepted.extend_from_slice(&data[..count]);
                (Stage::Send, (count as u32).to_le_bytes().to_vec())
            }
            Some(0x32) => (Stage::Read, state.tls_reads.pop_front().unwrap_or_else(|| alloc::vec![0, 0])),
            _ => match api::ipc::decode::<NetRequest>(bytes).unwrap() {
                NetRequest::Resolve { .. } => (Stage::Resolve, typed(NetResponse::Addr([127,0,0,1]))),
                NetRequest::TcpConnect { .. } => { state.cap += 1; (Stage::Connect, typed(NetResponse::CapId(state.cap))) }
                NetRequest::SocketState { .. } => (Stage::WaitConnect, typed(NetResponse::State(0x03))),
                NetRequest::TcpSendReady { data, .. } => {
                    if state.not_ready_once { state.not_ready_once = false; (Stage::Send, typed(NetResponse::NotReady)) }
                    else { let count = data.len().min(7); state.accepted.extend_from_slice(&data[..count]); (Stage::Send, typed(NetResponse::WriteProgress(count as u32))) }
                }
                NetRequest::TcpRecvReady { .. } => (Stage::Read, match state.reads.pop_front() {
                    Some(data) => data, None => typed(NetResponse::Eof),
                }),
                _ => panic!("Unexpected request"),
            },
        };
        let reply = if state.hold == Some(stage) { None } else { Some(reply) };
        state.operations.insert(id, reply); Ok(id)
    }
    fn take(&mut self, operation: IpcOpId, reply: &mut [u8]) -> Result<IpcTakeResult, ipc::IpcError> {
        let mut state = self.0.borrow_mut();
        if state.cancelled.remove(&operation) {
            state.operations.remove(&operation);
            return Ok(IpcTakeResult::Terminal { status: IpcTerminal::Cancelled, len: 0 });
        }
        match state.operations.get(&operation) {
            Some(None) => Ok(IpcTakeResult::Pending),
            Some(Some(bytes)) => {
                let len = bytes.len(); reply[..len].copy_from_slice(bytes); state.operations.remove(&operation);
                Ok(IpcTakeResult::Terminal { status: IpcTerminal::Reply, len })
            }
            None => Err(ipc::IpcError::InvalidOperation),
        }
    }
    fn cancel(&mut self, operation: IpcOpId) {
        let mut state = self.0.borrow_mut();
        // A settled reply wins, exactly as in the kernel operation ABI.
        if matches!(state.operations.get(&operation), Some(None)) { state.cancelled.insert(operation); }
    }
    fn close(&mut self, _: usize, cap: u64, _: bool) { self.0.borrow_mut().closes.push(cap); }
    fn cleanup(&mut self) {}
}
fn job(url: &str, fake: &Fake) -> FetchJob<Fake> { FetchJob::with_transport(url, 4096, None, fake.clone()).unwrap() }
fn finish(job: &mut FetchJob<Fake>) -> Result<FetchResult, String> {
    for _ in 0..512 { if let Poll::Ready(result) = job.poll() { return result; } }
    panic!("Job failed to reach a bounded terminal")
}
fn advance_to_pending(job: &mut FetchJob<Fake>, stage: Stage) {
    for _ in 0..256 {
        assert!(job.poll().is_pending());
        if job.stage == stage && job.pending.is_some() { return; }
    }
    panic!("Did not reach requested stage")
}
#[test]
fn partial_writes_and_not_ready_preserve_every_request_byte() {
    let fake = Fake::default(); fake.0.borrow_mut().not_ready_once = true;
    fake.0.borrow_mut().reads.push_back(typed(NetResponse::Data(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello")));
    let mut job = job("http://host:8080/page?q=1#anchor", &fake);
    let expected = job.request.clone(); let result = finish(&mut job).unwrap();
    assert_eq!(result.body, b"hello"); assert_eq!(result.url, "http://host:8080/page?q=1#anchor");
    assert_eq!(fake.0.borrow().accepted, expected); assert_eq!(fake.0.borrow().closes, alloc::vec![1]);
    assert!(fake.0.borrow().operations.is_empty());
}
#[test]
fn cancelling_dns_connect_and_read_drains_exact_operation() {
    for stage in [Stage::Resolve, Stage::Connect, Stage::WaitConnect, Stage::Read] {
        let fake = Fake::default(); fake.0.borrow_mut().hold = Some(stage);
        let mut old = job("http://old/page", &fake); advance_to_pending(&mut old, stage);
        old.cancel(); assert!(old.poll().is_pending()); assert!(fake.0.borrow().operations.is_empty());
        let closes = fake.0.borrow().closes.len(); assert_eq!(closes, usize::from(matches!(stage, Stage::WaitConnect | Stage::Read)));
        fake.0.borrow_mut().hold = None;
        fake.0.borrow_mut().reads.push_back(typed(NetResponse::Data(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nnew")));
        let mut newer = job("http://new/page", &fake); assert_eq!(finish(&mut newer).unwrap().body, b"new");
    }
}
#[test]
fn connect_reply_winning_cancel_race_is_closed() {
    let fake = Fake::default(); let mut job = job("http://host/", &fake);
    advance_to_pending(&mut job, Stage::Connect); // Cap reply is settled but not taken.
    job.cancel(); assert_eq!(fake.0.borrow().closes, alloc::vec![1]);
    assert!(fake.0.borrow().operations.is_empty());
}
#[test]
fn deadline_measures_time_not_number_of_polls() {
    let fake = Fake::default(); fake.0.borrow_mut().hold = Some(Stage::Read);
    let mut job = job("http://host/", &fake); advance_to_pending(&mut job, Stage::Read);
    for _ in 0..50 { assert!(job.poll().is_pending()); }
    fake.0.borrow_mut().now = LOAD_TICKS;
    assert!(matches!(job.poll(), Poll::Ready(Err(error)) if error.contains("deadline")));
    assert_eq!(fake.0.borrow().closes, alloc::vec![1]); assert!(fake.0.borrow().operations.is_empty());
}
#[test]
fn redirects_follow_bounded_safe_targets_and_close_each_cap() {
    let fake = Fake::default();
    fake.0.borrow_mut().reads.push_back(typed(NetResponse::Data(b"HTTP/1.1 302 Found\r\nLocation: ../next#heading\r\n\r\n")));
    fake.0.borrow_mut().reads.push_back(typed(NetResponse::Data(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")));
    let mut job = job("http://host/a/start?q=1", &fake); let result = finish(&mut job).unwrap();
    assert_eq!(result.url, "http://host/next#heading"); assert_eq!(result.body, b"ok");
    assert_eq!(fake.0.borrow().closes, alloc::vec![1, 2]);
    let fake = Fake::default();
    fake.0.borrow_mut().reads.push_back(typed(NetResponse::Data(b"HTTP/1.1 302 Found\r\nLocation: /\r\n\r\n")));
    assert!(finish(&mut job_for_loop(&fake)).unwrap_err().contains("loop"));
}
fn job_for_loop(fake: &Fake) -> FetchJob<Fake> { job("http://host/", fake) }
#[test]
fn cross_origin_resource_redirect_is_blocked_and_short_body_is_error() {
    let fake = Fake::default();
    fake.0.borrow_mut().reads.push_back(typed(NetResponse::Data(b"HTTP/1.1 302 Found\r\nLocation: http://evil/x\r\n\r\n")));
    let mut resource = FetchJob::with_transport("http://host/style.css", 4096, Some("http://host".into()), fake.clone()).unwrap();
    assert!(finish(&mut resource).unwrap_err().contains("Cross-origin"));
    let fake = Fake::default();
    fake.0.borrow_mut().reads.push_back(typed(NetResponse::Data(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nab")));
    assert!(finish(&mut job("http://host/", &fake)).unwrap_err().contains("Truncated"));
}
#[test]
fn tls_error_is_not_eof_and_gemini_uses_exact_raw_operations() {
    let fake = Fake::default(); fake.0.borrow_mut().tls_reads.push_back(alloc::vec![255,255]);
    assert!(finish(&mut job("https://host/", &fake)).unwrap_err().contains("TLS read failed"));
    let fake = Fake::default(); let body = b"20 text/gemini\r\n# Hello\n";
    let mut response = (body.len() as u16).to_le_bytes().to_vec(); response.extend_from_slice(body);
    fake.0.borrow_mut().tls_reads.push_back(response);
    let result = finish(&mut job("gemini://host/welcome", &fake)).unwrap();
    assert!(result.gemini); assert_eq!(result.body, b"# Hello\n");
    assert_eq!(fake.0.borrow().accepted, b"gemini://host/welcome\r\n");
}
#[test]
fn redirect_limit_is_enforced_without_retries() {
    let fake = Fake::default();
    for index in 0..=REDIRECT_LIMIT {
        let response = format!("HTTP/1.1 302 Found\r\nLocation: /step{}\r\n\r\n", index);
        fake.0.borrow_mut().reads.push_back(typed(NetResponse::Data(response.as_bytes())));
    }
    assert!(finish(&mut job("http://host/", &fake)).unwrap_err().contains("Too many redirects"));
    assert_eq!(fake.0.borrow().closes.len(), REDIRECT_LIMIT + 1);
}

#[test]
fn busy_is_unaccepted_and_retried_only_until_deadline() {
    let fake = Fake::default(); fake.0.borrow_mut().busy_once = true;
    fake.0.borrow_mut().reads.push_back(typed(NetResponse::Data(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")));
    let mut job = job("http://host/", &fake);
    assert!(job.poll().is_pending()); assert!(fake.0.borrow().operations.is_empty());
    assert_eq!(finish(&mut job).unwrap().body, b"ok");
}

#[test]
fn response_memory_budget_rejects_before_buffer_growth() {
    let mut decoder = ResponseDecoder::new(4, false);
    assert!(decoder.feed(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello").is_err());
    assert!(decoder.body.is_empty()); assert_eq!(decoder.body.capacity(), 0);
}

#[test]
fn success_location_is_not_followed_and_header_limit_rejects_growth() {
    let fake = Fake::default();
    fake.0.borrow_mut().reads.push_back(typed(NetResponse::Data(b"HTTP/1.1 200 OK\r\nLocation: /other\r\nContent-Length: 2\r\n\r\nok")));
    let result = finish(&mut job("http://host/", &fake)).unwrap();
    assert_eq!(result.url, "http://host/"); assert_eq!(fake.0.borrow().closes.len(), 1);
    let mut decoder = ResponseDecoder::new(4, false);
    let chunk = alloc::vec![b'x'; 4096];
    for _ in 0..4 { decoder.feed(&chunk).unwrap(); }
    assert!(decoder.feed(b"x").is_err()); assert!(decoder.body.is_empty());
}
