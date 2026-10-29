//! Driving the Sunset client over a byte transport.
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use sha2::{Digest, Sha256};
use sunset::{Client, Runner};

/// The connection's byte stream. `recv` returns `Ok(0)` when nothing is available
/// yet; end of stream is `Err(TransportError::Closed)`.
pub trait Transport {
    fn send(&mut self, data: &[u8]) -> Result<usize, TransportError>;
    fn recv(&mut self, buf: &mut [u8]) -> Result<usize, TransportError>;
    /// Called when neither side made progress; may sleep briefly. Returns
    /// `Err(TimedOut)` once the caller's deadline has passed.
    fn idle(&mut self) -> Result<(), TransportError>;
    /// Monotonic milliseconds (rekey age, KEX deadline, holds).
    fn now_ms(&mut self) -> u64;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportError {
    Closed,
    TimedOut,
    Io,
}

#[derive(Debug)]
pub enum ClientError {
    Transport(TransportError),
    Protocol(sunset::Error),
    /// The server ended the connection before a host key was offered.
    Ended,
}

impl From<sunset::Error> for ClientError {
    fn from(e: sunset::Error) -> Self {
        ClientError::Protocol(e)
    }
}

impl From<TransportError> for ClientError {
    fn from(e: TransportError) -> Self {
        ClientError::Transport(e)
    }
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientError::Transport(TransportError::Closed) => f.write_str("connection closed"),
            ClientError::Transport(TransportError::TimedOut) => f.write_str("timed out"),
            ClientError::Transport(TransportError::Io) => f.write_str("network I/O error"),
            ClientError::Protocol(e) => write!(f, "protocol error: {e}"),
            ClientError::Ended => f.write_str("server closed the connection"),
        }
    }
}

/// A server host key as offered during KEX.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostKey {
    pub algorithm: &'static str,
    pub key: [u8; 32],
}

impl HostKey {
    /// The SSH wire blob: string algorithm, string key.
    pub fn blob(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(4 + self.algorithm.len() + 4 + 32);
        b.extend_from_slice(&(self.algorithm.len() as u32).to_be_bytes());
        b.extend_from_slice(self.algorithm.as_bytes());
        b.extend_from_slice(&32u32.to_be_bytes());
        b.extend_from_slice(&self.key);
        b
    }

    /// OpenSSH-style `SHA256:<unpadded base64>` fingerprint of the blob.
    pub fn fingerprint(&self) -> String {
        let digest = Sha256::digest(self.blob());
        let mut s = String::from("SHA256:");
        base64_unpadded(&digest, &mut s);
        s
    }
}

fn base64_unpadded(data: &[u8], out: &mut String) {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for chunk in data.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
}

/// Largest transport read; the inbound queue `pump` keeps never holds more.
pub const RECV_CHUNK: usize = 2048;

/// Move pending runner output to the transport and transport input to the runner.
/// Returns whether any byte moved. `pending` holds at most [`RECV_CHUNK`] bytes (give it
/// that capacity up front and it never reallocates).
pub fn pump<T: Transport>(
    runner: &mut Runner<'_, Client>,
    t: &mut T,
    pending: &mut Vec<u8>,
) -> Result<bool, ClientError> {
    let mut moved = false;
    loop {
        let out = runner.output_buf();
        if out.is_empty() {
            break;
        }
        let n = t.send(out)?;
        if n == 0 {
            break;
        }
        runner.consume_output(n);
        moved = true;
    }
    if pending.is_empty() {
        let mut buf = [0u8; RECV_CHUNK];
        let n = t.recv(&mut buf)?;
        pending.extend_from_slice(&buf[..n]);
    }
    while !pending.is_empty() {
        let n = runner.input(pending)?;
        if n == 0 {
            break;
        }
        pending.drain(..n);
        moved = true;
    }
    Ok(moved)
}
