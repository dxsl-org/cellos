// SPDX-License-Identifier: MPL-2.0
//! Cold-boot configuration shared by the kernel and init. This library has no
//! filesystem, logging, hardware, or security-policy side effects.
#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use api::syscall::service;
use serde::Deserialize;

pub const MAX_CONFIG_BYTES: usize = 16_384;
pub const MAX_CELLS: usize = 32;
pub const MAX_NAME_BYTES: usize = 64;
pub const MAX_PATH_BYTES: usize = 256;
pub const MAX_ARGV_BYTES: usize = 512;
/// Must match ostd::args::set_spawn_argv's structured argument framing.
const ARGV_PREFIX_BYTES: usize = b"\0argv1\0".len();
pub const SYSTEM_PATH: &str = "/etc/cellos/system.toml";
pub const SERVICES_PATH: &str = "/etc/cellos/services.toml";
pub const AUTOLOAD_PATH: &str = "/etc/cellos/autoload.toml";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Off,
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LoggingConfig {
    pub level: LogLevel,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MemoryConfig {
    pub default_cell_heap_mib: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self { default_cell_heap_mib: 16 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemConfig {
    pub version: u32,
    pub logging: LoggingConfig,
    pub memory: MemoryConfig,
    /// An omitted [logging] retains the kernel's later legacy quiet-mode policy.
    pub logging_explicit: bool,
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            version: 1,
            logging: LoggingConfig::default(),
            memory: MemoryConfig::default(),
            logging_explicit: false,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SystemDocument {
    version: u32,
    #[serde(default)]
    logging: Option<LoggingConfig>,
    #[serde(default)]
    memory: MemoryConfig,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub enum RestartPolicy {
    #[serde(rename = "always")]
    Always,
    #[serde(rename = "on-failure")]
    OnFailure,
    #[default]
    #[serde(rename = "never")]
    Never,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub enum Registration {
    #[default]
    #[serde(rename = "init")]
    Init,
    #[serde(rename = "self-ready")]
    SelfReady,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellSpec {
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "enabled_default")]
    pub enabled: bool,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub restart: RestartPolicy,
    #[serde(default)]
    pub service_id: Option<u16>,
    #[serde(default)]
    pub registration: Registration,
    #[serde(default)]
    pub after: Vec<String>,
    #[serde(default = "timeout_default")]
    pub ready_timeout_ticks: u64,
}

const fn enabled_default() -> bool { true }
const fn timeout_default() -> u64 { 500 }

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellConfig {
    pub version: u32,
    #[serde(default)]
    pub cells: Vec<CellSpec>,
}

impl Default for CellConfig {
    fn default() -> Self {
        Self { version: 1, cells: Vec::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    Empty,
    TooLarge,
    InvalidUtf8,
    Toml(String),
    UnsupportedVersion(u32),
    HeapOutOfRange,
    TooManyCells,
    InvalidCell { name: String, field: &'static str },
    Duplicate { field: &'static str, name: String },
    Dependency { name: String, dependency: String, reason: &'static str },
    Cycle,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("configuration is empty"),
            Self::TooLarge => write!(f, "configuration exceeds {MAX_CONFIG_BYTES} bytes"),
            Self::InvalidUtf8 => f.write_str("configuration is not UTF-8"),
            Self::Toml(error) => write!(f, "invalid TOML schema: {error}"),
            Self::UnsupportedVersion(version) => write!(f, "unsupported configuration version {version}"),
            Self::HeapOutOfRange => f.write_str("default_cell_heap_mib must be in 1..=16"),
            Self::TooManyCells => write!(f, "configuration exceeds {MAX_CELLS} combined cells"),
            Self::InvalidCell { name, field } => write!(f, "cell {name:?}: invalid {field}"),
            Self::Duplicate { field, name } => write!(f, "cell {name:?}: duplicate {field}"),
            Self::Dependency { name, dependency, reason } => write!(f, "cell {name:?}: dependency {dependency:?} {reason}"),
            Self::Cycle => f.write_str("cell dependency cycle"),
        }
    }
}

fn parse<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ConfigError> {
    if bytes.is_empty() { return Err(ConfigError::Empty); }
    if bytes.len() > MAX_CONFIG_BYTES { return Err(ConfigError::TooLarge); }
    let text = core::str::from_utf8(bytes).map_err(|_| ConfigError::InvalidUtf8)?;
    toml::from_str(text).map_err(|error| ConfigError::Toml(error.to_string()))
}

pub fn parse_system(bytes: &[u8]) -> Result<SystemConfig, ConfigError> {
    let document: SystemDocument = parse(bytes)?;
    check_version(document.version)?;
    if !(1..=16).contains(&document.memory.default_cell_heap_mib) {
        return Err(ConfigError::HeapOutOfRange);
    }
    Ok(SystemConfig {
        version: document.version,
        logging_explicit: document.logging.is_some(),
        logging: document.logging.unwrap_or_default(),
        memory: document.memory,
    })
}

/// Parse one file and validate its local identities and field bounds. Dependency
/// resolution must wait for validate_plan, since autoload may reference services.
pub fn parse_cells(bytes: &[u8]) -> Result<CellConfig, ConfigError> {
    let config: CellConfig = parse(bytes)?;
    validate_local(&config)?;
    Ok(config)
}

fn check_version(version: u32) -> Result<(), ConfigError> {
    if version == 1 { Ok(()) } else { Err(ConfigError::UnsupportedVersion(version)) }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= MAX_NAME_BYTES
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        && name != "." && name != ".."
}

fn invalid(cell: &CellSpec, field: &'static str) -> ConfigError {
    ConfigError::InvalidCell { name: cell.name.clone(), field }
}

fn validate_local(config: &CellConfig) -> Result<(), ConfigError> {
    check_version(config.version)?;
    if config.cells.len() > MAX_CELLS { return Err(ConfigError::TooManyCells); }
    for (index, cell) in config.cells.iter().enumerate() {
        if !valid_name(&cell.name) { return Err(invalid(cell, "name")); }
        let basename = cell.path.strip_prefix("/bin/").unwrap_or("");
        if cell.path.len() > MAX_PATH_BYTES || basename.is_empty()
            || basename == "." || basename == ".."
            || !basename.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err(invalid(cell, "path (expected /bin/<basename>)"));
        }
        const FIXED_NAMES: &[&str] = &["init", "vfs", "block", "nvme", "ahci"];
        const FIXED_PATHS: &[&str] = &[
            "/bin/init", "/bin/vfs", "/bin/block", "/bin/nvme", "/bin/ahci",
            "/bin/ocel-js", "/bin/ocel-pdf",
        ];
        if FIXED_NAMES.iter().any(|name| cell.name.eq_ignore_ascii_case(name))
            || FIXED_PATHS.iter().any(|path| cell.path.eq_ignore_ascii_case(path))
            || cell.service_id.is_some_and(|id| matches!(id,
                service::VFS | service::BLOCK_DRIVER | service::OCEL_ACTIVATOR
                | service::OCEL_JS | service::OCEL_PDF))
        {
            return Err(invalid(cell, "reserved bootstrap or demand-only identity"));
        }
        if cell.service_id == Some(0) { return Err(invalid(cell, "service_id")); }
        if cell.registration == Registration::SelfReady && cell.service_id.is_none() {
            return Err(invalid(cell, "self-ready registration requires service_id"));
        }
        if !(1..=5000).contains(&cell.ready_timeout_ticks) {
            return Err(invalid(cell, "ready_timeout_ticks"));
        }
        let mut argv_bytes = ARGV_PREFIX_BYTES;
        for arg in &cell.args {
            if arg.as_bytes().contains(&0) { return Err(invalid(cell, "args contain NUL")); }
            argv_bytes = argv_bytes.checked_add(arg.len()).and_then(|n| n.checked_add(1))
                .filter(|n| *n <= MAX_ARGV_BYTES)
                .ok_or_else(|| invalid(cell, "encoded args exceed 512 bytes"))?;
        }
        for dependency in &cell.after {
            if !valid_name(dependency) { return Err(invalid(cell, "after")); }
        }
        for earlier in &config.cells[..index] { check_unique(earlier, cell)?; }
    }
    Ok(())
}

fn check_unique(earlier: &CellSpec, cell: &CellSpec) -> Result<(), ConfigError> {
    let field = if earlier.name == cell.name {
        "name"
    } else if earlier.path.eq_ignore_ascii_case(&cell.path) {
        // BootFS is FAT: case differences cannot create independent binaries.
        "path"
    } else if earlier.service_id.is_some() && earlier.service_id == cell.service_id {
        "service_id"
    } else {
        return Ok(());
    };
    Err(ConfigError::Duplicate { field, name: cell.name.clone() })
}

fn validate_pair(config: &CellConfig, prior: &CellConfig) -> Result<(), ConfigError> {
    validate_local(config)?;
    validate_local(prior)?;
    if config.cells.len() + prior.cells.len() > MAX_CELLS {
        return Err(ConfigError::TooManyCells);
    }
    for cell in &config.cells {
        for earlier in &prior.cells { check_unique(earlier, cell)?; }
    }
    Ok(())
}

fn dependency_error(cell: &CellSpec, dependency: &str, reason: &'static str) -> ConfigError {
    ConfigError::Dependency { name: cell.name.clone(), dependency: dependency.to_string(), reason }
}

fn validate_dependencies(config: &CellConfig, prior: &CellConfig) -> Result<(), ConfigError> {
    for cell in &config.cells {
        for dependency in &cell.after {
            let target = config.cells.iter().chain(&prior.cells).find(|entry| entry.name == *dependency)
                .ok_or_else(|| dependency_error(cell, dependency, "does not exist in this or the earlier stage"))?;
            if !target.enabled {
                return Err(dependency_error(cell, dependency, "is disabled"));
            }
        }
    }
    Ok(())
}

/// Validate the entire plan before either stage launches any configured cell.
pub fn validate_plan(services: &CellConfig, autoload: &CellConfig) -> Result<(), ConfigError> {
    validate_pair(autoload, services)?;
    // A service cannot wait for an autoload entry, including an enabled one.
    validate_dependencies(services, &CellConfig::default())?;
    validate_dependencies(autoload, services)?;
    walk_order(services, |_| {})?;
    walk_order(autoload, |_| {})?;
    Ok(())
}

/// Stable topological order of enabled entries. Prior entries are already ready;
/// references to disabled or absent entries are never treated as satisfied.
pub fn ordered_indices(config: &CellConfig, prior: &CellConfig) -> Result<Vec<usize>, ConfigError> {
    validate_pair(config, prior)?;
    validate_dependencies(config, prior)?;
    let enabled = config.cells.iter().filter(|cell| cell.enabled).count();
    let mut indices = Vec::with_capacity(enabled);
    walk_order(config, |index| indices.push(index))?;
    Ok(indices)
}

// Validation uses a no-op visitor, avoiding allocation of discarded order lists.
fn walk_order(config: &CellConfig, mut visit: impl FnMut(usize)) -> Result<(), ConfigError> {
    let mut emitted = [false; MAX_CELLS];
    let enabled = config.cells.iter().filter(|cell| cell.enabled).count();
    let mut count = 0;
    while count < enabled {
        let before = count;
        for (index, cell) in config.cells.iter().enumerate() {
            if !cell.enabled || emitted[index] { continue; }
            let ready = cell.after.iter().all(|dependency| {
                config.cells.iter().position(|entry| entry.name == *dependency)
                    .map(|target| emitted[target]).unwrap_or(true)
            });
            if ready {
                emitted[index] = true;
                count += 1;
                visit(index);
            }
        }
        if count == before { return Err(ConfigError::Cycle); }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
