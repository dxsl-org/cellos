extern crate alloc;

use super::ReadError;
use crate::text_engine::records::{extend_input, InputBufferError, MAX_INPUT_BYTES};
use alloc::vec::Vec;
use ostd::syscall;

pub(super) fn read_path_bytes(path: &str) -> Result<Vec<u8>, ReadError> {
    if let Some((size, is_dir)) = crate::cmd_fs::stat_file_vfs(path) {
        if is_dir {
            return Err(ReadError::Io);
        }
        if size > MAX_INPUT_BYTES {
            return Err(ReadError::InputTooLarge);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(size)
            .map_err(|_| ReadError::AllocationFailed)?;
        bytes.resize(size, 0);
        if size == 0 {
            return Ok(bytes);
        }
        if crate::cmd_fs::read_file_vfs(path, &mut bytes) == size {
            return Ok(bytes);
        }
        return Err(ReadError::Io);
    }

    let fd = syscall::sys_open(path).map_err(|_| ReadError::Io)?;
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 512];
    loop {
        match syscall::sys_read(fd, &mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if let Err(err) = extend_input(&mut bytes, &chunk[..n]) {
                    syscall::sys_close(fd);
                    return Err(match err {
                        InputBufferError::TooLarge => ReadError::InputTooLarge,
                        InputBufferError::AllocationFailed => ReadError::AllocationFailed,
                    });
                }
            }
            Err(_) => {
                syscall::sys_close(fd);
                return Err(ReadError::Io);
            }
        }
    }
    syscall::sys_close(fd);
    Ok(bytes)
}
