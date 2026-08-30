use crate::cli::CliResult;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub fn read_bounded(path: &Path, label: &str, max_len: u64) -> CliResult<Vec<u8>> {
    let file = File::open(path)
        .map_err(|error| format!("cannot open {label} {}: {error}", path.display()))?;
    let declared = file
        .metadata()
        .map_err(|error| format!("cannot inspect {label} {}: {error}", path.display()))?
        .len();
    if declared > max_len {
        return Err(format!("{label} exceeds its {max_len}-byte limit"));
    }
    let capacity = usize::try_from(declared)
        .map_err(|_| format!("{label} length does not fit the host address space"))?;
    let read_limit = max_len
        .checked_add(1)
        .ok_or_else(|| format!("{label} limit is too large"))?;
    let mut bytes = Vec::with_capacity(capacity);
    file.take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read {label} {}: {error}", path.display()))?;
    if u64::try_from(bytes.len()).map_err(|_| format!("{label} length does not fit u64"))? > max_len
    {
        return Err(format!(
            "{label} changed while reading or exceeds its limit"
        ));
    }
    Ok(bytes)
}

pub fn read_seed(path: &Path) -> CliResult<[u8; 32]> {
    let seed = read_exact_32(path, "seed")?;
    if seed.iter().all(|byte| *byte == 0) {
        return Err("seed file must contain a nonzero 32-byte seed".to_owned());
    }
    Ok(seed)
}

pub fn read_public_key(path: &Path) -> CliResult<[u8; 32]> {
    let key = read_exact_32(path, "public key")?;
    if key.iter().all(|byte| *byte == 0) {
        return Err("public-key file must contain a nonzero 32-byte key".to_owned());
    }
    Ok(key)
}

fn read_exact_32(path: &Path, label: &str) -> CliResult<[u8; 32]> {
    let bytes = read_bounded(path, label, 32)?;
    bytes.try_into().map_err(|bytes: Vec<u8>| {
        format!(
            "{label} file {} must contain exactly 32 raw bytes, found {}",
            path.display(),
            bytes.len()
        )
    })
}

pub struct ReservedOutputs {
    transcript: File,
    summary: File,
    transcript_path: PathBuf,
    summary_path: PathBuf,
    committed: bool,
}

impl ReservedOutputs {
    pub fn create(transcript_path: &Path, summary_path: &Path) -> CliResult<Self> {
        let transcript = reserve(transcript_path, "transcript output")?;
        let summary = match reserve(summary_path, "summary output") {
            Ok(file) => file,
            Err(error) => {
                drop(transcript);
                let _ = fs::remove_file(transcript_path);
                return Err(error);
            }
        };
        Ok(Self {
            transcript,
            summary,
            transcript_path: transcript_path.to_owned(),
            summary_path: summary_path.to_owned(),
            committed: false,
        })
    }

    pub fn commit(mut self, transcript: &[u8], summary: &[u8]) -> CliResult<()> {
        self.transcript
            .write_all(transcript)
            .map_err(|error| format!("cannot write transcript output: {error}"))?;
        self.summary
            .write_all(summary)
            .map_err(|error| format!("cannot write summary output: {error}"))?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for ReservedOutputs {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.transcript_path);
            let _ = fs::remove_file(&self.summary_path);
        }
    }
}

fn reserve(path: &Path, label: &str) -> CliResult<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("cannot create {label} {}: {error}", path.display()))
}

pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}
