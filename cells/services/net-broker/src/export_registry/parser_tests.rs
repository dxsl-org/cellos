extern crate alloc;

use super::*;
use alloc::{format, vec};

#[test]
fn registry_parse_rejects_duplicate_export_pair() {
    let cfg = format!(
        "c2c_exports_version=1\n\
         export_0_service_id=8\n\
         export_0_export_id=1\n\
         export_0_version=1\n\
         export_0_retry_class=idempotent\n\
         export_0_scope=local\n\
         export_1_service_id=8\n\
         export_1_export_id=1\n\
         export_1_version=1\n\
         export_1_retry_class=never\n\
         export_1_scope=public\n"
    );
    assert!(ExportRegistry::parse_bytes(cfg.as_bytes()).is_err());
}

#[test]
fn registry_parse_rejects_missing_version_and_oversize() {
    let missing_version = format!(
        "export_0_service_id=8\n\
         export_0_export_id=1\n\
         export_0_version=1\n\
         export_0_retry_class=idempotent\n\
         export_0_scope=local\n"
    );
    assert!(ExportRegistry::parse_bytes(missing_version.as_bytes()).is_err());
    assert!(ExportRegistry::parse_bytes(&vec![b'a'; EXPORT_REGISTRY_MAX_BYTES + 1]).is_err());
}

#[test]
fn registry_parse_rejects_malformed_line_and_duplicate_global_version() {
    let malformed = b"c2c_exports_version=1\nexport_0_service_id\n";
    let duplicate_version = b"c2c_exports_version=1\nc2c_exports_version=1\n";

    assert_eq!(
        ExportRegistry::parse_bytes(malformed),
        Err(RegistryError::MalformedLine)
    );
    assert_eq!(
        ExportRegistry::parse_bytes(duplicate_version),
        Err(RegistryError::DuplicateField)
    );
}

#[test]
fn registry_parse_rejects_empty_numbers_unknown_keys_and_non_ascii() {
    let empty_number = format!(
        "c2c_exports_version=1\n\
         export_0_service_id=\n\
         export_0_export_id=1\n\
         export_0_version=1\n\
         export_0_retry_class=idempotent\n\
         export_0_scope=local\n"
    );
    let unknown_key = format!(
        "c2c_exports_version=1\n\
         export_0_service_id=8\n\
         export_0_export_id=1\n\
         export_0_version=1\n\
         export_0_retry_class=idempotent\n\
         export_0_scope=local\n\
         export_0_extra=nope\n"
    );

    assert_eq!(
        ExportRegistry::parse_bytes(empty_number.as_bytes()),
        Err(RegistryError::InvalidValue)
    );
    assert_eq!(
        ExportRegistry::parse_bytes(unknown_key.as_bytes()),
        Err(RegistryError::UnknownKey)
    );
    assert_eq!(
        ExportRegistry::parse_bytes("c2c_exports_version=1\nexport_0_scope=locál\n".as_bytes()),
        Err(RegistryError::NonAscii)
    );
}

#[test]
fn registry_parse_rejects_duplicate_fields_missing_fields_bad_version_and_out_of_bounds_index() {
    let duplicate_field = format!(
        "c2c_exports_version=1\n\
         export_0_service_id=8\n\
         export_0_service_id=9\n\
         export_0_export_id=1\n\
         export_0_version=1\n\
         export_0_retry_class=idempotent\n\
         export_0_scope=local\n"
    );
    let missing_field = format!(
        "c2c_exports_version=1\n\
         export_0_service_id=8\n\
         export_0_export_id=1\n\
         export_0_version=1\n\
         export_0_scope=local\n"
    );
    let bad_version = format!(
        "c2c_exports_version=2\n\
         export_0_service_id=8\n\
         export_0_export_id=1\n\
         export_0_version=1\n\
         export_0_retry_class=idempotent\n\
         export_0_scope=local\n"
    );
    let out_of_bounds = format!(
        "c2c_exports_version=1\n\
         export_16_service_id=8\n\
         export_16_export_id=1\n\
         export_16_version=1\n\
         export_16_retry_class=idempotent\n\
         export_16_scope=local\n"
    );

    assert_eq!(
        ExportRegistry::parse_bytes(duplicate_field.as_bytes()),
        Err(RegistryError::DuplicateField)
    );
    assert_eq!(
        ExportRegistry::parse_bytes(missing_field.as_bytes()),
        Err(RegistryError::MissingField)
    );
    assert_eq!(
        ExportRegistry::parse_bytes(bad_version.as_bytes()),
        Err(RegistryError::UnsupportedVersion)
    );
    assert_eq!(
        ExportRegistry::parse_bytes(out_of_bounds.as_bytes()),
        Err(RegistryError::UnknownKey)
    );
}

#[test]
fn registry_parse_rejects_invalid_retry_class_and_scope() {
    let bad_retry = format!(
        "c2c_exports_version=1\n\
         export_0_service_id=8\n\
         export_0_export_id=1\n\
         export_0_version=1\n\
         export_0_retry_class=maybe\n\
         export_0_scope=local\n"
    );
    let bad_scope = format!(
        "c2c_exports_version=1\n\
         export_0_service_id=8\n\
         export_0_export_id=1\n\
         export_0_version=1\n\
         export_0_retry_class=idempotent\n\
         export_0_scope=cluster\n"
    );

    assert_eq!(
        ExportRegistry::parse_bytes(bad_retry.as_bytes()),
        Err(RegistryError::InvalidValue)
    );
    assert_eq!(
        ExportRegistry::parse_bytes(bad_scope.as_bytes()),
        Err(RegistryError::InvalidValue)
    );
}
