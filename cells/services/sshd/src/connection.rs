//! Connection protocol (RFC 4254), reachable only in `ActiveSession`: one
//! `session` channel running the built-in shell or one `exec` command.
//! Channel messages before `CHANNEL_OPEN`, or for another channel number, are
//! protocol errors.

extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;

use crate::session::{SessionPhase, SshSession};
use crate::shell;
use crate::ssh_packet::{
    PacketReader, PacketWriter, SSH_MSG_CHANNEL_CLOSE, SSH_MSG_CHANNEL_DATA, SSH_MSG_CHANNEL_EOF,
    SSH_MSG_CHANNEL_OPEN, SSH_MSG_CHANNEL_OPEN_CONFIRMATION, SSH_MSG_CHANNEL_OPEN_FAILURE,
    SSH_MSG_CHANNEL_REQUEST, SSH_MSG_CHANNEL_SUCCESS, SSH_MSG_CHANNEL_WINDOW_ADJUST,
    SSH_MSG_GLOBAL_REQUEST, SSH_MSG_REQUEST_FAILURE,
};

/// Our number for the single channel.
const SERVER_CHANNEL: u32 = 0;
const WINDOW: u32 = 2_000_000;
const MAX_PACKET: u32 = 32768;
/// Largest `CHANNEL_DATA` string sent, below the 32 KiB every client accepts.
const DATA_CHUNK: usize = 16 * 1024;
/// RFC 4254 §5.1 `SSH_OPEN_*` reasons.
const OPEN_ADMINISTRATIVELY_PROHIBITED: u32 = 1;
const OPEN_UNKNOWN_CHANNEL_TYPE: u32 = 3;

const MALFORMED: &str = "malformed connection message";

pub struct Channel {
    client_id: u32,
    open: bool,
    shell: bool,
    line: Vec<u8>,
}

impl Channel {
    pub fn new() -> Self {
        Self {
            client_id: 0,
            open: false,
            shell: false,
            line: Vec::new(),
        }
    }
}

impl SshSession {
    pub(crate) fn connection_message(
        &mut self,
        msg: u8,
        body: &[u8],
    ) -> Result<Vec<Vec<u8>>, &'static str> {
        let mut r = PacketReader::new(body);
        match msg {
            SSH_MSG_GLOBAL_REQUEST => {
                r.read_string().ok_or(MALFORMED)?;
                let want_reply = r.read_bool().ok_or(MALFORMED)?;
                Ok(if want_reply {
                    vec![vec![SSH_MSG_REQUEST_FAILURE]]
                } else {
                    Vec::new()
                })
            }
            SSH_MSG_CHANNEL_OPEN => self.channel_open(&mut r),
            SSH_MSG_CHANNEL_WINDOW_ADJUST
            | SSH_MSG_CHANNEL_DATA
            | SSH_MSG_CHANNEL_EOF
            | SSH_MSG_CHANNEL_CLOSE
            | SSH_MSG_CHANNEL_REQUEST => {
                if !self.channel.open {
                    return Err("no open channel");
                }
                if r.read_u32().ok_or(MALFORMED)? != SERVER_CHANNEL {
                    return Err("unknown channel");
                }
                self.channel_message(msg, &mut r)
            }
            _ => Err("message not allowed in this phase"),
        }
    }

    fn channel_open(&mut self, r: &mut PacketReader) -> Result<Vec<Vec<u8>>, &'static str> {
        let kind = r.read_string().ok_or(MALFORMED)?;
        let sender = r.read_u32().ok_or(MALFORMED)?;
        let mut w = PacketWriter::new();
        if kind != b"session" || self.channel.open {
            let reason = if kind != b"session" {
                OPEN_UNKNOWN_CHANNEL_TYPE
            } else {
                OPEN_ADMINISTRATIVELY_PROHIBITED
            };
            w.write_byte(SSH_MSG_CHANNEL_OPEN_FAILURE);
            w.write_u32(sender);
            w.write_u32(reason);
            w.write_string(b"one session channel only");
            w.write_string(b"");
            return Ok(vec![w.as_slice().to_vec()]);
        }
        self.channel.client_id = sender;
        self.channel.open = true;
        w.write_byte(SSH_MSG_CHANNEL_OPEN_CONFIRMATION);
        w.write_u32(sender);
        w.write_u32(SERVER_CHANNEL);
        w.write_u32(WINDOW);
        w.write_u32(MAX_PACKET);
        Ok(vec![w.as_slice().to_vec()])
    }

    fn channel_message(
        &mut self,
        msg: u8,
        r: &mut PacketReader,
    ) -> Result<Vec<Vec<u8>>, &'static str> {
        let mut out = Vec::new();
        match msg {
            SSH_MSG_CHANNEL_REQUEST => {
                let request = r.read_string().ok_or(MALFORMED)?;
                if r.read_bool().ok_or(MALFORMED)? {
                    out.push(self.with_channel(SSH_MSG_CHANNEL_SUCCESS));
                }
                if request == b"shell" {
                    self.channel.shell = true;
                    self.channel_data(shell::BANNER, &mut out);
                } else if request == b"exec" {
                    let cmd = r.read_utf8_string().ok_or(MALFORMED)?;
                    self.channel_data(shell::execute(cmd.trim()).as_bytes(), &mut out);
                    let mut status = PacketWriter::new();
                    status.write_byte(SSH_MSG_CHANNEL_REQUEST);
                    status.write_u32(self.channel.client_id);
                    status.write_string(b"exit-status");
                    status.write_bool(false);
                    status.write_u32(0);
                    out.push(status.as_slice().to_vec());
                    out.push(self.with_channel(SSH_MSG_CHANNEL_EOF));
                    out.push(self.with_channel(SSH_MSG_CHANNEL_CLOSE));
                    self.phase = SessionPhase::Closed;
                }
            }
            SSH_MSG_CHANNEL_DATA => {
                let data = r.read_string().ok_or(MALFORMED)?;
                if self.channel.shell {
                    let echo = shell::feed(&mut self.channel.line, data);
                    self.channel_data(&echo, &mut out);
                }
            }
            SSH_MSG_CHANNEL_EOF => out.push(self.with_channel(SSH_MSG_CHANNEL_CLOSE)),
            SSH_MSG_CHANNEL_CLOSE => {
                out.push(self.with_channel(SSH_MSG_CHANNEL_CLOSE));
                self.phase = SessionPhase::Closed;
            }
            // Flow control is not enforced: outbound bursts are small and bounded.
            _ => {}
        }
        Ok(out)
    }

    fn with_channel(&self, msg: u8) -> Vec<u8> {
        let mut w = PacketWriter::new();
        w.write_byte(msg);
        w.write_u32(self.channel.client_id);
        w.as_slice().to_vec()
    }

    fn channel_data(&self, bytes: &[u8], out: &mut Vec<Vec<u8>>) {
        for chunk in bytes.chunks(DATA_CHUNK) {
            let mut w = PacketWriter::new();
            w.write_byte(SSH_MSG_CHANNEL_DATA);
            w.write_u32(self.channel.client_id);
            w.write_string(chunk);
            out.push(w.as_slice().to_vec());
        }
    }
}
