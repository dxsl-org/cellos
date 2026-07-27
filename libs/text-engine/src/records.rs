use alloc::vec::Vec;

pub const MAX_FILE_OPERANDS: usize = 16;
pub const MAX_INPUT_BYTES: usize = 64 * 1024;
pub const MAX_RECORD_BYTES: usize = 4096;
pub const MAX_RECORDS: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputBufferError {
    TooLarge,
    AllocationFailed,
}

pub fn extend_input(bytes: &mut Vec<u8>, chunk: &[u8]) -> Result<(), InputBufferError> {
    let new_len = bytes
        .len()
        .checked_add(chunk.len())
        .ok_or(InputBufferError::TooLarge)?;
    if new_len > MAX_INPUT_BYTES {
        return Err(InputBufferError::TooLarge);
    }
    bytes
        .try_reserve(chunk.len())
        .map_err(|_| InputBufferError::AllocationFailed)?;
    bytes.extend_from_slice(chunk);
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordError {
    InputTooLarge,
    RecordTooLong,
    TooManyRecords,
}

pub struct RecordReader<'a> {
    input: &'a str,
    offset: usize,
    count: usize,
}

impl<'a> RecordReader<'a> {
    pub fn new(input: &'a str) -> Result<Self, RecordError> {
        if input.len() > MAX_INPUT_BYTES {
            return Err(RecordError::InputTooLarge);
        }
        Ok(Self {
            input,
            offset: 0,
            count: 0,
        })
    }

    pub fn collect(mut self) -> Result<Vec<&'a str>, RecordError> {
        let mut records = Vec::new();
        while let Some(record) = self.next_record()? {
            records.push(record);
        }
        Ok(records)
    }

    pub fn next_record(&mut self) -> Result<Option<&'a str>, RecordError> {
        if self.offset >= self.input.len() {
            return Ok(None);
        }
        if self.count >= MAX_RECORDS {
            return Err(RecordError::TooManyRecords);
        }
        let tail = &self.input[self.offset..];
        let next = tail.find('\n').map(|idx| self.offset + idx);
        let end = next.unwrap_or(self.input.len());
        let record = &self.input[self.offset..end];
        if record.len() > MAX_RECORD_BYTES {
            return Err(RecordError::RecordTooLong);
        }
        self.offset = next.map(|idx| idx + 1).unwrap_or(self.input.len());
        self.count += 1;
        Ok(Some(record.trim_end_matches('\r')))
    }
}

#[cfg(test)]
mod tests {
    use super::{extend_input, InputBufferError, MAX_INPUT_BYTES};
    use alloc::vec;

    #[test]
    fn bounded_input_accepts_exact_limit() {
        let mut bytes = vec![0; MAX_INPUT_BYTES - 1];
        extend_input(&mut bytes, &[1]).expect("exact limit is valid");
        assert_eq!(bytes.len(), MAX_INPUT_BYTES);
    }

    #[test]
    fn bounded_input_rejects_before_growth() {
        let mut bytes = vec![0; MAX_INPUT_BYTES];
        let capacity = bytes.capacity();
        assert_eq!(
            extend_input(&mut bytes, &[1]),
            Err(InputBufferError::TooLarge)
        );
        assert_eq!(bytes.len(), MAX_INPUT_BYTES);
        assert_eq!(bytes.capacity(), capacity);
    }
}
