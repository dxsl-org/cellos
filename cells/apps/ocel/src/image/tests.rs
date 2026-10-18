// SPDX-License-Identifier: MIT
use super::*;

// Real 3x2 PNGs with a uniform color, generated with Pillow.
const RGBA: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 3, 0, 0, 0, 2, 8, 6, 0,
    0, 0, 157, 116, 102, 26, 0, 0, 0, 19, 73, 68, 65, 84, 120, 156, 99, 188, 164, 193, 213, 192, 0,
    5, 76, 12, 72, 0, 0, 36, 118, 1, 136, 193, 65, 42, 169, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66,
    96, 130,
];
const GRAY_ALPHA: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 3, 0, 0, 0, 2, 8, 4, 0,
    0, 0, 55, 125, 174, 145, 0, 0, 0, 22, 73, 68, 65, 84, 120, 156, 99, 244, 109, 96, 96, 96, 96,
    96, 98, 96, 96, 96, 96, 96, 0, 0, 10, 19, 0, 209, 241, 21, 107, 93, 0, 0, 0, 0, 73, 69, 78, 68,
    174, 66, 96, 130,
];

#[test]
fn png_channels_and_alpha_survive_bgra_conversion() {
    for (encoded, expected) in [(RGBA, [10, 40, 210, 128]), (GRAY_ALPHA, [77, 77, 77, 128])] {
        let decoded = decode(encoded).unwrap();
        assert_eq!((decoded.width, decoded.height), (3, 2));
        assert_eq!(decoded.pixels, expected.repeat(6));
    }
}

#[test]
fn truncated_or_corrupt_images_are_rejected() {
    assert!(decode(&RGBA[..40]).is_none());
    let mut corrupt = RGBA.to_vec();
    corrupt[50] ^= 0xff;
    assert!(decode(&corrupt).is_none());
    assert!(decode(b"not an image").is_none());
}

#[test]
fn decoded_pixel_budget_is_checked_without_overflow() {
    let height = MAX_PIXELS / 1024;
    assert_eq!(checked_pixels(1024, height), Some(MAX_PIXELS));
    assert_eq!(checked_pixels(1024, height + 1), None);
    assert_eq!(checked_pixels(usize::MAX, 2), None);
    assert_eq!(checked_pixels(10, 0), None);
}

#[test]
fn bitmap_minimum_signed_height_does_not_panic() {
    let mut header = alloc::vec![0;54];
    header[..2].copy_from_slice(b"BM");
    header[14..18].copy_from_slice(&40u32.to_le_bytes());
    header[18..22].copy_from_slice(&1i32.to_le_bytes());
    header[22..26].copy_from_slice(&i32::MIN.to_le_bytes());
    assert!(decode(&header).is_none());
}

#[test]
fn bitmap_rejects_unimplemented_masks_and_pixels_overlapping_header() {
    let mut bmp = alloc::vec![0u8; 58];
    bmp[..2].copy_from_slice(b"BM");
    bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
    bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
    bmp[18..22].copy_from_slice(&1i32.to_le_bytes());
    bmp[22..26].copy_from_slice(&1i32.to_le_bytes());
    bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
    bmp[28..30].copy_from_slice(&32u16.to_le_bytes());
    bmp[54..58].copy_from_slice(&[12, 34, 56, 255]);
    assert_eq!(decode(&bmp).unwrap().pixels, [12, 34, 56, 255]);
    bmp[30..34].copy_from_slice(&3u32.to_le_bytes());
    assert!(decode(&bmp).is_none(), "bitfields must not silently use BGRA byte order");
    bmp[30..34].copy_from_slice(&0u32.to_le_bytes());
    bmp[10..14].copy_from_slice(&20u32.to_le_bytes());
    assert!(decode(&bmp).is_none(), "DIB bytes are not pixels");
}

#[test]
fn successive_images_advance_layout_and_share_pixel_storage() {
    use crate::doc::{DocNode, Document};
    use alloc::rc::Rc;
    let pixels = Rc::new(alloc::vec![10, 40, 210, 255]);
    let mut document = Document::new();
    document.nodes = alloc::vec![
        DocNode::Image {
            width: 1,
            height: 1,
            pixels: Rc::clone(&pixels)
        },
        DocNode::Image {
            width: 1,
            height: 1,
            pixels: Rc::clone(&pixels)
        },
    ];
    document.compute_layout(&mut ostd::typography::TextFonts::new(), 200);
    assert_eq!(
        document.layout_boxes[1].y_offset,
        document.layout_boxes[0].y_offset + 17
    );
    assert!(Rc::ptr_eq(
        &pixels,
        &document.layout_boxes[0].image.as_ref().unwrap().2
    ));
}

#[test]
fn wide_images_fit_viewport_without_changing_aspect_ratio() {
    use crate::doc::{DocNode, Document};
    use alloc::rc::Rc;
    let mut document = Document::new();
    document.nodes = alloc::vec![DocNode::Image {
        width: 400,
        height: 200,
        pixels: Rc::new(alloc::vec![0; 400 * 200 * 4]),
    }];
    document.compute_layout(&mut ostd::typography::TextFonts::new(), 232);
    assert_eq!(document.layout_boxes[0].height, 116);
    document.compute_layout(&mut ostd::typography::TextFonts::new(), 832);
    assert_eq!(document.layout_boxes[0].height, 216);
}

#[test]
fn webp_decoding_lossy_and_lossless() {
    const WEBP_LOSSLESS: &[u8] = &[
        82, 73, 70, 70, 28, 0, 0, 0, 87, 69, 66, 80, 86, 80, 56, 76, 15, 0, 0, 0, 47, 1, 64, 0,
        0, 7, 16, 253, 143, 254, 7, 34, 162, 255, 1, 0,
    ];
    const WEBP_LOSSY: &[u8] = &[
        82, 73, 70, 70, 60, 0, 0, 0, 87, 69, 66, 80, 86, 80, 56, 32, 48, 0, 0, 0, 208, 1, 0,
        157, 1, 42, 2, 0, 2, 0, 1, 64, 38, 37, 160, 2, 116, 186, 1, 248, 0, 3, 176, 0, 254, 242,
        235, 127, 252, 216, 21, 205, 115, 239, 247, 255, 210, 224, 253, 46, 15, 210, 224, 255,
        210, 144, 0, 0,
    ];
    let img_ll = decode(WEBP_LOSSLESS).expect("decode lossless WebP");
    assert_eq!((img_ll.width, img_ll.height), (2, 2));
    assert_eq!(img_ll.pixels.len(), 2 * 2 * 4);
    // Lossless 2x2 red image in BGRA: [b=0, g=0, r=255, a=255]
    assert_eq!(&img_ll.pixels[0..4], &[0, 0, 255, 255]);

    let img_lossy = decode(WEBP_LOSSY).expect("decode lossy WebP");
    assert_eq!((img_lossy.width, img_lossy.height), (2, 2));
    assert_eq!(img_lossy.pixels.len(), 2 * 2 * 4);
    // Lossy 2x2 red image in BGRA: red channel dominant
    assert!(img_lossy.pixels[2] > 200);

    extern crate std;
    if let Ok(tuoitre_bytes) = std::fs::read("/tmp/tuoitre_sample.webp") {
        let img_tt = decode(&tuoitre_bytes).expect("decode actual Tuoi Tre WebP image");
        assert!(img_tt.width > 0 && img_tt.height > 0);
        assert_eq!(img_tt.pixels.len(), (img_tt.width * img_tt.height * 4) as usize);
    }
}
