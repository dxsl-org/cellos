use api::ipc::{NetRequest, NetResponse};
use ostd::service::NetRef;
use ostd::{ViError, ViResult};

/// Send `message` prefixed with a two-byte little-endian length.
pub(super) fn write_message(net: &mut NetRef, cap_id: u32, message: &[u8]) -> ViResult<()> {
    let length = (message.len() as u16).to_le_bytes();
    let mut response = [0u8; api::ipc::IPC_BUF_SIZE];
    net.call::<NetRequest, NetResponse>(
        &NetRequest::TcpSend {
            cap_id,
            data: &length,
        },
        &mut response,
    )
    .map_err(|_| ViError::IO)?;
    net.call::<NetRequest, NetResponse>(
        &NetRequest::TcpSend {
            cap_id,
            data: message,
        },
        &mut response,
    )
    .map_err(|_| ViError::IO)?;
    Ok(())
}

/// Receive one length-prefixed message into `buffer`.
pub(super) fn read_message(net: &mut NetRef, cap_id: u32, buffer: &mut [u8]) -> ViResult<usize> {
    let mut response = [0u8; api::ipc::IPC_BUF_SIZE];
    let header = match net
        .call::<NetRequest, NetResponse>(&NetRequest::TcpRecv { cap_id, buf_len: 2 }, &mut response)
        .map_err(|_| ViError::IO)?
    {
        NetResponse::Data(data) => data,
        _ => return Err(ViError::IO),
    };
    if header.len() < 2 {
        return Err(ViError::IO);
    }
    let message_len = u16::from_le_bytes([header[0], header[1]]) as usize;
    if message_len > buffer.len() {
        return Err(ViError::IO);
    }
    let payload = match net
        .call::<NetRequest, NetResponse>(
            &NetRequest::TcpRecv {
                cap_id,
                buf_len: message_len as u32,
            },
            &mut response,
        )
        .map_err(|_| ViError::IO)?
    {
        NetResponse::Data(data) => data,
        _ => return Err(ViError::IO),
    };
    let received = payload.len().min(message_len);
    buffer[..received].copy_from_slice(&payload[..received]);
    Ok(received)
}
