use redoxfs::{Disk, BLOCK_SIZE};
use std::{cell::RefCell, rc::Rc};
use syscall::error::{Error, Result, EIO};

const DISK_BYTES: usize = 8 * 1024 * 1024;
const SECTOR_BYTES: usize = 512;

#[derive(Clone)]
pub struct FailingMemDisk(Rc<RefCell<DiskState>>);

struct DiskState {
    bytes: Vec<u8>,
    calls: Vec<(u64, usize)>,
    sectors: usize,
    fail_before: Option<usize>,
}

impl FailingMemDisk {
    pub fn new() -> Self {
        Self(Rc::new(RefCell::new(DiskState {
            bytes: vec![0; DISK_BYTES],
            calls: Vec::new(),
            sectors: 0,
            fail_before: None,
        })))
    }

    pub fn image(&self) -> Vec<u8> {
        self.0.borrow().bytes.clone()
    }

    pub fn restore(&self, image: &[u8]) {
        let mut state = self.0.borrow_mut();
        state.bytes.copy_from_slice(image);
        state.calls.clear();
        state.sectors = 0;
        state.fail_before = None;
    }

    pub fn arm(&self, ordinal: Option<usize>) {
        let mut state = self.0.borrow_mut();
        state.calls.clear();
        state.sectors = 0;
        state.fail_before = ordinal;
    }

    pub fn trace(&self) -> (Vec<(u64, usize)>, usize) {
        let state = self.0.borrow();
        (state.calls.clone(), state.sectors)
    }
}

impl Disk for FailingMemDisk {
    unsafe fn read_at(&mut self, block: u64, buffer: &mut [u8]) -> Result<usize> {
        let state = self.0.borrow();
        let start = block as usize * BLOCK_SIZE as usize;
        let end = start.checked_add(buffer.len()).ok_or(Error::new(EIO))?;
        let source = state.bytes.get(start..end).ok_or(Error::new(EIO))?;
        buffer.copy_from_slice(source);
        Ok(buffer.len())
    }

    unsafe fn write_at(&mut self, block: u64, buffer: &[u8]) -> Result<usize> {
        let mut state = self.0.borrow_mut();
        state.calls.push((block, buffer.len()));
        for (index, chunk) in buffer.chunks(SECTOR_BYTES).enumerate() {
            state.sectors += 1;
            if state.fail_before == Some(state.sectors) {
                return Err(Error::new(EIO));
            }
            let start = block as usize * BLOCK_SIZE as usize + index * SECTOR_BYTES;
            let end = start.checked_add(SECTOR_BYTES).ok_or(Error::new(EIO))?;
            let target = state.bytes.get_mut(start..end).ok_or(Error::new(EIO))?;
            target.fill(0);
            target[..chunk.len()].copy_from_slice(chunk);
        }
        Ok(buffer.len())
    }

    fn size(&mut self) -> Result<u64> {
        Ok(DISK_BYTES as u64)
    }
}
