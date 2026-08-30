use crate::Error;

#[derive(Clone, Copy)]
pub(crate) struct Element<'a> {
    pub tag: u8,
    pub full: &'a [u8],
    pub value: &'a [u8],
}

pub(crate) struct Reader<'a> {
    input: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    pub fn new(input: &'a [u8]) -> Self {
        Self { input, offset: 0 }
    }

    pub fn next(&mut self) -> Result<Element<'a>, Error> {
        let start = self.offset;
        let tag = *self.input.get(self.offset).ok_or(Error::MalformedDer)?;
        if tag & 0x1f == 0x1f {
            return Err(Error::MalformedDer);
        }
        self.offset += 1;
        let first = *self.input.get(self.offset).ok_or(Error::MalformedDer)?;
        self.offset += 1;
        let len = if first & 0x80 == 0 {
            first as usize
        } else {
            let count = (first & 0x7f) as usize;
            if count == 0 || count > core::mem::size_of::<usize>() {
                return Err(Error::MalformedDer);
            }
            let bytes = self
                .input
                .get(self.offset..self.offset + count)
                .ok_or(Error::MalformedDer)?;
            if bytes[0] == 0 {
                return Err(Error::MalformedDer);
            }
            self.offset += count;
            let mut value = 0usize;
            for byte in bytes {
                value = value.checked_mul(256).ok_or(Error::MalformedDer)?;
                value = value
                    .checked_add(*byte as usize)
                    .ok_or(Error::MalformedDer)?;
            }
            if value < 128 {
                return Err(Error::MalformedDer);
            }
            value
        };
        let end = self.offset.checked_add(len).ok_or(Error::MalformedDer)?;
        let value = self
            .input
            .get(self.offset..end)
            .ok_or(Error::MalformedDer)?;
        self.offset = end;
        Ok(Element {
            tag,
            full: &self.input[start..end],
            value,
        })
    }

    pub fn required(&mut self, tag: u8) -> Result<Element<'a>, Error> {
        let element = self.next()?;
        if element.tag != tag {
            return Err(Error::MalformedDer);
        }
        Ok(element)
    }

    pub fn finish(self) -> Result<(), Error> {
        if self.offset == self.input.len() {
            Ok(())
        } else {
            Err(Error::TrailingData)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.offset == self.input.len()
    }
}

pub(crate) fn one(input: &[u8], tag: u8) -> Result<Element<'_>, Error> {
    let mut reader = Reader::new(input);
    let element = reader.required(tag)?;
    reader.finish()?;
    Ok(element)
}

pub(crate) fn positive_integer(value: &[u8]) -> Result<&[u8], Error> {
    if value.is_empty() || value[0] & 0x80 != 0 {
        return Err(Error::InvalidSerial);
    }
    if value.len() > 1 && value[0] == 0 && value[1] & 0x80 == 0 {
        return Err(Error::InvalidSerial);
    }
    let unsigned = if value.len() > 1 && value[0] == 0 {
        &value[1..]
    } else {
        value
    };
    if unsigned.iter().all(|byte| *byte == 0) {
        return Err(Error::InvalidSerial);
    }
    Ok(unsigned)
}

pub(crate) fn valid_oid(value: &[u8]) -> bool {
    if value.is_empty() || value.last().is_some_and(|byte| byte & 0x80 != 0) {
        return false;
    }
    let mut start = true;
    for byte in value {
        if start && *byte == 0x80 {
            return false;
        }
        start = byte & 0x80 == 0;
    }
    true
}
