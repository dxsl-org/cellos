use crate::registers::{
    A, C, CLKT, C_CLEAR, C_I2CEN, C_READ, C_ST, DLEN, FIFO, POLL_BUDGET, S, S_CLEAR, S_CLKT,
    S_DONE, S_ERR, S_RXD, S_TXD, S_TXW,
};
use hal_i2c::I2cError;
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

pub(crate) struct BcmBscCore<I> {
    pub(crate) io: I,
}

impl<I: RegisterIo> BcmBscCore<I> {
    pub(crate) fn new(io: I) -> Self {
        Self { io }
    }

    pub(crate) fn write(&mut self, addr: u8, bytes: &[u8]) -> Result<(), I2cError> {
        if bytes.is_empty() {
            return Ok(());
        }
        self.start(addr, bytes.len(), false)?;
        let written = self.push_tx(bytes)?;
        self.finish(written, 0)
    }

    pub(crate) fn read(&mut self, addr: u8, buf: &mut [u8]) -> Result<(), I2cError> {
        if buf.is_empty() {
            return Ok(());
        }
        self.start(addr, buf.len(), true)?;
        let read = self.pull_rx(buf)?;
        self.finish(0, read)
    }

    pub(crate) fn write_read(
        &mut self,
        addr: u8,
        wr: &[u8],
        rd: &mut [u8],
    ) -> Result<(), I2cError> {
        if wr.is_empty() && rd.is_empty() {
            return Ok(());
        }
        if wr.is_empty() {
            return self.read(addr, rd);
        }
        if rd.is_empty() {
            return self.write(addr, wr);
        }
        if rd.len() > u16::MAX as usize {
            return Err(I2cError::BusError);
        }
        self.start(addr, wr.len(), false)?;
        // The FIFO starts empty. TXW therefore proves the write transfer is
        // active before we queue its bytes and arm the repeated start.
        let status = self.wait(S_TXW)?;
        if status & S_ERR != 0 {
            return Err(self.nack(0, 0));
        }
        let written = self.push_tx(wr)?;
        self.write_reg(DLEN, rd.len() as u32)?;
        self.write_reg(C, C_I2CEN | C_READ | C_ST)?;
        let read = self.pull_rx(rd)?;
        self.finish(written, read)
    }

    fn start(&mut self, addr: u8, len: usize, read: bool) -> Result<(), I2cError> {
        if addr > 0x7F || len > u16::MAX as usize {
            return Err(I2cError::BusError);
        }
        self.write_reg(C, C_I2CEN | C_CLEAR)?;
        self.write_reg(S, S_CLEAR)?;
        self.write_reg(CLKT, 0)?;
        self.write_reg(A, addr as u32)?;
        self.write_reg(DLEN, len as u32)?;
        self.write_reg(C, C_I2CEN | C_CLEAR | if read { C_READ } else { 0 } | C_ST)
    }

    fn push_tx(&mut self, bytes: &[u8]) -> Result<usize, I2cError> {
        for (index, byte) in bytes.iter().enumerate() {
            let status = self.wait(S_TXD)?;
            if status & S_ERR != 0 {
                return Err(self.nack(index, 0));
            }
            self.write_reg(FIFO, *byte as u32)?;
        }
        Ok(bytes.len())
    }

    fn pull_rx(&mut self, buf: &mut [u8]) -> Result<usize, I2cError> {
        for (index, slot) in buf.iter_mut().enumerate() {
            let status = self.wait(S_RXD)?;
            if status & S_ERR != 0 {
                return Err(self.nack(0, index));
            }
            *slot = self.read_reg(FIFO)? as u8;
        }
        Ok(buf.len())
    }

    fn finish(&mut self, written: usize, read: usize) -> Result<(), I2cError> {
        let status = self.wait(S_DONE)?;
        if status & S_ERR != 0 {
            return Err(self.nack(written, read));
        }
        self.cleanup()
    }

    fn wait(&mut self, mask: u32) -> Result<u32, I2cError> {
        for _ in 0..POLL_BUDGET {
            let status = self.read_reg(S)?;
            if status & S_CLKT != 0 {
                return self.bus_error();
            }
            if status & (mask | S_ERR) != 0 {
                return Ok(status);
            }
        }
        self.bus_error()
    }

    fn read_reg(&mut self, offset: usize) -> Result<u32, I2cError> {
        self.io.read(offset).or_else(|_| self.bus_error())
    }

    fn write_reg(&mut self, offset: usize, value: u32) -> Result<(), I2cError> {
        self.io.write(offset, value).or_else(|_| self.bus_error())
    }

    fn nack(&mut self, written: usize, read: usize) -> I2cError {
        let _ = self.cleanup();
        if written == 0 && read == 0 {
            I2cError::NackAddress
        } else {
            I2cError::NackData
        }
    }

    fn bus_error<T>(&mut self) -> Result<T, I2cError> {
        let _ = self.cleanup();
        Err(I2cError::BusError)
    }

    fn cleanup(&mut self) -> Result<(), I2cError> {
        self.io.write(S, S_CLEAR).map_err(|_| I2cError::BusError)?;
        self.io
            .write(C, C_I2CEN | C_CLEAR)
            .map_err(|_| I2cError::BusError)
    }
}
