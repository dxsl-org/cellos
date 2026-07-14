// TCP helper wrappers over ViCell IPC.
// All calls are synchronous — sys_recv blocks in the kernel until the net service responds.

extern crate alloc;
use alloc::vec::Vec;

use api::ipc::{NetRequest, NetResponse, IPC_BUF_SIZE};
use ostd::syscall::{sys_recv, sys_send, sys_yield, SyscallResult};

/// Create a listening TCP socket on `port`. Returns listen cap_id or None.
pub fn tcp_listen(port: u16, net_ep: usize) -> Option<u32> {
    let mut req = [0u8; IPC_BUF_SIZE];
    let mut resp = [0u8; IPC_BUF_SIZE];
    let encoded = api::ipc::encode(&NetRequest::TcpListen { port }, &mut req).ok()?;
    sys_send(net_ep, encoded);
    match sys_recv(0, &mut resp) {
        SyscallResult::Ok(_) => match api::ipc::decode::<NetResponse>(&resp) {
            Ok(NetResponse::CapId(c)) => Some(c),
            _ => None,
        },
        _ => None,
    }
}

/// Accept one incoming connection on `listen_cap`. Returns stream cap_id or None.
pub fn tcp_accept(listen_cap: u32, net_ep: usize) -> Option<u32> {
    let mut req = [0u8; IPC_BUF_SIZE];
    let mut resp = [0u8; IPC_BUF_SIZE];
    let encoded = api::ipc::encode(&NetRequest::TcpAccept { cap_id: listen_cap }, &mut req).ok()?;
    sys_send(net_ep, encoded);
    match sys_recv(0, &mut resp) {
        SyscallResult::Ok(_) => match api::ipc::decode::<NetResponse>(&resp) {
            Ok(NetResponse::CapId(c)) => Some(c),
            _ => None,
        },
        _ => None,
    }
}

/// Send all of `data` to `cap` in ≤480-byte chunks (leaves room for IPC encoding overhead).
pub fn tcp_send_all(cap: u32, net_ep: usize, data: &[u8]) {
    let mut sent = 0usize;
    let mut req = [0u8; IPC_BUF_SIZE];
    let mut resp = [0u8; IPC_BUF_SIZE];
    while sent < data.len() {
        let chunk_len = (data.len() - sent).min(480);
        let chunk = &data[sent..sent + chunk_len];
        let n = match api::ipc::encode(
            &NetRequest::TcpSend {
                cap_id: cap,
                data: chunk,
            },
            &mut req,
        ) {
            Ok(b) => b.len(),
            Err(_) => break,
        };
        sys_send(net_ep, &req[..n]);
        match sys_recv(0, &mut resp) {
            SyscallResult::Ok(_) => match api::ipc::decode::<NetResponse>(&resp) {
                Ok(NetResponse::Ok) => {
                    sent += chunk_len;
                }
                // Net service may return Data([count as 4 LE bytes]) on some builds.
                Ok(NetResponse::Data(b)) if b.len() >= 4 => {
                    let mut arr = [0u8; 4];
                    arr.copy_from_slice(&b[..4]);
                    sent += (u32::from_le_bytes(arr) as usize).min(chunk_len);
                }
                _ => break,
            },
            _ => break,
        }
    }
}

/// Receive incoming HTTP request bytes until `\r\n\r\n` or max 4096 bytes is reached.
pub fn recv_request(cap: u32, net_ep: usize) -> Vec<u8> {
    let mut buf: Vec<u8> = Vec::with_capacity(512);
    let mut req = [0u8; IPC_BUF_SIZE];
    let mut resp = [0u8; IPC_BUF_SIZE];
    let n = match api::ipc::encode(
        &NetRequest::TcpRecv {
            cap_id: cap,
            buf_len: 256,
        },
        &mut req,
    ) {
        Ok(b) => b.len(),
        Err(_) => return buf,
    };
    for _ in 0..200 {
        if buf.len() > 4096 {
            break;
        }
        sys_send(net_ep, &req[..n]);
        match sys_recv(0, &mut resp) {
            SyscallResult::Ok(_) => match api::ipc::decode::<NetResponse>(&resp) {
                Ok(NetResponse::Data(data)) if !data.is_empty() => {
                    buf.extend_from_slice(data);
                    if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                Ok(NetResponse::Ok) | Ok(NetResponse::Data(_)) => {
                    sys_yield();
                }
                _ => break,
            },
            _ => break,
        }
    }
    buf
}

/// Close a TCP connection.
pub fn tcp_close(cap: u32, net_ep: usize) {
    let mut req = [0u8; IPC_BUF_SIZE];
    let mut resp = [0u8; IPC_BUF_SIZE];
    if let Ok(encoded) = api::ipc::encode(&NetRequest::TcpClose { cap_id: cap }, &mut req) {
        sys_send(net_ep, encoded);
        let _ = sys_recv(0, &mut resp);
    }
}

/// Read a VFS file using ReadAsync + Poll. Returns file bytes or empty on error.
pub fn vfs_read_file(path: &str, vfs_ep: usize) -> Vec<u8> {
    use api::ipc::{VfsRequest, VfsResponse};
    let mut ipc = [0u8; IPC_BUF_SIZE];

    let n = match api::ipc::encode(&VfsRequest::ReadAsync { path }, &mut ipc) {
        Ok(b) => b.len(),
        Err(_) => return Vec::new(),
    };
    sys_send(vfs_ep, &ipc[..n]);
    let handle = match sys_recv(0, &mut ipc) {
        SyscallResult::Ok(_) => match api::ipc::decode::<VfsResponse>(&ipc) {
            Ok(VfsResponse::PendingHandle(h)) => h,
            _ => return Vec::new(),
        },
        _ => return Vec::new(),
    };

    let n = match api::ipc::encode(&VfsRequest::Poll { handle }, &mut ipc) {
        Ok(b) => b.len(),
        Err(_) => return Vec::new(),
    };
    sys_send(vfs_ep, &ipc[..n]);
    match sys_recv(0, &mut ipc) {
        SyscallResult::Ok(_) => match api::ipc::decode::<VfsResponse>(&ipc) {
            Ok(VfsResponse::Data(data)) => data.to_vec(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// List a VFS directory. Returns newline-separated names or empty.
pub fn vfs_list_dir(path: &str, vfs_ep: usize) -> Vec<u8> {
    use api::ipc::{VfsRequest, VfsResponse};
    let mut ipc = [0u8; IPC_BUF_SIZE];

    let n = match api::ipc::encode(&VfsRequest::ListDir(path), &mut ipc) {
        Ok(b) => b.len(),
        Err(_) => return Vec::new(),
    };
    sys_send(vfs_ep, &ipc[..n]);
    match sys_recv(0, &mut ipc) {
        SyscallResult::Ok(_) => match api::ipc::decode::<VfsResponse>(&ipc) {
            Ok(VfsResponse::Data(data)) => data.to_vec(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}
