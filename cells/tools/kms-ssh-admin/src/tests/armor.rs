//! Armor decode: accepted forms and every refusal, before any KMS frame exists.

use super::{filler, plain_blob, wrapped_blob};
use crate::armor::{self, decode, encode, ArmorError, BLOB_MAX, TEXT_MAX};
use crate::secret::Secret;

fn block(body: &str) -> std::string::String {
    std::format!("-----BEGIN OPENSSH PRIVATE KEY-----\n{body}\n-----END OPENSSH PRIVATE KEY-----\n")
}

fn refused(text: &str) -> ArmorError {
    match decode(text.as_bytes()) {
        Ok(_) => panic!("accepted: {text:?}"),
        Err(error) => error,
    }
}

#[test]
fn a_block_decodes_to_the_exact_blob_at_every_width_and_line_ending() {
    for blob in [
        plain_blob(7),
        wrapped_blob(),
        filler(1),
        filler(2),
        filler(3),
        filler(250),
        filler(290),
    ] {
        for (width, newline) in [
            (70, "\n"),
            (64, "\n"),
            (76, "\r\n"),
            (70, "\r\n"),
            (4, "\n"),
            (100_000, "\n"),
        ] {
            let text = encode(&blob, width, newline);
            let decoded =
                decode(text.as_bytes()).unwrap_or_else(|e| panic!("{} {width}: {e:?}", blob.len()));
            assert_eq!(
                decoded.as_slice(),
                &blob[..],
                "len {} width {width}",
                blob.len()
            );
        }
    }
}

#[test]
fn empty_lines_outside_the_block_and_a_missing_final_break_are_accepted() {
    let blob = filler(100);
    let plain = encode(&blob, 70, "\n");
    for text in [
        std::format!("\n\n{plain}\n\n"),
        plain.trim_end().to_string(),
        plain.replace('\n', "\r\n"),
    ] {
        assert_eq!(decode(text.as_bytes()).unwrap().as_slice(), &blob[..]);
    }
}

#[test]
fn the_size_boundaries_are_exact() {
    for len in [1usize, 96, 97, 65_534, 65_535, BLOB_MAX] {
        let blob = filler(len);
        let text = encode(&blob, 70, "\n");
        assert!(
            text.len() <= TEXT_MAX,
            "a maximal blob fits the text cap: {}",
            text.len()
        );
        assert_eq!(
            decode(text.as_bytes()).unwrap().as_slice(),
            &blob[..],
            "{len}"
        );
    }
    // One byte more than the ABI blob cap shares the base64 length of the cap: refused by the
    // decoded length, not by the text.
    let text = encode(&filler(BLOB_MAX + 1), 70, "\n");
    assert_eq!(refused(&text), ArmorError::BadLength);
    let text = encode(&filler(BLOB_MAX + 2), 70, "\n");
    assert_eq!(refused(&text), ArmorError::BadLength);
}

#[test]
fn text_over_the_cap_is_refused_before_anything_else() {
    let text = std::vec![b'A'; TEXT_MAX + 1];
    assert_eq!(decode(&text).err(), Some(ArmorError::TextTooLong));
    // Exactly the cap is not refused for its size (it is refused for being no block).
    let text = std::vec![b'A'; TEXT_MAX];
    assert_eq!(decode(&text).err(), Some(ArmorError::MissingBegin));
}

#[test]
fn nothing_or_only_blank_lines_is_empty() {
    for text in ["", "\n", "\r\n\r\n\n"] {
        assert_eq!(refused(text), ArmorError::Empty, "{text:?}");
    }
}

#[test]
fn missing_or_wrong_markers_are_refused() {
    let body = "AAAA";
    let begin = "-----BEGIN OPENSSH PRIVATE KEY-----";
    let end = "-----END OPENSSH PRIVATE KEY-----";
    assert_eq!(
        refused(&std::format!("{body}\n{end}\n")),
        ArmorError::MissingBegin
    );
    assert_eq!(
        refused(&std::format!("{begin}\n{body}\n")),
        ArmorError::MissingEnd
    );
    assert_eq!(
        refused(&std::format!("{begin}\n{body}")),
        ArmorError::MissingEnd
    );
    assert_eq!(
        refused(&std::format!(
            "-----BEGIN RSA PRIVATE KEY-----\n{body}\n-----END RSA PRIVATE KEY-----\n"
        )),
        ArmorError::MissingBegin
    );
    // A mismatched footer never closes the block.
    assert_eq!(
        refused(&std::format!(
            "{begin}\n{body}\n-----END RSA PRIVATE KEY-----\n"
        )),
        ArmorError::BadBodyLine
    );
    for marker in [
        "-----BEGIN OPENSSH PRIVATE KEY----- ",
        " -----BEGIN OPENSSH PRIVATE KEY-----",
        "-----begin openssh private key-----",
        "-----BEGIN OPENSSH PRIVATE KEY----",
        "-----BEGIN OPENSSH PRIVATE KEY-----x",
    ] {
        assert_eq!(
            refused(&std::format!("{marker}\n{body}\n{end}\n")),
            ArmorError::MissingBegin,
            "{marker:?}"
        );
    }
    for marker in [
        "-----END OPENSSH PRIVATE KEY----- ",
        "-----END OPENSSH PRIVATE KEY-----x",
        "-----end openssh private key-----",
    ] {
        assert_eq!(
            refused(&std::format!("{begin}\n{body}\n{marker}\n")),
            ArmorError::BadBodyLine,
            "{marker:?}"
        );
    }
}

#[test]
fn two_blocks_and_text_outside_the_block_are_refused() {
    let one = block("AAAA");
    assert_eq!(
        refused(&std::format!("{one}{one}")),
        ArmorError::SecondBlock
    );
    assert_eq!(
        refused(&std::format!("{one}-----BEGIN OPENSSH PRIVATE KEY-----\n")),
        ArmorError::SecondBlock
    );
    assert_eq!(refused("-----BEGIN OPENSSH PRIVATE KEY-----\n-----BEGIN OPENSSH PRIVATE KEY-----\nAAAA\n-----END OPENSSH PRIVATE KEY-----\n"), ArmorError::SecondBlock);
    assert_eq!(
        refused(&std::format!("{one}trailing\n")),
        ArmorError::TextAfterBlock
    );
    assert_eq!(
        refused(&std::format!("{one} \n")),
        ArmorError::TextAfterBlock
    );
    assert_eq!(
        refused(&std::format!("hello\n{one}")),
        ArmorError::MissingBegin
    );
    assert_eq!(
        refused(&std::format!("$ cat key\n{one}")),
        ArmorError::MissingBegin
    );
}

#[test]
fn body_lines_outside_the_alphabet_are_refused() {
    for line in [
        "AAAA AAAA",
        "AAAA\tAAAA",
        "AA_A",
        "AA-A",
        "AA.A",
        "Proc-Type: 4,ENCRYPTED",
        "AAAA\u{00e9}",
        "AAAA\0",
        " AAAA",
        "AAAA ",
    ] {
        assert_eq!(refused(&block(line)), ArmorError::BadBodyLine, "{line:?}");
    }
    // An empty line inside the body.
    assert_eq!(refused(&block("AAAA\n\nAAAA")), ArmorError::BadBodyLine);
}

#[test]
fn padding_must_be_canonical() {
    for body in [
        "AAA", "AAAAA", "AAAAAA", "A", "AA=", "AAA==", "A===", "====", "AA=A", "AAAA=", "=AAA",
    ] {
        assert_eq!(refused(&block(body)), ArmorError::BadBase64, "{body}");
    }
    // Padding in the middle of the body, across lines.
    assert_eq!(refused(&block("AA==\nAAAA")), ArmorError::BadBase64);
    // Non-zero trailing bits: `QR==` is not the canonical encoding of any byte.
    assert_eq!(refused(&block("QR==")), ArmorError::BadBase64);
    assert_eq!(refused(&block("QUJ=")), ArmorError::BadBase64);
    // The canonical neighbours decode.
    assert_eq!(decode(block("QQ==").as_bytes()).unwrap().as_slice(), b"A");
    assert_eq!(decode(block("QUI=").as_bytes()).unwrap().as_slice(), b"AB");
    assert_eq!(decode(block("QUJD").as_bytes()).unwrap().as_slice(), b"ABC");
}

#[test]
fn only_lf_and_crlf_break_lines() {
    assert_eq!(
        refused("-----BEGIN OPENSSH PRIVATE KEY-----\rAAAA\r-----END OPENSSH PRIVATE KEY-----\r"),
        ArmorError::BareCarriageReturn
    );
    assert_eq!(refused(&block("AA\rAA")), ArmorError::BareCarriageReturn);
    assert_eq!(
        refused(
            "-----BEGIN OPENSSH PRIVATE KEY-----\r\r\nAAAA\r\n-----END OPENSSH PRIVATE KEY-----"
        ),
        ArmorError::BareCarriageReturn
    );
}

#[test]
fn an_empty_body_is_refused() {
    assert_eq!(
        refused("-----BEGIN OPENSSH PRIVATE KEY-----\n-----END OPENSSH PRIVATE KEY-----\n"),
        ArmorError::EmptyBody
    );
}

#[test]
fn more_base64_than_a_maximal_blob_needs_is_refused() {
    let line = "A".repeat(90_000);
    assert_eq!(refused(&block(&line)), ArmorError::BadLength);
}

#[test]
fn a_secret_buffer_is_wiped_by_wipe_and_never_grows() {
    let mut secret = Secret::with_capacity(8);
    assert!(secret.push_slice(b"abcdefgh"));
    assert!(
        !secret.push(b'x'),
        "a full buffer refuses, it never reallocates"
    );
    let (ptr, cap) = secret.allocation();
    assert_eq!(cap, 8);
    secret.wipe();
    assert!(secret.is_empty());
    // The wipe empties the buffer in place: same allocation, so no copy of the key bytes is
    // left behind by a move. (The test code itself contains no unchecked memory access; the
    // zeroing is done by `ostd::session::zeroize`, volatile stores over the whole capacity.)
    assert_eq!(secret.allocation(), (ptr, cap));
    let mut secret = Secret::with_capacity(8);
    assert!(secret.push_slice(b"abcdefgh"));
    let (ptr, cap) = secret.allocation();
    secret.pop();
    secret.truncate(4);
    assert_eq!(secret.as_slice(), b"abcd");
    assert_eq!(
        secret.allocation(),
        (ptr, cap),
        "shrinking never reallocates"
    );
    // A wiped buffer is reusable up to its fixed capacity and no further.
    secret.wipe();
    assert!(secret.push_slice(b"12345678"));
    assert!(!secret.push(b'9'));
}

#[test]
fn the_markers_are_the_openssh_ones() {
    assert_eq!(armor::BEGIN, b"-----BEGIN OPENSSH PRIVATE KEY-----");
    assert_eq!(armor::END, b"-----END OPENSSH PRIVATE KEY-----");
}
