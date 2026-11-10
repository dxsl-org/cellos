// SPDX-License-Identifier: MPL-2.0
//! ARM NEON SIMD kernels for tensor-math (Phase 08, Spec 24 §4).
//!
//! Compiled only where NEON is a static target feature: the `-softfloat` Cell targets have no
//! FP/SIMD ABI and the kernel does not preserve Q registers across Cell context switches, so
//! those builds keep the scalar path.
//!
//! [`dot_f32_neon`] reproduces the scalar four-lane order bit for bit, because `matvec` and
//! `matvec_q8_0` promise identical results; [`dot_q8_0_block_neon`] is exact integer math.

#![allow(unsafe_code)]

use core::arch::aarch64::*;

/// f32 inner product in the same four-lane order as the scalar `accumulate_products` +
/// `reduce_lanes`; zips the operands, so unequal lengths read only the common prefix.
#[inline]
pub(crate) fn dot_f32_neon(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    let a_chunks = a.chunks_exact(4);
    let b_chunks = b.chunks_exact(4);
    let (a_rem, b_rem) = (a_chunks.remainder(), b_chunks.remainder());
    let mut lanes = [0.0f32; 4];
    // SAFETY: every load reads exactly the four f32 of a `chunks_exact(4)` chunk, and the store
    // writes the four f32 of `lanes`; NEON is a static target feature of this build.
    unsafe {
        let mut acc = vdupq_n_f32(0.0);
        for (left, right) in a_chunks.zip(b_chunks) {
            // Separate multiply and add: a fused MLA would round differently from the scalar path.
            acc = vaddq_f32(
                acc,
                vmulq_f32(vld1q_f32(left.as_ptr()), vld1q_f32(right.as_ptr())),
            );
        }
        vst1q_f32(lanes.as_mut_ptr(), acc);
    }
    for (left, right) in a_rem.iter().zip(b_rem) {
        lanes[0] += left * right;
    }
    (lanes[0] + lanes[1]) + (lanes[2] + lanes[3])
}

/// Integer dot product of 32 i8 weights and 32 i8 activations (exact, like the scalar loop).
///
/// Uses `vmull_s8` widening multiply and `vpadalq_s16` pairwise accumulate, available on every
/// ARMv8.0 core; each i8·i8 product is at most 128² = 16384, so it fits the i16 lanes.
#[inline]
pub(crate) fn dot_q8_0_block_neon(w: &[u8; 32], x: &[u8; 32]) -> i32 {
    let w_ptr = w.as_ptr().cast::<i8>();
    let x_ptr = x.as_ptr().cast::<i8>();
    // SAFETY: the four 16-byte loads cover exactly the two 32-byte arrays; NEON is a static
    // target feature of this build.
    unsafe {
        let w0 = vld1q_s8(w_ptr);
        let x0 = vld1q_s8(x_ptr);
        let w1 = vld1q_s8(w_ptr.add(16));
        let x1 = vld1q_s8(x_ptr.add(16));

        let mut acc = vdupq_n_s32(0);
        acc = vpadalq_s16(acc, vmull_s8(vget_low_s8(w0), vget_low_s8(x0)));
        acc = vpadalq_s16(acc, vmull_s8(vget_high_s8(w0), vget_high_s8(x0)));
        acc = vpadalq_s16(acc, vmull_s8(vget_low_s8(w1), vget_low_s8(x1)));
        acc = vpadalq_s16(acc, vmull_s8(vget_high_s8(w1), vget_high_s8(x1)));
        vaddvq_s32(acc)
    }
}
