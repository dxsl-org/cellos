//! In-memory `SshBlobIo`: two files, a `dirs` flag, fault points and a poison
//! mode. Test only; it can be reopened over its files to model a reboot.

use std::vec::Vec;

use crate::ssh::persist::blob_io::{BlobError, Read, Slot, SshBlobIo, RECORD_MAX};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum Call {
    EnsureDirs,
    Read(Slot),
    Write(Slot),
}

/// One-shot fault on the next `write`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum WriteFault {
    /// Error, nothing stored.
    Fail,
    /// Store the first `n` bytes, then error (a torn write).
    Tear(usize),
}

/// One-shot fault on the first read of a slot after a successful write to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum ReadbackFault {
    Error,
    /// Return the bytes with one bit flipped.
    Flip,
}

#[derive(Debug, Clone, Default)]
pub(in super::super) struct MemBlobIo {
    pub files: [Option<Vec<u8>>; 2],
    pub dirs: bool,
    pub fail_ensure_dirs: bool,
    /// Persistent read error per slot (A, B).
    pub fail_read: [bool; 2],
    pub write_fault: Option<WriteFault>,
    pub readback_fault: Option<ReadbackFault>,
    /// A fault that fired poisons every later call (models a poisoned `/srv`
    /// backend).
    pub poison_on_failure: bool,
    pub poisoned: bool,
    /// Every call, in order.
    pub calls: Vec<Call>,
    /// Every payload ever handed to `write`, including failed and torn ones.
    pub received: Vec<Vec<u8>>,
    just_written: Option<Slot>,
}

fn index(slot: Slot) -> usize {
    usize::from(slot.id())
}

impl MemBlobIo {
    pub(in super::super) fn new() -> Self {
        Self::default()
    }

    /// The same files after a reboot: faults, poison and the call log reset.
    pub(in super::super) fn reopen(&self) -> Self {
        Self {
            files: self.files.clone(),
            dirs: self.dirs,
            ..Self::default()
        }
    }

    pub(in super::super) fn file(&self, slot: Slot) -> Option<&Vec<u8>> {
        self.files.get(index(slot)).and_then(Option::as_ref)
    }

    pub(in super::super) fn put(&mut self, slot: Slot, bytes: &[u8]) {
        self.dirs = true;
        self.files[index(slot)] = Some(bytes.to_vec());
    }

    pub(in super::super) fn writes(&self) -> usize {
        self.calls
            .iter()
            .filter(|call| matches!(call, Call::Write(_)))
            .count()
    }

    fn fault_fired(&mut self) {
        if self.poison_on_failure {
            self.poisoned = true;
        }
    }
}

impl SshBlobIo for MemBlobIo {
    fn ensure_dirs(&mut self) -> Result<(), BlobError> {
        self.calls.push(Call::EnsureDirs);
        if self.poisoned || self.fail_ensure_dirs {
            return Err(BlobError);
        }
        self.dirs = true;
        Ok(())
    }

    fn read(&mut self, slot: Slot, out: &mut [u8; RECORD_MAX]) -> Result<Read, BlobError> {
        self.calls.push(Call::Read(slot));
        let after_write = self.just_written.take() == Some(slot);
        if self.poisoned || self.fail_read[index(slot)] {
            return Err(BlobError);
        }
        let mut flip = false;
        if after_write {
            match self.readback_fault.take() {
                Some(ReadbackFault::Error) => {
                    self.fault_fired();
                    return Err(BlobError);
                }
                Some(ReadbackFault::Flip) => flip = true,
                None => {}
            }
        }
        let Some(bytes) = self.files[index(slot)].as_ref() else {
            return Ok(Read::Absent);
        };
        if bytes.len() > RECORD_MAX {
            return Ok(Read::Oversize);
        }
        out[..bytes.len()].copy_from_slice(bytes);
        if flip && !bytes.is_empty() {
            out[0] ^= 1;
        }
        Ok(Read::Present(bytes.len()))
    }

    fn write(&mut self, slot: Slot, bytes: &[u8]) -> Result<(), BlobError> {
        self.calls.push(Call::Write(slot));
        self.received.push(bytes.to_vec());
        if self.poisoned || !self.dirs {
            return Err(BlobError);
        }
        match self.write_fault.take() {
            Some(WriteFault::Fail) => {
                self.fault_fired();
                return Err(BlobError);
            }
            Some(WriteFault::Tear(n)) => {
                self.files[index(slot)] = Some(bytes[..n.min(bytes.len())].to_vec());
                self.fault_fired();
                return Err(BlobError);
            }
            None => {}
        }
        self.files[index(slot)] = Some(bytes.to_vec());
        self.just_written = Some(slot);
        Ok(())
    }
}
