use crate::{der, der::Reader, Error};

const CLIENT_AUTH: &[u8] = &[0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x02];

pub(super) fn parse_basic(input: &[u8]) -> Result<(bool, Option<u8>), Error> {
    let sequence = der::one(input, 0x30)?;
    let mut fields = Reader::new(sequence.value);
    if fields.is_empty() {
        return Ok((false, None));
    }
    let ca = fields.required(0x01)?;
    if ca.value != [0xff] {
        return Err(Error::InvalidBasicConstraints);
    }
    let path_len = if fields.is_empty() {
        None
    } else {
        let integer = fields.required(0x02)?;
        let value = integer.value;
        if value.len() != 1 || value[0] & 0x80 != 0 {
            return Err(Error::InvalidBasicConstraints);
        }
        Some(value[0])
    };
    fields.finish()?;
    Ok((true, path_len))
}

pub(super) fn parse_key_usage(input: &[u8]) -> Result<u16, Error> {
    let bits = der::one(input, 0x03)?.value;
    if bits.len() < 2 || bits.len() > 3 || bits[0] > 7 || *bits.last().unwrap() == 0 {
        return Err(Error::InvalidKeyUsage);
    }
    if bits.last().unwrap().trailing_zeros() != bits[0] as u32 {
        return Err(Error::InvalidKeyUsage);
    }
    let mut usage = (bits[1] as u16) << 8;
    if bits.len() == 3 {
        usage |= bits[2] as u16;
    }
    Ok(usage)
}

pub(super) fn parse_eku(input: &[u8]) -> Result<bool, Error> {
    let sequence = der::one(input, 0x30)?;
    let mut fields = Reader::new(sequence.value);
    let only = fields.required(0x06)?.value == CLIENT_AUTH;
    fields.finish()?;
    Ok(only)
}

pub(super) fn parse_san(input: &[u8]) -> Result<&[u8], Error> {
    let sequence = der::one(input, 0x30)?;
    let mut names = Reader::new(sequence.value);
    let dns = names.required(0x82)?.value;
    if !valid_dns(dns) {
        return Err(Error::InvalidSan);
    }
    names.finish()?;
    Ok(dns)
}

pub(super) fn parse_aki(input: &[u8]) -> Result<&[u8], Error> {
    let sequence = der::one(input, 0x30)?;
    let mut fields = Reader::new(sequence.value);
    let key = fields.required(0x80)?.value;
    if key.is_empty() {
        return Err(Error::InvalidAuthorityKeyIdentifier);
    }
    fields.finish()?;
    Ok(key)
}

pub(super) fn parse_name_constraints<'a>(
    input: &'a [u8],
    permitted: &mut [Option<&'a [u8]>; 4],
    excluded: &mut [Option<&'a [u8]>; 4],
) -> Result<(), Error> {
    let sequence = der::one(input, 0x30)?;
    let mut fields = Reader::new(sequence.value);
    let mut permitted_seen = false;
    let mut excluded_seen = false;
    while !fields.is_empty() {
        let field = fields.next()?;
        let (target, seen) = match field.tag {
            0xa0 => (&mut *permitted, &mut permitted_seen),
            0xa1 => (&mut *excluded, &mut excluded_seen),
            _ => return Err(Error::InvalidNameConstraints),
        };
        if *seen || field.value.is_empty() {
            return Err(Error::InvalidNameConstraints);
        }
        *seen = true;
        let mut subtrees = Reader::new(field.value);
        while !subtrees.is_empty() {
            let subtree = subtrees.required(0x30)?;
            let mut parts = Reader::new(subtree.value);
            let dns = parts.required(0x82)?.value;
            let constraint = dns.strip_prefix(b".").unwrap_or(dns);
            if !valid_dns(constraint) || !parts.is_empty() {
                return Err(Error::InvalidNameConstraints);
            }
            let slot = target
                .iter_mut()
                .find(|slot| slot.is_none())
                .ok_or(Error::InvalidNameConstraints)?;
            *slot = Some(dns);
        }
    }
    if permitted.iter().all(Option::is_none) && excluded.iter().all(Option::is_none) {
        return Err(Error::InvalidNameConstraints);
    }
    Ok(())
}

fn valid_dns(name: &[u8]) -> bool {
    if name.is_empty()
        || name.len() > 253
        || !name.is_ascii()
        || name.first() == Some(&b'.')
        || name.last() == Some(&b'.')
    {
        return false;
    }
    name.split(|byte| *byte == b'.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && label.first().is_some_and(u8::is_ascii_alphanumeric)
            && label.last().is_some_and(u8::is_ascii_alphanumeric)
            && label
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
    })
}
