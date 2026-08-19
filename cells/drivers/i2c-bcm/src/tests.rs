use crate::controller::{BcmBscCore, RegisterIo};
use crate::registers::{
    A, C, C_CLEAR, C_I2CEN, C_READ, C_ST, FIFO, S, S_CLEAR, S_DONE, S_RXD, S_TXD, S_TXW,
};
use hal_i2c::I2cError;
use std::collections::VecDeque;
use std::vec::Vec;
use types::ViError;

#[derive(Default)]
struct FakeIo {
    fifo_reads: VecDeque<u32>,
    script: VecDeque<u32>,
    writes: Vec<(usize, u32)>,
}

impl FakeIo {
    fn with(script: &[u32], fifo_reads: &[u32]) -> Self {
        Self {
            fifo_reads: VecDeque::new(),
            script: script.iter().copied().collect(),
            writes: Vec::new(),
        }
        .with_fifo(fifo_reads)
    }

    fn with_fifo(mut self, fifo_reads: &[u32]) -> Self {
        self.fifo_reads = fifo_reads.iter().copied().collect();
        self
    }
}

impl RegisterIo for FakeIo {
    fn read(&mut self, offset: usize) -> Result<u32, ViError> {
        Ok(match offset {
            FIFO => self.fifo_reads.pop_front().unwrap_or(0),
            S => self.script.pop_front().unwrap_or(S_DONE),
            _ => 0,
        })
    }

    fn write(&mut self, offset: usize, value: u32) -> Result<(), ViError> {
        self.writes.push((offset, value));
        Ok(())
    }
}

fn trace(
    script: &[u32],
    fifo_reads: &[u32],
    wr: &[u8],
    rd_len: usize,
) -> (Result<Vec<u8>, I2cError>, Vec<(usize, u32)>) {
    let mut core = BcmBscCore::new(FakeIo::with(script, fifo_reads));
    let mut rd = vec![0; rd_len];
    let result = if rd_len == 0 {
        core.write(0x44, wr).map(|_| rd.clone())
    } else if wr.is_empty() {
        core.read(0x44, &mut rd).map(|_| rd.clone())
    } else {
        core.write_read(0x44, wr, &mut rd).map(|_| rd.clone())
    };
    (result, core.io.writes)
}

#[test]
fn write_read_repeated_start_sequence() {
    let (result, writes) = trace(&[S_TXW, S_TXD, S_RXD, S_DONE], &[0x12], &[0x2C], 1);
    assert_eq!(result.unwrap(), vec![0x12]);
    let write_start = writes
        .iter()
        .position(|entry| *entry == (C, C_I2CEN | C_CLEAR | C_ST))
        .unwrap();
    let read_start = writes
        .iter()
        .position(|entry| *entry == (C, C_I2CEN | C_READ | C_ST))
        .unwrap();
    assert_eq!(writes[0], (C, C_I2CEN | C_CLEAR));
    assert!(writes.contains(&(A, 0x44)));
    assert!(writes.contains(&(FIFO, 0x2C)));
    assert!(write_start < read_start);
    assert!(!writes[write_start + 1..read_start].contains(&(S, S_CLEAR)));
    assert!(!writes[write_start + 1..read_start].contains(&(C, C_I2CEN | C_CLEAR)));
    assert_eq!(writes.last().copied(), Some((C, C_I2CEN | C_CLEAR)));
}

#[test]
fn write_read_waits_for_active_empty_fifo_before_queueing_write() {
    let (result, writes) = trace(&[0, 0, S_TXW, S_TXD, S_RXD, S_DONE], &[0x34], &[0x2C], 1);
    assert_eq!(result.unwrap(), vec![0x34]);

    let fifo_write = writes
        .iter()
        .position(|entry| *entry == (FIFO, 0x2C))
        .unwrap();
    let write_start = writes
        .iter()
        .position(|entry| *entry == (C, C_I2CEN | C_CLEAR | C_ST))
        .unwrap();
    assert!(write_start < fifo_write);
}

#[test]
fn address_nack_stops_and_returns_nack_address() {
    let (result, writes) = trace(&[1 << 8], &[], &[0x2C], 0);
    assert_eq!(result, Err(I2cError::NackAddress));
    assert!(writes.ends_with(&[(S, S_CLEAR), (C, C_I2CEN | C_CLEAR)]));
}

#[test]
fn data_nack_stops_and_returns_nack_data() {
    let (result, _) = trace(&[S_TXD, 1 << 8], &[], &[0x2C, 0x06], 0);
    assert_eq!(result, Err(I2cError::NackData));
}

#[test]
fn timeout_resets_ta_and_fifo() {
    let (result, writes) = trace(&[0; 1100], &[], &[0x2C], 0);
    assert_eq!(result, Err(I2cError::BusError));
    assert!(writes.ends_with(&[(S, S_CLEAR), (C, C_I2CEN | C_CLEAR)]));
}

#[test]
fn oversized_transfer_fails_before_touching_controller() {
    let mut core = BcmBscCore::new(FakeIo::default());
    let bytes = vec![0; u16::MAX as usize + 1];
    assert_eq!(core.write(0x44, &bytes), Err(I2cError::BusError));
    assert!(core.io.writes.is_empty());
}
