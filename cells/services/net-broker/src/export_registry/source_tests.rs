extern crate alloc;

use super::*;
use alloc::vec::Vec;

struct FakeSource {
    listing: Result<Vec<u8>, ()>,
    stat: Result<(u64, bool), ()>,
    read: Result<Vec<u8>, ()>,
}

impl RegistrySource for FakeSource {
    fn list_dir(&mut self, _path: &str) -> Result<Vec<u8>, ()> {
        self.listing.clone()
    }

    fn stat(&mut self, _path: &str) -> Result<(u64, bool), ()> {
        self.stat
    }

    fn read_file_bounded(&mut self, _path: &str, _max_bytes: usize) -> Result<Vec<u8>, ()> {
        self.read.clone()
    }
}

fn valid_bytes() -> Vec<u8> {
    b"c2c_exports_version=1\n\
      export_0_service_id=8\n\
      export_0_export_id=1\n\
      export_0_version=1\n\
      export_0_retry_class=idempotent\n\
      export_0_scope=local\n"
        .to_vec()
}

#[test]
fn source_flow_rejects_list_failure() {
    let exports = load_remote_exports(&mut FakeSource {
        listing: Err(()),
        stat: Ok((0, false)),
        read: Ok(valid_bytes()),
    });
    assert_eq!(
        exports.disabled_reason(),
        RemoteDisabledReason::RegistryInvalid
    );
}

#[test]
fn source_flow_reports_clean_absent_and_exact_listing_match() {
    let absent = load_remote_exports(&mut FakeSource {
        listing: Ok(b"cluster.cfg\n".to_vec()),
        stat: Ok((0, false)),
        read: Ok(valid_bytes()),
    });
    let exact = load_remote_exports(&mut FakeSource {
        listing: Ok(b"cluster.cfg\nc2c-exports.cfg\n".to_vec()),
        stat: Ok((valid_bytes().len() as u64, false)),
        read: Ok(valid_bytes()),
    });

    assert_eq!(
        absent.disabled_reason(),
        RemoteDisabledReason::RegistryAbsent
    );
    assert_eq!(
        exact.disabled_reason(),
        RemoteDisabledReason::NoSecureIdentity
    );
}

#[test]
fn source_flow_rejects_prefix_and_suffix_listing_matches() {
    let prefix = load_remote_exports(&mut FakeSource {
        listing: Ok(b"prefix-c2c-exports.cfg\n".to_vec()),
        stat: Ok((0, false)),
        read: Ok(valid_bytes()),
    });
    let suffix = load_remote_exports(&mut FakeSource {
        listing: Ok(b"c2c-exports.cfg.bak\n".to_vec()),
        stat: Ok((0, false)),
        read: Ok(valid_bytes()),
    });

    assert_eq!(
        prefix.disabled_reason(),
        RemoteDisabledReason::RegistryAbsent
    );
    assert_eq!(
        suffix.disabled_reason(),
        RemoteDisabledReason::RegistryAbsent
    );
}

#[test]
fn source_flow_rejects_stat_failure_directory_and_oversize() {
    let stat_failure = load_remote_exports(&mut FakeSource {
        listing: Ok(b"c2c-exports.cfg\n".to_vec()),
        stat: Err(()),
        read: Ok(valid_bytes()),
    });
    let directory = load_remote_exports(&mut FakeSource {
        listing: Ok(b"c2c-exports.cfg\n".to_vec()),
        stat: Ok((0, true)),
        read: Ok(valid_bytes()),
    });
    let oversize = load_remote_exports(&mut FakeSource {
        listing: Ok(b"c2c-exports.cfg\n".to_vec()),
        stat: Ok((EXPORT_REGISTRY_MAX_BYTES as u64 + 1, false)),
        read: Ok(valid_bytes()),
    });

    assert_eq!(
        stat_failure.disabled_reason(),
        RemoteDisabledReason::RegistryInvalid
    );
    assert_eq!(
        directory.disabled_reason(),
        RemoteDisabledReason::RegistryInvalid
    );
    assert_eq!(
        oversize.disabled_reason(),
        RemoteDisabledReason::RegistryInvalid
    );
}

#[test]
fn source_flow_rejects_read_failure_and_invalid_bytes() {
    let read_failure = load_remote_exports(&mut FakeSource {
        listing: Ok(b"c2c-exports.cfg\n".to_vec()),
        stat: Ok((16, false)),
        read: Err(()),
    });
    let invalid = load_remote_exports(&mut FakeSource {
        listing: Ok(b"c2c-exports.cfg\n".to_vec()),
        stat: Ok((16, false)),
        read: Ok(b"c2c_exports_version=2\n".to_vec()),
    });

    assert_eq!(
        read_failure.disabled_reason(),
        RemoteDisabledReason::RegistryInvalid
    );
    assert_eq!(
        invalid.disabled_reason(),
        RemoteDisabledReason::RegistryInvalid
    );
}
