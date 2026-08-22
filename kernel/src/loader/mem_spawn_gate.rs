//! Admission gate for ELF images handed to the kernel as caller-supplied bytes
//! (`Syscall::SpawnFromMem`).
//!
//! Such a spawn arrives with a *name*, never a kernel-resolved path, and that name
//! is fully caller-controlled. Every path-keyed decision inside
//! [`super::spawn_gated`] — the `/bin/` manifest-privilege gate,
//! [`super::legacy_path_caps`], `CapSet::with_path_caps`, the operator-policy
//! lookup, the trusted-core recovery list, and the `/bin/vfs` block-region grant —
//! would become caller-selectable if that name were passed through as the path: a
//! `SpawnCap` holder naming `"/bin/vfs"` would inherit that install path's
//! authority. This module therefore reduces the name to a label that provably
//! matches none of those patterns, so the byte-derived trust checks (Ed25519
//! signature, manifest) run while no path-derived authority can be forged.

use alloc::string::String;
use types::ViResult;

/// Prefix carried by every label this module produces. The label invariant — never
/// `/bin/`-prefixed, never equal to nor suffixed by a privileged install path — is
/// what keeps a caller-supplied name from selecting path-based capabilities.
const MEM_LABEL_PREFIX: &str = "/mem/";

/// Component used when a name carries no character that survives filtering.
const FALLBACK_NAME: &str = "cell";

/// Longest label component retained; keeps the label far below `MAX_CELL_PATH`
/// however long the caller's name is.
const MAX_LABEL_NAME: usize = 64;

/// Verify and spawn a cell from an ELF image the caller supplied as bytes.
///
/// Runs the one admission gate shared by every spawn path
/// ([`super::spawn_gated`]): Ed25519 signature over the bytes (fail-closed under
/// `signing-required`), manifest-privilege check, capability intersection with the
/// spawner's ceiling, operator policy, syscall allowlist, cluster membership,
/// memory quota, and integrity measurement. `caller_name` is advisory: it is
/// reduced to a `/mem/` label before the gate sees it and can only ever cost the
/// child privilege, never gain it.
///
/// The child's identity is derived inside atomic publication; no runnable task
/// can retain the kernel `CellId(0)` sentinel.
///
/// # Errors
/// - `ViError::PermissionDenied` — signature invalid, or absent under
///   `signing-required`; or the image's manifest declares privilege that a cell
///   outside `/bin/` may not hold.
/// - `ViError::InvalidInput` — malformed ELF or unsupported relocation.
/// - `ViError::OutOfMemory` — no frames available for the cell's segments.
pub fn spawn_from_mem_gated(
    elf_bytes: &[u8],
    caller_name: &str,
    request: super::SpawnRequest,
) -> ViResult<usize> {
    let label = mem_label(caller_name);
    log::info!(
        "[loader] SpawnFromMem: {} ({} bytes, requested name {:?})",
        label,
        elf_bytes.len(),
        caller_name
    );
    super::spawn_gated(elf_bytes, &label, request)
}

/// Reduce an untrusted name to a `/mem/`-prefixed advisory label.
///
/// Invariants upheld for every input, hostile ones included:
/// - the component after the prefix holds no `/`, so no `ends_with` match against a
///   `/bin/...` install path can succeed. Keeping only the final component is what
///   enforces this; `is_label_char` rejecting `/` is redundant today and exists so
///   the invariant survives a future change to the component extraction;
/// - the result is never `/bin/`-prefixed, so the manifest-privilege gate treats
///   the image as a user cell and `legacy_path_caps` grants nothing;
/// - the result is never a member of the operator-policy or trusted-core path
///   sets, both of which hold only exactly-matched `/bin/` paths.
pub(super) fn mem_label(caller_name: &str) -> String {
    let base = caller_name.rsplit('/').next().unwrap_or("");
    let mut component = String::new();
    for ch in base.chars().filter(is_label_char).take(MAX_LABEL_NAME) {
        component.push(ch);
    }
    let mut label = String::from(MEM_LABEL_PREFIX);
    label.push_str(match component.as_str() {
        // "." and ".." are filtered out rather than kept: they are legal here (the
        // consumers compare whole strings, never canonicalise) but would make the
        // measurement-log label unreadable.
        "" | "." | ".." => FALLBACK_NAME,
        other => other,
    });
    label
}

/// Characters allowed in a label component; `/` is among those excluded, as the
/// invariant documented on [`mem_label`] requires.
fn is_label_char(c: &char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')
}
