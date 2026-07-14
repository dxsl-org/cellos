//! DICE Compound Device Identifier (CDI) derivation.
//!
//! `CDI_n = HKDF(CDI_{n-1}, H(layer_n))` — the standard DICE layering construction
//! (dossier-4 Decision 1), specialized to a 32-byte output with empty `info`
//! (this crate has no use for HKDF's `info` parameter beyond the fixed-size CDI
//! chain, so it is omitted rather than plumbed through for no callers).
//!
//! Pure software: the derivation math takes a caller-supplied root and layer
//! hashes, with no dependency on where the root comes from. P01 supplies the
//! real boot-time measurement aggregate as a layer hash; P02 supplies a
//! Silo-anchored root. Neither changes this function's signature.

use super::hkdf::{expand, extract};

/// Derive one CDI layer: `HKDF-SHA256(salt = prev, ikm = layer_hash, info = "",
/// L = 32)`. `prev` is the parent layer's CDI (or the root secret for the first
/// layer); `layer_hash` is that layer's measurement (e.g. an ELF hash).
pub fn derive_cdi(prev: &[u8; 32], layer_hash: &[u8; 32]) -> [u8; 32] {
    let prk = extract(prev, layer_hash);
    let mut out = [0u8; 32];
    expand(&prk, &[], &mut out);
    out
}

/// Derive the final CDI by folding a chain of layer hashes onto a root secret,
/// in order: `cdi_0 = root; cdi_i = derive_cdi(cdi_{i-1}, layer_hashes[i])`.
pub fn derive_chain(root: &[u8; 32], layer_hashes: &[[u8; 32]]) -> [u8; 32] {
    let mut cdi = *root;
    for h in layer_hashes {
        cdi = derive_cdi(&cdi, h);
    }
    cdi
}

#[cfg(test)]
mod tests {
    use super::{derive_cdi, derive_chain};

    #[test]
    fn derive_cdi_is_deterministic() {
        let prev = [0x11u8; 32];
        let hash = [0x22u8; 32];
        assert_eq!(derive_cdi(&prev, &hash), derive_cdi(&prev, &hash));
    }

    #[test]
    fn derive_cdi_is_sensitive_to_every_input() {
        let prev = [0x11u8; 32];
        let hash_a = [0x22u8; 32];
        let hash_b = [0x23u8; 32];
        assert_ne!(
            derive_cdi(&prev, &hash_a),
            derive_cdi(&prev, &hash_b),
            "changing layer_hash must change the derived CDI"
        );

        let prev_b = [0x12u8; 32];
        assert_ne!(
            derive_cdi(&prev, &hash_a),
            derive_cdi(&prev_b, &hash_a),
            "changing prev must change the derived CDI"
        );
    }

    #[test]
    fn derive_chain_matches_manual_fold() {
        let root = [0xAAu8; 32];
        let layers = [[0x01u8; 32], [0x02u8; 32], [0x03u8; 32]];

        let chained = derive_chain(&root, &layers);

        let step1 = derive_cdi(&root, &layers[0]);
        let step2 = derive_cdi(&step1, &layers[1]);
        let step3 = derive_cdi(&step2, &layers[2]);

        assert_eq!(chained, step3);
    }

    #[test]
    fn derive_chain_empty_layers_returns_root() {
        let root = [0x55u8; 32];
        assert_eq!(derive_chain(&root, &[]), root);
    }
}
