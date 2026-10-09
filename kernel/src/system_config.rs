// SPDX-License-Identifier: MPL-2.0
//! Apply system.toml once, directly from the embedded VIFS1 after fs::init().
//! Hardware discovery, boot memory-map construction, UART setup, and allocator
//! initialization have already happened: this config cannot change those earlier
//! settings. Heap configuration lowers future cell defaults, never the ceiling.

use alloc::vec::Vec;
use api::fs::OpenMode;
use cellos_boot_config::{parse_system, LogLevel, SystemConfig, MAX_CONFIG_BYTES, SYSTEM_PATH};
use core::sync::atomic::{AtomicBool, Ordering};
use types::ViError;

static LOGGING_EXPLICIT: AtomicBool = AtomicBool::new(false);

pub fn logging_explicit() -> bool {
    LOGGING_EXPLICIT.load(Ordering::Acquire)
}

/// Missing is the only fallback. A present empty, oversized, truncated, unreadable,
/// or malformed file halts before init (and any cap-bearing cell) can be spawned.
pub fn load_from_vifs1() {
    let config = match read_system() {
        Ok(Some(bytes)) => parse_system(&bytes)
            .unwrap_or_else(|error| panic!("[system-config] {}: {}", SYSTEM_PATH, error)),
        Ok(None) => {
            log::info!("[system-config] {} absent; using legacy defaults (info, 16 MiB; later quiet-mode defaults retained)", SYSTEM_PATH);
            SystemConfig::default()
        }
        Err(error) => panic!("[system-config] {}: {}", SYSTEM_PATH, error),
    };
    crate::memory::cell_quota::set_default_quota_bytes(config.memory.default_cell_heap_mib * 1024 * 1024);
    LOGGING_EXPLICIT.store(config.logging_explicit, Ordering::Release);
    log::set_max_level(match config.logging.level {
        LogLevel::Off => log::LevelFilter::Off,
        LogLevel::Error => log::LevelFilter::Error,
        LogLevel::Warn => log::LevelFilter::Warn,
        LogLevel::Info => log::LevelFilter::Info,
        LogLevel::Debug => log::LevelFilter::Debug,
        LogLevel::Trace => log::LevelFilter::Trace,
    });
    log::info!("[system-config] applied: logging={:?}, default_cell_heap_mib={}", config.logging.level, config.memory.default_cell_heap_mib);
}

/// Do not use fs::read_file_from_vifs1: that legacy ELF reader treats empty as
/// missing, allocates the unbounded declared size, and accepts truncated files.
fn read_system() -> Result<Option<Vec<u8>>, &'static str> {
    let (mut file, size) = {
        let guard = crate::fs::VIFS1.lock();
        let fs = guard.as_ref().ok_or("embedded filesystem is not mounted")?;
        // stat preserves FAT IO errors; open's legacy implementation collapses
        // them into NotFound. Only stat's genuine NotFound permits defaults.
        let stat = match fs.stat(SYSTEM_PATH) {
            Ok(stat) => stat,
            Err(ViError::NotFound) => return Ok(None),
            Err(_) => return Err("cannot stat embedded configuration"),
        };
        if !stat.exists || stat.is_dir { return Err("configuration is not a regular file"); }
        if stat.size == 0 { return Err("configuration is empty"); }
        if stat.size > MAX_CONFIG_BYTES as u64 { return Err("configuration exceeds 16384 bytes"); }
        let size = usize::try_from(stat.size).map_err(|_| "configuration size is not representable")?;
        let file = fs.open(SYSTEM_PATH, OpenMode::Read).map_err(|_| "cannot open embedded configuration")?;
        (file, size)
    };
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(size).map_err(|_| "cannot allocate configuration buffer")?;
    bytes.resize(size, 0);
    let mut offset = 0;
    while offset < size {
        let end = (offset + 4096).min(size);
        let count = file.read(&mut bytes[offset..end]).map_err(|_| "cannot read embedded configuration")?;
        if count == 0 || count > end - offset { return Err("configuration is truncated or inconsistent"); }
        offset += count;
    }
    Ok(Some(bytes))
}
