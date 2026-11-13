use super::*;

const DIGEST: [u8; 32] = [0xA5; 32];

fn record(identity: ImageIdentity) -> CellImage {
    CellImage {
        cell_id: 7,
        generation: 3,
        identity,
    }
}

#[test]
fn signed_record_round_trips_and_matches_only_its_name() {
    let image = record(ImageIdentity::signed(DIGEST, ANCHOR_RELEASE, b"sshd").expect("valid"));
    let bytes = image.to_bytes();
    assert_eq!(CellImage::from_bytes(&bytes), Some(image));
    assert!(image.identity.is_signed_program("sshd"));
    assert!(!image.identity.is_signed_program("ssh"));
    assert!(!image.identity.is_signed_program("sshd2"));
    assert_eq!(image.identity.digest(), &DIGEST);
}

#[test]
fn unsigned_image_never_matches_a_program() {
    let image = record(ImageIdentity::unsigned(DIGEST));
    assert_eq!(CellImage::from_bytes(&image.to_bytes()), Some(image));
    assert!(!image.identity.is_signed_program("sshd"));
    assert!(image.identity.name().is_empty());
    // A record claiming a name without a signing anchor is refused.
    let mut forged = image.to_bytes();
    forged[3] = 4;
    forged[56..60].copy_from_slice(b"sshd");
    assert_eq!(CellImage::from_bytes(&forged), None);
}

#[test]
fn constructor_refuses_unknown_anchor_and_bad_names() {
    assert!(ImageIdentity::signed(DIGEST, ANCHOR_UNSIGNED, b"sshd").is_none());
    assert!(ImageIdentity::signed(DIGEST, 9, b"sshd").is_none());
    for bad in [
        &b"SSHD"[..],
        b"ss hd",
        b"ss/hd",
        &[b'a'; CELL_IMAGE_NAME_MAX + 1],
    ] {
        assert!(ImageIdentity::signed(DIGEST, ANCHOR_DEV, bad).is_none());
    }
    let anonymous = ImageIdentity::signed(DIGEST, ANCHOR_DEV, b"").expect("no name");
    assert!(anonymous.is_signed() && !anonymous.is_signed_program(""));
}

#[test]
fn decode_is_strict() {
    let good =
        record(ImageIdentity::signed(DIGEST, ANCHOR_DEV, b"sshd").expect("valid")).to_bytes();
    let mutate = |at: usize, value: u8| {
        let mut bytes = good;
        bytes[at] = value;
        CellImage::from_bytes(&bytes)
    };
    assert!(mutate(0, 2).is_none(), "unknown version");
    assert!(mutate(2, 9).is_none(), "unknown anchor");
    assert!(mutate(5, 1).is_none(), "reserved byte");
    assert!(mutate(70, b'x').is_none(), "name padding");
    assert!(
        mutate(3, CELL_IMAGE_NAME_MAX as u8 + 1).is_none(),
        "name length"
    );
    assert!(CellImage::from_bytes(&good[..CELL_IMAGE_LEN - 1]).is_none());
    let mut zero_cell = good;
    zero_cell[8..16].fill(0);
    assert!(CellImage::from_bytes(&zero_cell).is_none());
}

#[test]
fn image_names_are_lowercase_and_bounded() {
    assert!(is_valid_image_name(b"sshd"));
    assert!(is_valid_image_name(b"service-net_2"));
    assert!(!is_valid_image_name(b""));
    assert!(!is_valid_image_name(b"Sshd"));
}
