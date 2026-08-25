//! Strict fail-closed parser and encoders for raw TLS IPC wire format.

extern crate alloc;

use alloc::vec::Vec;

pub const TLS_CLOSE_OP: u8 = 0x15;
pub const TLS_CONNECT_OP: u8 = 0x30;
pub const TLS_SEND_OP: u8 = 0x31;
pub const TLS_RECV_OP: u8 = 0x32;

pub const MAX_RAW_TLS_SEND: usize = 501;
pub const MAX_TLS_RECV_DATA: usize = 4094;
#[derive(Debug, PartialEq, Eq)]
pub enum RawTlsRequest<'a> {
    Close {
        cap: u64,
    },
    Connect {
        addr: [u8; 4],
        port: u16,
        hostname: &'a str,
    },
    Send {
        cap: u64,
        data: &'a [u8],
    },
    Recv {
        cap: u64,
        buf_len: usize,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum RawTlsError {
    BufferTooShort,
    UnknownOpcode(u8),
    InvalidLength,
    OversizePayload(usize),
    InvalidHostname,
}

pub fn parse_raw_tls_request(buf: &[u8]) -> Result<RawTlsRequest<'_>, RawTlsError> {
    if buf.len() < 9 {
        return Err(RawTlsError::BufferTooShort);
    }
    let opcode = buf[0];
    let cap = u64::from_le_bytes(
        buf[1..9]
            .try_into()
            .map_err(|_| RawTlsError::BufferTooShort)?,
    );

    match opcode {
        TLS_CLOSE_OP => Ok(RawTlsRequest::Close { cap }),
        TLS_CONNECT_OP => {
            if buf.len() < 17 {
                return Err(RawTlsError::BufferTooShort);
            }
            let addr = [buf[9], buf[10], buf[11], buf[12]];
            let port = u16::from_le_bytes([buf[13], buf[14]]);
            let hn_len = u16::from_le_bytes([buf[15], buf[16]]) as usize;
            if hn_len > 495 {
                return Err(RawTlsError::OversizePayload(hn_len));
            }
            if 17 + hn_len > buf.len() {
                return Err(RawTlsError::InvalidLength);
            }
            let hostname = core::str::from_utf8(&buf[17..17 + hn_len])
                .map_err(|_| RawTlsError::InvalidHostname)?;
            Ok(RawTlsRequest::Connect {
                addr,
                port,
                hostname,
            })
        }
        TLS_SEND_OP => {
            if buf.len() < 11 {
                return Err(RawTlsError::BufferTooShort);
            }
            let data_len = u16::from_le_bytes([buf[9], buf[10]]) as usize;
            if data_len > MAX_RAW_TLS_SEND {
                return Err(RawTlsError::OversizePayload(data_len));
            }
            if 11 + data_len > buf.len() {
                return Err(RawTlsError::InvalidLength);
            }
            Ok(RawTlsRequest::Send {
                cap,
                data: &buf[11..11 + data_len],
            })
        }
        TLS_RECV_OP => {
            if buf.len() < 13 {
                return Err(RawTlsError::BufferTooShort);
            }
            let want = u32::from_le_bytes(
                buf[9..13]
                    .try_into()
                    .map_err(|_| RawTlsError::BufferTooShort)?,
            ) as usize;
            Ok(RawTlsRequest::Recv {
                cap,
                buf_len: want.min(MAX_TLS_RECV_DATA),
            })
        }
        other => Err(RawTlsError::UnknownOpcode(other)),
    }
}

pub fn encode_tls_recv_reply(data: &[u8]) -> Vec<u8> {
    let bounded_data = if data.len() > MAX_TLS_RECV_DATA {
        &data[..MAX_TLS_RECV_DATA]
    } else {
        data
    };
    let mut resp = alloc::vec![0u8; 2 + bounded_data.len()];
    resp[0..2].copy_from_slice(&(bounded_data.len() as u16).to_le_bytes());
    resp[2..].copy_from_slice(bounded_data);
    resp
}

#[cfg(test)]
mod tests;
