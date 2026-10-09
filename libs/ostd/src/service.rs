// SPDX-License-Identifier: MPL-2.0

//! Service discovery helpers for ViCell cells.
//!
//! Two levels of abstraction:
//! - Free functions [`lookup`] / [`register`] — thin wrappers for one-shot calls.
//! - [`ServiceRef<const ID>`] — a caching handle with a typed [`ServiceRef::call`] method
//!   for cells that talk to the same service repeatedly.
//!
//! [`ServiceRef`] resolves the provider's **binding** (`{tid, cell_id, generation}`) from
//! the kernel's registry rather than a bare tid, and reports a failed exchange against a
//! provider that has since been replaced as [`CallFailure::StaleBinding`] instead of
//! quietly re-targeting the call.

use crate::{syscall, ViError, ViResult};
use api::ipc::IPC_BUF_SIZE;
use api::service_binding::{ServiceBinding, SERVICE_BINDING_LEN};

pub use api::syscall::service;

/// Resolve the live provider tid of a well-known service.
///
/// Returns `Some(tid)` when a live provider is registered. Returns `None` during
/// the brief death→respawn window — the caller should retry with a small delay.
///
/// # Example
/// ```no_run
/// let vfs = ostd::service::lookup(ostd::service::service::VFS).expect("VFS not ready");
/// ```
pub fn lookup(service_id: u16) -> Option<usize> {
    syscall::sys_lookup_service(service_id)
}

/// Register the calling cell as the live provider of `service_id`.
///
/// Requires `SpawnCap` except for kernel-defined, operation-specific self-registration
/// capabilities. The development/test-hooks Silo route is one such exception and can
/// publish only `service::SILO` with `tid=0`; it cannot register any other endpoint.
///
/// # Errors
/// Returns `ViError::PermissionDenied` when the caller lacks matching authority, or
/// `ViError::Unknown` if the registry is full.
pub fn register(service_id: u16, tid: usize) -> ViResult<()> {
    match syscall::sys_register_service(service_id, tid) {
        syscall::SyscallResult::Ok(_) => Ok(()),
        syscall::SyscallResult::Err(_) => Err(ViError::PermissionDenied),
    }
}

// ─── ServiceRef — caching typed IPC handle ───────────────────────────────────

/// A caching handle to a well-known service.
///
/// Resolves the live provider's **binding** — `{tid, cell_id, generation}`, from the
/// kernel's own registry — on first use and caches it. When the service restarts (new
/// TID, or a new Cell incarnation), call [`invalidate`][Self::invalidate] so the next
/// [`call`][Self::call] re-resolves, or ask [`is_live`][Self::is_live] whether the
/// descriptor this handle holds is still the live one.
///
/// # Usage
/// ```no_run
/// use ostd::service::{service, ServiceRef};
/// use ostd::services::ipc::{VfsRequest, VfsResponse, IPC_BUF_SIZE};
/// use ostd::ViResult;
///
/// # fn stat() -> ViResult<()> {
/// let mut vfs: ServiceRef<{ service::VFS }> = ServiceRef::new();
/// let mut response_buffer = [0u8; IPC_BUF_SIZE];
/// let response: VfsResponse =
///     vfs.call(&VfsRequest::Stat("/tmp"), &mut response_buffer)?;
/// # let _ = response;
/// # Ok(())
/// # }
/// ```
pub struct ServiceRef<const ID: u16> {
    /// The resolved provider binding, not just its tid: a tid says where to send,
    /// never which provider incarnation is there.
    cached: Option<ServiceBinding>,
}

impl<const ID: u16> ServiceRef<ID> {
    /// Create a new, unresolved handle. Resolution happens lazily on the first [`call`][Self::call].
    pub const fn new() -> Self {
        Self { cached: None }
    }

    /// Resolve the live provider TID, retrying up to 8 times with a scheduler yield between
    /// attempts (mirrors the [`ConfigClient::endpoint`] pattern).
    ///
    /// Returns `None` during the brief death→respawn window.
    pub fn resolve(&mut self) -> Option<usize> {
        self.binding().map(|binding| binding.tid as usize)
    }

    /// Resolve the live provider's binding, retrying like [`resolve`][Self::resolve].
    ///
    /// Where [`lookup`] answers *where* to send, this answers *which provider
    /// incarnation* is there, out of the kernel's own service registry
    /// (`LookupServiceBound`). The handle caches it: the record is a descriptor, it
    /// grants nothing, and the kernel re-verifies the live binding when the tid is
    /// actually used.
    ///
    /// Returns `None` when nothing is registered, during the death→respawn window, and
    /// while the provider is hidden behind a hot-swap pause — every one of those is
    /// "no binding", and the only correct response is to refuse the call.
    pub fn binding(&mut self) -> Option<ServiceBinding> {
        if let Some(binding) = self.cached {
            return Some(binding);
        }
        let mut record = [0u8; SERVICE_BINDING_LEN];
        for _ in 0..8 {
            if let Some(binding) = syscall::sys_lookup_service_bound(ID, &mut record) {
                self.cached = Some(binding);
                return Some(binding);
            }
            crate::task::yield_now();
        }
        None
    }

    /// Whether the binding this handle holds is still the live provider binding.
    ///
    /// Compares the whole descriptor, so a provider that re-registered under a new tid —
    /// or a Cell that was replaced by a new incarnation — makes this `false`, and the
    /// stale descriptor is dropped. A handle that has not resolved yet is not live.
    ///
    /// This costs one registry lookup, which is why [`call`][Self::call] does not pay for
    /// it on the happy path: use it before work whose retry safety the caller has to
    /// decide itself, as the contract requires, rather than to re-check every message.
    pub fn is_live(&mut self) -> bool {
        let Some(held) = self.cached else {
            return false;
        };
        let mut record = [0u8; SERVICE_BINDING_LEN];
        match syscall::sys_lookup_service_bound(ID, &mut record) {
            Some(live) if live == held => true,
            _ => {
                self.cached = None;
                false
            }
        }
    }

    /// Clear the cached binding. Call this after a send error to force re-resolution on the
    /// next [`call`][Self::call] (the service may have restarted with a new TID).
    pub fn invalidate(&mut self) {
        self.cached = None;
    }

    /// Send a postcard-encoded `req` to the service and decode the response into `resp_buf`.
    ///
    /// The decoded `Resp` may borrow bytes from `resp_buf` (e.g. `VfsResponse::Data(&[u8])`).
    /// Keep `resp_buf` alive as long as you use the returned value.
    ///
    /// A send, receive or reply-identity failure is classified against the registry: if the
    /// provider this call named is no longer the live one, the call is reported as
    /// `NotFound` — refused, not delivered under a stale descriptor — and the descriptor is
    /// dropped; otherwise it is reported as `IO` against the provider that did answer.
    ///
    /// # Errors
    /// - `ViError::NotFound` — service not registered after 8 retries, or the provider
    ///   incarnation this call named is no longer the live one.
    /// - `ViError::InvalidArgument` — `req` could not be encoded (message too large).
    /// - `ViError::IO` — send or receive syscall failed, or response decoding failed,
    ///   against a provider that is still the live one.
    pub fn call<'b, Req, Resp>(
        &mut self,
        req: &Req,
        resp_buf: &'b mut [u8; IPC_BUF_SIZE],
    ) -> ViResult<Resp>
    where
        Req: serde::Serialize,
        Resp: serde::Deserialize<'b>,
    {
        let held = self.binding().ok_or(ViError::NotFound)?;
        let mut send_buf = [0u8; IPC_BUF_SIZE];
        match crate::ipc::service_call_typed::<Req, Resp>(
            held.tid as usize,
            req,
            &mut send_buf,
            resp_buf,
        ) {
            Ok(resp) => Ok(resp),
            Err(crate::ipc::IpcError::Encode) => Err(ViError::InvalidArgument),
            Err(crate::ipc::IpcError::Decode)
            | Err(crate::ipc::IpcError::InvalidOperation)
            | Err(crate::ipc::IpcError::BufferTooSmall) => Err(ViError::IO),
            Err(crate::ipc::IpcError::Send)
            | Err(crate::ipc::IpcError::Recv)
            | Err(crate::ipc::IpcError::WrongSender) => {
                let mut record = [0u8; SERVICE_BINDING_LEN];
                let live = syscall::sys_lookup_service_bound(ID, &mut record);
                let verdict = classify_call_failure(held, live);
                self.invalidate();
                match verdict {
                    CallFailure::StaleBinding => Err(ViError::NotFound),
                    CallFailure::ProviderError => Err(ViError::IO),
                }
            }
        }
    }
}

/// What a failed typed exchange means, given the binding it used and the binding the
/// kernel reports for that service now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallFailure {
    /// The provider the failed call named is no longer the live endpoint of the service:
    /// the call was refused and nothing was delivered under the stale descriptor.
    StaleBinding,
    /// The same live endpoint failed the exchange — a send, receive or reply-identity
    /// error against the provider the caller named.
    ProviderError,
}

/// Decide what a failed exchange against `held` means, now that `live` is what the
/// kernel reports for that service.
///
/// The **whole** descriptor must match, not just the Cell identity: a provider Cell can
/// re-register under a new tid without its generation changing, and the tid is what a
/// call actually addresses. Absent, changed or non-live all mean the descriptor the
/// caller held is stale.
///
/// Pure by construction: the kernel is the only source of bindings, so this rule is
/// testable without one.
pub fn classify_call_failure(held: ServiceBinding, live: Option<ServiceBinding>) -> CallFailure {
    match live {
        Some(live) if live == held => CallFailure::ProviderError,
        _ => CallFailure::StaleBinding,
    }
}

impl<const ID: u16> Default for ServiceRef<ID> {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience type alias — VFS service handle.
pub type VfsRef = ServiceRef<{ service::VFS }>;
/// Convenience type alias — net service handle.
pub type NetRef = ServiceRef<{ service::NET }>;
/// Convenience type alias — input service handle.
pub type InputRef = ServiceRef<{ service::INPUT }>;
/// Convenience type alias — config service handle.
pub type ConfigRef = ServiceRef<{ service::CONFIG }>;
/// Convenience type alias — compositor service handle.
pub type CompositorRef = ServiceRef<{ service::COMPOSITOR }>;
