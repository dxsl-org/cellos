//! SSH-2.0 Binary Packet Protocol (RFC 4253) serialization and deserialization.

#![forbid(unsafe_code)]

extern crate alloc;
use alloc::vec::Vec;

pub const SSH_MSG_DISCONNECT: u8 = 1;
pub const SSH_MSG_IGNORE: u8 = 2;
pub const SSH_MSG_UNIMPLEMENTED: u8 = 3;
pub const SSH_MSG_SERVICE_REQUEST: u8 = 5;
pub const SSH_MSG_SERVICE_ACCEPT: u8 = 6;
pub const SSH_MSG_KEXINIT: u8 = 20;
pub const SSH_MSG_NEWKEYS: u8 = 21;
pub const SSH_MSG_KEX_ECDH_INIT: u8 = 30;
pub const SSH_MSG_KEX_ECDH_REPLY: u8 = 31;
pub const SSH_MSG_USERAUTH_REQUEST: u8 = 50;
pub const SSH_MSG_USERAUTH_FAILURE: u8 = 51;
pub const SSH_MSG_USERAUTH_SUCCESS: u8 = 52;
pub const SSH_MSG_GLOBAL_REQUEST: u8 = 80;
pub const SSH_MSG_REQUEST_SUCCESS: u8 = 81;
pub const SSH_MSG_REQUEST_FAILURE: u8 = 82;
pub const SSH_MSG_CHANNEL_OPEN: u8 = 90;
pub const SSH_MSG_CHANNEL_OPEN_CONFIRMATION: u8 = 91;
pub const SSH_MSG_CHANNEL_OPEN_FAILURE: u8 = 92;
pub const SSH_MSG_CHANNEL_WINDOW_ADJUST: u8 = 93;
pub const SSH_MSG_CHANNEL_DATA: u8 = 94;
pub const SSH_MSG_CHANNEL_EOF: u8 = 96;
pub const SSH_MSG_CHANNEL_CLOSE: u8 = 97;
pub const SSH_MSG_CHANNEL_REQUEST: u8 = 98;
pub const SSH_MSG_CHANNEL_SUCCESS: u8 = 99;
pub const SSH_MSG_CHANNEL_FAILURE: u8 = 100;

pub struct PacketWriter {
    buf: Vec<u8>,
}

impl PacketWriter {
    pub fn new() -> Self {
        Self {
            buf: Vec::with_capacity(256),
        }
    }

    pub fn write_byte(&mut self, b: u8) {
        self.buf.push(b);
    }

    pub fn write_bool(&mut self, b: bool) {
        self.buf.push(if b { 1 } else { 0 });
    }

    pub fn write_u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    pub fn write_u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    pub fn write_string(&mut self, s: &[u8]) {
        self.write_u32(s.len() as u32);
        self.buf.extend_from_slice(s);
    }

    pub fn write_name_list(&mut self, names: &[&str]) {
        let mut total_len = 0;
        for (i, name) in names.iter().enumerate() {
            total_len += name.len();
            if i + 1 < names.len() {
                total_len += 1; // comma
            }
        }
        self.write_u32(total_len as u32);
        for (i, name) in names.iter().enumerate() {
            self.buf.extend_from_slice(name.as_bytes());
            if i + 1 < names.len() {
                self.buf.push(b',');
            }
        }
    }

    pub fn write_mpint(&mut self, val: &[u8]) {
        // Strip leading zeros
        let mut start = 0;
        while start < val.len() && val[start] == 0 {
            start += 1;
        }
        let trimmed = &val[start..];
        if trimmed.is_empty() {
            self.write_u32(0);
            return;
        }
        // If highest bit is set, need a leading 0x00 so it's not negative
        if (trimmed[0] & 0x80) != 0 {
            self.write_u32((trimmed.len() + 1) as u32);
            self.buf.push(0x00);
            self.buf.extend_from_slice(trimmed);
        } else {
            self.write_u32(trimmed.len() as u32);
            self.buf.extend_from_slice(trimmed);
        }
    }

    pub fn write_raw(&mut self, raw: &[u8]) {
        self.buf.extend_from_slice(raw);
    }

    /// Pack into SSH Binary Packet (RFC 4253 §6) unencrypted
    pub fn build_unencrypted_packet(self) -> Vec<u8> {
        let payload = self.buf;
        let block_size = 8;
        // packet_length = 1 (padding_length) + payload.len() + padding_len
        // (4 + packet_length) must be multiple of block_size, padding_len >= 4
        let min_padding = 4;
        let total_unpadded = 4 + 1 + payload.len() + min_padding;
        let rem = total_unpadded % block_size;
        let pad_len = min_padding + if rem == 0 { 0 } else { block_size - rem };
        let packet_length = 1 + payload.len() + pad_len;

        let mut out = Vec::with_capacity(4 + packet_length);
        out.extend_from_slice(&(packet_length as u32).to_be_bytes());
        out.push(pad_len as u8);
        out.extend_from_slice(&payload);
        // padding bytes (can be zero for unencrypted)
        out.resize(out.len() + pad_len, 0);
        out
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.buf
    }
}

pub struct PacketReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> PacketReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    pub fn read_byte(&mut self) -> Option<u8> {
        if self.pos < self.data.len() {
            let b = self.data[self.pos];
            self.pos += 1;
            Some(b)
        } else {
            None
        }
    }

    pub fn read_bool(&mut self) -> Option<bool> {
        self.read_byte().map(|b| b != 0)
    }

    pub fn read_u32(&mut self) -> Option<u32> {
        if self.pos + 4 <= self.data.len() {
            let val = u32::from_be_bytes([
                self.data[self.pos],
                self.data[self.pos + 1],
                self.data[self.pos + 2],
                self.data[self.pos + 3],
            ]);
            self.pos += 4;
            Some(val)
        } else {
            None
        }
    }

    pub fn read_u64(&mut self) -> Option<u64> {
        if self.pos + 8 <= self.data.len() {
            let mut b = [0u8; 8];
            b.copy_from_slice(&self.data[self.pos..self.pos + 8]);
            self.pos += 8;
            Some(u64::from_be_bytes(b))
        } else {
            None
        }
    }

    pub fn read_string(&mut self) -> Option<&'a [u8]> {
        let len = self.read_u32()? as usize;
        if self.pos + len <= self.data.len() {
            let slice = &self.data[self.pos..self.pos + len];
            self.pos += len;
            Some(slice)
        } else {
            None
        }
    }

    pub fn read_utf8_string(&mut self) -> Option<&'a str> {
        let slice = self.read_string()?;
        core::str::from_utf8(slice).ok()
    }

    pub fn read_raw(&mut self, len: usize) -> Option<&'a [u8]> {
        if self.pos + len <= self.data.len() {
            let slice = &self.data[self.pos..self.pos + len];
            self.pos += len;
            Some(slice)
        } else {
            None
        }
    }
}
