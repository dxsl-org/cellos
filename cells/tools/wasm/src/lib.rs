#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use types::ViError;

/// Keep the host-side loader on the existing bounded VFS path and cap modules
/// to the same 64 KiB ceiling used by other direct file readers.
pub const MAX_WASM_MODULE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadWasmError {
    TooLarge,
    Unavailable,
}

pub fn finalize_wasm_read(read_result: Result<Vec<u8>, ViError>) -> Result<Vec<u8>, LoadWasmError> {
    match read_result {
        Ok(bytes) if bytes.is_empty() => Err(LoadWasmError::Unavailable),
        Ok(bytes) => Ok(bytes),
        Err(err) => Err(map_read_error(err)),
    }
}

fn map_read_error(err: ViError) -> LoadWasmError {
    match err {
        ViError::OutOfMemory => LoadWasmError::TooLarge,
        _ => LoadWasmError::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn loads_wasm_bytes_on_success() {
        let result = finalize_wasm_read(Ok(vec![0x00, 0x61, 0x73, 0x6d]));
        assert_eq!(result, Ok(vec![0x00, 0x61, 0x73, 0x6d]));
    }

    #[test]
    fn maps_missing_file_to_unavailable_error() {
        let result = finalize_wasm_read(Err(ViError::IO));
        assert_eq!(result, Err(LoadWasmError::Unavailable));
    }

    #[test]
    fn maps_oversize_file_to_too_large_error() {
        let result = finalize_wasm_read(Err(ViError::OutOfMemory));
        assert_eq!(result, Err(LoadWasmError::TooLarge));
    }
}
