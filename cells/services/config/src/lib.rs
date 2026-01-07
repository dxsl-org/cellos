#![no_std]

//! Configuration and Key-Value Store interfaces.

use ostd::prelude::*;
use core::option::Option;
use core::marker::{Send, Sync};

/// Key-Value Store interface.
pub trait ConfigStore: Send + Sync {
    /// Get a configuration value.
    fn get(&self, key: &str) -> Option<&[u8]>;
    
    /// Set a configuration value.
    fn set(&mut self, key: &str, value: &[u8]) -> Result<()>;
    
    /// Remove a configuration value.
    fn remove(&mut self, key: &str) -> Result<()>;
    
    /// Persist changes to storage.
    fn flush(&mut self) -> Result<()>;
}

/// Configuration key-value pair.
pub struct ConfigEntry {
    /// Key.
    pub key: alloc::string::String,
    /// Value.
    pub value: alloc::vec::Vec<u8>,
}

extern crate alloc;
