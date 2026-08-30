use crate::peer_config::ascii::{eq_slice, parse_ipv4, parse_u16_ascii, starts_with, trim_ascii};
use types::kms::{validate_hostname, RELAY_HOSTNAME_MAX};

/// Validated endpoint for a future authenticated relay transport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RelayEndpoint {
    ip: [u8; 4],
    port: u16,
    hostname: [u8; RELAY_HOSTNAME_MAX],
    hostname_len: u8,
}

impl RelayEndpoint {
    /// Return the validated numeric IPv4 address.
    pub const fn ip(&self) -> [u8; 4] {
        self.ip
    }

    /// Return the validated nonzero TCP port.
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// Return the validated lowercase DNS hostname without zero padding.
    pub fn hostname(&self) -> &[u8] {
        &self.hostname[..self.hostname_len as usize]
    }
}

/// Deterministic failures for the security-relevant relay endpoint fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelayConfigError {
    /// A recognized field occurs more than once.
    DuplicateKey,
    /// At least one relay field exists but `relay_ip` is absent.
    MissingIp,
    /// At least one relay field exists but `relay_port` is absent.
    MissingPort,
    /// At least one relay field exists but `relay_hostname` is absent.
    MissingHostname,
    /// `relay_ip` is not a four-octet decimal IPv4 address.
    InvalidIp,
    /// `relay_port` is not a decimal integer in `1..=65535`.
    InvalidPort,
    /// `relay_hostname` violates the frozen lowercase DNS profile.
    InvalidHostname,
    /// A key in the reserved `relay_*` namespace is unsupported or malformed.
    UnknownRelayKey,
}

/// Parse the optional global relay endpoint from flat `cluster.cfg` bytes.
///
/// Blank lines, comments, and non-relay keys are ignored. If any relay key is
/// present, all three unique keys must be valid. This function only validates a
/// future TLS target; it performs no I/O, resolution, authentication, or dialing.
///
/// Returns `Ok(None)` when no relay key exists, `Ok(Some(endpoint))` for a
/// complete endpoint, or [`RelayConfigError`] for a malformed declaration.
pub fn parse_relay_endpoint_bytes(
    config: &[u8],
) -> Result<Option<RelayEndpoint>, RelayConfigError> {
    let mut ip = None;
    let mut port = None;
    let mut hostname = None;
    let mut saw_relay_key = false;

    for line in config.split(|&byte| byte == b'\n') {
        let line = trim_ascii(line);
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        let Some(separator) = line.iter().position(|&byte| byte == b'=') else {
            if starts_with(line, b"relay_") {
                return Err(RelayConfigError::UnknownRelayKey);
            }
            continue;
        };
        let key = trim_ascii(&line[..separator]);
        if !starts_with(key, b"relay_") {
            continue;
        }
        saw_relay_key = true;
        let value = trim_ascii(&line[separator + 1..]);
        if eq_slice(key, b"relay_ip") {
            if ip.is_some() {
                return Err(RelayConfigError::DuplicateKey);
            }
            ip = Some(parse_ipv4(value).ok_or(RelayConfigError::InvalidIp)?);
        } else if eq_slice(key, b"relay_port") {
            if port.is_some() {
                return Err(RelayConfigError::DuplicateKey);
            }
            let parsed = parse_u16_ascii(value).ok_or(RelayConfigError::InvalidPort)?;
            if parsed == 0 {
                return Err(RelayConfigError::InvalidPort);
            }
            port = Some(parsed);
        } else if eq_slice(key, b"relay_hostname") {
            if hostname.is_some() {
                return Err(RelayConfigError::DuplicateKey);
            }
            if !validate_hostname(value) {
                return Err(RelayConfigError::InvalidHostname);
            }
            let mut bytes = [0u8; RELAY_HOSTNAME_MAX];
            bytes[..value.len()].copy_from_slice(value);
            hostname = Some((bytes, value.len() as u8));
        } else {
            return Err(RelayConfigError::UnknownRelayKey);
        }
    }

    if !saw_relay_key {
        return Ok(None);
    }
    let ip = ip.ok_or(RelayConfigError::MissingIp)?;
    let port = port.ok_or(RelayConfigError::MissingPort)?;
    let (hostname, hostname_len) = hostname.ok_or(RelayConfigError::MissingHostname)?;
    Ok(Some(RelayEndpoint {
        ip,
        port,
        hostname,
        hostname_len,
    }))
}

#[cfg(test)]
mod tests;
