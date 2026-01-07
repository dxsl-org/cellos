//! Filesystem Subsystem

pub mod fat;

use alloc::sync::Arc;
use crate::sync::Spinlock;
use api::fs::ViFileSystem;

/// Global Filesystem Instance (viFS1)
pub static VIFS1: Spinlock<Option<Arc<dyn ViFileSystem>>> = Spinlock::new(None);

pub fn init() {
    log::info!("Filesystem: Initializing...");
    
    // Attempt to mount FAT32 from VirtIO Block
    match fat::ViFatFS::new() {
        Ok(fs) => {
            log::info!("Filesystem: FAT32 Mounted Successfully.");
            *VIFS1.lock() = Some(Arc::new(fs));
        }
        Err(e) => {
            log::error!("Filesystem: Failed to mount FAT32: {:?}", e);
        }
    }
}
