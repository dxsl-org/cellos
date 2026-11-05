use super::*;

/// Build a CSS blob: header, `ucode_dw` uCode dwords, `key_dw` RSA dwords,
/// optional modulus/exponent dwords (present in the size fields only).
fn blob(ucode_dw: u32, key_dw: u32, modulus_dw: u32, exponent_dw: u32, sw: u32) -> Vec<u8> {
    let header_dw = 32 + key_dw + modulus_dw + exponent_dw;
    let mut b = vec![0u8; 128 + (ucode_dw + key_dw) as usize * 4];
    let put =
        |b: &mut Vec<u8>, off: usize, v: u32| b[off..off + 4].copy_from_slice(&v.to_le_bytes());
    put(&mut b, 4, header_dw);
    put(&mut b, 24, header_dw + ucode_dw);
    put(&mut b, 28, key_dw);
    put(&mut b, 32, modulus_dw);
    put(&mut b, 36, exponent_dw);
    put(&mut b, 64, sw);
    put(&mut b, 68, 0x0001_0602); // submission 1.6.2
    put(&mut b, 120, 0x1000);
    put(&mut b, 124, 1 << 2); // pre-production
    b
}

const V70_72_1: u32 = (70 << 16) | (72 << 8) | 1;

#[test]
fn locates_ucode_and_rsa_and_reads_versions() {
    let image = parse(&blob(100, 96, 64, 1, V70_72_1)).unwrap();
    assert_eq!(image.release, Version::new(70, 72, 1));
    assert_eq!(image.compatibility, Version::new(1, 6, 2));
    assert_eq!(image.build_type, BuildType::PreProduction);
    assert_eq!(image.private_data_size, 0x1000);
    assert_eq!((image.ucode_offset, image.ucode_len), (128, 400));
    assert_eq!((image.rsa_offset, image.rsa_len), (528, 384));
}

#[test]
fn truncated_image_without_modulus_is_accepted() {
    // Modulus/exponent sizes appear in the header but their bytes are absent.
    let b = blob(10, 4, 64, 1, V70_72_1);
    assert_eq!(b.len(), 128 + 14 * 4);
    assert!(parse(&b).is_ok());
}

#[test]
fn rejects_short_blob_before_reading_fields() {
    assert_eq!(parse(&[0u8; 127]), Err(FwError::TooShortForHeader));
}

#[test]
fn rejects_header_size_that_does_not_leave_128_bytes() {
    let mut b = blob(10, 4, 0, 0, V70_72_1);
    b[4..8].copy_from_slice(&(33u32 + 4).to_le_bytes());
    assert_eq!(parse(&b), Err(FwError::BadHeaderSize));
}

#[test]
fn key_sizes_larger_than_header_are_an_error_not_a_wrap() {
    let mut b = blob(10, 4, 0, 0, V70_72_1);
    b[28..32].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(parse(&b), Err(FwError::BadHeaderSize));
}

#[test]
fn total_size_below_header_size_is_rejected() {
    let mut b = blob(10, 4, 0, 0, V70_72_1);
    b[24..28].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(parse(&b), Err(FwError::BadTotalSize));
}

#[test]
fn missing_rsa_bytes_is_truncated() {
    let mut b = blob(10, 4, 0, 0, V70_72_1);
    b.truncate(b.len() - 1);
    assert_eq!(parse(&b), Err(FwError::Truncated));
}

#[test]
fn release_older_than_70_29_2_is_rejected() {
    let old = (70 << 16) | (29 << 8) | 1;
    assert_eq!(
        parse(&blob(10, 4, 0, 0, old)),
        Err(FwError::ReleaseTooOld(Version::new(70, 29, 1)))
    );
    assert!(parse(&blob(10, 4, 0, 0, (70 << 16) | (29 << 8) | 2)).is_ok());
}

#[test]
fn only_the_pinned_release_passes_the_pin_check() {
    let image = parse(&blob(10, 4, 0, 0, V70_72_1)).unwrap();
    assert_eq!(check_pinned(&image, Version::new(70, 72, 1)), Ok(()));
    assert_eq!(
        check_pinned(&image, Version::new(70, 72, 2)),
        Err(FwError::NotPinned {
            found: Version::new(70, 72, 1),
            pinned: Version::new(70, 72, 2)
        })
    );
}
