use super::ascii::{parse_u16_ascii, parse_u8_ascii, parse_usize_ascii, set_once, trim_ascii};
use super::*;

#[derive(Clone, Copy)]
struct ExportBuilder {
    used: bool,
    service_id: Option<u16>,
    export_id: Option<u16>,
    version: Option<u8>,
    retry_class: Option<RetryClass>,
    scope: Option<ExportScope>,
}

impl ExportBuilder {
    const fn new() -> Self {
        Self {
            used: false,
            service_id: None,
            export_id: None,
            version: None,
            retry_class: None,
            scope: None,
        }
    }

    fn set(&mut self, field: &[u8], val: &[u8]) -> Result<(), RegistryError> {
        self.used = true;
        if eq_ascii(field, b"service_id") {
            self.set_service_id(val)
        } else if eq_ascii(field, b"export_id") {
            self.set_export_id(val)
        } else if eq_ascii(field, b"version") {
            self.set_version(val)
        } else if eq_ascii(field, b"retry_class") {
            self.set_retry_class(val)
        } else if eq_ascii(field, b"scope") {
            self.set_scope(val)
        } else {
            Err(RegistryError::UnknownKey)
        }
    }

    fn set_service_id(&mut self, val: &[u8]) -> Result<(), RegistryError> {
        set_once(&mut self.service_id, parse_u16_ascii(val))
    }

    fn set_export_id(&mut self, val: &[u8]) -> Result<(), RegistryError> {
        set_once(&mut self.export_id, parse_u16_ascii(val))
    }

    fn set_version(&mut self, val: &[u8]) -> Result<(), RegistryError> {
        let version = parse_u8_ascii(val).ok_or(RegistryError::InvalidValue)?;
        if version != EXPORT_REGISTRY_VERSION {
            return Err(RegistryError::UnsupportedVersion);
        }
        set_once(&mut self.version, Some(version))
    }

    fn set_retry_class(&mut self, val: &[u8]) -> Result<(), RegistryError> {
        set_once(&mut self.retry_class, RetryClass::parse(val))
    }

    fn set_scope(&mut self, val: &[u8]) -> Result<(), RegistryError> {
        set_once(&mut self.scope, ExportScope::parse(val))
    }

    fn build(&self) -> Result<Option<ExportRecord>, RegistryError> {
        if !self.used {
            return Ok(None);
        }
        Ok(Some(ExportRecord {
            service_id: self.service_id.ok_or(RegistryError::MissingField)?,
            export_id: self.export_id.ok_or(RegistryError::MissingField)?,
            version: self.version.ok_or(RegistryError::MissingField)?,
            retry_class: self.retry_class.ok_or(RegistryError::MissingField)?,
            scope: self.scope.ok_or(RegistryError::MissingField)?,
        }))
    }
}

pub fn parse_registry_bytes(data: &[u8]) -> Result<ExportRegistry, RegistryError> {
    if data.len() > EXPORT_REGISTRY_MAX_BYTES {
        return Err(RegistryError::TooLarge);
    }
    if !data.is_ascii() {
        return Err(RegistryError::NonAscii);
    }

    let mut version = None;
    let mut builders = [const { ExportBuilder::new() }; MAX_EXPORTS];

    for raw_line in data.split(|&b| b == b'\n') {
        let line = trim_ascii(raw_line);
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        let Some(eq) = line.iter().position(|&b| b == b'=') else {
            return Err(RegistryError::MalformedLine);
        };
        let key = trim_ascii(&line[..eq]);
        let val = trim_ascii(&line[eq + 1..]);

        if eq_ascii(key, GLOBAL_VERSION_KEY) {
            let parsed = parse_u8_ascii(val).ok_or(RegistryError::InvalidValue)?;
            if parsed != EXPORT_REGISTRY_VERSION {
                return Err(RegistryError::UnsupportedVersion);
            }
            if version.replace(parsed).is_some() {
                return Err(RegistryError::DuplicateField);
            }
            continue;
        }

        let (idx, field) = parse_export_key(key).ok_or(RegistryError::UnknownKey)?;
        let builder = builders.get_mut(idx).ok_or(RegistryError::UnknownKey)?;
        builder.set(field, val)?;
    }

    if version != Some(EXPORT_REGISTRY_VERSION) {
        return Err(RegistryError::MissingVersion);
    }

    let mut registry = ExportRegistry::new();
    for builder in &builders {
        let Some(record) = builder.build()? else {
            continue;
        };
        if registry.exports.iter().flatten().any(|existing| {
            existing.service_id == record.service_id && existing.export_id == record.export_id
        }) {
            return Err(RegistryError::DuplicateExport);
        }
        if registry.len >= MAX_EXPORTS {
            return Err(RegistryError::TooLarge);
        }
        registry.exports[registry.len] = Some(record);
        registry.len += 1;
    }

    Ok(registry)
}

fn parse_export_key(key: &[u8]) -> Option<(usize, &[u8])> {
    let rest = key.strip_prefix(EXPORT_PREFIX)?;
    let sep = rest.iter().position(|&b| b == b'_')?;
    let idx = parse_usize_ascii(&rest[..sep])?;
    Some((idx, &rest[sep + 1..]))
}
