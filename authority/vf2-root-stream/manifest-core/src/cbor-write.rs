use crate::{Error, Result};

pub(crate) struct Writer<'a> {
    out: &'a mut [u8],
    pos: usize,
}

impl<'a> Writer<'a> {
    pub(crate) fn new(out: &'a mut [u8]) -> Self {
        Self { out, pos: 0 }
    }
    pub(crate) fn len(&self) -> usize {
        self.pos
    }
    fn byte(&mut self, value: u8) -> Result<()> {
        let slot = self.out.get_mut(self.pos).ok_or(Error::OutputTooSmall)?;
        *slot = value;
        self.pos += 1;
        Ok(())
    }
    pub(crate) fn raw(&mut self, value: &[u8]) -> Result<()> {
        let end = self.pos.checked_add(value.len()).ok_or(Error::Overflow)?;
        let dst = self
            .out
            .get_mut(self.pos..end)
            .ok_or(Error::OutputTooSmall)?;
        dst.copy_from_slice(value);
        self.pos = end;
        Ok(())
    }
    fn head(&mut self, major: u8, value: u64) -> Result<()> {
        if value < 24 {
            return self.byte((major << 5) | value as u8);
        }
        if value <= u8::MAX as u64 {
            self.byte((major << 5) | 24)?;
            return self.byte(value as u8);
        }
        if value <= u16::MAX as u64 {
            self.byte((major << 5) | 25)?;
            return self.raw(&(value as u16).to_be_bytes());
        }
        if value <= u32::MAX as u64 {
            self.byte((major << 5) | 26)?;
            return self.raw(&(value as u32).to_be_bytes());
        }
        self.byte((major << 5) | 27)?;
        self.raw(&value.to_be_bytes())
    }
    pub(crate) fn uint(&mut self, value: u64) -> Result<()> {
        self.head(0, value)
    }
    pub(crate) fn map(&mut self, length: u64) -> Result<()> {
        self.head(5, length)
    }
    pub(crate) fn array(&mut self, length: u64) -> Result<()> {
        self.head(4, length)
    }
    pub(crate) fn bstr(&mut self, value: &[u8]) -> Result<()> {
        let length = u64::try_from(value.len()).map_err(|_| Error::Overflow)?;
        self.head(2, length)?;
        self.raw(value)
    }
    pub(crate) fn tstr(&mut self, value: &str) -> Result<()> {
        let length = u64::try_from(value.len()).map_err(|_| Error::Overflow)?;
        self.head(3, length)?;
        self.raw(value.as_bytes())
    }
    #[cfg(feature = "signing")]
    pub(crate) fn tag(&mut self, value: u64) -> Result<()> {
        self.head(6, value)
    }
}
