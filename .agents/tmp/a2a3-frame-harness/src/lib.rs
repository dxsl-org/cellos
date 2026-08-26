#![allow(dead_code)]

extern crate alloc;

pub type PhysAddr = usize;

pub mod boot {
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub enum MemoryType {
        Usable,
        Reserved,
    }

    #[derive(Clone, Copy)]
    pub struct MemoryMapEntry {
        pub base: usize,
        pub length: usize,
        pub ty: MemoryType,
    }
}

pub mod sync {
    pub type Spinlock<T> = spin::Mutex<T>;
}

#[path = "../../../../kernel/src/memory/frame.rs"]
mod frame;
