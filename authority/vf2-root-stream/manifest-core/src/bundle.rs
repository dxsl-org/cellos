use crate::{
    decode_payload, sha256, validate_manifest, verify_cose, Error, ExpectedManifest, Manifest,
    ManifestLimits, Result, MAX_COSE_LEN, XMODEM_BLOCK_LEN,
};

/// Borrowed result of complete envelope, manifest, padding, and component verification.
#[derive(Debug, Eq, PartialEq)]
pub struct VerifiedBundle<'a> {
    pub manifest: Manifest,
    pub cose: &'a [u8],
    pub component_region: &'a [u8],
    pub logical_len: usize,
}

/// Returns `4 + cose.len() + component_region.len()` with checked arithmetic.
pub fn outer_encoded_len(cose_len: usize, component_region_len: usize) -> Result<usize> {
    4usize
        .checked_add(cose_len)
        .and_then(|n| n.checked_add(component_region_len))
        .ok_or(Error::Overflow)
}

/// Writes `u32be cose_len || cose || component_region` and returns its exact length.
pub fn encode_outer(cose: &[u8], component_region: &[u8], out: &mut [u8]) -> Result<usize> {
    if cose.is_empty() || cose.len() > MAX_COSE_LEN {
        return Err(Error::LimitExceeded);
    }
    let cose_len = u32::try_from(cose.len()).map_err(|_| Error::Overflow)?;
    let length = outer_encoded_len(cose.len(), component_region.len())?;
    let dst = out.get_mut(..length).ok_or(Error::OutputTooSmall)?;
    dst[..4].copy_from_slice(&cose_len.to_be_bytes());
    dst[4..4 + cose.len()].copy_from_slice(cose);
    dst[4 + cose.len()..].copy_from_slice(component_region);
    Ok(length)
}

/// Verifies one padded logical stream. The input must be whole 1,024-byte payload
/// blocks returned by `decode_xmodem`; every byte after the signed region must be 0x1a.
pub fn verify_bundle<'a>(
    padded: &'a [u8],
    public_key: &[u8; 32],
    expected: &ExpectedManifest,
    limits: &ManifestLimits,
    signature_scratch: &mut [u8],
) -> Result<VerifiedBundle<'a>> {
    if padded.is_empty() || padded.len() % XMODEM_BLOCK_LEN != 0 {
        return Err(Error::InvalidPadding);
    }
    let prefix = padded.get(..4).ok_or(Error::Truncated)?;
    let cose_len = u32::from_be_bytes([prefix[0], prefix[1], prefix[2], prefix[3]]);
    if cose_len == 0 || cose_len > limits.max_cose_length {
        return Err(Error::LimitExceeded);
    }
    let cose_len = usize::try_from(cose_len).map_err(|_| Error::Overflow)?;
    if cose_len > MAX_COSE_LEN {
        return Err(Error::LimitExceeded);
    }
    let cose_end = 4usize.checked_add(cose_len).ok_or(Error::Overflow)?;
    let cose = padded.get(4..cose_end).ok_or(Error::Truncated)?;
    let payload = verify_cose(cose, public_key, signature_scratch)?;
    let manifest = decode_payload(payload)?;
    validate_manifest(&manifest, expected, limits)?;
    let region_len =
        usize::try_from(manifest.component_region_length).map_err(|_| Error::Overflow)?;
    let logical_len = cose_end.checked_add(region_len).ok_or(Error::Overflow)?;
    let block_count = logical_len
        .checked_add(XMODEM_BLOCK_LEN - 1)
        .ok_or(Error::Overflow)?
        / XMODEM_BLOCK_LEN;
    let expected_padded = block_count
        .checked_mul(XMODEM_BLOCK_LEN)
        .ok_or(Error::Overflow)?;
    if padded.len() != expected_padded {
        return Err(Error::TrailingData);
    }
    let component_region = padded.get(cose_end..logical_len).ok_or(Error::Truncated)?;
    if padded[logical_len..].iter().any(|byte| *byte != 0x1a) {
        return Err(Error::InvalidPadding);
    }
    verify_components(&manifest, component_region)?;
    Ok(VerifiedBundle {
        manifest,
        cose,
        component_region,
        logical_len,
    })
}

/// Verifies all four component digests over their exact declared slices.
pub fn verify_components(manifest: &Manifest, component_region: &[u8]) -> Result<()> {
    if u64::try_from(component_region.len()).map_err(|_| Error::Overflow)?
        != manifest.component_region_length
    {
        return Err(Error::WrongRegionLength);
    }
    for component in &manifest.components {
        let start = usize::try_from(component.offset).map_err(|_| Error::Overflow)?;
        let length = usize::try_from(component.length).map_err(|_| Error::Overflow)?;
        let end = start.checked_add(length).ok_or(Error::Overflow)?;
        let bytes = component_region
            .get(start..end)
            .ok_or(Error::WrongRegionLength)?;
        if sha256(bytes) != component.sha256 {
            return Err(Error::DigestMismatch);
        }
    }
    Ok(())
}
