extern crate alloc;

use super::*;
use alloc::format;

fn local_registry() -> alloc::string::String {
    format!(
        "c2c_exports_version=1\n\
         export_0_service_id=8\n\
         export_0_export_id=1\n\
         export_0_version=1\n\
         export_0_retry_class=idempotent\n\
         export_0_scope=local\n"
    )
}

#[test]
fn absent_registry_disables_remote() {
    let exports = RemoteExports::from_bytes(None);
    assert_eq!(
        exports.disabled_reason(),
        RemoteDisabledReason::RegistryAbsent
    );
    assert_eq!(exports.export_count(), 0);
}

#[test]
fn local_registry_without_secure_identity_stays_disabled_but_keeps_records() {
    let exports = RemoteExports::from_bytes(Some(local_registry().as_bytes()));
    assert_eq!(
        exports.disabled_reason(),
        RemoteDisabledReason::NoSecureIdentity
    );
    assert_eq!(exports.export_count(), 1);
    let record = exports.registry.find(8, 1).expect("record");
    assert_eq!(record.scope, ExportScope::Local);
    assert_eq!(record.retry_class, RetryClass::Idempotent);
    assert_eq!(record.version, EXPORT_REGISTRY_VERSION);
}

#[test]
fn remote_registry_without_secure_identity_keeps_records_but_stays_disabled() {
    let cfg = format!(
        "c2c_exports_version=1\n\
         export_0_service_id=8\n\
         export_0_export_id=2\n\
         export_0_version=1\n\
         export_0_retry_class=conditional\n\
         export_0_scope=remote\n"
    );
    let exports = RemoteExports::from_bytes(Some(cfg.as_bytes()));
    assert_eq!(
        exports.disabled_reason(),
        RemoteDisabledReason::NoSecureIdentity
    );
    let record = exports.registry.find(8, 2).expect("record");
    assert_eq!(record.scope, ExportScope::Remote);
    assert_eq!(record.retry_class, RetryClass::Conditional);
}

#[test]
fn public_registry_without_secure_identity_keeps_records_but_stays_disabled() {
    let cfg = format!(
        "c2c_exports_version=1\n\
         export_0_service_id=8\n\
         export_0_export_id=9\n\
         export_0_version=1\n\
         export_0_retry_class=never\n\
         export_0_scope=public\n"
    );
    let exports = RemoteExports::from_bytes(Some(cfg.as_bytes()));

    assert_eq!(
        exports.disabled_reason(),
        RemoteDisabledReason::NoSecureIdentity
    );
    let record = exports.registry.find(8, 9).expect("record");
    assert_eq!(record.scope, ExportScope::Public);
    assert_eq!(record.retry_class, RetryClass::Never);
}

#[test]
fn empty_versioned_registry_is_valid_but_exports_nothing() {
    let exports = RemoteExports::from_bytes(Some(b"c2c_exports_version=1\n"));
    assert_eq!(
        exports.disabled_reason(),
        RemoteDisabledReason::NoSecureIdentity
    );
    assert_eq!(exports.export_count(), 0);
    assert!(exports.registry.is_empty());
    assert_eq!(exports.registry.get(0), None);
}

#[test]
fn registry_parse_keeps_service_and_export_ids_distinct() {
    let exports = RemoteExports::from_bytes(Some(
        b"c2c_exports_version=1\n\
          export_0_service_id=8\n\
          export_0_export_id=3\n\
          export_0_version=1\n\
          export_0_retry_class=idempotent\n\
          export_0_scope=local\n",
    ));

    let record = exports.registry.find(8, 3).expect("record");
    assert_eq!(record.service_id, 8);
    assert_eq!(record.export_id, 3);
}
