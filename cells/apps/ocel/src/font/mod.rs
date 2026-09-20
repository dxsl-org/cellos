// SPDX-License-Identifier: MIT
//! Typography and Unicode font rasterizer for Ocel.
//!
//! Provides ASCII + Vietnamese Unicode (Tiếng Việt có dấu) font rendering.

use ostd::font::FONT8X8;

#[derive(Copy, Clone, Debug)]
pub enum Diacritic {
    None,
    Acute,      // Dấu sắc (/)
    Grave,      // Dấu huyền (\)
    Hook,       // Dấu hỏi (?)
    Tilde,      // Dấu ngã (~)
    DotBelow,   // Dấu nặng (.)
    Hat,        // Dấu nón (â, ê, ô)
    Breve,      // Dấu trăng (ă)
    Horn,       // Dấu râu (ơ, ư)
    Bar,        // Gạch ngang (đ, Đ)
    HatAcute,   // ấ, ế, ố
    HatGrave,   // ầ, ề, ồ
    HatHook,    // ổ, ể, ổ
    HatTilde,   // ẫ, ễ, ỗ
    HatDot,     // ậ, ệ, ộ
    BreveAcute, // ắ
    BreveGrave, // ằ
    BreveHook,  // ẳ
    BreveTilde, // ẵ
    BreveDot,   // ặ
    HornAcute,  // ớ, ứ
    HornGrave,  // ờ, ừ
    HornHook,   // ở, ử
    HornTilde,  // ỡ, ữ
    HornDot,    // ợ, ự
}

pub fn get_glyph(c: char) -> [u8; 8] {
    let (base_char, dia) = decompose_vietnamese(c);

    let base_code = base_char as u32;
    let base_idx = if (0x20..=0x7E).contains(&base_code) {
        (base_code - 0x20) as usize
    } else {
        0
    };

    let mut glyph = FONT8X8[base_idx];

    // Apply diacritic modifications
    match dia {
        Diacritic::None => {}
        Diacritic::Acute => {
            glyph[0] |= 0x10;
            glyph[1] |= 0x20;
        }
        Diacritic::Grave => {
            glyph[0] |= 0x20;
            glyph[1] |= 0x10;
        }
        Diacritic::Hook => {
            glyph[0] |= 0x38;
            glyph[1] |= 0x10;
        }
        Diacritic::Tilde => {
            glyph[0] |= 0x2C;
            glyph[1] |= 0x18;
        }
        Diacritic::DotBelow => {
            glyph[7] |= 0x18;
        }
        Diacritic::Hat => {
            glyph[0] |= 0x18;
            glyph[1] |= 0x24;
        }
        Diacritic::Breve => {
            glyph[0] |= 0x24;
            glyph[1] |= 0x18;
        }
        Diacritic::Horn => {
            glyph[1] |= 0x06;
            glyph[2] |= 0x02;
        }
        Diacritic::Bar => {
            glyph[3] |= 0x78;
        }
        Diacritic::HatAcute => {
            glyph[0] |= 0x18;
            glyph[1] |= 0x34;
        }
        Diacritic::HatGrave => {
            glyph[0] |= 0x18;
            glyph[1] |= 0x2C;
        }
        Diacritic::HatHook => {
            glyph[0] |= 0x38;
            glyph[1] |= 0x24;
        }
        Diacritic::HatTilde => {
            glyph[0] |= 0x2C;
            glyph[1] |= 0x24;
        }
        Diacritic::HatDot => {
            glyph[0] |= 0x18;
            glyph[1] |= 0x24;
            glyph[7] |= 0x18;
        }
        Diacritic::BreveAcute => {
            glyph[0] |= 0x34;
            glyph[1] |= 0x18;
        }
        Diacritic::BreveGrave => {
            glyph[0] |= 0x2C;
            glyph[1] |= 0x18;
        }
        Diacritic::BreveHook => {
            glyph[0] |= 0x3C;
            glyph[1] |= 0x18;
        }
        Diacritic::BreveTilde => {
            glyph[0] |= 0x2E;
            glyph[1] |= 0x18;
        }
        Diacritic::BreveDot => {
            glyph[0] |= 0x24;
            glyph[1] |= 0x18;
            glyph[7] |= 0x18;
        }
        Diacritic::HornAcute => {
            glyph[0] |= 0x10;
            glyph[1] |= 0x26;
            glyph[2] |= 0x02;
        }
        Diacritic::HornGrave => {
            glyph[0] |= 0x20;
            glyph[1] |= 0x16;
            glyph[2] |= 0x02;
        }
        Diacritic::HornHook => {
            glyph[0] |= 0x38;
            glyph[1] |= 0x16;
            glyph[2] |= 0x02;
        }
        Diacritic::HornTilde => {
            glyph[0] |= 0x2C;
            glyph[1] |= 0x16;
            glyph[2] |= 0x02;
        }
        Diacritic::HornDot => {
            glyph[1] |= 0x06;
            glyph[2] |= 0x02;
            glyph[7] |= 0x18;
        }
    }

    glyph
}

pub fn decompose_vietnamese(c: char) -> (char, Diacritic) {
    match c {
        // 'a' variants
        'à' => ('a', Diacritic::Grave),
        'á' => ('a', Diacritic::Acute),
        'ả' => ('a', Diacritic::Hook),
        'ã' => ('a', Diacritic::Tilde),
        'ạ' => ('a', Diacritic::DotBelow),

        'ă' => ('a', Diacritic::Breve),
        'ằ' => ('a', Diacritic::BreveGrave),
        'ắ' => ('a', Diacritic::BreveAcute),
        'ẳ' => ('a', Diacritic::BreveHook),
        'ẵ' => ('a', Diacritic::BreveTilde),
        'ặ' => ('a', Diacritic::BreveDot),

        'â' => ('a', Diacritic::Hat),
        'ầ' => ('a', Diacritic::HatGrave),
        'ấ' => ('a', Diacritic::HatAcute),
        'ẩ' => ('a', Diacritic::HatHook),
        'ẫ' => ('a', Diacritic::HatTilde),
        'ậ' => ('a', Diacritic::HatDot),

        // 'e' variants
        'è' => ('e', Diacritic::Grave),
        'é' => ('e', Diacritic::Acute),
        'ẻ' => ('e', Diacritic::Hook),
        'ẽ' => ('e', Diacritic::Tilde),
        'ẹ' => ('e', Diacritic::DotBelow),

        'ê' => ('e', Diacritic::Hat),
        'ề' => ('e', Diacritic::HatGrave),
        'ế' => ('e', Diacritic::HatAcute),
        'ể' => ('e', Diacritic::HatHook),
        'ễ' => ('e', Diacritic::HatTilde),
        'ệ' => ('e', Diacritic::HatDot),

        // 'i' variants
        'ì' => ('i', Diacritic::Grave),
        'í' => ('i', Diacritic::Acute),
        'ỉ' => ('i', Diacritic::Hook),
        'ĩ' => ('i', Diacritic::Tilde),
        'ị' => ('i', Diacritic::DotBelow),

        // 'o' variants
        'ò' => ('o', Diacritic::Grave),
        'ó' => ('o', Diacritic::Acute),
        'ỏ' => ('o', Diacritic::Hook),
        'õ' => ('o', Diacritic::Tilde),
        'ọ' => ('o', Diacritic::DotBelow),

        'ô' => ('o', Diacritic::Hat),
        'ồ' => ('o', Diacritic::HatGrave),
        'ố' => ('o', Diacritic::HatAcute),
        'ổ' => ('o', Diacritic::HatHook),
        'ỗ' => ('o', Diacritic::HatTilde),
        'ộ' => ('o', Diacritic::HatDot),

        'ơ' => ('o', Diacritic::Horn),
        'ờ' => ('o', Diacritic::HornGrave),
        'ớ' => ('o', Diacritic::HornAcute),
        'ở' => ('o', Diacritic::HornHook),
        'ỡ' => ('o', Diacritic::HornTilde),
        'ợ' => ('o', Diacritic::HornDot),

        // 'u' variants
        'ù' => ('u', Diacritic::Grave),
        'ú' => ('u', Diacritic::Acute),
        'ủ' => ('u', Diacritic::Hook),
        'ũ' => ('u', Diacritic::Tilde),
        'ụ' => ('u', Diacritic::DotBelow),

        'ư' => ('u', Diacritic::Horn),
        'ừ' => ('u', Diacritic::HornGrave),
        'ứ' => ('u', Diacritic::HornAcute),
        'ử' => ('u', Diacritic::HornHook),
        'ữ' => ('u', Diacritic::HornTilde),
        'ự' => ('u', Diacritic::HornDot),

        // 'y' variants
        'ỳ' => ('y', Diacritic::Grave),
        'ý' => ('y', Diacritic::Acute),
        'ỷ' => ('y', Diacritic::Hook),
        'ỹ' => ('y', Diacritic::Tilde),
        'ỵ' => ('y', Diacritic::DotBelow),

        // 'd' / 'D' variants
        'đ' => ('d', Diacritic::Bar),
        'Đ' => ('D', Diacritic::Bar),

        // Uppercase 'A'
        'À' | 'Á' | 'Ả' | 'Ã' | 'Ạ' => ('A', Diacritic::Acute),
        'Ă' | 'Ằ' | 'Ắ' | 'Ẳ' | 'Ẵ' | 'Ặ' => ('A', Diacritic::Breve),
        'Â' | 'Ầ' | 'Ấ' | 'Ẩ' | 'Ẫ' | 'Ậ' => ('A', Diacritic::Hat),

        // Uppercase 'E'
        'È' | 'É' | 'Ẻ' | 'Ẽ' | 'Ẹ' => ('E', Diacritic::Acute),
        'Ê' | 'Ề' | 'Ế' | 'Ể' | 'Ễ' | 'Ệ' => ('E', Diacritic::Hat),

        // Uppercase 'I'
        'Ì' | 'Í' | 'Ỉ' | 'Ĩ' | 'Ị' => ('I', Diacritic::Acute),

        // Uppercase 'O'
        'Ò' | 'Ó' | 'Ỏ' | 'Õ' | 'Ọ' => ('O', Diacritic::Acute),
        'Ô' | 'Ồ' | 'Ố' | 'Ổ' | 'Ỗ' | 'Ộ' => ('O', Diacritic::Hat),
        'Ơ' | 'Ờ' | 'Ớ' | 'Ở' | 'Ỡ' | 'Ợ' => ('O', Diacritic::Horn),

        // Uppercase 'U'
        'Ù' | 'Ú' | 'Ủ' | 'Ũ' | 'Ụ' => ('U', Diacritic::Acute),
        'Ư' | 'Ừ' | 'Ứ' | 'Ử' | 'Ữ' | 'Ự' => ('U', Diacritic::Horn),

        // Uppercase 'Y'
        'Ỳ' | 'Ý' | 'Ỷ' | 'Ỹ' | 'Ỵ' => ('Y', Diacritic::Acute),

        _ => (c, Diacritic::None),
    }
}
