// TCP helper wrappers over ViCell IPC.
// All calls are synchronous — sys_recv blocks in the kernel until the net service responds.

extern crate alloc;
use alloc::vec::Vec;

use api::ipc::{NetRequest, NetResponse, IPC_BUF_SIZE};
use ostd::clients::VfsClient;
use ostd::ipc::{service_call_typed, IpcError};
use ostd::syscall::sys_yield;

const TCP_SEND_ZERO_PROGRESS_RETRY_LIMIT: usize = 4;

fn net_call<'a>(
    net_ep: usize,
    req: &NetRequest<'_>,
    send_buf: &mut [u8; IPC_BUF_SIZE],
    recv_buf: &'a mut [u8; IPC_BUF_SIZE],
) -> Result<NetResponse<'a>, IpcError> {
    service_call_typed(net_ep, req, send_buf, recv_buf)
}

pub(crate) fn decode_net_send_progress(
    response: Result<NetResponse<'_>, IpcError>,
    chunk_len: usize,
) -> Option<usize> {
    match response {
        Ok(NetResponse::Ok) => Some(chunk_len),
        Ok(NetResponse::Data(b)) if b.len() >= 4 => {
            let mut arr = [0u8; 4];
            arr.copy_from_slice(&b[..4]);
            Some((u32::from_le_bytes(arr) as usize).min(chunk_len))
        }
        _ => None,
    }
}

pub(crate) fn map_tcp_recv_response(
    response: Result<NetResponse<'_>, IpcError>,
) -> Option<Option<&[u8]>> {
    match response {
        Ok(NetResponse::Data(data)) if !data.is_empty() => Some(Some(data)),
        Ok(NetResponse::Ok) | Ok(NetResponse::Data(_)) => Some(None),
        _ => None,
    }
}

pub(crate) fn tcp_send_all_with<F, Y>(data: &[u8], mut send_chunk: F, mut on_retry: Y) -> bool
where
    F: FnMut(&[u8]) -> Option<usize>,
    Y: FnMut(),
{
    let mut sent = 0usize;
    let mut zero_progress_retries = 0usize;
    while sent < data.len() {
        let chunk_len = (data.len() - sent).min(480);
        let chunk = &data[sent..sent + chunk_len];
        match send_chunk(chunk) {
            Some(written) if written > 0 => {
                sent += written.min(chunk_len);
                zero_progress_retries = 0;
            }
            _ => {
                zero_progress_retries += 1;
                if zero_progress_retries >= TCP_SEND_ZERO_PROGRESS_RETRY_LIMIT {
                    return false;
                }
                on_retry();
            }
        }
    }
    true
}

/// Create a listening TCP socket on `port`. Returns listen cap_id or None.
pub fn tcp_listen(port: u16, net_ep: usize) -> Option<u32> {
    let mut req = [0u8; IPC_BUF_SIZE];
    let mut resp = [0u8; IPC_BUF_SIZE];
    match net_call(net_ep, &NetRequest::TcpListen { port }, &mut req, &mut resp) {
        Ok(NetResponse::CapId(c)) => Some(c),
        _ => None,
    }
}

/// Accept one incoming connection on `listen_cap`. Returns stream cap_id or None.
pub fn tcp_accept(listen_cap: u32, net_ep: usize) -> Option<u32> {
    let mut req = [0u8; IPC_BUF_SIZE];
    let mut resp = [0u8; IPC_BUF_SIZE];
    match net_call(
        net_ep,
        &NetRequest::TcpAccept { cap_id: listen_cap },
        &mut req,
        &mut resp,
    ) {
        Ok(NetResponse::CapId(c)) => Some(c),
        _ => None,
    }
}

/// Send all of `data` to `cap` in ≤480-byte chunks (leaves room for IPC encoding overhead).
pub fn tcp_send_all(cap: u32, net_ep: usize, data: &[u8]) -> bool {
    let mut req = [0u8; IPC_BUF_SIZE];
    let mut resp = [0u8; IPC_BUF_SIZE];
    tcp_send_all_with(
        data,
        |chunk| {
            decode_net_send_progress(
                net_call(
                    net_ep,
                    &NetRequest::TcpSend {
                        cap_id: cap,
                        data: chunk,
                    },
                    &mut req,
                    &mut resp,
                ),
                chunk.len(),
            )
        },
        sys_yield,
    )
}

/// Receive incoming HTTP request bytes until `\r\n\r\n` or max 4096 bytes is reached.
pub fn recv_request(cap: u32, net_ep: usize) -> Vec<u8> {
    let mut buf: Vec<u8> = Vec::with_capacity(512);
    let mut req = [0u8; IPC_BUF_SIZE];
    let mut resp = [0u8; IPC_BUF_SIZE];
    let recv_req = NetRequest::TcpRecv {
        cap_id: cap,
        buf_len: 256,
    };
    for _ in 0..200 {
        if buf.len() > 4096 {
            break;
        }
        match map_tcp_recv_response(net_call(net_ep, &recv_req, &mut req, &mut resp)) {
            Some(Some(data)) => {
                buf.extend_from_slice(data);
                if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            Some(None) => {
                sys_yield();
            }
            None => break,
        }
    }
    buf
}

/// Close a TCP connection.
pub fn tcp_close(cap: u32, net_ep: usize) {
    let mut req = [0u8; IPC_BUF_SIZE];
    let mut resp = [0u8; IPC_BUF_SIZE];
    let _ = net_call(
        net_ep,
        &NetRequest::TcpClose { cap_id: cap },
        &mut req,
        &mut resp,
    );
}

/// List a VFS directory. Returns newline-separated names or empty.
pub fn vfs_list_dir(path: &str, _vfs_ep: usize) -> Vec<u8> {
    let mut vfs = VfsClient::new();
    vfs.list_dir(path).unwrap_or_default()
}
