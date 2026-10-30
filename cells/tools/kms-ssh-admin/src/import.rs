//! `kms-ssh-admin import NAME`: read the armored key on the echo-off lease, decode it
//! strictly, and drive Begin / Chunk x n / Commit (ops 15-17, Abort 18 on failure).
//!
//! The key is imported as is: a wrapped key's passphrase never enters this cell and its
//! decryptability is not checked here (K2 O-15). The cell prints the blob length and a
//! SHA-256 prefix so the operator can compare it with the host's decoded blob.

use ostd::clients::kms::{ssh_chunk_count, KmsClientError, SshKeyStatusPayload, KMS_SSH_CHUNK_LEN};

use crate::armor::{self, TEXT_MAX};
use crate::console::{Block, Console};
use crate::kms::Kms;
use crate::report::{self, EXIT_CANCELLED, EXIT_NO_TERMINAL, EXIT_OK, EXIT_USAGE};
use crate::secret::Secret;

/// What a completed import tells the operator.
pub struct Imported {
    pub status: SshKeyStatusPayload,
    pub blob_len: usize,
    pub digest: [u8; 32],
}

/// Drive one import of `blob` (1..=65,536 bytes) as `name`. Aborts the pending import
/// (best effort) on any failure after Begin; a timeout aborts nothing, because the client
/// is poisoned and could not send it.
pub fn drive(kms: &dyn Kms, name: &[u8], blob: &[u8]) -> Result<Imported, KmsClientError> {
    let total = u32::try_from(blob.len()).map_err(|_| KmsClientError::InvalidPayload)?;
    if total == 0 || blob.len() > armor::BLOB_MAX {
        return Err(KmsClientError::InvalidPayload);
    }
    let digest = ostd::session::sha256(blob);
    let begin = kms.import_begin(name, total, &digest)?;
    let committed = stage_and_commit(kms, &begin, total, blob, &digest);
    if let Err(error) = &committed {
        if *error != KmsClientError::Timeout && begin.handle != 0 {
            let _ = kms.import_abort(begin.handle);
        }
    }
    Ok(Imported {
        status: committed?,
        blob_len: blob.len(),
        digest,
    })
}

fn stage_and_commit(
    kms: &dyn Kms,
    begin: &ostd::clients::kms::SshTransferBeginResponsePayload,
    total: u32,
    blob: &[u8],
    digest: &[u8; 32],
) -> Result<SshKeyStatusPayload, KmsClientError> {
    // Begin's reply: a nonzero handle, our length, the canonical count and capacity.
    let count = ssh_chunk_count(total);
    if begin.handle == 0
        || begin.total_len != total
        || begin.chunk_count != count
        || usize::from(begin.chunk_capacity) != KMS_SSH_CHUNK_LEN
    {
        return Err(KmsClientError::MismatchedResponse);
    }
    let mut offset = 0usize;
    for index in 0..count {
        let take = (blob.len() - offset).min(KMS_SSH_CHUNK_LEN);
        let ack = kms.import_chunk(begin.handle, index, &blob[offset..offset + take])?;
        offset += take;
        if ack.handle != begin.handle
            || ack.next_chunk_index != index + 1
            || ack.received_len as usize != offset
        {
            return Err(KmsClientError::MismatchedResponse);
        }
    }
    if offset != blob.len() {
        return Err(KmsClientError::MismatchedResponse);
    }
    let status = kms.import_commit(begin.handle, count, digest)?;
    if status.key_present != 1 {
        return Err(KmsClientError::MismatchedResponse);
    }
    Ok(status)
}

pub fn run(name: &str, console: &mut dyn Console, kms: &dyn Kms) -> i32 {
    console.err(report::IMPORT_PROMPT);
    let mut text = Secret::with_capacity(TEXT_MAX);
    let read = console.read_key_block(&mut text);
    // The echo-off line break the operator cannot see.
    console.err("\n");
    match read {
        Block::Done => {}
        Block::Cancelled => {
            text.wipe();
            console.err(report::IMPORT_CANCELLED);
            return EXIT_CANCELLED;
        }
        Block::TooLong => {
            text.wipe();
            console.err(report::TEXT_TOO_LONG);
            return EXIT_USAGE;
        }
        Block::Lost => {
            text.wipe();
            return EXIT_NO_TERMINAL;
        }
    }
    let decoded = armor::decode(text.as_slice());
    // The key text is not needed past the decode.
    drop(text);
    let blob = match decoded {
        Ok(blob) => blob,
        Err(error) => {
            console.err(&report::line(error.message()));
            return EXIT_USAGE;
        }
    };
    match drive(kms, name.as_bytes(), blob.as_slice()) {
        Ok(done) => {
            console.out(&report::imported(
                name,
                &done.status,
                done.blob_len,
                &done.digest,
            ));
            if console.lost() {
                EXIT_NO_TERMINAL
            } else {
                EXIT_OK
            }
        }
        Err(error) => {
            let (code, message) = report::kms_failure(error);
            console.err(&message);
            code
        }
    }
}
