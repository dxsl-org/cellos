use log::info;

use alloc::collections::VecDeque;
use crate::prelude::*;

/// Physical Page Size (4KiB)
pub const PAGE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Frame {
    pub number: usize,
}

impl Frame {
    pub fn start_address(&self) -> usize {
        self.number * PAGE_SIZE
    }
}

pub struct FrameAllocator {
    recycled: VecDeque<Frame>,
    next: usize,
}

impl Default for FrameAllocator {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameAllocator {
    pub fn new() -> Self {
        Self {
            recycled: VecDeque::new(),
            next: 1, // Start at frame 1 (skip 0/NULL)
        }
    }

    pub fn allocate(&mut self) -> Option<Frame> {
        if let Some(frame) = self.recycled.pop_front() {
            Some(frame)
        } else {
            let frame = Frame { number: self.next };
            self.next += 1;
            // In a real OS, checking against max RAM is needed here.
            Some(frame)
        }
    }

    pub fn deallocate(&mut self, frame: Frame) {
        self.recycled.push_back(frame);
    }
}

// Global (Simulated) Allocator
static mut FRAME_ALLOCATOR: Option<FrameAllocator> = None;

pub fn init() {
    unsafe {
        FRAME_ALLOCATOR = Some(FrameAllocator::new());
    }
    info!("Memory: Frame Allocator Initialized.");
}

pub fn alloc_frame() -> Option<Frame> {
    unsafe {
        FRAME_ALLOCATOR.as_mut().unwrap().allocate()
    }
}

pub fn dealloc_frame(frame: Frame) {
    unsafe {
        FRAME_ALLOCATOR.as_mut().unwrap().deallocate(frame)
    }
}
