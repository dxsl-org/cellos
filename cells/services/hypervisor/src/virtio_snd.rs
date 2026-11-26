//! virtio-sound device model (device id 25, virtio 1.2 §5.14): x86
//! virtio-mmio slot 6, announced to the Chrome guest only.
//!
//! One output stream in `wire::audio`'s format (S16LE, 48 kHz, stereo), no
//! jacks, one stereo channel map; no feature bits. Queues: controlq 0,
//! eventq 1 (its buffers stay posted: this device sends no events), txq 2,
//! rxq 3 (no input stream, so nothing is taken from it).
//!
//! Control requests are answered at once ([`control`]). Tx buffers complete
//! at playback pace from the stream's clock ([`clock`]), polled on every
//! vCPU thread before interrupts are raised (`x86_machine::Machine::raise_due`),
//! whether or not an audio service plays the bytes: a guest without sound
//! still never stalls. Their PCM goes to a bounded [`HostQueue`] that the
//! BSP's thread forwards (`client_audio`).

mod clock;
mod control;
mod host_queue;
mod query;
mod ring;
mod tx;

pub use host_queue::HostQueue;

use crate::virtio_mmio::{QueueCfg, VirtioDevice};
use crate::virtqueue::{process_notify, DescBuf};
use ring::{lengths, read_readable, write_writable};
use tx::Output;

const CONTROLQ: usize = 0;
const TXQ: usize = 2;
/// The longest control request (PCM_SET_PARAMS); one byte more is read so an
/// overlong request is seen as one.
const MAX_REQUEST: usize = 24;

pub struct SndDev {
    ctl_avail: u16,
    ctl_used: u16,
    out: Output,
}

impl SndDev {
    pub fn new() -> Self {
        Self {
            ctl_avail: 0,
            ctl_used: 0,
            out: Output::new(),
        }
    }

    /// Complete the tx buffers the stream has played. Whether used entries
    /// were published; the caller signals them.
    pub fn poll(&mut self, vm_id: usize) -> bool {
        if !self.playing() {
            return false;
        }
        self.out.poll(vm_id, ostd::syscall::sys_get_time())
    }

    /// The guest started the stream and has not stopped it.
    pub fn playing(&self) -> bool {
        self.out.pcm.playing()
    }

    /// PCM waiting for the audio service.
    pub fn host(&self) -> &HostQueue {
        &self.out.host
    }

    pub fn host_mut(&mut self) -> &mut HostQueue {
        &mut self.out.host
    }

    /// How many times the guest started the stream.
    pub fn starts(&self) -> u32 {
        self.out.starts
    }

    fn control(&mut self, vm_id: usize, qcfg: &QueueCfg) -> bool {
        let out = &mut self.out;
        let now_ns = ostd::syscall::sys_get_time();
        let mut tx_published = false;
        let published = process_notify(
            vm_id,
            qcfg,
            &mut self.ctl_avail,
            &mut self.ctl_used,
            |bufs: &[DescBuf]| {
                let (readable, writable) = lengths(bufs);
                let mut request = [0u8; MAX_REQUEST + 1];
                let len = readable.min(request.len() as u64) as usize;
                let got = read_readable(vm_id, bufs, 0, &mut request[..len]);
                let room = usize::try_from(writable).unwrap_or(usize::MAX);
                let reply = out.pcm.handle(&request[..got], room);
                // A RELEASE completes the stream's tx buffers before its reply.
                tx_published |= out.apply(reply.effect, vm_id, now_ns);
                write_writable(vm_id, bufs, &reply.bytes) as u32
            },
        );
        published > 0 || tx_published
    }
}

impl VirtioDevice for SndDev {
    fn device_id(&self) -> u32 {
        25
    }

    fn num_queues(&self) -> usize {
        4
    }

    fn notify(&mut self, q: usize, qcfg: &QueueCfg, vm_id: usize, _vcpu_id: usize) -> bool {
        match q {
            CONTROLQ => self.control(vm_id, qcfg),
            TXQ => self.out.notify(vm_id, qcfg, ostd::syscall::sys_get_time()),
            _ => false,
        }
    }

    fn config_read(&self, offset: usize) -> u32 {
        match offset {
            0 => control::JACKS,
            4 => control::STREAMS,
            8 => control::CHMAPS,
            _ => 0,
        }
    }

    fn reset(&mut self) {
        self.ctl_avail = 0;
        self.ctl_used = 0;
        self.out.reset();
    }
}
