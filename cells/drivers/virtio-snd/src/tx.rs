//! Playback: stream start/stop, feeding queued PCM to the tx queue a period
//! per buffer, reaping completions, and device recovery. Called once per
//! turn of the receive loop.

use crate::device::{Device, EVENTQ, TXQ};
use crate::pace;
use crate::pcm::PcmQueue;
use crate::proto::{self, EVENT_LEN, PCM_STATUS_LEN, PERIOD_BYTES, STATUS_LEN, S_OK, XFER_LEN};
use crate::ring::{self, TX_PCM, TX_STATUS};
use crate::service::DeviceView;
use crate::virtq::Mem;
use ostd::io::print_fmt;
use virtio_drivers::transport::{DeviceStatus, Transport};

/// How often a running cell reads the device status.
const CHECK_MS: u64 = 1_000;
/// Device resets attempted before the device is declared failed for good.
const MAX_RECOVERIES: u32 = 3;

/// Playback bookkeeping across loop turns.
#[derive(Default)]
pub struct Playback {
    /// When the running stream last had nothing queued or in flight.
    idle_since: Option<u64>,
    last_check: u64,
    recoveries: u32,
    /// tx buffers the device completed with a non-OK status since START.
    errors: u32,
}

impl Device {
    /// What a reply reports about the device.
    pub fn view(&self) -> DeviceView {
        DeviceView {
            healthy: self.healthy,
            in_flight: self.slot_bytes.iter().sum(),
        }
    }

    /// Whether the receive loop must come back within a tick.
    pub fn running(&self) -> bool {
        self.running
    }

    /// One playback turn at monotonic time `now` (ms). `held`: a client
    /// holds the stream; without one the queue drains and the stream stops.
    pub fn pump(&mut self, pcm: &mut PcmQueue, held: bool, now: u64) {
        if self.healthy {
            self.reap();
        }
        if !self.running || now.saturating_sub(self.play.last_check) >= CHECK_MS {
            self.play.last_check = now;
            self.check(pcm);
        }
        if !self.healthy {
            return;
        }
        let draining = !held;
        if !self.running {
            if !pace::should_start(pcm.len(), draining) {
                return;
            }
            let params = proto::set_params(self.stream);
            let prepare = proto::pcm_hdr(proto::R_PCM_PREPARE, self.stream);
            let start = proto::pcm_hdr(proto::R_PCM_START, self.stream);
            if !self.session("start", &[&params, &prepare, &start]) {
                return;
            }
            self.running = true;
            self.play.errors = 0;
        }
        self.feed(pcm, draining);
        if !pcm.is_empty() || self.tx.in_flight() != 0 {
            self.play.idle_since = None;
            return;
        }
        let since = *self.play.idle_since.get_or_insert(now);
        if pace::should_stop(draining, now.saturating_sub(since)) {
            let stop = proto::pcm_hdr(proto::R_PCM_STOP, self.stream);
            let release = proto::pcm_hdr(proto::R_PCM_RELEASE, self.stream);
            self.session("stop", &[&stop, &release]);
            self.running = false;
            self.play.idle_since = None;
            if self.play.errors != 0 {
                let _ = print_fmt(format_args!(
                    "[virtio-snd] device failed {} tx buffers\n",
                    self.play.errors
                ));
            }
        }
    }

    /// Runs `requests` in order; on the first failure logs it, marks the
    /// device unhealthy and returns false.
    fn session(&mut self, what: &str, requests: &[&[u8]]) -> bool {
        for request in requests {
            if let Err(error) = self.control(request, &mut [0; STATUS_LEN]) {
                self.healthy = false;
                let _ = print_fmt(format_args!("[virtio-snd] {what} failed: {error:?}\n"));
                return false;
            }
        }
        true
    }

    /// Fills free tx slots from `pcm` while the pacing policy allows.
    fn feed(&mut self, pcm: &mut PcmQueue, draining: bool) {
        let mut fed = false;
        while let Some(slot) = self.tx.free_slot() {
            let in_flight = self.tx.in_flight() as usize;
            let Some(take) = pace::next_period(pcm.len(), in_flight, draining) else {
                break;
            };
            // Header and period are one device-readable span; a short tail
            // is padded with silence.
            let mut buf = [0u8; XFER_LEN + PERIOD_BYTES];
            buf[..XFER_LEN].copy_from_slice(&proto::xfer(self.stream));
            pcm.pop_into(&mut buf[TX_PCM..TX_PCM + take]);
            let base = ring::tx_slot(slot);
            self.dma.write(base, &buf);
            self.dma.write(base + TX_STATUS, &[0; PCM_STATUS_LEN]);
            let chain = [
                (base, buf.len(), false),
                (base + TX_STATUS, PCM_STATUS_LEN, true),
            ];
            self.tx.submit(&self.dma, slot, &chain);
            self.slot_bytes[usize::from(slot)] = take;
            fed = true;
        }
        if fed {
            self.transport.notify(TXQ);
        }
    }

    /// Collects tx completions and recycles event buffers. A used ring that
    /// returns something never submitted leaves the device unhealthy.
    fn reap(&mut self) {
        loop {
            match self.tx.pop_used(&self.dma) {
                Ok(Some((slot, _))) => {
                    let mut status = [0u8; PCM_STATUS_LEN];
                    self.dma.read(ring::tx_slot(slot) + TX_STATUS, &mut status);
                    if proto::status(&status) != S_OK {
                        self.play.errors += 1;
                    }
                    self.slot_bytes[usize::from(slot)] = 0;
                }
                Ok(None) => break,
                Err(_) => {
                    self.healthy = false;
                    return;
                }
            }
        }
        let mut reposted = false;
        loop {
            match self.event.pop_used(&self.dma) {
                Ok(Some((slot, _))) => {
                    let buf = [(ring::event_buf(slot), EVENT_LEN, true)];
                    self.event.submit(&self.dma, slot, &buf);
                    reposted = true;
                }
                Ok(None) => break,
                Err(_) => {
                    self.healthy = false;
                    return;
                }
            }
        }
        if reposted {
            self.transport.notify(EVENTQ);
        }
    }

    /// Resets a device that failed a request or flagged
    /// `DEVICE_NEEDS_RESET`, at most [`MAX_RECOVERIES`] times in the cell's
    /// life; after that the device stays failed and queued PCM is dropped.
    fn check(&mut self, pcm: &mut PcmQueue) {
        let needs_reset = self
            .transport
            .get_status()
            .contains(DeviceStatus::DEVICE_NEEDS_RESET);
        if self.healthy && !needs_reset {
            return;
        }
        if self.play.recoveries >= MAX_RECOVERIES {
            self.healthy = false;
            pcm.clear();
            return;
        }
        self.play.recoveries += 1;
        self.running = false;
        let _ = print_fmt(format_args!(
            "[virtio-snd] device failed; reset {}/{MAX_RECOVERIES}\n",
            self.play.recoveries
        ));
        if let Err(error) = self.reinit() {
            self.healthy = false;
            let _ = print_fmt(format_args!("[virtio-snd] reset failed: {error}\n"));
        }
        self.play.idle_since = None;
    }
}
