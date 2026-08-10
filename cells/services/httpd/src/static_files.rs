extern crate alloc;

use alloc::vec::Vec;
use api::ipc::VfsResponse;
use ostd::ipc::IpcError;
use ostd::ViError;

pub const STATIC_FILE_MAX_BYTES: usize = 64 * 1024;
const VFS_MISSING_PATH_ERR_CODE: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticFileResult {
    Body(Vec<u8>),
    NotFound,
    InternalError,
}

pub fn classify_static_file_preflight_wire(
    stat: Result<VfsResponse<'_>, IpcError>,
) -> Result<(), StaticFileResult> {
    match stat {
        Ok(VfsResponse::Stat { is_dir: false, .. }) => Ok(()),
        Ok(VfsResponse::Err(VFS_MISSING_PATH_ERR_CODE)) => Err(StaticFileResult::NotFound),
        Ok(_) | Err(_) => Err(StaticFileResult::InternalError),
    }
}

pub fn classify_static_file_read(read: Result<Vec<u8>, ViError>) -> StaticFileResult {
    match read {
        Ok(body) => StaticFileResult::Body(body),
        Err(ViError::NotFound) => StaticFileResult::NotFound,
        Err(_) => StaticFileResult::InternalError,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        classify_static_file_preflight_wire, classify_static_file_read, StaticFileResult,
        STATIC_FILE_MAX_BYTES,
    };
    use alloc::vec;
    use alloc::vec::Vec;
    use core::cell::Cell;
    use ostd::ipc::IpcError;
    use ostd::ViError;

    #[test]
    fn large_static_files_are_still_success() {
        let body = vec![b'x'; 1024];
        assert_eq!(
            classify_static_file_read(Ok(body.clone())),
            StaticFileResult::Body(body)
        );
        assert!(STATIC_FILE_MAX_BYTES > 480);
    }

    #[test]
    fn missing_files_map_to_not_found() {
        assert_eq!(
            classify_static_file_read(Err(ViError::NotFound)),
            StaticFileResult::NotFound
        );
        assert_eq!(
            classify_static_file_preflight_wire(Ok(api::ipc::VfsResponse::Err(1))),
            Err(StaticFileResult::NotFound)
        );
    }

    #[test]
    fn existing_empty_file_is_not_absent() {
        assert_eq!(
            classify_static_file_preflight_wire(Ok(api::ipc::VfsResponse::Stat {
                size: 0,
                is_dir: false,
            })),
            Ok(())
        );
        assert_eq!(
            classify_static_file_read(Ok(Vec::new())),
            StaticFileResult::Body(Vec::new())
        );
    }

    #[test]
    fn internal_failures_stay_internal() {
        assert_eq!(
            classify_static_file_read(Err(ViError::OutOfMemory)),
            StaticFileResult::InternalError
        );
        assert_eq!(
            classify_static_file_read(Err(ViError::IO)),
            StaticFileResult::InternalError
        );
        assert_eq!(
            classify_static_file_preflight_wire(Err(IpcError::Recv)),
            Err(StaticFileResult::InternalError)
        );
    }

    #[test]
    fn directories_are_not_treated_as_missing() {
        assert_eq!(
            classify_static_file_preflight_wire(Ok(api::ipc::VfsResponse::Stat {
                size: 0,
                is_dir: true,
            })),
            Err(StaticFileResult::InternalError)
        );
    }

    #[test]
    fn unexpected_wire_replies_stay_internal() {
        assert_eq!(
            classify_static_file_preflight_wire(Ok(api::ipc::VfsResponse::Ok)),
            Err(StaticFileResult::InternalError)
        );
    }

    #[test]
    fn classification_has_no_cache_and_reuses_each_read() {
        let reads = Cell::new(0u8);
        let first = classify_static_file_read(Ok({
            reads.set(reads.get() + 1);
            vec![b'1'; 600]
        }));
        let second = classify_static_file_read(Ok({
            reads.set(reads.get() + 1);
            vec![b'2'; 600]
        }));

        assert_eq!(reads.get(), 2);
        assert_eq!(first, StaticFileResult::Body(vec![b'1'; 600]));
        assert_eq!(second, StaticFileResult::Body(vec![b'2'; 600]));
    }
}
