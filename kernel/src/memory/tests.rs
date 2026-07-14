//! Memory allocator tests — called at boot-time by the kernel test runner.
//!
//! All functions are `pub` so the test runner in `kernel/src/main.rs` can
//! invoke them via `memory::tests::run_all()`.  They use `log::info!` for
//! output and `assert!` / `assert_eq!` for correctness.

#![allow(dead_code)]

use alloc::boxed::Box;
use alloc::vec::Vec;

/// Run every memory test and log a summary.
pub fn run_all() {
    log::info!("=== Memory Tests ===");
    test_box_allocation();
    test_large_vec_allocation();
    test_sequential_alloc_free();
    test_stress_10k_alloc_free();
    test_multiple_sizes();
    test_vec_push_pop();
    test_nested_box();
    log::info!("=== Memory Tests PASSED ===");
}

// ─── Basic heap ───────────────────────────────────────────────────────────────

fn test_box_allocation() {
    let v = Box::new(42u32);
    assert_eq!(*v, 42, "Box<u32> value mismatch");
    log::info!("  [ok] Box<u32>");
}

fn test_large_vec_allocation() {
    const N: usize = 1_000;
    let mut v: Vec<usize> = Vec::with_capacity(N);
    for i in 0..N {
        v.push(i);
    }
    assert_eq!(v.len(), N);
    assert_eq!(v[500], 500);
    log::info!("  [ok] Vec<usize> len={}", N);
}

// ─── Stress: 10K alloc/free cycles ───────────────────────────────────────────

/// Allocate 10K `Box<u64>` values sequentially and verify each is readable after
/// the previous one is dropped.  This exercises the bump/slab allocator's free-
/// list path and checks that no corruption occurs under repeated small allocations.
fn test_stress_10k_alloc_free() {
    const ITERS: usize = 10_000;
    for i in 0..ITERS {
        let b = Box::new(i as u64);
        // Verify the value survived the allocation and any allocator bookkeeping.
        assert_eq!(*b, i as u64, "stress iter {} corrupted", i);
        // `b` is dropped here — returns memory to the allocator.
    }
    log::info!("  [ok] 10K Box<u64> stress (alloc+drop each iter)");
}

/// Allocate 100 elements, keep them alive simultaneously, then free in
/// reverse order to stress the free-list path.
fn test_sequential_alloc_free() {
    const N: usize = 100;
    let mut boxes: Vec<Box<usize>> = Vec::with_capacity(N);
    for i in 0..N {
        boxes.push(Box::new(i));
    }
    // Verify all values intact while they are all live.
    for (i, b) in boxes.iter().enumerate() {
        assert_eq!(**b, i, "live-batch value[{}] mismatch", i);
    }
    // Drop in reverse order.
    while let Some(b) = boxes.pop() {
        let _ = b; // dropped here
    }
    log::info!("  [ok] sequential alloc then reverse-free ({} items)", N);
}

// ─── Various sizes ────────────────────────────────────────────────────────────

/// Allocate objects of several different sizes to exercise size-class paths in
/// the allocator (if present).
fn test_multiple_sizes() {
    // Small (< 16 bytes)
    let a = Box::new(1u8);
    assert_eq!(*a, 1);

    // Medium (64 bytes)
    let b = Box::new([0u8; 64]);
    assert_eq!(b[0], 0);
    assert_eq!(b[63], 0);

    // Larger (4 KB)
    let c = Box::new([0u8; 4096]);
    assert_eq!(c[0], 0);
    assert_eq!(c[4095], 0);

    // Very large (64 KB)
    let d = Box::new([0u8; 65536]);
    assert_eq!(d[0], 0);
    assert_eq!(d[65535], 0);

    log::info!("  [ok] multi-size allocations (1B, 64B, 4KB, 64KB)");
}

// ─── Vec push/pop stress ─────────────────────────────────────────────────────

/// Push 5 000 u32s and pop them, verifying LIFO order.  This stresses
/// reallocation-on-growth (Vec::push may call alloc when capacity is exceeded).
fn test_vec_push_pop() {
    const N: usize = 5_000;
    let mut v: Vec<u32> = Vec::new();
    for i in 0..N {
        v.push(i as u32);
    }
    for i in (0..N).rev() {
        assert_eq!(v.pop(), Some(i as u32), "pop order mismatch at {}", i);
    }
    assert!(v.is_empty());
    log::info!("  [ok] Vec<u32> push+pop {} items (LIFO order)", N);
}

// ─── Nested Box ──────────────────────────────────────────────────────────────

/// Allocate a Box pointing to another Box.  Tests that the allocator handles
/// pointer-sized objects and that the heap can survive nested allocations.
fn test_nested_box() {
    let inner: Box<u64> = Box::new(0xDEAD_BEEF);
    let outer: Box<Box<u64>> = Box::new(inner);
    assert_eq!(**outer, 0xDEAD_BEEF);
    log::info!("  [ok] nested Box<Box<u64>>");
}
