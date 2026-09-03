//! Current working directory built-ins: cd and pwd.

use alloc::{string::String, vec::Vec};
use ostd::prelude::*;

const MAX_SHELL_CWD_BYTES: usize = 4096;

/// Retrieve the current task's working directory.
///
/// Returns a validated UTF-8 path no longer than [`MAX_SHELL_CWD_BYTES`].
/// Returns `ViError::OutOfMemory`, `ViError::IO`, or `ViError::InvalidInput`
/// without inventing a fallback directory.
pub fn get_shell_cwd() -> ViResult<String> {
    let mut buf = Vec::new();
    buf.try_reserve_exact(MAX_SHELL_CWD_BYTES)
        .map_err(|_| ViError::OutOfMemory)?;
    buf.resize(MAX_SHELL_CWD_BYTES, 0);
    let len = ostd::syscall::sys_getcwd(&mut buf).map_err(|_| ViError::IO)?;
    buf.truncate(len);
    String::from_utf8(buf).map_err(|_| ViError::InvalidInput)
}

/// `pwd` — print the current working directory.
pub fn cmd_pwd(_args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let cwd = get_shell_cwd()?;
    crate::executor::shell_println(&cwd);
    Ok(())
}

/// `cd` — change the current working directory.
pub fn cmd_cd(mut args: crate::text_engine::args::LegacyArgs<'_>) -> ViResult<()> {
    let target = match args.next() {
        Some(t) => t,
        None => {
            ostd::io::println("cd: missing operand");
            return Err(ViError::InvalidArgument);
        }
    };
    if args.next().is_some() {
        ostd::io::println("cd: too many arguments");
        return Err(ViError::InvalidArgument);
    }
    match ostd::syscall::sys_chdir(target) {
        Ok(()) => Ok(()),
        Err(_) => {
            ostd::io::print("cd: cannot change directory to '");
            ostd::io::print(target);
            ostd::io::println("'");
            Err(ViError::NotFound)
        }
    }
}
