use crate::{
    decode_record, encode_record, recover, ExpectedIdentity, FullRecord, RecordAuthenticator,
    RecoveredRecord, RecoveryError, SlotRole, RECORD_MAX,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendError {
    Unavailable,
}

pub trait Counter {
    fn read(&mut self) -> Result<u64, BackendError>;
    fn increment(&mut self) -> Result<u64, BackendError>;
    /// Irreversibly prevent this counter domain from serving after reboot.
    fn seal(&mut self);
    fn is_sealed(&mut self) -> Result<bool, BackendError>;
}

pub trait SlotStorage {
    fn read(&mut self, slot: SlotRole, output: &mut [u8]) -> Result<usize, BackendError>;
    fn erase(&mut self, slot: SlotRole) -> Result<(), BackendError>;
    fn write(&mut self, slot: SlotRole, value: &[u8]) -> Result<(), BackendError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalError {
    Counter,
    Storage,
    CounterMismatch,
    InvalidRecord,
    Sealed,
    Recovery(RecoveryError),
}

pub struct Journal<C, S, A> {
    counter: C,
    storage: S,
    authenticator: A,
    expected: ExpectedIdentity,
    sealed: bool,
}

impl<C: Counter, S: SlotStorage, A: RecordAuthenticator> Journal<C, S, A> {
    /// Construct a journal over protected counter, slot, and authenticator seams.
    pub const fn new(counter: C, storage: S, authenticator: A, expected: ExpectedIdentity) -> Self {
        Self {
            counter,
            storage,
            authenticator,
            expected,
            sealed: false,
        }
    }

    /// Recover one exact counter-matching slot or return a sealing error.
    pub fn recover(&mut self) -> Result<RecoveredRecord, JournalError> {
        self.ensure_open()?;
        let counter = self.counter.read().map_err(|_| JournalError::Counter)?;
        let mut first = [0u8; RECORD_MAX];
        let mut second = [0u8; RECORD_MAX];
        let first_len = match self.storage.read(SlotRole::A, &mut first) {
            Ok(value) => value,
            Err(_) if counter != 0 => return Err(self.seal()),
            Err(_) => return Err(JournalError::Storage),
        };
        let second_len = match self.storage.read(SlotRole::B, &mut second) {
            Ok(value) => value,
            Err(_) if counter != 0 => return Err(self.seal()),
            Err(_) => return Err(JournalError::Storage),
        };
        let Some(first) = first.get(..first_len) else {
            return Err(if counter == 0 {
                JournalError::Storage
            } else {
                self.seal()
            });
        };
        let Some(second) = second.get(..second_len) else {
            return Err(if counter == 0 {
                JournalError::Storage
            } else {
                self.seal()
            });
        };
        match recover(
            counter,
            [present(first), present(second)],
            &self.authenticator,
            &self.expected,
        ) {
            Ok(value) => Ok(value),
            Err(error) if counter == 0 => Err(JournalError::Recovery(error)),
            Err(_) => Err(self.seal()),
        }
    }

    /// Persist `next` using increment, inactive-slot write, and read-back order.
    /// Any failure after counter increment returns `Sealed` because rollback is
    /// no longer provable.
    pub fn commit(&mut self, mut next: FullRecord) -> Result<RecoveredRecord, JournalError> {
        self.ensure_open()?;
        let observed = self.counter.read().map_err(|_| JournalError::Counter)?;
        let current = if observed == 0 {
            None
        } else {
            Some(self.recover()?)
        };
        let next_counter = match observed.checked_add(1) {
            Some(value) => value,
            None => return Err(self.seal()),
        };
        let target = current
            .as_ref()
            .map(|value| value.record().slot_role.other())
            .unwrap_or(SlotRole::A);
        next.counter = next_counter;
        next.slot_role = target;
        validate_identity(&next, &self.expected)?;
        next.validate_successor(current.as_ref().map(RecoveredRecord::record))
            .map_err(|_| JournalError::InvalidRecord)?;
        let mut encoded = [0u8; RECORD_MAX];
        let length = encode_record(&next, &self.authenticator, &mut encoded)
            .map_err(|_| JournalError::InvalidRecord)?;
        match self.counter.increment() {
            Ok(value) if value == next_counter => {}
            _ => return Err(self.seal()),
        }
        if self.storage.erase(target).is_err() {
            return Err(self.seal());
        }
        if self.storage.write(target, &encoded[..length]).is_err() {
            return Err(self.seal());
        }
        let mut read_back = [0u8; RECORD_MAX];
        let read_len = match self.storage.read(target, &mut read_back) {
            Ok(value) => value,
            Err(_) => return Err(self.seal()),
        };
        let Some(read_back) = read_back.get(..read_len) else {
            return Err(self.seal());
        };
        let decoded = match decode_record(read_back, &self.authenticator) {
            Ok(value) => value,
            Err(_) => return Err(self.seal()),
        };
        if decoded != next || !matches_identity(&decoded, &self.expected) {
            return Err(self.seal());
        }
        Ok(RecoveredRecord { record: decoded })
    }

    fn ensure_open(&mut self) -> Result<(), JournalError> {
        if self.sealed
            || self
                .counter
                .is_sealed()
                .map_err(|_| JournalError::Counter)?
        {
            return Err(JournalError::Sealed);
        }
        Ok(())
    }

    fn seal(&mut self) -> JournalError {
        self.counter.seal();
        self.sealed = true;
        JournalError::Sealed
    }

    pub fn into_parts(self) -> (C, S, A) {
        (self.counter, self.storage, self.authenticator)
    }
}

fn present(bytes: &[u8]) -> Option<&[u8]> {
    (!bytes.is_empty()).then_some(bytes)
}

fn validate_identity(record: &FullRecord, expected: &ExpectedIdentity) -> Result<(), JournalError> {
    record
        .validate_identity(
            &expected.device_id,
            &expected.authority_id,
            &expected.lane_id,
        )
        .map_err(|_| JournalError::InvalidRecord)
}

fn matches_identity(record: &FullRecord, expected: &ExpectedIdentity) -> bool {
    validate_identity(record, expected).is_ok()
}
