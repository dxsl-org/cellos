// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Pure codec and limit tests. Resource authority is exercised at its real owner.

extern crate std;
use std::string::String;
use std::vec::Vec;
use crate::codec::{
    encode_key_action, encode_key_action_with_paste, validate_paste, validate_text,
    Decoder, KeyAction, KeyState, KeySymbol, Modifiers,
};
use crate::controls::{ScreenControls, ViewportSize};
use crate::error::TerminalError;

fn key(symbol: KeySymbol, modifiers: Modifiers) -> KeyAction {
    KeyAction::Key { symbol, modifiers, state: KeyState::Press }
}

fn decode(bytes: &[u8]) -> Vec<KeyAction> {
    let mut decoder = Decoder::new();
    let mut out = Vec::new();
    decoder.feed(bytes, 0, &mut out).unwrap();
    decoder.flush(&mut out).unwrap();
    out
}

fn encode(action: &KeyAction) -> Vec<u8> {
    let mut bytes = Vec::new();
    encode_key_action(action, &mut bytes).unwrap();
    bytes
}

#[test]
fn utf8_and_chunk_boundaries_preserve_actions() {
    let bytes = "aé🙂z\r".as_bytes();
    let expected = decode(bytes);
    assert_eq!(expected.last(), Some(&key(KeySymbol::Enter, Modifiers::NONE)));
    assert_eq!(expected.iter().flat_map(encode).collect::<Vec<_>>(), bytes);
    for split in 0..=bytes.len() {
        let mut decoder = Decoder::new();
        let mut out = Vec::new();
        decoder.feed(&bytes[..split], 0, &mut out).unwrap();
        decoder.feed(&bytes[split..], 1, &mut out).unwrap();
        decoder.flush(&mut out).unwrap();
        assert_eq!(out, expected);
    }
    for malformed in [&b"\xc0\xaf"[..], &b"\xed\xa0\x80"[..], &b"\xf4\x90\x80\x80"[..], &b"\x80"[..]] {
        let mut decoder = Decoder::new();
        let mut out = Vec::new();
        assert_eq!(decoder.feed(malformed, 0, &mut out), Err(TerminalError::UnsupportedInput));
        assert!(out.is_empty());
    }
    let mut decoder = Decoder::new();
    let mut out = Vec::new();
    decoder.feed(b"\xf0\x9f\x99", 0, &mut out).unwrap();
    assert!(out.is_empty());
    assert_eq!(decoder.flush(&mut out), Err(TerminalError::UnsupportedInput));
}

#[test]
fn c0_aliases_do_not_normalize_backspace_or_newline() {
    for byte in 0u8..=31 {
        if byte == 0x1b { continue; }
        let action = match byte {
            0 => key(KeySymbol::Character(' '), Modifiers::CONTROL),
            9 => key(KeySymbol::Tab, Modifiers::NONE),
            13 => key(KeySymbol::Enter, Modifiers::NONE),
            1..=26 => key(KeySymbol::Character((b'a' + byte - 1) as char), Modifiers::CONTROL),
            28 => key(KeySymbol::Character('\\'), Modifiers::CONTROL),
            29 => key(KeySymbol::Character(']'), Modifiers::CONTROL),
            30 => key(KeySymbol::Character('^'), Modifiers::CONTROL),
            31 => key(KeySymbol::Character('_'), Modifiers::CONTROL),
            _ => unreachable!(),
        };
        assert_eq!(decode(&[byte]), [action.clone()]);
        assert_eq!(encode(&action), [byte]);
        let mut alt = match action { KeyAction::Key { symbol, modifiers, .. } => key(symbol, Modifiers { alt: true, ..modifiers }), _ => unreachable!() };
        assert_eq!(decode(&[0x1b, byte]), [alt.clone()]);
        assert_eq!(encode(&alt), [0x1b, byte]);
        // Named aliases do not invent a Control modifier.
        if let KeyAction::Key { state, .. } = &mut alt { *state = KeyState::Release; }
        assert!(encode(&alt).is_empty());
    }
    assert_eq!(decode(b"\x7f"), [key(KeySymbol::Backspace, Modifiers::NONE)]);
    assert_eq!(encode(&key(KeySymbol::Character('?'), Modifiers::CONTROL)), b"\x7f");
}

#[test]
fn every_navigation_and_function_modifier_has_the_frozen_bytes() {
    let named = [
        (KeySymbol::Up, 0u8, b'A'), (KeySymbol::Down, 0, b'B'),
        (KeySymbol::Right, 0, b'C'), (KeySymbol::Left, 0, b'D'),
        (KeySymbol::Home, 0, b'H'), (KeySymbol::End, 0, b'F'),
        (KeySymbol::Insert, 2, b'~'), (KeySymbol::Delete, 3, b'~'),
        (KeySymbol::PageUp, 5, b'~'), (KeySymbol::PageDown, 6, b'~'),
        (KeySymbol::F(1), 0, b'P'), (KeySymbol::F(2), 0, b'Q'),
        (KeySymbol::F(3), 0, b'R'), (KeySymbol::F(4), 0, b'S'),
        (KeySymbol::F(5), 15, b'~'), (KeySymbol::F(6), 17, b'~'),
        (KeySymbol::F(7), 18, b'~'), (KeySymbol::F(8), 19, b'~'),
        (KeySymbol::F(9), 20, b'~'), (KeySymbol::F(10), 21, b'~'),
        (KeySymbol::F(11), 23, b'~'), (KeySymbol::F(12), 24, b'~'),
    ];
    for (symbol, number, final_byte) in named {
        for bits in 0..8 {
            let mods = Modifiers::from_bits(bits).unwrap();
            let action = key(symbol, mods);
            let expected = if bits == 0 {
                if number > 0 { std::format!("\x1b[{}~", number) }
                else if matches!(symbol, KeySymbol::F(_)) { std::format!("\x1bO{}", final_byte as char) }
                else { std::format!("\x1b[{}", final_byte as char) }
            } else {
                std::format!("\x1b[{};{}{}", if number == 0 { 1 } else { number }, mods.csi_parameter(), final_byte as char)
            };
            assert_eq!(encode(&action), expected.as_bytes());
            assert_eq!(decode(expected.as_bytes()), [action.clone()]);
            let (code, flags, state) = action.key_fields().unwrap();
            assert_eq!(KeyAction::from_key(code, flags, state), Ok(action));
        }
    }
}

#[test]
fn named_shortcuts_and_character_modifiers_are_exact() {
    for shift in [false, true] {
        for alt in [false, true] {
            let mods = Modifiers { shift, alt, control: false };
            for (symbol, suffix) in [(KeySymbol::Enter, b'\r'), (KeySymbol::Backspace, 0x7f), (KeySymbol::Escape, 0x1b)] {
                let mut expected = Vec::new();
                if alt { expected.push(0x1b); }
                expected.push(suffix);
                assert_eq!(encode(&key(symbol, mods)), expected);
            }
            let action = key(KeySymbol::Tab, mods);
            let mut expected = Vec::new();
            if alt { expected.push(0x1b); }
            expected.extend_from_slice(if shift { b"\x1b[Z" } else { b"\t" });
            assert_eq!(encode(&action), expected);
            assert_eq!(decode(&expected), [action]);
        }
    }
    for (scalar, byte) in [('a', 1), ('Z', 26), (' ', 0), ('@', 0), ('[', 27), ('\\', 28), (']', 29), ('^', 30), ('_', 31), ('?', 127)] {
        for shift in [false, true] {
            for alt in [false, true] {
                let mut expected = Vec::new();
                if alt { expected.push(27); }
                expected.push(byte);
                assert_eq!(encode(&key(KeySymbol::Character(scalar), Modifiers { control: true, shift, alt })), expected);
            }
        }
    }
    assert_eq!(encode(&key(KeySymbol::Character('🙂'), Modifiers::ALT)), "\x1b🙂".as_bytes());
}

#[test]
fn unsupported_actions_never_append_even_an_escape_prefix() {
    let invalid = [
        key(KeySymbol::F(0), Modifiers::NONE), key(KeySymbol::F(13), Modifiers::ALT),
        key(KeySymbol::Character('!'), Modifiers { alt: true, control: true, shift: false }),
        key(KeySymbol::Character('x'), Modifiers::NONE),
        key(KeySymbol::Character('\n'), Modifiers::ALT),
        key(KeySymbol::Enter, Modifiers::CONTROL), key(KeySymbol::Backspace, Modifiers::CONTROL),
        key(KeySymbol::Escape, Modifiers::CONTROL), key(KeySymbol::Tab, Modifiers::CONTROL),
        KeyAction::Text(String::from("good\ninvalid")), KeyAction::Paste(String::from("good\x1binvalid")),
    ];
    for action in invalid {
        let mut out = Vec::from(&b"sentinel"[..]);
        assert_eq!(encode_key_action(&action, &mut out), Err(TerminalError::UnsupportedInput));
        assert_eq!(out, b"sentinel");
    }
    assert!(KeyAction::from_key(0, 0, 1).is_err());
    assert!(KeyAction::from_key(0x110001, 8, 1).is_err());
    assert!(KeyAction::from_key(0x110001, 0, 3).is_err());
    assert!(KeyAction::from_key(0xd800, 4, 1).is_err());
    assert!(Modifiers::from_csi_parameter(1).is_err());
    assert!(Modifiers::from_csi_parameter(9).is_err());
}

#[test]
fn release_and_repeat_are_typed_without_expanding_repeat_counts() {
    for state in [KeyState::Release, KeyState::Press, KeyState::Repeat] {
        let action = KeyAction::Key { symbol: KeySymbol::Up, modifiers: Modifiers::CONTROL, state };
        assert_eq!(action.key_fields().unwrap(), (0x110010, 2, state as u8));
        assert_eq!(encode(&action), if state == KeyState::Release { &b""[..] } else { &b"\x1b[1;5A"[..] });
    }
    assert_eq!(decode(b"\x1b[A\x1b[A"), [key(KeySymbol::Up, Modifiers::NONE), key(KeySymbol::Up, Modifiers::NONE)]);
}

#[test]
fn escape_deadline_is_receipt_relative_monotonic_and_not_extended() {
    let mut decoder = Decoder::new();
    let mut out = Vec::new();
    decoder.feed(b"\x1b", 999_999, &mut out).unwrap();
    assert_eq!(decoder.deadline_ns(), Some(30_999_999));
    decoder.expire(30_999_998, &mut out).unwrap();
    assert!(out.is_empty());
    decoder.expire(30_999_999, &mut out).unwrap();
    assert_eq!(out, [key(KeySymbol::Escape, Modifiers::NONE)]);
    assert_eq!(decoder.expire(1, &mut out), Err(TerminalError::InvalidInput));
    for (prefix, expected) in [(b"\x1b[".as_slice(), KeySymbol::Character('[')), (b"\x1bO".as_slice(), KeySymbol::Character('O'))] {
        let mut decoder = Decoder::new();
        let mut out = Vec::new();
        decoder.feed(prefix, 0, &mut out).unwrap();
        decoder.expire(30_000_000, &mut out).unwrap();
        assert_eq!(out, [key(expected, Modifiers::ALT)]);
    }
    let mut decoder = Decoder::new();
    let mut out = Vec::new();
    decoder.feed(b"\x1b", 0, &mut out).unwrap();
    decoder.feed(b"[", 29_000_000, &mut out).unwrap();
    assert_eq!(decoder.deadline_ns(), Some(30_000_000));
    decoder.feed(b"A", 29_999_999, &mut out).unwrap();
    assert_eq!(out, [key(KeySymbol::Up, Modifiers::NONE)]);
    let mut decoder = Decoder::new();
    let mut out = Vec::new();
    decoder.feed(b"\x1b", 0, &mut out).unwrap();
    decoder.feed(b"\x1b", 30_000_000, &mut out).unwrap();
    decoder.expire(60_000_000, &mut out).unwrap();
    assert_eq!(out, [key(KeySymbol::Escape, Modifiers::NONE), key(KeySymbol::Escape, Modifiers::NONE)]);
    assert_eq!(decode(b"\x1b\x1b"), [key(KeySymbol::Escape, Modifiers::ALT)]);
}

#[test]
fn unknown_and_noncanonical_controls_never_guess_keys() {
    for bytes in [&b"\x1b[1A"[..], &b"\x1b[1;1A"[..], &b"\x1b[01;5A"[..], &b"\x1b[1;9A"[..], &b"\x1b[2;3;4~"[..], &b"\x1b[99~"[..], &b"\x1bOx"[..], &b"\x1b[201~"[..]] {
        let mut decoder = Decoder::new();
        let mut out = Vec::new();
        assert_eq!(decoder.feed(bytes, 0, &mut out), Err(TerminalError::UnsupportedInput));
        assert!(out.is_empty());
    }
    let mut decoder = Decoder::new();
    let mut out = Vec::new();
    decoder.feed(b"\x1b[12", 0, &mut out).unwrap();
    assert_eq!(decoder.expire(30_000_000, &mut out), Err(TerminalError::UnsupportedInput));
    assert!(out.is_empty());
    let mut decoder = Decoder::new();
    let mut oversized = Vec::from(&b"\x1b["[..]);
    oversized.resize(Decoder::MAX_CONTROL_LEN + 1, b'1');
    assert_eq!(decoder.feed(&oversized, 0, &mut out), Err(TerminalError::UnsupportedInput));
    assert!(out.is_empty());
}

#[test]
fn paste_is_validated_atomically_and_includes_the_full_64k_payload() {
    let payload = "x".repeat(Decoder::MAX_PASTE_LEN);
    let action = KeyAction::Paste(payload.clone());
    let bytes = encode(&action);
    assert_eq!(bytes.len(), Decoder::MAX_PASTE_LEN + 12);
    let mut decoder = Decoder::new();
    let mut out = Vec::new();
    decoder.feed(&bytes[..bytes.len() - 1], 0, &mut out).unwrap();
    assert!(out.is_empty());
    decoder.feed(&bytes[bytes.len() - 1..], 1, &mut out).unwrap();
    assert_eq!(out, [action.clone()]);
    let mut unbracketed = Vec::new();
    encode_key_action_with_paste(&action, false, &mut unbracketed).unwrap();
    assert_eq!(unbracketed, payload.as_bytes());
    for forbidden in [0, 1, 3, 8, 11, 12, 14, 27, 31, 127, 255] {
        let mut bytes = Vec::from(&b"\x1b[200~valid"[..]);
        bytes.push(forbidden);
        bytes.extend_from_slice(b"\x1b[201~");
        let mut decoder = Decoder::new();
        let mut out = Vec::new();
        assert!(decoder.feed(&bytes, 0, &mut out).is_err());
        assert!(out.is_empty());
    }
    let mut decoder = Decoder::new();
    let mut out = Vec::new();
    decoder.feed(b"\x1b[200~valid\t\r\n", 0, &mut out).unwrap();
    assert_eq!(decoder.flush(&mut out), Err(TerminalError::UnsupportedInput));
    assert!(out.is_empty());
    let oversized = KeyAction::Paste("x".repeat(Decoder::MAX_PASTE_LEN + 1));
    let mut output = Vec::from(&b"prefix"[..]);
    assert_eq!(encode_key_action(&oversized, &mut output), Err(TerminalError::BufferOverflow));
    assert_eq!(output, b"prefix");
    let mut decoder = Decoder::new();
    let mut out = Vec::new();
    decoder.feed(b"\x1b[200~", 0, &mut out).unwrap();
    assert_eq!(decoder.feed("x".repeat(Decoder::MAX_PASTE_LEN + 1).as_bytes(), 0, &mut out), Err(TerminalError::BufferOverflow));
    assert!(out.is_empty());
}

#[test]
fn fragmented_paste_preserves_provenance_and_cr_lf() {
    let bytes = "\x1b[200~é\t\r\n🙂\x1b[201~\r".as_bytes();
    let expected = [KeyAction::Paste(String::from("é\t\r\n🙂")), key(KeySymbol::Enter, Modifiers::NONE)];
    for split in 0..=bytes.len() {
        let mut decoder = Decoder::new();
        let mut out = Vec::new();
        decoder.feed(&bytes[..split], 0, &mut out).unwrap();
        decoder.feed(&bytes[split..], 1, &mut out).unwrap();
        decoder.flush(&mut out).unwrap();
        assert_eq!(out, expected);
    }
    assert_eq!(validate_paste(b"\t\r\n"), Ok("\t\r\n"));
    assert_eq!(validate_text(b"\t"), Err(TerminalError::UnsupportedInput));
}

#[test]
fn viewport_refuses_invalid_dimensions_instead_of_clamping() {
    assert_eq!(ViewportSize::default(), ViewportSize { rows: 24, cols: 80, size_known: false });
    let largest = ViewportSize::try_new(256, 512).unwrap();
    assert_eq!(largest.cells(), ViewportSize::MAX_ACTIVE_CELLS);
    for (rows, cols) in [(0, 80), (24, 0), (257, 80), (24, 513), (u16::MAX, u16::MAX)] {
        assert_eq!(ViewportSize::try_new(rows, cols), Err(TerminalError::InvalidInput));
        assert!(ViewportSize { rows, cols, size_known: true }.validate().is_err());
    }
}

#[test]
#[should_panic(expected = "invalid cellos-vt1 viewport")]
fn compatibility_viewport_constructor_rejects_invalid_dimensions() {
    let _ = ViewportSize::new(0, 80);
}

#[test]
fn screen_controls_keep_the_profile_bytes() {
    let mut out = Vec::new();
    ScreenControls::move_cursor_to(10, 20, &mut out);
    ScreenControls::clear_screen(&mut out);
    ScreenControls::set_cursor_visible(false, &mut out);
    ScreenControls::enter_alternate_screen(&mut out);
    assert_eq!(out, b"\x1b[10;20H\x1b[2J\x1b[H\x1b[?25l\x1b[?1049h");
}

#[test]
fn fragmented_keys_and_ambiguous_alt_prefixes_do_not_lose_following_bytes() {
    for bytes in [
        &b"\x1b[1;8A"[..], &b"\x1b[24;2~"[..], &b"\x1bOP"[..],
        &b"\x1b\x1b[Z"[..], &b"\x1b\xc3\xa9"[..], &b"\x1b\x00"[..],
        &b"\x1b\x1bx"[..], &b"\x1b\x1b[x"[..],
    ] {
        let expected = decode(bytes);
        assert_eq!(expected.iter().flat_map(encode).collect::<Vec<_>>(), bytes);
        for split in 0..=bytes.len() {
            let mut decoder = Decoder::new();
            let mut out = Vec::new();
            decoder.feed(&bytes[..split], 100, &mut out).unwrap();
            decoder.feed(&bytes[split..], 101, &mut out).unwrap();
            decoder.flush(&mut out).unwrap();
            assert_eq!(out, expected);
        }
    }
}

#[test]
fn deadline_overflow_is_rejected_without_an_action() {
    let mut decoder = Decoder::new();
    let mut out = Vec::new();
    assert!(decoder.feed(b"\x1b", u64::MAX, &mut out).is_err());
    assert!(out.is_empty());
}

#[test]
fn immutable_context_roundtrips_exact_argv_raw_cwd_and_sorted_environment() {
    use crate::context::{ChildContext, Grant};
    use api::cell_session::ObjectRefV1;
    let operation = ObjectRefV1 { id: 11, generation: 7 };
    let directory = ObjectRefV1 { id: 23, generation: 9 };
    let cwd = b"/raw/\xff folder";
    let args = ["", "two words", "", "é"];
    let environment = [("A", ""), ("HOME", "/with space"), ("Z", "é")];
    let grant = Grant::cwd(directory).unwrap();
    let original = ChildContext::new(operation, &args, cwd, &environment, &[grant]).unwrap();
    let context = ChildContext::from_bytes(original.as_bytes().to_vec()).unwrap();
    assert_eq!(context.as_bytes(), original.as_bytes());
    assert_eq!(context.operation(), operation);
    assert_eq!(context.argv().collect::<Vec<_>>(), args);
    assert_eq!(context.cwd(), cwd);
    assert_eq!(context.environment().collect::<Vec<_>>(), environment);
    assert_eq!(context.grants().collect::<Vec<_>>(), [grant]);
    assert_eq!(context.cwd_grant(), Some(directory));
    let encoded_argv = b"\0argv1\0\0two words\0\0\xc3\xa9\0";
    assert_eq!(&context.as_bytes()[48..48 + encoded_argv.len()], encoded_argv);
}

#[test]
fn immutable_context_rejects_malformed_environment_and_duplicate_slots() {
    use crate::context::{ChildContext, Grant, GrantSlot};
    use api::cell_session::{ObjectRefV1, Status};
    let operation = ObjectRefV1 { id: 1, generation: 1 };
    for environment in [
        &[("B", "x"), ("A", "y")][..],
        &[("A", "x"), ("A", "y")][..],
        &[("", "x")][..], &[("A=B", "x")][..],
        &[("A\0", "x")][..], &[("A", "x\0")][..],
    ] {
        assert!(matches!(ChildContext::new(operation, &[], b"", environment, &[]), Err(Status::Malformed)));
    }
    let endpoint = Grant::endpoint(
        GrantSlot::Stdin, ObjectRefV1 { id: 4, generation: 2 }, 1,
    ).unwrap();
    assert!(matches!(
        ChildContext::new(operation, &[], b"", &[], &[endpoint, endpoint]),
        Err(Status::Malformed)
    ));
    assert!(matches!(
        ChildContext::new(operation, &[], b"/cwd", &[], &[]),
        Err(Status::Denied)
    ));
    let valid = ChildContext::new(operation, &[], b"", &[("A", "x"), ("B", "y")], &[]).unwrap();
    // Empty argv occupies exactly the seven-byte canonical prefix.
    let environment_offset = 48 + b"\0argv1\0".len();
    for (offset, replacement) in [
        (environment_offset + 4, b'='),
        (environment_offset + 5, 0),
        (environment_offset + 6 + 4, b'A'),
        (environment_offset + 4, 0xff),
    ] {
        let mut bytes = valid.as_bytes().to_vec();
        bytes[offset] = replacement;
        assert!(matches!(ChildContext::from_bytes(bytes), Err(Status::Malformed)));
    }
    let mut wrong_count = valid.as_bytes().to_vec();
    wrong_count[16..18].copy_from_slice(&1u16.to_le_bytes());
    assert!(matches!(ChildContext::from_bytes(wrong_count), Err(Status::Malformed)));
    // A valid wire record cannot bypass duplicate-slot checks at decode time.
    let valid = ChildContext::new(operation, &[], b"", &[], &[endpoint]).unwrap();
    let mut duplicate = valid.as_bytes().to_vec();
    let grant_bytes = duplicate[duplicate.len() - 40..].to_vec();
    duplicate.extend_from_slice(&grant_bytes);
    let total = duplicate.len() as u32;
    duplicate[4..8].copy_from_slice(&total.to_le_bytes());
    duplicate[18..20].copy_from_slice(&2u16.to_le_bytes());
    assert!(matches!(ChildContext::from_bytes(duplicate), Err(Status::Malformed)));
}

fn foreground_grants(cwd: bool, completion: bool) -> Vec<crate::context::Grant> {
    use crate::context::{Grant, GrantSlot};
    use api::cell_session::ObjectRefV1;
    let r = |id| ObjectRefV1 { id, generation: 3 };
    let mut grants = std::vec![
        Grant::endpoint(GrantSlot::Stdin, r(1), 17).unwrap(),
        Grant::endpoint(GrantSlot::Stdout, r(2), 30).unwrap(),
        Grant::endpoint(GrantSlot::Stderr, r(3), 30).unwrap(),
        Grant::endpoint(GrantSlot::Control, r(4), 17).unwrap(),
    ];
    if cwd { grants.push(Grant::cwd(r(5)).unwrap()); }
    if completion {
        let rights = crate::EndpointRights::WRITE_ONLY.bits() as u16;
        grants.push(Grant::endpoint(GrantSlot::Completion, r(6), rights).unwrap());
    }
    grants
}

#[test]
fn foreground_context_len_counts_the_completion_grant() {
    use crate::context::ChildContext;
    use crate::session::foreground_context_len;
    use api::cell_session::{ObjectRefV1, Status};
    let operation = ObjectRefV1 { id: 9, generation: 1 };
    let argv = ["-c", ": p09-fault before-request kill; true"];
    let environment = [("HOME", "/home"), ("TERM", "cellos")];
    for (cwd, completion) in [(false, false), (false, true), (true, false), (true, true)] {
        let path: &[u8] = if cwd { b"/home/user" } else { b"" };
        let grants = foreground_grants(cwd, completion);
        let context = ChildContext::new(operation, &argv, path, &environment, &grants).unwrap();
        let (len, count) = foreground_context_len(&argv, path, &environment, completion).unwrap();
        assert_eq!((len, count), (context.as_bytes().len(), context.grants().count()), "cwd {cwd} completion {completion}");
        assert_eq!(context.completion_grant().is_some(), completion);
    }
    let long = "x".repeat(600);
    assert_eq!(foreground_context_len(&[long.as_str()], b"", &[], true).err(), Some(Status::QuotaExceeded));
}

#[test]
fn completion_writer_is_the_slot_5_endpoint_on_the_context_operation() {
    use crate::context::ChildContext;
    use crate::completion_writer;
    use api::cell_session::ObjectRefV1;
    let operation = ObjectRefV1 { id: 9, generation: 1 };
    let with = ChildContext::new(operation, &[], b"", &[], &foreground_grants(false, true)).unwrap();
    let writer = completion_writer(&with).expect("slot 5 writer");
    assert_eq!((writer.reference().raw(), writer.operation().raw()), (ObjectRefV1 { id: 6, generation: 3 }, operation));
    // Stdout/stderr writers are never mistaken for the completion stream.
    let without = ChildContext::new(operation, &[], b"/", &[], &foreground_grants(true, false)).unwrap();
    assert!(completion_writer(&without).is_none());
}

#[test]
fn pipeline_stage_context_carries_the_completion_slot_only_when_requested() {
    use crate::context::{ChildContext, Grant, GrantSlot};
    use crate::pipe_child::{validate, PipeSpec};
    use api::cell_session::ObjectRefV1;
    let operation = ObjectRefV1 { id: 9, generation: 1 };
    let r = |id| ObjectRefV1 { id, generation: 3 };
    let argv = ["ssh", "--user", "u", "--command", "true"];
    let environment: [(&str, &str); 0] = [];
    for (cwd, completion) in [(false, false), (false, true), (true, false), (true, true)] {
        let path: &[u8] = if cwd { b"/home/user" } else { b"" };
        // Ascending slot order, as the stage launch builds them: stdio, cwd, completion.
        let mut grants = std::vec![
            Grant::endpoint(GrantSlot::Stdin, r(1), crate::EndpointRights::READ_ONLY.bits() as u16).unwrap(),
            Grant::endpoint(GrantSlot::Stdout, r(2), crate::EndpointRights::WRITE_DELEGATE.bits() as u16).unwrap(),
            Grant::endpoint(GrantSlot::Stderr, r(3), crate::EndpointRights::WRITE_DELEGATE.bits() as u16).unwrap(),
        ];
        if cwd { grants.push(Grant::cwd(r(5)).unwrap()); }
        if completion {
            grants.push(Grant::endpoint(GrantSlot::Completion, r(6), crate::EndpointRights::WRITE_ONLY.bits() as u16).unwrap());
        }
        let context = ChildContext::new(operation, &argv, path, &environment, &grants).unwrap();
        let spec = PipeSpec { path: "/bin/cellbox-ssh", argv: &argv, cwd: path, environment: &environment, cwd_source: None, completion };
        assert_eq!(validate(&spec), Ok(context.as_bytes().len()), "cwd {cwd} completion {completion}");
        assert_eq!(context.completion_grant().is_some(), completion);
        assert_eq!(context.grants().count(), 3 + usize::from(cwd) + usize::from(completion));
        // The stage never has a control grant: that is how a child tells a pipeline stage
        // from a terminal foreground.
        assert!(context.grants().all(|g| g.slot != GrantSlot::Control));
    }
}

fn event_frame(sequence: u64, epoch: u64, kind: u16, payload: &[u8]) -> Vec<u8> {
    use api::services::session::EventHeaderV1;
    let mut bytes = std::vec![0; 24 + payload.len()];
    EventHeaderV1 {
        sequence, epoch, kind, payload_len: payload.len() as u16, flags: 0,
    }.encode(&mut bytes).unwrap();
    bytes[24..].copy_from_slice(payload);
    bytes
}

/// A reader's own control ring: strict epochs, owner notices discarded.
fn feed_reader(decoder: &mut crate::input::ControlDecoder, bytes: &[u8], epoch: u64, out: &mut std::collections::VecDeque<crate::input::Event>) -> Result<(), api::cell_session::Status> {
    decoder.feed(bytes, epoch, out, &mut std::collections::VecDeque::new(), false)
}

#[test]
fn event_decode_validates_key_namespace_state_and_payload_limits() {
    use crate::input::{Event, EventKind};
    use api::cell_session::Status;
    let mut payload = [0; 16];
    payload[..4].copy_from_slice(&0x110010u32.to_le_bytes());
    payload[4..8].copy_from_slice(&2u32.to_le_bytes());
    for pressed in 0..=2 {
        payload[8] = pressed;
        let event = Event::decode(&event_frame(4, 7, 1, &payload)).unwrap();
        assert_eq!(event, Event {
            sequence: 4, epoch: 7,
            kind: EventKind::Key { code: 0x110010, modifiers: 2, pressed },
        });
        assert!(!event.is_control());
    }
    for (offset, value, error) in [
        (8, 3, Status::Malformed), (9, 1, Status::Malformed),
        (4, 8, Status::UnsupportedInput),
    ] {
        let mut invalid = payload;
        invalid[offset] = value;
        assert_eq!(Event::decode(&event_frame(1, 1, 1, &invalid)), Err(error));
    }
    assert_eq!(Event::decode(&event_frame(1, 1, 1, &payload[..15])), Err(Status::Malformed));
    let mut too_long = payload.to_vec();
    too_long.push(0);
    assert_eq!(Event::decode(&event_frame(1, 1, 1, &too_long)), Err(Status::Malformed));
    let largest = "x".repeat(512);
    assert_eq!(Event::decode(&event_frame(1, 1, 2, largest.as_bytes())).unwrap().kind, EventKind::Text(largest));
    let mut oversized = event_frame(1, 1, 2, &[b'x'; 512]);
    oversized.push(b'x');
    oversized[18..20].copy_from_slice(&513u16.to_le_bytes());
    assert_eq!(Event::decode(&oversized), Err(Status::Malformed));
    for frame in [
        event_frame(0, 1, 7, b""), event_frame(1, 0, 7, b""),
        event_frame(1, 1, 6, b"x"), event_frame(1, 1, 7, b"x"),
        event_frame(1, 1, 8, &0u32.to_le_bytes()),
        event_frame(1, 1, 8, &6u32.to_le_bytes()),
        event_frame(1, 1, 5, &[0, 0, 80, 0]),
    ] {
        assert!(Event::decode(&frame).is_err());
    }
    // S5-3 annex §3: 4 FrontendLost and 5 FrontendAttached are admitted Lost reasons.
    for reason in [4u32, 5] {
        assert_eq!(Event::decode(&event_frame(1, 1, 8, &reason.to_le_bytes())).unwrap().kind, EventKind::Lost(reason));
    }
}

#[test]
fn control_decoder_reassembles_frames_and_preserves_owner_sequence() {
    use crate::input::{ControlDecoder, EventKind};
    use std::collections::VecDeque;
    let mut bytes = event_frame(3, 9, 5, &[24, 0, 80, 0]);
    bytes.extend_from_slice(&event_frame(4, 9, 6, b""));
    bytes.extend_from_slice(&event_frame(5, 9, 7, b""));
    bytes.extend_from_slice(&event_frame(6, 9, 8, &2u32.to_le_bytes()));
    for split in 0..=bytes.len() {
        let mut decoder = ControlDecoder::default();
        let mut out = VecDeque::new();
        feed_reader(&mut decoder, &bytes[..split], 9, &mut out).unwrap();
        feed_reader(&mut decoder, &bytes[split..], 9, &mut out).unwrap();
        assert!(!decoder.is_partial());
        assert_eq!(out.iter().map(|e| e.sequence).collect::<Vec<_>>(), [3, 4, 5, 6]);
        assert_eq!(out.iter().map(|e| e.kind.clone()).collect::<Vec<_>>(), [
            EventKind::Resize { rows: 24, cols: 80 }, EventKind::Interrupt,
            EventKind::Eof, EventKind::Lost(2),
        ]);
    }
    let mut decoder = ControlDecoder::default();
    let mut out = VecDeque::new();
    for byte in bytes { feed_reader(&mut decoder, &[byte], 9, &mut out).unwrap(); }
    assert_eq!(out.len(), 4);
    assert!(!decoder.is_partial());
}

#[test]
fn control_decoder_rejects_wrong_epoch_malformed_and_oversize_without_delivery() {
    use crate::input::ControlDecoder;
    use api::cell_session::Status;
    use std::collections::VecDeque;
    let mut decoder = ControlDecoder::default();
    let mut out = VecDeque::new();
    assert_eq!(feed_reader(&mut decoder, &event_frame(1, 8, 7, b""), 9, &mut out), Err(Status::WrongEpoch));
    assert!(out.is_empty());
    assert!(!decoder.is_partial());
    let mut malformed = event_frame(1, 9, 7, b"");
    malformed[20] = 1;
    assert!(feed_reader(&mut decoder, &malformed, 9, &mut out).is_err());
    assert!(out.is_empty());
    decoder.clear();
    let mut oversized = event_frame(1, 9, 7, b"");
    oversized[18..20].copy_from_slice(&513u16.to_le_bytes());
    assert_eq!(feed_reader(&mut decoder, &oversized, 9, &mut out), Err(Status::Malformed));
    assert!(!decoder.is_partial());
    assert!(out.is_empty());
    assert_eq!(feed_reader(&mut decoder, &event_frame(1, 9, 2, b"typed"), 9, &mut out), Err(Status::Malformed));
    assert!(out.is_empty());
    let complete = event_frame(1, 9, 5, &[24, 0, 80, 0]);
    for cut in 1..complete.len() {
        let mut decoder = ControlDecoder::default();
        feed_reader(&mut decoder, &complete[..cut], 9, &mut out).unwrap();
        assert!(decoder.is_partial());
        assert!(out.is_empty());
        // is_partial exposes truncated input at EOF; clear prevents an old
        // epoch's bytes from completing a new epoch's control record.
        decoder.clear();
        assert!(!decoder.is_partial());
        feed_reader(&mut decoder, &event_frame(2, 10, 7, b""), 10, &mut out).unwrap();
        assert_eq!(out.pop_front().unwrap().epoch, 10);
    }
}

#[test]
fn control_event_payload_lengths_are_exact_not_prefix_decodes() {
    use crate::input::Event;
    use api::cell_session::Status;
    for (kind, valid_payload) in [
        (5, &[24, 0, 80, 0][..]), (6, &b""[..]),
        (7, &b""[..]), (8, &[1, 0, 0, 0][..]),
    ] {
        let valid = event_frame(1, 1, kind, valid_payload);
        assert!(Event::decode(&valid).unwrap().is_control());
        assert!(Event::decode(&valid[..valid.len() - 1]).is_err());
        let mut trailing = valid.clone();
        trailing.push(0);
        assert_eq!(Event::decode(&trailing), Err(Status::Malformed));
        let mut extra_payload = valid_payload.to_vec();
        extra_payload.push(0);
        assert_eq!(
            Event::decode(&event_frame(1, 1, kind, &extra_payload)),
            Err(Status::Malformed)
        );
        if !valid_payload.is_empty() {
            assert_eq!(
                Event::decode(&event_frame(1, 1, kind, &valid_payload[..valid_payload.len() - 1])),
                Err(Status::Malformed)
            );
        }
    }
}

#[test]
fn consumed_text_is_wiped_in_place_and_other_events_are_untouched() {
    use crate::input::{Event, EventKind};
    let mut event = Event::decode(&event_frame(1, 1, 2, b"hunter2")).unwrap();
    let (pointer, capacity) = match &event.kind {
        EventKind::Text(text) => { assert_eq!(text, "hunter2"); (text.as_ptr(), text.capacity()) }
        other => panic!("{other:?}"),
    };
    event.scrub();
    match &event.kind {
        // The same allocation was zeroed and truncated: no copy was left behind by a realloc.
        EventKind::Text(text) => { assert!(text.is_empty()); assert_eq!(text.as_ptr(), pointer); assert_eq!(text.capacity(), capacity); }
        other => panic!("{other:?}"),
    }
    let mut key = Event::decode(&event_frame(2, 1, 1, &[0x01, 0, 0x11, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0])).unwrap();
    let before = key.clone();
    key.scrub();
    assert_eq!(key, before);
    let mut interrupt = Event::decode(&event_frame(3, 1, 6, b"")).unwrap();
    interrupt.scrub();
    assert!(matches!(interrupt.kind, EventKind::Interrupt));
}

#[test]
fn key_action_scrub_wipes_text_and_paste_only() {
    let mut text = KeyAction::Text(String::from("pw"));
    let mut paste = KeyAction::Paste(String::from("multi\nline secret"));
    let mut plain = key(KeySymbol::Enter, Modifiers::NONE);
    for action in [&mut text, &mut paste, &mut plain] { action.scrub(); }
    assert_eq!(text, KeyAction::Text(String::new()));
    assert_eq!(paste, KeyAction::Paste(String::new()));
    assert_eq!(plain, key(KeySymbol::Enter, Modifiers::NONE));
}

#[test]
fn decoder_failure_discards_pending_bytes_and_decodes_the_next_input_cleanly() {
    let mut decoder = Decoder::new();
    let mut out = Vec::new();
    decoder.feed(b"\x1b[12", 0, &mut out).unwrap();
    assert_eq!(decoder.expire(30_000_000, &mut out), Err(TerminalError::UnsupportedInput));
    assert!(out.is_empty());
    decoder.feed(b"ok", 30_000_001, &mut out).unwrap();
    assert_eq!(out, decode(b"ok"));
}

#[test]
fn control_decoder_clear_discards_a_partial_frame() {
    use crate::input::ControlDecoder;
    use std::collections::VecDeque;
    let frame = event_frame(5, 1, 5, &[24, 0, 80, 0]);
    let mut decoder = ControlDecoder::default();
    let mut out = VecDeque::new();
    feed_reader(&mut decoder, &frame[..10], 1, &mut out).unwrap();
    assert!(decoder.is_partial());
    decoder.clear();
    assert!(!decoder.is_partial());
    feed_reader(&mut decoder, &frame, 1, &mut out).unwrap();
    assert_eq!(out.len(), 1);
}

#[test]
fn controller_ring_delivers_owner_trust_notices_and_drops_what_an_ended_epoch_left() {
    use crate::input::{ControlDecoder, EventKind};
    use api::cell_session::trust::TrustPromptV1;
    use api::services::session::event_kind;
    use std::collections::VecDeque;
    // Exactly what input writes to the controller's own control ring during a foreground
    // (epoch 6): the prompt descriptor, then the end notice. Before it, an unread Resize
    // and an end notice from the controller's earlier epoch 4 are still in the ring.
    let prompt = TrustPromptV1 { port: 2222, host: "gateway", algorithm: "ssh-ed25519", fingerprint: "SHA256:abc" };
    let mut payload = [0u8; TrustPromptV1::MAX_LEN];
    let len = prompt.encode(&mut payload).unwrap();
    let mut ring = event_frame(3, 4, event_kind::RESIZE, &[24, 0, 80, 0]);
    ring.extend_from_slice(&event_frame(4, 4, event_kind::TRUST_PROMPT_END, &0u32.to_le_bytes()));
    ring.extend_from_slice(&event_frame(9, 6, event_kind::TRUST_PROMPT, &payload[..len]));
    ring.extend_from_slice(&event_frame(10, 6, event_kind::TRUST_PROMPT_END, &1u32.to_le_bytes()));
    let expected = [
        EventKind::TrustPrompt { host: "gateway".into(), port: 2222, algorithm: "ssh-ed25519".into(), fingerprint: "SHA256:abc".into() },
        EventKind::TrustPromptEnd { accepted: true },
    ];
    for split in 0..=ring.len() {
        let mut decoder = ControlDecoder::default();
        let (mut out, mut notices) = (VecDeque::new(), VecDeque::new());
        decoder.feed(&ring[..split], 6, &mut out, &mut notices, true).unwrap();
        decoder.feed(&ring[split..], 6, &mut out, &mut notices, true).unwrap();
        // Notices never become reader events: next_event/next_control cannot deliver or acknowledge them.
        assert!(out.is_empty() && !decoder.is_partial(), "split {split}");
        assert_eq!(notices.iter().map(|e| e.kind.clone()).collect::<Vec<_>>(), expected, "split {split}");
        assert!(notices.iter().all(|e| e.is_control() && e.is_owner_notice()));
    }
    // On a reader's own ring the stale Resize is still a protocol error.
    let mut decoder = ControlDecoder::default();
    assert_eq!(feed_reader(&mut decoder, &ring, 6, &mut VecDeque::new()), Err(api::cell_session::Status::WrongEpoch));
    // A forged prompt carrying a terminal escape in a field never decodes.
    let mut forged = event_frame(11, 6, event_kind::TRUST_PROMPT, &payload[..len]);
    forged[24 + 4] = 0x1b;
    let mut decoder = ControlDecoder::default();
    let mut notices = VecDeque::new();
    assert_eq!(decoder.feed(&forged, 6, &mut VecDeque::new(), &mut notices, true), Err(api::cell_session::Status::Malformed));
    assert!(notices.is_empty());
}
