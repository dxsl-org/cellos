//! The dev root: a pinned public constant, never a secret.

use super::*;
use crate::ssh::persist::devroot::{mac_key, DEV_ROOT_LABEL};

#[test]
fn the_label_is_pinned_byte_for_byte() {
    assert_eq!(
        DEV_ROOT_LABEL,
        b"CELLOS-KSSH-T1-DEV-ROOT-v1 PUBLIC CONSTANT NOT A SECRET"
    );
    assert_eq!(DEV_ROOT_LABEL.len(), 55);
}

#[test]
fn mac_key_golden() {
    // blake2s-256 of the label, computed independently (python hashlib).
    assert_eq!(
        mac_key(),
        hex32("e6ee4e76ccf6c77fd5922dbeeecf818b9d7a34f58e679700c3c757749c9c16ce")
    );
    // Deterministic: a constant, not generated.
    assert_eq!(mac_key(), mac_key());
}
