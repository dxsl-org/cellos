//! Strict fail-closed parser and encoders for the raw TLS IPC wire
//! ([`wire::proto::tls`]).

extern crate alloc;

use alloc::vec::Vec;
use wire::proto::tls;

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
    if buf.len() < tls::HEADER_LEN {
        return Err(RawTlsError::BufferTooShort);
    }
    let opcode = buf[0];
    let cap = u64::from_le_bytes(
        buf[1..tls::HEADER_LEN]
            .try_into()
            .map_err(|_| RawTlsError::BufferTooShort)?,
    );

    match opcode {
        tls::CLOSE => Ok(RawTlsRequest::Close { cap }),
        tls::CONNECT => {
            if buf.len() < tls::CONNECT_HEADER_LEN {
                return Err(RawTlsError::BufferTooShort);
            }
            let addr = [buf[9], buf[10], buf[11], buf[12]];
            let port = u16::from_le_bytes([buf[13], buf[14]]);
            let hn_len = u16::from_le_bytes([buf[15], buf[16]]) as usize;
            if hn_len > tls::MAX_HOSTNAME {
                return Err(RawTlsError::OversizePayload(hn_len));
            }
            if tls::CONNECT_HEADER_LEN + hn_len > buf.len() {
                return Err(RawTlsError::InvalidLength);
            }
            let hostname = core::str::from_utf8(
                &buf[tls::CONNECT_HEADER_LEN..tls::CONNECT_HEADER_LEN + hn_len],
            )
            .map_err(|_| RawTlsError::InvalidHostname)?;
            Ok(RawTlsRequest::Connect {
                addr,
                port,
                hostname,
            })
        }
        tls::SEND => {
            if buf.len() < tls::SEND_HEADER_LEN {
                return Err(RawTlsError::BufferTooShort);
            }
            let data_len = u16::from_le_bytes([buf[9], buf[10]]) as usize;
            if data_len > tls::MAX_SEND {
                return Err(RawTlsError::OversizePayload(data_len));
            }
            if tls::SEND_HEADER_LEN + data_len > buf.len() {
                return Err(RawTlsError::InvalidLength);
            }
            Ok(RawTlsRequest::Send {
                cap,
                data: &buf[tls::SEND_HEADER_LEN..tls::SEND_HEADER_LEN + data_len],
            })
        }
        tls::RECV => {
            if buf.len() < tls::RECV_REQUEST_LEN {
                return Err(RawTlsError::BufferTooShort);
            }
            let want = u32::from_le_bytes(
                buf[tls::HEADER_LEN..tls::RECV_REQUEST_LEN]
                    .try_into()
                    .map_err(|_| RawTlsError::BufferTooShort)?,
            ) as usize;
            Ok(RawTlsRequest::Recv {
                cap,
                buf_len: want.min(tls::MAX_RECV_DATA),
            })
        }
        other => Err(RawTlsError::UnknownOpcode(other)),
    }
}

pub fn encode_tls_recv_reply(data: &[u8]) -> Vec<u8> {
    let bounded_data = if data.len() > tls::MAX_RECV_DATA {
        &data[..tls::MAX_RECV_DATA]
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
