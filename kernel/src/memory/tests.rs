//! Integration Tests for Memory Allocator
//!
//! Intended to be run manually.

#![allow(dead_code)]

use alloc::boxed::Box;
use alloc::vec::Vec;

/// Basic Heap Test
pub fn run_heap_tests() {
    log::info!("Running Heap Tests...");
    test_box_allocation();
    test_large_vec_allocation();
    log::info!("Heap Tests Passed!");
}

fn test_box_allocation() {
    let heap_val = Box::new(41);
    assert_eq!(*heap_val, 41);
    log::info!("Box<u32> OK");
}

fn test_large_vec_allocation() {
    let n = 1000;
    let mut vec = Vec::new();
    for i in 0..n {
        vec.push(i);
    }
    assert_eq!(vec.len(), n);
    assert_eq!(vec[500], 500);
    log::info!("Vec<usize> (len={}) OK", n);
}
