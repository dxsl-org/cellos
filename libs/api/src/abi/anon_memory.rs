// SPDX-License-Identifier: Apache-2.0
//! Tier-2 large anonymous memory syscall constants (`AnonMemory = 440`); ADR-0033, CP1 §6.

/// Operation: allocate / reserve anonymous memory.
pub const ANON_MAP: usize = 1;
/// Operation: unmap anonymous memory in range.
pub const ANON_UNMAP: usize = 2;
/// Operation: change protections of anonymous memory in range.
pub const ANON_PROTECT: usize = 3;

/// Protection flag: read access allowed.
pub const PROT_READ: usize = 1 << 0;
/// Protection flag: write access allowed.
pub const PROT_WRITE: usize = 1 << 1;

/// MAP flag: map at exact fixed address.
pub const ANON_FIXED: usize = 1 << 8;

/// Maximum bytes that can be eagerly committed in one MAP call: 64 MiB.
pub const ANON_COMMIT_MAX_BYTES: usize = 64 * 1024 * 1024;

/// Error sentinels for `AnonMemory` (syscall 440).
pub const ERR_PERMISSION_DENIED: usize = usize::MAX;
pub const ERR_TRY_AGAIN: usize = usize::MAX - 1;
pub const ERR_INVALID_INPUT: usize = usize::MAX - 3;
pub const ERR_NOT_SUPPORTED: usize = usize::MAX - 5;
pub const ERR_OUT_OF_MEMORY: usize = usize::MAX - 6;
