// SPDX-License-Identifier: MIT
//! Demand leases and exact-operation IPC. No legacy mailbox message is consumed.

use alloc::format;
use alloc::string::String;
use ocel_service_proto::{Engine, Failure, Request, Response, FRAME_BYTES};
use ostd::ipc::{self, IpcSubmitError, IpcTakeResult, IpcTerminal};
use ostd::syscall::{sys_get_scheduler_ticks, sys_lookup_service, sys_yield};

const CALL_TICKS: u64 = 500;
const RELEASE_TICKS: u64 = 100;

pub(crate) trait Transport {
    fn lookup(&mut self, service: u16) -> Option<usize>;
    fn call(&mut self, peer: usize, request: &[u8], reply: &mut [u8], ticks: u64) -> Result<usize, String>;
}

#[derive(Default)]
pub(crate) struct KernelTransport;
impl Transport for KernelTransport {
    fn lookup(&mut self, service: u16) -> Option<usize> { sys_lookup_service(service) }
    fn call(&mut self, peer: usize, request: &[u8], reply: &mut [u8], ticks: u64) -> Result<usize, String> {
        bounded_call(&mut KernelOperations, peer, request, reply, ticks)
    }
}

trait Operations {
    fn now(&mut self) -> Option<u64>;
    fn submit(&mut self, peer: usize, request: &[u8]) -> Result<usize, IpcSubmitError>;
    fn take(&mut self, op: usize, reply: &mut [u8]) -> Result<IpcTakeResult, ipc::IpcError>;
    fn wait(&mut self, ticks: u64);
    fn yield_once(&mut self);
    fn cancel(&mut self, op: usize);
}
struct KernelOperations;
impl Operations for KernelOperations {
    fn now(&mut self) -> Option<u64> { sys_get_scheduler_ticks() }
    fn submit(&mut self, peer: usize, request: &[u8]) -> Result<usize, IpcSubmitError> { ipc::submit(peer, request) }
    fn take(&mut self, op: usize, reply: &mut [u8]) -> Result<IpcTakeResult, ipc::IpcError> { ipc::take(op, reply) }
    fn wait(&mut self, ticks: u64) { ipc::wait(ticks); }
    fn yield_once(&mut self) { sys_yield(); }
    fn cancel(&mut self, op: usize) { let _ = ipc::cancel(op); }
}

fn bounded_call(ops: &mut impl Operations, peer: usize, request: &[u8], reply: &mut [u8], ticks: u64) -> Result<usize, String> {
    let started = ops.now().ok_or_else(|| String::from("Engine IPC deadline clock is unavailable."))?;
    let remaining = |now: Option<u64>| now.map(|now| ticks.saturating_sub(now.wrapping_sub(started))).unwrap_or(0);
    let op = loop {
        if remaining(ops.now()) == 0 { return Err(String::from("Engine IPC admission timed out.")); }
        match ops.submit(peer, request) {
            Ok(op) => break op,
            Err(IpcSubmitError::Busy) => ops.yield_once(),
            Err(error) => return Err(format!("Engine IPC admission failed: {:?}.", error)),
        }
    };
    let result = loop {
        match ops.take(op, reply) {
            Ok(IpcTakeResult::Terminal { status: IpcTerminal::Reply, len }) => return Ok(len),
            Ok(IpcTakeResult::Terminal { status, .. }) => return Err(format!("Engine IPC failed: {:?}.", status)),
            Err(error) => break Err(format!("Engine IPC response failed: {:?}.", error)),
            Ok(IpcTakeResult::Pending) => {}
        }
        let left = remaining(ops.now());
        if left == 0 { break Err(String::from("Engine IPC response timed out.")); }
        ops.wait(left);
    };
    // Cancellation isolates late replies from subsequent calls. Take the retained
    // cancellation terminal even on decode/buffer errors to free the operation slot.
    ops.cancel(op);
    let mut discard = [0u8; 4096];
    let _ = ops.take(op, &mut discard);
    result
}

fn service(engine: Engine) -> u16 {
    match engine { Engine::JavaScript => api::syscall::service::OCEL_JS, Engine::Pdf => api::syscall::service::OCEL_PDF }
}
fn name(engine: Engine) -> &'static str {
    match engine { Engine::JavaScript => "JavaScript", Engine::Pdf => "PDF" }
}

pub(crate) struct Lease<T: Transport = KernelTransport> {
    transport: T,
    engine: Engine,
    broker: usize,
    token: Option<u64>,
    provider: usize,
}
impl<T: Transport> Lease<T> {
    pub(crate) fn acquire_with(engine: Engine, mut transport: T) -> Result<Self, String> {
        let broker = transport.lookup(api::syscall::service::OCEL_ACTIVATOR)
            .ok_or_else(|| format!("{} engine is unavailable: activation service is not registered. No fallback engine is available.", name(engine)))?;
        let mut frame = [0u8; FRAME_BYTES];
        // Exact operation tokens authenticate the provider incarnation: unlike
        // sender-masked legacy receive, only this broker can complete the call.
        let len = transport.call(broker, &Request::Acquire { engine }.encode(), &mut frame, CALL_TICKS)?;
        let response = if len == FRAME_BYTES { Response::decode(&frame) } else { None };
        let (received, token, provider) = match response {
            Some(Response::Ready { engine: received, lease, tid }) => (received, lease, usize::try_from(tid).ok()),
            Some(Response::Error { engine: received, failure }) if received == engine => {
                let reason = match failure {
                    Failure::Unavailable => "optional engine is not installed or available",
                    Failure::Denied => "activation was denied",
                    Failure::Busy => "activation lease limit is reached",
                    Failure::Failed => "engine initialization failed",
                };
                return Err(format!("{} engine is unavailable: {}. No fallback engine is available.", name(engine), reason));
            }
            _ => return Err(String::from("Invalid engine activation reply (engine or frame mismatch).")),
        };
        let mut lease = Self { transport, engine: received, broker, token: Some(token), provider: provider.unwrap_or(0) };
        // Own every valid Ready token before validating it. Even an unexpected
        // engine must not leave forgotten demand behind when the reply is rejected.
        if received != engine { return Err(String::from("Invalid engine activation reply (engine mismatch).")); }
        lease.validate()?;
        Ok(lease)
    }

    fn validate(&mut self) -> Result<(), String> {
        if self.token.is_none()
            || self.transport.lookup(api::syscall::service::OCEL_ACTIVATOR) != Some(self.broker)
            || self.transport.lookup(service(self.engine)) != Some(self.provider)
            || self.provider == 0
        {
            return Err(format!("{} engine provider changed or stopped; reload the document to activate it again.", name(self.engine)));
        }
        Ok(())
    }

    pub(crate) fn exchange(&mut self, request: &[u8], reply: &mut [u8]) -> Result<usize, String> {
        self.validate()?;
        self.transport.call(self.provider, request, reply, CALL_TICKS)
    }

    pub(crate) fn release(&mut self) -> Result<(), String> {
        let Some(token) = self.token else { return Ok(()); };
        if self.transport.lookup(api::syscall::service::OCEL_ACTIVATOR) != Some(self.broker) {
            self.token = None; // The old broker incarnation cannot own live leases.
            return Err(String::from("Engine activation provider stopped before release."));
        }
        let mut frame = [0u8; FRAME_BYTES];
        let len = self.transport.call(self.broker, &Request::Release { engine: self.engine, lease: token }.encode(), &mut frame, RELEASE_TICKS)?;
        if len != FRAME_BYTES || Response::decode(&frame) != Some(Response::Released { engine: self.engine }) {
            return Err(String::from("Engine lease release was not acknowledged."));
        }
        self.token = None;
        Ok(())
    }
}
impl<T: Transport> Drop for Lease<T> {
    fn drop(&mut self) {
        if let Err(error) = self.release() { ostd::io::println(&format!("[ocel] {}", error)); }
    }
}

#[cfg(test)]
pub(crate) mod tests;
