use p256::ecdsa::VerifyingKey;

use crate::Error;

const ALG_ECC: u16 = 0x0023;
const ALG_SHA256: u16 = 0x000b;
const ALG_NULL: u16 = 0x0010;
const ALG_ECDSA: u16 = 0x0018;
const CURVE_NIST_P256: u16 = 0x0003;
const REQUIRED_ATTRIBUTES: u32 = 0x0004_0032;
const FORBIDDEN_ATTRIBUTES: u32 = 0x0003_0000;
const SPKI_PREFIX: [u8; 27] = [
    0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08, 0x2a,
    0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00, 0x04,
];

pub(crate) fn parse(input: &[u8]) -> Result<[u8; 91], Error> {
    let mut outer = Cursor::new(input);
    let size = outer.u16()? as usize;
    let public = outer.bytes(size)?;
    outer.finish()?;
    let mut fields = Cursor::new(public);
    if fields.u16()? != ALG_ECC || fields.u16()? != ALG_SHA256 {
        return Err(Error::InvalidTpmPublic);
    }
    let attributes = fields.u32()?;
    if attributes & REQUIRED_ATTRIBUTES != REQUIRED_ATTRIBUTES
        || attributes & FORBIDDEN_ATTRIBUTES != 0
    {
        return Err(Error::InvalidTpmPublic);
    }
    let policy_len = fields.u16()? as usize;
    if !matches!(policy_len, 0 | 32) {
        return Err(Error::InvalidTpmPublic);
    }
    fields.bytes(policy_len)?;
    if fields.u16()? != ALG_NULL
        || fields.u16()? != ALG_ECDSA
        || fields.u16()? != ALG_SHA256
        || fields.u16()? != CURVE_NIST_P256
        || fields.u16()? != ALG_NULL
        || fields.u16()? != 32
    {
        return Err(Error::InvalidTpmPublic);
    }
    let x = fields.bytes(32)?;
    if fields.u16()? != 32 {
        return Err(Error::InvalidTpmPublic);
    }
    let y = fields.bytes(32)?;
    fields.finish()?;
    let mut spki = [0u8; 91];
    spki[..SPKI_PREFIX.len()].copy_from_slice(&SPKI_PREFIX);
    spki[27..59].copy_from_slice(x);
    spki[59..].copy_from_slice(y);
    VerifyingKey::from_sec1_bytes(&spki[26..]).map_err(|_| Error::InvalidTpmPublic)?;
    Ok(spki)
}

struct Cursor<'a> {
    input: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self { input, offset: 0 }
    }

    fn bytes(&mut self, count: usize) -> Result<&'a [u8], Error> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or(Error::InvalidTpmPublic)?;
        let value = self
            .input
            .get(self.offset..end)
            .ok_or(Error::InvalidTpmPublic)?;
        self.offset = end;
        Ok(value)
    }

    fn u16(&mut self) -> Result<u16, Error> {
        let bytes: [u8; 2] = self
            .bytes(2)?
            .try_into()
            .map_err(|_| Error::InvalidTpmPublic)?;
        Ok(u16::from_be_bytes(bytes))
    }

    fn u32(&mut self) -> Result<u32, Error> {
        let bytes: [u8; 4] = self
            .bytes(4)?
            .try_into()
            .map_err(|_| Error::InvalidTpmPublic)?;
        Ok(u32::from_be_bytes(bytes))
    }

    fn finish(self) -> Result<(), Error> {
        if self.offset == self.input.len() {
            Ok(())
        } else {
            Err(Error::InvalidTpmPublic)
        }
    }
}
