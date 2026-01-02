use alloc::string::String;
use alloc::collections::BTreeMap;
use crate::sync::Spinlock;
use crate::prelude::*;

pub mod virtio_hal;
pub mod virtio_blk;
pub mod virtio_gpu;
pub mod console_drv;

pub struct DriverRegistry {
    /// Map Name -> Driver ID
    name_to_id: BTreeMap<String, usize>,
    next_id: usize,
}

impl Default for DriverRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl DriverRegistry {
    pub fn new() -> Self {
        Self {
            name_to_id: BTreeMap::new(),
            // ID 0 is reserved for Kernel/System
            // ID 1 is reserved for Console (for now, or we register it properly)
            next_id: 2, 
        }
    }

    pub fn register(&mut self, name: &str) -> usize {
        if let Some(&id) = self.name_to_id.get(name) {
            return id;
        }
        
        let id = self.next_id;
        self.name_to_id.insert(String::from(name), id);
        self.next_id += 1;
        id
    }

    pub fn get_id(&self, name: &str) -> Option<usize> {
        self.name_to_id.get(name).copied()
    }
}

pub static DRIVER_REGISTRY: Spinlock<Option<DriverRegistry>> = Spinlock::new(None);

pub fn init() {
    let mut reg = DRIVER_REGISTRY.lock();
    *reg = Some(DriverRegistry::new());
    
    // Manually register Console as ID 1 for now, to keep compatibility
    if let Some(r) = reg.as_mut() {
        r.name_to_id.insert(String::from("console"), 1);
    }

    // Init Console Driver
    console_drv::init();

    // Init VirtIO
    virtio_blk::init_driver();
    virtio_gpu::init_driver();
}

pub fn resolve(name: &str) -> Option<usize> {
    if let Some(reg) = DRIVER_REGISTRY.lock().as_ref() {
        reg.get_id(name)
    } else {
        None
    }
}

pub fn register_driver(name: &str) -> usize {
    if let Some(reg) = DRIVER_REGISTRY.lock().as_mut() {
        reg.register(name)
    } else {
        0
    }
}
