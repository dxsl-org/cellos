---
phase: 8
title: "Compact terminal editor with truthful save behavior"
status: completed-private-evidence
priority: P1
tier: thinking
dependencies: [4, 5]
---
# Phase 08: Compact editor

## Overview and requirements

Deliver CBX-21/22: an approachable config/script/text editor inspired by Kibi with Ratatui presentation. This is not a general IDE/plugin/build program or an attempt to preserve Kibi's line-count limit.

## Architecture and related files

Add pure edit-buffer/operation logic and command/session adapters under `cells/tools/cellbox`; use Terminal SDK and Ratatui integration, not direct Unix terminal calls. Use admitted VFS scoped clients for open/save. Any replacement/flush/conflict primitive changes require P01's exact VFS owner/ABI approvals and cannot be invented as local guarantees.

## Implementation steps

1. Adapt suitable pinned Kibi ideas/code with attribution and split pure edit state from terminal/VFS. Implement bounded undo/redo with explicit limits, not unbounded whole-document copies per keypress.
2. Implement open/create/save/save-as, navigation, selection/copy/cut/paste, search, go-to-line, dirty-exit confirmation and visible shortcut help. Support UTF-8, width/continuation, tabs, existing line endings and final-newline preservation.
3. Add lightweight highlighting for only the P01-selected operational file types. Reject unsupported oversized/binary input honestly rather than corrupting it or claiming arbitrary-file support.
4. Implement safe save using scoped permissions and handle/version-bound conditional VFS operations. Save As to an existing destination needs explicit overwrite confirmation plus enforcing create-if-absent/replace-if-version semantics; creation/mutation after confirmation must be rejected at commit. Preserve original/destination bytes and dirty edit state on denied, short, interrupted, replacement or conflict failure. These are required behaviors, not invented opcode names or assumed atomic/flush guarantees.
5. Keep editor in foreground terminal-session mode with correct resize/focus and exit restoration. It consumes a file path or declared byte input, not an arbitrary object pipeline. Clipboard means the selected local edit/session operation unless a separately authorized system bridge exists.
6. Exercise real serial/GUI editing, resize, dirty exit, save-as and reopen. Inject original-file conflicts plus existing Save As destinations, destination creation/mutation after confirmation, short temporary writes and failed replacement. Observe stored destination/original bytes and retained dirty buffer on every refusal/failure, using disposable files only.

## Success criteria

- [x] All selected editing operations work interactively and retain correct buffer/dirty state. On target: typing, motion, multibyte Backspace, Enter/Tab, paste, find with wrap, go-to, selection with Ctrl-C copy and Ctrl-X cut, Ctrl-V, undo/redo, Save/Save As, and resize (terminal 459, GUI EDIT-0..5).
- [x] File bytes reopen as intended, including line endings and final newline; undo stays within its declared bound. Exact round trip on target; the undo bound is shown by host tests.
- [x] Original and Save As destination races/failures preserve stored bytes and dirty buffer; no stale overwrite confirmation bypasses conditional commit, and durability claims match evidence. Conflict and the overwrite race are shown on target. Staging, replacement and indeterminate faults are shown by the VFS self-test and host tests (S8-5 deviation 1). The claim is "synced, not crash-proof" (D78-2).
- [x] Terminal state restores on exit/cancel/app loss; no IDE, plugin or hidden command-execution scope is added. Exit and dirty-exit restore are shown on target; loss of the frontend (125) is shown by host tests only.

## Verification

Keep edit-operation/boundary/save-conflict regressions that catch plausible data-loss bugs; do not test copied UI strings. Real target save/reopen and actual screen/input proof are mandatory. Use private data/images and avoid destructive power-cut experiments on user volumes.

## Assumptions

VFS offers the approved replacement/conflict/flush behavior or exact prerequisite. Verify before claiming atomic or durable saves; if absent, finish its approved dependency rather than downgrade the required data-safety outcome.

## Security and risk assessment

File handles/cwd/paths obey caller scope; pasted text never becomes a shell command automatically. Preserve originals/backups for failure injection. Code/image rollback does not undo user saves, so recovery is explicit backup/forward repair, not a binary downgrade.

## Deviation log

2026-10-20 — **Completion/data-preservation claims withdrawn; incomplete/blocked.** `EditBuffer` source was added in `libs/text-engine::editor`; the historical 51-host-test snapshot is retained only as unit evidence. The former assertions of exact CRLF/trailing-newline preservation and dirty-buffer protection are not accepted: review identified editor data-corruption behavior, an editor no-op applet path and no admitted conditional save provider. No real terminal editing or VFS save/reopen/conflict/failure proof closes CBX-21/22. Design consent remains recorded; owner/allocation/provider admission is incomplete and CP2 pending/not eligible. All criteria remain unchecked and rollout is blocked under [reports/rollout-containment.md](reports/rollout-containment.md); no source fix or storage guarantee is asserted by this correction.

2026-10-24 — **CP1 proposal drafted** ([phase-08-s0-proposal.md](phase-08-s0-proposal.md)). User decisions before CP1:
- **D78-1:** D03 conditional save is CellosFS-only (`/srv`). FAT32 and bootfs files can be opened, but save to them is refused (a deviation from contract §3.1).
- **D78-2:** durability is reported as "synced, not crash-proof"; no power-cut experiments.
- **D78-3:** `EditBuffer` is rewritten.

2026-10-24 — **CP1 approved (user)** for D78-1..3, D03 §3.1 and slices S8-1..S8-5. The phase is now `in_progress`.

2026-10-25 — **S8-5 CP2 approved (user)**, including deviations 1–2. At the user's request, `edit-find-select-redo` was added before closing: find, wrap, go-to, Ctrl-Space selection, Ctrl-C copy, paste, Ctrl-X cut, undo, redo, then save. It checks exact bytes and passed on the first run (terminal 459/459, 174 markers; `reports/s8-5-evidence/find-select-redo/`). The phase is now `completed-private-evidence`. Rollout stays blocked; CBX-21/22 stay pending until Phase 10.

2026-10-25 — **Hash addendum: recorded 2026-10-25 at closure, post-approval state** (Phase 01 closure dossier gap C2-c). Doc-only. This addendum adds no scope or approval. The CP1 (D78-1..3, 2026-10-24) and the S8-1..S8-5 CP2s above remain the binding records. These SHA-256 values bind the current bytes. They are not the bytes on the approval dates: the shared `libs/api/src/services/ipc.rs` also carries the later P09 trust requests 40–42 and `SaveOutcome::CommittedDurable`.

| Approved proposal | SHA-256 |
|---|---|
| [phase-08-s0-proposal.md](phase-08-s0-proposal.md) | `1d7094ca30f8fdca53923f424bb0aef7ba27542602e804ce36296b58ed5b8a0f` |

| Key source file | Bytes | SHA-256 |
|---|---:|---|
| `cells/tools/cellbox/src/edit.rs` | 8990 | `96874b44adf96488961b238a52ec4f75e4ee1366805471eccfb6921e13867ce3` |
| `cells/tools/cellbox/src/edit/editor.rs` | 15068 | `921f002b455cdf78b209e2126af1ce499041f555c4a6dc87d4ad6e7b1ab987bc` |
| `cells/tools/cellbox/src/edit/render.rs` | 7485 | `fb93253773a0b0b0fa358601166a22e85f70ef04c536c53f6dce457381a5c6e4` |
| `cells/tools/cellbox/src/edit/store.rs` | 1794 | `4cb7f3b3d2c7757d160c256a594b56b8ea63e05a3039a5de61e1cb768b34585e` |
| `cells/tools/cellbox/src/edit/tests.rs` | 8802 | `925f9257921ba0e17ee402de616bd97796b8725a5e8a577a40f7d14bafc6a4ad` |
| `libs/text-engine/src/editor.rs` | 20771 | `9d40ab41f01717ec383648f9e3234fe8626e0f3cd9a244a37014c9a20f8fb6d3` |
| `libs/text-engine/src/editor/highlight.rs` | 6133 | `619b921ad6e3a6a16b59d828d8d3d3dc6e05fbeab021bb06224b2e0339f25c9f` |
| `libs/text-engine/src/editor/tests.rs` | 9429 | `0d54d7085f2f315de03c439b2412fe77d178bbfdd43912b3a697137412e09021` |
| `cells/services/vfs/src/save.rs` | 11969 | `bc9fc017b9ee42472e85cd48c1fec1d097e933a09ac1cae9e3240da513a74ef4` |
| `cells/services/vfs/src/save/selftest.rs` | 6869 | `9d6b3a6527d152c7fadc2e19cf224b0d4061989062fe2bc1ac77f1e63a8f5af0` |
| `cells/services/vfs/src/save/tests.rs` | 11067 | `599895704933968822ad3547768fc23aaaf30616a06764dcb1acb06e56045106` |
| `libs/api/src/services/ipc.rs` (shared: D03-save `Observe` 33 … `SaveAbort` 39) | 48085 | `ca8916570450f794c427d8519efb1e275b686a9e3c90d498a831fc5d936b0072` |
| `libs/cellos-fs/tests/conditional_replace.rs` | 5137 | `366fc3ec901a6f645225ce4c091ac55c930129877cb128e86e136b14a777e59b` |
| `cells/tests/phase04-terminal-driver/src/edit.rs` | 13884 | `649e1cd06b8718b344973fb877dbb0a6c242b366adcc81b3d26ad383c3576bee` |
