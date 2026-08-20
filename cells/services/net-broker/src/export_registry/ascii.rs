use super::RegistryError;

pub fn trim_ascii(s: &[u8]) -> &[u8] {
    let s = match s.iter().position(|b| !b.is_ascii_whitespace()) {
        Some(i) => &s[i..],
        None => return &[],
    };
    match s.iter().rposition(|b| !b.is_ascii_whitespace()) {
        Some(i) => &s[..=i],
        None => s,
    }
}

pub fn parse_u8_ascii(s: &[u8]) -> Option<u8> {
    if s.is_empty() {
        return None;
    }
    let mut n: u16 = 0;
    for &b in s {
        if !b.is_ascii_digit() {
            return None;
        }
        n = n * 10 + (b - b'0') as u16;
        if n > u8::MAX as u16 {
            return None;
        }
    }
    Some(n as u8)
}

pub fn parse_u16_ascii(s: &[u8]) -> Option<u16> {
    if s.is_empty() {
        return None;
    }
    let mut n: u32 = 0;
    for &b in s {
        if !b.is_ascii_digit() {
            return None;
        }
        n = n * 10 + (b - b'0') as u32;
        if n > u16::MAX as u32 {
            return None;
        }
    }
    Some(n as u16)
}

pub fn parse_usize_ascii(s: &[u8]) -> Option<usize> {
    if s.is_empty() {
        return None;
    }
    let mut n: usize = 0;
    for &b in s {
        if !b.is_ascii_digit() {
            return None;
        }
        n = n.checked_mul(10)?.checked_add((b - b'0') as usize)?;
    }
    Some(n)
}

pub fn set_once<T: Copy>(slot: &mut Option<T>, value: Option<T>) -> Result<(), RegistryError> {
    let value = value.ok_or(RegistryError::InvalidValue)?;
    if slot.replace(value).is_some() {
        return Err(RegistryError::DuplicateField);
    }
    Ok(())
}
