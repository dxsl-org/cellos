# Phase 04 — VFS Grant-Backed Writes

## Context Links
`docs/specs/09-vfs.md`; `libs/api/src/services/ipc.rs`; `cells/services/vfs/src/dispatch.rs`; `libs/ostd/src/fs.rs`; `cells/tests/vfs-test/src/main.rs`.

## Overview
Enable the existing `WriteGrant` contract through owner-bound VFS routing while preserving authorization, quotas, and commit ordering.

## Key Insights
The wire and SDK helper exist; the service rejects every write grant because it cannot map a cap to an authorized writable path. The small SDK helper falsely reports `Ok(0)`.

## Requirements
No ABI changes. Re-authorize before grant access; validate owner, range, offset and quota before mutation; acknowledge only after committed write; conceal invalid/wrong-owner distinctions.

## Architecture
Service-local file/cap records bind a caller-owned writable target to a grant write. Existing AccessTable, quota manager, backend manager, and grant validation remain authority layers.

## Related Code Files
`dispatch.rs`, `file_handles.rs`, `handle_table.rs`, `access/rules.rs`, `libs/ostd/src/fs.rs`, VFS guest/integration tests.

## Implementation Steps
1. Select/reuse the established owner-bound handle record if it supports write semantics; otherwise add private routing state.
2. Perform authorization/range/quota checks before grant read and backend mutation.
3. Return committed byte counts through OSTD helper.
4. Replace fail-closed regression case with positive and negative guest evidence.

## Todo List
- [ ] Trace existing handle and grant authority paths.
- [ ] Implement atomic routed grant write.
- [ ] Run guest and integration verification.

## Success Criteria
Authorized writes commit exact bytes and report their count; invalid/denied/quota-overflow cases do not mutate backend or quota; acknowledgement follows visibility.

## Risk Assessment
Incorrect cap-to-path binding can bypass path policy or quota. Do not expose reason distinctions to untrusted callers.

## Security Considerations
Validate kernel-shared bytes only after caller/path authority; free grants only after synchronous reply.

## Next Steps
Retain directory-capability migration as a separate approved cutover.