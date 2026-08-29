// SPDX-License-Identifier: MPL-2.0
//! Typed local and remote Cell endpoint descriptors.
//!
//! Local calls use direct sender-masked IPC. Remote endpoints carry only
//! authenticated route metadata; Phase 04 deliberately exposes no transmit
//! method while remote dispatch remains disabled.

use api::ipc::IPC_BUF_SIZE;
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
    marker: PhantomData<fn() -> M>,
}

impl<M: CellMethod> LocalEndpoint<M> {
    /// Bind a typed endpoint to `tid`, rejecting the reserved zero TID.
    pub const fn new(tid: usize) -> Result<Self, EndpointError> {
        if tid == 0 {
            return Err(EndpointError::InvalidLocalTid);
        }
        Ok(Self {
            tid,
            marker: PhantomData,
        })
    }

    /// Return the direct local service TID.
    pub const fn tid(&self) -> usize {
        self.tid
    }

    /// Execute one typed request/reply exchange directly with the local TID.
    ///
    /// The returned value may borrow from `response_buffer`.
    ///
    /// # Errors
    /// Returns `InvalidArgument` for an oversized request and `IO` for send,
    /// receive, wrong-sender, or decode failures.
    pub fn call<'a>(
        &self,
        request: &M::Request,
        response_buffer: &'a mut [u8; IPC_BUF_SIZE],
    ) -> ViResult<M::Response<'a>> {
        let mut send_buffer = [0u8; IPC_BUF_SIZE];
        match ipc::service_call_typed(self.tid, request, &mut send_buffer, response_buffer) {
            Ok(response) => Ok(response),
            Err(ipc::IpcError::Encode) => Err(ViError::InvalidArgument),
            Err(ipc::IpcError::Send)
            | Err(ipc::IpcError::Recv)
            | Err(ipc::IpcError::WrongSender)
            | Err(ipc::IpcError::Decode) => Err(ViError::IO),
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
