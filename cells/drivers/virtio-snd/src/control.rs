//! The control queue: one request at a time, polled to completion within a
//! budget (virtio 1.2 §5.14.6.1).

use crate::device::{CtrlError, Device, CONTROLQ};
use crate::proto;
use crate::ring::{CTRL_REQ_LEN, CTRL_REQ_OFF, CTRL_RESP_LEN, CTRL_RESP_OFF};
use crate::virtq::Mem;
use ostd::syscall::sys_get_time_ms;
use virtio_drivers::transport::Transport;

/// Budget of one control request.
const CTRL_BUDGET_MS: u64 = 1_000;
/// The same bound in yields when no monotonic clock is available.
const CTRL_FALLBACK_YIELDS: u32 = 100_000;

impl Device {
    /// Runs one control request and fills `resp` with the response. A
    /// timeout or a bad completion leaves the device unhealthy, and while the
    /// device may still own the control chain no request is issued.
    pub fn control(&mut self, req: &[u8], resp: &mut [u8]) -> Result<(), CtrlError> {
        assert!(req.len() <= CTRL_REQ_LEN && resp.len() <= CTRL_RESP_LEN);
        if self.ctrl.in_flight() != 0 {
            return Err(CtrlError::Timeout);
        }
        self.dma.write(CTRL_REQ_OFF, req);
        self.dma
            .write(CTRL_RESP_OFF, &[0; CTRL_RESP_LEN][..resp.len()]);
        let chain = [
            (CTRL_REQ_OFF, req.len(), false),
            (CTRL_RESP_OFF, resp.len(), true),
        ];
        self.ctrl.submit(&self.dma, 0, &chain);
        self.transport.notify(CONTROLQ);
        let start = sys_get_time_ms();
        let mut yields = 0u32;
        loop {
            match self.ctrl.pop_used(&self.dma) {
                Ok(Some(_)) => break,
                Ok(None) => {}
                Err(_) => {
                    self.healthy = false;
                    return Err(CtrlError::BadCompletion);
                }
            }
            let expired = match (start, sys_get_time_ms()) {
                (Some(start), Some(now)) => now.saturating_sub(start) >= CTRL_BUDGET_MS,
                _ => yields >= CTRL_FALLBACK_YIELDS,
            };
            if expired {
                self.healthy = false;
                return Err(CtrlError::Timeout);
            }
            yields += 1;
            ostd::task::yield_now();
        }
        self.dma.read(CTRL_RESP_OFF, resp);
        match proto::status(resp) {
            proto::S_OK => Ok(()),
            status => Err(CtrlError::Status(status)),
        }
    }
}
