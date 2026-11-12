//! The one-page DMA buffer every sector read/write goes through (audit 08 S-11).
//!
//! A timed-out command is not aborted: the device may still DMA a late READ into the
//! page, or fetch a late WRITE's payload from it. Reusing that page for the next
//! request would let the stale transfer overwrite the next result or write the next
//! payload to the old LBA, so a page with a command outstanding is retired - kept
//! allocated and authorized for the driver's lifetime, never freed - and a fresh
//! page takes its place.

use crate::dma::AuthorizedDma;
use crate::layout;
use alloc::vec::Vec;
use ostd::dma::DmaBuf;
use ostd::io::print_fmt;
use types::block_wire::MAX_RUN_SECTORS;

/// Retired pages kept before the driver stops serving I/O. Each holds one page, and
/// a device that keeps abandoning commands is not one to keep feeding memory.
const QUARANTINE_LIMIT: usize = 16;

pub struct BounceBuffer {
    bdf: u32,
    /// `None` once the driver refuses further I/O.
    live: Option<AuthorizedDma<DmaBuf>>,
    /// Pages a timed-out command may still access; held so they are never freed.
    retired: Vec<AuthorizedDma<DmaBuf>>,
}

impl BounceBuffer {
    /// Allocate and authorize the first page for the device at `bdf`.
    pub fn new(bdf: u32) -> Result<Self, &'static str> {
        Ok(Self {
            bdf,
            live: Some(Self::fresh(bdf)?),
            // Reserved up front so retiring a page never allocates mid-request.
            retired: Vec::with_capacity(QUARANTINE_LIMIT),
        })
    }

    fn fresh(bdf: u32) -> Result<AuthorizedDma<DmaBuf>, &'static str> {
        let buf = DmaBuf::alloc(1).ok_or("allocation failed")?;
        let buf = match AuthorizedDma::authorize(buf, |buf| buf.authorize(bdf)) {
            Ok(buf) => buf,
            Err(_) => return Err("authorization failed"),
        };
        // A full `OP_READ_RUN` must fit one PRP1 transfer, or every run would be
        // refused while single-sector reads still worked.
        if layout::blocks_in_one_prp(buf.iova()) < usize::from(MAX_RUN_SECTORS) {
            // Dropped without `free` (DmaBuf has no Drop): a page authorized for the
            // device is never handed back to the allocator.
            return Err("iova would need a second PRP");
        }
        Ok(buf)
    }

    /// The page for the next request; `None` once I/O is refused.
    pub fn page(&self) -> Option<&AuthorizedDma<DmaBuf>> {
        self.live.as_ref()
    }

    /// The device may still access the current page: retire it and switch to a fresh
    /// one, or refuse further I/O when the limit is reached or no page is available.
    pub fn quarantine(&mut self) {
        let Some(old) = self.live.take() else {
            return;
        };
        self.retired.push(old);
        if self.retired.len() >= QUARANTINE_LIMIT {
            let _ = print_fmt(format_args!(
                "[nvme] {} abandoned commands: refusing further I/O\n",
                self.retired.len()
            ));
            return;
        }
        match Self::fresh(self.bdf) {
            Ok(buf) => {
                let _ = print_fmt(format_args!(
                    "[nvme] io buffer quarantined after timeout; new iova=0x{:x}\n",
                    buf.iova()
                ));
                self.live = Some(buf);
            }
            Err(why) => {
                let _ = print_fmt(format_args!(
                    "[nvme] replacement io buffer {}: refusing further I/O\n",
                    why
                ));
            }
        }
    }
}
