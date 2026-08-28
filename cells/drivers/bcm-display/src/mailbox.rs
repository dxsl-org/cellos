use crate::{
    matches_property_response, PropertyBuffer, MAILBOX_READ_STATUS, MAILBOX_WRITE_STATUS,
    PROPERTY_CHANNEL,
};
use ostd::dma::DmaBuf;
use ostd::mmio::MmioRegion;
use ostd::syscall::{sys_grant_cache_sync_begin, sys_grant_cache_sync_complete};
use types::{ViError, ViResult};

const MBOX_READ: usize = 0x00;
const MBOX_WRITE: usize = 0x20;
const MBOX_FULL: u32 = 0x8000_0000;
const MBOX_EMPTY: u32 = 0x4000_0000;
const VC_UNCACHED_ALIAS: u32 = 0xC000_0000;
const POLL_LIMIT: usize = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransportState {
    Ready,
    Submitted,
    Poisoned,
}

/// BCM283x mailbox transport backed by one cell-lifetime property page.
///
/// After submission, an I/O failure leaves VideoCore ownership indeterminate.
/// The page is retained and this transport must never submit another request.
pub struct BcmMailbox {
    region: MmioRegion,
    dma: DmaBuf,
    state: TransportState,
}

impl BcmMailbox {
    pub fn open() -> ViResult<Self> {
        let base = hal_soc_bcm27xx::BCM2837.mmio.mailbox_base;
        let size = hal_soc_bcm27xx::BCM2837.mmio.mailbox_grant_size;
        let region = ostd::mmio::request_region(base, size)?;
        let dma = DmaBuf::alloc(1).ok_or(ViError::OutOfMemory)?;
        diagnostic_number("[bcm-display] mailbox DMA page bytes ", dma.size());
        Ok(Self {
            region,
            dma,
            state: TransportState::Ready,
        })
    }

    pub fn call<const N: usize>(&mut self, buffer: &mut PropertyBuffer<N>) -> ViResult<()> {
        if self.state == TransportState::Poisoned {
            ostd::io::println("[bcm-display] mailbox diagnostic: transport already poisoned");
            return Err(ViError::IO);
        }
        let byte_len = N
            .checked_mul(core::mem::size_of::<u32>())
            .ok_or(ViError::InvalidInput)?;
        if byte_len == 0 || byte_len > self.dma.size() {
            return Err(ViError::InvalidInput);
        }
        let phys = u32::try_from(self.dma.phys()).map_err(|_| ViError::InvalidInput)?;
        if phys & 0xC000_000F != 0 {
            diagnostic_number(
                "[bcm-display] mailbox diagnostic: invalid DMA address ",
                phys as usize,
            );
            return Err(ViError::InvalidInput);
        }
        let dma_words = self.dma.virt().cast::<u32>();
        // SAFETY: `dma_words` addresses this mailbox's owned DmaBuf, and
        // `byte_len` proved N words fit before this copy. No Rust reference to
        // the DMA page survives this operation or the following device access.
        unsafe { core::ptr::copy_nonoverlapping(buffer.data.as_ptr(), dma_words, N) };
        let bus_addr = phys | VC_UNCACHED_ALIAS;
        if let Err(error) = self.drain_stale() {
            ostd::io::println("[bcm-display] mailbox diagnostic: drain failed");
            return Err(error);
        }
        if let Err(error) = self.wait_not_full() {
            ostd::io::println("[bcm-display] mailbox diagnostic: write-ready wait failed");
            return Err(error);
        }
        let Some(token) = sys_grant_cache_sync_begin(self.dma.phys(), 0, byte_len) else {
            ostd::io::println("[bcm-display] mailbox diagnostic: cache-sync begin denied");
            return Err(ViError::IO);
        };
        ostd::io::println("[bcm-display] mailbox cache-sync begin accepted");
        if self
            .region
            .write::<u32>(MBOX_WRITE, (bus_addr & !0xF) | PROPERTY_CHANNEL)
            .is_err()
        {
            ostd::io::println("[bcm-display] mailbox diagnostic: submit MMIO write failed");
            return Err(self.poison());
        }
        self.state = TransportState::Submitted;
        if self.wait_matching_response(bus_addr).is_err() {
            diagnostic_number(
                "[bcm-display] mailbox diagnostic: response timeout for bus address ",
                bus_addr as usize,
            );
            return Err(self.poison());
        }
        if !sys_grant_cache_sync_complete(token) {
            diagnostic_number(
                "[bcm-display] mailbox diagnostic: cache-sync completion denied for token ",
                token,
            );
            return Err(self.poison());
        }
        ostd::io::println("[bcm-display] mailbox cache-sync exact completion accepted");
        self.state = TransportState::Ready;
        // SAFETY: successful exact completion invalidated this DmaBuf range and
        // released its device pin. `dma_words` still points to the owned page;
        // the checked N words fit and no device access remains authorized.
        unsafe { core::ptr::copy_nonoverlapping(dma_words, buffer.data.as_mut_ptr(), N) };
        Ok(())
    }

    fn drain_stale(&self) -> ViResult<()> {
        for _ in 0..POLL_LIMIT {
            if self.region.read::<u32>(MAILBOX_READ_STATUS)? & MBOX_EMPTY != 0 {
                return Ok(());
            }
            let _ = self.region.read::<u32>(MBOX_READ)?;
        }
        Err(ViError::IO)
    }

    fn wait_not_full(&self) -> ViResult<()> {
        for _ in 0..POLL_LIMIT {
            if self.region.read::<u32>(MAILBOX_WRITE_STATUS)? & MBOX_FULL == 0 {
                return Ok(());
            }
        }
        Err(ViError::IO)
    }

    fn wait_matching_response(&self, bus_addr: u32) -> ViResult<()> {
        for _ in 0..POLL_LIMIT {
            if self.region.read::<u32>(MAILBOX_READ_STATUS)? & MBOX_EMPTY != 0 {
                continue;
            }
            let response = self.region.read::<u32>(MBOX_READ)?;
            if matches_property_response(response, bus_addr) {
                return Ok(());
            }
        }
        Err(ViError::IO)
    }

    fn poison(&mut self) -> ViError {
        self.state = TransportState::Poisoned;
        ViError::IO
    }
}

fn diagnostic_number(label: &str, value: usize) {
    ostd::io::print(label);
    ostd::io::print_usize(value);
    ostd::io::println("");
}
