extern crate alloc;

use alloc::vec::Vec;
use app_wasm::{finalize_wasm_read, LoadWasmError, MAX_WASM_MODULE_BYTES};
use ostd::clients::VfsClient;
use ostd::ViResult;

pub(crate) fn load_wasm_bytes(path: &str) -> Result<Vec<u8>, LoadWasmError> {
    load_wasm_bytes_with(path, |wasm_path, max_bytes| {
        let mut vfs = VfsClient::new();
        vfs.read_file_bounded(wasm_path, max_bytes)
    })
}

fn load_wasm_bytes_with<F>(path: &str, read_file: F) -> Result<Vec<u8>, LoadWasmError>
where
    F: FnOnce(&str, usize) -> ViResult<Vec<u8>>,
{
    finalize_wasm_read(read_file(path, MAX_WASM_MODULE_BYTES))
}
