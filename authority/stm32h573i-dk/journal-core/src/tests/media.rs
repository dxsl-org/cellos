use crate::{BackendError, Counter, SlotRole, SlotStorage};
use std::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    CounterRead,
    CounterSeal,
    CounterIncrement,
    Read(SlotRole),
    Erase(SlotRole),
    Write(SlotRole),
}

pub struct FakeCounter {
    pub value: u64,
    pub fail_increment: bool,
    pub sealed: bool,
    pub events: Vec<Event>,
}
impl Counter for FakeCounter {
    fn read(&mut self) -> Result<u64, BackendError> {
        self.events.push(Event::CounterRead);
        Ok(self.value)
    }
    fn increment(&mut self) -> Result<u64, BackendError> {
        self.events.push(Event::CounterIncrement);
        if self.fail_increment {
            return Err(BackendError::Unavailable);
        }
        self.value += 1;
        Ok(self.value)
    }
    fn seal(&mut self) {
        self.events.push(Event::CounterSeal);
        self.sealed = true;
    }
    fn is_sealed(&mut self) -> Result<bool, BackendError> {
        Ok(self.sealed)
    }
}

#[derive(Clone, Copy)]
pub enum StorageFault {
    None,
    Erase,
    Write,
    PartialWrite,
    CorruptRead,
    BadLength,
}

pub struct FakeStorage {
    pub slots: [Vec<u8>; 2],
    pub fault: StorageFault,
    pub events: Vec<Event>,
}
impl FakeStorage {
    pub fn empty(fault: StorageFault) -> Self {
        Self {
            slots: [Vec::new(), Vec::new()],
            fault,
            events: Vec::new(),
        }
    }
}
impl SlotStorage for FakeStorage {
    fn read(&mut self, slot: SlotRole, output: &mut [u8]) -> Result<usize, BackendError> {
        self.events.push(Event::Read(slot));
        if matches!(self.fault, StorageFault::BadLength) {
            return Ok(output.len() + 1);
        }
        let value = &self.slots[slot as usize];
        output[..value.len()].copy_from_slice(value);
        if matches!(self.fault, StorageFault::CorruptRead) && !value.is_empty() {
            output[0] ^= 1;
        }
        Ok(value.len())
    }
    fn erase(&mut self, slot: SlotRole) -> Result<(), BackendError> {
        self.events.push(Event::Erase(slot));
        if matches!(self.fault, StorageFault::Erase) {
            return Err(BackendError::Unavailable);
        }
        self.slots[slot as usize].clear();
        Ok(())
    }
    fn write(&mut self, slot: SlotRole, value: &[u8]) -> Result<(), BackendError> {
        self.events.push(Event::Write(slot));
        if matches!(self.fault, StorageFault::Write) {
            return Err(BackendError::Unavailable);
        }
        if matches!(self.fault, StorageFault::PartialWrite) {
            self.slots[slot as usize] = value[..value.len() / 2].to_vec();
            return Err(BackendError::Unavailable);
        }
        self.slots[slot as usize] = value.to_vec();
        Ok(())
    }
}
