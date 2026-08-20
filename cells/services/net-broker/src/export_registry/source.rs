extern crate alloc;

use super::{ExportRegistry, RemoteExports};
use alloc::vec::Vec;

pub(crate) const EXPORT_REGISTRY_MAX_BYTES: usize = 4 * 1024;
const EXPORT_REGISTRY_DIR: &str = "/etc/cellos";
const EXPORT_REGISTRY_BASENAME: &[u8] = b"c2c-exports.cfg";
const EXPORT_REGISTRY_PATH: &str = "/etc/cellos/c2c-exports.cfg";

pub(crate) trait RegistrySource {
    fn list_dir(&mut self, path: &str) -> Result<Vec<u8>, ()>;
    fn stat(&mut self, path: &str) -> Result<(u64, bool), ()>;
    fn read_file_bounded(&mut self, path: &str, max_bytes: usize) -> Result<Vec<u8>, ()>;
}

pub(crate) fn load_remote_exports<S: RegistrySource>(source: &mut S) -> RemoteExports {
    let listing = match source.list_dir(EXPORT_REGISTRY_DIR) {
        Ok(listing) => listing,
        Err(()) => return RemoteExports::invalid(),
    };
    if !listing_has_registry_entry(&listing) {
        return RemoteExports::absent();
    }
    let (size, is_dir) = match source.stat(EXPORT_REGISTRY_PATH) {
        Ok(meta) => meta,
        Err(()) => return RemoteExports::invalid(),
    };
    if is_dir || size > EXPORT_REGISTRY_MAX_BYTES as u64 {
        return RemoteExports::invalid();
    }
    let data = match source.read_file_bounded(EXPORT_REGISTRY_PATH, EXPORT_REGISTRY_MAX_BYTES) {
        Ok(data) => data,
        Err(()) => return RemoteExports::invalid(),
    };
    let registry = match ExportRegistry::parse_bytes(&data) {
        Ok(registry) => registry,
        Err(_) => return RemoteExports::invalid(),
    };
    RemoteExports::from_registry(registry)
}

fn listing_has_registry_entry(listing: &[u8]) -> bool {
    listing
        .split(|&b| b == b'\n')
        .any(|entry| entry == EXPORT_REGISTRY_BASENAME)
}
