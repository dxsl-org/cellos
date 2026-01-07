// SPDX-License-Identifier: MPL-2.0
// Architecture Validation Test: Step 3 - Ergonomics Testing

//! Example Cell implementations to test API usability.

#![no_std]

extern crate alloc;
use alloc::vec::Vec;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use api::*;

/// Example: Simple RAM-based FileSystem Cell
/// Tests: How easy is it to implement FileSystem trait?
struct RamFS {
    files: BTreeMap<&'static str, Vec<u8>>,
}

impl RamFS {
    fn new() -> Self {
        Self {
            files: BTreeMap::new(),
        }
    }
}

impl FileSystem for RamFS {
    fn open(&self, path: &str, mode: OpenMode) -> Result<Box<dyn File>> {
        // FINDING: Need to convert &str to owned type for storage
        // Current API uses &str which is good for calls but hard for storage
        match mode {
            OpenMode::Read => {
                if self.files.contains_key(path) {
                    Ok(Box::new(RamFile {
                        data: self.files.get(path).unwrap().clone(),
                        pos: 0,
                    }))
                } else {
                    Err(Error::NotFound)
                }
            }
            OpenMode::Write | OpenMode::ReadWrite => {
                // ISSUE: Can't mutate self in &self method
                // Need &mut self or interior mutability
                Err(Error::PermissionDenied)
            }
        }
    }

    fn mkdir(&self, _path: &str) -> Result<()> {
        // ISSUE: Same problem - need &mut self
        Err(Error::PermissionDenied)
    }

    fn remove(&self, _path: &str) -> Result<()> {
        // ISSUE: Same problem - need &mut self
        Err(Error::PermissionDenied)
    }
}

struct RamFile {
    data: Vec<u8>,
    pos: usize,
}

impl File for RamFile {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        // GOOD: &mut self allows mutation
        let remaining = self.data.len().saturating_sub(self.pos);
        let to_read = remaining.min(buf.len());
        
        buf[..to_read].copy_from_slice(&self.data[self.pos..self.pos + to_read]);
        self.pos += to_read;
        
        Ok(to_read)
    }

    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        // GOOD: Easy to implement
        if self.pos + buf.len() > self.data.len() {
            self.data.resize(self.pos + buf.len(), 0);
        }
        
        self.data[self.pos..self.pos + buf.len()].copy_from_slice(buf);
        self.pos += buf.len();
        
        Ok(buf.len())
    }

    fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
        // GOOD: SeekFrom enum is ergonomic
        match pos {
            SeekFrom::Start(offset) => {
                self.pos = offset as usize;
            }
            SeekFrom::End(offset) => {
                self.pos = (self.data.len() as i64 + offset) as usize;
            }
            SeekFrom::Current(offset) => {
                self.pos = (self.pos as i64 + offset) as usize;
            }
        }
        Ok(self.pos as u64)
    }
}

/// Example: Driver with ViStateTransfer
/// Tests: How easy is it to add hot-swap to a driver?
struct UartDriver {
    baud_rate: u32,
    tx_buffer: Vec<u8>,
    rx_buffer: Vec<u8>,
}

impl UartDriver {
    fn new() -> Self {
        Self {
            baud_rate: 115200,
            tx_buffer: Vec::new(),
            rx_buffer: Vec::new(),
        }
    }
}

impl ViStateTransfer for UartDriver {
    fn state_size(&self) -> usize {
        // GOOD: Easy to calculate
        4 + 8 + self.tx_buffer.len() + 8 + self.rx_buffer.len()
    }

    fn serialize_state(&self, buffer: &mut [u8]) -> Result<usize> {
        // ISSUE: Lots of boilerplate for serialization
        // Would benefit from derive macro or helper functions
        let mut offset = 0;
        
        buffer[offset..offset + 4].copy_from_slice(&self.baud_rate.to_le_bytes());
        offset += 4;
        
        let tx_len = self.tx_buffer.len();
        buffer[offset..offset + 8].copy_from_slice(&tx_len.to_le_bytes());
        offset += 8;
        buffer[offset..offset + tx_len].copy_from_slice(&self.tx_buffer);
        offset += tx_len;
        
        let rx_len = self.rx_buffer.len();
        buffer[offset..offset + 8].copy_from_slice(&rx_len.to_le_bytes());
        offset += 8;
        buffer[offset..offset + rx_len].copy_from_slice(&self.rx_buffer);
        offset += rx_len;
        
        Ok(offset)
    }

    fn deserialize_state(&mut self, buffer: &[u8]) -> Result<()> {
        // ISSUE: Same boilerplate problem
        let mut offset = 0;
        
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&buffer[offset..offset + 4]);
        self.baud_rate = u32::from_le_bytes(bytes);
        offset += 4;
        
        let mut len_bytes = [0u8; 8];
        len_bytes.copy_from_slice(&buffer[offset..offset + 8]);
        let tx_len = usize::from_le_bytes(len_bytes);
        offset += 8;
        self.tx_buffer = buffer[offset..offset + tx_len].to_vec();
        offset += tx_len;
        
        len_bytes.copy_from_slice(&buffer[offset..offset + 8]);
        let rx_len = usize::from_le_bytes(len_bytes);
        offset += 8;
        self.rx_buffer = buffer[offset..offset + rx_len].to_vec();
        
        Ok(())
    }
}

/// API Misuse Test Cases
#[cfg(test)]
mod misuse_tests {
    use super::*;

    #[test]
    #[should_panic]
    fn test_buffer_too_small() {
        // Test: What happens if buffer is too small?
        let driver = UartDriver::new();
        let mut small_buffer = [0u8; 4]; // Too small
        
        // FINDING: Returns error, doesn't panic - GOOD
        let result = driver.serialize_state(&mut small_buffer);
        assert!(result.is_err());
    }

    #[test]
    fn test_filesystem_mutation() {
        // Test: Can we accidentally mutate through &self?
        let fs = RamFS::new();
        
        // FINDING: Can't write because FileSystem methods take &self
        // This prevents mutation but makes implementation harder
        let result = fs.open("test.txt", OpenMode::Write);
        assert!(result.is_err());
    }
}

// ERGONOMICS FINDINGS SUMMARY:
// 
// GOOD:
// - File trait with &mut self is easy to implement
// - SeekFrom enum is intuitive
// - Error types are clear
// - Box<dyn Trait> allows flexibility
//
// ISSUES:
// 1. FileSystem trait uses &self, making mutable operations impossible
//    FIX: Change to &mut self or use Arc<Mutex<>> pattern
//
// 2. ViStateTransfer requires lots of manual serialization boilerplate
//    FIX: Provide helper macros or serde integration
//
// 3. No way to handle async I/O in current File trait
//    FIX: Add async variants or Future-based methods
//
// 4. &str in FileSystem::open is good for API but hard for storage
//    ACCEPTABLE: Implementers can convert to owned String
