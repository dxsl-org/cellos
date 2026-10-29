//! `cellbox ssh` argv (command-contracts §4.1). Every unsupported flag or form is
//! refused here, before any network, trust or credential effect.
use alloc::string::String;
use core::fmt;
use core::net::{Ipv4Addr, Ipv6Addr};

pub const DEFAULT_PORT: u16 = 22;
pub const DEFAULT_CONNECT_TIMEOUT: u32 = 30;
pub const MAX_COMMAND: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Auth {
    Password,
    PublicKey { key_id: String },
}

/// The configured host identifier: the exact trust key spelling (§4.1, D03).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Host {
    Name(String),
    V4(Ipv4Addr),
    V6(Ipv6Addr),
}

impl fmt::Display for Host {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Host::Name(n) => f.write_str(n),
            Host::V4(a) => write!(f, "{a}"),
            Host::V6(a) => write!(f, "{a}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub user: String,
    pub auth: Auth,
    pub host: Host,
    pub port: u16,
    pub connect_timeout: u32,
    pub timeout: Option<u32>,
    /// `None`: PTY shell session; `Some`: one non-PTY exec command string.
    pub command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgError {
    Missing(&'static str),
    Repeated(&'static str),
    Unsupported(String),
    Invalid(&'static str),
    Extra(String),
}

impl fmt::Display for ArgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArgError::Missing(what) => write!(f, "missing {what}"),
            ArgError::Repeated(what) => write!(f, "{what} given more than once"),
            ArgError::Unsupported(arg) => write!(f, "unsupported option: {arg}"),
            ArgError::Invalid(what) => write!(f, "invalid {what}"),
            ArgError::Extra(arg) => write!(f, "unexpected argument: {arg}"),
        }
    }
}

fn number(value: Option<&str>, min: u32, max: u32, what: &'static str) -> Result<u32, ArgError> {
    let v = value.ok_or(ArgError::Missing(what))?;
    if v.is_empty() || v.len() > 10 || !v.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ArgError::Invalid(what));
    }
    let n: u32 = v.parse().map_err(|_| ArgError::Invalid(what))?;
    if (min..=max).contains(&n) { Ok(n) } else { Err(ArgError::Invalid(what)) }
}

fn set<T>(slot: &mut Option<T>, value: T, what: &'static str) -> Result<(), ArgError> {
    if slot.is_some() {
        return Err(ArgError::Repeated(what));
    }
    *slot = Some(value);
    Ok(())
}

fn valid_user(u: &str) -> bool {
    (1..=31).contains(&u.len()) && u.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

fn valid_key_id(k: &str) -> bool {
    (1..=64).contains(&k.len()) && k.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}

/// Parse HOST as an IP literal (canonical form) or an ASCII DNS name (lowercased,
/// one trailing dot removed). No `user@host`, URI, port suffix, wildcard or
/// non-ASCII spelling.
pub fn parse_host(raw: &str) -> Result<Host, ArgError> {
    if let Ok(a) = raw.parse::<Ipv4Addr>() {
        return Ok(Host::V4(a));
    }
    if raw.contains(':') {
        return raw.parse::<Ipv6Addr>().map(Host::V6).map_err(|_| ArgError::Invalid("host"));
    }
    let name = raw.strip_suffix('.').unwrap_or(raw);
    let ok = (1..=253).contains(&name.len())
        && name.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                && !label.starts_with('-')
                && !label.ends_with('-')
        })
        // An all-numeric dotted name would read as a malformed IPv4 literal.
        && !name.bytes().all(|b| b.is_ascii_digit() || b == b'.');
    if !ok {
        return Err(ArgError::Invalid("host"));
    }
    Ok(Host::Name(name.to_ascii_lowercase()))
}

/// Parse the arguments after `ssh`.
pub fn parse<'a>(argv: impl IntoIterator<Item = &'a str>) -> Result<Args, ArgError> {
    let mut it = argv.into_iter();
    let (mut user, mut auth, mut identity, mut port, mut ct, mut timeout) =
        (None, None, None, None, None, None);
    let mut host = None;
    while let Some(arg) = it.next() {
        match arg {
            "--user" => {
                let u = it.next().ok_or(ArgError::Missing("--user value"))?;
                if !valid_user(u) {
                    return Err(ArgError::Invalid("user"));
                }
                set(&mut user, String::from(u), "--user")?;
            }
            "--auth" => {
                let a = match it.next() {
                    Some("password") => false,
                    Some("publickey") => true,
                    Some(_) => return Err(ArgError::Invalid("--auth (password|publickey)")),
                    None => return Err(ArgError::Missing("--auth value")),
                };
                set(&mut auth, a, "--auth")?;
            }
            "--identity" => {
                let k = it.next().ok_or(ArgError::Missing("--identity value"))?;
                if !valid_key_id(k) {
                    return Err(ArgError::Invalid("key id"));
                }
                set(&mut identity, String::from(k), "--identity")?;
            }
            "-p" | "--port" => {
                let p = number(it.next(), 1, 65535, "port")? as u16;
                set(&mut port, p, "--port")?;
            }
            "--connect-timeout" => set(&mut ct, number(it.next(), 1, 300, "connect timeout")?, "--connect-timeout")?,
            "--timeout" => set(&mut timeout, number(it.next(), 1, 86400, "timeout")?, "--timeout")?,
            "--" => {
                host = Some(it.next().ok_or(ArgError::Missing("HOST"))?);
                break;
            }
            a if a.starts_with('-') => return Err(ArgError::Unsupported(String::from(a))),
            a => {
                host = Some(a);
                break;
            }
        }
    }
    let host = parse_host(host.ok_or(ArgError::Missing("HOST"))?)?;
    let command = match it.next() {
        None => None,
        Some("--command") => {
            let c = it.next().ok_or(ArgError::Missing("--command value"))?;
            if c.is_empty() || c.len() > MAX_COMMAND {
                return Err(ArgError::Invalid("command"));
            }
            Some(String::from(c))
        }
        Some(other) => return Err(ArgError::Extra(String::from(other))),
    };
    if let Some(extra) = it.next() {
        return Err(ArgError::Extra(String::from(extra)));
    }
    let user = user.ok_or(ArgError::Missing("--user"))?;
    let auth = match (auth.ok_or(ArgError::Missing("--auth"))?, identity) {
        (false, None) => Auth::Password,
        (false, Some(_)) => return Err(ArgError::Invalid("--identity with --auth password")),
        (true, Some(key_id)) => Auth::PublicKey { key_id },
        (true, None) => return Err(ArgError::Missing("--identity")),
    };
    Ok(Args {
        user,
        auth,
        host,
        port: port.unwrap_or(DEFAULT_PORT),
        connect_timeout: ct.unwrap_or(DEFAULT_CONNECT_TIMEOUT),
        timeout,
        command,
    })
}

/// Split helper for tests and callers holding owned strings.
pub fn parse_owned(argv: &[String]) -> Result<Args, ArgError> {
    parse(argv.iter().map(String::as_str))
}
