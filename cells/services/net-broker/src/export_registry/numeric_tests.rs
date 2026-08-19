use super::ascii::{parse_u16_ascii, parse_u8_ascii, parse_usize_ascii};

#[test]
fn numeric_parsers_accept_zero_and_max_boundaries() {
    assert_eq!(parse_u8_ascii(b"0"), Some(0));
    assert_eq!(parse_u8_ascii(b"255"), Some(255));
    assert_eq!(parse_u16_ascii(b"0"), Some(0));
    assert_eq!(parse_u16_ascii(b"65535"), Some(65535));
    assert_eq!(parse_usize_ascii(b"0"), Some(0));
}

#[test]
fn numeric_parsers_reject_overflow_values() {
    assert_eq!(parse_u8_ascii(b"256"), None);
    assert_eq!(parse_u16_ascii(b"65536"), None);
    assert_eq!(
        parse_usize_ascii(b"9999999999999999999999999999999999999999"),
        None
    );
}
