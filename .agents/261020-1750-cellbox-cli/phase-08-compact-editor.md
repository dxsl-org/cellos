---
phase: 8
title: "Compact terminal editor with truthful save behavior"
status: pending
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

- [ ] All selected editing operations work interactively and retain correct buffer/dirty state.
- [ ] File bytes reopen as intended, including line endings and final newline; undo stays within its declared bound.
- [ ] Original and Save As destination races/failures preserve stored bytes and dirty buffer; no stale overwrite confirmation bypasses conditional commit, and durability claims match evidence.
- [ ] Terminal state restores on exit/cancel/app loss; no IDE, plugin or hidden command-execution scope is added.

## Verification

Keep edit-operation/boundary/save-conflict regressions that catch plausible data-loss bugs; do not test copied UI strings. Real target save/reopen and actual screen/input proof are mandatory. Use private data/images and avoid destructive power-cut experiments on user volumes.

## Assumptions

VFS offers the approved replacement/conflict/flush behavior or exact prerequisite. Verify before claiming atomic or durable saves; if absent, finish its approved dependency rather than downgrade the required data-safety outcome.

## Security and risk assessment

File handles/cwd/paths obey caller scope; pasted text never becomes a shell command automatically. Preserve originals/backups for failure injection. Code/image rollback does not undo user saves, so recovery is explicit backup/forward repair, not a binary downgrade.

## Deviation log

None; implementation pending. Record decisions/deviations/surprises, especially storage guarantees, with reason/impact/revert or escalation.
