use authority_protocol::WireError;

pub struct Writer<'a> {
    output: &'a mut [u8],
    position: usize,
}

impl<'a> Writer<'a> {
    pub fn new(output: &'a mut [u8]) -> Self {
        Self {
            output,
            position: 0,
        }
    }

    pub const fn len(&self) -> usize {
        self.position
    }

    pub fn put(&mut self, value: &[u8]) -> Result<(), WireError> {
        let end = self
            .position
            .checked_add(value.len())
            .ok_or(WireError::BufferTooSmall)?;
        let target = self
            .output
            .get_mut(self.position..end)
            .ok_or(WireError::BufferTooSmall)?;
        target.copy_from_slice(value);
        self.position = end;
        Ok(())
    }

    pub fn u8(&mut self, value: u8) -> Result<(), WireError> {
        self.put(&[value])
    }

    pub fn u16(&mut self, value: u16) -> Result<(), WireError> {
        self.put(&value.to_be_bytes())
    }
    pub fn u32(&mut self, value: u32) -> Result<(), WireError> {
        self.put(&value.to_be_bytes())
    }

    pub fn u64(&mut self, value: u64) -> Result<(), WireError> {
        self.put(&value.to_be_bytes())
    }
}

pub struct Reader<'a> {
    input: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    pub const fn new(input: &'a [u8]) -> Self {
        Self { input, position: 0 }
    }

    pub const fn remaining(&self) -> usize {
        self.input.len() - self.position
    }

    pub fn take(&mut self, length: usize) -> Result<&'a [u8], WireError> {
        let end = self
            .position
            .checked_add(length)
            .ok_or(WireError::Truncated)?;
        let value = self
            .input
            .get(self.position..end)
            .ok_or(WireError::Truncated)?;
        self.position = end;
        Ok(value)
    }

    pub fn u8(&mut self) -> Result<u8, WireError> {
        Ok(self.take(1)?[0])
    }

    pub fn u16(&mut self) -> Result<u16, WireError> {
        Ok(u16::from_be_bytes(self.array()?))
    }
    pub fn u32(&mut self) -> Result<u32, WireError> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    pub fn u64(&mut self) -> Result<u64, WireError> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    pub fn array<const N: usize>(&mut self) -> Result<[u8; N], WireError> {
        self.take(N)?.try_into().map_err(|_| WireError::Truncated)
    }
}
