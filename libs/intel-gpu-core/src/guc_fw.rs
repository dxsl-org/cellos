//! GuC firmware CSS header (`drivers/gpu/drm/xe/abi/uc_fw_abi.h`, MIT) and the
//! size/version rules of `parse_css_header` / `guc_read_css_info` in
//! `drivers/gpu/drm/xe/xe_uc_fw.c`.
//!
//! Only the 128-byte public header is read. The uCode and RSA bytes are located,
//! never interpreted (the firmware licence forbids reverse engineering).

/// Size of `struct uc_css_header`.
pub const CSS_HEADER_LEN: usize = 128;

/// Oldest GuC release `xe` accepts (`guc_read_css_info`).
pub const MIN_GUC_RELEASE: Version = Version::new(70, 29, 2);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u8,
    pub minor: u8,
    pub patch: u8,
}

impl Version {
    pub const fn new(major: u8, minor: u8, patch: u8) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Decode the `CSS_SW_VERSION_UC_{MAJOR,MINOR,PATCH}` fields (bits 23:16, 15:8, 7:0).
    pub const fn from_css(word: u32) -> Self {
        Self::new((word >> 16) as u8, (word >> 8) as u8, word as u8)
    }
}

/// `CSS_UKERNEL_INFO_BUILDTYPE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildType {
    Production,
    PreProduction,
    Debug,
    Unknown(u8),
}

/// Located parts of a CSS-format GuC image. Offsets are byte offsets into the blob.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GucImage {
    pub release: Version,
    /// Submission interface version (`submission_version`).
    pub compatibility: Version,
    pub build_type: BuildType,
    pub private_data_size: u32,
    pub ucode_offset: usize,
    pub ucode_len: usize,
    pub rsa_offset: usize,
    pub rsa_len: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FwError {
    /// Blob shorter than the CSS header (`-ENODATA` upstream).
    TooShortForHeader,
    /// `header_size_dw` minus the key/modulus/exponent sizes is not 128 bytes
    /// (`-EPROTO` upstream), including arithmetic underflow.
    BadHeaderSize,
    /// `size_dw` smaller than `header_size_dw`.
    BadTotalSize,
    /// Blob shorter than header + uCode + RSA (`-ENOEXEC` upstream).
    Truncated,
    /// Release older than [`MIN_GUC_RELEASE`].
    ReleaseTooOld(Version),
    /// Release differs from the pinned one (ADR-0033 §5: refuse other versions).
    NotPinned { found: Version, pinned: Version },
}

fn word(blob: &[u8], byte_off: usize) -> u32 {
    let b = &blob[byte_off..byte_off + 4];
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// Parse and size-check a GuC CSS image.
pub fn parse(blob: &[u8]) -> Result<GucImage, FwError> {
    if blob.len() < CSS_HEADER_LEN {
        return Err(FwError::TooShortForHeader);
    }
    let header_dw = word(blob, 4);
    let size_dw = word(blob, 24);
    let (key_dw, modulus_dw, exponent_dw) = (word(blob, 28), word(blob, 32), word(blob, 36));

    let css_dw = header_dw
        .checked_sub(key_dw)
        .and_then(|v| v.checked_sub(modulus_dw))
        .and_then(|v| v.checked_sub(exponent_dw))
        .ok_or(FwError::BadHeaderSize)?;
    if css_dw as usize * 4 != CSS_HEADER_LEN {
        return Err(FwError::BadHeaderSize);
    }
    let ucode_len = size_dw
        .checked_sub(header_dw)
        .ok_or(FwError::BadTotalSize)? as usize
        * 4;
    let rsa_len = key_dw as usize * 4;
    let needed = CSS_HEADER_LEN
        .checked_add(ucode_len)
        .and_then(|v| v.checked_add(rsa_len))
        .ok_or(FwError::Truncated)?;
    if blob.len() < needed {
        return Err(FwError::Truncated);
    }

    let release = Version::from_css(word(blob, 64));
    if release < MIN_GUC_RELEASE {
        return Err(FwError::ReleaseTooOld(release));
    }
    let build_type = match (word(blob, 124) >> 2) & 0x3 {
        0 => BuildType::Production,
        1 => BuildType::PreProduction,
        2 => BuildType::Debug,
        other => BuildType::Unknown(other as u8),
    };
    Ok(GucImage {
        release,
        compatibility: Version::from_css(word(blob, 68)),
        build_type,
        private_data_size: word(blob, 120),
        ucode_offset: CSS_HEADER_LEN,
        ucode_len,
        rsa_offset: CSS_HEADER_LEN + ucode_len,
        rsa_len,
    })
}

/// Accept exactly the pinned release (stricter than upstream's major-match rule,
/// as ADR-0033 §5 requires).
pub fn check_pinned(image: &GucImage, pinned: Version) -> Result<(), FwError> {
    if image.release == pinned {
        Ok(())
    } else {
        Err(FwError::NotPinned {
            found: image.release,
            pinned,
        })
    }
}

#[cfg(test)]
mod tests;
