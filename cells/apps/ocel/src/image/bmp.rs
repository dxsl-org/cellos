// SPDX-License-Identifier: MIT
//! Pure-Rust, zero-dependency uncompressed BMP decoder for Ocel.

extern crate alloc;
use alloc::vec::Vec;

pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>, // BGRA8888 packed
}

pub fn decode_bmp(data: &[u8]) -> Option<DecodedImage> {
    if data.len() < 54 {
        return None;
    }

    // 1. Check magic "BM"
    if data[0] != b'B' || data[1] != b'M' {
        return None;
    }

    // 2. Read pixel array offset
    let pixel_offset = u32::from_le_bytes([data[10], data[11], data[12], data[13]]) as usize;

    // 3. Read DIB header
    let header_size = u32::from_le_bytes([data[14], data[15], data[16], data[17]]) as usize;
    if header_size < 40 || data.len() < 14 + header_size {
        return None;
    }

    let width_i = i32::from_le_bytes([data[18], data[19], data[20], data[21]]);
    let height_i = i32::from_le_bytes([data[22], data[23], data[24], data[25]]);
    if width_i <= 0 || width_i > 4096 || height_i.abs() > 4096 {
        return None;
    }

    let width = width_i as u32;
    let (height, bottom_up) = if height_i < 0 {
        ((-height_i) as u32, false)
    } else {
        (height_i as u32, true)
    };

    let planes = u16::from_le_bytes([data[26], data[27]]);
    if planes != 1 {
        return None;
    }

    let bpp = u16::from_le_bytes([data[28], data[29]]);
    if bpp != 24 && bpp != 32 {
        return None;
    }

    let compression = u32::from_le_bytes([data[30], data[31], data[32], data[33]]);
    if compression != 0 && compression != 3 {
        // Only uncompressed BI_RGB (0) or BI_BITFIELDS (3)
        return None;
    }

    if pixel_offset >= data.len() {
        return None;
    }

    // 4. Decode scanlines
    let stride = ((width * bpp as u32).div_ceil(32) * 4) as usize;
    let mut pixels = alloc::vec![0u8; (width * height * 4) as usize];

    let bytes_per_pixel = (bpp / 8) as usize;

    for y in 0..height {
        let src_row = if bottom_up { height - 1 - y } else { y };

        let row_start = pixel_offset + (src_row as usize) * stride;
        if row_start + (width as usize) * bytes_per_pixel > data.len() {
            return None;
        }

        let dst_row_start = (y as usize) * (width as usize) * 4;

        for x in 0..width {
            let src_idx = row_start + (x as usize) * bytes_per_pixel;
            let dst_idx = dst_row_start + (x as usize) * 4;

            let b = data[src_idx];
            let g = data[src_idx + 1];
            let r = data[src_idx + 2];
            let a = if bpp == 32 { data[src_idx + 3] } else { 255 };

            pixels[dst_idx] = b;
            pixels[dst_idx + 1] = g;
            pixels[dst_idx + 2] = r;
            pixels[dst_idx + 3] = a;
        }
    }

    Some(DecodedImage {
        width,
        height,
        pixels,
    })
}
