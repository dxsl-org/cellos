---
phase: 2
title: "Grep practical subset"
status: complete
effort: "2-3 days"
---

# Phase 2 — Grep Practical Subset

## Context Links

- Plan: [plan.md](plan.md) · Depends on phase 1.
- Existing implementation: `cells/tools/shell/src/cmd_fs.rs`

## Overview

- **Priority:** P1
- Replace the literal-only monolith with a focused grep module using the shared engine.

## Key Insights

- `grep` requires status 0 for selected lines, 1 for no selection, and 2 for error.
- `-F` and `-E` are explicit; default remains fixed-string for backward compatibility.

## Requirements

- Options: `-F -E -e PAT -f FILE -i -v -n -c -q -x`; composable short flags.
- Pipeline stdin plus one or more file operands; filename prefix only for multiple files.
- Preserve existing recursive `-r` behavior as compatibility mode with depth 16.
- Empty patterns, unreadable files, unknown flags, and incompatible options are deterministic.
- No locale-sensitive classes or BRE/backreference claims.

## Architecture

`grep::parse -> Vec<Pattern> -> stream records -> selection policy -> OutputSink/status`.
Fixed and ERE-lite paths share output/count/quiet logic.

## Related Code Files

- **Create:** `cells/tools/shell/src/text_tools.rs`, `text_tools/grep.rs`
- **Modify:** `cmd_fs.rs`, `main.rs`, `executor.rs`, help/completion text
- **Tests:** pure grep tests and guest `shell_test.rs`

## Implementation Steps

1. Move grep parsing/execution into a focused module.
2. Add multiple patterns, pattern files, exact-line and quiet modes.
3. Add multi-file prefixing and exact 0/1/2 status semantics.
4. Retain and bound recursive traversal.
5. Add pipeline, file, combined-flag, regex and failure tests.

## Todo List

- [ ] options and operands implemented
- [ ] status semantics implemented
- [ ] recursive compatibility retained
- [ ] tests green

## Success Criteria

- [ ] `cat file | grep -E -n "^[A-Z]+"` selects expected records.
- [ ] `grep -q` emits nothing; `$?` distinguishes match/no-match/error.
- [ ] Multiple `-e` and `-f` patterns work within documented limits.

## Risk Assessment

- Recursive VFS replies can truncate; detect malformed/truncated listings and fail loud.
- Case folding is ASCII-only and documented.

## Security Considerations

- Cap patterns/files/recursion/record length; never silently drop excess input.

## Next Steps

Reuse matcher behavior in sed and mini-AWK.
