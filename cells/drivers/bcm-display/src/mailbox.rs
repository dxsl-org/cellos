use crate::PropertyBuffer;
use ostd::mmio::MmioRegion;
use types::{ViError, ViResult};

const MBOX_READ: usize = 0x00;
const MBOX_STATUS: usize = 0x18;
const MBOX_WRITE: usize = 0x20;

const MBOX_FULL: u32 = 0x8000_0000;
const MBOX_EMPTY: u32 = 0x4000_0000;

const CHANNEL_PROPERTY: u32 = 8;
const VC_UNCACHED_ALIAS: u32 = 0xC000_0000;

pub struct BcmMailbox {
    region: MmioRegion,
}

impl BcmMailbox {
    pub fn new(region: MmioRegion) -> Self {
        Self { region }
    }

    pub fn open() -> ViResult<Self> {
        let base = hal_soc_bcm27xx::BCM2837.mmio.mailbox_base;
        let size = hal_soc_bcm27xx::BCM2837.mmio.mailbox_grant_size;
        let region = ostd::mmio::request_region(base, size)?;
        Ok(Self::new(region))
    }

    pub fn call<const N: usize>(&self, buffer: &mut PropertyBuffer<N>) -> ViResult<()> {
        let ptr = buffer.data.as_mut_ptr() as usize;
        let bus_addr = (ptr as u32) | VC_UNCACHED_ALIAS;

        // 1. Wait until mailbox is not full
        let mut timeout = 1_000_000;
        while (self.region.read::<u32>(MBOX_STATUS)? & MBOX_FULL) != 0 {
            timeout -= 1;
            if timeout == 0 {
                return Err(ViError::IO);
            }
        }

        // 2. Write buffer bus address with channel
        let write_val = (bus_addr & !0xF) | (CHANNEL_PROPERTY & 0xF);
        self.region.write::<u32>(MBOX_WRITE, write_val)?;

        // 3. Wait for response on matching channel
        timeout = 1_000_000;
        loop {
            while (self.region.read::<u32>(MBOX_STATUS)? & MBOX_EMPTY) != 0 {
                timeout -= 1;
                if timeout == 0 {
                    return Err(ViError::IO);
                }
            }

            let resp = self.region.read::<u32>(MBOX_READ)?;
            if (resp & 0xF) == (CHANNEL_PROPERTY & 0xF) {
                if (resp & !0xF) == (bus_addr & !0xF) {
                    break;
                }
            }
        }

        // 4. Verify request/response success code (0x8000_0000)
        if buffer.data[1] == 0x8000_0000 {
            Ok(())
        } else {
            Err(ViError::InvalidInput)
        }
    }
}
