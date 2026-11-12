// SPDX-License-Identifier: MPL-2.0

//! Service discovery helpers for ViCell cells.
//!
//! Two levels of abstraction:
//! - Free functions [`lookup`] / [`register`] — thin wrappers for one-shot calls.
//! - [`ServiceRef<const ID>`] — a caching handle with a typed [`ServiceRef::call`] method
//!   for cells that talk to the same service repeatedly.
//!
//! [`ServiceRef`] resolves the provider's **binding** (`{tid, cell_id, generation}`) from
//! the kernel's registry rather than a bare tid. A call returns a [`CallOutcome`]: a reply,
//! a refusal (the request never ran) or an uncertain result (it may have run). A failed
//! exchange against a provider that has since been replaced is reported as
//! [`Uncertainty::ProviderReplaced`] instead of quietly re-targeting the call.

#[cfg(doc)]
use crate::call_outcome::Uncertainty;
use crate::call_outcome::{self, CallOutcome, Refusal};
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

/// The [`api::syscall::service_flags`] the namespace owner (init, and the
/// supervisor when it rolls a hot swap back) registers `service_id` with.
///
/// VFS keeps its outer caller across nested IPC and watches the owners it
/// serves; NET, the compositor and the Intel GPU driver watch the owners whose
/// resources they reclaim. Each entry is an owner decision with its own reclaim
/// consumer.
pub const fn namespace_flags(service_id: u16) -> usize {
    use api::syscall::service_flags::{ALL, OWNER_WATCH};
    match service_id {
        service::VFS => ALL,
        service::NET | service::COMPOSITOR | service::XE_GPU => OWNER_WATCH,
        _ => 0,
    }
}

/// Register the calling cell as the live provider of `service_id`.
///
/// Well-known ids require trusted init or `SupervisorCap`; a `SpawnCap`-only cell may
/// publish only a private id it owns. Kernel-defined, operation-specific self-registration
/// capabilities are separate exceptions: the development/test-hooks Silo route is one and can
/// publish only `service::SILO` with `tid=0`; it cannot register any other endpoint.
///
/// # Errors
/// Returns `ViError::PermissionDenied` when the caller lacks matching authority, or
/// `ViError::Unknown` if the registry is full.
pub fn register(service_id: u16, tid: usize) -> ViResult<()> {
    match syscall::sys_register_service(service_id, tid, 0) {
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
/// let response: VfsResponse = vfs
///     .call(&VfsRequest::Stat("/tmp"), &mut response_buffer)
///     .into_result()?;
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
    /// The outcome says whether the request may have run: [`Refused`][CallOutcome::Refused]
    /// never executed, [`Uncertain`][CallOutcome::Uncertain] might have. Lifecycle failures
    /// are classified against the registry, so a provider that was replaced is reported as
    /// such, and the cached descriptor is dropped on every lifecycle failure. This method
    /// never retries a request after acceptance. Use [`CallOutcome::into_result`] when only
    /// an error code is needed.
    pub fn call<'b, Req, Resp>(
        &mut self,
        req: &Req,
        resp_buf: &'b mut [u8; IPC_BUF_SIZE],
    ) -> CallOutcome<Resp>
    where
        Req: serde::Serialize,
        Resp: serde::Deserialize<'b>,
    {
        let Some(held) = self.binding() else {
            return CallOutcome::Refused(Refusal::NoBinding);
        };
        let mut send_buf = [0u8; IPC_BUF_SIZE];
        match crate::ipc::service_call_typed::<Req, Resp>(
            held.tid as usize,
            req,
            &mut send_buf,
            resp_buf,
        ) {
            Ok(resp) => CallOutcome::Reply(resp),
            Err(error) if call_outcome::needs_binding_check(error) => {
                let mut record = [0u8; SERVICE_BINDING_LEN];
                let live = syscall::sys_lookup_service_bound(ID, &mut record);
                let stale = classify_call_failure(held, live) == CallFailure::StaleBinding;
                self.invalidate();
                call_outcome::classify(error, stale).into()
            }
            Err(error) => call_outcome::classify(error, false).into(),
        }
    }
}

/// What a failed typed exchange means, given the binding it used and the binding the
/// kernel reports for that service now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallFailure {
    /// The named provider is no longer the live endpoint. This is a binding verdict,
    /// not a claim that the failed operation was refused or had no side effects.
    StaleBinding,
    /// The same live endpoint failed the exchange or the operation expired.
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

/// Whether `sender_tid` — the tid the kernel stamped on a received message — is the
/// live provider of `service_id` right now.
///
/// For a cell that serves exactly one peer service (a NIC driver serves only the net
/// service): it asks the kernel's registry on every call instead of trusting a cached
/// tid, so a restarted or hot-swapped provider is followed and a provider that is
/// not yet registered (init registers a service after spawning it) is refused until
/// it is. Never pass a tid taken from a message payload.
pub fn sender_is_live_provider(service_id: u16, sender_tid: usize) -> bool {
    let mut record = [0u8; SERVICE_BINDING_LEN];
    binding_names_sender(
        syscall::sys_lookup_service_bound(service_id, &mut record),
        sender_tid,
    )
}

/// Pure rule behind [`sender_is_live_provider`]: the kernel's live binding must exist,
/// name a live provider, and address exactly `sender_tid`. Tids are never re-issued
/// within one boot, so a matching tid is that provider's task and nobody else's.
pub fn binding_names_sender(live: Option<ServiceBinding>, sender_tid: usize) -> bool {
    matches!(live, Some(binding) if binding.is_live() && binding.tid == sender_tid as u64)
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

#[cfg(test)]
mod tests {
    use super::{namespace_flags, service};
    use api::syscall::service_flags::{ALL, OWNER_WATCH};

    /// Tripwire: which services the namespace owner flags is an owner decision.
    /// Adding one is a reviewed change with its own reclaim consumer and kernel
    /// grant coverage, never a silent widening.
    #[test]
    fn namespace_flags_are_an_explicit_table() {
        let flagged: alloc::vec::Vec<(u16, usize)> = (0..=u16::MAX)
            .map(|id| (id, namespace_flags(id)))
            .filter(|&(_, flags)| flags != 0)
            .collect();
        assert_eq!(
            flagged,
            [
                (service::VFS, ALL),
                (service::NET, OWNER_WATCH),
                (service::COMPOSITOR, OWNER_WATCH),
                (service::XE_GPU, OWNER_WATCH),
            ]
        );
    }
}
