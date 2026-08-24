//! Minimal PPM parsing shared by QEMU screen-capture integration tests.

/// Decoded binary PPM frame.
pub struct PpmFrame {
    width: usize,
    height: usize,
    pub pixels: Vec<u8>,
}

/// Read a binary PPM image produced by QEMU's `screendump` command.
pub fn read_ppm_frame(path: &str) -> PpmFrame {
    let bytes = std::fs::read(path).unwrap_or_else(|error| panic!("{path}: {error}"));
    assert!(bytes.starts_with(b"P6"), "{path}: not a binary PPM");

    let mut tokens = Vec::new();
    let mut token_start = None;
    for (index, byte) in bytes.iter().copied().enumerate() {
        if byte.is_ascii_whitespace() {
            if let Some(start) = token_start.take() {
                tokens.push(&bytes[start..index]);
                if tokens.len() == 4 {
                    let width = std::str::from_utf8(tokens[1])
                        .expect("PPM width is UTF-8")
                        .parse()
                        .expect("PPM width is numeric");
                    let height = std::str::from_utf8(tokens[2])
                        .expect("PPM height is UTF-8")
                        .parse()
                        .expect("PPM height is numeric");
                    return PpmFrame {
                        width,
                        height,
                        pixels: bytes[index + 1..].to_vec(),
                    };
                }
            }
        } else if token_start.is_none() {
            token_start = Some(index);
        }
    }
    panic!("{path}: PPM header has fewer than four tokens");
}

/// Copy a rectangular RGB region from a decoded frame.
pub fn pixel_region(
    frame: &PpmFrame,
    left: usize,
    top: usize,
    right: usize,
    bottom: usize,
) -> Vec<u8> {
    assert!(
        right <= frame.width && bottom <= frame.height,
        "PPM region exceeds frame"
    );
    let mut pixels = Vec::with_capacity((right - left) * (bottom - top) * 3);
    for y in top..bottom {
        let row_start = (y * frame.width + left) * 3;
        let row_end = (y * frame.width + right) * 3;
        pixels.extend_from_slice(&frame.pixels[row_start..row_end]);
    }
    pixels
}
