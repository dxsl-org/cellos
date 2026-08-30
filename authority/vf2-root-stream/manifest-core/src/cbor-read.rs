use crate::{Error, Result};

pub(crate) struct Reader<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(input: &'a [u8]) -> Self {
        Self { input, pos: 0 }
    }
    pub(crate) fn done(&self) -> Result<()> {
        if self.pos == self.input.len() {
            Ok(())
        } else {
            Err(Error::TrailingData)
        }
    }
    fn byte(&mut self) -> Result<u8> {
        let value = *self.input.get(self.pos).ok_or(Error::Truncated)?;
        self.pos += 1;
        Ok(value)
    }
    fn take(&mut self, length: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(length).ok_or(Error::Overflow)?;
        let value = self.input.get(self.pos..end).ok_or(Error::Truncated)?;
        self.pos = end;
        Ok(value)
    }
    fn head(&mut self, major: u8) -> Result<u64> {
        let first = self.byte()?;
        if first >> 5 != major {
            return Err(Error::WrongType);
        }
        match first & 31 {
            n @ 0..=23 => Ok(n as u64),
            24 => {
                let n = self.byte()? as u64;
                if n < 24 {
                    Err(Error::NonCanonical)
                } else {
                    Ok(n)
                }
            }
            25 => {
                let b = self.take(2)?;
                let n = u16::from_be_bytes([b[0], b[1]]) as u64;
                if n <= u8::MAX as u64 {
                    Err(Error::NonCanonical)
                } else {
                    Ok(n)
                }
            }
            26 => {
                let b = self.take(4)?;
                let n = u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as u64;
                if n <= u16::MAX as u64 {
                    Err(Error::NonCanonical)
                } else {
                    Ok(n)
                }
            }
            27 => {
                let b = self.take(8)?;
                let n = u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
                if n <= u32::MAX as u64 {
                    Err(Error::NonCanonical)
                } else {
                    Ok(n)
                }
            }
            _ => Err(Error::NonCanonical),
        }
    }
    pub(crate) fn uint(&mut self) -> Result<u64> {
        self.head(0)
    }
    pub(crate) fn expect_uint(&mut self, value: u64) -> Result<()> {
        if self.uint()? == value {
            Ok(())
        } else {
            Err(Error::UnknownKey)
        }
    }
    pub(crate) fn map(&mut self) -> Result<u64> {
        self.head(5)
    }
    pub(crate) fn array(&mut self) -> Result<u64> {
        self.head(4)
    }
    pub(crate) fn tag(&mut self) -> Result<u64> {
        self.head(6)
    }
    pub(crate) fn bstr(&mut self) -> Result<&'a [u8]> {
        let len = usize::try_from(self.head(2)?).map_err(|_| Error::Overflow)?;
        self.take(len)
    }
    pub(crate) fn tstr(&mut self) -> Result<&'a str> {
        let len = usize::try_from(self.head(3)?).map_err(|_| Error::Overflow)?;
        core::str::from_utf8(self.take(len)?).map_err(|_| Error::WrongType)
    }
}
