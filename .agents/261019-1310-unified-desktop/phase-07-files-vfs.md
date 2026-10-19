---
phase: 7
title: "Complete VFS operations and standalone Files navigation"
status: pending
priority: P1
tier: thinking
dependencies: [1, 2, 3, 4, 5, 6]
---
# Phase 07 — Files and backend truth

## Overview
Implement real filesystem navigation and safe operations, then expose the approved List/Grid/Columns UI. A 480-byte directory snapshot cannot be passed off as complete Files support.

## Requirements
AC-01/04/05/13/14/15. List default; Grid and Miller Columns, common Places/path/search/selection, stable view mode on snap; safe create/rename/delete/copy/move with truthful results.

## Architecture
Files is a separate ViUI app and native view consumer. Navigation owns directory handles and cancellable, revision-correlated requests, not unbounded whole-tree scans. Reuse VFS caller `(CellId,generation)` and handle-relative authority. Existing copied listing is bounded at 480 bytes; file stat reports size/is_dir only. Introduce exact approved pagination/metadata/operation deltas as needed rather than fabricate timestamps/permissions. VFS has limited rename and no complete copy/move/Trash transaction contract; inspect each backend's capabilities before displaying actions.

## Related Files
- Create: `cells/apps/files/` app manifest/build/model/views/operation controller; workspace and desktop catalog registration.
- Modify after approved delta: `libs/api/src/services/ipc.rs`, `libs/ostd/src/clients/vfs.rs`, VFS dispatch/dirs/file_handles/backend/manager/access/mount modules and affected backend implementations.
- Existing verification: VFS owner-lifetime and mutation/failure suites, `tests/integration/tests/srv-cellosfs.rs`; docs/vfs-api.md and Files guide.

## Implementation Steps
1. Under the phase-01 exact approval, implement bounded paginated enumeration including unambiguous continuation/error states and directory-change behavior (restart-with-notice, never silently omit entries). Preserve handle limits/revocation and encode full names without newline ambiguity. Search is bounded in current directory by default, not an invented full-disk index.
2. Add client operations required by Files: chunked reads/writes/create/sync, correct same-backend rename semantics and backend identity/capability projection. Access denial, mount/root protection, unsupported and unavailable metadata remain explicit. Do not expand G2-J/frozen filesystem format under this UI plan.
3. Define lifecycle for create/rename/copy/move/delete with source/destination authority, conflict choices, cancellation and partial-result reporting. Copy binds source identity plus content revision or an exclusive read lease; use bounded buffers into a distinct destination staging object, propagate short writes/flush errors, then sync/verify. Replace an existing destination only via a supported safe namespace commit, never truncate it before copy succeeds. Before move deletes source, revalidate the same source revision/lease. Alias, descendant/link/mount races are checked through handles. If required stability/replace primitive is absent, preserve source/prior destination and report unsupported/partial outcome. Test concurrent source mutation and overwrite cancellation/disk-full/flush failure. Do not claim crash-atomicity or widen G2-J.
4. Implement List with sorting and true available metadata; Grid uses the shared bounded thumbnail/image primitives extracted in phase 06; Columns has single-click directory expansion, ancestor branch replacement, adjustable widths and keyboard traversal. Reuse one model/selection/operation controller, no duplicate view-specific I/O. Keep mode on resize; horizontal column scrolling keeps active choice reachable. Represent links only if backend really supports them, never fabricate entries.
5. Implement Places, mounted volumes and user pin/unpin, Back/Forward, breadcrumb/editable path and local search. Pin preferences use the phase-02 approved store with explicit persistence acknowledgement; missing/inaccessible targets remain removable, not auto-rebound by guessed names. Errors/permission changes leave other sessions usable. Enter opens via association; F2/Ctrl+X/C/V/Delete operate according to actual capabilities. Before permanent deletion disclose irreversibility; offer Trash/Undo only for explicitly implemented reversible backend effects, no silent unsupported buttons.
6. Register Files views in unified frames and shared appearance. Handle active-app `+` by creating an independent browsing session through the correlated create-view contract, preserving the original session's location/history/selection; failure or cancelled creation leaves the original usable. Open With grants only selected resource intent; preview/select must not run script/ELF. Explicit Run is separate, confirmed and admitted. Native fs paths like `/bin/ocel` in user-readable catalogs confer no execute authority.
7. Exercise actual writable disposable FAT/native/RamFS/read-only boot locations: directory larger than wire frame, long/Unicode filenames, mode retention in narrow snap, rename conflicts, permission denied, partial copy, cancellation, disk full, backend loss and changed source. Verify source/destination bytes and persisted content to supported evidence ceiling.

## Assumptions
G2-J crash-preserving durability is not available/approved; do not promise it. Pagination, richer metadata and copy/move policy are new proposed contracts needing CP1/CP2. No full-text recursive search or general link implementation is implied.

## Success Criteria
- [ ] More than 480 bytes of entries are reachable without omission/truncation; stale directory continuation cannot silently display mixed data.
- [ ] All three real views share selection/actions and remain keyboard/mouse usable after resize/theme change.
- [ ] Real safe operations match conflict/cancel/error semantics; cross-device failures preserve source; UI accurately states partial completion and irreversible delete.
- [ ] Read-only/denied backend actions fail without mutation; same-app views never leak handles across owner generations.
- [ ] Active-app `+` creates a second independent Files view; failure/cancel, pin/unpin and restored pinned-location navigation exercise the real UI and preference semantics.

## Security Considerations
Handle-relative least authority; no traversal through `..`/mount/link into an undelegated namespace, no TOCTOU path reopening after a selected resource was verified. Quota-bound enumeration/thumbnail/copy concurrency. Untrusted names are data and cannot inject new list entries or launch arguments.

## Risk Assessment
Data-loss risk is high. No code rollback restores already deleted/replaced user files; all failure/cancel/storage tests use disposable volumes and retain exact outcomes. Roll back VFS/protocol/client/Files cutover coherently; preserve existing volume bytes and never autoformat or repair to make a test pass. Missing frozen storage capability blocks only its exact claim, not safe supported operations.

## Deviation Log
Append during execution.
