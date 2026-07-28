//! Operand loading for `grep`: stdin, single files, and recursive VFS walks.

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use text_engine::grep::{push_input, Config, LoadedInput};
use text_engine::records::MAX_INPUT_BYTES;

const GREP_MAX_DEPTH: usize = 16;

mod read;
mod vfs;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadError {
    Io,
    InputTooLarge,
    AllocationFailed,
    InvalidUtf8,
}

impl ReadError {
    pub fn message(self) -> &'static str {
        match self {
            Self::Io => "cannot read",
            Self::InputTooLarge => "input exceeds 65536-byte limit",
            Self::AllocationFailed => "input allocation failed",
            Self::InvalidUtf8 => "input is not valid UTF-8",
        }
    }
}

pub fn load_inputs(cfg: &Config) -> Result<Vec<LoadedInput>, String> {
    if cfg.files.is_empty() {
        let stdin = crate::executor::shell_stdin();
        if stdin.len() > MAX_INPUT_BYTES {
            return Err(String::from("stdin exceeds 65536-byte limit"));
        }
        let text = core::str::from_utf8(stdin)
            .map(String::from)
            .map_err(|_| String::from("stdin is not valid UTF-8"))?;
        return Ok(alloc::vec![LoadedInput { label: None, text }]);
    }
    let mut inputs = Vec::new();
    let mut total_bytes = 0usize;
    if cfg.recursive {
        for path in &cfg.files {
            walk_path(path, 0, &mut total_bytes, &mut inputs)?;
        }
    } else {
        let prefix = cfg.files.len() > 1;
        for path in &cfg.files {
            let text = read_text_file(path).map_err(|err| format!("{} '{path}'", err.message()))?;
            push_input(
                &mut inputs,
                &mut total_bytes,
                prefix.then(|| path.clone()),
                text,
            )?;
        }
    }
    Ok(inputs)
}

pub fn read_text_file(path: &str) -> Result<String, ReadError> {
    let bytes = read::read_path_bytes(path)?;
    core::str::from_utf8(&bytes)
        .map(String::from)
        .map_err(|_| ReadError::InvalidUtf8)
}

fn walk_path(
    path: &str,
    depth: usize,
    total_bytes: &mut usize,
    inputs: &mut Vec<LoadedInput>,
) -> Result<(), String> {
    if depth >= GREP_MAX_DEPTH {
        return Err(format!(
            "recursion depth exceeds {GREP_MAX_DEPTH} at '{path}'"
        ));
    }
    match vfs::stat_path(path)? {
        Some(true) => {
            for entry in vfs::list_dir(path)? {
                let child = vfs::join_path(path, &entry.name);
                if entry.is_dir {
                    walk_path(&child, depth + 1, total_bytes, inputs)?;
                } else {
                    let text = read_text_file(&child)
                        .map_err(|err| format!("{} '{child}'", err.message()))?;
                    push_input(inputs, total_bytes, Some(child), text)?;
                }
            }
            Ok(())
        }
        Some(false) => {
            let text = read_text_file(path).map_err(|err| format!("{} '{path}'", err.message()))?;
            push_input(inputs, total_bytes, Some(String::from(path)), text)?;
            Ok(())
        }
        None => Err(format!("cannot stat '{path}'")),
    }
}
