//! The `AUDIO` receive loop: requests in, replies out, the device fed every
//! turn.

use crate::device::Device;
use crate::log;
use crate::pcm::{PcmQueue, CAPACITY};
use crate::service::{Event, Grants, Service};
use ostd::ipc::Incoming;
use ostd::syscall::{
    sys_get_time_ms, sys_grant_copy_rows_owned_by, sys_grant_slice_with_len_owned_by, sys_recv,
    sys_recv_timeout, GrantRows, SyscallResult,
};

/// Grant access as the kernel-reported sender: the Grant must still be that
/// tid's, and each copy leases it until this task's next receive.
struct KernelGrants;

impl Grants for KernelGrants {
    fn len_owned_by(&mut self, grant: u32, owner: usize) -> Option<usize> {
        sys_grant_slice_with_len_owned_by(grant as usize, owner).map(|(_, len)| len)
    }

    fn copy(&mut self, grant: u32, owner: usize, offset: usize, dst: &mut [u8]) -> bool {
        if dst.is_empty() {
            return true;
        }
        let rows = GrantRows {
            src_offset: offset,
            src_stride: 0,
            dst_offset: 0,
            dst_stride: 0,
            row_len: dst.len(),
            rows: 1,
        };
        sys_grant_copy_rows_owned_by(grant as usize, owner, rows, dst).is_some()
    }
}

/// Serves `AUDIO` on `device` forever. While the stream runs or PCM waits,
/// each receive gives up after one scheduler tick (10 ms) so the device is
/// fed between requests; otherwise it blocks.
pub fn serve(mut device: Device) -> ! {
    let mut service = Service::new(PcmQueue::new(CAPACITY));
    let mut buf = [0u8; wire::ipc::IPC_BUF_SIZE];
    // Stands in for the clock when `GetTime` is unavailable: 10 ms a turn.
    let mut turns = 0u64;
    loop {
        // Zeroed so a short message (CLOSE is one byte) decodes with zeros.
        buf.fill(0);
        let received = if device.running() || !service.queue.is_empty() {
            sys_recv_timeout(0, &mut buf, 1)
        } else {
            sys_recv(0, &mut buf)
        };
        if let SyscallResult::Ok(sender) = received {
            if sender != 0 {
                // Before anything else can receive: the token of a call.
                let incoming = Incoming::current(sender);
                let outcome = service.handle(&buf, sender, &mut KernelGrants, device.view());
                if let Some(reply) = outcome.reply {
                    let _ = incoming.reply(&reply.encode());
                }
                outcome.events.into_iter().flatten().for_each(log_event);
            }
        }
        turns += 10;
        let now = sys_get_time_ms().unwrap_or(turns);
        let held = service.held();
        device.pump(&mut service.queue, held, now);
    }
}

fn log_event(event: Event) {
    match event {
        Event::Opened { tid } => log(format_args!("[virtio-snd] stream opened by tid {tid}\n")),
        Event::Closed { tid, totals } => {
            log(format_args!("[virtio-snd] stream closed (tid {tid})\n"));
            log(format_args!(
                "[virtio-snd] played {} bytes, dropped {}\n",
                totals.played, totals.dropped
            ));
        }
    }
}
