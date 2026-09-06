//! Memory footprint measurement — allocator-backed, not time-based.
//!
//! Reports physical frames committed by the kernel frame allocator. This
//! includes the reserved kernel heap and is not a resident-set measurement.

use api::benchmark::ViBenchmark;

pub struct MemoryFootprintBench {
    measured_bytes: u64,
}

impl MemoryFootprintBench {
    pub fn new() -> Self {
        Self { measured_bytes: 0 }
    }

    /// Return the last measurement in bytes (valid after `run_once`).
    pub fn bytes(&self) -> u64 {
        self.measured_bytes
    }
}

impl ViBenchmark for MemoryFootprintBench {
    fn name(&self) -> &'static str {
        "memory_footprint"
    }

    fn run_once(&mut self) -> api::ViResult<u64> {
        let info = ostd::syscall::sys_mem_info().map_err(|_| api::ViError::IO)?;
        if info.total_frames != info.used_frames.saturating_add(info.free_frames) {
            return Err(api::ViError::InvalidInput);
        }
        self.measured_bytes = info
            .used_frames
            .checked_mul(info.page_size)
            .ok_or(api::ViError::InvalidInput)?;
        Ok(self.measured_bytes)
    }
}

impl Default for MemoryFootprintBench {
    fn default() -> Self {
        Self::new()
    }
}
