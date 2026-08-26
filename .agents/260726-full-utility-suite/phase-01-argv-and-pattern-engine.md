---
phase: 1
title: "Argv and pattern engine"
status: complete
effort: "3-5 days"
---

# Phase 1 — Argv and Pattern Engine

## Context Links

- Plan: [plan.md](plan.md)
- Current parser/argv loss: `cells/tools/shell/src/parser.rs`, `executor.rs`
- Existing text tools: `cells/tools/shell/src/cmd_fs.rs`

## Overview

- **Priority:** P1
- Preserve parsed argument boundaries through built-in dispatch and introduce reusable bounded
  fixed-string/ERE-lite and record-reading modules.

## Key Insights

- Rejoining argv and calling `split_whitespace()` breaks quoted patterns and replacements.
- POSIX BRE requires backreferences; the delivered dialect must be named ERE-lite.
- Matching must be linear-time, `no_std + alloc`, and reusable by grep/sed/mini-AWK.

## Requirements

- Parsed words retain quote metadata. Single quotes are literal/no-expansion, double quotes retain
  current variable expansion, and unquoted words keep existing behavior.
- Built-ins receive argument slices without leaking joined strings.
- Existing simple unquoted commands remain behavior-compatible.
- Pattern text is at most 256 bytes; record/input/file counts have explicit limits.
- Matcher caps AST depth at 32, compiled states at 4,096, repetition bounds at 256, and rejects
  nested quantified alternation that would exceed those limits.
- Matcher uses `regex-automata` (Thompson-NFA/DFA-equivalent linear engine) for ERE-lite anchors,
  classes, groups, alternation, and bounded quantifiers; rejects backreferences, look-around,
  invalid patterns, and compiled-size overflow loudly.
- No unsafe code and no whole-file-only requirement for file operands.

## Architecture

`parser Word{text,quote} -> quote-aware expansion -> dispatch_builtin(&[String]) -> utility option
parser -> Pattern -> RecordReader -> OutputSink`. Keep parsing/matching pure and host-testable;
shell/VFS adapters stay thin.

## Related Code Files

- **Modify:** `cells/tools/shell/src/parser.rs`, `executor.rs`, `main.rs`, `Cargo.toml`
- **Create:** `cells/tools/shell/src/text_engine.rs`,
  `cells/tools/shell/src/text_engine/{args,matcher,records}.rs`
- **Tests:** host-testable engine/parser modules and `shell_test.rs`
- Legacy `executor.rs`/`cmd_fs.rs` changes are thin adapters only; they must not grow materially.

## Implementation Steps

1. Preserve quoted argv boundaries from AST through built-in dispatch.
2. Extract a bounded record reader from current 512-byte file loops.
3. Add fixed and ERE-lite pattern compilation/matching with explicit limits.
4. Introduce an explicit `UtilityStatus::{Selected, NotSelected, Error}` return contract mapped to
   `0/1/2`, without changing unrelated built-ins.
5. Preserve command status through stdout/stderr redirection; VFS write failure overrides success.
6. Add pure parser/matcher/status regression tests, including redirected failures and adversarial
   nested-alternation, repetition-expansion, state-limit and long-no-match patterns.

## Todo List

- [ ] argv boundaries preserved
- [ ] bounded pattern/record APIs added
- [ ] exit-status plumbing added
- [ ] redirected status and write errors preserved
- [ ] host tests green

## Success Criteria

- [ ] `grep -e "a b"`, `sed "s|a b|c|"`, and quoted mini-AWK programs reach handlers intact.
- [ ] Invalid/oversized patterns return status 2 and a concise diagnostic.
- [ ] Existing shell parser/pipeline/redirection tests remain green.
- [ ] Single-quoted `$0..$9` reaches mini-AWK literally; redirected utility errors stay nonzero.

## Risk Assessment

- Parser changes affect every built-in; isolate compatibility tests before migration.
- New dependency size may inflate shell; compile with no-default-features and record binary delta.

## Security Considerations

- Hard limits prevent pattern/record/file-count memory exhaustion.
- Only linear-time matching; no recursive/backtracking regex execution.

## Next Steps

Phases 2–4 consume the stable engine API; phase 5 consumes status plumbing for batch `top`.
