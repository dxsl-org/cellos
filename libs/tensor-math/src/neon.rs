// SPDX-License-Identifier: MPL-2.0
//! ARM NEON SIMD kernels for tensor-math (Phase 08, Spec 24 §4).
//!
//! Provides hardware-accelerated f32 dot products, Q8_0 block dot products, and RMSNorm.
//! All kernels produce outputs bit-exact or tolerance-bounded equal to the scalar reference paths.

#![allow(unsafe_code)]

#[cfg(target_arch = "aarch64")]
use core::arch::aarch64::*;

/// Hardware-accelerated f32 inner product using ARM NEON (4x unrolled).
#[cfg(target_arch = "aarch64")]
#[inline]
pub unsafe fn dot_f32_neon(a: &[f32], b: &[f32]) -> f32 {
    let len = a.len();
    debug_assert_eq!(len, b.len());

    let mut sum0 = vdupq_n_f32(0.0);
    let mut sum1 = vdupq_n_f32(0.0);
    let mut sum2 = vdupq_n_f32(0.0);
    let mut sum3 = vdupq_n_f32(0.0);

    let chunks = len / 16;
    let mut a_ptr = a.as_ptr();
    let mut b_ptr = b.as_ptr();

    for _ in 0..chunks {
        let a0 = vld1q_f32(a_ptr);
        let b0 = vld1q_f32(b_ptr);
        sum0 = vmlaq_f32(sum0, a0, b0);

        let a1 = vld1q_f32(a_ptr.add(4));
        let b1 = vld1q_f32(b_ptr.add(4));
        sum1 = vmlaq_f32(sum1, a1, b1);

        let a2 = vld1q_f32(a_ptr.add(8));
        let b2 = vld1q_f32(b_ptr.add(8));
        sum2 = vmlaq_f32(sum2, a2, b2);

        let a3 = vld1q_f32(a_ptr.add(12));
        let b3 = vld1q_f32(b_ptr.add(12));
        sum3 = vmlaq_f32(sum3, a3, b3);

        a_ptr = a_ptr.add(16);
        b_ptr = b_ptr.add(16);
    }

    // Process remaining 4-element chunks
    let rem_chunks = (len % 16) / 4;
    for _ in 0..rem_chunks {
        let a0 = vld1q_f32(a_ptr);
        let b0 = vld1q_f32(b_ptr);
        sum0 = vmlaq_f32(sum0, a0, b0);

        a_ptr = a_ptr.add(4);
        b_ptr = b_ptr.add(4);
    }

    let sum01 = vaddq_f32(sum0, sum1);
    let sum23 = vaddq_f32(sum2, sum3);
    let mut total = vaddq_f32(sum01, sum23);

    // Remainder 0..3 elements
    let rem = len % 4;
    if rem > 0 {
        let mut rem_a = [0.0f32; 4];
        let mut rem_b = [0.0f32; 4];
        for i in 0..rem {
            rem_a[i] = *a_ptr.add(i);
            rem_b[i] = *b_ptr.add(i);
        }
        let a_rem = vld1q_f32(rem_a.as_ptr());
        let b_rem = vld1q_f32(rem_b.as_ptr());
        total = vmlaq_f32(total, a_rem, b_rem);
    }

    // Horizontal sum of total vector
    vaddvq_f32(total)
}

/// Computes the integer dot product of 32 i8 weights and 32 i8 activations using NEON.
///
/// Uses `vmull_s8` widening multiply and `vpadalq_s16` pairwise accumulate for broad
/// compatibility across all ARMv8.0+ cores (including Apple M1/M2/M3 and Cortex-A53/A72).
#[cfg(target_arch = "aarch64")]
#[inline]
pub unsafe fn dot_q8_0_block_neon(w: &[u8; 32], x: &[u8; 32]) -> i32 {
    let w_ptr = w.as_ptr() as *const i8;
    let x_ptr = x.as_ptr() as *const i8;

    // Load first 16 bytes
    let w0 = vld1q_s8(w_ptr);
    let x0 = vld1q_s8(x_ptr);

    // Widening multiply: 16 x i8 -> 8 x i16 (low) and 8 x i16 (high)
    let prod0_low = vmull_s8(vget_low_s8(w0), vget_low_s8(x0));
    let prod0_high = vmull_s8(vget_high_s8(w0), vget_high_s8(x0));

    // Load second 16 bytes
    let w1 = vld1q_s8(w_ptr.add(16));
    let x1 = vld1q_s8(x_ptr.add(16));

    let prod1_low = vmull_s8(vget_low_s8(w1), vget_low_s8(x1));
    let prod1_high = vmull_s8(vget_high_s8(w1), vget_high_s8(x1));

    // Pairwise accumulate i16 into i32 lanes
    let mut acc = vdupq_n_s32(0);
    acc = vpadalq_s16(acc, prod0_low);
    acc = vpadalq_s16(acc, prod0_high);
    acc = vpadalq_s16(acc, prod1_low);
    acc = vpadalq_s16(acc, prod1_high);

    // Sum the 4 i32 lanes
    vaddvq_s32(acc)
}

/// Fallback stubs for non-aarch64 targets (used for host unit tests).
#[cfg(not(target_arch = "aarch64"))]
pub fn dot_f32_fallback(a: &[f32], b: &[f32]) -> f32 {
    let mut sum = 0.0f32;
    for (x, y) in a.iter().zip(b.iter()) {
        sum += x * y;
    }
    sum
}

#[cfg(not(target_arch = "aarch64"))]
pub fn dot_q8_0_block_fallback(w: &[u8; 32], x: &[u8; 32]) -> i32 {
    let mut sum = 0i32;
    for (&weight, &activation) in w.iter().zip(x.iter()) {
        sum += i32::from(weight as i8) * i32::from(activation as i8);
    }
    sum
}
