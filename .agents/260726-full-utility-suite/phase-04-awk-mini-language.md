---
phase: 4
title: "Mini-AWK language"
status: complete
effort: "3-5 days"
---

# Phase 4 — Mini-AWK Language

## Context Links

- Plan: [plan.md](plan.md) · Depends on phase 1.
- Existing field extractor: `cells/tools/shell/src/cmd_fs.rs`

## Overview

- **Priority:** P2
- Expand the current field extractor into a documented shell-friendly mini-language without
  claiming POSIX AWK compatibility.

## Key Insights

- Full AWK is a programming language and outside this round.
- The shell must preserve quote metadata: single-quoted programs bypass `$` expansion.

## Requirements

- `-F SEP`, regex pattern filter, `NR`, `NF`, `$0..$9`.
- `print` expressions with comma-separated fields/literals.
- Numeric/string `== != < <= > >=`, boolean `&& || !`, and simple `+ - * / %`.
- Optional `BEGIN`/`END` print-only actions if they fit the same bounded grammar.
- Pipeline/file input; deterministic syntax/runtime error status.
- Reject arrays, functions, loops, assignment, user variables, system calls, and unbounded actions.

## Architecture

Bounded lexer -> Pratt expression parser -> compact AST -> per-record environment -> OutputSink.
Use explicit node/depth/string limits and no recursion proportional to input size.

## Related Code Files

- **Create:** `cells/tools/shell/src/text_tools/awk.rs`,
  `text_tools/awk/{lexer,parser,runtime}.rs`
- **Modify:** `cmd_fs.rs`, help text
- **Tests:** lexer/parser/runtime unit tests and shell pipeline scenarios

## Implementation Steps

1. Freeze and document grammar plus compatibility syntax.
2. Implement bounded lexer/expression parser and diagnostics.
3. Evaluate fields/built-ins/comparisons/arithmetic/print per record.
4. Preserve the old `awk [cols] [file]` form through a compatibility parser.
5. Add positive, missing-field, divide-by-zero, malformed and limit tests, plus shell regression
   cases proving single-quoted `$0/$1/$2/$?` survive while function positional expansion and
   double-quoted shell expansion remain unchanged.

## Todo List

- [ ] grammar frozen
- [ ] lexer/parser
- [ ] runtime and compatibility mode
- [ ] tests green

## Success Criteria

- [ ] `awk -F, '$2 >= 10 { print NR, $1 }'` works from file and pipeline.
- [ ] `NF`, missing fields, numeric/string comparisons and simple arithmetic are deterministic.
- [ ] Unsupported full-AWK features fail loudly with a mini-AWK diagnostic.

## Risk Assessment

- Quote-aware words are a cross-shell change; single quotes are literal while double quotes retain
  variable expansion, and regression tests pin both contracts.

## Security Considerations

- AST nodes, nesting, fields and output per record are bounded; division by zero is an error.

## Next Steps

Phase 5 completes system observability and the full validation matrix.
