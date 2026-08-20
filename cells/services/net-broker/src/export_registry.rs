// SPDX-License-Identifier: Apache-2.0
//! C2C export registry — non-secret broker policy input.
//!
//! The registry is boot-provisioned ASCII config. It is not a secret store.
//! Remote/public exports stay disabled until secure node identity exists.

#![allow(dead_code)]

#[cfg(test)]
mod numeric_tests;
#[cfg(test)]
mod parser_tests;
#[cfg(test)]
mod source_tests;
#[cfg(test)]
mod tests;

mod ascii;
mod parser;
mod source;

const MAX_EXPORTS: usize = 16;
const GLOBAL_VERSION_KEY: &[u8] = b"c2c_exports_version";
const EXPORT_PREFIX: &[u8] = b"export_";

pub const EXPORT_REGISTRY_VERSION: u8 = 1;
pub const EXPORT_REGISTRY_PATH: &str = "/etc/cellos/c2c-exports.cfg";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteDisabledReason {
    RegistryAbsent,
    RegistryInvalid,
    NoSecureIdentity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExportScope {
    Local,
    Remote,
    Public,
}

impl ExportScope {
    fn parse(val: &[u8]) -> Option<Self> {
        if eq_ascii(val, b"local") {
            Some(Self::Local)
        } else if eq_ascii(val, b"remote") {
            Some(Self::Remote)
        } else if eq_ascii(val, b"public") {
            Some(Self::Public)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RetryClass {
    Idempotent,
    Conditional,
    Never,
}

impl RetryClass {
    fn parse(val: &[u8]) -> Option<Self> {
        if eq_ascii(val, b"idempotent") {
            Some(Self::Idempotent)
        } else if eq_ascii(val, b"conditional") {
            Some(Self::Conditional)
        } else if eq_ascii(val, b"never") {
            Some(Self::Never)
        } else {
            None
        }
    }
}

fn eq_ascii(a: &[u8], b: &[u8]) -> bool {
    a == b
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ExportRecord {
    service_id: u16,
    export_id: u16,
    version: u8,
    retry_class: RetryClass,
    scope: ExportScope,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RegistryError {
    TooLarge,
    NonAscii,
    MissingVersion,
    UnsupportedVersion,
    MalformedLine,
    UnknownKey,
    DuplicateField,
    DuplicateExport,
    InvalidValue,
    MissingField,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ExportRegistry {
    exports: [Option<ExportRecord>; MAX_EXPORTS],
    len: usize,
}

impl ExportRegistry {
    const fn new() -> Self {
        Self {
            exports: [None; MAX_EXPORTS],
            len: 0,
        }
    }

    fn len(&self) -> usize {
        self.len
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn get(&self, idx: usize) -> Option<ExportRecord> {
        self.exports.get(idx).copied().flatten()
    }

    fn find(&self, service_id: u16, export_id: u16) -> Option<ExportRecord> {
        self.exports
            .iter()
            .flatten()
            .copied()
            .find(|rec| rec.service_id == service_id && rec.export_id == export_id)
    }

    fn parse_bytes(data: &[u8]) -> Result<Self, RegistryError> {
        parser::parse_registry_bytes(data)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemoteExports {
    disabled_reason: RemoteDisabledReason,
    registry: ExportRegistry,
}

impl RemoteExports {
    pub(crate) fn absent() -> Self {
        Self {
            disabled_reason: RemoteDisabledReason::RegistryAbsent,
            registry: ExportRegistry::new(),
        }
    }

    pub(crate) fn invalid() -> Self {
        Self {
            disabled_reason: RemoteDisabledReason::RegistryInvalid,
            registry: ExportRegistry::new(),
        }
    }

    pub(crate) fn from_bytes(data: Option<&[u8]>) -> Self {
        let Some(data) = data else {
            return Self::absent();
        };
        let Ok(registry) = ExportRegistry::parse_bytes(data) else {
            return Self::invalid();
        };
        Self::from_registry(registry)
    }

    fn from_registry(registry: ExportRegistry) -> Self {
        Self {
            disabled_reason: RemoteDisabledReason::NoSecureIdentity,
            registry,
        }
    }

    pub fn disabled_reason(&self) -> RemoteDisabledReason {
        self.disabled_reason
    }

    pub fn export_count(&self) -> usize {
        self.registry.len()
    }
}

pub(crate) use source::{load_remote_exports, RegistrySource, EXPORT_REGISTRY_MAX_BYTES};
