use crate::registers::{
    CLK, CLOCK_DIVIDER, CS, CS_CLEAR, CS_DONE, CS_RXD, CS_TA, CS_TXD, FIFO, POLL_BUDGET,
};
use hal_spi::SpiError;
#[cfg(feature = "runtime")]
use ostd::mmio::MmioRegion;
use types::ViError;

pub(crate) trait RegisterIo {
    fn read(&mut self, offset: usize) -> Result<u32, ViError>;
    fn write(&mut self, offset: usize, value: u32) -> Result<(), ViError>;
}

#[cfg(feature = "runtime")]
pub(crate) struct MmioBus {
    mmio: MmioRegion,
}

#[cfg(feature = "runtime")]
impl MmioBus {
    pub(crate) fn new(mmio: MmioRegion) -> Self {
        Self { mmio }
    }
}

#[cfg(feature = "runtime")]
impl RegisterIo for MmioBus {
    fn read(&mut self, offset: usize) -> Result<u32, ViError> {
        self.mmio.read_u32(offset)
    }

    fn write(&mut self, offset: usize, value: u32) -> Result<(), ViError> {
        self.mmio.write_u32(offset, value)
    }
}

pub(crate) struct BcmSpiCore<I> {
    pub(crate) io: I,
    hold_cs: bool,
    active: bool,
}

impl<I: RegisterIo> BcmSpiCore<I> {
    pub(crate) fn new(io: I) -> Self {
        Self {
            io,
            hold_cs: false,
            active: false,
        }
    }

    pub(crate) fn cs_select(&mut self) -> Result<(), SpiError> {
        if !self.active {
            self.begin()?;
        }
        self.hold_cs = true;
        Ok(())
    }

    pub(crate) fn cs_deselect(&mut self) -> Result<(), SpiError> {
        self.hold_cs = false;
        if self.active {
            self.end()?;
        }
        Ok(())
    }

    pub(crate) fn transfer(&mut self, tx: &[u8], rx: &mut [u8]) -> Result<(), SpiError> {
        self.run(tx, rx)
    }

    pub(crate) fn write(&mut self, bytes: &[u8]) -> Result<(), SpiError> {
        self.run(bytes, &mut [])
    }

    fn run(&mut self, tx: &[u8], rx: &mut [u8]) -> Result<(), SpiError> {
        let total = tx.len().max(rx.len());
        if total == 0 {
            return Ok(());
        }
        if total > u16::MAX as usize {
            return Err(SpiError::TransferError);
        }
        let auto = !self.active;
        if auto {
            self.begin()?;
        }
        for index in 0..total {
            self.wait(CS_TXD)?;
            let byte = tx.get(index).copied().unwrap_or(0);
            self.io
                .write(FIFO, byte as u32)
                .map_err(|_| SpiError::TransferError)
                .or_else(|err| self.abort(err))?;
            self.wait(CS_RXD)?;
            let value = self
                .io
                .read(FIFO)
                .map_err(|_| SpiError::TransferError)
                .or_else(|err| self.abort(err))? as u8;
            if index < rx.len() {
                rx[index] = value;
            }
        }
        self.wait(CS_DONE)?;
        if auto && !self.hold_cs {
            self.end()?;
        }
        Ok(())
    }

    fn begin(&mut self) -> Result<(), SpiError> {
        self.io
            .write(CLK, CLOCK_DIVIDER)
            .map_err(|_| SpiError::BusError)?;
        self.active = true;
        self.io
            .write(CS, CS_CLEAR)
            .map_err(|_| SpiError::BusError)
            .or_else(|err| self.abort(err))?;
        self.io
            .write(CS, CS_CLEAR | CS_TA)
            .map_err(|_| SpiError::BusError)
            .or_else(|err| self.abort(err))?;
        Ok(())
    }

    fn end(&mut self) -> Result<(), SpiError> {
        self.active = false;
        self.io
            .write(CS, CS_CLEAR)
            .map_err(|_| SpiError::TransferError)
    }

    fn abort<T>(&mut self, err: SpiError) -> Result<T, SpiError> {
        let _ = self.end();
        Err(err)
    }

    fn wait(&mut self, mask: u32) -> Result<(), SpiError> {
        for _ in 0..POLL_BUDGET {
            let status = self
                .io
                .read(CS)
                .map_err(|_| SpiError::BusError)
                .or_else(|err| self.abort(err))?;
            if status & mask != 0 {
                return Ok(());
            }
        }
        self.abort(SpiError::TransferError)
    }
}
