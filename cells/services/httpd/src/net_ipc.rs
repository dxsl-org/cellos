//! Owned, nonblocking IPC submissions for the HTTP reactor.
//! Every request is copied into the kernel's bounded operation queue before returning.

use api::ipc::{NetRequest, NetResponse, VfsRequest, VfsResponse, IPC_BUF_SIZE};
use ostd::ipc::{self, IpcOpId, IpcSubmitError, IpcTakeResult, IpcTerminal};
use serde::Serialize;

pub(crate) type Op = IpcOpId;

pub(crate) fn submit<T: Serialize>(peer: usize, req: &T) -> Result<Op, IpcSubmitError> {
    let mut bytes = [0u8; IPC_BUF_SIZE];
    let encoded = api::ipc::encode(req, &mut bytes).map_err(|_| IpcSubmitError::InvalidRequest)?;
    ipc::submit(peer, encoded)
}

/// A terminal operation is removed from kernel storage on successful take.
/// The borrowed response must not outlive `reply`.
pub(crate) fn take<'a>(op: Op, reply: &'a mut [u8; IPC_BUF_SIZE]) -> Option<Result<&'a [u8], IpcTerminal>> {
    match ipc::take(op, reply) {
        Ok(IpcTakeResult::Pending) => None,
        Ok(IpcTakeResult::Terminal { status: IpcTerminal::Reply, len }) if len <= reply.len() => Some(Ok(&reply[..len])),
        Ok(IpcTakeResult::Terminal { status, .. }) => Some(Err(status)),
        Err(_) => Some(Err(IpcTerminal::Indeterminate)),
    }
}

pub(crate) fn net<'a>(reply: &'a [u8]) -> Option<NetResponse<'a>> {
    api::ipc::decode(reply).ok()
}

pub(crate) fn vfs<'a>(reply: &'a [u8]) -> Option<VfsResponse<'a>> {
    api::ipc::decode(reply).ok()
}

pub(crate) fn send_net(peer: usize, req: &NetRequest<'_>) -> Result<Op, IpcSubmitError> {
    submit(peer, req)
}

pub(crate) fn send_vfs(peer: usize, req: &VfsRequest<'_>) -> Result<Op, IpcSubmitError> {
    submit(peer, req)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestReadError {
    Incomplete,
    BadRequest,
    TooLarge,
}

pub(crate) const MAX_HTTP_REQUEST_BYTES: usize = 4096;

/// Exactly one HTTP/1.1 request, never dispatch a partially received body.
pub(crate) fn request_complete_len(buf: &[u8]) -> Result<Option<usize>, RequestReadError> {
    let Some(header_end) = buf.windows(4).position(|part| part == b"\r\n\r\n") else {
        return Ok(None);
    };
    let header_end = header_end + 4;
    let mut headers = [httparse::EMPTY_HEADER; 16];
    let mut request = httparse::Request::new(&mut headers);
    if !matches!(request.parse(&buf[..header_end]), Ok(httparse::Status::Complete(_))) {
        return Err(RequestReadError::BadRequest);
    }
    let mut body_len = None;
    for header in request.headers.iter() {
        if header.name.eq_ignore_ascii_case("Transfer-Encoding") {
            return Err(RequestReadError::BadRequest);
        }
        if header.name.eq_ignore_ascii_case("Content-Length") {
            if body_len.is_some() {
                return Err(RequestReadError::BadRequest);
            }
            let value = core::str::from_utf8(header.value).map_err(|_| RequestReadError::BadRequest)?;
            body_len = Some(value.trim().parse::<usize>().map_err(|_| RequestReadError::BadRequest)?);
        }
    }
    let expected = header_end.checked_add(body_len.unwrap_or(0)).ok_or(RequestReadError::TooLarge)?;
    if expected > MAX_HTTP_REQUEST_BYTES {
        return Err(RequestReadError::TooLarge);
    }
    Ok(Some(expected))
}

#[cfg(test)]
mod framing_tests {
    use super::{request_complete_len, RequestReadError};

    #[test]
    fn fragmented_header_and_body_remain_incomplete() {
        let header = b"POST /api/infer HTTP/1.1\r\nContent-Length: 5\r\n\r\n";
        assert_eq!(request_complete_len(&header[..header.len() - 1]), Ok(None));
        assert_eq!(request_complete_len(header), Ok(Some(header.len() + 5)));
        let mut partial = header.to_vec();
        partial.extend_from_slice(b"abc");
        assert_eq!(request_complete_len(&partial), Ok(Some(header.len() + 5)));
    }

    #[test]
    fn conflicting_framing_never_dispatches() {
        for header in [
            b"POST / HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\nx".as_slice(),
            b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n".as_slice(),
            b"POST / HTTP/1.1\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\nx".as_slice(),
            b"POST / HTTP/1.1\r\nContent-Length: -2\r\n\r\n".as_slice(),
        ] {
            assert_eq!(request_complete_len(header), Err(RequestReadError::BadRequest));
        }
    }

    #[test]
    fn oversized_or_overflowing_length_is_rejected_before_dispatch() {
        let huge = b"POST / HTTP/1.1\r\nContent-Length: 18446744073709551615\r\n\r\n";
        assert_eq!(request_complete_len(huge), Err(RequestReadError::TooLarge));
        let big = b"POST / HTTP/1.1\r\nContent-Length: 4096\r\n\r\n";
        assert_eq!(request_complete_len(big), Err(RequestReadError::TooLarge));
    }
}
