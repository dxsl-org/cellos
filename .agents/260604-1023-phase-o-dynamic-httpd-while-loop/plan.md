---
title: "Phase O — Dynamic httpd + Shell while-loop"
description: "Per-request VFS reads in httpd (live content) and a while/do/done shell loop."
status: pending
priority: P2
effort: 3h
branch: main
tags: [shell, httpd, networking, parser, executor]
created: 2026-06-04
---

# Phase O — Dynamic httpd + Shell `while` Loop

Two independent, low-risk features. No `libs/api` / `libs/types` changes (Law 1 safe).

## Sub-tasks

| # | Task | Files | Build steps | Test |
|---|------|-------|-------------|------|
| O-1 | Dynamic httpd: move `vfs_read` inside accept loop; 404 on empty read | `cells/apps/net-tools/src/bin/httpd.rs` | httpd cell → disk cell table (step 5 only) | `network_httpd_dynamic_content` |
| O-2 | Shell `while/do/done` loop | `parser.rs`, `executor.rs`, `boot.rs` | full shell pipeline (steps 1–5) | `shell_while_loop` |

**Parallel-safe**: O-1 and O-2 touch disjoint files. Only collision is the disk cell-table rebuild (step 5) — run once after both land.

## Dependency graph

```
O-1 (httpd.rs) ─┐
                ├─→ step 5 (write-cell-table) ─→ integration tests
O-2 (shell) ────┘   (O-2 also needs steps 1–4)
```

## Key constraints (verified against code)

- **No new `Tok` variants** for `while/do/done` — keep as `Word(...)`, match by string. Same rule that fixed `lua -e "if x then ... end"` in Phase N (parser.rs:84-93, 154-157).
- Executor exit-code convention: `Ok(())`→0, `Err(_)`→1 (executor.rs:268). `vcat` on missing file returns `Err(ViError::NotFound)` (cmd_fs.rs:381) → drives loop exit.
- `rm` is a built-in dispatched at executor.rs:242 → unlinks VFS file. Safe to use inside the while body.
- httpd response header is rebuilt per request; `file_len` must be recomputed per iteration (currently captured once at httpd.rs:188).

## Success criteria

- [ ] `network_httpd_dynamic_content` passes: GET reflects `vwrite` overwrite without restarting httpd.
- [ ] httpd returns HTTP 404 (not crash/hang) when the served file is missing on a request.
- [ ] `shell_while_loop` passes: body runs once for an existing flag file, loop exits after `rm`.
- [ ] False-condition while runs body 0 times (no `SHOULD_NOT_APPEAR`).
- [ ] Existing 41 integration tests still pass (regression guard for `if/then/fi` + httpd).
- [ ] `cargo clippy -- -D warnings` clean on shell + net-tools.

## Phase file

- [phase-01-implementation.md](./phase-01-implementation.md)

## Unresolved questions

- None blocking. `file_buf` size stays 4096 (matches VFS OP_READ reply cap); 404 body is fixed-string, no length math needed.
