//! Exit codes, the fixed output text and the KMS error mapping (k7-cp1-draft §3.3).
//!
//! Every message here is a literal or is built from nonsecret facts only: the key name the
//! operator typed on the command line, KMS status fields (custody, fingerprint of the
//! public key, posture) and the length and a SHA-256 prefix of the blob the cell itself
//! decoded. No key text byte and no blob byte is ever formatted.

use alloc::format;
use alloc::string::String;
use base64ct::{Base64Unpadded, Encoding};
use ostd::clients::kms::{
    KmsClientError, KmsCustodyPosture, KmsErrorCode, KmsSshKeyCustody,
    SshKeyStatusPayload,
};

pub const EXIT_OK: i32 = 0;
/// Refused by the key service (a typed error name or message is printed), or the key
/// service is unreachable or answered something unexpected.
pub const EXIT_REFUSED: i32 = 1;
/// Bad arguments, or key text that is not one armored OpenSSH private key.
pub const EXIT_USAGE: i32 = 2;
/// The key service did not answer in time.
pub const EXIT_TIMEOUT: i32 = 124;
/// No terminal session, or the session was lost.
pub const EXIT_NO_TERMINAL: i32 = 125;
/// Cancelled by the operator (Escape, Ctrl-C, a word other than `delete`).
pub const EXIT_CANCELLED: i32 = 130;

pub const USAGE: &str = "usage: kms-ssh-admin status [NAME] | import NAME | delete NAME\n";
pub const NO_TERMINAL: &str = "kms-ssh-admin: an interactive terminal session is required\n";
pub const IMPORT_PROMPT: &str =
    "Paste the OpenSSH private key now (nothing is echoed; Escape cancels):\n";
pub const IMPORT_CANCELLED: &str = "kms-ssh-admin: cancelled; nothing imported\n";
pub const DELETE_PROMPT: &str = "Type 'delete' and press Enter to delete this key (Escape cancels): ";
pub const DELETE_CANCELLED: &str = "kms-ssh-admin: cancelled; nothing deleted\n";
pub const TEXT_TOO_LONG: &str = "kms-ssh-admin: key text is longer than 98304 bytes\n";
pub const PLAIN_WARNING: &str =
    "  WARNING: unencrypted key held by the key service (development posture)\n";

/// `kms-ssh-admin: <message>\n`.
pub fn line(message: &str) -> String {
    format!("kms-ssh-admin: {message}\n")
}

pub fn custody_name(custody: KmsSshKeyCustody) -> &'static str {
    match custody {
        KmsSshKeyCustody::None => "none",
        KmsSshKeyCustody::Plain => "plain",
        KmsSshKeyCustody::PassphraseWrapped => "passphrase-wrapped",
    }
}

pub fn posture_text(posture: KmsCustodyPosture) -> &'static str {
    match posture {
        KmsCustodyPosture::Unassessed => "posture unassessed",
        KmsCustodyPosture::DevPostureVolatile => "DEV_POSTURE volatile: lost when the key service restarts",
        KmsCustodyPosture::DevPosturePersistent => {
            "DEV_POSTURE persistent: no rollback or crash-durability claim"
        }
    }
}

/// `SHA256:` and the unpadded base64 of the digest: the value `ssh-keygen -lf` prints.
pub fn fingerprint(digest: &[u8; 32]) -> String {
    let mut text = [0u8; 43];
    // 32 bytes encode to 43 unpadded characters; the buffer is exactly that long.
    let encoded = Base64Unpadded::encode(digest, &mut text).unwrap_or("");
    format!("SHA256:{encoded}")
}

/// The first eight bytes of `digest` as 16 lowercase hex characters.
pub fn digest_prefix(digest: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(16);
    for byte in &digest[..8] {
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 15)]));
    }
    text
}

/// Facts of one stored key, the same three lines for status, import and delete.
fn key_lines(status: &SshKeyStatusPayload) -> String {
    format!(
        "  custody: {}\n  fingerprint: {}\n  posture: {}\n",
        custody_name(status.custody),
        fingerprint(&status.fingerprint_sha256),
        posture_text(status.posture),
    )
}

/// `status` without a name.
pub fn leaf_status(status: &SshKeyStatusPayload) -> String {
    format!(
        "kms-ssh-admin: key service ssh leaf\n  readiness: {:?}\n  keys: {}\n  posture: {}\n",
        status.readiness,
        status.key_count,
        posture_text(status.posture),
    )
}

/// `status NAME`.
pub fn key_status(name: &str, status: &SshKeyStatusPayload) -> String {
    format!("kms-ssh-admin: key '{name}'\n{}", key_lines(status))
}

/// The key a `delete` is about to remove.
pub fn delete_target(name: &str, status: &SshKeyStatusPayload) -> String {
    format!("kms-ssh-admin: delete key '{name}'\n{}", key_lines(status))
}

pub fn deleted(name: &str) -> String {
    format!("kms-ssh-admin: deleted key '{name}'\n")
}

/// A completed import: KMS's facts about the stored key, and what the cell itself decoded
/// (length and a SHA-256 prefix) so the operator can compare it with the host's blob.
pub fn imported(name: &str, status: &SshKeyStatusPayload, blob_len: usize, digest: &[u8; 32]) -> String {
    let mut text = format!(
        "kms-ssh-admin: imported key '{name}'\n{}  blob: {blob_len} bytes, sha256 prefix {}\n",
        key_lines(status),
        digest_prefix(digest),
    );
    if status.custody == KmsSshKeyCustody::Plain {
        text.push_str(PLAIN_WARNING);
    }
    text
}

/// Exit code and one-line message for a failed KMS call. The mapping is total: every error
/// the client can return has one fixed message (service codes without a row print the
/// code's own name).
pub fn kms_failure(error: KmsClientError) -> (i32, String) {
    let (code, message): (i32, &str) = match error {
        KmsClientError::Timeout => (EXIT_TIMEOUT, "the key service did not answer in time"),
        KmsClientError::ServiceNotFound => (EXIT_REFUSED, "key service not found"),
        KmsClientError::Ipc => (EXIT_REFUSED, "key service transport error"),
        KmsClientError::WrongSender => (EXIT_REFUSED, "the reply did not come from the key service"),
        KmsClientError::MismatchedResponse | KmsClientError::InvalidPayload | KmsClientError::Wire(_) => {
            (EXIT_REFUSED, "key service protocol error")
        }
        KmsClientError::Service(code) => return (EXIT_REFUSED, service_message(code)),
    };
    (code, line(message))
}

fn service_message(code: KmsErrorCode) -> String {
    let text: &str = match code {
        KmsErrorCode::PermissionDenied | KmsErrorCode::CallerUnattested => {
            "not an admin caller (admin flag or policy missing)"
        }
        KmsErrorCode::SshKeyUnavailable => "key service not serving",
        KmsErrorCode::SshKeyNotFound => "no such key",
        KmsErrorCode::SshKeyExists => {
            "an identity with this name or public key already exists; delete the existing identity first"
        }
        KmsErrorCode::SshStoreFull => {
            "the key store is full: delete the existing identity first, or the key exceeds the store budget"
        }
        KmsErrorCode::SshImportPending => "an import is already open for this caller; try again",
        KmsErrorCode::SshHandleInvalid => "transfer handle refused by the key service; try again",
        KmsErrorCode::SshOrderInvalid => "transfer order refused by the key service; try again",
        KmsErrorCode::SshImportIntegrity => "transfer corrupted; try again",
        KmsErrorCode::SshKeyFormatRejected => {
            "key format refused (openssh-key-v1 ed25519; none, or bcrypt + aes256-ctr only)"
        }
        KmsErrorCode::InvalidRequest => "request refused as invalid",
        other => return line(&format!("key service refused: {other:?}")),
    };
    line(text)
}
