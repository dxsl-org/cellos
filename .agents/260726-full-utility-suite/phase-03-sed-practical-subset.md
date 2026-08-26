---
phase: 3
title: "Sed practical subset"
status: complete
effort: "2-3 days"
---

# Phase 3 — Sed Practical Subset

## Context Links

- Plan: [plan.md](plan.md) · Depends on phase 1.
- Existing implementation: `cells/tools/shell/src/cmd_fs.rs`

## Overview

- **Priority:** P1
- Deliver a bounded single-command stream editor with honest ERE-lite semantics.

## Key Insights

- Replacement `&` needs matched-range information; capture backreferences remain out of scope.
- Alternate delimiters are necessary for paths and quoted replacements.

## Requirements

- `-n`; commands `s<d>pat<d>replacement<d>[gp]`, `/pat/p`, `/pat/d`, and `Np`.
- ERE-lite address/substitution patterns; fixed-string compatibility for existing scripts.
- Replacement supports `&` and escaped delimiter/backslash.
- Pipeline/file input, status 0 on success and 2 on parse/I/O/limit errors.
- Reject multi-command, hold-space, labels, branching, transliteration, and file-write commands.

## Architecture

`sed::parse_script -> Command -> per-record address match -> edit/print policy -> OutputSink`.
Parser and substitution are pure modules.

## Related Code Files

- **Create:** `cells/tools/shell/src/text_tools/sed.rs`
- **Modify:** `cmd_fs.rs`, help text
- **Tests:** parser/substitution unit tests and guest shell scenarios

## Implementation Steps

1. Parse delimiters, escapes, addresses, flags, and strict trailing input.
2. Execute substitutions with first/global and `&` replacement.
3. Implement regex print/delete and numeric print with default-print rules.
4. Integrate file/pipeline adapters and status diagnostics.
5. Cover malformed scripts, limits, duplicates, and no-match behavior.

## Todo List

- [ ] script parser
- [ ] substitution engine
- [ ] address/default-print execution
- [ ] tests green

## Success Criteria

- [ ] `sed "s|/old path|/new path|g"` preserves quoted spaces.
- [ ] `sed -n "/^ERR/p"` and `/pattern/d` match expected lines.
- [ ] Unsupported or malformed scripts return status 2 without partial mutation.

## Risk Assessment

- Regex replacement can expand output; cap produced record size and fail before allocation growth.

## Security Considerations

- No in-place file editing; source data stays unchanged on parser/runtime error.

## Next Steps

Mini-AWK reuses the same pattern and record contracts.
