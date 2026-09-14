//! f32 tensor kernels for the Cellos native CPU inference engine (Spec 24 §4).
//!
//! The crate is `no_std` and allocation-free: every kernel reads its operands from slices and
//! writes into a caller-supplied output slice, so the engine can run it out of a static arena
//! with no allocator in the loop. The bare-metal targets ship no `libm`, so the few
//! transcendental functions needed (`sqrt`, `exp`, `pow`, `cos`, `sin`) come from the `libm`
//! crate.
//!
//! # Invariants
//!
//! - **Shapes are validated, arithmetic is not.** Every kernel checks its operand lengths
//!   before it touches memory and reports a [`MathError`] instead of panicking, indexing out
//!   of bounds, or leaving `out` partially written. Malformed *numeric* input (NaN, ±inf) is
//!   not a shape error: it propagates, and [`softmax_in_place`] still returns a probability
//!   vector for it.
//! - **Check precedence** is [`MathError::Empty`] → [`MathError::NotDivisible`] →
//!   [`MathError::ShapeMismatch`], so a caller can rely on which error it gets.
//! - An operation that is undefined over a zero-length operand is [`MathError::Empty`]
//!   (`matvec`/`matvec_q8_0` with no rows or no columns, `rms_norm`, `dot`, `rope_normal` over
//!   an empty vector). A pure element-wise combine over empty slices is the identity and
//!   returns `Ok`.
//! - Quantized operands live in [`quant`], which documents the Q8_0 block layout and the
//!   bounds checks its decoders perform.

#![cfg_attr(not(test), no_std)]

use core::fmt;

pub mod quant;

/// Errors that are the caller's fault (shape mismatch), never arithmetic faults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathError {
    /// Operand lengths disagree with the declared shape.
    ShapeMismatch,
    /// A length is not a whole number of blocks (`cols % 32 != 0` for Q8_0, an odd-length
    /// RoPE head).
    NotDivisible,
    /// The operation is undefined over a zero-length operand.
    Empty,
}

impl fmt::Display for MathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            MathError::ShapeMismatch => "operand shape mismatch",
            MathError::NotDivisible => "length is not a whole number of blocks",
            MathError::Empty => "operation is undefined over an empty operand",
        };
        f.write_str(message)
    }
}

impl core::error::Error for MathError {}

/// `out[r] = dot(W[r], x)`, `W` row-major `rows × cols`, `x.len() == cols`.
///
/// Rows are summed in four independent f32 lanes that are added together at the end; that
/// order is shared with [`matvec_q8_0`], so dequantized Q8_0 weights reproduce the dense
/// result exactly rather than merely approximately.
pub fn matvec(
    out: &mut [f32],
    w: &[f32],
    x: &[f32],
    rows: usize,
    cols: usize,
) -> Result<(), MathError> {
    if rows == 0 || cols == 0 {
        return Err(MathError::Empty);
    }
    let Some(elements) = rows.checked_mul(cols) else {
        return Err(MathError::ShapeMismatch);
    };
    if w.len() != elements || x.len() != cols || out.len() != rows {
        return Err(MathError::ShapeMismatch);
    }
    for (row, slot) in w.chunks_exact(cols).zip(out.iter_mut()) {
        *slot = dot_f32(row, x);
    }
    Ok(())
}

/// Same as [`matvec`] with Q8_0-quantized rows; `w` is `rows × row_bytes` where
/// `row_bytes = cols / 32 * 34` (cols must be a multiple of 32).
///
/// Each 34-byte block is decoded to f32 on the stack and accumulated in the same lane order as
/// [`matvec`], so `matvec_q8_0(out, q, x, r, c)` and `matvec(out, dequantize(q), x, r, c)`
/// agree bit for bit when the dequantized weights are produced from the same block bytes.
///
/// The engine ships [`matvec_q8_0_int8`] instead, because decoding a block to f32 costs more per MAC
/// than the integer dot product (measured: 6.8 vs 12.8 GFLOP/s for the decode, and the decode is the
/// dense kernel's equal — see `libs/ai-engine/benches/cpu_engine.rs`). This kernel is kept as the
/// f32 reference the integer kernel is bounded against, and as the benchmark's comparison row; its
/// own staging block still beats converting i8 inside the accumulation loop (10.3 vs 7.5 GFLOP/s at
/// `-O3`), so do not "simplify" it into the loop.
pub fn matvec_q8_0(
    out: &mut [f32],
    w: &[u8],
    x: &[f32],
    rows: usize,
    cols: usize,
) -> Result<(), MathError> {
    if rows == 0 || cols == 0 {
        return Err(MathError::Empty);
    }
    let Some(row_bytes) = q8_0_row_bytes(cols) else {
        return Err(MathError::NotDivisible);
    };
    let Some(bytes) = rows.checked_mul(row_bytes) else {
        return Err(MathError::ShapeMismatch);
    };
    if w.len() != bytes || x.len() != cols || out.len() != rows {
        return Err(MathError::ShapeMismatch);
    }
    for (row, slot) in w.chunks_exact(row_bytes).zip(out.iter_mut()) {
        let mut acc = [0.0f32; 4];
        let blocks = row.chunks_exact(quant::Q8_0_BLOCK_BYTES);
        let vectors = x.chunks_exact(quant::Q8_0_BLOCK_WEIGHTS);
        for (block, vector) in blocks.zip(vectors) {
            let mut decoded = [0.0f32; quant::Q8_0_BLOCK_WEIGHTS];
            quant::q8_0_block_to_f32(block, &mut decoded)?;
            accumulate_products(&mut acc, &decoded, vector);
        }
        *slot = reduce_lanes(acc);
    }
    Ok(())
}

/// Row bytes for a Q8_0 tensor with `cols` columns, or `None` when `cols` is not a multiple of
/// [`quant::Q8_0_BLOCK_WEIGHTS`].
pub fn q8_0_row_bytes(cols: usize) -> Option<usize> {
    if !cols.is_multiple_of(quant::Q8_0_BLOCK_WEIGHTS) {
        return None;
    }
    (cols / quant::Q8_0_BLOCK_WEIGHTS).checked_mul(quant::Q8_0_BLOCK_BYTES)
}

/// Same as [`matvec_q8_0`] with a Q8_0-quantized activation row instead of an f32 vector.
///
/// `x_q8` is `q8_0_row_bytes(x.len())` bytes as produced by [`quant::q8_0_row_from_f32`], so the
/// activation is rounded once per projection and each block of 32 weights is then a single integer
/// dot product:
///
/// ```text
/// block part = d_w · d_a · Σ (wᵢ · aᵢ)      (Σ in i32, so it is exact)
/// ```
///
/// `|Σ| ≤ 32 · 128² = 524_288`, four orders of magnitude inside `i32`, so the integer part never
/// overflows and carries no rounding at all; the only error against [`matvec_q8_0`] is the half-step
/// of the activation quantization. The per-block parts of one row are summed in four f32 lanes and
/// collapsed with [`reduce_lanes`], the same order both f32 kernels use.
///
/// This is the shipped engine path: it removes the 32-element f32 decode (i8→f32 convert, scale
/// multiply, store) that made the f32-staging kernel ~2× slower per MAC than the dense one, and it
/// removes every f32 multiply and add from the inner loop — which is what decides cost on the cell,
/// where softfloat is emulated.
pub fn matvec_q8_0_int8(
    out: &mut [f32],
    w: &[u8],
    x_q8: &[u8],
    rows: usize,
    cols: usize,
) -> Result<(), MathError> {
    if rows == 0 || cols == 0 {
        return Err(MathError::Empty);
    }
    let Some(row_bytes) = q8_0_row_bytes(cols) else {
        return Err(MathError::NotDivisible);
    };
    let Some(bytes) = rows.checked_mul(row_bytes) else {
        return Err(MathError::ShapeMismatch);
    };
    if w.len() != bytes || x_q8.len() != row_bytes || out.len() != rows {
        return Err(MathError::ShapeMismatch);
    }
    for (row, slot) in w.chunks_exact(row_bytes).zip(out.iter_mut()) {
        let mut acc = [0.0f32; 4];
        for (index, (block, vector)) in row
            .chunks_exact(quant::Q8_0_BLOCK_BYTES)
            .zip(x_q8.chunks_exact(quant::Q8_0_BLOCK_BYTES))
            .enumerate()
        {
            let (Some(&w_lo), Some(&w_hi)) = (block.first(), block.get(1)) else {
                return Err(MathError::ShapeMismatch);
            };
            let (Some(&a_lo), Some(&a_hi)) = (vector.first(), vector.get(1)) else {
                return Err(MathError::ShapeMismatch);
            };
            let d_w = quant::f16_to_f32(u16::from_le_bytes([w_lo, w_hi]));
            let d_a = quant::f16_to_f32(u16::from_le_bytes([a_lo, a_hi]));
            let mut sum = 0i32;
            for (&weight, &activation) in block[2..].iter().zip(&vector[2..]) {
                sum += i32::from(weight as i8) * i32::from(activation as i8);
            }
            acc[index % 4] += (d_w * d_a) * sum as f32;
        }
        *slot = reduce_lanes(acc);
    }
    Ok(())
}

/// `out[i] = x[i] / sqrt(mean(x²) + eps) * weight[i]`.
///
/// Computed with one reciprocal square root, so the stored value is
/// `(x[i] * (1 / sqrt(mean + eps))) * weight[i]`; `eps` is the caller's numerical floor (use
/// `0.0` for an exact normalisation).
pub fn rms_norm(out: &mut [f32], x: &[f32], weight: &[f32], eps: f32) -> Result<(), MathError> {
    if x.is_empty() {
        return Err(MathError::Empty);
    }
    if out.len() != x.len() || weight.len() != x.len() {
        return Err(MathError::ShapeMismatch);
    }
    let mut sum_of_squares = 0.0f32;
    for &value in x {
        sum_of_squares += value * value;
    }
    let mean = sum_of_squares / x.len() as f32;
    let scale = 1.0 / libm::sqrtf(mean + eps);
    for ((slot, &value), &factor) in out.iter_mut().zip(x).zip(weight) {
        *slot = (value * scale) * factor;
    }
    Ok(())
}

/// In-place Llama NORMAL RoPE over one head vector: pairs `(i, i + half)` rotated by
/// `pos * freq_base^(-2i/dim)`.
///
/// The rotation angle is evaluated in f64 (the frequency table is the numerically sensitive
/// part of RoPE) and rounded to f32 for the rotation itself. `pos == 0` returns immediately, so
/// position zero is the exact identity for every input including non-finite ones.
pub fn rope_normal(head: &mut [f32], pos: usize, freq_base: f32) -> Result<(), MathError> {
    if head.is_empty() {
        return Err(MathError::Empty);
    }
    let dim = head.len();
    if !dim.is_multiple_of(2) {
        return Err(MathError::NotDivisible);
    }
    if pos == 0 {
        // cos(0) = 1 and sin(0) = 0 exactly, for any `freq_base`.
        return Ok(());
    }
    let half = dim / 2;
    let (low, high) = head.split_at_mut(half);
    for (i, (low_slot, high_slot)) in low.iter_mut().zip(high.iter_mut()).enumerate() {
        let exponent = -2.0 * (i as f64) / (dim as f64);
        let angle = pos as f64 * libm::pow(freq_base as f64, exponent);
        let cos = libm::cos(angle) as f32;
        let sin = libm::sin(angle) as f32;
        let (x0, x1) = (*low_slot, *high_slot);
        *low_slot = x0 * cos - x1 * sin;
        *high_slot = x0 * sin + x1 * cos;
    }
    Ok(())
}

/// Numerically stable in-place softmax (subtract max).
///
/// The output always sums to 1 and is never NaN: when the input carries no finite ranking
/// information (every element `-inf`, or a NaN anywhere) the uniform distribution `1/n` is
/// written instead. An empty slice is a no-op.
pub fn softmax_in_place(x: &mut [f32]) {
    if x.is_empty() {
        return;
    }
    let mut max = f32::NEG_INFINITY;
    for &value in x.iter() {
        if value > max {
            max = value;
        }
    }
    if !max.is_finite() {
        fill_uniform(x);
        return;
    }
    let mut sum = 0.0f32;
    for slot in x.iter_mut() {
        let weight = libm::expf(*slot - max);
        *slot = weight;
        sum += weight;
    }
    if !(sum.is_finite() && sum > 0.0) {
        fill_uniform(x);
        return;
    }
    let inverse = 1.0 / sum;
    for slot in x.iter_mut() {
        *slot *= inverse;
    }
}

/// SiLU(x) = x * sigmoid(x) = x / (1 + exp(-x)).
pub fn silu(x: f32) -> f32 {
    x / (1.0 + libm::expf(-x))
}

/// `gate[i] = silu(gate[i]) * up[i]` — the Llama-family SwiGLU feed-forward activation.
///
/// The gate projection is the one that goes through SiLU (llama.cpp `ggml_swiglu`: `silu(gate) * up`).
/// Swapping the two branches changes the model output, so this direction is pinned by the engine's
/// golden-oracle test, not only by this crate's unit tests.
pub fn swiglu_in_place(gate: &mut [f32], up: &[f32]) -> Result<(), MathError> {
    if gate.len() != up.len() {
        return Err(MathError::ShapeMismatch);
    }
    for (slot, &value) in gate.iter_mut().zip(up) {
        *slot = silu(*slot) * value;
    }
    Ok(())
}

/// Inner product of two equal-length slices.
pub fn dot(a: &[f32], b: &[f32]) -> Result<f32, MathError> {
    if a.is_empty() || b.is_empty() {
        return Err(MathError::Empty);
    }
    if a.len() != b.len() {
        return Err(MathError::ShapeMismatch);
    }
    Ok(dot_f32(a, b))
}

/// `out[i] += src[i] * scale`.
pub fn scaled_add_in_place(out: &mut [f32], src: &[f32], scale: f32) -> Result<(), MathError> {
    if out.len() != src.len() {
        return Err(MathError::ShapeMismatch);
    }
    for (slot, &value) in out.iter_mut().zip(src) {
        *slot += value * scale;
    }
    Ok(())
}

/// `out[i] += src[i]`.
pub fn add_in_place(out: &mut [f32], src: &[f32]) -> Result<(), MathError> {
    if out.len() != src.len() {
        return Err(MathError::ShapeMismatch);
    }
    for (slot, &value) in out.iter_mut().zip(src) {
        *slot += value;
    }
    Ok(())
}

/// Index of the largest value (first wins on ties); `None` for an empty slice.
///
/// NaN never compares greater, so an all-NaN slice yields `Some(0)`.
pub fn argmax(x: &[f32]) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (index, &value) in x.iter().enumerate() {
        // "Keep the incumbent unless the candidate is strictly greater" is what NaN requires:
        // NaN never compares greater, so an all-NaN slice keeps index 0.
        let replace = match best {
            Some((_, best_value)) => value > best_value,
            None => true,
        };
        if replace {
            best = Some((index, value));
        }
    }
    best.map(|(index, _)| index)
}

/// Deterministic PRNG (xorshift64*) used by sampling.
///
/// The state is never zero: seeding with 0 is remapped to the golden-ratio constant, so any
/// seed produces a usable, non-degenerate stream. The stream is a pure function of the seed.
pub struct Rng(u64);

impl Rng {
    /// Creates a generator from `seed`. Two generators built from the same seed produce the
    /// same sequence.
    pub fn new(seed: u64) -> Self {
        // xorshift64* has a fixed point at zero; substitute the golden ratio instead.
        Rng(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }

    /// Next 32 bits of the output stream (the high half of the xorshift64* output).
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as u32
    }

    /// Uniform in [0, 1): 24 bits of mantissa precision, so the value is always `< 1`.
    pub fn next_f32(&mut self) -> f32 {
        const INV_TWO_POW_24: f32 = 1.0 / 16_777_216.0;
        (self.next_u32() >> 8) as f32 * INV_TWO_POW_24
    }
}

/// Greedy or top-k sampling.
///
/// `temperature <= 0.0` (or NaN) selects [`argmax`]. Otherwise the logits are divided by
/// `temperature`, restricted to the `k` largest (all when `k == 0`), softmaxed, and one index is
/// drawn from `rng`. Ties at the cut-off are broken towards the lower index, matching
/// [`argmax`]. Returns `None` only for an empty `logits` slice.
///
/// `logits` is modified in place — scaled by the temperature, with everything outside the
/// candidate set set to `-inf` — and ends up holding the sampling distribution. The selection
/// uses `O(1)` auxiliary memory and `O(k · n)` time; `k` is the caller's bounded candidate count.
pub fn sample_top_k(
    logits: &mut [f32],
    k: usize,
    temperature: f32,
    rng: &mut Rng,
) -> Option<usize> {
    if logits.is_empty() {
        return None;
    }
    // A non-positive or NaN temperature means greedy decoding (see the crate docs).
    if temperature.is_nan() || temperature <= 0.0 {
        return argmax(logits);
    }
    let inverse_temperature = 1.0 / temperature;
    for value in logits.iter_mut() {
        *value *= inverse_temperature;
    }

    let keep = if k == 0 {
        logits.len()
    } else {
        k.min(logits.len())
    };
    if keep < logits.len() {
        let threshold = kth_largest(logits, keep);
        let mut above = 0usize;
        for &value in logits.iter() {
            if value > threshold {
                above += 1;
            }
        }
        let mut ties_allowed = keep.saturating_sub(above);
        for value in logits.iter_mut() {
            if *value > threshold {
                continue;
            }
            if *value == threshold && ties_allowed > 0 {
                ties_allowed -= 1;
                continue;
            }
            *value = f32::NEG_INFINITY;
        }
    }

    softmax_in_place(logits);
    let mut mass = 0.0f32;
    for &probability in logits.iter() {
        if probability > 0.0 {
            mass += probability;
        }
    }
    let target = rng.next_f32() * mass;
    let mut cumulative = 0.0f32;
    let mut fallback = 0usize;
    for (index, &probability) in logits.iter().enumerate() {
        if probability > 0.0 {
            fallback = index;
            cumulative += probability;
            if cumulative > target {
                return Some(index);
            }
        }
    }
    // Only reachable when rounding puts `target` inside the last bucket's error.
    Some(fallback)
}

/// `k`-th largest value of `x`, duplicates counted (1-based), or `-inf` when `x` holds fewer
/// than `k` values above `-inf`. NaN elements rank nowhere and are never returned.
fn kth_largest(x: &[f32], k: usize) -> f32 {
    let mut upper: Option<f32> = None;
    let mut counted = 0usize;
    loop {
        let mut next = f32::NEG_INFINITY;
        let mut found = false;
        for &value in x.iter() {
            let eligible = match upper {
                Some(bound) => value < bound,
                None => true,
            };
            if eligible && value > next {
                next = value;
                found = true;
            }
        }
        if !found {
            return next;
        }
        for &value in x.iter() {
            if value == next {
                counted += 1;
            }
        }
        if counted >= k {
            return next;
        }
        upper = Some(next);
    }
}

/// Writes the uniform distribution over `x`; `x` must be non-empty.
fn fill_uniform(x: &mut [f32]) {
    debug_assert!(!x.is_empty());
    let uniform = 1.0 / x.len() as f32;
    for slot in x.iter_mut() {
        *slot = uniform;
    }
}

/// Four-lane f32 accumulation of `a[i] * b[i]`; `a` and `b` must be equally long.
#[inline]
fn accumulate_products(acc: &mut [f32; 4], a: &[f32], b: &[f32]) {
    debug_assert_eq!(a.len(), b.len());
    for (left, right) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
        acc[0] += left[0] * right[0];
        acc[1] += left[1] * right[1];
        acc[2] += left[2] * right[2];
        acc[3] += left[3] * right[3];
    }
    for (left, right) in a
        .chunks_exact(4)
        .remainder()
        .iter()
        .zip(b.chunks_exact(4).remainder())
    {
        acc[0] += left * right;
    }
}

/// Collapses the four accumulation lanes in a fixed order.
#[inline]
fn reduce_lanes(acc: [f32; 4]) -> f32 {
    (acc[0] + acc[1]) + (acc[2] + acc[3])
}

/// Sequential f32 inner product over equally long slices.
#[inline]
fn dot_f32(a: &[f32], b: &[f32]) -> f32 {
    let mut acc = [0.0f32; 4];
    accumulate_products(&mut acc, a, b);
    reduce_lanes(acc)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pseudo-random f32 in `[-amplitude, amplitude]`.
    fn random_values(rng: &mut Rng, count: usize, amplitude: f32) -> Vec<f32> {
        (0..count)
            .map(|_| (rng.next_f32() * 2.0 - 1.0) * amplitude)
            .collect()
    }

    /// Packs 32 f32 weights into one Q8_0 block with an f16-exact power-of-two scale, and
    /// returns the block's bytes alongside the weights it dequantizes to.
    fn quantize_block(
        values: &[f32],
        scale_bits: u16,
    ) -> ([u8; quant::Q8_0_BLOCK_BYTES], Vec<f32>) {
        let scale = quant::f16_to_f32(scale_bits);
        let mut block = [0u8; quant::Q8_0_BLOCK_BYTES];
        block[..2].copy_from_slice(&scale_bits.to_le_bytes());
        let mut dequantized = Vec::with_capacity(quant::Q8_0_BLOCK_WEIGHTS);
        for (index, &value) in values.iter().enumerate() {
            let quantized = if scale == 0.0 {
                0.0
            } else {
                (value / scale).round().clamp(-127.0, 127.0)
            };
            block[2 + index] = quantized as i8 as u8;
            dequantized.push(scale * quantized);
        }
        (block, dequantized)
    }

    #[test]
    fn matvec_matches_a_hand_computed_case() {
        let w = [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0]; // 2 × 3
        let x = [1.0f32, 0.5, -1.0];
        let mut out = [0.0f32; 2];
        matvec(&mut out, &w, &x, 2, 3).expect("shapes agree");
        assert_eq!(out, [-1.0, 0.5]);

        let mut single = [0.0f32; 1];
        matvec(&mut single, &[3.0], &[0.25], 1, 1).expect("shapes agree");
        assert_eq!(single, [0.75]);
    }

    #[test]
    fn matvec_agrees_with_a_naive_loop_on_a_taller_matrix() {
        let mut rng = Rng::new(0x5EED);
        let (rows, cols) = (5usize, 7usize);
        let w = random_values(&mut rng, rows * cols, 1.0);
        let x = random_values(&mut rng, cols, 1.0);
        let mut out = vec![0.0f32; rows];
        matvec(&mut out, &w, &x, rows, cols).expect("shapes agree");
        for r in 0..rows {
            let naive: f32 = (0..cols).map(|c| w[r * cols + c] * x[c]).sum();
            assert!(
                (out[r] - naive).abs() < 1e-5,
                "row {r}: {} vs {naive}",
                out[r]
            );
        }
    }

    #[test]
    fn matvec_q8_0_matches_matvec_on_dequantized_weights() {
        // Two rows, one block each, scale 0.5 (f16 0x3800), weights 1..=32 and 2..=33.
        let mut bytes = Vec::new();
        for shift in 0..2i32 {
            bytes.extend_from_slice(&0x3800u16.to_le_bytes());
            for i in 1..=32i32 {
                bytes.push((i + shift) as i8 as u8);
            }
        }
        let x: Vec<f32> = (0..32).map(|i| i as f32 * 0.25 - 2.0).collect();
        let mut quantized_out = [0.0f32; 2];
        matvec_q8_0(&mut quantized_out, &bytes, &x, 2, 32).expect("shapes agree");

        let mut dense = vec![0.0f32; 2 * 32];
        for r in 0..2 {
            for c in 0..32 {
                dense[r * 32 + c] = 0.5 * ((c as i32 + 1 + r as i32) as f32);
            }
        }
        let mut dense_out = [0.0f32; 2];
        matvec(&mut dense_out, &dense, &x, 2, 32).expect("shapes agree");

        for r in 0..2 {
            assert!((quantized_out[r] - dense_out[r]).abs() < 1e-4);
        }
        assert_eq!(
            quantized_out, dense_out,
            "the lane order is shared, so this is exact"
        );
    }

    #[test]
    fn matvec_q8_0_over_a_random_matrix_equals_dense_matvec_over_dequantized_rows() {
        for &(rows, cols, seed) in &[(1usize, 32usize, 1u64), (3, 64, 2), (7, 96, 3), (4, 320, 4)] {
            check_q8_0_equivalence(rows, cols, seed);
        }
    }

    fn check_q8_0_equivalence(rows: usize, cols: usize, seed: u64) {
        let blocks_per_row = cols / quant::Q8_0_BLOCK_WEIGHTS;
        let row_bytes = q8_0_row_bytes(cols).expect("cols is a multiple of 32");
        let scale_bits = [0x2000u16, 0x1C00, 0x2400, 0x1800]; // 2^-7, 2^-9, 2^-6, 2^-11
        let mut rng = Rng::new(seed);
        let mut weights = vec![0u8; rows * row_bytes];
        let mut dense = vec![0.0f32; rows * cols];
        let mut magnitude = 0.0f32;
        for r in 0..rows {
            for b in 0..blocks_per_row {
                let bits = scale_bits[(r + b) % scale_bits.len()];
                let amplitude = quant::f16_to_f32(bits) * 126.0;
                let values = random_values(&mut rng, quant::Q8_0_BLOCK_WEIGHTS, amplitude);
                let (block, dequantized) = quantize_block(&values, bits);
                let byte_offset = r * row_bytes + b * quant::Q8_0_BLOCK_BYTES;
                weights[byte_offset..byte_offset + quant::Q8_0_BLOCK_BYTES].copy_from_slice(&block);
                let float_offset = r * cols + b * quant::Q8_0_BLOCK_WEIGHTS;
                dense[float_offset..float_offset + quant::Q8_0_BLOCK_WEIGHTS]
                    .copy_from_slice(&dequantized);
            }
        }
        let x = random_values(&mut rng, cols, 1.0);
        let mut quantized_out = vec![0.0f32; rows];
        matvec_q8_0(&mut quantized_out, &weights, &x, rows, cols).expect("shapes agree");
        let mut dense_out = vec![0.0f32; rows];
        matvec(&mut dense_out, &dense, &x, rows, cols).expect("shapes agree");

        for r in 0..rows {
            magnitude = magnitude.max(dense_out[r].abs());
            assert!(
                (quantized_out[r] - dense_out[r]).abs() < 1e-4,
                "rows {rows}, cols {cols}, row {r}: {} vs {}",
                quantized_out[r],
                dense_out[r]
            );
        }
        assert!(
            magnitude > 1e-3,
            "the fixture must produce non-trivial activations"
        );
    }

    #[test]
    fn matvec_q8_0_int8_is_exact_when_quantization_is_exact() {
        // Both operands exactly representable at scale 0.5: the weights are 0.5·q with q = 1..=32 and
        // 2..=33, the activations are multiples of 0.5 whose peak is 63.5 (so amax/127 = 0.5 exactly).
        // Every product is a multiple of 0.25 well inside f32's exact range, so the integer kernel and
        // the dense kernel must agree bit for bit: the integer dot and the block scale add nothing.
        let mut weights = Vec::new();
        for shift in 0..2i32 {
            weights.extend_from_slice(&0x3800u16.to_le_bytes());
            for i in 1..=32i32 {
                weights.push((i + shift) as i8 as u8);
            }
        }
        let x: Vec<f32> = (0..32).map(|i| 63.5 - i as f32).collect();
        let mut x_q8 = vec![0u8; q8_0_row_bytes(32).expect("32 columns is one block")];
        quant::q8_0_row_from_f32(&x, &mut x_q8).expect("32 values is one block");
        assert_eq!(
            u16::from_le_bytes([x_q8[0], x_q8[1]]),
            quant::f32_to_f16(0.5),
            "the fixture's activation scale must be exact, or the test proves nothing"
        );

        let mut int8_out = [0.0f32; 2];
        matvec_q8_0_int8(&mut int8_out, &weights, &x_q8, 2, 32).expect("shapes agree");

        let mut dense = vec![0.0f32; 2 * 32];
        for r in 0..2 {
            for c in 0..32 {
                dense[r * 32 + c] = 0.5 * (c as i32 + 1 + r as i32) as f32;
            }
        }
        let mut dense_out = [0.0f32; 2];
        matvec(&mut dense_out, &dense, &x, 2, 32).expect("shapes agree");
        assert_eq!(int8_out, dense_out);
    }

    #[test]
    fn matvec_q8_0_int8_stays_within_the_activation_quantization_bound() {
        // The only difference from the f32-staging kernel is that the activation is rounded to i8:
        // each element moves by at most d_a/2, which the row scales by |wᵢ| = d_w·|qᵢ|. Summing
        // d_w·d_a·0.5·Σ|qᵢ| over the row's blocks is therefore a hard bound on the difference.
        for &(rows, cols, seed) in &[(1usize, 32usize, 11u64), (3, 64, 12), (5, 160, 13)] {
            let blocks_per_row = cols / quant::Q8_0_BLOCK_WEIGHTS;
            let row_bytes = q8_0_row_bytes(cols).expect("cols is a multiple of 32");
            let scale_bits = [0x2000u16, 0x1C00, 0x2400, 0x1800]; // 2^-7, 2^-9, 2^-6, 2^-11
            let mut rng = Rng::new(seed);
            let mut weights = vec![0u8; rows * row_bytes];
            let mut bound = 0.0f32;
            for r in 0..rows {
                for b in 0..blocks_per_row {
                    let bits = scale_bits[(r + b) % scale_bits.len()];
                    let amplitude = quant::f16_to_f32(bits) * 126.0;
                    let values = random_values(&mut rng, quant::Q8_0_BLOCK_WEIGHTS, amplitude);
                    let (block, _) = quantize_block(&values, bits);
                    let byte_offset = r * row_bytes + b * quant::Q8_0_BLOCK_BYTES;
                    weights[byte_offset..byte_offset + quant::Q8_0_BLOCK_BYTES]
                        .copy_from_slice(&block);
                }
            }
            let x = random_values(&mut rng, cols, 1.0);
            let mut x_q8 = vec![0u8; row_bytes];
            quant::q8_0_row_from_f32(&x, &mut x_q8).expect("cols is a whole number of blocks");
            for r in 0..rows {
                for b in 0..blocks_per_row {
                    let base = r * row_bytes + b * quant::Q8_0_BLOCK_BYTES;
                    let d_w =
                        quant::f16_to_f32(u16::from_le_bytes([weights[base], weights[base + 1]]));
                    let d_a = quant::f16_to_f32(u16::from_le_bytes([
                        x_q8[b * quant::Q8_0_BLOCK_BYTES],
                        x_q8[b * quant::Q8_0_BLOCK_BYTES + 1],
                    ]));
                    let sum_abs: f32 = weights[base + 2..base + quant::Q8_0_BLOCK_BYTES]
                        .iter()
                        .map(|byte| (*byte as i8 as f32).abs())
                        .sum();
                    bound += d_w * d_a * 0.5 * sum_abs;
                }
            }

            let mut int8_out = vec![0.0f32; rows];
            matvec_q8_0_int8(&mut int8_out, &weights, &x_q8, rows, cols).expect("shapes agree");
            let mut staging_out = vec![0.0f32; rows];
            matvec_q8_0(&mut staging_out, &weights, &x, rows, cols).expect("shapes agree");

            for r in 0..rows {
                let error = (int8_out[r] - staging_out[r]).abs();
                assert!(
                    error <= bound + 1e-5,
                    "rows {rows}, cols {cols}, row {r}: error {error} exceeds the bound {bound}"
                );
                assert!(
                    int8_out[r] != 0.0 && staging_out[r] != 0.0,
                    "row {r} must be non-trivial for the comparison to mean anything"
                );
            }
        }
    }

    #[test]
    fn matvec_q8_0_int8_checks_its_shapes() {
        let weights = vec![0u8; q8_0_row_bytes(32).expect("one block")];
        let activations = vec![0u8; q8_0_row_bytes(32).expect("one block")];
        let mut out = [0.0f32; 1];
        assert_eq!(
            matvec_q8_0_int8(&mut out, &weights, &activations, 0, 32),
            Err(MathError::Empty)
        );
        assert_eq!(
            matvec_q8_0_int8(&mut out, &weights, &activations, 1, 0),
            Err(MathError::Empty)
        );
        assert_eq!(
            matvec_q8_0_int8(&mut out, &weights, &activations, 1, 33),
            Err(MathError::NotDivisible)
        );
        assert_eq!(
            matvec_q8_0_int8(&mut out, &weights, &activations, 2, 32),
            Err(MathError::ShapeMismatch),
            "out must hold one value per row"
        );
        assert_eq!(
            matvec_q8_0_int8(&mut out, &weights[..33], &activations, 1, 32),
            Err(MathError::ShapeMismatch),
            "w must hold one whole row"
        );
        assert_eq!(
            matvec_q8_0_int8(&mut out, &weights, &activations[..33], 1, 32),
            Err(MathError::ShapeMismatch),
            "x_q8 must hold one whole row"
        );
        assert_eq!(
            matvec_q8_0_int8(&mut [0.0f32; 2], &weights, &activations, 1, 32),
            Err(MathError::ShapeMismatch),
            "out must be exactly one value per row, as in the sibling kernels"
        );
    }

    #[test]
    fn quantized_rows_stay_within_the_quantization_error_of_the_source() {
        // The quantize/dequantize fixture itself must be faithful to the weights it encodes.
        let values: Vec<f32> = (0..32).map(|i| (i as f32 - 15.5) / 16.0).collect();
        let (block, dequantized) = quantize_block(&values, 0x2000); // scale 2^-7
        let mut decoded = [0.0f32; quant::Q8_0_BLOCK_WEIGHTS];
        quant::q8_0_block_to_f32(&block, &mut decoded).expect("well-formed block decodes");
        assert_eq!(decoded.as_slice(), dequantized.as_slice());
        for (source, restored) in values.iter().zip(&dequantized) {
            assert!((source - restored).abs() <= 2.0f32.powi(-8));
        }
    }

    #[test]
    fn kernels_reject_bad_shapes() {
        let mut out = [0.0f32; 4];
        let w = [0.0f32; 8];
        let x = [0.0f32; 4];
        // matvec
        assert_eq!(matvec(&mut out[..2], &w, &x, 2, 4), Ok(()));
        assert_eq!(
            matvec(&mut out[..2], &w, &x, 3, 4),
            Err(MathError::ShapeMismatch)
        );
        assert_eq!(
            matvec(&mut out[..2], &w, &[0.0; 3], 2, 4),
            Err(MathError::ShapeMismatch)
        );
        assert_eq!(
            matvec(&mut out[..1], &w, &x, 2, 4),
            Err(MathError::ShapeMismatch)
        );
        assert_eq!(matvec(&mut out, &[], &[], 0, 4), Err(MathError::Empty));
        assert_eq!(matvec(&mut out, &[], &[], 2, 0), Err(MathError::Empty));
        // matvec_q8_0
        let q = [0u8; 2 * 34];
        assert_eq!(matvec_q8_0(&mut out[..2], &q, &[0.0; 32], 2, 32), Ok(()));
        assert_eq!(
            matvec_q8_0(&mut out[..2], &q, &[0.0; 32], 2, 33),
            Err(MathError::NotDivisible)
        );
        assert_eq!(
            matvec_q8_0(&mut out[..2], &q[..34], &[0.0; 32], 2, 32),
            Err(MathError::ShapeMismatch)
        );
        assert_eq!(
            matvec_q8_0(&mut out[..2], &q, &[0.0; 31], 2, 32),
            Err(MathError::ShapeMismatch)
        );
        assert_eq!(
            matvec_q8_0(&mut out, &q, &[0.0; 32], 2, 32),
            Err(MathError::ShapeMismatch)
        );
        assert_eq!(
            matvec_q8_0(&mut out, &[], &[], 0, 32),
            Err(MathError::Empty)
        );
        assert_eq!(matvec_q8_0(&mut out, &[], &[], 2, 0), Err(MathError::Empty));
        // rms_norm
        assert_eq!(rms_norm(&mut out, &x, &x, 0.0), Ok(()));
        assert_eq!(
            rms_norm(&mut out[..3], &x, &x, 0.0),
            Err(MathError::ShapeMismatch)
        );
        assert_eq!(
            rms_norm(&mut out, &x, &x[..3], 0.0),
            Err(MathError::ShapeMismatch)
        );
        assert_eq!(
            rms_norm(&mut out[..0], &[], &[], 0.0),
            Err(MathError::Empty)
        );
        // rope_normal
        let mut head = [0.0f32; 4];
        assert_eq!(rope_normal(&mut head, 3, 10_000.0), Ok(()));
        assert_eq!(
            rope_normal(&mut head[..3], 3, 10_000.0),
            Err(MathError::NotDivisible)
        );
        assert_eq!(rope_normal(&mut [], 3, 10_000.0), Err(MathError::Empty));
        // swiglu_in_place
        let mut gate = [0.0f32; 4];
        assert_eq!(swiglu_in_place(&mut gate, &x), Ok(()));
        assert_eq!(
            swiglu_in_place(&mut gate[..3], &x),
            Err(MathError::ShapeMismatch)
        );
        assert_eq!(swiglu_in_place(&mut [], &[]), Ok(()));
        // dot
        assert_eq!(dot(&x, &x), Ok(0.0));
        assert_eq!(dot(&x, &x[..3]), Err(MathError::ShapeMismatch));
        assert_eq!(dot(&[], &[]), Err(MathError::Empty));
        assert_eq!(dot(&x, &[]), Err(MathError::Empty));
        // scaled_add_in_place / add_in_place
        assert_eq!(scaled_add_in_place(&mut gate, &x, 0.5), Ok(()));
        assert_eq!(
            scaled_add_in_place(&mut gate, &x[..2], 0.5),
            Err(MathError::ShapeMismatch)
        );
        assert_eq!(add_in_place(&mut gate, &x), Ok(()));
        assert_eq!(
            add_in_place(&mut gate, &x[..2]),
            Err(MathError::ShapeMismatch)
        );
        assert_eq!(scaled_add_in_place(&mut [], &[], 0.5), Ok(()));
        assert_eq!(add_in_place(&mut [], &[]), Ok(()));
    }

    #[test]
    fn rms_norm_scales_to_unit_rms_and_applies_the_weight() {
        let x = [1.0f32, -2.0, 3.0, 4.0];
        let unit = [1.0f32; 4];
        let mut out = [0.0f32; 4];
        rms_norm(&mut out, &x, &unit, 0.0).expect("shapes agree");
        let rms = 7.5f32.sqrt(); // sqrt(mean(x²)) = sqrt(30/4)
        for (got, source) in out.iter().zip(&x) {
            assert!((got - source / rms).abs() < 1e-6);
        }
        let energy: f32 = out.iter().map(|v| v * v).sum::<f32>() / 4.0;
        assert!((energy - 1.0).abs() < 1e-6, "unit weight leaves unit RMS");

        let weight = [2.0f32, 0.0, 0.5, -1.0];
        rms_norm(&mut out, &x, &weight, 0.0).expect("shapes agree");
        for i in 0..4 {
            assert!((out[i] - (x[i] / rms) * weight[i]).abs() < 1e-6);
        }

        rms_norm(&mut out, &x, &unit, 4.5).expect("shapes agree");
        let eps_rms = (7.5f32 + 4.5).sqrt();
        for i in 0..4 {
            assert!((out[i] - x[i] / eps_rms).abs() < 1e-6);
        }
    }

    #[test]
    fn rope_normal_is_identity_at_position_zero_and_preserves_norm() {
        let original = [1.0f32, -0.5, 0.25, 2.0, -1.5, 0.75, 0.125, -0.25];
        let mut head = original;
        rope_normal(&mut head, 0, 10_000.0).expect("shape agrees");
        assert_eq!(head, original, "position 0 is the exact identity");

        let mut rotated = original;
        rope_normal(&mut rotated, 7, 10_000.0).expect("shape agrees");
        assert_ne!(rotated, original);
        let before: f32 = original.iter().map(|v| v * v).sum();
        let after: f32 = rotated.iter().map(|v| v * v).sum();
        assert!((before - after).abs() < 1e-5, "{before} vs {after}");

        // Pair (0, 4) rotates by pos * base^0 = 7 rad.
        let (cos, sin) = (7.0f32.cos(), 7.0f32.sin());
        let (x0, x1) = (original[0], original[4]);
        assert!((rotated[0] - (x0 * cos - x1 * sin)).abs() < 1e-5);
        assert!((rotated[4] - (x0 * sin + x1 * cos)).abs() < 1e-5);
        // Pair (1, 5) rotates by pos * base^(-2/8).
        let angle = 7.0 * 10_000f64.powf(-0.25);
        let (cos, sin) = (angle.cos() as f32, angle.sin() as f32);
        let (x0, x1) = (original[1], original[5]);
        assert!((rotated[1] - (x0 * cos - x1 * sin)).abs() < 1e-5);
        assert!((rotated[5] - (x0 * sin + x1 * cos)).abs() < 1e-5);
    }

    #[test]
    fn rope_normal_handles_a_two_element_head() {
        let mut head = [3.0f32, 4.0];
        rope_normal(&mut head, 5, 10_000.0).expect("shape agrees");
        let angle = 5.0f64;
        let (cos, sin) = (angle.cos() as f32, angle.sin() as f32);
        assert!((head[0] - (3.0 * cos - 4.0 * sin)).abs() < 1e-5);
        assert!((head[1] - (3.0 * sin + 4.0 * cos)).abs() < 1e-5);
        let norm: f32 = head.iter().map(|v| v * v).sum();
        assert!((norm - 25.0).abs() < 1e-4);
    }

    #[test]
    fn softmax_sums_to_one_and_ignores_a_constant_offset() {
        let base = [1.0f32, -2.0, 0.5, 3.25];
        let mut plain = base;
        softmax_in_place(&mut plain);
        let sum: f32 = plain.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6, "sum = {sum}");
        assert!(plain.iter().all(|p| *p > 0.0));

        let mut shifted = base;
        for value in shifted.iter_mut() {
            *value += 1_000.0;
        }
        softmax_in_place(&mut shifted);
        for i in 0..base.len() {
            assert!((plain[i] - shifted[i]).abs() < 1e-6);
        }

        // The largest logit keeps the largest probability, and equal logits split evenly.
        assert!(plain[3] > plain[0] && plain[0] > plain[2] && plain[2] > plain[1]);
        let mut equal = [0.5f32; 5];
        softmax_in_place(&mut equal);
        assert_eq!(equal, [0.2f32; 5]);

        // Degenerate inputs still produce a probability vector.
        let mut all_neg_inf = [f32::NEG_INFINITY; 3];
        softmax_in_place(&mut all_neg_inf);
        assert_eq!(all_neg_inf, [1.0 / 3.0; 3]);
        let mut with_nan = [0.0f32, f32::NAN, 1.0];
        softmax_in_place(&mut with_nan);
        assert_eq!(with_nan, [1.0 / 3.0; 3]);
        softmax_in_place(&mut []);
    }

    #[test]
    fn swiglu_and_elementwise_combines_behave() {
        let up = [0.0f32, 1.0, -1.0, 10.0];
        let mut gate = [1.0f32, 2.0, 3.0, 4.0];
        swiglu_in_place(&mut gate, &up).expect("shapes agree");
        // The gate branch carries the activation; the up branch is the plain multiplicand.
        assert_eq!(gate[0], 0.0); // up[0] = 0
        assert!((gate[1] - silu(2.0)).abs() < 1e-6);
        assert!((gate[2] - -1.0 * silu(3.0)).abs() < 1e-6);
        assert!((gate[3] - 10.0 * silu(4.0)).abs() < 1e-4);

        // Branch order matters: swapping the arguments must change the result.
        let mut swapped = [1.0f32, 2.0, 3.0, 4.0];
        swiglu_in_place(&mut swapped, &[0.0, 1.0, -1.0, 10.0]).expect("shapes agree");
        let mut swapped_swapped = [0.0f32, 1.0, -1.0, 10.0];
        swiglu_in_place(&mut swapped_swapped, &[1.0, 2.0, 3.0, 4.0]).expect("shapes agree");
        assert_ne!(swapped, swapped_swapped);

        // silu is the identity-ish ramp for large positive input and 0 for large negative.
        assert_eq!(silu(0.0), 0.0);
        assert!((silu(20.0) - 20.0).abs() < 1e-6);
        assert!(silu(-20.0).abs() < 1e-7);
        assert!((silu(1.0) - 0.731_058_6).abs() < 1e-6);

        let mut acc = [1.0f32, 2.0, 3.0];
        add_in_place(&mut acc, &[0.5, -0.5, 0.0]).expect("shapes agree");
        assert_eq!(acc, [1.5, 1.5, 3.0]);
        scaled_add_in_place(&mut acc, &[2.0, -2.0, 4.0], 0.25).expect("shapes agree");
        assert_eq!(acc, [2.0, 1.0, 4.0]);
    }

    #[test]
    fn dot_matches_a_naive_loop() {
        let a = [1.0f32, -2.0, 3.5, 0.25, -1.0, 2.0];
        let b = [0.5f32, 4.0, -1.0, 2.0, 0.0, -0.5];
        let naive: f32 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
        assert!((dot(&a, &b).expect("shapes agree") - naive).abs() < 1e-6);
        assert_eq!(dot(&[2.0], &[3.0]), Ok(6.0));
    }

    #[test]
    fn argmax_takes_the_first_of_equal_maxima() {
        assert_eq!(argmax(&[]), None);
        assert_eq!(argmax(&[1.0]), Some(0));
        assert_eq!(argmax(&[1.0, 5.0, -2.0]), Some(1));
        assert_eq!(argmax(&[1.0, 5.0, 5.0]), Some(1));
        assert_eq!(argmax(&[f32::NAN, f32::NAN]), Some(0));
        assert_eq!(argmax(&[f32::NEG_INFINITY, f32::NEG_INFINITY]), Some(0));
    }

    #[test]
    fn rng_is_deterministic_and_stays_in_range() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..64 {
            let value = a.next_f32();
            assert_eq!(value, b.next_f32());
            assert!((0.0..1.0).contains(&value), "{value} out of range");
            assert_eq!(a.next_u32(), b.next_u32());
        }
        // A zero seed must not collapse the stream.
        let mut zero_seeded = Rng::new(0);
        let first = zero_seeded.next_u32();
        let second = zero_seeded.next_u32();
        assert_ne!(first, second);
        assert_ne!(first, 0);
        assert_eq!(Rng::new(7).next_u32(), Rng::new(7).next_u32());
        // The stream is not constant, and successive floats differ.
        let mut rng = Rng::new(9);
        let mut previous = rng.next_f32();
        let mut distinct = 0;
        for _ in 0..1000 {
            let value = rng.next_f32();
            if value != previous {
                distinct += 1;
            }
            previous = value;
        }
        assert!(
            distinct > 900,
            "the stream is degenerate: {distinct} changes"
        );
    }

    #[test]
    fn sample_top_k_with_temperature_zero_is_argmax() {
        let logits = [0.25f32, 3.0, 3.0, -4.0];
        let mut rng = Rng::new(1);
        assert_eq!(sample_top_k(&mut logits.clone(), 0, 0.0, &mut rng), Some(1));
        assert_eq!(sample_top_k(&mut logits.clone(), 2, 0.0, &mut rng), Some(1));
        assert_eq!(
            sample_top_k(&mut logits.clone(), 0, -1.0, &mut rng),
            Some(1)
        );
        assert_eq!(
            sample_top_k(&mut logits.clone(), 0, f32::NAN, &mut rng),
            Some(1)
        );
        assert_eq!(sample_top_k(&mut [], 0, 0.0, &mut rng), None);
    }

    #[test]
    fn sample_top_k_with_k_one_always_returns_the_maximum_index() {
        let logits = [-1.0f32, 0.5, 4.0, 4.0, -3.0];
        let mut rng = Rng::new(0xABCD);
        for _ in 0..200 {
            let mut working = logits;
            assert_eq!(sample_top_k(&mut working, 1, 1.0, &mut rng), Some(2));
        }
        // With a unique maximum the single candidate is that index.
        let unique = [-1.0f32, 0.5, 4.0, -3.0];
        for _ in 0..200 {
            let mut working = unique;
            assert_eq!(sample_top_k(&mut working, 1, 0.7, &mut rng), Some(2));
        }
    }

    #[test]
    fn sample_top_k_is_reproducible_and_restricted_to_the_candidates() {
        let logits = [0.5f32, 2.0, -1.0, 1.25, 0.0];
        let top_three = [0usize, 1, 3];
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        let mut drawn = Vec::new();
        for _ in 0..64 {
            let mut working = logits;
            drawn.push(sample_top_k(&mut working, 3, 1.0, &mut a).expect("non-empty"));
            let mut mirror = logits;
            assert_eq!(
                sample_top_k(&mut mirror, 3, 1.0, &mut b),
                drawn.last().copied()
            );
        }
        for index in &drawn {
            assert!(top_three.contains(index), "drew {index} outside the top-3");
        }
        assert!(
            drawn.iter().any(|i| *i != 1),
            "k = 3 must not collapse onto one token"
        );

        // Once restricted, the buffer holds the sampling distribution.
        let mut working = logits;
        sample_top_k(&mut working, 2, 1.0, &mut a).expect("non-empty");
        let sum: f32 = working.iter().sum();
        assert!(working[2] == 0.0 && working[4] == 0.0);
        assert!((sum - 1.0).abs() < 1e-6, "sum = {sum}");
    }

    #[test]
    fn sample_top_k_with_k_zero_uses_every_logit() {
        let logits = [0.0f32, 1.0, 2.0];
        let mut rng = Rng::new(11);
        let mut seen = [false; 3];
        for _ in 0..256 {
            let mut working = logits;
            let index = sample_top_k(&mut working, 0, 100.0, &mut rng).expect("non-empty");
            seen[index] = true;
        }
        // A high temperature flattens the distribution but never selects out of range.
        assert!(seen[2], "the most likely token should appear at some point");
    }
}
