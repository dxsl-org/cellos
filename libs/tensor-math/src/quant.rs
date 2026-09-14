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

/// f32 → IEEE-754 binary16 bits, round-to-nearest-even — the inverse of [`f16_to_f32`], used to
/// store the per-block scale of a freshly quantized activation row.
///
/// Magnitudes above the largest finite half (65504) saturate instead of rounding to infinity: the
/// only caller converts a block peak (a finite f32 by construction), and an infinite scale would
/// turn a finite activation block into NaN.
pub fn f32_to_f16(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32;
    let mantissa = bits & 0x007f_ffff;

    if exponent == 0xff {
        // Infinity keeps its sign; every NaN collapses to one quiet NaN payload.
        return sign | if mantissa == 0 { 0x7c00 } else { 0x7e00 };
    }
    if exponent == 0 {
        // An f32 subnormal is far below the smallest half subnormal (2^-24).
        return sign;
    }

    // `value = wide * 2^(exponent - 150)` with the implicit leading one made explicit.
    let half_exponent = exponent - 112;
    if half_exponent >= 0x1f {
        return sign | 0x7bff;
    }
    if half_exponent <= 0 {
        // Subnormal half: `value = k * 2^-24`, so `k = round(wide * 2^(exponent - 126))`.
        let shift = 126 - exponent;
        if shift > 24 {
            return sign;
        }
        let wide = mantissa | 0x0080_0000;
        let mut k = wide >> shift;
        let remainder = wide & ((1 << shift) - 1);
        let halfway = 1 << (shift - 1);
        if remainder > halfway || (remainder == halfway && k & 1 == 1) {
            k += 1;
        }
        // Rounding up out of the subnormal range lands exactly on the smallest normal (0x0400).
        return sign | k as u16;
    }

    let mut rounded = mantissa;
    let remainder = mantissa & 0x1fff;
    if remainder > 0x1000 || (remainder == 0x1000 && mantissa & 0x2000 != 0) {
        rounded += 0x2000;
        if rounded & 0x0080_0000 != 0 {
            // The mantissa carried into the exponent and is zero afterwards.
            let carried = half_exponent + 1;
            if carried >= 0x1f {
                return sign | 0x7bff;
            }
            return sign | ((carried as u16) << 10);
        }
    }
    sign | ((half_exponent as u16) << 10) | ((rounded >> 13) as u16)
}

/// Quantize `x` into Q8_0 blocks, writing `q8_0_row_bytes(x.len())` bytes into `out`.
///
/// The rule is GGML's, with one deliberate difference: the quantizer divides by the *stored*
/// (f16-rounded) scale, not by the unrounded `amax/127` GGML keeps in a register. Quantizing against
/// the scale a reader will actually multiply by keeps the per-weight error inside half a step, and it
/// is the convention the fixture's weight packer already uses. Rounding is half-away-from-zero
/// (`roundf`), clamped to the `i8` range, so `amax` maps to ±127.
///
/// A block whose peak is zero (or too small to survive the f16 scale) quantizes to all zeros with a
/// zero scale: it dequantizes to zero rather than to NaN. A NaN element also quantizes to zero
/// (Rust's saturating float-to-int cast) instead of propagating, because a NaN *scale* would poison
/// every weight in the row; a block that is entirely NaN has a zero peak and so quantizes to zero.
pub fn q8_0_row_from_f32(x: &[f32], out: &mut [u8]) -> Result<(), MathError> {
    if x.is_empty() {
        return Err(MathError::Empty);
    }
    let Some(row_bytes) = crate::q8_0_row_bytes(x.len()) else {
        return Err(MathError::NotDivisible);
    };
    if out.len() != row_bytes {
        return Err(MathError::ShapeMismatch);
    }
    for (block, slot) in x
        .chunks_exact(Q8_0_BLOCK_WEIGHTS)
        .zip(out.chunks_exact_mut(Q8_0_BLOCK_BYTES))
    {
        let mut peak = 0.0f32;
        for value in block {
            peak = peak.max(value.abs());
        }
        let scale_bits = f32_to_f16(peak / 127.0);
        let scale = f16_to_f32(scale_bits);
        slot[..2].copy_from_slice(&scale_bits.to_le_bytes());
        for (byte, value) in slot[2..].iter_mut().zip(block) {
            let quantized = if scale > 0.0 {
                let scaled = value / scale;
                let rounded = if scaled >= 0.0 {
                    (scaled + 0.5) as i32
                } else {
                    (scaled - 0.5) as i32
                };
                rounded.clamp(i32::from(i8::MIN), i32::from(i8::MAX)) as i8
            } else {
                0
            };
            *byte = quantized as u8;
        }
    }
    Ok(())
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

    #[test]
    fn f16_encode_is_the_exact_inverse_of_the_decoder() {
        // Every finite half (and both infinities) round-trips bit-exactly; NaN payloads collapse to
        // one quiet NaN of the same sign by design, so they are checked separately.
        for bits in 0u16..=u16::MAX {
            let value = f16_to_f32(bits);
            if value.is_nan() {
                assert_eq!(
                    f32_to_f16(value),
                    (bits & 0x8000) | 0x7e00,
                    "NaN half {bits:#06x} re-encodes as a quiet NaN of the same sign"
                );
                continue;
            }
            assert_eq!(
                f32_to_f16(value),
                bits,
                "half {bits:#06x} ({value}) must re-encode to its own bits"
            );
        }
    }

    #[test]
    fn f16_encode_rounds_to_nearest_even_and_saturates() {
        assert_eq!(f32_to_f16(0.0), 0x0000);
        assert_eq!(f32_to_f16(-0.0), 0x8000);
        assert_eq!(f32_to_f16(1.0), 0x3C00);
        assert_eq!(f32_to_f16(0.5), 0x3800);
        assert_eq!(f32_to_f16(-2.0), 0xC000);
        assert_eq!(f32_to_f16(65504.0), 0x7BFF);
        assert_eq!(
            f32_to_f16(65520.0),
            0x7BFF,
            "past the top: saturate, never inf"
        );
        assert_eq!(f32_to_f16(f32::INFINITY), 0x7C00);
        assert_eq!(f32_to_f16(f32::NEG_INFINITY), 0xFC00);
        assert_eq!(f32_to_f16(2f32.powi(-24)), 0x0001, "smallest subnormal");
        assert_eq!(
            f32_to_f16(2f32.powi(-25)),
            0x0000,
            "exactly half a step: ties to even"
        );
        assert_eq!(
            f32_to_f16(3.0 * 2f32.powi(-25)),
            0x0002,
            "1.5 steps rounds up to even"
        );
        assert_eq!(f32_to_f16(1e-30), 0x0000, "below the range: signed zero");
        assert_eq!(f32_to_f16(-1e-30), 0x8000);
        // Halfway cases around 1.0: the step to the next half is 2^-10, so 1 + 2^-11 is exactly the
        // midpoint between 0x3C00 (even mantissa) and 0x3C01, and 1 + 3·2^-11 is the midpoint
        // between 0x3C01 and 0x3C02 (even mantissa again). Ties therefore go up, then stay.
        assert_eq!(f32_to_f16(1.0 + 2f32.powi(-11)), 0x3C00);
        assert_eq!(f32_to_f16(1.0 + 2f32.powi(-11) + 2f32.powi(-23)), 0x3C01);
        assert_eq!(
            f32_to_f16(1.0 + 2f32.powi(-10)),
            0x3C01,
            "exact half, no rounding at all"
        );
        assert_eq!(f32_to_f16(1.0 + 3.0 * 2f32.powi(-11)), 0x3C02);
        assert_eq!(f32_to_f16(1.0 + 2f32.powi(-9)), 0x3C02);
    }

    #[test]
    fn q8_0_row_quantizes_a_peak_to_full_range() {
        let mut x: Vec<f32> = (0..32).map(|i| (i as f32 - 16.0) * 0.25).collect();
        x[0] = -4.0;
        x[31] = 4.0;
        let mut row = [0u8; Q8_0_BLOCK_BYTES];
        q8_0_row_from_f32(&x, &mut row).expect("32 weights is one block");

        let scale_bits = u16::from_le_bytes([row[0], row[1]]);
        let scale = f16_to_f32(scale_bits);
        // The block peak is ±4.0: GGML's rule stores amax/127 as f16.
        assert_eq!(scale_bits, f32_to_f16(4.0 / 127.0));
        assert_eq!(i32::from(row[2] as i8), -127);
        assert_eq!(i32::from(row[Q8_0_BLOCK_BYTES - 1] as i8), 127);

        // Every dequantized weight is within half a step of the value that went in.
        let mut decoded = [0.0f32; Q8_0_BLOCK_WEIGHTS];
        q8_0_block_to_f32(&row, &mut decoded).expect("the row we just wrote decodes");
        for (original, quantized) in x.iter().zip(decoded) {
            assert!(
                (original - quantized).abs() <= scale * 0.5 + f32::EPSILON,
                "{original} quantized to {quantized} (scale {scale})"
            );
        }
    }

    #[test]
    fn q8_0_row_handles_zero_nan_and_shape_errors() {
        let zeros = [0.0f32; 32];
        let mut row = [0xffu8; Q8_0_BLOCK_BYTES];
        q8_0_row_from_f32(&zeros, &mut row).expect("a zero block is representable");
        assert_eq!(
            &row, &[0u8; Q8_0_BLOCK_BYTES],
            "zero in, zero scale and zero weights out"
        );

        let mut with_nan = [1.0f32; 32];
        with_nan[7] = f32::NAN;
        q8_0_row_from_f32(&with_nan, &mut row).expect("NaN elements do not fail the quantizer");
        let mut decoded = [0.0f32; Q8_0_BLOCK_WEIGHTS];
        q8_0_block_to_f32(&row, &mut decoded).expect("decodes");
        assert_eq!(decoded[7], 0.0, "a NaN element quantizes to zero");
        let scale = f16_to_f32(u16::from_le_bytes([row[0], row[1]]));
        assert_eq!(
            u16::from_le_bytes([row[0], row[1]]),
            f32_to_f16(1.0 / 127.0),
            "a NaN peak is ignored, not propagated"
        );
        assert!(
            (decoded[0] - 1.0).abs() <= scale * 0.5 + f32::EPSILON,
            "the finite elements still quantize normally (got {})",
            decoded[0]
        );

        let mut many = [0u8; 2 * Q8_0_BLOCK_BYTES];
        assert_eq!(
            q8_0_row_from_f32(&[1.0f32; 32], &mut many),
            Err(MathError::ShapeMismatch),
            "out must be exactly the row's byte length"
        );
        assert_eq!(
            q8_0_row_from_f32(&[1.0f32; 33], &mut [0u8; Q8_0_BLOCK_BYTES + 2]),
            Err(MathError::NotDivisible),
            "a partial block is a shape error"
        );
        assert_eq!(
            q8_0_row_from_f32(&[], &mut []),
            Err(MathError::Empty),
            "an empty row is undefined"
        );
    }
}
