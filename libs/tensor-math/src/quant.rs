//! Q8_0 (GGML) block layout and decoders.
//!
//! A Q8_0 block is [`Q8_0_BLOCK_BYTES`] bytes: a little-endian IEEE-754 binary16 scale
//! followed by [`Q8_0_BLOCK_WEIGHTS`] signed 8-bit weights, dequantized as
//! `w = scale * q`. Blocks are the unit of decoding — a partial block is rejected — so a
//! quantized tensor always has `cols % Q8_0_BLOCK_WEIGHTS == 0` weights per row.
//!
//! Invariant: every decoder validates its input length *before* reading and never indexes
//! outside the buffer it was handed. Malformed bytes produce [`MathError::ShapeMismatch`]
//! rather than a panic, so hostile model files cannot take the engine down.

use super::MathError;

/// Weights stored in one Q8_0 block.
pub const Q8_0_BLOCK_WEIGHTS: usize = 32;

/// Bytes occupied by one Q8_0 block (2-byte f16 scale + 32 i8 weights).
pub const Q8_0_BLOCK_BYTES: usize = 34;

/// Decode one Q8_0 block (2-byte f16 scale + 32 i8) into `out`.
///
/// `block` must hold at least [`Q8_0_BLOCK_BYTES`] bytes; trailing bytes are ignored so the
/// function can be applied straight to a sub-slice of a tensor buffer.
pub fn q8_0_block_to_f32(
    block: &[u8],
    out: &mut [f32; Q8_0_BLOCK_WEIGHTS],
) -> Result<(), MathError> {
    let raw = block
        .get(..Q8_0_BLOCK_BYTES)
        .ok_or(MathError::ShapeMismatch)?;
    let mut bytes = raw.iter();
    // Both `next()` calls are `Some` because `raw` is exactly `Q8_0_BLOCK_BYTES` long.
    let (Some(&lo), Some(&hi)) = (bytes.next(), bytes.next()) else {
        return Err(MathError::ShapeMismatch);
    };
    let scale = f16_to_f32(u16::from_le_bytes([lo, hi]));
    for (slot, &weight) in out.iter_mut().zip(bytes) {
        *slot = scale * f32::from(weight as i8);
    }
    Ok(())
}

/// IEEE-754 binary16 bits → f32 (subnormals, inf, nan included).
///
/// The conversion is exact: every binary16 value, including subnormals and NaN payloads, has
/// an exact binary32 representation.
pub fn f16_to_f32(bits: u16) -> f32 {
    let sign = (bits as u32 & 0x8000) << 16;
    let exponent = (bits as u32 >> 10) & 0x1f;
    let mantissa = bits as u32 & 0x03ff;
    let out = if exponent == 0 {
        if mantissa == 0 {
            sign
        } else {
            // Subnormal half: value = mantissa * 2^-24. Shift the leading one up to the f32
            // implicit-bit position; bits shifted out are discarded by the mask.
            let leading = 31 - mantissa.leading_zeros();
            let exponent32 = leading + 103; // (leading - 24) + bias(127)
            let mantissa32 = (mantissa << (23 - leading)) & 0x007f_ffff;
            sign | (exponent32 << 23) | mantissa32
        }
    } else if exponent == 0x1f {
        // Infinity when the mantissa is zero, otherwise NaN with the payload preserved.
        sign | 0x7f80_0000 | (mantissa << 13)
    } else {
        sign | ((exponent + 112) << 23) | (mantissa << 13)
    };
    f32::from_bits(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f16_decodes_the_edge_cases() {
        assert_eq!(f16_to_f32(0x0000), 0.0);
        assert_eq!(f16_to_f32(0x8000).to_bits(), (-0.0f32).to_bits());
        assert_eq!(f16_to_f32(0x3C00), 1.0);
        assert_eq!(f16_to_f32(0xC000), -2.0);
        assert_eq!(f16_to_f32(0xC100), -2.5);
        assert_eq!(f16_to_f32(0x3555), 0.333_251_95);
        assert_eq!(f16_to_f32(0x7C00), f32::INFINITY);
        assert_eq!(f16_to_f32(0xFC00), f32::NEG_INFINITY);
        assert!(f16_to_f32(0x7E00).is_nan());
        assert!(f16_to_f32(0xFE00).is_nan());
        assert!(f16_to_f32(0x7C01).is_nan());
        // Largest finite half and the subnormal range.
        assert_eq!(f16_to_f32(0x7BFF), 65504.0);
        assert_eq!(f16_to_f32(0x0001), 2f32.powi(-24));
        assert_eq!(f16_to_f32(0x0200), 2f32.powi(-15));
        assert_eq!(f16_to_f32(0x03FF), 1023.0 * 2f32.powi(-24));
        assert_eq!(f16_to_f32(0x0400), 2f32.powi(-14));
    }

    #[test]
    fn q8_0_block_decodes_scale_and_weights() {
        let mut block = [0u8; Q8_0_BLOCK_BYTES];
        block[..2].copy_from_slice(&0x3800u16.to_le_bytes()); // 0.5
        for (i, byte) in block[2..].iter_mut().enumerate() {
            *byte = (i as i32 - 16) as i8 as u8;
        }
        let mut out = [f32::NAN; Q8_0_BLOCK_WEIGHTS];
        q8_0_block_to_f32(&block, &mut out).expect("well-formed block decodes");
        for (i, value) in out.iter().enumerate() {
            assert_eq!(*value, 0.5 * (i as i32 - 16) as f32);
        }
        assert_eq!(out[0], -8.0);
        assert_eq!(out[16], 0.0);
        assert_eq!(out[31], 7.5);
    }

    #[test]
    fn q8_0_block_tolerates_trailing_bytes_and_rejects_short_ones() {
        let mut long = [0u8; Q8_0_BLOCK_BYTES + 5];
        long[..2].copy_from_slice(&0x3C00u16.to_le_bytes()); // 1.0
        long[2] = 7;
        let mut out = [0.0f32; Q8_0_BLOCK_WEIGHTS];
        q8_0_block_to_f32(&long, &mut out).expect("a longer slice still decodes one block");
        assert_eq!(out[0], 7.0);

        for len in [0usize, 1, 2, 33] {
            let short = [0u8; 33];
            let mut out = [0.0f32; Q8_0_BLOCK_WEIGHTS];
            assert_eq!(
                q8_0_block_to_f32(&short[..len], &mut out),
                Err(MathError::ShapeMismatch),
                "a {len}-byte block must be rejected"
            );
        }
    }

    #[test]
    fn row_bytes_is_a_whole_number_of_blocks() {
        assert_eq!(crate::q8_0_row_bytes(0), Some(0));
        assert_eq!(crate::q8_0_row_bytes(32), Some(34));
        assert_eq!(crate::q8_0_row_bytes(64), Some(68));
        assert_eq!(crate::q8_0_row_bytes(32 * 100), Some(34 * 100));
        assert_eq!(crate::q8_0_row_bytes(1), None);
        assert_eq!(crate::q8_0_row_bytes(33), None);
        assert_eq!(crate::q8_0_row_bytes(63), None);
    }
}
