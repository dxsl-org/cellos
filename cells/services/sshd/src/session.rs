//! SSH-2.0 Session State Machine, Channel Multiplexing, and Shell Execution.

#![forbid(unsafe_code)]

extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::cipher::CipherMode;
use crate::kex::{KexState, SERVER_VERSION, SERVER_VERSION_RAW};
use crate::ssh_packet::{
    PacketReader, PacketWriter, SSH_MSG_CHANNEL_CLOSE, SSH_MSG_CHANNEL_DATA,
    SSH_MSG_CHANNEL_EOF, SSH_MSG_CHANNEL_OPEN, SSH_MSG_CHANNEL_OPEN_CONFIRMATION,
    SSH_MSG_CHANNEL_REQUEST, SSH_MSG_CHANNEL_SUCCESS, SSH_MSG_CHANNEL_WINDOW_ADJUST,
    SSH_MSG_KEXINIT, SSH_MSG_KEX_ECDH_INIT, SSH_MSG_NEWKEYS, SSH_MSG_SERVICE_ACCEPT,
    SSH_MSG_SERVICE_REQUEST, SSH_MSG_USERAUTH_FAILURE, SSH_MSG_USERAUTH_REQUEST,
    SSH_MSG_USERAUTH_SUCCESS,
};

#[derive(PartialEq, Eq, Debug)]
pub enum SessionPhase {
    VersionExchange,
    KexInit,
    EcdhInit,
    WaitNewKeys,
    ServiceRequest,
    UserAuth,
    ActiveSession,
    Closed,
}

pub struct SshSession {
    pub phase: SessionPhase,
    pub kex: KexState,
    pub cipher: CipherMode,
    pub rx_buf: Vec<u8>,
    pub client_channel_id: u32,
    pub channel_open: bool,
    pub shell_active: bool,
    pub line_buffer: Vec<u8>,
}

impl SshSession {
    pub fn new() -> Self {
        Self {
            phase: SessionPhase::VersionExchange,
            kex: KexState::new(),
            cipher: CipherMode::new_plaintext(),
            rx_buf: Vec::with_capacity(4096),
            client_channel_id: 0,
            channel_open: false,
            shell_active: false,
            line_buffer: Vec::with_capacity(128),
        }
    }

    /// Initial bytes to send upon TCP connection establishment
    pub fn initial_outbound(&self) -> Vec<u8> {
        SERVER_VERSION.to_vec()
    }

    /// Process incoming raw bytes from TCP socket and return responses to send
    pub fn process_incoming(&mut self, data: &[u8]) -> Vec<u8> {
        self.rx_buf.extend_from_slice(data);
        let mut responses = Vec::new();

        loop {
            match self.phase {
                SessionPhase::VersionExchange => {
                    // Look for \n in rx_buf
                    if let Some(pos) = self.rx_buf.iter().position(|&b| b == b'\n') {
                        let version_line = &self.rx_buf[..pos];
                        let trimmed = version_line.strip_suffix(b"\r").unwrap_or(version_line);
                        self.kex.client_version = trimmed.to_vec();
                        self.rx_buf.drain(..=pos);

                        // Send Server KEXINIT
                        let kexinit_payload = self.kex.make_server_kexinit();
                        let pkt = self.cipher.pack_outbound(&kexinit_payload);
                        responses.extend_from_slice(&pkt);

                        self.phase = SessionPhase::KexInit;
                    } else {
                        break; // wait for more data
                    }
                }
                SessionPhase::KexInit => {
                    match self.cipher.unpack_inbound(&self.rx_buf) {
                        Ok(Some((payload, consumed))) => {
                            self.rx_buf.drain(..consumed);
                            if !payload.is_empty() && payload[0] == SSH_MSG_KEXINIT {
                                self.kex.client_kexinit = payload;
                                self.phase = SessionPhase::EcdhInit;
                            }
                        }
                        Ok(None) => break,
                        Err(_) => {
                            self.phase = SessionPhase::Closed;
                            break;
                        }
                    }
                }
                SessionPhase::EcdhInit => {
                    match self.cipher.unpack_inbound(&self.rx_buf) {
                        Ok(Some((payload, consumed))) => {
                            self.rx_buf.drain(..consumed);
                            if !payload.is_empty() && payload[0] == SSH_MSG_KEX_ECDH_INIT {
                                let mut r = PacketReader::new(&payload[1..]);
                                if let Some(client_pubkey) = r.read_string() {
                                    if let Some((ecdh_reply, newkeys)) =
                                        self.kex.handle_ecdh_init(client_pubkey)
                                    {
                                        responses.extend_from_slice(
                                            &self.cipher.pack_outbound(&ecdh_reply),
                                        );
                                        responses.extend_from_slice(
                                            &self.cipher.pack_outbound(&newkeys),
                                        );
                                        self.phase = SessionPhase::WaitNewKeys;
                                    } else {
                                        self.phase = SessionPhase::Closed;
                                        break;
                                    }
                                }
                            }
                        }
                        Ok(None) => break,
                        Err(_) => {
                            self.phase = SessionPhase::Closed;
                            break;
                        }
                    }
                }
                SessionPhase::WaitNewKeys => {
                    match self.cipher.unpack_inbound(&self.rx_buf) {
                        Ok(Some((payload, consumed))) => {
                            self.rx_buf.drain(..consumed);
                            if !payload.is_empty() && payload[0] == SSH_MSG_NEWKEYS {
                                // Transition to ChaCha20-Poly1305
                                let c2s_key = self.kex.derive_key(b'C', 64);
                                let s2c_key = self.kex.derive_key(b'D', 64);
                                self.cipher =
                                    CipherMode::new_chacha20(&c2s_key, &s2c_key, 3, 3);
                                self.phase = SessionPhase::ServiceRequest;
                            }
                        }
                        Ok(None) => break,
                        Err(_) => {
                            self.phase = SessionPhase::Closed;
                            break;
                        }
                    }
                }
                SessionPhase::ServiceRequest | SessionPhase::UserAuth | SessionPhase::ActiveSession => {
                    match self.cipher.unpack_inbound(&self.rx_buf) {
                        Ok(Some((payload, consumed))) => {
                            ostd::io::println("[sshd] decrypted packet!");
                            self.rx_buf.drain(..consumed);
                            if let Some(resp) = self.handle_authenticated_packet(&payload) {
                                responses.extend_from_slice(&resp);
                            }
                        }
                        Ok(None) => break,
                        Err(_) => {
                            ostd::io::println("[sshd] cipher.unpack_inbound returned Err!");
                            self.phase = SessionPhase::Closed;
                            break;
                        }
                    }
                }
                SessionPhase::Closed => break,
            }
        }

        responses
    }

    fn handle_authenticated_packet(&mut self, payload: &[u8]) -> Option<Vec<u8>> {
        if payload.is_empty() {
            return None;
        }
        let msg_type = payload[0];
        let mut r = PacketReader::new(&payload[1..]);

        match msg_type {
            SSH_MSG_SERVICE_REQUEST => {
                let service_name = r.read_utf8_string()?;
                if service_name == "ssh-userauth" {
                    let mut w = PacketWriter::new();
                    w.write_byte(SSH_MSG_SERVICE_ACCEPT);
                    w.write_string(b"ssh-userauth");
                    self.phase = SessionPhase::UserAuth;
                    Some(self.cipher.pack_outbound(w.as_slice()))
                } else {
                    None
                }
            }
            SSH_MSG_USERAUTH_REQUEST => {
                let _user = r.read_utf8_string()?;
                let _service = r.read_utf8_string()?;
                let _method = r.read_utf8_string()?;

                // Instant authenticated access (root/dev profile: no password prompt needed)
                let mut w = PacketWriter::new();
                w.write_byte(SSH_MSG_USERAUTH_SUCCESS);
                self.phase = SessionPhase::ActiveSession;
                Some(self.cipher.pack_outbound(w.as_slice()))
            }
            SSH_MSG_CHANNEL_OPEN => {
                let channel_type = r.read_utf8_string()?;
                let sender_channel = r.read_u32()?;
                let _init_window = r.read_u32()?;
                let _max_packet = r.read_u32()?;

                if channel_type == "session" {
                    self.client_channel_id = sender_channel;
                    self.channel_open = true;

                    let mut w = PacketWriter::new();
                    w.write_byte(SSH_MSG_CHANNEL_OPEN_CONFIRMATION);
                    w.write_u32(sender_channel); // recipient_channel
                    w.write_u32(0);              // server channel = 0
                    w.write_u32(2_000_000);      // initial window size
                    w.write_u32(32768);          // max packet size
                    Some(self.cipher.pack_outbound(w.as_slice()))
                } else {
                    None
                }
            }
            SSH_MSG_CHANNEL_REQUEST => {
                let recipient_channel = r.read_u32()?;
                let request_type = r.read_utf8_string()?;
                let want_reply = r.read_bool().unwrap_or(false);

                let mut out = Vec::new();
                if want_reply {
                    let mut w = PacketWriter::new();
                    w.write_byte(SSH_MSG_CHANNEL_SUCCESS);
                    w.write_u32(recipient_channel);
                    out.extend_from_slice(&self.cipher.pack_outbound(w.as_slice()));
                }

                if request_type == "shell" {
                    self.shell_active = true;
                    let banner = b"\r\n\x1b[1;36m=== Cellos Remote SSH Server v0.2.1 (x86_64) ===\x1b[0m\r\nType 'help' for built-in commands.\r\n\r\nCellos > ";
                    out.extend_from_slice(&self.make_channel_data(banner));
                } else if request_type == "exec" {
                    let cmd = r.read_utf8_string().unwrap_or("");
                    let response = self.execute_command(cmd.trim());
                    if !response.is_empty() {
                        out.extend_from_slice(&self.make_channel_data(response.as_bytes()));
                    }
                    let mut ext = PacketWriter::new();
                    ext.write_byte(SSH_MSG_CHANNEL_REQUEST);
                    ext.write_u32(self.client_channel_id);
                    ext.write_string(b"exit-status");
                    ext.write_bool(false);
                    ext.write_u32(0);
                    out.extend_from_slice(&self.cipher.pack_outbound(ext.as_slice()));

                    let mut w_eof = PacketWriter::new();
                    w_eof.write_byte(SSH_MSG_CHANNEL_EOF);
                    w_eof.write_u32(self.client_channel_id);
                    out.extend_from_slice(&self.cipher.pack_outbound(w_eof.as_slice()));

                    let mut w_close = PacketWriter::new();
                    w_close.write_byte(SSH_MSG_CHANNEL_CLOSE);
                    w_close.write_u32(self.client_channel_id);
                    out.extend_from_slice(&self.cipher.pack_outbound(w_close.as_slice()));
                    self.phase = SessionPhase::Closed;
                }
                Some(out)
            }
            SSH_MSG_CHANNEL_DATA => {
                let recipient_channel = r.read_u32()?;
                let data = r.read_string()?;
                let mut out = Vec::new();

                for &b in data {
                    if b == b'\r' || b == b'\n' {
                        // Echo newline
                        out.extend_from_slice(&self.make_channel_data(b"\r\n"));
                        let cmd_str = core::str::from_utf8(&self.line_buffer).unwrap_or("");
                        let response = self.execute_command(cmd_str.trim());
                        if !response.is_empty() {
                            out.extend_from_slice(&self.make_channel_data(response.as_bytes()));
                        }
                        out.extend_from_slice(&self.make_channel_data(b"Cellos > "));
                        self.line_buffer.clear();
                    } else if b == 0x7F || b == 0x08 {
                        // Backspace
                        if !self.line_buffer.is_empty() {
                            self.line_buffer.pop();
                            out.extend_from_slice(&self.make_channel_data(b"\x08 \x08"));
                        }
                    } else if (0x20..=0x7E).contains(&b) {
                        self.line_buffer.push(b);
                        // Echo character back
                        out.extend_from_slice(&self.make_channel_data(&[b]));
                    }
                }
                Some(out)
            }
            SSH_MSG_CHANNEL_WINDOW_ADJUST => {
                // Ignore window adjustments
                None
            }
            SSH_MSG_CHANNEL_EOF => {
                let mut w = PacketWriter::new();
                w.write_byte(SSH_MSG_CHANNEL_CLOSE);
                w.write_u32(self.client_channel_id);
                Some(self.cipher.pack_outbound(w.as_slice()))
            }
            SSH_MSG_CHANNEL_CLOSE => {
                let mut w = PacketWriter::new();
                w.write_byte(SSH_MSG_CHANNEL_CLOSE);
                w.write_u32(self.client_channel_id);
                self.phase = SessionPhase::Closed;
                Some(self.cipher.pack_outbound(w.as_slice()))
            }
            _ => None,
        }
    }

    fn make_channel_data(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut w = PacketWriter::new();
        w.write_byte(SSH_MSG_CHANNEL_DATA);
        w.write_u32(self.client_channel_id);
        w.write_string(bytes);
        self.cipher.pack_outbound(w.as_slice())
    }

    fn execute_command(&self, cmd: &str) -> String {
        if cmd.is_empty() {
            return String::new();
        }
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let name = parts[0];

        match name {
            "help" => {
                String::from(
                    "Cellos Shell v0.2.1 (SSH Remote Session) — built-in commands:\r\n  System:  uname  ps  uptime  free  clear  help  exit\r\n  Files:   ls /bin\r\n",
                )
            }
            "uname" => {
                String::from("Cellos cellos-kernel 0.2.1 x86_64\r\n")
            }
            "uptime" => {
                let ticks = ostd::syscall::sys_get_scheduler_ticks().unwrap_or(0);
                let secs = ticks / 100;
                format!("uptime: {} seconds ({} ticks)\r\n", secs, ticks)
            }
            "clear" => {
                String::from("\x1b[2J\x1b[H")
            }
            "ps" => {
                let mut rows = [api::syscall::ProcessInfo::default(); 32];
                let mut out = String::from("  PID  STATE      NAME\r\n  ---  ---------  ----------------\r\n");
                if let Ok(count) = ostd::syscall::sys_get_procs(&mut rows) {
                    for row in rows.iter().take(count) {
                        let state = match row.state {
                            1 => "Ready",
                            2 => "Waiting",
                            3 => "Exited",
                            4 => "Running",
                            _ => "Unknown",
                        };
                        let name = core::str::from_utf8(&row.name)
                            .unwrap_or("unknown")
                            .trim_matches('\0');
                        out.push_str(&format!("  {:<4} {:<10} {}\r\n", row.id, state, name));
                    }
                }
                out
            }
            "ls" => {
                String::from(
                    "/bin:\r\n  ahci       cat        compositor config     e1000      echo\r\n  fb-console igb        input      kill       ls         net\r\n  nvme       platform   ps         serial     shell      sshd\r\n  xhci\r\n",
                )
            }
            "free" => {
                String::from("Heap: 4096 KB total, single-address-space isolation\r\n")
            }
            "exit" => {
                String::from("Connection closed by foreign host.\r\n")
            }
            other => {
                format!("shell: command not found: {}\r\n", other)
            }
        }
    }
}
