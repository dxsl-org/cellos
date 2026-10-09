// SPDX-License-Identifier: MPL-2.0
//! Typed local and remote Cell endpoint descriptors.
//!
//! Local calls use direct sender-masked IPC. Remote endpoints carry only
//! authenticated route metadata; Phase 04 deliberately exposes no transmit
//! method while remote dispatch remains disabled.

use api::ipc::IPC_BUF_SIZE;
use api::service_binding::{ServiceBinding, SERVICE_BINDING_LEN};
use api::services::cluster::{CellNetId, ClusterId};
use core::marker::PhantomData;
use serde::{Deserialize, Serialize};
use types::c2c::{RelativeDeadline, RetryClass, ServerEpoch};

use crate::{ipc, ViError, ViResult};

/// Typed request/response contract shared by local and remote endpoints.
pub trait CellMethod {
    /// Serialized request type for this method.
    type Request: Serialize;
    /// Response type, optionally borrowing from the caller's receive buffer.
    type Response<'a>: Deserialize<'a>;

    /// Stable remote service identifier.
    const SERVICE_ID: u16;
    /// Stable exported-method identifier within the service.
    const EXPORT_ID: u16;
    /// Retry safety applied to transport loss or indeterminate completion.
    const RETRY_CLASS: RetryClass;
}

/// Invalid endpoint metadata that cannot identify a routable target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointError {
    InvalidLocalTid,
    InvalidRemoteIdentity,
    /// No live provider binding exists for the method's service.
    NoLiveBinding,
}

/// Observable remote-call outcomes; never collapse these into local IPC errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteCallError {
    NoService,
    Unreachable,
    Timeout,
    Busy,
    Indeterminate,
    AuthFailed,
    ProtocolError,
    NotSupported,
}

/// Direct local endpoint. Calls never resolve or contact the net-broker.
pub struct LocalEndpoint<M: CellMethod> {
    tid: usize,
    /// The provider binding this endpoint resolved, when it was created by
    /// [`LocalEndpoint::bind`]. A tid-only endpoint has none, and then this type makes
    /// no claim about the provider behind the tid.
    binding: Option<ServiceBinding>,
    marker: PhantomData<fn() -> M>,
}

impl<M: CellMethod> LocalEndpoint<M> {
    /// Bind a typed endpoint to `tid`, rejecting the reserved zero TID.
    ///
    /// This trusts a caller-supplied tid: it resolves nothing, so the endpoint carries no
    /// provider identity and a failed exchange is reported as `IO` rather than classified
    /// against the registry. Use [`bind`][Self::bind] when the service identity matters.
    pub const fn new(tid: usize) -> Result<Self, EndpointError> {
        if tid == 0 {
            return Err(EndpointError::InvalidLocalTid);
        }
        Ok(Self {
            tid,
            binding: None,
            marker: PhantomData,
        })
    }

    /// Resolve the method's service to its live provider binding and bind to it.
    ///
    /// Where [`new`][Self::new] trusts a caller-supplied tid, this asks the kernel which
    /// provider incarnation is live for `M::SERVICE_ID` and refuses when there is none —
    /// no binding means the call is refused, never sent to a tid that may belong to a
    /// dead incarnation. The endpoint then carries that identity, so a later exchange
    /// against a provider that has since been replaced is reported as `NotFound` instead
    /// of a generic I/O error.
    ///
    /// # Errors
    /// - `EndpointError::NoLiveBinding` — nothing is registered for `M::SERVICE_ID`, or
    ///   the provider is paused behind a hot-swap: absent bindings are refusals, not
    ///   retries.
    pub fn bind() -> Result<Self, EndpointError> {
        let mut record = [0u8; SERVICE_BINDING_LEN];
        match crate::syscall::sys_lookup_service_bound(M::SERVICE_ID, &mut record) {
            Some(binding) => Ok(Self {
                tid: binding.tid as usize,
                binding: Some(binding),
                marker: PhantomData,
            }),
            None => Err(EndpointError::NoLiveBinding),
        }
    }

    /// Return the direct local service TID.
    pub const fn tid(&self) -> usize {
        self.tid
    }

    /// The provider binding this endpoint resolved, or `None` for a tid-only endpoint.
    pub const fn binding(&self) -> Option<ServiceBinding> {
        self.binding
    }

    /// Execute one typed request/reply exchange directly with the local TID.
    ///
    /// The returned value may borrow from `response_buffer`.
    ///
    /// # Errors
    /// Returns `InvalidArgument` for an oversized request and `IO` for send,
    /// receive, wrong-sender, or decode failures. An endpoint bound with
    /// [`bind`][Self::bind] additionally reports `NotFound` when the provider it resolved
    /// is no longer the live one: the exchange was refused, not delivered under a stale
    /// descriptor.
    pub fn call<'a>(
        &self,
        request: &M::Request,
        response_buffer: &'a mut [u8; IPC_BUF_SIZE],
    ) -> ViResult<M::Response<'a>> {
        let mut send_buffer = [0u8; IPC_BUF_SIZE];
        match ipc::service_call_typed(self.tid, request, &mut send_buffer, response_buffer) {
            Ok(response) => Ok(response),
            Err(ipc::IpcError::Encode) => Err(ViError::InvalidArgument),
            Err(ipc::IpcError::Decode)
            | Err(ipc::IpcError::InvalidOperation)
            | Err(ipc::IpcError::BufferTooSmall) => Err(ViError::IO),
            Err(ipc::IpcError::Busy)
            | Err(ipc::IpcError::PreDispatchTimeout)
            | Err(ipc::IpcError::Indeterminate)
            | Err(ipc::IpcError::Cancelled) => Err(ViError::IO),
            Err(ipc::IpcError::Send)
            | Err(ipc::IpcError::Recv)
            | Err(ipc::IpcError::WrongSender)
            | Err(ipc::IpcError::PeerGone) => match self.binding {
                // A tid-only endpoint knows no provider identity, so it cannot tell a
                // replaced provider from a failed one and must not guess.
                None => Err(ViError::IO),
                Some(held) => {
                    let mut record = [0u8; SERVICE_BINDING_LEN];
                    let live = crate::syscall::sys_lookup_service_bound(M::SERVICE_ID, &mut record);
                    match crate::service::classify_call_failure(held, live) {
                        crate::service::CallFailure::StaleBinding => Err(ViError::NotFound),
                        crate::service::CallFailure::ProviderError => Err(ViError::IO),
                    }
                }
            },
        }
    }
}

/// Authenticated metadata for one remote exported-server incarnation.
pub struct RemoteEndpoint<M: CellMethod> {
    destination: CellNetId,
    cluster: ClusterId,
    server_epoch: ServerEpoch,
    marker: PhantomData<fn() -> M>,
}

impl<M: CellMethod> RemoteEndpoint<M> {
    /// Construct a remote descriptor learned from authenticated discovery.
    ///
    /// # Errors
    /// Rejects zero node, cluster, service, or export identities.
    pub const fn new(
        destination: CellNetId,
        cluster: ClusterId,
        server_epoch: ServerEpoch,
    ) -> Result<Self, EndpointError> {
        if all_zero(&destination.0) || cluster.0 == 0 || M::SERVICE_ID == 0 || M::EXPORT_ID == 0 {
            return Err(EndpointError::InvalidRemoteIdentity);
        }
        Ok(Self {
            destination,
            cluster,
            server_epoch,
            marker: PhantomData,
        })
    }

    /// Refuse remote transmission while the protected-provider and dispatch
    /// gates remain closed.
    ///
    /// The validated relative deadline is mandatory, but no argument is encoded
    /// or sent. This boundary preserves the final typed return shape without
    /// contacting the net-broker.
    ///
    /// # Errors
    /// Always returns `RemoteCallError::NotSupported` in Phase 04.
    pub fn call<'a>(
        &self,
        _request: &M::Request,
        _relative_deadline: RelativeDeadline,
        _response_buffer: &'a mut [u8; IPC_BUF_SIZE],
    ) -> Result<M::Response<'a>, RemoteCallError> {
        Err(RemoteCallError::NotSupported)
    }

    /// Return the authenticated destination node.
    pub const fn destination(&self) -> CellNetId {
        self.destination
    }

    /// Return the destination cluster used for routing.
    pub const fn cluster(&self) -> ClusterId {
        self.cluster
    }

    /// Return the observed live server incarnation.
    pub const fn server_epoch(&self) -> ServerEpoch {
        self.server_epoch
    }

    /// Return the method's stable service identifier.
    pub const fn service_id(&self) -> u16 {
        M::SERVICE_ID
    }

    /// Return the method's stable export identifier.
    pub const fn export_id(&self) -> u16 {
        M::EXPORT_ID
    }

    /// Return the method's declared retry safety.
    pub const fn retry_class(&self) -> RetryClass {
        M::RETRY_CLASS
    }
}

/// Deliberate locality union. Callers must match before invoking local IPC or
/// constructing a future remote envelope.
pub enum CellEndpoint<M: CellMethod> {
    Local(LocalEndpoint<M>),
    Remote(RemoteEndpoint<M>),
}

const fn all_zero(bytes: &[u8; 32]) -> bool {
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != 0 {
            return false;
        }
        index += 1;
    }
    true
}
