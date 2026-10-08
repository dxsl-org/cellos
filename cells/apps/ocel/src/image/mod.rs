// SPDX-License-Identifier: MIT
//! Image decoding module for Ocel.

pub mod bmp;
pub use bmp::decode_bmp;

use bmp::DecodedImage;
use zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};

// Bound decoded storage before asking a codec to allocate its pixel buffer.
const MAX_PIXELS: usize = 1024 * 1024;

pub fn is_image_path(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, extension)| {
        ["bmp", "png", "jpg", "jpeg"]
            .iter()
            .any(|supported| extension.eq_ignore_ascii_case(supported))
    })
}

pub fn decode(data: &[u8]) -> Option<DecodedImage> {
    let options = DecoderOptions::new_safe()
        .set_max_width(4096)
        .set_max_height(4096)
        .inflate_set_limit(MAX_PIXELS * 8 + 4096)
        .png_set_strip_to_8bit(true);
    if data.starts_with(b"BM") {
        return decode_bmp(data);
    }
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        let mut decoder = zune_png::PngDecoder::new_with_options(ZCursor::new(data), options);
        decoder.decode_headers().ok()?;
        let (width, height) = decoder.dimensions()?;
        let count = checked_pixels(width, height)?;
        let colorspace = decoder.colorspace()?;
        let pixels = decoder.decode_raw().ok()?;
        return convert(width, height, count, pixels, colorspace);
    }
    if data.starts_with(b"\xff\xd8") {
        let options = options.jpeg_set_out_colorspace(ColorSpace::RGBA);
        let mut decoder = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(data), options);
        decoder.decode_headers().ok()?;
        let (width, height) = decoder.dimensions()?;
        let count = checked_pixels(width, height)?;
        let pixels = decoder.decode().ok()?;
        return convert(width, height, count, pixels, ColorSpace::RGBA);
    }
    None
}

fn checked_pixels(width: usize, height: usize) -> Option<usize> {
    let count = width.checked_mul(height)?;
    (width > 0 && height > 0 && count <= MAX_PIXELS).then_some(count)
}

fn convert(
    width: usize,
    height: usize,
    count: usize,
    mut pixels: alloc::vec::Vec<u8>,
    colorspace: ColorSpace,
) -> Option<DecodedImage> {
    let channels = match colorspace {
        ColorSpace::RGBA => 4,
        ColorSpace::RGB => 3,
        ColorSpace::LumaA => 2,
        ColorSpace::Luma => 1,
        _ => return None,
    };
    if pixels.len() != count.checked_mul(channels)? {
        return None;
    }
    // Expand backwards in the existing allocation; never keep two full images.
    pixels.resize(count * 4, 0);
    for i in (0..count).rev() {
        let src = i * channels;
        let (r, g, b, a) = match channels {
            4 => (
                pixels[src],
                pixels[src + 1],
                pixels[src + 2],
                pixels[src + 3],
            ),
            3 => (pixels[src], pixels[src + 1], pixels[src + 2], 255),
            2 => (pixels[src], pixels[src], pixels[src], pixels[src + 1]),
            _ => (pixels[src], pixels[src], pixels[src], 255),
        };
        pixels[i * 4..i * 4 + 4].copy_from_slice(&[b, g, r, a]);
    }
    Some(DecodedImage {
        width: width as u32,
        height: height as u32,
        pixels,
    })
}

#[cfg(test)]
mod tests;
