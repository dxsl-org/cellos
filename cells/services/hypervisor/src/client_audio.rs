//! The guest's sound to the audio service (`abi::syscall::service::AUDIO`,
//! `wire::audio`), from the BSP's thread.
//!
//! The virtio-snd model queues the guest's PCM ([`HostQueue`]); this client
//! sends it in chunks of at most `MAX_CHUNK` bytes through one Grant it
//! registers and shares read-only with the driver. A Grant belongs to the
//! task that registered it and the driver copies it as the sender's, so every
//! call comes from the BSP's thread (`run_loop_x86::serve_host`), like the
//! clipboard's.
//!
//! Calls never block ([`call`]): the vCPU never waits on the driver. One is
//! outstanding at a time, so the Grant is not refilled while the driver may
//! copy it; an abandoned call's chunk is counted dropped (a late copy can then
//! tear at most that one chunk).
//!
//! Sound never stalls the guest: no service, a refusal, a full driver queue
//! or a timeout lose sound, and the device's clock keeps completing buffers.

mod call;

use call::{Call, Outcome, Outstanding, Submitted};
use ostd::io::println;
use ostd::syscall::{sys_grant_register, sys_grant_share, sys_grant_unregister, sys_post};
use wire::audio::{AudioOpen, AudioWrite, MAX_CHUNK, STATUS_DEVICE, STATUS_NOT_OPEN, STATUS_OK};

use crate::virtio_snd::{HostQueue, SndDev};

/// How often a missing audio service is looked up again.
const LOOKUP_INTERVAL_NS: u64 = 1_000_000_000;
/// `sys_grant_share` permission.
const READ_ONLY: u8 = 0;

pub struct AudioClient {
    provider: Option<usize>,
    next_lookup_ns: u64,
    absent_reported: bool,
    /// The PCM Grant, 0 until the first open.
    grant: usize,
    shared_with: usize,
    open: bool,
    /// The service refused this run of the stream; sound is dropped until the
    /// guest stops it.
    refused: bool,
    call: Option<Outstanding>,
    sent: u64,
    dropped: u64,
}

impl AudioClient {
    pub const fn new() -> Self {
        Self {
            provider: None,
            next_lookup_ns: 0,
            absent_reported: false,
            grant: 0,
            shared_with: 0,
            open: false,
            refused: false,
            call: None,
            sent: 0,
            dropped: 0,
        }
    }

    /// Move the stream's state and PCM toward the service (BSP's thread, at
    /// a host boundary). Never blocks.
    pub fn pump(&mut self, snd: &mut SndDev, now_ns: u64) {
        if let Some(call) = self.call.take() {
            match call.take(now_ns) {
                Outcome::Pending(call) => {
                    self.call = Some(call);
                    return;
                }
                Outcome::Answered(call, status) => self.settle(call, status),
                Outcome::Gone(call) => {
                    self.lost_provider();
                    self.settle(call, None);
                }
            }
        }
        if !snd.playing() {
            self.refused = false;
            if self.open {
                self.post_close();
            }
            return;
        }
        let Some(provider) = self.provider(now_ns) else {
            snd.host_mut().set_accepting(false);
            return;
        };
        snd.host_mut().set_accepting(!self.refused);
        if self.refused {
            return;
        }
        if self.open {
            self.write(provider, snd.host_mut(), now_ns);
        } else {
            self.open_stream(provider, now_ns);
        }
    }

    /// Close the stream and report what reached the service (run loop end).
    pub fn finish(&mut self, snd: &SndDev) {
        if let Some(call) = self.call.take() {
            self.dropped += u64::from(call.write_len());
        }
        if self.open {
            self.post_close();
        }
        if self.grant != 0 {
            let _ = sys_grant_unregister(self.grant);
            self.grant = 0;
        }
        if snd.starts() == 0 {
            return;
        }
        let dropped = self.dropped + snd.host().dropped() + snd.host().len() as u64;
        println(&alloc::format!(
            "[hv-x86] sound: {} bytes to the audio service, {} dropped",
            self.sent,
            dropped
        ));
    }

    /// The service's task, looked up at most every [`LOOKUP_INTERVAL_NS`].
    fn provider(&mut self, now_ns: u64) -> Option<usize> {
        if self.provider.is_none() && now_ns >= self.next_lookup_ns {
            self.next_lookup_ns = now_ns.saturating_add(LOOKUP_INTERVAL_NS);
            self.provider = ostd::service::provider_tid(abi::syscall::service::AUDIO);
            if self.provider.is_none() && !self.absent_reported {
                println("[hv-x86] sound: no audio service; playing silence");
                self.absent_reported = true;
            }
        }
        self.provider
    }

    fn open_stream(&mut self, provider: usize, now_ns: u64) {
        let Some(grant) = self.grant(provider) else {
            self.refuse(None);
            return;
        };
        let request = AudioOpen {
            grant: grant as u32,
        };
        self.submit(provider, &request.encode(), Call::Open, now_ns);
    }

    /// Send the oldest queued PCM, if any.
    fn write(&mut self, provider: usize, host: &mut HostQueue, now_ns: u64) {
        let len = ostd::grant::with_shared_bytes_mut(self.grant, |bytes| {
            let end = bytes.len().min(MAX_CHUNK as usize);
            host.peek_into(&mut bytes[..end])
        })
        .unwrap_or(0);
        if len == 0 {
            return;
        }
        let request = AudioWrite { len: len as u32 };
        if self.submit(provider, &request.encode(), Call::Write(len as u32), now_ns) {
            host.consume(len);
        }
    }

    /// Submit one call; whether it was accepted.
    fn submit(&mut self, provider: usize, request: &[u8], call: Call, now_ns: u64) -> bool {
        match Outstanding::submit(provider, request, call, now_ns) {
            Submitted::Accepted(call) => {
                self.call = Some(call);
                true
            }
            Submitted::Busy => false,
            Submitted::Gone => {
                self.lost_provider();
                false
            }
        }
    }

    fn settle(&mut self, call: Call, status: Option<u32>) {
        match call {
            Call::Open if status == Some(STATUS_OK) => self.open = true,
            Call::Open if status.is_some() => self.refuse(status),
            // Unanswered: whether the driver bound the stream is unknown, so
            // a CLOSE resets it before the next OPEN.
            Call::Open => self.post_close(),
            Call::Write(len) if status == Some(STATUS_OK) => self.sent += u64::from(len),
            Call::Write(len) => {
                self.dropped += u64::from(len);
                match status {
                    Some(STATUS_NOT_OPEN) => self.open = false,
                    Some(STATUS_DEVICE) => self.refuse(status),
                    _ => {}
                }
            }
        }
    }

    /// The PCM Grant, registered once and shared with the current provider.
    fn grant(&mut self, provider: usize) -> Option<usize> {
        if self.grant == 0 {
            self.grant = sys_grant_register(MAX_CHUNK as usize)?;
        }
        if self.shared_with != provider {
            if !sys_grant_share(self.grant, provider, READ_ONLY) {
                return None;
            }
            self.shared_with = provider;
        }
        Some(self.grant)
    }

    fn refuse(&mut self, status: Option<u32>) {
        self.refused = true;
        println(&alloc::format!(
            "[hv-x86] sound: the audio service refused the stream (status {status:?})"
        ));
    }

    fn post_close(&mut self) {
        if let Some(provider) = self.provider {
            let _ = sys_post(provider, &[wire::audio::CLOSE]);
        }
        self.open = false;
    }

    /// The service is gone: look it up again (it may come back as another
    /// task).
    fn lost_provider(&mut self) {
        self.provider = None;
        self.open = false;
    }
}
