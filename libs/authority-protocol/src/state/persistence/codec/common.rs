use crate::WireError;

pub(super) struct Writer<'a> {
    output: &'a mut [u8],
    offset: usize,
}
impl<'a> Writer<'a> {
    pub(super) fn new(output: &'a mut [u8]) -> Self {
        Self { output, offset: 0 }
    }
    pub(super) fn put(&mut self, bytes: &[u8]) -> Result<(), WireError> {
        let end = self
            .offset
            .checked_add(bytes.len())
            .ok_or(WireError::BufferTooSmall)?;
        let target = self
            .output
            .get_mut(self.offset..end)
            .ok_or(WireError::BufferTooSmall)?;
        target.copy_from_slice(bytes);
        self.offset = end;
        Ok(())
    }
    pub(super) fn u8(&mut self, value: u8) -> Result<(), WireError> {
        self.put(&[value])
    }
    pub(super) fn u64(&mut self, value: u64) -> Result<(), WireError> {
        self.put(&value.to_le_bytes())
    }
    pub(super) const fn finish(self) -> usize {
        self.offset
    }
}

pub(super) struct Reader<'a> {
    input: &'a [u8],
    offset: usize,
}
impl<'a> Reader<'a> {
    pub(super) const fn new(input: &'a [u8]) -> Self {
        Self { input, offset: 0 }
    }
    pub(super) fn take(&mut self, length: usize) -> Result<&'a [u8], WireError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(WireError::Truncated)?;
        let value = self
            .input
            .get(self.offset..end)
            .ok_or(WireError::Truncated)?;
        self.offset = end;
        Ok(value)
    }
    pub(super) fn array<const N: usize>(&mut self) -> Result<[u8; N], WireError> {
        self.take(N)?.try_into().map_err(|_| WireError::Truncated)
    }
    pub(super) fn u8(&mut self) -> Result<u8, WireError> {
        Ok(self.take(1)?[0])
    }
    pub(super) fn u64(&mut self) -> Result<u64, WireError> {
        Ok(u64::from_le_bytes(self.array()?))
    }
    pub(super) fn finish(self) -> Result<(), WireError> {
        if self.offset == self.input.len() {
            Ok(())
        } else {
            Err(WireError::TrailingBytes)
        }
    }
}
