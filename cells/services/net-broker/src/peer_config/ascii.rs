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

pub fn starts_with(s: &[u8], prefix: &[u8]) -> bool {
    s.len() >= prefix.len() && &s[..prefix.len()] == prefix
}

pub fn eq_slice(a: &[u8], b: &[u8]) -> bool {
    a == b
}

pub fn parse_ipv4(s: &[u8]) -> Option<[u8; 4]> {
    let mut parts = [0u8; 4];
    let mut idx = 0;
    let mut cur: u16 = 0;
    let mut any = false;
    for &b in s {
        if b == b'.' {
            if idx >= 3 {
                return None;
            }
            parts[idx] = cur as u8;
            idx += 1;
            cur = 0;
            any = false;
        } else if b.is_ascii_digit() {
            cur = cur * 10 + (b - b'0') as u16;
            if cur > 255 {
                return None;
            }
            any = true;
        } else {
            return None;
        }
    }
    if !any || idx != 3 {
        return None;
    }
    parts[3] = cur as u8;
    Some(parts)
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
        if n > 65535 {
            return None;
        }
    }
    Some(n as u16)
}

pub fn parse_addr(s: &[u8]) -> Option<([u8; 4], u16)> {
    let colon = s.iter().rposition(|&b| b == b':')?;
    let ip = parse_ipv4(&s[..colon])?;
    let port = parse_u16_ascii(&s[colon + 1..])?;
    Some((ip, port))
}

pub fn parse_hex32(s: &[u8]) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, chunk) in s.chunks_exact(2).enumerate() {
        let hi = hex_nibble(chunk[0])?;
        let lo = hex_nibble(chunk[1])?;
        out[i] = (hi << 4) | lo;
    }
    Some(out)
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
