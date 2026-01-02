use super::frame::{Frame, PAGE_SIZE};
use alloc::vec::Vec;
use crate::prelude::*;


#[derive(Clone, Copy, Debug)]
pub struct Page {
    pub number: usize,
}

impl Page {
    pub fn starting_address(&self) -> usize {
        self.number * PAGE_SIZE
    }
}

pub struct PageTable {
    // In simulation, we just keep a list of mappings.
    // In real hardware, this would be a multi-level Radix Tree (L4/L3/L2/L1).
    mappings: Vec<(Page, Frame, PageFlags)>,
}

#[derive(Clone, Copy, Debug)]
pub enum PageFlags {
    Read     = 1 << 0,
    Write    = 1 << 1,
    Execute  = 1 << 2,
    User     = 1 << 3,
}

impl Default for PageTable {
    fn default() -> Self {
        Self::new()
    }
}

impl PageTable {
    pub fn new() -> Self {
        Self {
            mappings: Vec::new(),
        }
    }

    pub fn map(&mut self, page: Page, frame: Frame, flags: PageFlags) {
        // info!("Paging: Map Virt {:X} -> Phys {:X} ({:?})", page.starting_address(), frame.start_address(), flags);
        self.mappings.push((page, frame, flags));
    }

    pub fn translate(&self, addr: usize) -> Option<usize> {
        let page_num = addr / PAGE_SIZE;
        let offset = addr % PAGE_SIZE;

        for (p, f, _) in &self.mappings {
            if p.number == page_num {
                return Some(f.start_address() + offset);
            }
        }
        None
    }
}
