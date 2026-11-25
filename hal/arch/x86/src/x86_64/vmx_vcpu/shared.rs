// SPDX-License-Identifier: MIT
//! What other threads hand a vCPU, and what they read about it, without the
//! vCPU's lock: its owner holds that for a whole run, and a run lasts until
//! its budget ends.
//!
//! Senders post with an atomic OR and then *kick* the vCPU: when
//! [`VcpuShared::running_on`] names a CPU, a host interrupt sent there ends
//! the guest's time on it (external-interrupt exiting), and the owner's next
//! entry collects the mail. The owner publishes `running_on` before it reads
//! the mailbox and the sender reads it after posting, both sequentially
//! consistent, so either the owner sees the mail or the sender sees the vCPU
//! running and interrupts it. An interrupt that arrives between the owner's
//! check and its VM entry stays pending at the CPU and ends the guest right
//! after entry.

use core::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering};

const SEQ: Ordering = Ordering::SeqCst;
/// `start` bits: an INIT arrived; a start-up IPI arrived (vector in 8..16).
const START_INIT: u32 = 1 << 0;
const START_SIPI: u32 = 1 << 1;

#[derive(Default)]
pub struct VcpuShared {
    /// External-interrupt vectors the VMM raised (`InjectIrq`).
    lines: [AtomicU64; 4],
    /// Fixed interrupts other vCPUs' local APICs sent to this one.
    ipis: [AtomicU64; 4],
    /// INIT and start-up IPIs received, for an application processor.
    start: AtomicU32,
    /// This vCPU's logical APIC ID (LDR bits 31:24), mirrored by its owner so
    /// senders can match a logical destination without the vCPU's lock.
    logical_id: AtomicU8,
    /// The host CPU this vCPU is in a run on, plus one; 0 between runs.
    running_on: AtomicUsize,
}

fn set_bit(words: &[AtomicU64; 4], vector: u8) {
    words[usize::from(vector >> 6)].fetch_or(1 << (vector & 63), SEQ);
}

fn take_bits(words: &[AtomicU64; 4]) -> [u64; 4] {
    core::array::from_fn(|index| words[index].swap(0, SEQ))
}

impl VcpuShared {
    /// Raise external interrupt `vector` (16..=255; the caller filters).
    pub fn raise_line(&self, vector: u8) {
        set_bit(&self.lines, vector);
    }

    /// A fixed interrupt from another vCPU's local APIC.
    pub fn post_ipi(&self, vector: u8) {
        set_bit(&self.ipis, vector);
    }

    pub fn post_init(&self) {
        self.start.fetch_or(START_INIT, SEQ);
    }

    pub fn post_sipi(&self, vector: u8) {
        self.start
            .fetch_or(START_SIPI | (u32::from(vector) << 8), SEQ);
    }

    /// The host CPU to interrupt so the vCPU sees new mail, if it is running.
    pub fn running_on(&self) -> Option<usize> {
        self.running_on.load(SEQ).checked_sub(1)
    }

    pub fn logical_id(&self) -> u8 {
        self.logical_id.load(Ordering::Relaxed)
    }

    // ── The owner's side ─────────────────────────────────────────────────────

    pub fn set_running(&self, cpu: Option<usize>) {
        self.running_on.store(cpu.map_or(0, |cpu| cpu + 1), SEQ);
    }

    pub(super) fn set_logical_id(&self, id: u8) {
        self.logical_id.store(id, Ordering::Relaxed);
    }

    pub(super) fn take_lines(&self) -> [u64; 4] {
        take_bits(&self.lines)
    }

    pub(super) fn take_ipis(&self) -> [u64; 4] {
        take_bits(&self.ipis)
    }

    /// `(INIT received, start-up vector received)`.
    pub(super) fn take_start(&self) -> (bool, Option<u8>) {
        let start = self.start.swap(0, SEQ);
        (
            start & START_INIT != 0,
            (start & START_SIPI != 0).then_some((start >> 8) as u8),
        )
    }

    /// An INIT or start-up IPI waits to be taken.
    pub(super) fn start_pending(&self) -> bool {
        self.start.load(SEQ) != 0
    }
}

/// Every vector set in a 256-bit word array, lowest first.
pub(super) fn vectors(words: [u64; 4]) -> impl Iterator<Item = u8> {
    (0..256u16).filter_map(move |vector| {
        (words[usize::from(vector >> 6)] & (1 << (vector & 63)) != 0).then_some(vector as u8)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mail_is_taken_once_and_start_up_keeps_its_vector() {
        let shared = VcpuShared::default();
        shared.raise_line(0x30);
        shared.raise_line(0x30);
        shared.raise_line(0xEF);
        shared.post_ipi(0xFD);
        shared.post_init();
        shared.post_sipi(0x9A);
        assert!(vectors(shared.take_lines()).eq([0x30, 0xEF]));
        assert!(vectors(shared.take_lines()).eq([]));
        assert!(vectors(shared.take_ipis()).eq([0xFD]));
        assert_eq!(shared.take_start(), (true, Some(0x9A)));
        assert_eq!(shared.take_start(), (false, None));
    }

    #[test]
    fn running_on_names_the_cpu_only_during_a_run() {
        let shared = VcpuShared::default();
        assert_eq!(shared.running_on(), None);
        shared.set_running(Some(0));
        assert_eq!(shared.running_on(), Some(0));
        shared.set_running(None);
        assert_eq!(shared.running_on(), None);
    }
}
