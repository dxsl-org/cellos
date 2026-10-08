//! One bounded poll per TLS operation per net-service turn.
//! No TLS request receives a reply until its original operation finishes.

extern crate alloc;

use crate::{
    interface::VirtioNetDevice,
    socket_state::SocketState,
    socket_table::{SocketOwner, SocketTable},
    tls::socket::TlsSocketEntry,
    tls::transport::{clear_tls_context, set_tls_context},
    tls_wire::encode_tls_recv_reply,
};
use alloc::boxed::Box;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::vec::Vec;
use core::future::Future;
use core::ops::Bound::{Excluded, Unbounded};
use core::pin::Pin;
use core::task::{Context, Poll, Waker};
use ostd::syscall::{sys_get_time, sys_try_send, SyscallResult};
use smoltcp::{iface::{Interface, SocketHandle, SocketSet}, socket::tcp};

const TCP_TIMEOUT: u64 = 150_000_000; // 15 seconds (sys_get_time: 10 MHz)
const TLS_TIMEOUT: u64 = 300_000_000; // 30 seconds for one TLS operation
const REPLY_TIMEOUT: u64 = 300_000_000; // legacy receiver may not yet be scheduled

type Handshake = Pin<Box<dyn Future<Output = Result<TlsSocketEntry, embedded_tls::TlsError>>>>;
type Sending = Pin<Box<dyn Future<Output = (TlsSocketEntry, Result<usize, embedded_tls::TlsError>)>>>;
type Receiving = Pin<Box<dyn Future<Output = (TlsSocketEntry, Vec<u8>, Result<usize, embedded_tls::TlsError>)>>>;

enum State {
    Connecting(String),
    Handshake(Handshake),
    Send(Sending),
    Recv(Receiving),
    Vacant,
}

pub enum Request {
    Send(Vec<u8>),
    Recv(usize),
}

#[derive(Clone, Copy)]
pub enum ReplyTo {
    Async(usize),
    Legacy(usize),
}

impl ReplyTo {
    pub fn capture(sender: usize) -> Self {
        match ostd::ipc::current() {
            Some(token) => Self::Async(token),
            None => Self::Legacy(sender),
        }
    }
}

struct Queued {
    reply: ReplyTo,
    request: Request,
}

struct Pending {
    owner: SocketOwner,
    reply: ReplyTo,
    handle: SocketHandle,
    deadline: u64,
    state: State,
    queue: VecDeque<Queued>,
}

struct LegacyReply {
    sender: usize,
    owner: SocketOwner,
    cap: Option<u64>,
    bytes: Vec<u8>,
    deadline: u64,
}

#[derive(Default)]
pub struct TlsPending {
    operations: BTreeMap<u64, Pending>,
    replies: VecDeque<LegacyReply>,
}

fn failed(request: &Request) -> &'static [u8] {
    match request {
        Request::Send(_) => &[0, 0, 0, 0],
        Request::Recv(_) => &[0, 0],
    }
}

impl TlsPending {
    pub fn is_empty(&self) -> bool {
        self.operations.is_empty() && self.replies.is_empty()
    }

    pub fn contains_cap(&self, cap: u64) -> bool {
        self.operations.contains_key(&cap) || self.replies.iter().any(|reply| reply.cap == Some(cap))
    }

    pub fn connect(&mut self, cap: u64, handle: SocketHandle, owner: SocketOwner,
        reply: ReplyTo, hostname: String) {
        self.operations.insert(cap, Pending {
            owner, reply, handle,
            deadline: sys_get_time().saturating_add(TCP_TIMEOUT),
            state: State::Connecting(hostname), queue: VecDeque::new(),
        });
    }

    pub fn enqueue(&mut self, cap: u64, owner: SocketOwner, reply: ReplyTo,
        request: Request, tls_table: &mut BTreeMap<u64, TlsSocketEntry>,
        table: &SocketTable) {
        if let Some(op) = self.operations.get_mut(&cap) {
            if op.owner == owner {
                op.queue.push_back(Queued { reply, request });
            } else {
                self.answer(reply, owner, None, failed(&request));
            }
        } else if let Some(handle) = table.get(cap, owner) {
            if let Some(entry) = tls_table.remove(&cap) {
                let state = start(entry, request);
                self.operations.insert(cap, Pending {
                    owner, reply, handle,
                    deadline: sys_get_time().saturating_add(TLS_TIMEOUT),
                    state, queue: VecDeque::new(),
                });
            } else {
                self.answer(reply, owner, None, failed(&request));
            }
        } else {
            self.answer(reply, owner, None, failed(&request));
        }
    }

    /// Async callers have kernel operation tokens. Legacy callers use their
    /// original sender TID; a nonblocking send is retried until their masked
    /// receive is ready, never holding the net dispatcher hostage.
    fn answer(&mut self, reply: ReplyTo, owner: SocketOwner, cap: Option<u64>, bytes: &[u8]) -> bool {
        match reply {
            ReplyTo::Async(token) => ostd::ipc::reply(token, bytes).is_ok(),
            ReplyTo::Legacy(sender) => {
                self.replies.push_back(LegacyReply {
                    sender, owner, cap, bytes: bytes.to_vec(),
                    deadline: sys_get_time().saturating_add(REPLY_TIMEOUT),
                });
                true
            }
        }
    }

    fn flush_replies(&mut self, sockets: &mut SocketSet<'_>, table: &mut SocketTable,
        tls_table: &mut BTreeMap<u64, TlsSocketEntry>) {
        let count = self.replies.len();
        for _ in 0..count {
            let Some(reply) = self.replies.pop_front() else { break; };
            if matches!(sys_try_send(reply.sender, &reply.bytes), SyscallResult::Ok(0)) {
                continue;
            }
            if sys_get_time() < reply.deadline {
                self.replies.push_back(reply);
                continue;
            }
            if let Some(cap) = reply.cap {
                self.cancel_cap(cap);
                tls_table.remove(&cap);
                if let Some(handle) = table.remove_internal(cap) {
                    sockets.remove(handle);
                }
            }
        }
    }

    /// Cancel the pending future (which owns keys) BEFORE removing its TCP socket.
    /// Call before SocketTable::remove_owner and the TLS table's owner cleanup.
    pub fn cancel_owner(&mut self, owner: SocketOwner, sockets: &mut SocketSet<'_>,
        table: &mut SocketTable) {
        let mut cursor = None;
        while let Some(cap) = self.next_key(cursor) {
            cursor = Some(cap);
            if self.operations.get(&cap).is_some_and(|op| op.owner == owner) {
                let op = self.operations.remove(&cap).unwrap();
                drop(op);
                if let Some(handle) = table.remove_internal(cap) {
                    sockets.remove(handle);
                }
            }
        }
        self.replies.retain(|reply| reply.owner != owner);
    }

    /// A close from the owner cancels any in-flight and queued calls on this cap.
    /// The caller still performs normal SocketTable and TLS table removal.
    pub fn cancel_cap(&mut self, cap: u64) {
        self.replies.retain(|reply| reply.cap != Some(cap));
        if let Some(op) = self.operations.remove(&cap) {
            let Pending { reply, owner, state, queue, .. } = op;
            let response: &[u8] = match &state {
                State::Connecting(_) | State::Handshake(_) => &[0; 8],
                State::Send(_) => &[0; 4],
                State::Recv(_) => &[0; 2],
                State::Vacant => &[],
            };
            drop(state);
            self.answer(reply, owner, None, response);
            for queued in queue {
                self.answer(queued.reply, owner, None, failed(&queued.request));
            }
        }
    }

    fn next_key(&self, after: Option<u64>) -> Option<u64> {
        match after {
            None => self.operations.first_key_value().map(|(&cap, _)| cap),
            Some(cap) => self.operations.range((Excluded(cap), Unbounded)).next().map(|(&cap, _)| cap),
        }
    }

    /// Poll every pending operation once. The caller must continue regular NIC
    /// pumping and smoltcp polling even when no request arrives.
    pub fn poll(&mut self, iface: &mut Interface, device: &mut VirtioNetDevice,
        sockets: &mut SocketSet<'_>, table: &mut SocketTable,
        tls_table: &mut BTreeMap<u64, TlsSocketEntry>) {
        let mut cursor = None;
        while let Some(cap) = self.next_key(cursor) {
            cursor = Some(cap);
            let mut op = self.operations.remove(&cap).unwrap();
            if table.get(cap, op.owner) != Some(op.handle) || sys_get_time() >= op.deadline {
                self.fail(cap, op, sockets, table);
                continue;
            }
            let result = match &mut op.state {
                State::Connecting(_) => match sockets.get_mut::<tcp::Socket>(op.handle).state() {
                    tcp::State::Established => Step::TcpConnected,
                    tcp::State::Closed | tcp::State::CloseWait => Step::Failed,
                    _ => Step::Waiting,
                },
                State::Handshake(future) => {
                    match poll_future(future, iface, device, sockets) {
                        Poll::Ready(Ok(entry)) => Step::Connected(entry),
                        Poll::Ready(Err(_)) => Step::Failed,
                        Poll::Pending => Step::Waiting,
                    }
                }
                State::Send(future) => match poll_future(future, iface, device, sockets) {
                    Poll::Ready((entry, result)) => Step::Sent(entry, result),
                    Poll::Pending => Step::Waiting,
                },
                State::Recv(future) => match poll_future(future, iface, device, sockets) {
                    Poll::Ready((entry, data, result)) => Step::Received(entry, data, result),
                    Poll::Pending => Step::Waiting,
                },
                State::Vacant => unreachable!(),
            };
            match result {
                Step::Waiting => { self.operations.insert(cap, op); }
                Step::TcpConnected => {
                    table.set_state(cap, SocketState::Connected);
                    let State::Connecting(hostname) = core::mem::replace(&mut op.state, State::Vacant) else {
                        unreachable!();
                    };
                    op.state = State::Handshake(Box::pin(TlsSocketEntry::handshake(op.handle, hostname)));
                    op.deadline = sys_get_time().saturating_add(TLS_TIMEOUT);
                    self.operations.insert(cap, op);
                }
                Step::Failed => self.fail(cap, op, sockets, table),
                Step::Connected(entry) => {
                    // Drop the finished future before moving the established
                    // connection into the live table.
                    op.state = State::Vacant;
                    if self.answer(op.reply, op.owner, Some(cap), &cap.to_le_bytes()) {
                        tls_table.insert(cap, entry);
                        self.start_queued(cap, op, tls_table);
                    } else {
                        drop(entry);
                        for queued in op.queue {
                            self.answer(queued.reply, op.owner, None, failed(&queued.request));
                        }
                        if let Some(handle) = table.remove_internal(cap) { sockets.remove(handle); }
                    }
                }
                Step::Sent(entry, result) => {
                    op.state = State::Vacant;
                    let count = result.unwrap_or(0) as u32;
                    if self.answer(op.reply, op.owner, Some(cap), &count.to_le_bytes()) {
                        tls_table.insert(cap, entry);
                        self.start_queued(cap, op, tls_table);
                    } else {
                        drop(entry);
                        for queued in op.queue { self.answer(queued.reply, op.owner, None, failed(&queued.request)); }
                        if let Some(handle) = table.remove_internal(cap) { sockets.remove(handle); }
                    }
                }
                Step::Received(entry, data, result) => {
                    op.state = State::Vacant;
                    let response = match result {
                        Ok(len) => encode_tls_recv_reply(&data[..len]),
                        Err(_) => alloc::vec![0; 2],
                    };
                    if self.answer(op.reply, op.owner, Some(cap), &response) {
                        tls_table.insert(cap, entry);
                        self.start_queued(cap, op, tls_table);
                    } else {
                        drop(entry);
                        for queued in op.queue { self.answer(queued.reply, op.owner, None, failed(&queued.request)); }
                        if let Some(handle) = table.remove_internal(cap) { sockets.remove(handle); }
                    }
                }
            }
        }
        self.flush_replies(sockets, table, tls_table);
    }

    fn start_queued(&mut self, cap: u64, mut op: Pending,
        tls_table: &mut BTreeMap<u64, TlsSocketEntry>) {
        if let Some(next) = op.queue.pop_front() {
            let entry = tls_table.remove(&cap).expect("completed TLS entry");
            op.reply = next.reply;
            op.deadline = sys_get_time().saturating_add(TLS_TIMEOUT);
            op.state = start(entry, next.request);
            self.operations.insert(cap, op);
        }
    }

    fn fail(&mut self, cap: u64, op: Pending, sockets: &mut SocketSet<'_>, table: &mut SocketTable) {
        let Pending { reply, owner, state, queue, .. } = op;
        let response: &[u8] = match &state {
            State::Connecting(_) | State::Handshake(_) => &[0; 8],
            State::Send(_) => &[0; 4],
            State::Recv(_) => &[0; 2],
            State::Vacant => &[],
        };
        // The suspended future owns TLS keys and record buffers: destroy it
        // before the smoltcp socket can be removed.
        drop(state);
        self.answer(reply, owner, None, response);
        for queued in queue { self.answer(queued.reply, owner, None, failed(&queued.request)); }
        if let Some(handle) = table.remove_internal(cap) { sockets.remove(handle); }
    }
}

enum Step {
    Waiting,
    TcpConnected,
    Failed,
    Connected(TlsSocketEntry),
    Sent(TlsSocketEntry, Result<usize, embedded_tls::TlsError>),
    Received(TlsSocketEntry, Vec<u8>, Result<usize, embedded_tls::TlsError>),
}

fn start(mut entry: TlsSocketEntry, request: Request) -> State {
    match request {
        Request::Send(data) => State::Send(Box::pin(async move {
            let result = entry.send(&data).await;
            (entry, result)
        })),
        Request::Recv(size) => State::Recv(Box::pin(async move {
            let mut data = alloc::vec![0u8; size];
            let result = entry.recv(&mut data).await;
            (entry, data, result)
        })),
    }
}

fn poll_future<T>(future: &mut Pin<Box<dyn Future<Output = T>>>, iface: &mut Interface,
    device: &mut VirtioNetDevice, sockets: &mut SocketSet<'_>) -> Poll<T> {
    // The transport leaves Pending on absent data/space; the outer service
    // dispatcher, not a self-waking busy loop, is responsible for the next poll.
    let mut context = Context::from_waker(Waker::noop());
    unsafe { set_tls_context(iface, device, sockets as *mut SocketSet<'_> as *mut ()) };
    let result = future.as_mut().poll(&mut context);
    clear_tls_context();
    result
}

