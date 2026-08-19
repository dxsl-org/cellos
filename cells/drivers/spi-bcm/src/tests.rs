use crate::controller::{BcmSpiCore, RegisterIo};
use crate::registers::{CLK, CLOCK_DIVIDER, CS, CS_CLEAR, CS_DONE, CS_RXD, CS_TA, CS_TXD, FIFO};
use hal_spi::SpiError;
use std::collections::VecDeque;
use std::vec::Vec;
use types::ViError;

struct FakeIo {
    status: VecDeque<u32>,
    fifo_reads: VecDeque<u32>,
    writes: Vec<(usize, u32)>,
    fail_write_at: Option<usize>,
    fail_read_at: Option<usize>,
}

impl FakeIo {
    fn new(status: &[u32], fifo_reads: &[u32]) -> Self {
        Self {
            status: status.iter().copied().collect(),
            fifo_reads: fifo_reads.iter().copied().collect(),
            writes: Vec::new(),
            fail_write_at: None,
            fail_read_at: None,
        }
    }
}

impl RegisterIo for FakeIo {
    fn read(&mut self, offset: usize) -> Result<u32, ViError> {
        if self.fail_read_at == Some(self.writes.len()) {
            self.fail_read_at = None;
            return Err(ViError::IO);
        }
        Ok(match offset {
            CS => self.status.pop_front().unwrap_or(CS_DONE),
            FIFO => self.fifo_reads.pop_front().unwrap_or(0),
            _ => 0,
        })
    }

    fn write(&mut self, offset: usize, value: u32) -> Result<(), ViError> {
        if self.fail_write_at == Some(self.writes.len()) {
            self.fail_write_at = None;
            return Err(ViError::IO);
        }
        self.writes.push((offset, value));
        Ok(())
    }
}

fn trace(io: FakeIo, tx: &[u8], rx_len: usize) -> (Result<Vec<u8>, SpiError>, Vec<(usize, u32)>) {
    let mut core = BcmSpiCore::new(io);
    let mut rx = vec![0; rx_len];
    let result = if rx_len == 0 {
        core.write(tx).map(|_| rx.clone())
    } else {
        core.transfer(tx, &mut rx).map(|_| rx.clone())
    };
    (result, core.io.writes)
}

#[test]
fn mode0_transfer_programs_cs_clk_len() {
    let (result, writes) = trace(
        FakeIo::new(&[CS_TXD, CS_RXD, CS_TXD, CS_RXD, CS_DONE], &[0xAB, 0xCD]),
        &[0x9F, 0x00],
        2,
    );
    assert_eq!(result.unwrap(), vec![0xAB, 0xCD]);
    assert!(writes.contains(&(CLK, CLOCK_DIVIDER)));
    assert!(writes.contains(&(CS, CS_CLEAR)));
    assert!(writes.contains(&(CS, CS_CLEAR | CS_TA)));
    assert!(writes.contains(&(FIFO, 0x9F)));
    assert!(writes.contains(&(FIFO, 0x00)));
    assert_eq!(writes.last().copied(), Some((CS, CS_CLEAR)));
}

#[test]
fn timeout_clears_ta_and_cs() {
    let (result, writes) = trace(FakeIo::new(&[0; 1100], &[]), &[0x9F], 0);
    assert_eq!(result, Err(SpiError::TransferError));
    assert_eq!(writes.last().copied(), Some((CS, CS_CLEAR)));
}

#[test]
fn write_error_still_deasserts_cs() {
    let mut io = FakeIo::new(&[CS_TXD], &[]);
    io.fail_write_at = Some(3);
    let (result, writes) = trace(io, &[0x9F], 0);
    assert!(result.is_err());
    assert_eq!(writes.last().copied(), Some((CS, CS_CLEAR)));
}

#[test]
fn read_error_still_deasserts_cs() {
    let mut io = FakeIo::new(&[CS_TXD, CS_RXD], &[]);
    io.fail_read_at = Some(4);
    let (result, writes) = trace(io, &[0x9F], 1);
    assert!(result.is_err());
    assert_eq!(writes.last().copied(), Some((CS, CS_CLEAR)));
}

#[test]
fn explicit_chip_select_holds_ta_until_deselect() {
    let io = FakeIo::new(&[CS_TXD, CS_RXD, CS_DONE], &[0xAB]);
    let mut core = BcmSpiCore::new(io);
    core.cs_select().unwrap();
    core.transfer(&[0x9F], &mut [0]).unwrap();
    assert_eq!(core.io.writes.last().copied(), Some((FIFO, 0x9F)));
    core.cs_deselect().unwrap();
    assert_eq!(core.io.writes.last().copied(), Some((CS, CS_CLEAR)));
}

#[test]
fn oversized_transfer_fails_before_selecting_device() {
    let io = FakeIo::new(&[], &[]);
    let mut core = BcmSpiCore::new(io);
    let bytes = vec![0; u16::MAX as usize + 1];
    assert_eq!(core.write(&bytes), Err(SpiError::TransferError));
    assert!(core.io.writes.is_empty());
}
