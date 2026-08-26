//! Service-net relay profile validation: mount paths and manifest schema.
//!
//! Symlink/metadata inspection is impossible without VFS stat support
//! (deviation 8), so validation is strictly lexical plus schema-allowlist:
//! absolute canonical paths only, and private-key material is rejected by
//! field name before any file is ever opened.

/// Longest accepted mount path.
pub const PATH_MAX: usize = 4096;
/// Longest accepted single path component (POSIX NAME_MAX).
const COMPONENT_MAX: usize = 255;

/// Validate one mounted read-only path lexically.
///
/// Absolute, no empty/`.`/`..` components, bounded length, no trailing or
/// doubled slashes, NUL-free. Duplicate rejection happens at the schema
/// layer where each key may appear once.
pub fn validate_mount_path(path: &str) -> bool {
    if path.len() > PATH_MAX || !path.starts_with('/') || path.contains('\0') {
        return false;
    }
    for part in path[1..].split('/') {
        match part {
            "" => return false, // doubled slash or trailing slash
            "." | ".." => return false,
            _ => {}
        }
        if part.len() > COMPONENT_MAX {
            return false;
        }
    }
    true
}

/// Validate the relay hostname against the frozen DNS profile.
pub fn validate_relay_hostname(hostname: &str) -> bool {
    types::kms::validate_hostname(hostname.as_bytes())
}

/// Manifest sections and the only keys each accepts. Private-key fields are
/// permitted exclusively in the host-only `[server]` section; their appearance
/// anywhere else is a hard rejection.
const SCHEMA: &[(&str, &[&str])] = &[
    (
        "relay",
        &["bind_host", "hostname", "port", "min_tls_version"],
    ),
    (
        "trust",
        &["relay_ca_der", "active_ca_sha256", "next_ca_sha256"],
    ),
    (
        "client",
        &["certificate_chain_der", "key_handle", "node_id_sha256"],
    ),
    (
        "server",
        &[
            "certificate_pem",
            "private_key_pem",
            "client_issuing_ca_pem",
        ],
    ),
    (
        "authorization",
        &["net_service_identity", "policy_handle", "relay_denylist"],
    ),
];

fn section_keys(section: &str) -> Option<&'static [&'static str]> {
    SCHEMA
        .iter()
        .find(|(name, _)| *name == section)
        .map(|(_, keys)| *keys)
}

/// Validate one `(section.key, value)` entry against the allowlist.
///
/// Returns `false` for unknown sections or keys, duplicates, private-key
/// leakage outside `[server]`, malformed hex fingerprints, and non-DNS
/// relay hostnames.
pub fn validate_entry(
    section: &str,
    key: &str,
    value: &str,
    seen: &mut heapless::String<128>,
) -> bool {
    let Some(keys) = section_keys(section) else {
        return false;
    };
    if !keys.contains(&key) {
        return false;
    }
    // Duplicates: "section.key" must not already be present.
    let mut probe = heapless::String::<128>::new();
    if push_entry(&mut probe, section, key).is_err() {
        return false;
    }
    let entry = &probe[..probe.len() - 1]; // drop trailing ';'
    if seen.split(';').any(|existing| existing == entry) {
        return false;
    }
    if key.contains("private_key") && section != "server" {
        return false;
    }
    // Fingerprints are 64 lowercase hex characters.
    let hex_ok = |value: &str| {
        value.len() == 64
            && value
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9') || matches!(b, b'a'..=b'f'))
    };
    match key {
        "active_ca_sha256" | "next_ca_sha256" | "node_id_sha256" => {
            if !hex_ok(value) {
                return false;
            }
        }
        "hostname" => {
            if !validate_relay_hostname(value) {
                return false;
            }
        }
        "relay_ca_der"
        | "certificate_chain_der"
        | "certificate_pem"
        | "private_key_pem"
        | "client_issuing_ca_pem"
        | "relay_denylist" => {
            if !validate_mount_path(value) {
                return false;
            }
        }
        "port" => {
            if !matches!(value.parse::<u16>(), Ok(1..=u16::MAX)) {
                return false;
            }
        }
        "min_tls_version" => {
            if value != "1.3" {
                return false;
            }
        }
        _ => {}
    }
    push_entry(seen, section, key).is_ok()
}

fn push_entry(target: &mut heapless::String<128>, section: &str, key: &str) -> Result<(), ()> {
    use core::fmt::Write;
    write!(target, "{section}.{key};").map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mount_paths_are_lexically_strict() {
        assert!(validate_mount_path("/mnt/relay/ca.der"));
        assert!(!validate_mount_path("mnt/relay/ca.der"));
        assert!(!validate_mount_path("/mnt/../secret"));
        assert!(!validate_mount_path("/mnt/./ca.der"));
        assert!(!validate_mount_path("/mnt//ca.der"));
        assert!(!validate_mount_path("/mnt/ca.der/"));
        assert!(!validate_mount_path("/"));
        assert!(!validate_mount_path(&"/mnt/".repeat(PATH_MAX)));
    }

    #[test]
    fn private_key_fields_are_confined_to_server_section() {
        let mut seen = heapless::String::<128>::new();
        assert!(validate_entry(
            "server",
            "private_key_pem",
            "/host/key.pem",
            &mut seen
        ));
        assert!(!validate_entry(
            "client",
            "private_key_pem",
            "/mnt/key.pem",
            &mut seen
        ));
    }

    #[test]
    fn duplicates_unknown_keys_and_bad_values_are_rejected() {
        let mut seen = heapless::String::<128>::new();
        assert!(validate_entry(
            "trust",
            "active_ca_sha256",
            &"a".repeat(64),
            &mut seen
        ));
        assert!(!validate_entry(
            "trust",
            "active_ca_sha256",
            &"a".repeat(64),
            &mut seen
        ));
        assert!(!validate_entry("trust", "unknown", "x", &mut seen));
        assert!(!validate_entry("nosuch", "x", "y", &mut seen));
        assert!(!validate_entry(
            "trust",
            "active_ca_sha256",
            &"A".repeat(64),
            &mut seen
        ));
        assert!(!validate_entry(
            "trust",
            "active_ca_sha256",
            "short",
            &mut seen
        ));
        assert!(!validate_entry(
            "relay",
            "hostname",
            "Relay.example",
            &mut seen
        ));
        assert!(validate_entry(
            "relay",
            "hostname",
            "relay.example.internal",
            &mut seen
        ));
        assert!(!validate_entry("relay", "port", "99999", &mut seen));
        assert!(validate_entry("relay", "port", "443", &mut seen));
        assert!(!validate_entry("relay", "port", "0", &mut seen));
        assert!(!validate_entry(
            "relay",
            "min_tls_version",
            "1.2",
            &mut seen
        ));
        assert!(validate_entry("relay", "min_tls_version", "1.3", &mut seen));
    }
}
