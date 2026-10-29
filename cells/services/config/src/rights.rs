//! Q right (Phase 01 CP1, 2026-10-25): who may write which Config key.
//!
//! `Get` and `List` are open to every caller: Config is not a secret store
//! (D-Q3). `Set` and `Delete` (D-Q4) are admitted only when all three hold:
//! the kernel attested the caller (`CallerIdentity` present), the caller's Cell
//! holds `CALLER_FLAG_CONFIG_WRITE`, and the key is in the shell namespace
//! (D-Q2): 1..=64 bytes of `[A-Za-z_][A-Za-z0-9_]*`, not a system key. The
//! system keys (`PATH`, `OS`) are seeded at start and immutable at runtime for
//! every caller.
//!
//! A refusal is `ConfigResponse::Err(CONFIG_ERR_DENIED)`; the runtime answers
//! it before touching the store. The client never pre-filters: this module is
//! the only decision point.

use api::caller_identity::{CallerIdentity, CALLER_FLAG_CONFIG_WRITE};
use api::ipc::{ConfigRequest, CONFIG_ERR_DENIED};

/// Keys seeded by the service and never writable at runtime.
pub(crate) const SYSTEM_KEYS: [&str; 2] = ["PATH", "OS"];

/// Longest key in the shell namespace, in bytes.
pub(crate) const MAX_SHELL_KEY: usize = 64;

/// Whether `key` is in the shell namespace: a shell identifier of 1..=64
/// bytes that is not a system key.
pub(crate) fn shell_key_admitted(key: &str) -> bool {
    let bytes = key.as_bytes();
    let Some((&first, rest)) = bytes.split_first() else {
        return false;
    };
    bytes.len() <= MAX_SHELL_KEY
        && (first == b'_' || first.is_ascii_alphabetic())
        && rest.iter().all(|&b| b == b'_' || b.is_ascii_alphanumeric())
        && !SYSTEM_KEYS.contains(&key)
}

/// Admission for one decoded request.
///
/// No `_` arm: a new `ConfigRequest` variant does not compile until it is
/// classified here.
pub(crate) fn admit(request: &ConfigRequest<'_>, caller: Option<&CallerIdentity>) -> Result<(), u8> {
    let key = match request {
        ConfigRequest::Get(_) | ConfigRequest::List => return Ok(()),
        ConfigRequest::Set { key, .. } => key,
        ConfigRequest::Delete(key) => key,
    };
    let granted = caller.is_some_and(|c| c.flags & CALLER_FLAG_CONFIG_WRITE != 0);
    if granted && shell_key_admitted(key) {
        Ok(())
    } else {
        Err(CONFIG_ERR_DENIED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use api::caller_identity::{CALLER_FLAG_NET_RIGHTS_MASK, CALLER_FLAG_VFS_MUTATE};

    const fn caller(flags: u32) -> CallerIdentity {
        CallerIdentity {
            flags,
            cell_id: 9,
            generation: 1,
            sender_tid: 9,
        }
    }

    const SHELL: CallerIdentity = caller(CALLER_FLAG_CONFIG_WRITE | CALLER_FLAG_VFS_MUTATE);
    const NO_Q: CallerIdentity = caller(CALLER_FLAG_VFS_MUTATE | CALLER_FLAG_NET_RIGHTS_MASK);

    fn writes(key: &str) -> [ConfigRequest<'_>; 2] {
        [ConfigRequest::Set { key, value: "v" }, ConfigRequest::Delete(key)]
    }

    #[test]
    fn reads_are_open_to_every_caller() {
        for request in [ConfigRequest::Get("PATH"), ConfigRequest::Get(""), ConfigRequest::List] {
            assert_eq!(admit(&request, None), Ok(()));
            assert_eq!(admit(&request, Some(&NO_Q)), Ok(()));
            assert_eq!(admit(&request, Some(&SHELL)), Ok(()));
        }
    }

    #[test]
    fn granted_caller_writes_shell_identifiers() {
        let longest = "K".repeat(MAX_SHELL_KEY);
        for key in ["A", "_", "P01Q_OK", "path", "Path", "OS_", "_PATH", "a1_b2", longest.as_str()] {
            for request in writes(key) {
                assert_eq!(admit(&request, Some(&SHELL)), Ok(()), "{request:?}");
            }
        }
    }

    #[test]
    fn missing_identity_or_right_denies_every_write() {
        for caller in [None, Some(&NO_Q), Some(&caller(0))] {
            for request in writes("P01Q_OK") {
                assert_eq!(admit(&request, caller), Err(CONFIG_ERR_DENIED), "{request:?}");
            }
        }
    }

    #[test]
    fn keys_outside_the_namespace_are_denied_even_with_the_right() {
        let too_long = "K".repeat(MAX_SHELL_KEY + 1);
        for key in [
            "PATH", "OS", "", "1A", "a.b", "a-b", "a b", "a=b", "/bin", "ä", "A\u{0}", too_long.as_str(),
        ] {
            for request in writes(key) {
                assert_eq!(admit(&request, Some(&SHELL)), Err(CONFIG_ERR_DENIED), "{request:?}");
            }
        }
    }
}
