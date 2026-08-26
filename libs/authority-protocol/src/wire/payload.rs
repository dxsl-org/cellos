use crate::{Bounded, WireError};

pub(super) struct Writer<'a> {
    bytes: &'a mut [u8],
    offset: usize,
}

impl<'a> Writer<'a> {
    pub(super) fn new(bytes: &'a mut [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    pub(super) fn finish(self) -> usize {
        self.offset
    }

    pub(super) fn u8(&mut self, value: u8) -> Result<(), WireError> {
        self.put(&[value])
    }

    pub(super) fn u16(&mut self, value: u16) -> Result<(), WireError> {
        self.put(&value.to_le_bytes())
    }

    pub(super) fn u32(&mut self, value: u32) -> Result<(), WireError> {
        self.put(&value.to_le_bytes())
    }

    pub(super) fn u64(&mut self, value: u64) -> Result<(), WireError> {
        self.put(&value.to_le_bytes())
    }

    pub(super) fn i64(&mut self, value: i64) -> Result<(), WireError> {
        self.put(&value.to_le_bytes())
    }

    pub(super) fn put(&mut self, value: &[u8]) -> Result<(), WireError> {
        let end = self
            .offset
            .checked_add(value.len())
            .ok_or(WireError::OversizePayload)?;
        let target = self
            .bytes
            .get_mut(self.offset..end)
            .ok_or(WireError::BufferTooSmall)?;
        target.copy_from_slice(value);
        self.offset = end;
        Ok(())
    }

    pub(super) fn bounded<const N: usize>(&mut self, value: &Bounded<N>) -> Result<(), WireError> {
        self.u16(value.len() as u16)?;
        self.put(value.as_slice())
    }
}

pub(super) struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    pub(super) fn finish(self) -> Result<(), WireError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(WireError::TrailingBytes)
        }
    }

    pub(super) fn u8(&mut self) -> Result<u8, WireError> {
        Ok(self.take(1)?[0])
    }

    pub(super) fn u16(&mut self) -> Result<u16, WireError> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    pub(super) fn u32(&mut self) -> Result<u32, WireError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    pub(super) fn u64(&mut self) -> Result<u64, WireError> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    pub(super) fn i64(&mut self) -> Result<i64, WireError> {
        Ok(i64::from_le_bytes(self.array()?))
    }

    pub(super) fn array<const N: usize>(&mut self) -> Result<[u8; N], WireError> {
        self.take(N)?
            .try_into()
            .map_err(|_| WireError::InvalidLength)
    }

    pub(super) fn bounded<const N: usize>(&mut self) -> Result<Bounded<N>, WireError> {
        let length = self.u16()? as usize;
        if length > N {
            return Err(WireError::OversizePayload);
        }
        Bounded::from_slice(self.take(length)?).ok_or(WireError::InvalidLength)
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], WireError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(WireError::OversizePayload)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(WireError::Truncated)?;
        self.offset = end;
        Ok(value)
    }
}
